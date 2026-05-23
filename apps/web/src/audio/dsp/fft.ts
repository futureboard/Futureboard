export function fft(real: Float32Array, imag: Float32Array, inverse = false): void {
  const n = real.length;
  if (n !== imag.length || n === 0 || (n & (n - 1)) !== 0) {
    throw new Error("fft expects non-empty power-of-two real/imag buffers");
  }

  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      const tr = real[i]; real[i] = real[j]; real[j] = tr;
      const ti = imag[i]; imag[i] = imag[j]; imag[j] = ti;
    }
  }

  for (let len = 2; len <= n; len <<= 1) {
    const angle = (inverse ? 2 : -2) * Math.PI / len;
    const wLenR = Math.cos(angle);
    const wLenI = Math.sin(angle);

    for (let i = 0; i < n; i += len) {
      let wr = 1;
      let wi = 0;
      const half = len >> 1;

      for (let j = 0; j < half; j++) {
        const even = i + j;
        const odd = even + half;
        const uR = real[even];
        const uI = imag[even];
        const vR = real[odd] * wr - imag[odd] * wi;
        const vI = real[odd] * wi + imag[odd] * wr;

        real[even] = uR + vR;
        imag[even] = uI + vI;
        real[odd] = uR - vR;
        imag[odd] = uI - vI;

        const nextWr = wr * wLenR - wi * wLenI;
        wi = wr * wLenI + wi * wLenR;
        wr = nextWr;
      }
    }
  }

  if (inverse) {
    const inv = 1 / n;
    for (let i = 0; i < n; i++) {
      real[i] *= inv;
      imag[i] *= inv;
    }
  }
}

export function principalArg(phase: number): number {
  return phase - 2 * Math.PI * Math.round(phase / (2 * Math.PI));
}

export function hannWindow(size: number): Float32Array {
  const win = new Float32Array(size);
  const n1 = size - 1;
  for (let i = 0; i < size; i++) {
    win[i] = 0.5 * (1 - Math.cos((2 * Math.PI * i) / n1));
  }
  return win;
}
