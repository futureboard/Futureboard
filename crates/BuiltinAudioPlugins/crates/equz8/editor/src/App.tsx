import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  type WheelEvent,
} from 'react'
import {
  connectBridge,
  postParam,
  type Band,
  type EqParams,
  type FilterType,
} from './bridge'
import './Editor.scss'

const BAND_COLORS = [
  '#58b9df',
  '#64cdb5',
  '#9acb72',
  '#d4c060',
  '#de9e5b',
  '#d8797d',
  '#ae7fca',
  '#7b91d4',
] as const

const FILTER_TYPES: { type: FilterType; label: string; wire: number }[] = [
  { type: 'highpass', label: 'HP', wire: 0 },
  { type: 'lowshelf', label: 'Low shelf', wire: 1 },
  { type: 'bell', label: 'Bell', wire: 2 },
  { type: 'notch', label: 'Notch', wire: 3 },
  { type: 'highshelf', label: 'High shelf', wire: 4 },
  { type: 'lowpass', label: 'LP', wire: 5 },
]

const DEFAULT_PARAMS: EqParams = {
  power: true,
  outputDb: 0,
  mix: 100,
  bands: [
    { active: true, bandType: 'highpass', freq: 50, gainDb: 0, q: 0.7 },
    { active: true, bandType: 'lowshelf', freq: 120, gainDb: 0, q: 0.8 },
    { active: true, bandType: 'bell', freq: 250, gainDb: 2.5, q: 1.2 },
    { active: true, bandType: 'bell', freq: 750, gainDb: -1.5, q: 1.4 },
    { active: true, bandType: 'bell', freq: 1500, gainDb: 1, q: 1 },
    { active: true, bandType: 'bell', freq: 3500, gainDb: 0, q: 1.1 },
    { active: true, bandType: 'highshelf', freq: 8000, gainDb: 1.5, q: 0.8 },
    { active: true, bandType: 'lowpass', freq: 16000, gainDb: 0, q: 0.7 },
  ],
}

const GRAPH_WIDTH = 1040
const GRAPH_HEIGHT = 352
const GAIN_RANGE = 18
const FREQUENCIES = [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000]
const GAINS = [18, 12, 6, 0, -6, -12, -18]

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value))
}

function frequencyToX(frequency: number, width = GRAPH_WIDTH) {
  return (Math.log10(frequency / 20) / Math.log10(20000 / 20)) * width
}

function xToFrequency(x: number, width = GRAPH_WIDTH) {
  return 20 * Math.pow(20000 / 20, clamp(x / width, 0, 1))
}

function gainToY(gain: number, height = GRAPH_HEIGHT) {
  return ((GAIN_RANGE - gain) / (GAIN_RANGE * 2)) * height
}

function yToGain(y: number, height = GRAPH_HEIGHT) {
  return GAIN_RANGE - clamp(y / height, 0, 1) * GAIN_RANGE * 2
}

function formatFrequency(value: number) {
  if (value >= 1000) {
    const precision = value < 10000 ? 2 : 1
    return `${(value / 1000).toFixed(precision).replace(/\.?0+$/, '')}k`
  }
  return `${Math.round(value)}`
}

function formatGain(value: number) {
  const normalized = Math.abs(value) < 0.05 ? 0 : value
  return `${normalized > 0 ? '+' : ''}${normalized.toFixed(1)}`
}

type Coefficients = {
  b0: number
  b1: number
  b2: number
  a1: number
  a2: number
}

