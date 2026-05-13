export const TRACK_COLORS = [
  "#56C7C9", // cyan / lead
  "#7EDB9A", // green / drums
  "#F2C96D", // amber / bass
  "#F27E77", // coral / vocal
  "#A99CFF", // violet / synth
  "#6EB7E8", // blue / keys
  "#E89B61", // orange / percussion
  "#D982B6", // rose / fx
  "#A8D36F", // lime / guitar
  "#9CAFE8", // periwinkle / pads
  "#C49A6C", // warm brown / acoustic
  "#71D6B5", // mint / bus
] as const;

export function getTrackColor(index: number): string {
  return TRACK_COLORS[index % TRACK_COLORS.length];
}

export const TRACK_HEIGHT = 76;
export const HEADER_WIDTH = 272;
export const RULER_HEIGHT = 30;
export const BROWSER_WIDTH = 272;
export const INSPECTOR_WIDTH = 292;
export const MIXER_HEIGHT = 240;

export const C = {
  // Core surfaces
  bg: "#111419",
  sunken: "#0B0D11",
  surface: "#181C23",
  surfaceHigh: "#202632",
  surfaceHover: "#252C38",
  surfaceActive: "#2B3442",

  // Borders
  border: "#2B3440",
  borderSoft: "rgba(255,255,255,0.055)",
  borderHard: "#43505F",

  // Text
  faint: "#566372",
  dim: "#8A96A6",
  text: "#EEF3F8",
  textSoft: "#C9D2DC",

  // Main identity
  accent: "#56C7C9",
  accentSoft: "rgba(86,199,201,0.16)",
  accentHard: "#7EE4E6",

  // Status
  green: "#7EDB9A",
  red: "#F27E77",
  yellow: "#F2C96D",
  orange: "#E89B61",
  violet: "#A99CFF",
  blue: "#6EB7E8",

  // Timeline
  gridMinor: "rgba(255,255,255,0.035)",
  gridMajor: "rgba(255,255,255,0.075)",
  playhead: "#56C7C9",
  selection: "rgba(86,199,201,0.18)",

  // Clips / waveform
  clipBg: "rgba(86,199,201,0.16)",
  clipBorder: "rgba(86,199,201,0.42)",
  waveform: "rgba(226,236,246,0.72)",

  // Mixer
  meterGreen: "#7EDB9A",
  meterYellow: "#F2C96D",
  meterRed: "#F27E77",
} as const;
