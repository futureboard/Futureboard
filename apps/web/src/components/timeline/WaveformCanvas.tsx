import { useEffect, useRef } from "react";
import type { WaveformPeaks } from "../../types/daw";

type Props = {
  peaks: WaveformPeaks;
  width: number;
  height: number;
  color?: string;
};

export function WaveformCanvas({ peaks, width, height, color = "rgba(255,255,255,0.7)" }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || width < 1 || height < 1) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    canvas.width = Math.ceil(width);
    canvas.height = Math.ceil(height);
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    if (peaks.peaks.length === 0) return;

    const peakCount = peaks.peaks.length / (peaks.channelCount * 2);
    const mid = canvas.height / 2;
    const pxPerPeak = canvas.width / peakCount;

    ctx.fillStyle = color;
    for (let i = 0; i < peakCount; i++) {
      const base = i * peaks.channelCount * 2;
      const min = peaks.peaks[base];
      const max = peaks.peaks[base + 1];
      const y1 = mid - max * mid;
      const y2 = mid - min * mid;
      ctx.fillRect(Math.floor(i * pxPerPeak), Math.floor(y1), Math.max(1, Math.ceil(pxPerPeak)), Math.max(1, Math.ceil(y2 - y1)));
    }
  }, [peaks, width, height, color]);

  return <canvas ref={canvasRef} className="block" style={{ width, height, imageRendering: "pixelated" }} />;
}
