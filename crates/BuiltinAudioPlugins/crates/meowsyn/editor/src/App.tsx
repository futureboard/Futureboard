import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react'
import './App.scss'

type Voice = {
  oscillators: OscillatorNode[]
  noise?: AudioBufferSourceNode
  amp: GainNode
}

type VoiceSettings = {
  attack: number
  release: number
  cutoff: number
  resonance: number
  shape: number
  sub: number
  drive: number
  volume: number
}

class CatSynthEngine {
  private context?: AudioContext
  private master?: GainNode
  private compressor?: DynamicsCompressorNode
  private voices = new Map<number, Voice>()

  async wake(volume: number) {
    if (!this.context) {
      this.context = new AudioContext()
      this.master = this.context.createGain()
      this.compressor = this.context.createDynamicsCompressor()
      this.compressor.threshold.value = -16
      this.compressor.knee.value = 12
      this.compressor.ratio.value = 4
      this.master.connect(this.compressor).connect(this.context.destination)
    }
    this.setVolume(volume)
    if (this.context.state === 'suspended') await this.context.resume()
  }

  setVolume(volume: number) {
    if (!this.context || !this.master) return
    this.master.gain.setTargetAtTime(Math.pow(volume / 100, 1.7) * 0.46, this.context.currentTime, 0.015)
  }

  noteOn(note: number, settings: VoiceSettings, automatic = false) {
    if (!this.context || !this.master || this.voices.has(note)) return
    const ctx = this.context
    const now = ctx.currentTime
    const frequency = 440 * Math.pow(2, (note - 69) / 12)
    const amp = ctx.createGain()
    const warmth = ctx.createBiquadFilter()
    const formantA = ctx.createBiquadFilter()
    const formantB = ctx.createBiquadFilter()
    const formantC = ctx.createBiquadFilter()
    const dry = ctx.createGain()
    const wet = ctx.createGain()

    amp.gain.setValueAtTime(0.0001, now)
    amp.gain.exponentialRampToValueAtTime(0.58, now + 0.012 + settings.attack * 0.0022)
    if (automatic) {
      amp.gain.setValueAtTime(0.58, now + 0.16)
      amp.gain.exponentialRampToValueAtTime(0.0001, now + 0.82)
    }

    warmth.type = 'lowpass'
    warmth.frequency.value = 700 + settings.cutoff * 42
    warmth.Q.value = 0.4 + settings.resonance * 0.085
    formantA.type = formantB.type = formantC.type = 'bandpass'
    formantA.Q.value = 7.5
    formantB.Q.value = 9
    formantC.Q.value = 10
    formantA.frequency.setValueAtTime(780, now)
    formantA.frequency.exponentialRampToValueAtTime(340, now + 0.48)
    formantB.frequency.setValueAtTime(1080, now)
    formantB.frequency.exponentialRampToValueAtTime(2180, now + 0.52)
    formantC.frequency.setValueAtTime(2600, now)
    formantC.frequency.exponentialRampToValueAtTime(2850, now + 0.6)
    dry.gain.value = 0.26
    wet.gain.value = 0.78 + settings.drive * 0.003

    warmth.connect(dry).connect(amp)
    warmth.connect(formantA).connect(wet)
    warmth.connect(formantB).connect(wet)
    warmth.connect(formantC).connect(wet)
    wet.connect(amp)
    amp.connect(this.master)

    const oscillators = [0, 1, 2].map((voiceIndex) => {
      const oscillator = ctx.createOscillator()
      const level = ctx.createGain()
      oscillator.type = settings.shape > 70 ? 'square' : settings.shape < 35 ? 'triangle' : 'sawtooth'
      level.gain.value = voiceIndex === 0 ? 0.72 : voiceIndex === 1 ? 0.18 : settings.sub * 0.003
      const base = voiceIndex === 2 ? frequency / 2 : frequency * (voiceIndex === 1 ? 2 : 1)
      oscillator.frequency.setValueAtTime(base * 1.16, now)
      oscillator.frequency.exponentialRampToValueAtTime(base, now + 0.105)
      oscillator.detune.setValueAtTime(voiceIndex === 1 ? 4 : -3, now)
      oscillator.detune.setValueAtTime(voiceIndex === 1 ? 4 : -3, now + 0.2)
      oscillator.detune.linearRampToValueAtTime(voiceIndex === 1 ? 18 : 11, now + 0.65)
      oscillator.connect(level).connect(warmth)
      oscillator.start(now)
      if (automatic) oscillator.stop(now + 0.86)
      return oscillator
    })

    const noiseBuffer = ctx.createBuffer(1, Math.floor(ctx.sampleRate * 0.09), ctx.sampleRate)
    const samples = noiseBuffer.getChannelData(0)
    for (let i = 0; i < samples.length; i += 1) samples[i] = (Math.random() * 2 - 1) * (1 - i / samples.length)
    const noise = ctx.createBufferSource()
    const noiseGain = ctx.createGain()
    const noiseFilter = ctx.createBiquadFilter()
    noise.buffer = noiseBuffer
    noiseGain.gain.value = 0.085
    noiseFilter.type = 'bandpass'
    noiseFilter.frequency.value = 1750
    noise.connect(noiseFilter).connect(noiseGain).connect(amp)
    noise.start(now)

    this.voices.set(note, { oscillators, noise, amp })
    if (automatic) window.setTimeout(() => this.voices.delete(note), 900)
  }

