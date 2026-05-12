export const TRACK_COLORS = [
  "#5aa7ff",
  "#63c174",
  "#e0b24d",
  "#f06a61",
  "#b995ff",
  "#55c9d7",
  "#ed8f4c",
  "#ee86b7",
  "#90c95f",
  "#8ea8e8",
] as const;

export function getTrackColor(index: number): string {
  return TRACK_COLORS[index % TRACK_COLORS.length];
}

export const TRACK_HEIGHT   = 76;
export const HEADER_WIDTH   = 212;
export const RULER_HEIGHT   = 32;
export const BROWSER_WIDTH  = 212;
export const INSPECTOR_WIDTH = 220;
export const MIXER_HEIGHT   = 180;

export const C = {
  bg:          "#111418",
  sunken:      "#0c0f12",
  surface:     "#171b20",
  surfaceHigh: "#20262d",
  border:      "#303943",
  borderHard:  "#3d4854",
  dim:         "#5d6874",
  text:        "#d5dde6",
  accent:      "#5aa7ff",
  green:       "#63c174",
  red:         "#f06a61",
  yellow:      "#e0b24d",
} as const;
