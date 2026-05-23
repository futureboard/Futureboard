export function dbToGain(db: number): number {
  return Math.pow(10, db / 20);
}

export function gainToDb(gain: number): number {
  return 20 * Math.log10(Math.max(1e-10, gain));
}

export function smoothParam(param: AudioParam, value: number, now: number, timeConst = 0.015): void {
  param.setTargetAtTime(value, now, timeConst);
}

export function rampParam(param: AudioParam, value: number, now: number, rampTime = 0.02): void {
  param.linearRampToValueAtTime(value, now + rampTime);
}

export function equalPowerMix(mixPercent: number): { dry: number; wet: number } {
  const t = clamp(mixPercent / 100, 0, 1) * (Math.PI / 2);
  return { dry: Math.cos(t), wet: Math.sin(t) };
}

export function clamp(v: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, v));
}

export function msToSeconds(ms: number): number {
  return ms / 1000;
}