function coefficients(band: Band, sampleRate = 48000): Coefficients {
  const w0 = (2 * Math.PI * clamp(band.freq, 20, 20000)) / sampleRate
  const cos = Math.cos(w0)
  const sin = Math.sin(w0)
  const alpha = sin / (2 * clamp(band.q, 0.1, 12))
  const A = Math.pow(10, band.gainDb / 40)
  let b0: number
  let b1: number
  let b2: number
  let a0: number
  let a1: number
  let a2: number

  switch (band.bandType) {
    case 'highpass':
      b0 = (1 + cos) / 2
      b1 = -(1 + cos)
      b2 = (1 + cos) / 2
      a0 = 1 + alpha
      a1 = -2 * cos
      a2 = 1 - alpha
      break
    case 'lowpass':
      b0 = (1 - cos) / 2
      b1 = 1 - cos
      b2 = (1 - cos) / 2
      a0 = 1 + alpha
      a1 = -2 * cos
      a2 = 1 - alpha
      break
    case 'notch':
      b0 = 1
      b1 = -2 * cos
      b2 = 1
      a0 = 1 + alpha
      a1 = -2 * cos
      a2 = 1 - alpha
      break
    case 'lowshelf': {
      const root = 2 * Math.sqrt(A) * alpha
      b0 = A * ((A + 1) - (A - 1) * cos + root)
      b1 = 2 * A * ((A - 1) - (A + 1) * cos)
      b2 = A * ((A + 1) - (A - 1) * cos - root)
      a0 = (A + 1) + (A - 1) * cos + root
      a1 = -2 * ((A - 1) + (A + 1) * cos)
      a2 = (A + 1) + (A - 1) * cos - root
      break
    }
    case 'highshelf': {
      const root = 2 * Math.sqrt(A) * alpha
      b0 = A * ((A + 1) + (A - 1) * cos + root)
      b1 = -2 * A * ((A - 1) + (A + 1) * cos)
      b2 = A * ((A + 1) + (A - 1) * cos - root)
      a0 = (A + 1) - (A - 1) * cos + root
      a1 = 2 * ((A - 1) - (A + 1) * cos)
      a2 = (A + 1) - (A - 1) * cos - root
      break
    }
    default:
      b0 = 1 + alpha * A
      b1 = -2 * cos
      b2 = 1 - alpha * A
      a0 = 1 + alpha / A
      a1 = -2 * cos
      a2 = 1 - alpha / A
  }

  return {
    b0: b0 / a0,
    b1: b1 / a0,
    b2: b2 / a0,
    a1: a1 / a0,
    a2: a2 / a0,
  }
}

function magnitudeDb(coeff: Coefficients, frequency: number, sampleRate = 48000) {
  const w = (2 * Math.PI * frequency) / sampleRate
  const c1 = Math.cos(w)
  const s1 = Math.sin(w)
  const c2 = Math.cos(2 * w)
  const s2 = Math.sin(2 * w)
  const nr = coeff.b0 + coeff.b1 * c1 + coeff.b2 * c2
  const ni = -coeff.b1 * s1 - coeff.b2 * s2
  const dr = 1 + coeff.a1 * c1 + coeff.a2 * c2
  const di = -coeff.a1 * s1 - coeff.a2 * s2
  const magnitude = Math.sqrt((nr * nr + ni * ni) / (dr * dr + di * di))
  return 20 * Math.log10(Math.max(magnitude, 0.000001))
}

function responsePath(
  bands: Band[],
  width: number,
  height: number,
  onlyBand?: number,
) {
  const active = bands
    .map((band, index) => ({ band, index }))
    .filter(
      ({ band, index }) =>
        band.active && (onlyBand === undefined || onlyBand === index),
    )
    .map(({ band }) => coefficients(band))
  const points: string[] = []
  for (let index = 0; index <= 240; index += 1) {
    const x = (index / 240) * width
    const frequency = xToFrequency(x, width)
    const db = active.reduce(
      (sum, coeff) => sum + magnitudeDb(coeff, frequency),
      0,
    )
    points.push(
      `${index === 0 ? 'M' : 'L'}${x.toFixed(1)},${gainToY(clamp(db, -24, 24), height).toFixed(1)}`,
    )
  }
  return points.join(' ')
}

function Icon({ children, size = 16 }: { children: ReactNode; size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} aria-hidden="true">
      {children}
    </svg>
  )
}

const KNOB_ANGLE_START = 135
const KNOB_ANGLE_SWEEP = 270
const KNOB_DRAG_SPAN = 190

