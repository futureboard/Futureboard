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

export const semanticColors = {
  surface: {
    base: "#171B22",
    sunken: "#11151B",
    panel: "#202631",
    raised: "#2A3240",
    hover: "#313A49",
    active: "#394456",
    overlay: "rgba(0,0,0,0.22)",
    subtle: "rgba(255,255,255,0.03)",
    selected: "rgba(255,255,255,0.038)",
  },
  border: {
    subtle: "rgba(255,255,255,0.075)",
    muted: "rgba(255,255,255,0.055)",
    strong: "#536173",
    focus: "rgba(95,206,208,0.55)",
  },
  text: {
    primary: "#F1F5F9",
    secondary: "#D2DBE6",
    muted: "#9AA7B8",
    faint: "#6B7888",
    disabled: "rgba(255,255,255,0.3)",
  },
  accent: {
    primary: "#5FCED0",
    hover: "#8AE9EB",
    soft: "rgba(95,206,208,0.18)",
    border: "rgba(95,206,208,0.48)",
  },
  status: {
    success: "#85E0A3",
    warning: "#F4CF7A",
    error: "#F4877F",
    info: "#7BC4F0",
  },
} as const;

export const C = {
  // Core surfaces - lighter dark, still professional
  bg: semanticColors.surface.base,
  sunken: semanticColors.surface.sunken,
  surface: semanticColors.surface.panel,
  surfaceHigh: semanticColors.surface.raised,
  surfaceHover: semanticColors.surface.hover,
  surfaceActive: semanticColors.surface.active,

  // Borders
  border: "#3A4554",
  borderSoft: semanticColors.border.subtle,
  borderHard: semanticColors.border.strong,

  // Text
  faint: semanticColors.text.faint,
  dim: semanticColors.text.muted,
  text: semanticColors.text.primary,
  textSoft: semanticColors.text.secondary,

  // Main identity
  accent: semanticColors.accent.primary,
  accentSoft: semanticColors.accent.soft,
  accentHard: semanticColors.accent.hover,

  // Status
  green: semanticColors.status.success,
  red: semanticColors.status.error,
  yellow: semanticColors.status.warning,
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
