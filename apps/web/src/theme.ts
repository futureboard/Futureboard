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
export const HEADER_WIDTH = 320;
export const RULER_HEIGHT = 30;
export const BROWSER_WIDTH = 272;
export const INSPECTOR_WIDTH = 292;
export const MIXER_HEIGHT = 240;

export const C = {
  // Core surfaces - lighter dark, still professional
  bg: "#171B22",
  sunken: "#11151B",
  surface: "#202631",
  surfaceHigh: "#2A3240",
  surfaceHover: "#313A49",
  surfaceActive: "#394456",

  // Borders
  border: "#3A4554",
  borderSoft: "rgba(255,255,255,0.075)",
  borderHard: "#536173",

  // Text
  faint: "#6B7888",
  dim: "#9AA7B8",
  text: "#F1F5F9",
  textSoft: "#D2DBE6",

  // Main identity
  accent: "#5FCED0",
  accentSoft: "rgba(95,206,208,0.18)",
  accentHard: "#8AE9EB",

  // Status
  green: "#85E0A3",
  red: "#F4877F",
  yellow: "#F4CF7A",
  orange: "#EFA66D",
  violet: "#B7ABFF",
  blue: "#7BC4F0",

  // Timeline
  gridMinor: "rgba(255,255,255,0.045)",
  gridMajor: "rgba(255,255,255,0.095)",
  playhead: "#5FCED0",
  selection: "rgba(95,206,208,0.20)",

  // Clips / waveform
  clipBg: "rgba(95,206,208,0.18)",
  clipBorder: "rgba(95,206,208,0.48)",
  waveform: "rgba(234,242,250,0.76)",

  // Mixer
  meterGreen: "#85E0A3",
  meterYellow: "#F4CF7A",
  meterRed: "#F4877F",
} as const;