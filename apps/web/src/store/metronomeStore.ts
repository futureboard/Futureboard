import { create } from "zustand";

export type MetronomeSound = "classic" | "wood" | "digital" | "soft";
export type MetronomeSubdivision = "quarter" | "eighth" | "sixteenth";

export type MetronomeState = {
  enabled: boolean;
  countInEnabled: boolean;
  countInBars: number;
  volume: number;
  accentVolume: number;
  sound: MetronomeSound;
  subdivision: MetronomeSubdivision;

  toggle: () => void;
  setVolume: (v: number) => void;
  toggleCountIn: () => void;
  setCountInBars: (bars: number) => void;
  setSound: (sound: MetronomeSound) => void;
  setSubdivision: (sub: MetronomeSubdivision) => void;
};

export const useMetronomeStore = create<MetronomeState>((set) => ({
  enabled: false,
  countInEnabled: false,
  countInBars: 1,
  volume: 0.8,
  accentVolume: 1.0,
  sound: "digital",
  subdivision: "quarter",

  toggle: () => set((state) => ({ enabled: !state.enabled })),
  setVolume: (volume) => set({ volume }),
  toggleCountIn: () => set((state) => ({ countInEnabled: !state.countInEnabled })),
  setCountInBars: (countInBars) => set({ countInBars }),
  setSound: (sound) => set({ sound }),
  setSubdivision: (subdivision) => set({ subdivision }),
}));