function polar(cx: number, cy: number, radius: number, degrees: number) {
  const radians = (degrees * Math.PI) / 180
  return [
    cx + radius * Math.cos(radians),
    cy + radius * Math.sin(radians),
  ] as const
}

function knobArc(progress: number, radius: number) {
  const from = KNOB_ANGLE_START
  const to = from + KNOB_ANGLE_SWEEP * clamp(progress, 0, 1)
  const [x1, y1] = polar(50, 50, radius, from)
  const [x2, y2] = polar(50, 50, radius, to)
  const large = to - from > 180 ? 1 : 0
  return `M ${x1} ${y1} A ${radius} ${radius} 0 ${large} 1 ${x2} ${y2}`
}

function ValueControl({
  label,
  value,
  min,
  max,
  step,
  unit,
  format,
  defaultValue,
  toProgress,
  fromProgress,
  disabled,
  onChange,
}: {
  label: string
  value: number
  min: number
  max: number
  step: number
  unit: string
  format: (value: number) => string
  defaultValue: number
  toProgress?: (value: number) => number
  fromProgress?: (progress: number) => number
  disabled?: boolean
  onChange: (value: number) => void
}) {
  const [active, setActive] = useState(false)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')
  const dialRef = useRef<HTMLDivElement>(null)
  const start = useRef<{ y: number; progress: number } | null>(null)
  const progress = clamp(
    toProgress ? toProgress(value) : (value - min) / (max - min),
    0,
    1,
  )
  const defaultProgress = clamp(
    toProgress
      ? toProgress(defaultValue)
      : (defaultValue - min) / (max - min),
    0,
    1,
  )
  const valueAt = useCallback(
    (nextProgress: number) =>
      fromProgress
        ? fromProgress(nextProgress)
        : min + nextProgress * (max - min),
    [fromProgress, max, min],
  )
  const angle = KNOB_ANGLE_START + KNOB_ANGLE_SWEEP * progress
  const [pointerX1, pointerY1] = polar(50, 50, 14, angle)
  const [pointerX2, pointerY2] = polar(50, 50, 33, angle)
  const formattedValue = `${format(value)}${unit ? ` ${unit}` : ''}`

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (disabled || editing) return
    event.preventDefault()
    start.current = { y: event.clientY, progress }
    event.currentTarget.setPointerCapture(event.pointerId)
    setActive(true)
  }
  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!start.current || disabled) return
    const delta = (start.current.y - event.clientY) / KNOB_DRAG_SPAN
    const nextProgress = clamp(
      start.current.progress + (event.shiftKey ? delta * 0.2 : delta),
      0,
      1,
    )
    const raw = valueAt(nextProgress)
    onChange(clamp(Math.round(raw / step) * step, min, max))
  }

  useEffect(() => {
    const dial = dialRef.current
    if (!dial || disabled) return
    const onWheel = (event: globalThis.WheelEvent) => {
      event.preventDefault()
      const amount = event.shiftKey ? 0.01 : 0.03
      const next = clamp(
        progress + (event.deltaY < 0 ? amount : -amount),
        0,
        1,
      )
      onChange(valueAt(next))
    }
    dial.addEventListener('wheel', onWheel, { passive: false })
    return () => dial.removeEventListener('wheel', onWheel)
  }, [disabled, onChange, progress, valueAt])

  const commitEdit = () => {
    const parsed = Number(draft.replace(/[^\d.+-]/g, ''))
    if (Number.isFinite(parsed)) onChange(clamp(parsed, min, max))
    setEditing(false)
  }

  return (
    <div
      className={`value-control knob ${active ? 'active' : ''} ${disabled ? 'disabled' : ''}`}
    >
      <span className="knob-label">{label}</span>
      <div
        ref={dialRef}
        className="knob-dial"
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={value}
        aria-valuetext={formattedValue}
        title={`${label} — drag up/down (Shift = fine), double-click to reset`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={(event) => {
          start.current = null
          setActive(false)
          if (event.currentTarget.hasPointerCapture(event.pointerId)) {
            event.currentTarget.releasePointerCapture(event.pointerId)
          }
        }}
        onPointerCancel={() => {
          start.current = null
          setActive(false)
        }}
        onDoubleClick={() => onChange(defaultValue)}
        onContextMenu={(event) => {
          event.preventDefault()
          onChange(defaultValue)
        }}
        onKeyDown={(event) => {
          if (disabled) return
          const amount = event.shiftKey ? 0.01 : 0.05
          if (event.key === 'ArrowUp' || event.key === 'ArrowRight') {
            event.preventDefault()
            onChange(valueAt(clamp(progress + amount, 0, 1)))
          }
          if (event.key === 'ArrowDown' || event.key === 'ArrowLeft') {
            event.preventDefault()
            onChange(valueAt(clamp(progress - amount, 0, 1)))
          }
          if (event.key === 'Home') {
            event.preventDefault()
            onChange(min)
          }
          if (event.key === 'End') {
            event.preventDefault()
            onChange(max)
          }
        }}
      >
        <svg viewBox="0 0 100 100" aria-hidden="true">
          <path className="knob-arc-bg" d={knobArc(1, 43)} />
          <path className="knob-arc" d={knobArc(progress, 43)} />
          <line
            className="knob-tick"
            x1={polar(
              50,
              50,
              46,
              KNOB_ANGLE_START + KNOB_ANGLE_SWEEP * defaultProgress,
            )[0]}
            y1={polar(
              50,
              50,
              46,
              KNOB_ANGLE_START + KNOB_ANGLE_SWEEP * defaultProgress,
            )[1]}
            x2={polar(
              50,
              50,
              50,
              KNOB_ANGLE_START + KNOB_ANGLE_SWEEP * defaultProgress,
            )[0]}
            y2={polar(
              50,
              50,
              50,
              KNOB_ANGLE_START + KNOB_ANGLE_SWEEP * defaultProgress,
            )[1]}
          />
          <circle className="knob-body" cx="50" cy="50" r="33" />
          <line
            className="knob-pointer"
            x1={pointerX1}
            y1={pointerY1}
            x2={pointerX2}
            y2={pointerY2}
          />
        </svg>
      </div>

      {editing ? (
        <input
          className="knob-input"
          autoFocus
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commitEdit}
          onKeyDown={(event) => {
            if (event.key === 'Enter') commitEdit()
            if (event.key === 'Escape') setEditing(false)
          }}
        />
      ) : (
        <button
          type="button"
          className="knob-value"
          disabled={disabled}
          title="Click to type a value"
          onClick={() => {
            setDraft(String(Number(value.toFixed(2))))
            setEditing(true)
          }}
        >
          {formattedValue}
        </button>
      )}
    </div>
  )
}

