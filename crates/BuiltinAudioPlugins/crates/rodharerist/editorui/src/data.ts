export type CategoryId =
  | "dyn"
  | "dist"
  | "amp"
  | "mod"
  | "delay"
  | "verb"
  | "cab";

export type Category = {
  name: string;
  short: string;
  color: string;
  rgb: string;
  node: string;
};

export type Preset = {
  id: string;
  name: string;
  category: CategoryId;
  model: string;
  values: Record<string, number>;
  /** Stages in the signal path (Helix order). Empty = empty path. */
  path?: CategoryId[];
  /** Stages bypassed (off) when loaded. */
  bypassed?: CategoryId[];
};

export type Model = {
  id: string;
  name: string;
  /** Compact label for Path blocks */
  short: string;
  sub: string;
};

export type Param = {
  id: string;
  name: string;
  min: number;
  max: number;
  val: number;
  unit: string;
};

export const presetsData: Preset[] = [
  {
    id: "00A",
    name: "Empty",
    category: "amp",
    model: "mandarin",
    values: {},
    path: [],
    bypassed: ["dyn", "dist", "amp", "mod", "delay", "verb", "cab"],
  },
  {
    id: "01A",
    name: "Twin Sparkle",
    category: "amp",
    model: "twin",
    values: { amp_gain: 2.5, amp_bass: 4.5, amp_middle: 5.0, amp_treble: 6.5, amp_presence: 5.5, amp_master: 5.5 },
  },
  {
    id: "02B",
    name: "Dark & Lush",
    category: "verb",
    model: "plate",
    values: { reverb_decay: 8.5 },
  },
  {
    id: "02C",
    name: "Tripod Gallop",
    category: "dist",
    model: "screamer",
    values: { drive_gain: 7.5 },
  },
  {
    id: "02D",
    name: "SC Small Lead",
    category: "delay",
    model: "tape",
    values: { delay_mix: 30 },
  },
  {
    id: "03A",
    name: "Wild Year",
    category: "mod",
    model: "chorus",
    values: { chorus_mix: 40 },
  },
  {
    id: "04A",
    name: "Rats Nest Riff",
    category: "dist",
    model: "rat",
    values: { drive_gain: 8.0, drive_tone: 4.0 },
  },
  {
    id: "05A",
    name: "Chime Room",
    category: "amp",
    model: "topboost",
    values: { amp_gain: 4.0, amp_middle: 3.5, amp_treble: 7.0, amp_presence: 6.5 },
  },
  {
    id: "06D",
    name: "Mandarine Gaze",
    category: "amp",
    model: "mandarin",
    values: {
      amp_gain: 6.0,
      amp_bass: 5.1,
      amp_middle: 4.8,
      amp_treble: 4.8,
      amp_presence: 5.0,
      amp_master: 3.5,
    },
  },
  {
    id: "07A",
    name: "Modern Crush",
    category: "amp",
    model: "recto",
    values: { amp_gain: 8.5, amp_bass: 6.0, amp_middle: 3.5, amp_treble: 5.5, amp_presence: 6.0, amp_master: 4.5 },
  },
  {
    id: "08B",
    name: "Electric Version",
    category: "amp",
    model: "plexi",
    values: { amp_gain: 7.5, amp_bass: 4.0, amp_middle: 6.2 },
  },
  {
    id: "08C",
    name: "JCM Stack",
    category: "amp",
    model: "jcm",
    values: { amp_gain: 7.0, amp_middle: 6.5, amp_treble: 5.5, amp_presence: 5.5 },
  },
  {
    id: "09A",
    name: "Funk Clean",
    category: "mod",
    model: "chorus",
    values: { chorus_mix: 60 },
  },
  {
    id: "10A",
    name: "Slate Solo",
    category: "amp",
    model: "slate",
    values: { amp_gain: 9.0, amp_bass: 5.5, amp_middle: 4.0, amp_treble: 6.0, amp_master: 4.0 },
  },
  {
    id: "11A",
    name: "Face Melt",
    category: "dist",
    model: "fuzz",
    values: { drive_gain: 9.0, drive_tone: 3.5, drive_level: 5.5 },
  },
  {
    id: "12A",
    name: "Bassman Room",
    category: "amp",
    model: "bassman",
    values: { amp_gain: 5.0, amp_bass: 7.0, amp_middle: 4.0, amp_treble: 5.0 },
  },
];

