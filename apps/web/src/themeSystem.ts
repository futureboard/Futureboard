import defaultTheme from "../../../packages/shared/themes/Default.json";

export type ThemeAppearance = "dark" | "light";

export type FutureboardTheme = {
  schemaVersion: number;
  id: string;
  name: string;
  version?: string;
  author?: string;
  appearance: ThemeAppearance;
  targets: Array<"native-gpui" | "web-ui" | string>;
  tokens: Record<string, unknown>;
  trackColors?: string[];
  web?: {
    cssVariables?: Record<string, string>;
  };
};

export const DEFAULT_THEME = defaultTheme as FutureboardTheme;

let activeTheme: FutureboardTheme = DEFAULT_THEME;

export function getActiveTheme(): FutureboardTheme {
  return activeTheme;
}

export function getThemeToken(theme: FutureboardTheme, path: string): string | undefined {
  let current: unknown = theme.tokens;
  for (const part of path.split(".")) {
    if (!current || typeof current !== "object" || !(part in current)) {
      return undefined;
    }
    current = (current as Record<string, unknown>)[part];
  }
  return typeof current === "string" ? current : undefined;
}

export function applyTheme(theme: FutureboardTheme, root: HTMLElement = document.documentElement) {
  activeTheme = theme;
  root.dataset.theme = theme.id;
  root.dataset.themeAppearance = theme.appearance;

  const cssVariables = theme.web?.cssVariables ?? DEFAULT_THEME.web?.cssVariables ?? {};
  for (const [variableName, tokenPath] of Object.entries(cssVariables)) {
    const value = getThemeToken(theme, tokenPath) ?? getThemeToken(DEFAULT_THEME, tokenPath);
    if (value) {
      root.style.setProperty(variableName, value);
    }
  }

  theme.trackColors?.forEach((color, index) => {
    root.style.setProperty(`--track-color-${index + 1}`, color);
  });
}

export function installDefaultTheme() {
  applyTheme(DEFAULT_THEME);
}
