import { useMemo, useState, type CSSProperties } from 'react'
import {
  IconAdjustmentsHorizontal,
  IconChevronLeft,
  IconChevronRight,
  IconHeadphones,
  IconLayoutSidebar,
  IconMinus,
  IconMusic,
  IconPlayerPlay,
  IconPlayerSkipBack,
  IconPlayerSkipForward,
  IconPlayerStop,
  IconPlus,
  IconRefresh,
  IconSearch,
  IconSettings,
  IconX,
} from '@tabler/icons-react'
import logoSvg from './assets/logo.svg'
import { SvgIcon } from './components/SvgIcon'
import './App.scss'

type KnobProps = { label: string; value: number; unit?: string; accent?: string; onChange: (value: number) => void }

const libraries = [
  { name: 'Nocturne Piano', tag: 'KEYS', color: '#e8a96a', size: '1.84 GB' },
  { name: 'Glass & Wire', tag: 'PLUCK', color: '#73b5a5', size: '620 MB' },
  { name: 'Tape Choir', tag: 'VOCAL', color: '#a991d4', size: '1.12 GB' },
  { name: 'Analog Dust', tag: 'SYNTH', color: '#d26d68', size: '840 MB' },
]

const waveform = [12,18,25,44,72,38,21,32,84,65,36,24,42,91,58,30,19,36,78,48,24,18,33,69,93,51,28,19,43,76,54,31,22,17,28,57,81,46,25,18,31,67,88,48,29,19,37,71,52,27,16,26,59,80,44,23,14,31,65,91,55,32,20,17,42,73,48,26,18,34,62,85,51,28,19,29,55,74,43,25,16,32,68,87,46,27,18,35,66,49,24,15,29,54,78,42,22,13,28,61,82,45,24,17,34,59,76,41,22,15,26,52,70,39]

const noteNames = ['C', 'C♯', 'D', 'D♯', 'E', 'F', 'F♯', 'G', 'G♯', 'A', 'A♯', 'B']
const pianoNotes = Array.from({ length: 88 }, (_, index) => index + 21)
const isBlackNote = (note: number) => [1, 3, 6, 8, 10].includes(note % 12)
const whiteNotes = pianoNotes.filter((note) => !isBlackNote(note))
const blackNotes = pianoNotes.filter(isBlackNote)
const noteLabel = (note: number) => `${noteNames[note % 12]}${Math.floor(note / 12) - 1}`

function Knob({ label, value, unit = '', accent = '#d9a15f', onChange }: KnobProps) {
  const angle = -140 + value * 2.8
  return (
    <label className="knob-control">
      <span className="knob-label">{label}</span>
      <span className="knob" style={{ '--angle': `${angle}deg`, '--accent': accent } as CSSProperties}>
        <i /><input type="range" min="0" max="100" value={value} aria-label={label} onChange={(event) => onChange(Number(event.target.value))} />
      </span>
      <output>{value}{unit}</output>
    </label>
  )
}