export const categories: Record<CategoryId, Category> = {
  dyn: {
    name: "Gate",
    short: "Gate",
    color: "var(--c-dyn)",
    rgb: "91, 124, 250",
    node: "gate",
  },
  dist: {
    name: "Distortion",
    short: "Dist",
    color: "var(--c-dist)",
    rgb: "232, 148, 42",
    node: "drive",
  },
  amp: {
    name: "Amp",
    short: "Amp",
    color: "var(--c-amp)",
    rgb: "232, 92, 92",
    node: "amp",
  },
  mod: {
    name: "Modulation",
    short: "Mod",
    color: "var(--c-mod)",
    rgb: "61, 184, 232",
    node: "mod",
  },
  delay: {
    name: "Delay",
    short: "Delay",
    color: "var(--c-delay)",
    rgb: "61, 214, 140",
    node: "delay",
  },
  verb: {
    name: "Reverb",
    short: "Verb",
    color: "var(--c-verb)",
    rgb: "168, 120, 240",
    node: "reverb",
  },
  cab: {
    name: "Cabinet",
    short: "Cab",
    color: "var(--c-cab)",
    rgb: "224, 112, 176",
    node: "cab",
  },
};

export const models: Record<CategoryId, Model[]> = {
  dyn: [
    {
      id: "gate",
      name: "Noise Gate",
      short: "Gate",
      sub: "Dynamic threshold noise reduction",
    },
  ],
  dist: [
    {
      id: "screamer",
      name: "Green Screamer",
      short: "Screamer",
      sub: "Tube drive mid-boost pedal",
    },
    {
      id: "minotaur",
      name: "Minotaur Boost",
      short: "Minotaur",
      sub: "Buffered analog clean boost",
    },
    {
      id: "rat",
      name: "Rats Nest",
      short: "Rat",
      sub: "Hard-clipping filthy distortion",
    },
    {
      id: "breaker",
      name: "Breaker Blues",
      short: "Breaker",
      sub: "Soft low-gain overdrive",
    },
    {
      id: "fuzz",
      name: "Face Fuzz",
      short: "Fuzz",
      sub: "Gated asymmetric fuzz",
    },
    {
      id: "centurion",
      name: "Centurion OD",
      short: "Centurion",
      sub: "Transparent mid-forward overdrive",
    },
  ],
  amp: [
    {
      id: "mandarin",
      name: "Mandarin 80",
      short: "Mandarin",
      sub: "1980 vintage British Orange tube head",
    },
    {
      id: "plexi",
      name: "Brit Plexi 100",
      short: "Plexi",
      sub: "Super Lead 1959 plexiglass Marshall",
    },
    {
      id: "twin",
      name: "Twin Clean",
      short: "Twin",
      sub: "High-headroom American clean combo",
    },
    {
      id: "topboost",
      name: "Top Boost",
      short: "TopBoost",
      sub: "Chiming British class-A combo",
    },
    {
      id: "recto",
      name: "Recto Modern",
      short: "Recto",
      sub: "Tight modern high-gain rectifier",
    },
    {
      id: "jcm",
      name: "JCM Crunch",
      short: "JCM",
      sub: "Classic British stack crunch",
    },
    {
      id: "slate",
      name: "Lead Slate",
      short: "Slate",
      sub: "Hot-rodded saturated lead amp",
    },
    {
      id: "bassman",
      name: "Bassman",
      short: "Bassman",
      sub: "Loose American bass-heavy head",
    },
    {
      id: "nam_capture",
      name: "NAM Capture",
      short: "NAM",
      sub: "Neural amp/cab capture (.nam)",
    },
    {
      id: "bypass",
      name: "Bypass",
      short: "Bypass",
      sub: "Pass the Tone/Amp slot through unprocessed",
    },
  ],
  mod: [
    {
      id: "chorus",
      name: "70s Analog Chorus",
      short: "Chorus",
      sub: "Warm analog modulated chorus",
    },
  ],
  delay: [
    { id: "tape", name: "Tape Echo", short: "Tape", sub: "Warm saturated tape delay" },
  ],
  verb: [
    {
      id: "plate",
      name: "Studio Plate",
      short: "Plate",
      sub: "Sustained metallic plate resonance",
    },
  ],
  cab: [
    {
      id: "vintage_cab",
      name: "1960v Vintage 4x12",
      short: "4x12",
      sub: "Celestion vintage cabinet sim",
    },
    {
      id: "american_2x12",
      name: "American 2x12",
      short: "2x12",
      sub: "Bright, tight open-back combo",
    },
    {
      id: "tweed_1x12",
      name: "Tweed 1x12",
      short: "Tweed",
      sub: "Small, boxy single-speaker combo",
    },
    {
      id: "modern_412",
      name: "Modern 4x12",
      short: "Modern",
      sub: "Tight, scooped, extended highs",
    },
  ],
};

