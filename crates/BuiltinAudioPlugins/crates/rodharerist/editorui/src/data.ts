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
};

export type Model = {
  id: string;
  name: string;
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
    id: "08B",
    name: "Electric Version",
    category: "amp",
    model: "plexi",
    values: { amp_gain: 7.5, amp_bass: 4.0, amp_middle: 6.2 },
  },
  {
    id: "09A",
    name: "Funk Clean",
    category: "mod",
    model: "chorus",
    values: { chorus_mix: 60 },
  },
];

export const categories: Record<CategoryId, Category> = {
  dyn: {
    name: "Gate",
    short: "Gate",
    color: "var(--c-dyn)",
    rgb: "99, 102, 241",
    node: "gate",
  },
  dist: {
    name: "Distortion",
    short: "Dist",
    color: "var(--c-dist)",
    rgb: "245, 158, 11",
    node: "drive",
  },
  amp: {
    name: "Amp",
    short: "Amp",
    color: "var(--c-amp)",
    rgb: "244, 63, 94",
    node: "amp",
  },
  mod: {
    name: "Modulation",
    short: "Mod",
    color: "var(--c-mod)",
    rgb: "59, 130, 246",
    node: "mod",
  },
  delay: {
    name: "Delay",
    short: "Delay",
    color: "var(--c-delay)",
    rgb: "16, 185, 129",
    node: "delay",
  },
  verb: {
    name: "Reverb",
    short: "Verb",
    color: "var(--c-verb)",
    rgb: "139, 92, 246",
    node: "reverb",
  },
  cab: {
    name: "Cabinet",
    short: "Cab",
    color: "var(--c-cab)",
    rgb: "236, 72, 153",
    node: "cab",
  },
};

export const models: Record<CategoryId, Model[]> = {
  dyn: [
    {
      id: "gate",
      name: "Noise Gate",
      sub: "Dynamic threshold noise reduction",
    },
  ],
  dist: [
    {
      id: "screamer",
      name: "Green Screamer",
      sub: "Tube drive mid-boost pedal",
    },
    {
      id: "minotaur",
      name: "Minotaur Boost",
      sub: "Buffered analog clean boost",
    },
  ],
  amp: [
    {
      id: "mandarin",
      name: "Mandarin 80",
      sub: "1980 vintage British Orange tube head",
    },
    {
      id: "plexi",
      name: "Brit Plexi 100",
      sub: "Super Lead 1959 plexiglass Marshall",
    },
  ],
  mod: [
    {
      id: "chorus",
      name: "70s Analog Chorus",
      sub: "Warm analog modulated chorus",
    },
  ],
  delay: [
    { id: "tape", name: "Tape Echo", sub: "Warm saturated tape delay" },
  ],
  verb: [
    {
      id: "plate",
      name: "Studio Plate",
      sub: "Sustained metallic plate resonance",
    },
  ],
  cab: [
    {
      id: "vintage_cab",
      name: "1960v Vintage 4x12",
      sub: "Celestion vintage cabinet sim",
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
      id: "drive_level",
      name: "Output",
      min: 0,
      max: 10,
      val: 7.0,
      unit: "",
    },
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
      id: "amp_master",
      name: "Master",
      min: 0,
      max: 10,
      val: 6.0,
      unit: "",
    },
  ],
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
    {
      id: "cab_mic",
      name: "Mic Pos",
      min: 0,
      max: 100,
      val: 20,
      unit: "%",
    },
    {
      id: "cab_dist",
      name: "Distance",
      min: 0,
      max: 100,
      val: 40,
      unit: "%",
    },
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

export const KNOB = {
  C: 2 * Math.PI * 45,
  arc: 2 * Math.PI * 45 * 0.75,
};

export function fmt(val: number, unit: string): string {
  if (unit === "" || unit === "s") return `${val.toFixed(1)}${unit}`;
  return `${Math.round(val)}${unit}`;
}

export function cloneParameters(): Record<string, Param[]> {
  const out: Record<string, Param[]> = {};
  for (const [modelId, params] of Object.entries(parameterDefaults)) {
    out[modelId] = params.map((p) => ({ ...p }));
  }
  return out;
}