  noteOff(note: number, release: number) {
    const voice = this.voices.get(note)
    if (!voice || !this.context) return
    const now = this.context.currentTime
    const tail = 0.055 + release * 0.006
    voice.amp.gain.cancelScheduledValues(now)
    voice.amp.gain.setTargetAtTime(0.0001, now, Math.max(0.015, tail / 5))
    voice.oscillators.forEach((oscillator) => oscillator.stop(now + tail + 0.08))
    this.voices.delete(note)
  }

  stopAll() {
    if (!this.context) return
    for (const note of this.voices.keys()) this.noteOff(note, 2)
  }

  destroy() {
    this.stopAll()
    void this.context?.close()
  }
}

type KnobProps = {
  label: string
  value: number
  min?: number
  max?: number
  unit?: string
  size?: 'sm' | 'md' | 'lg'
  onChange: (value: number) => void
}

const waveformPath = 'M0 44 C13 44 14 14 28 14 S42 73 56 73 S70 28 84 28 S98 55 112 55 S126 38 140 38 S154 48 168 48 S182 42 196 42 S210 45 224 45 S238 43 252 43 S266 44 280 44'

function Knob({ label, value, min = 0, max = 100, unit = '', size = 'md', onChange }: KnobProps) {
  const ratio = (value - min) / (max - min)
  const angle = -135 + ratio * 270
  const display = Number.isInteger(value) ? value : value.toFixed(1)

  return (
    <label className={`knob-control knob-control--${size}`}>
      <span className="knob__label">{label}</span>
      <span className="knob" style={{ '--angle': `${angle}deg` } as CSSProperties}>
        <span className="knob__cap"><i /></span>
        <input
          aria-label={label}
          type="range"
          min={min}
          max={max}
          step={max <= 10 ? 0.1 : 1}
          value={value}
          onChange={(event) => onChange(Number(event.target.value))}
        />
      </span>
      <span className="knob__value">{display}{unit}</span>
    </label>
  )
}

function Toggle({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button className={`toggle ${active ? 'is-active' : ''}`} type="button" onClick={onClick} aria-pressed={active}>
      <span className="toggle__lamp" />
      {label}
    </button>
  )
}

