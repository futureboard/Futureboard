import { DEFAULT_THEME, getThemeToken } from "./themeSystem";

function token(path: string, fallback: string): string {
  return getThemeToken(DEFAULT_THEME, path) ?? fallback;
}

export const TRACK_COLORS = (DEFAULT_THEME.trackColors ?? [
  "#56C7C9",
  "#7EDB9A",
  "#F2C96D",
  "#F27E77",
  "#A99CFF",
  "#6EB7E8",
  "#E89B61",
  "#D982B6",
  "#A8D36F",
  "#9CAFE8",
  "#C49A6C",
  "#71D6B5",
]) as readonly string[];

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
    base: token("surface.base", "#1E1F22"),
    sunken: token("surface.canvas", "#15161A"),
    panel: token("surface.panel", "#25262B"),
    raised: token("surface.raised", "#2B2D33"),
    hover: token("surface.hover", "#30323A"),
    active: token("surface.active", "#2B2D33"),
    overlay: token("surface.overlay", "#00000085"),
    subtle: token("surface.mixerStripAlt", "#FFFFFF05"),
    selected: token("surface.mixerStripSelected", "#FFFFFF14"),
  },
  border: {
    subtle: token("border.subtle", "#FFFFFF14"),
    muted: token("border.default", "#FFFFFF1F"),
    strong: token("border.strong", "#4C505C"),
    focus: token("border.focus", "#7B61FFB8"),
  },
  text: {
    primary: token("text.primary", "#DFE1E5"),
    secondary: token("text.secondary", "#C3C7D0"),
    muted: token("text.muted", "#8E96A3"),
    faint: token("text.faint", "#FFFFFF45"),
    disabled: token("text.disabled", "#FFFFFF3B"),
  },
  accent: {
    primary: token("accent.primary", "#7B61FF"),
    hover: token("accent.primaryHover", "#8D78FF"),
    soft: token("accent.soft", "#7B61FF30"),
    border: token("border.accent", "#7B61FF80"),
  },
  status: {
    success: token("status.success", "#6FCF97"),
    warning: token("status.warning", "#E5C07B"),
    error: token("status.error", "#FF6B68"),
    info: token("track.audio", "#5FCED0"),
  },
} as const;

export const C = {
  bg: semanticColors.surface.base,
  sunken: semanticColors.surface.sunken,
  surface: semanticColors.surface.panel,
  surfaceHigh: semanticColors.surface.raised,
  surfaceHover: semanticColors.surface.hover,
  surfaceActive: semanticColors.surface.active,

  border: semanticColors.border.strong,
  borderSoft: semanticColors.border.subtle,
  borderHard: semanticColors.border.strong,

  faint: semanticColors.text.faint,
  dim: semanticColors.text.muted,
  text: semanticColors.text.primary,
  textSoft: semanticColors.text.secondary,

  accent: semanticColors.accent.primary,
  accentSoft: semanticColors.accent.soft,
  accentHard: semanticColors.accent.hover,

  green: semanticColors.status.success,
  red: semanticColors.status.error,
  yellow: semanticColors.status.warning,
  orange: "#EFA66D",
  violet: token("accent.purple", "#BB86FC"),
  blue: semanticColors.status.info,

  gridMinor: token("timeline.gridMinor", "#FFFFFF08"),
  gridMajor: token("timeline.gridMajor", "#FFFFFF12"),
  playhead: token("timeline.playhead", "#FF6B68"),
  selection: token("timeline.selection", "#7B61FF30"),

  clipBg: token("accent.soft", "#7B61FF30"),
  clipBorder: token("border.accent", "#7B61FF80"),
  waveform: token("text.secondary", "#C3C7D0"),

  meterGreen: token("meter.low", "#6FCF97"),
  meterYellow: token("meter.mid", "#E5C07B"),
  meterRed: token("meter.high", "#FF6B68"),
} as const;
