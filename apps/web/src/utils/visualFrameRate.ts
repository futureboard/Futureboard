import { useSettingsStore, type VisualFrameRate } from "../store/settingsStore";

export function visualFrameRateToIntervalMs(rate: VisualFrameRate): number {
  return rate === "unlimited" ? 0 : 1000 / rate;
}

export function getVisualFrameIntervalMs(): number {
  return visualFrameRateToIntervalMs(useSettingsStore.getState().visualFrameRate);
}

export function shouldRunVisualFrame(lastFrameAt: number, now: number = performance.now()): boolean {
  const interval = getVisualFrameIntervalMs();
  return interval <= 0 || now - lastFrameAt >= interval;
}

export function visualFrameRateLabel(rate: VisualFrameRate): string {
  return rate === "unlimited" ? "Unlimited" : `${rate} FPS`;
}