export const parameterDefaults: Record<string, Param[]> = {
  gate: [
    {
      id: "gate_thresh",
      name: "Threshold",
      min: -80,
      max: 0,
      val: -55,
      unit: "dB",
    },
  ],
  screamer: [
    {
      id: "drive_gain",
      name: "Drive",
      min: 0,
      max: 10,
      val: 6.0,
      unit: "",
    },
    {
      id: "drive_tone",
      name: "Tone",
      min: 0,
      max: 10,
      val: 5.5,
      unit: "",
    },
    {
      id: "drive_level",
      name: "Level",
      min: 0,
      max: 10,
      val: 6.5,
      unit: "",
    },
  ],
  minotaur: [
    {
      id: "drive_gain",
      name: "Gain",
      min: 0,
      max: 10,
      val: 3.5,
      unit: "",
    },
    {
      id: "drive_tone",
      name: "Tone",
      min: 0,
      max: 10,
      val: 5.0,
      unit: "",
    },
    {
      id: "drive_level",
      name: "Output",
      min: 0,
      max: 10,
      val: 7.0,
      unit: "",
    },
  ],
  rat: [
    { id: "drive_gain", name: "Distortion", min: 0, max: 10, val: 7.5, unit: "" },
    { id: "drive_tone", name: "Filter", min: 0, max: 10, val: 4.5, unit: "" },
    { id: "drive_level", name: "Volume", min: 0, max: 10, val: 6.0, unit: "" },
  ],
  breaker: [
    { id: "drive_gain", name: "Drive", min: 0, max: 10, val: 4.5, unit: "" },
    { id: "drive_tone", name: "Tone", min: 0, max: 10, val: 5.5, unit: "" },
    { id: "drive_level", name: "Level", min: 0, max: 10, val: 6.5, unit: "" },
  ],
  fuzz: [
    { id: "drive_gain", name: "Fuzz", min: 0, max: 10, val: 8.5, unit: "" },
    { id: "drive_tone", name: "Tone", min: 0, max: 10, val: 3.5, unit: "" },
    { id: "drive_level", name: "Level", min: 0, max: 10, val: 5.5, unit: "" },
  ],
  centurion: [
    { id: "drive_gain", name: "Gain", min: 0, max: 10, val: 5.0, unit: "" },
    { id: "drive_tone", name: "Tone", min: 0, max: 10, val: 6.0, unit: "" },
    { id: "drive_level", name: "Output", min: 0, max: 10, val: 6.5, unit: "" },
  ],
  mandarin: [
    {
      id: "amp_gain",
      name: "Drive",
      min: 0,
      max: 10,
      val: 6.0,
      unit: "",
    },
    {
      id: "amp_bass",
      name: "Bass",
      min: 0,
      max: 10,
      val: 5.1,
      unit: "",
    },
    {
      id: "amp_middle",
      name: "Mid",
      min: 0,
      max: 10,
      val: 4.8,
      unit: "",
    },
    {
      id: "amp_treble",
      name: "Treble",
      min: 0,
      max: 10,
      val: 4.8,
      unit: "",
    },
    {
      id: "amp_presence",
      name: "Presence",
      min: 0,
      max: 10,
      val: 5.0,
      unit: "",
    },
    {
      id: "amp_master",
      name: "Master",
      min: 0,
      max: 10,
      val: 3.5,
      unit: "",
    },
  ],
  plexi: [
    {
      id: "amp_gain",
      name: "Pre Gain",
      min: 0,
      max: 10,
      val: 7.5,
      unit: "",
    },
    {
      id: "amp_bass",
      name: "Bass",
      min: 0,
      max: 10,
      val: 4.0,
      unit: "",
    },
    {
      id: "amp_middle",
      name: "Middle",
      min: 0,
      max: 10,
      val: 6.2,
      unit: "",
    },
    {
      id: "amp_treble",
      name: "Treble",
      min: 0,
      max: 10,
      val: 6.5,
      unit: "",
    },
    {
      id: "amp_presence",
      name: "Presence",
      min: 0,
      max: 10,
      val: 6.0,
      unit: "",
    },
    {
      id: "amp_master",
      name: "Master",
      min: 0,
      max: 10,
      val: 6.0,
      unit: "",
    },
  ],
  twin: [
    { id: "amp_gain", name: "Drive", min: 0, max: 10, val: 2.5, unit: "" },
    { id: "amp_bass", name: "Bass", min: 0, max: 10, val: 4.5, unit: "" },
    { id: "amp_middle", name: "Mid", min: 0, max: 10, val: 5.0, unit: "" },
    { id: "amp_treble", name: "Treble", min: 0, max: 10, val: 6.5, unit: "" },
    { id: "amp_presence", name: "Presence", min: 0, max: 10, val: 5.5, unit: "" },
    { id: "amp_master", name: "Master", min: 0, max: 10, val: 5.5, unit: "" },
  ],
  topboost: [
    { id: "amp_gain", name: "Drive", min: 0, max: 10, val: 4.0, unit: "" },
    { id: "amp_bass", name: "Bass", min: 0, max: 10, val: 4.0, unit: "" },
    { id: "amp_middle", name: "Mid", min: 0, max: 10, val: 3.5, unit: "" },
    { id: "amp_treble", name: "Treble", min: 0, max: 10, val: 7.0, unit: "" },
    { id: "amp_presence", name: "Presence", min: 0, max: 10, val: 6.5, unit: "" },
    { id: "amp_master", name: "Master", min: 0, max: 10, val: 5.0, unit: "" },
  ],
  recto: [
    { id: "amp_gain", name: "Drive", min: 0, max: 10, val: 8.5, unit: "" },
    { id: "amp_bass", name: "Bass", min: 0, max: 10, val: 6.0, unit: "" },
    { id: "amp_middle", name: "Mid", min: 0, max: 10, val: 3.5, unit: "" },
    { id: "amp_treble", name: "Treble", min: 0, max: 10, val: 5.5, unit: "" },
    { id: "amp_presence", name: "Presence", min: 0, max: 10, val: 6.0, unit: "" },
    { id: "amp_master", name: "Master", min: 0, max: 10, val: 4.5, unit: "" },
  ],
  jcm: [
    { id: "amp_gain", name: "Pre Gain", min: 0, max: 10, val: 7.0, unit: "" },
    { id: "amp_bass", name: "Bass", min: 0, max: 10, val: 5.0, unit: "" },
    { id: "amp_middle", name: "Middle", min: 0, max: 10, val: 6.5, unit: "" },
    { id: "amp_treble", name: "Treble", min: 0, max: 10, val: 5.5, unit: "" },
    { id: "amp_presence", name: "Presence", min: 0, max: 10, val: 5.5, unit: "" },
    { id: "amp_master", name: "Master", min: 0, max: 10, val: 5.0, unit: "" },
  ],
  slate: [
    { id: "amp_gain", name: "Drive", min: 0, max: 10, val: 9.0, unit: "" },
    { id: "amp_bass", name: "Bass", min: 0, max: 10, val: 5.5, unit: "" },
    { id: "amp_middle", name: "Mid", min: 0, max: 10, val: 4.0, unit: "" },
    { id: "amp_treble", name: "Treble", min: 0, max: 10, val: 6.0, unit: "" },
    { id: "amp_presence", name: "Presence", min: 0, max: 10, val: 6.5, unit: "" },
    { id: "amp_master", name: "Master", min: 0, max: 10, val: 4.0, unit: "" },
  ],
  bassman: [
    { id: "amp_gain", name: "Drive", min: 0, max: 10, val: 5.0, unit: "" },
    { id: "amp_bass", name: "Bass", min: 0, max: 10, val: 7.0, unit: "" },
    { id: "amp_middle", name: "Mid", min: 0, max: 10, val: 4.0, unit: "" },
    { id: "amp_treble", name: "Treble", min: 0, max: 10, val: 5.0, unit: "" },
    { id: "amp_presence", name: "Presence", min: 0, max: 10, val: 4.5, unit: "" },
    { id: "amp_master", name: "Master", min: 0, max: 10, val: 5.0, unit: "" },
  ],
  nam_capture: [
    { id: "nam_input_trim", name: "Input Trim", min: -24, max: 24, val: 0, unit: "dB" },
    { id: "nam_output_trim", name: "Output Trim", min: -24, max: 24, val: 0, unit: "dB" },
    { id: "nam_mix", name: "Mix", min: 0, max: 100, val: 100, unit: "%" },
  ],
  bypass: [],
  chorus: [
    {
      id: "chorus_rate",
      name: "Rate",
      min: 0,
      max: 10,
      val: 4.0,
      unit: "",
    },
    {
      id: "chorus_depth",
      name: "Depth",
      min: 0,
      max: 10,
      val: 5.5,
      unit: "",
    },
    {
      id: "chorus_mix",
      name: "Mix",
      min: 0,
      max: 100,
      val: 40,
      unit: "%",
    },
  ],
  tape: [
    {
      id: "delay_time",
      name: "Time",
      min: 40,
      max: 1200,
      val: 420,
      unit: "ms",
    },
    {
      id: "delay_fb",
      name: "Feedback",
      min: 0,
      max: 100,
      val: 35,
      unit: "%",
    },
    {
      id: "delay_mix",
      name: "Mix",
      min: 0,
      max: 100,
      val: 30,
      unit: "%",
    },
  ],
  plate: [
    {
      id: "reverb_decay",
      name: "Decay",
      min: 0.5,
      max: 15,
      val: 8.5,
      unit: "s",
    },
    {
      id: "reverb_mix",
      name: "Mix",
      min: 0,
      max: 100,
      val: 55,
      unit: "%",
    },
  ],
  vintage_cab: [
    { id: "cab_mic", name: "Mic Pos", min: 0, max: 100, val: 20, unit: "%" },
    { id: "cab_dist", name: "Distance", min: 0, max: 100, val: 40, unit: "%" },
  ],
  american_2x12: [
    { id: "cab_mic", name: "Mic Pos", min: 0, max: 100, val: 35, unit: "%" },
    { id: "cab_dist", name: "Distance", min: 0, max: 100, val: 30, unit: "%" },
  ],
  tweed_1x12: [
    { id: "cab_mic", name: "Mic Pos", min: 0, max: 100, val: 15, unit: "%" },
    { id: "cab_dist", name: "Distance", min: 0, max: 100, val: 55, unit: "%" },
  ],
  modern_412: [
    { id: "cab_mic", name: "Mic Pos", min: 0, max: 100, val: 45, unit: "%" },
    { id: "cab_dist", name: "Distance", min: 0, max: 100, val: 20, unit: "%" },
  ],
};

