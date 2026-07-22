import { describe, expect, test } from "bun:test";
import {
  INPUT_TRIM,
  OUTPUT_TRIM,
  SILENCE_DBFS,
  calibrationFor,
  distanceCm,
  formatDbfs,
  formatTrim,
  linearToDbfs,
  meterPosition,
  positionLabel,
} from "./globals";

describe("level conversion", () => {
  test("full scale is 0 dBFS", () => {
    expect(linearToDbfs(1)).toBeCloseTo(0, 5);
  });

  test("halving amplitude is about -6 dB", () => {
    expect(linearToDbfs(0.5)).toBeCloseTo(-6.02, 1);
  });

  test("silence and negative input floor rather than returning -Infinity", () => {
    expect(linearToDbfs(0)).toBe(SILENCE_DBFS);
    expect(Number.isFinite(linearToDbfs(0))).toBe(true);
  });

  test("levels below the floor are clamped to it", () => {
    expect(linearToDbfs(1e-9)).toBe(SILENCE_DBFS);
  });

  test("the floor renders as -inf rather than a misleading number", () => {
    expect(formatDbfs(SILENCE_DBFS)).toBe("-∞");
    expect(formatDbfs(-18.24)).toBe("-18.2");
  });
});

describe("meter position", () => {
  test("spans 0..1 across the visible range", () => {
    expect(meterPosition(0)).toBe(1);
    expect(meterPosition(SILENCE_DBFS)).toBe(0);
  });

  test("is clamped outside the visible range", () => {
    expect(meterPosition(12)).toBe(1);
    expect(meterPosition(-200)).toBe(0);
  });

  test("is monotonic, so a louder signal never draws shorter", () => {
    let prev = -1;
    for (let db = SILENCE_DBFS; db <= 0; db += 1) {
      const p = meterPosition(db);
      expect(p).toBeGreaterThanOrEqual(prev);
      prev = p;
    }
  });
});

describe("trim display", () => {
  test("is signed, because trims are bipolar", () => {
    expect(formatTrim(3)).toBe("+3.0");
    expect(formatTrim(-3)).toBe("-3.0");
  });

  test("does not render a signed zero", () => {
    expect(formatTrim(0)).toBe("0.0");
    expect(formatTrim(-0.01)).toBe("0.0");
  });

  test("trim ranges match the DSP's clamp of -24..24 dB", () => {
    for (const spec of [INPUT_TRIM, OUTPUT_TRIM]) {
      expect(spec.min).toBe(-24);
      expect(spec.max).toBe(24);
      expect(spec.val).toBe(0);
      expect(spec.unit).toBe("dB");
    }
  });
});

describe("NAM input calibration bands", () => {
  test("classifies levels into the documented bands", () => {
    expect(calibrationFor(-70, false)).toBe("silent");
    expect(calibrationFor(-30, false)).toBe("low");
    expect(calibrationFor(-18, false)).toBe("calibrated");
    expect(calibrationFor(-3, false)).toBe("hot");
    expect(calibrationFor(-0.05, false)).toBe("clipping");
  });

  test("a latched clip reports clipping regardless of the current level", () => {
    expect(calibrationFor(-40, true)).toBe("clipping");
  });

  test("band boundaries are contiguous with no gaps", () => {
    for (let db = SILENCE_DBFS; db <= 0; db += 0.5) {
      expect(calibrationFor(db, false)).toBeTruthy();
    }
  });
});

describe("cabinet mic display units", () => {
  test("distance maps the 0..100% param onto a 0-30 cm scale", () => {
    expect(distanceCm(0)).toBe(0);
    expect(distanceCm(100)).toBe(30);
    expect(distanceCm(50)).toBe(15);
  });

  test("position reads as centre, off-centre or edge", () => {
    expect(positionLabel(0)).toBe("Centre");
    expect(positionLabel(45)).toBe("Off-centre");
    expect(positionLabel(100)).toBe("Edge");
  });
});
