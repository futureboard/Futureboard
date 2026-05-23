import { create } from "zustand";

type TransportStore = {
  isPlaying: boolean;
  playheadTime: number;
  setIsPlaying: (v: boolean) => void;
  setPlayheadTime: (t: number) => void;
  isRecording: boolean;
  recordStartBeat: number;
  setIsRecording: (v: boolean, startBeat?: number) => void;
};

export const useTransportStore = create<TransportStore>((set) => ({
  isPlaying: false,
  playheadTime: 0,
  setIsPlaying: (isPlaying) => set({ isPlaying }),
  setPlayheadTime: (playheadTime) => set({ playheadTime }),
  isRecording: false,
  recordStartBeat: 0,
  setIsRecording: (isRecording, startBeat = 0) => set({ isRecording, recordStartBeat: startBeat }),
}));