export const chainOrder: CategoryId[] = [
  "dyn",
  "dist",
  "amp",
  "mod",
  "delay",
  "verb",
  "cab",
];

/** Index used by DSP `path_slot_*` / `StageKind`. */
export const stageIndex: Record<CategoryId, number> = {
  dyn: 0,
  dist: 1,
  amp: 2,
  mod: 3,
  delay: 4,
  verb: 5,
  cab: 6,
};

export const stageByIndex: CategoryId[] = [
  "dyn",
  "dist",
  "amp",
  "mod",
  "delay",
  "verb",
  "cab",
];

/** Pack a path into 7 DSP slots (empty = -1). */
export function pathToSlotValues(path: CategoryId[]): number[] {
  const slots = Array.from({ length: 7 }, () => -1);
  path.forEach((cat, i) => {
    if (i < 7) slots[i] = stageIndex[cat];
  });
  return slots;
}

export function defaultPath(): CategoryId[] {
  return [...chainOrder];
}

export function emptyPath(): CategoryId[] {
  return [];
}

export function rackFromPath(path: CategoryId[]): CategoryId[] {
  const inPath = new Set(path);
  return chainOrder.filter((c) => !inPath.has(c));
}

export const icons: Record<string, string> = {
  gate: '<rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>',
  drive:
    '<polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>',
  amp: '<rect x="2" y="3" width="20" height="14" rx="2"/><line x1="2" y1="10" x2="22" y2="10"/><circle cx="6" cy="14" r="1"/><circle cx="10" cy="14" r="1"/>',
  mod: '<path d="M2 12s2-6 5-6 5 12 10 12 5-6 5-6"/>',
  delay: '<circle cx="12" cy="12" r="9"/><polyline points="12 7 12 12 16 14"/>',
  reverb: '<path d="M12 3v18M17 6v12M22 10v4M7 6v12M2 10v4"/>',
  cab: '<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/>',
};

export function fmt(val: number, unit: string): string {
  if (unit === "" || unit === "s") return `${val.toFixed(1)}${unit}`;
  return `${Math.round(val)}${unit}`;
}

/** Canonical default for a parameter within a given model, if defined. */
export function defaultValueFor(
  modelId: string,
  paramId: string,
): number | undefined {
  return parameterDefaults[modelId]?.find((p) => p.id === paramId)?.val;
}

export function cloneParameters(): Record<string, Param[]> {
  const out: Record<string, Param[]> = {};
  for (const [modelId, params] of Object.entries(parameterDefaults)) {
    out[modelId] = params.map((p) => ({ ...p }));
  }
  return out;
}

/**
 * Build the complete parameter state for a factory preset. Presets contain
 * only their intentional overrides, so never layer one over the currently
 * edited state: that would leak controls from the previously selected preset.
 */
export function parametersForPreset(preset: Preset): Record<string, Param[]> {
  const parameters = cloneParameters();
  const modelParams = parameters[preset.model];
  if (!modelParams) return parameters;

  parameters[preset.model] = modelParams.map((param) => {
    const value = preset.values[param.id];
    return value === undefined ? param : { ...param, val: value };
  });
  return parameters;
}
