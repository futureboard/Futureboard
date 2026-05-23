import { resampleLinear } from "./resample";
import { fft, hannWindow, principalArg } from "./fft";

type F32 = Float32Array<ArrayBufferLike>;
type Quality = "draft" | "balanced" | "high";

const FFT_SIZE: Record<Quality, number> = {
  draft: 1024,
  balanced: 2048,
  high: 4096,
};

export function pitchShiftPhaseVocoder(
  channels: F32[],
  semitones: number,
  quality: Quality = "balanced",
): Float32Array[] {
  const clamped = Math.max(-24, Math.min(24, semitones));
  if (clamped === 0 || channels.length === 0) {
    return channels.map((ch) => new Float32Array(ch));
  }

  const pitchRatio = Math.pow(2, clamped / 12);
  const originalLength = channels[0].length;
  const fftSize = FFT_SIZE[quality] ?? FFT_SIZE.balanced;

  return channels.map((ch) => {
    const stretched = phaseVocoderStretchMono(ch, pitchRatio, fftSize);
    const shifted = resampleLinear(stretched, pitchRatio);
    return fitLength(shifted, originalLength);
  });
}

function phaseVocoderStretchMono(input: F32, stretchRatio: number, fftSize: number): Float32Array {
  const ratio = Math.max(0.25, Math.min(4.0, stretchRatio));
  if (input.length === 0) return new Float32Array(0);
  if (input.length < fftSize) return resampleLinear(input, 1 / ratio);

  const hopAnalysis = fftSize >> 2;
  const hopSynthesis = Math.max(1, Math.round(hopAnalysis * ratio));
  const outLen = Math.max(1, Math.ceil(input.length * ratio));
  const window = hannWindow(fftSize);
  const output = new Float32Array(outLen + fftSize);
  const windowSum = new Float32Array(outLen + fftSize);

  const real = new Float32Array(fftSize);
  const imag = new Float32Array(fftSize);
  const prevPhase = new Float32Array(fftSize / 2 + 1);
  const sumPhase = new Float32Array(fftSize / 2 + 1);
  const omega = new Float32Array(fftSize / 2 + 1);

  for (let k = 0; k < omega.length; k++) {
    omega[k] = (2 * Math.PI * k) / fftSize;
  }

  let frame = 0;
  for (let inPos = 0, outPos = 0; inPos + fftSize <= input.length; inPos += hopAnalysis, outPos += hopSynthesis) {
    real.fill(0);
    imag.fill(0);

    for (let i = 0; i < fftSize; i++) {
      real[i] = input[inPos + i] * window[i];
    }

    fft(real, imag, false);

    for (let k = 0; k <= fftSize / 2; k++) {
      const r = real[k];
      const im = imag[k];
      const mag = Math.hypot(r, im);
      const phase = Math.atan2(im, r);

      if (frame === 0) {
        sumPhase[k] = phase;
      } else {
        let delta = phase - prevPhase[k] - omega[k] * hopAnalysis;
        delta = principalArg(delta);
        const trueFreq = omega[k] + delta / hopAnalysis;
        sumPhase[k] += trueFreq * hopSynthesis;
      }

      prevPhase[k] = phase;
      real[k] = mag * Math.cos(sumPhase[k]);
      imag[k] = mag * Math.sin(sumPhase[k]);

      if (k > 0 && k < fftSize / 2) {
        real[fftSize - k] = real[k];
        imag[fftSize - k] = -imag[k];
      }
    }

    fft(real, imag, true);

    const copyLen = Math.min(fftSize, output.length - outPos);
    for (let i = 0; i < copyLen; i++) {
      const w = window[i];
      output[outPos + i] += real[i] * w;
      windowSum[outPos + i] += w * w;
    }

    frame++;
  }

  const trimmed = new Float32Array(outLen);
  for (let i = 0; i < outLen; i++) {
    const w = windowSum[i];
    if (w > 1e-7) {
      trimmed[i] = output[i] / w;
    } else {
      const srcPos = Math.min(input.length - 1, Math.floor(i / ratio));
      trimmed[i] = input[srcPos];
    }
  }
  return trimmed;
}

function fitLength(input: Float32Array, length: number): Float32Array {
  if (input.length === length) return input;
  const output = new Float32Array(length);
  output.set(input.subarray(0, Math.min(input.length, length)));
  if (input.length > 0 && input.length < length) {
    output.fill(input[input.length - 1], input.length);
  }
  return output;
}