function App() {
  const [browserTab, setBrowserTab] = useState<'Libraries' | 'Files'>('Libraries')
  const [selectedLibrary, setSelectedLibrary] = useState(0)
  const [activeInstrument, setActiveInstrument] = useState(0)
  const [editorTab, setEditorTab] = useState<'Instrument' | 'Mapping' | 'Wave'>('Instrument')
  const [playing, setPlaying] = useState(false)
  const [solo, setSolo] = useState(false)
  const [mute, setMute] = useState(false)
  const [activeNotes, setActiveNotes] = useState<number[]>([])
  const [sustain, setSustain] = useState(false)
  const [velocityCurve, setVelocityCurve] = useState<'SOFT' | 'LINEAR' | 'HARD'>('LINEAR')
  const [browserOpen, setBrowserOpen] = useState(true)
  const [selectedKeyswitch, setSelectedKeyswitch] = useState(24)
  const [values, setValues] = useState({ volume: 76, pan: 50, tune: 50, tone: 58, body: 43, noise: 27, attack: 8, release: 62, velocity: 74, space: 36 })
  const setValue = (key: keyof typeof values) => (value: number) => setValues((current) => ({ ...current, [key]: value }))

  const rack = useMemo(() => [
    { name: libraries[selectedLibrary].name, patch: selectedLibrary === 0 ? 'Felt / Intimate' : 'Main Ensemble', midi: 'A 1', output: 'st. 1', memory: selectedLibrary === 0 ? '512 MB' : '286 MB', color: libraries[selectedLibrary].color },
    { name: 'Tape Choir', patch: 'Breath Ensemble', midi: 'A 2', output: 'st. 2', memory: '348 MB', color: '#a991d4' },
  ], [selectedLibrary])

  const toggleNote = (note: number, down: boolean) => {
    if (down && note >= 24 && note <= 28) setSelectedKeyswitch(note)
    setActiveNotes((current) => down ? [...new Set([...current, note])] : current.filter((item) => item !== note))
  }

  const keyPosition = (note: number) => {
    if (isBlackNote(note)) {
      const precedingWhites = pianoNotes.filter((item) => item < note && !isBlackNote(item)).length
      return (precedingWhites / whiteNotes.length) * 100
    }
    const whiteIndex = whiteNotes.indexOf(note)
    return ((whiteIndex + 0.5) / whiteNotes.length) * 100
  }

  const ledState = (note: number) => {
    if (activeNotes.includes(note)) return 'is-active'
    if (note >= 24 && note <= 28) return note === selectedKeyswitch ? 'is-keyswitch is-selected' : 'is-keyswitch'
    if (note === 29) return 'is-latch'
    if (note >= 100) return 'is-unmapped'
    return ''
  }

  return (
    <main className="workspace">
      <section className="plugin-shell" aria-label="OpenSampler virtual instrument">
        <header className="titlebar">
          <div className="logo"><img src={logoSvg} alt="OpenSampler" /></div>
          <div className="global-preset"><button type="button" aria-label="Previous multi"><IconChevronLeft /></button><span><small>MULTI</small><strong>Studio Sketch 01</strong></span><button type="button" aria-label="Next multi"><IconChevronRight /></button></div>
          <div className="telemetry">
            <span><small>VOICES</small><b>12</b></span><span><small>MEMORY</small><b>860 MB</b></span>
            <div className="cpu"><small>CPU</small><i /><i /><i /><i /><i /></div>
          </div>
          <button className="settings icon-button" type="button" aria-label="Settings"><IconSettings /></button>
        </header>

        <div className="toolbar">
          <button type="button" className={browserOpen ? 'is-active' : ''} onClick={() => setBrowserOpen(!browserOpen)}><IconLayoutSidebar />BROWSER</button>
          <button type="button"><IconMusic />MASTER</button>
          <button type="button"><IconAdjustmentsHorizontal />OUTPUTS</button>
          <div className="toolbar__spacer" />
          <button type="button"><IconHeadphones />AUDITION</button>
          <button type="button"><IconPlus />ADD INSTRUMENT</button>
        </div>

        <div className={`main-area ${browserOpen ? '' : 'browser-is-closed'}`}>
          <aside className="browser-panel">
            <div className="browser-tabs">
              {(['Libraries', 'Files'] as const).map((tab) => <button type="button" key={tab} className={browserTab === tab ? 'is-active' : ''} onClick={() => setBrowserTab(tab)}>{tab}</button>)}
            </div>
            <label className="search"><IconSearch /><input aria-label="Search library" placeholder="Search instruments" /></label>
            <div className="browser-path"><span>COLLECTIONS</span><button className="icon-button" type="button" aria-label="Add collection"><IconPlus /></button></div>
            {browserTab === 'Libraries' ? (
              <div className="library-list">
                {libraries.map((library, index) => (
                  <button type="button" key={library.name} className={selectedLibrary === index ? 'is-selected' : ''} onClick={() => setSelectedLibrary(index)}>
                    <i style={{ '--library-color': library.color } as CSSProperties}><span>{library.name.charAt(0)}</span></i>
                    <span><strong>{library.name}</strong><small>{library.tag} · {library.size}</small></span>
                    <b><IconChevronRight /></b>
                  </button>
                ))}
              </div>
            ) : (
              <div className="file-tree">
                <span>⌄ Favorites</span><span>⌄ This Computer</span><span className="indent">▱ Samples</span><span className="indent">▱ Instruments</span><span>› Recent</span>
              </div>
            )}
            <div className="browser-footer"><span>4 LIBRARIES</span><button type="button">LOCATE</button></div>
          </aside>

          <section className="rack-area">
            <div className="rack-heading"><span>RACK</span><div><button type="button">1–16</button><button type="button">17–32</button></div><small>2 INSTRUMENTS</small></div>
            <div className="instrument-rack">
              {rack.map((instrument, index) => (
                <div className={`rack-slot ${activeInstrument === index ? 'is-active' : ''}`} key={`${instrument.name}-${index}`}>
                  <button type="button" className="rack-select" onClick={() => setActiveInstrument(index)} aria-label={`Select ${instrument.name}`}>
                    <i className="rack-color" style={{ '--rack-color': instrument.color } as CSSProperties} />
                    <span className="rack-number">{String(index + 1).padStart(2, '0')}</span>
                    <span className="rack-name"><strong>{instrument.name}</strong><small>{instrument.patch}</small></span>
                  </button>
                  <span className="rack-data"><small>MIDI</small><b>{instrument.midi}</b></span>
                  <span className="rack-data"><small>OUTPUT</small><b>{instrument.output}</b></span>
                  <span className="rack-data"><small>MEMORY</small><b>{instrument.memory}</b></span>
                  <span className="rack-level"><i /><i /></span>
                  <span className="rack-buttons"><button type="button" aria-label={`Solo ${instrument.name}`}>S</button><button type="button" aria-label={`Mute ${instrument.name}`}>M</button><button type="button" aria-label={`Remove ${instrument.name}`}><IconX /></button></span>
                </div>
              ))}
              <button className="empty-slot" type="button"><IconPlus /> <span>Drop instrument or sample here</span></button>
            </div>

            <article className="instrument-editor">
              <header className="instrument-header">
                <div className="instrument-avatar" style={{ '--rack-color': rack[activeInstrument].color } as CSSProperties}><span>{rack[activeInstrument].name.charAt(0)}</span></div>
                <div className="instrument-title"><small>INSTRUMENT {String(activeInstrument + 1).padStart(2, '0')}</small><strong>{rack[activeInstrument].name}</strong><span>{rack[activeInstrument].patch}</span></div>
                <div className="instrument-actions"><button className={solo ? 'is-on' : ''} type="button" onClick={() => setSolo(!solo)}>S</button><button className={mute ? 'is-on' : ''} type="button" onClick={() => setMute(!mute)}>M</button><button className="icon-button" type="button" aria-label="Reload instrument"><IconRefresh /></button></div>
                <Knob label="TUNE" value={values.tune} onChange={setValue('tune')} />
                <Knob label="PAN" value={values.pan} onChange={setValue('pan')} />
                <Knob label="VOLUME" value={values.volume} unit="%" onChange={setValue('volume')} />
                <div className="header-meter"><i /><i /><i /><i /><i /><i /></div>
              </header>
              <nav className="editor-tabs">
                {(['Instrument', 'Mapping', 'Wave'] as const).map((tab) => <button type="button" key={tab} className={editorTab === tab ? 'is-active' : ''} onClick={() => setEditorTab(tab)}>{tab}</button>)}
                <span /><button type="button">SCRIPT</button><button type="button">OPTIONS</button>
              </nav>

              <div className="editor-body">
                <section className="wave-panel">
                  <div className="wave-toolbar"><span>felt_c3_rr02.wav</span><small>48 kHz · 24 bit · 00:03.842</small><div><button className="icon-button" type="button" aria-label="Zoom out"><IconMinus /></button><button className="icon-button" type="button" aria-label="Zoom in"><IconPlus /></button></div></div>
                  <div className="waveform">
                    <div className="wave-ruler"><span>0.0</span><span>1.0</span><span>2.0</span><span>3.0</span></div>
                    <div className="wave-bars">{waveform.map((height, index) => <i key={index} style={{ height: `${height}%` }} />)}</div>
                    <span className="start-marker">START</span><span className="end-marker">END</span>
                    <span className="loop-marker loop-marker--start">L</span><span className="loop-marker loop-marker--end">R</span>
                    <div className="playhead" style={{ left: playing ? '74%' : '28%' }} />
                  </div>
                  <div className="transport"><button className="icon-button" type="button" aria-label="Previous sample"><IconPlayerSkipBack /></button><button className={`icon-button ${playing ? 'is-playing' : ''}`} type="button" aria-label={playing ? 'Stop' : 'Play'} onClick={() => setPlaying(!playing)}>{playing ? <IconPlayerStop /> : <IconPlayerPlay />}</button><button className="icon-button" type="button" aria-label="Next sample"><IconPlayerSkipForward /></button><span>ROOT <b>C3</b></span><span>LOOP <b>OFF</b></span><span>SNAP <b>ZERO</b></span></div>
                </section>

                <section className="control-panel">
                  <div className="control-group"><header><span>TONE</span><button type="button">BYPASS</button></header><div><Knob label="COLOR" value={values.tone} accent="#73b5a5" onChange={setValue('tone')} /><Knob label="BODY" value={values.body} accent="#73b5a5" onChange={setValue('body')} /><Knob label="NOISE" value={values.noise} accent="#73b5a5" onChange={setValue('noise')} /></div></div>
                  <div className="control-group"><header><span>ENVELOPE</span><button type="button">AMP</button></header><div><Knob label="ATTACK" value={values.attack} accent="#a991d4" onChange={setValue('attack')} /><Knob label="RELEASE" value={values.release} accent="#a991d4" onChange={setValue('release')} /><Knob label="VELOCITY" value={values.velocity} accent="#a991d4" onChange={setValue('velocity')} /></div></div>
                  <div className="control-group control-group--space"><header><span>SPACE</span><button type="button">ROOM A</button></header><div><Knob label="AMOUNT" value={values.space} accent="#d26d68" onChange={setValue('space')} /><div className="space-viz"><i/><i/><i/><i/><i/></div></div></div>
                </section>
              </div>
            </article>
          </section>
        </div>

        <section className="keyboard-section">
          <div className="keyboard-toolbar">
            <div className="keyboard-title"><SvgIcon className="custom-keyboard-icon"><path d="M3 7h18v10H3zM7 7v6m4-6v6m4-6v6m4-6v6" /><path d="M5 4h14" /></SvgIcon><span><strong>VIRTUAL PIANO</strong><small>88 KEY · FULL RANGE</small></span></div>
            <div className="performance-readout"><span><small>NOTE</small><strong>{activeNotes.length ? noteLabel(activeNotes[activeNotes.length - 1]) : '—'}</strong></span><span><small>VEL</small><strong>{activeNotes.length ? '100' : '000'}</strong></span><span><small>ART</small><strong>{['SUS','STAC','LEG','PIZZ','TREM'][selectedKeyswitch - 24]}</strong></span></div>
            <div className="curve-selector"><small>CURVE</small>{(['SOFT', 'LINEAR', 'HARD'] as const).map((curve) => <button type="button" key={curve} className={velocityCurve === curve ? 'is-active' : ''} onClick={() => setVelocityCurve(curve)}>{curve}</button>)}</div>
            <span className="midi-field"><small>OCT</small><button className="icon-button" type="button" aria-label="Octave down"><IconMinus /></button><b>0</b><button className="icon-button" type="button" aria-label="Octave up"><IconPlus /></button></span>
            <span className="midi-field"><small>MIDI</small><b>CH 01</b></span>
            <button type="button" className={`sustain-button ${sustain ? 'is-active' : ''}`} onClick={() => setSustain(!sustain)}><i />SUSTAIN</button>
            <span className="range-badge">A0 <i /> C8</span>
          </div>
          <div className="keyboard-performance">
            <div className="performance-controls"><div className="wheel"><i /></div><div className="wheel wheel--mod"><i /></div><span><small>PITCH</small><small>MOD</small></span></div>
            <div className="keybed-scroll">
              <div className="keybed">
                <div className="articulation-labels" aria-label="Keyswitch articulations">
                  {['SUS','STAC','LEG','PIZZ','TREM'].map((label, index) => <button type="button" key={label} className={selectedKeyswitch === index + 24 ? 'is-selected' : ''} style={{ left: `${keyPosition(index + 24)}%` }} onClick={() => setSelectedKeyswitch(index + 24)}>{label}</button>)}
                </div>
                <div className="led-strip" aria-label="MIDI note indicator strip">
                  <span className="articulation-range" style={{ left: `${keyPosition(24) - .6}%`, width: `${keyPosition(28) - keyPosition(24) + 1.2}%` }} />
                  {pianoNotes.map((note) => <i key={note} className={`led-cell ${ledState(note)} ${note === 60 ? 'is-root' : ''}`} style={{ left: `${keyPosition(note)}%` }} title={noteLabel(note)} />)}
                </div>
                <div className="keyboard" aria-label="88-key virtual piano">
                  {whiteNotes.map((note) => (
                    <button
                      key={note}
                      type="button"
                      className={`white-key ${activeNotes.includes(note) ? 'is-active' : ''}`}
                      onPointerDown={() => toggleNote(note, true)}
                      onPointerUp={() => !sustain && toggleNote(note, false)}
                      onPointerLeave={() => !sustain && toggleNote(note, false)}
                      aria-label={`Play ${noteLabel(note)}`}
                    ><span>{note % 12 === 0 || note === 21 || note === 108 ? noteLabel(note) : ''}</span></button>
                  ))}
                  {blackNotes.map((note) => (
                  <button
                    key={note}
                    type="button"
                    className={`black-key ${activeNotes.includes(note) ? 'is-active' : ''}`}
                    style={{ left: `${keyPosition(note)}%` }}
                    onPointerDown={() => toggleNote(note, true)}
                    onPointerUp={() => !sustain && toggleNote(note, false)}
                    onPointerLeave={() => !sustain && toggleNote(note, false)}
                    aria-label={`Play ${noteLabel(note)}`}
                  />
                  ))}
                </div>
              </div>
            </div>
          </div>
        </section>

        <footer className="statusbar"><span><i className="status-dot"/>AUDIO ENGINE READY</span><span>48,000 Hz / 256 smp</span><span className="statusbar__center">OpenSampler 0.9.2</span><span>DISK 3.2 MB/s</span><span>Futureboard Audio</span></footer>
      </section>
    </main>
  )
}

export default App