function App() {
  const [preset, setPreset] = useState(0)
  const [power, setPower] = useState(true)
  const [arp, setArp] = useState(true)
  const [hold, setHold] = useState(false)
  const [wave, setWave] = useState<'SAW' | 'SQR' | 'TRI'>('SAW')
  const [steps, setSteps] = useState([true, false, false, true, false, true, false, false, true, false, true, false, false, true, false, false])
  const [activeNotes, setActiveNotes] = useState<number[]>([])
  const [audioReady, setAudioReady] = useState(false)
  const synthRef = useRef<CatSynthEngine | null>(null)
  const [values, setValues] = useState({
    octave: 0,
    tune: 0,
    shape: 62,
    sub: 28,
    cutoff: 68,
    resonance: 38,
    drive: 24,
    attack: 6,
    decay: 44,
    sustain: 72,
    release: 31,
    rate: 42,
    depth: 26,
    mix: 77,
    volume: 71,
  })

  const setValue = (name: keyof typeof values) => (value: number) => setValues((current) => ({ ...current, [name]: value }))
  const presets = ['WARM WHISKERS', 'ALLEY BASS', 'VELVET PAWS', 'NIGHT PROWL']

  const cyclePreset = (direction: number) => {
    setPreset((current) => (current + direction + presets.length) % presets.length)
  }

  const toggleStep = (index: number) => {
    setSteps((current) => current.map((step, stepIndex) => stepIndex === index ? !step : step))
  }

  const playNote = useCallback(async (note: number, automatic = false) => {
    if (!power) return
    if (!synthRef.current) synthRef.current = new CatSynthEngine()
    await synthRef.current.wake(values.volume)
    setAudioReady(true)
    synthRef.current.noteOn(note, {
      attack: values.attack,
      release: values.release,
      cutoff: values.cutoff,
      resonance: values.resonance,
      shape: values.shape,
      sub: values.sub,
      drive: values.drive,
      volume: values.volume,
    }, automatic)
    setActiveNotes((current) => current.includes(note) ? current : [...current, note])
    if (automatic) window.setTimeout(() => setActiveNotes((current) => current.filter((item) => item !== note)), 850)
  }, [power, values.volume, values.attack, values.release, values.cutoff, values.resonance, values.shape, values.sub, values.drive])

  const stopNote = useCallback((note: number) => {
    synthRef.current?.noteOff(note, values.release)
    setActiveNotes((current) => current.filter((item) => item !== note))
  }, [values.release])

  useEffect(() => synthRef.current?.setVolume(power ? values.volume : 0), [power, values.volume])
  useEffect(() => () => synthRef.current?.destroy(), [])

  useEffect(() => {
    const keyMap: Record<string, number> = { z: 48, s: 49, x: 50, d: 51, c: 52, v: 53, g: 54, b: 55, h: 56, n: 57, j: 58, m: 59, ',': 60 }
    const down = (event: KeyboardEvent) => {
      const note = keyMap[event.key.toLowerCase()]
      if (note !== undefined && !event.repeat) void playNote(note)
    }
    const up = (event: KeyboardEvent) => {
      const note = keyMap[event.key.toLowerCase()]
      if (note !== undefined) stopNote(note)
    }
    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    return () => { window.removeEventListener('keydown', down); window.removeEventListener('keyup', up) }
  }, [playNote, stopNote])

  return (
    <main className="studio">
      <section className={`synth ${power ? '' : 'is-off'}`} aria-label="MeowSynth virtual synthesizer">
        <header className="topbar">
          <div className="brand">
            <span className="brand__mark" aria-hidden="true">M</span>
            <div><strong>MEOWSYNTH</strong><small>POLYPHONIC ANALOG INSTRUMENT</small></div>
          </div>

          <div className="preset-browser">
            <button type="button" onClick={() => cyclePreset(-1)} aria-label="Previous preset">‹</button>
            <div className="preset-display">
              <span>PRESET {String(preset + 1).padStart(2, '0')}</span>
              <strong>{presets[preset]}</strong>
            </div>
            <button type="button" onClick={() => cyclePreset(1)} aria-label="Next preset">›</button>
          </div>

          <div className="topbar__actions">
            <button className="utility-button" type="button">SAVE</button>
            <button className="power-button" type="button" onClick={() => setPower((current) => !current)} aria-pressed={power}>
              <span /> POWER
            </button>
          </div>
        </header>

        <div className="instrument-panel">
          <aside className="rail rail--left">
            <span>MOD</span>
            <div className="vertical-slider"><i style={{ bottom: `${values.depth}%` }} /><input aria-label="Modulation" type="range" value={values.depth} onChange={(event) => setValue('depth')(Number(event.target.value))} /></div>
            <span>PITCH</span>
            <div className="pitch-wheel"><i /></div>
          </aside>

          <div className="modules">
            <section className="module oscillator">
              <div className="module__heading"><span>01</span><h2>OSCILLATOR</h2><small>VOICE SOURCE</small></div>
              <div className="oscillator__body">
                <div className="wave-select" aria-label="Waveform selector">
                  {(['SAW', 'SQR', 'TRI'] as const).map((item) => (
                    <button key={item} type="button" className={wave === item ? 'is-active' : ''} onClick={() => setWave(item)}>
                      <svg viewBox="0 0 32 18" aria-hidden="true">
                        {item === 'SAW' && <path d="M2 15 16 3v12L30 3" />}
                        {item === 'SQR' && <path d="M2 15V3h14v12h14V3" />}
                        {item === 'TRI' && <path d="M2 15 9 3l14 12 7-12" />}
                      </svg>
                      {item}
                    </button>
                  ))}
                </div>
                <div className="control-row">
                  <Knob label="OCTAVE" value={values.octave} min={-2} max={2} onChange={setValue('octave')} />
                  <Knob label="TUNE" value={values.tune} min={-12} max={12} unit=" st" onChange={setValue('tune')} />
                  <Knob label="SHAPE" value={values.shape} onChange={setValue('shape')} />
                  <Knob label="SUB" value={values.sub} onChange={setValue('sub')} />
                </div>
              </div>
            </section>

            <section className="module scope-module">
              <div className="scope">
                <div className="scope__grid" />
                <svg viewBox="0 0 280 88" preserveAspectRatio="none" aria-label="Oscillator waveform display">
                  <path className="scope__glow" d={waveformPath} />
                  <path d={waveformPath} />
                </svg>
                <div className="scope__meta"><span>POLY 08</span><span>A4 / 440.0</span></div>
              </div>
              <div className="scope-controls">
                <Knob label="RATE" value={values.rate} size="sm" onChange={setValue('rate')} />
                <Knob label="DEPTH" value={values.depth} size="sm" onChange={setValue('depth')} />
                <div className="mini-toggles"><Toggle label="SYNC" active={true} onClick={() => undefined} /><Toggle label="KEY" active={false} onClick={() => undefined} /></div>
              </div>
            </section>

            <section className="module filter">
              <div className="module__heading"><span>02</span><h2>FILTER</h2><small>24 dB LADDER</small></div>
              <div className="filter__body control-row">
                <Knob label="CUTOFF" value={values.cutoff} size="lg" unit="%" onChange={setValue('cutoff')} />
                <Knob label="RESONANCE" value={values.resonance} onChange={setValue('resonance')} />
                <Knob label="DRIVE" value={values.drive} onChange={setValue('drive')} />
              </div>
            </section>

            <section className="module envelope">
              <div className="module__heading"><span>03</span><h2>ENVELOPE</h2><small>AMPLIFIER</small></div>
              <svg className="envelope__graph" viewBox="0 0 260 72" aria-hidden="true"><path d="M3 65 37 8 89 27h82l60 38h26" /><path className="envelope__fill" d="M3 65 37 8 89 27h82l60 38Z" /></svg>
              <div className="control-row">
                <Knob label="ATTACK" value={values.attack} size="sm" onChange={setValue('attack')} />
                <Knob label="DECAY" value={values.decay} size="sm" onChange={setValue('decay')} />
                <Knob label="SUSTAIN" value={values.sustain} size="sm" onChange={setValue('sustain')} />
                <Knob label="RELEASE" value={values.release} size="sm" onChange={setValue('release')} />
              </div>
            </section>

            <section className="module output">
              <div className="module__heading"><span>04</span><h2>OUTPUT</h2><small>MASTER</small></div>
              <div className="output__body">
                <div className="meter"><i /><i /><i /><i /><i /><i /><i /><i /></div>
                <Knob label="MIX" value={values.mix} onChange={setValue('mix')} />
                <Knob label="VOLUME" value={values.volume} size="lg" onChange={setValue('volume')} />
              </div>
            </section>
          </div>

          <aside className="rail rail--right">
            <span>VOICE</span><strong>08</strong>
            <span>GLIDE</span><div className="rail-knob" />
            <span>AGE</span><div className="rail-knob rail-knob--low" />
          </aside>
        </div>

        <section className="sequencer">
          <div className="sequencer__controls">
            <div><span>ARP / SEQUENCER</span><strong>118 <small>BPM</small></strong></div>
            <Toggle label="ARP" active={arp} onClick={() => setArp((current) => !current)} />
            <Toggle label="HOLD" active={hold} onClick={() => setHold((current) => !current)} />
            <button className="play-button" type="button" aria-label="Play sequence">▶</button>
          </div>
          <div className="steps">
            {steps.map((active, index) => (
              <button key={index} type="button" className={active ? 'is-active' : ''} onClick={() => toggleStep(index)} aria-label={`Step ${index + 1}`} aria-pressed={active}>
                <i style={{ height: `${20 + ((index * 17) % 48)}%` }} /><span>{String(index + 1).padStart(2, '0')}</span>
              </button>
            ))}
          </div>
        </section>

        <section className="performance-panel">
          <div className="cat-trigger">
            <div><span>CAT VOICE</span><strong>FORMANT ENGINE</strong><small>{audioReady ? 'AUDIO ACTIVE' : 'CLICK TO WAKE'}</small></div>
            <button type="button" onPointerDown={() => void playNote(55, true)} aria-label="Trigger meow sound">
              <span className="cat-face" aria-hidden="true"><i /><i /><b>ᴗ</b></span>
              MEOW
            </button>
          </div>
          <div className="keyboard" aria-label="Playable keyboard">
            {[48, 50, 52, 53, 55, 57, 59, 60].map((note, index) => (
              <button
                key={note}
                type="button"
                className={`key key--white ${activeNotes.includes(note) ? 'is-active' : ''}`}
                onPointerDown={() => void playNote(note)}
                onPointerUp={() => stopNote(note)}
                onPointerLeave={() => activeNotes.includes(note) && stopNote(note)}
                aria-label={`Play note ${note}`}
              ><span>{['Z','X','C','V','B','N','M',','][index]}</span></button>
            ))}
            {[
              { note: 49, left: 8.1, key: 'S' }, { note: 51, left: 20.6, key: 'D' },
              { note: 54, left: 45.6, key: 'G' }, { note: 56, left: 58.1, key: 'H' }, { note: 58, left: 70.6, key: 'J' },
            ].map(({ note, left, key }) => (
              <button
                key={note}
                type="button"
                className={`key key--black ${activeNotes.includes(note) ? 'is-active' : ''}`}
                style={{ left: `${left}%` }}
                onPointerDown={() => void playNote(note)}
                onPointerUp={() => stopNote(note)}
                onPointerLeave={() => activeNotes.includes(note) && stopNote(note)}
                aria-label={`Play note ${note}`}
              ><span>{key}</span></button>
            ))}
          </div>
          <div className="voice-info"><span>GESTURE</span><strong>M–YOW</strong><small>Z–M TO PLAY</small></div>
        </section>

        <footer className="footer-strip"><span>MEOWSYNTH / MS–01</span><span>DESIGNED FOR CURIOUS EARS</span><span>REV 1.0.4</span></footer>
      </section>
    </main>
  )
}

export default App
