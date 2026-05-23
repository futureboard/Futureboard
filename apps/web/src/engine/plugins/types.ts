import type { InsertDevice } from "../../types/daw";

export type InsertUpdateContext = {
  now: number;
  sampleRate: number;
  bpm: number;
};

export type InsertAudioNode = {
  readonly id: string;
  readonly input: AudioNode;
  readonly output: AudioNode;
  update(params: Record<string, number | string | boolean>, ctx: InsertUpdateContext): void;
  setEnabled(enabled: boolean, now: number): void;
  dispose(): void;
};

export type InsertNodeFactory = (
  audioCtx: AudioContext,
  device: InsertDevice,
  ctx: InsertUpdateContext
) => InsertAudioNode;
