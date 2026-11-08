import { describe, expect, it } from "vitest";
import { FLOOR_DB, meterPos, newMeter, stepMeter } from "./meter";

describe("meter", () => {
  it("measures a full-scale sine near 0 dBFS peak and -3 dB RMS", () => {
    const sine = Float32Array.from({ length: 4800 }, (_, i) => Math.sin((2 * Math.PI * 440 * i) / 48000));
    const m = stepMeter(newMeter(), sine, 0, 16);
    expect(m.peakDb).toBeGreaterThan(-0.1);
    expect(m.rmsDb).toBeCloseTo(-3.01, 1);
    expect(m.clipped).toBe(true);
  });

  it("falls at ~20 dB/s after the signal stops", () => {
    let m = stepMeter(newMeter(), new Float32Array([0.5]), 0, 16);
    const start = m.peakDb;
    m = stepMeter(m, new Float32Array(16), 500, 500);
    expect(start - m.peakDb).toBeCloseTo(10, 0);
  });

  it("maps the scale monotonically from floor to 0 dB", () => {
    expect(meterPos(FLOOR_DB)).toBe(0);
    expect(meterPos(0)).toBe(1);
    expect(meterPos(-6)).toBeGreaterThan(meterPos(-12));
  });
});