function App() {
  const [params, setParams] = useState<EqParams>(DEFAULT_PARAMS)
  const [selected, setSelected] = useState(2)
  const [connected, setConnected] = useState(false)
  const [analyzer, setAnalyzer] = useState(true)
  const [showIndividual, setShowIndividual] = useState(false)
  const graphRef = useRef<SVGSVGElement>(null)
  const dragging = useRef<number | null>(null)
  const [graphSize, setGraphSize] = useState({
    width: GRAPH_WIDTH,
    height: GRAPH_HEIGHT,
  })

  useEffect(
    () =>
      connectBridge(
        (nativeParams) => setParams(nativeParams),
        (next) => setConnected(next),
      ),
    [],
  )

  useEffect(() => {
    const graph = graphRef.current
    if (!graph) return
    const updateSize = () => {
      const rect = graph.getBoundingClientRect()
      if (rect.width <= 0 || rect.height <= 0) return
      setGraphSize((current) => {
        if (
          Math.abs(current.width - rect.width) < 0.5 &&
          Math.abs(current.height - rect.height) < 0.5
        ) {
          return current
        }
        return { width: rect.width, height: rect.height }
      })
    }
    updateSize()
    const observer = new ResizeObserver(updateSize)
    observer.observe(graph)
    return () => observer.disconnect()
  }, [])

  const updateBand = useCallback(
    (index: number, patch: Partial<Band>, send = true) => {
      setParams((current) => ({
        ...current,
        bands: current.bands.map((band, bandIndex) =>
          bandIndex === index ? { ...band, ...patch } : band,
        ),
      }))
      if (!send) return
      const prefix = `band${index + 1}_`
      if (patch.active !== undefined) {
        postParam(`${prefix}enabled`, patch.active ? 1 : 0)
      }
      if (patch.bandType !== undefined) {
        postParam(
          `${prefix}type`,
          FILTER_TYPES.find((item) => item.type === patch.bandType)?.wire ?? 2,
        )
      }
      if (patch.freq !== undefined) postParam(`${prefix}freq`, patch.freq)
      if (patch.gainDb !== undefined) {
        postParam(`${prefix}gainDb`, patch.gainDb)
      }
      if (patch.q !== undefined) postParam(`${prefix}q`, patch.q)
    },
    [],
  )

  const response = useMemo(
    () => responsePath(params.bands, graphSize.width, graphSize.height),
    [graphSize.height, graphSize.width, params.bands],
  )
  const selectedResponse = useMemo(
    () =>
      responsePath(
        params.bands,
        graphSize.width,
        graphSize.height,
        selected,
      ),
    [graphSize.height, graphSize.width, params.bands, selected],
  )
  const selectedBand = params.bands[selected]

  const moveBand = (
    event: ReactPointerEvent<SVGSVGElement>,
    index: number,
  ) => {
    const rect = graphRef.current?.getBoundingClientRect()
    const band = params.bands[index]
    if (!rect || !band) return
    const x = ((event.clientX - rect.left) / rect.width) * graphSize.width
    const y = ((event.clientY - rect.top) / rect.height) * graphSize.height
    const canGain =
      band.bandType === 'bell' ||
      band.bandType === 'lowshelf' ||
      band.bandType === 'highshelf'
    updateBand(index, {
      freq: clamp(xToFrequency(x, graphSize.width), 20, 20000),
      ...(canGain
        ? { gainDb: clamp(yToGain(y, graphSize.height), -18, 18) }
        : {}),
    })
  }

  const onNodeWheel = (event: WheelEvent<SVGGElement>, index: number) => {
    event.preventDefault()
    const band = params.bands[index]
    if (!band) return
    const scale = event.shiftKey ? 0.02 : 0.12
    updateBand(index, {
      q: clamp(band.q - Math.sign(event.deltaY) * scale, 0.1, 12),
    })
  }

  if (!selectedBand) return null
  const canGain = ['bell', 'lowshelf', 'highshelf'].includes(
    selectedBand.bandType,
  )

  return (
    <main className={`plugin ${params.power ? '' : 'is-bypassed'}`}>
      <header className="topbar">
        <div className="brand">
          <div className="brand-mark">
            <svg viewBox="0 0 32 24" aria-hidden="true">
              <path d="M2 16c4 0 4-8 8-8s4 10 8 10 4-13 8-13 3 6 4 6" />
            </svg>
          </div>
          <div>
            <strong>EQ-Z8</strong>
            <span>PARAMETRIC EQUALIZER</span>
          </div>
        </div>

        <div className="preset">
          <button aria-label="Previous preset">‹</button>
          <div>
            <span>Preset</span>
            <strong>Clean Start</strong>
          </div>
          <button aria-label="Next preset">›</button>
        </div>

        <div className="header-tools">
          <div className="connection" data-connected={connected}>
            <i /> {connected ? 'DSP' : 'PREVIEW'}
          </div>
          <button className="icon-button" aria-label="Undo" disabled>
            <Icon>
              <path d="M9 7 4 12l5 5M5 12h8a6 6 0 0 1 6 6" />
            </Icon>
          </button>
          <button className="icon-button" aria-label="Redo" disabled>
            <Icon>
              <path d="m15 7 5 5-5 5m4-5h-8a6 6 0 0 0-6 6" />
            </Icon>
          </button>
          <div className="ab-switch">
            <button className="active">A</button>
            <button>B</button>
          </div>
          <button
            className={`power ${params.power ? 'on' : ''}`}
            aria-label="Bypass equalizer"
            aria-pressed={params.power}
            onClick={() => {
              const power = !params.power
              setParams((current) => ({ ...current, power }))
              postParam('power', power ? 1 : 0)
            }}
          >
            <Icon size={17}>
              <path d="M12 3v9m5.7-5.7a8 8 0 1 1-11.4 0" />
            </Icon>
          </button>
        </div>
      </header>

      <section className="graph-shell">
        <div className="graph-toolbar">
          <div className="graph-mode">
            <button
              className={analyzer ? 'active' : ''}
              onClick={() => setAnalyzer((value) => !value)}
            >
              <Icon size={14}>
                <path d="M3 18V9m4 9V5m4 13v-7m4 7V3m4 15v-9" />
              </Icon>
              Analyzer
            </button>
            <span>
              {connected ? 'SPECTRUM IPC PENDING' : 'AWAITING SIGNAL'}
            </span>
          </div>
          <div className="graph-actions">
            <button
              className={showIndividual ? 'active' : ''}
              onClick={() => setShowIndividual((value) => !value)}
            >
              Band solo
            </button>
            <span className="scale">±18 dB</span>
          </div>
        </div>

        <svg
          ref={graphRef}
          className="response-graph"
          viewBox={`0 0 ${graphSize.width} ${graphSize.height}`}
          preserveAspectRatio="xMinYMin meet"
          onPointerMove={(event) => {
            if (dragging.current !== null) {
              moveBand(event, dragging.current)
            }
          }}
          onPointerUp={(event) => {
            dragging.current = null
            if (event.currentTarget.hasPointerCapture(event.pointerId)) {
              event.currentTarget.releasePointerCapture(event.pointerId)
            }
          }}
          onPointerCancel={() => (dragging.current = null)}
        >
          <defs>
            <linearGradient id="response-fill" x1="0" x2="0" y1="0" y2="1">
              <stop offset="0" stopColor="#66d2dc" stopOpacity=".18" />
              <stop offset=".5" stopColor="#66d2dc" stopOpacity=".035" />
              <stop offset="1" stopColor="#66d2dc" stopOpacity=".12" />
            </linearGradient>
          </defs>

          <rect
            width={graphSize.width}
            height={graphSize.height}
            className="graph-bg"
          />
          {FREQUENCIES.map((frequency) => (
            <g key={frequency}>
              <line
                x1={frequencyToX(frequency, graphSize.width)}
                x2={frequencyToX(frequency, graphSize.width)}
                y1={0}
                y2={graphSize.height}
                className={frequency === 1000 ? 'grid major' : 'grid'}
              />
              <text
                x={frequencyToX(frequency, graphSize.width)}
                y={graphSize.height - 10}
                className="axis-label frequency"
              >
                {formatFrequency(frequency)}
              </text>
            </g>
          ))}
          {GAINS.map((gain) => (
            <g key={gain}>
              <line
                x1={0}
                x2={graphSize.width}
                y1={gainToY(gain, graphSize.height)}
                y2={gainToY(gain, graphSize.height)}
                className={gain === 0 ? 'grid zero' : 'grid'}
              />
              <text
                x={graphSize.width - 8}
                y={gainToY(gain, graphSize.height) - 6}
                className="axis-label gain"
              >
                {gain > 0 ? '+' : ''}
                {gain}
              </text>
            </g>
          ))}

          <path
            className="response-fill"
            d={`${response} L${graphSize.width},${gainToY(0, graphSize.height)} L0,${gainToY(0, graphSize.height)} Z`}
            fill="url(#response-fill)"
          />
          {showIndividual && (
            <path d={selectedResponse} className="individual-response" />
          )}
          <path d={response} className="response-line-shadow" />
          <path d={response} className="response-line" />

          {params.bands.map((band, index) => {
            const nodeGain = ['bell', 'lowshelf', 'highshelf'].includes(
              band.bandType,
            )
              ? band.gainDb
              : 0
            const x = frequencyToX(band.freq, graphSize.width)
            const y = gainToY(nodeGain, graphSize.height)
            return (
              <g
                key={index}
                className={`band-node ${selected === index ? 'selected' : ''} ${band.active ? '' : 'disabled'}`}
                transform={`translate(${x} ${y})`}
                style={{ '--band-color': BAND_COLORS[index] } as CSSProperties}
                role="slider"
                tabIndex={0}
                aria-label={`Band ${index + 1}, ${formatFrequency(band.freq)} hertz`}
                onPointerDown={(event) => {
                  setSelected(index)
                  dragging.current = index
                  graphRef.current?.setPointerCapture(event.pointerId)
                }}
                onDoubleClick={() =>
                  updateBand(index, { gainDb: 0, q: 1 })
                }
                onWheel={(event) => onNodeWheel(event, index)}
                onKeyDown={(event) => {
                  const factor = event.shiftKey ? 1.01 : 1.05
                  if (
                    event.key === 'ArrowLeft' ||
                    event.key === 'ArrowRight'
                  ) {
                    event.preventDefault()
                    updateBand(index, {
                      freq: clamp(
                        band.freq *
                          (event.key === 'ArrowRight' ? factor : 1 / factor),
                        20,
                        20000,
                      ),
                    })
                  }
                  if (
                    event.key === 'ArrowUp' ||
                    event.key === 'ArrowDown'
                  ) {
                    event.preventDefault()
                    updateBand(index, {
                      gainDb: clamp(
                        band.gainDb +
                          (event.key === 'ArrowUp' ? 0.5 : -0.5),
                        -18,
                        18,
                      ),
                    })
                  }
                }}
              >
                {selected === index && (
                  <g className="node-readout" transform="translate(0 -34)">
                    <rect x="-52" y="-15" width="104" height="24" rx="3" />
                    <text textAnchor="middle" y="2">
                      {formatFrequency(band.freq)} Hz ·{' '}
                      {formatGain(band.gainDb)} dB
                    </text>
                  </g>
                )}
                <circle
                  r={selected === index ? 13 : 11}
                  className="node-ring"
                />
                <circle r={7} className="node-core" />
                <text textAnchor="middle" dominantBaseline="central">
                  {index + 1}
                </text>
              </g>
            )
          })}
        </svg>
      </section>

      <section className="control-deck">
        <div className="band-rail">
          {params.bands.map((band, index) => (
            <button
              key={index}
              className={`band-tab ${selected === index ? 'selected' : ''} ${band.active ? '' : 'disabled'}`}
              style={{ '--band-color': BAND_COLORS[index] } as CSSProperties}
              onClick={() => setSelected(index)}
            >
              <span className="band-index">{index + 1}</span>
              <span className="band-summary">
                <strong>
                  {
                    FILTER_TYPES.find((item) => item.type === band.bandType)
                      ?.label
                  }
                </strong>
                <small>{formatFrequency(band.freq)} Hz</small>
              </span>
              <span
                className="mini-power"
                role="switch"
                aria-checked={band.active}
                onClick={(event) => {
                  event.stopPropagation()
                  updateBand(index, { active: !band.active })
                }}
              />
            </button>
          ))}
        </div>

        <div
          className="detail-panel"
          style={{ '--band-color': BAND_COLORS[selected] } as CSSProperties}
        >
          <div className="filter-type">
            <span className="section-label">FILTER SHAPE</span>
            <div className="shape-grid">
              {FILTER_TYPES.map((item) => (
                <button
                  key={item.type}
                  className={selectedBand.bandType === item.type ? 'active' : ''}
                  title={item.label}
                  aria-label={item.label}
                  onClick={() =>
                    updateBand(selected, { bandType: item.type })
                  }
                >
                  <svg viewBox="0 0 32 20">
                    {item.type === 'highpass' && (
                      <path d="M3 17c8 0 6-12 15-12h11" />
                    )}
                    {item.type === 'lowpass' && (
                      <path d="M3 5h10c9 0 7 12 16 12" />
                    )}
                    {item.type === 'bell' && (
                      <path d="M3 15c7 0 7-10 13-10s6 10 13 10" />
                    )}
                    {item.type === 'notch' && (
                      <path d="M3 5c7 0 7 10 13 10s6-10 13-10" />
                    )}
                    {item.type === 'lowshelf' && (
                      <path d="M3 15h8c7 0 6-10 13-10h5" />
                    )}
                    {item.type === 'highshelf' && (
                      <path d="M3 5h5c7 0 6 10 13 10h8" />
                    )}
                  </svg>
                </button>
              ))}
            </div>
          </div>

          <ValueControl
            label="FREQ"
            value={selectedBand.freq}
            min={20}
            max={20000}
            step={1}
            unit="Hz"
            format={formatFrequency}
            defaultValue={DEFAULT_PARAMS.bands[selected]?.freq ?? 1000}
            toProgress={(freq) =>
              Math.log(freq / 20) / Math.log(20000 / 20)
            }
            fromProgress={(progress) =>
              20 * Math.pow(20000 / 20, progress)
            }
            onChange={(freq) => updateBand(selected, { freq })}
          />
          <ValueControl
            label="GAIN"
            value={selectedBand.gainDb}
            min={-18}
            max={18}
            step={0.1}
            unit="dB"
            format={formatGain}
            defaultValue={DEFAULT_PARAMS.bands[selected]?.gainDb ?? 0}
            disabled={!canGain}
            onChange={(gainDb) => updateBand(selected, { gainDb })}
          />
          <ValueControl
            label="Q"
            value={selectedBand.q}
            min={0.1}
            max={12}
            step={0.01}
            unit=""
            format={(value) => value.toFixed(2)}
            defaultValue={DEFAULT_PARAMS.bands[selected]?.q ?? 1}
            onChange={(q) => updateBand(selected, { q })}
          />

          <div className="dynamic-block">
            <div className="dynamic-heading">
              <span className="section-label">DYNAMIC</span>
              <button
                disabled
                title="Dynamic-band DSP is planned for the next DSP slice"
              >
                <i /> OFF
              </button>
            </div>
            <div className="dynamic-placeholder">
              <Icon size={17}>
                <path d="M3 12h4l2-6 4 12 2-6h6" />
              </Icon>
              <span>Static band</span>
              <small>Dynamic range &amp; threshold planned</small>
            </div>
          </div>

          <div className="output-block">
            <span className="section-label">OUTPUT</span>
            <ValueControl
              label="LEVEL"
              value={params.outputDb}
              min={-24}
              max={12}
              step={0.1}
              unit="dB"
              format={formatGain}
              defaultValue={0}
              onChange={(outputDb) => {
                setParams((current) => ({ ...current, outputDb }))
                postParam('outputDb', outputDb)
              }}
            />
          </div>
        </div>
      </section>

      <footer>
        <div>
          <span className="status-dot" /> ZERO LATENCY
        </div>
        <div className="hint">
          Drag a node · Scroll to adjust Q · Shift for fine control ·
          Double-click to reset
        </div>
        <div className="mix">
          <span>MIX</span>
          <input
            type="range"
            min="0"
            max="100"
            value={params.mix}
            onChange={(event) => {
              const mix = Number(event.target.value)
              setParams((current) => ({ ...current, mix }))
              postParam('mix', mix)
            }}
          />
          <strong>{Math.round(params.mix)}%</strong>
        </div>
      </footer>
    </main>
  )
}

export default App
