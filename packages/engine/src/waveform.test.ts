import { describe, expect, it } from "vitest";
import { computeWaveform } from "./waveform";

describe("computeWaveform", () => {
  it("puts a bass tone in the low band and a hiss in the high band", () => {
    const sr = 44100;
    const bass = Float32Array.from({ length: sr }, (_, i) => Math.sin((2 * Math.PI * 60 * i) / sr));
    const hiss = Float32Array.from({ length: sr }, (_, i) => (i % 2 ? 0.5 : -0.5));
    const w = computeWaveform([Float32Array.from([...bass, ...hiss])], sr);
    expect(w.peak.length).toBe(200);
    const mean = (a: Float32Array, s: number, e: number) => a.slice(s, e).reduce((x, y) => x + y, 0) / (e - s);
    expect(mean(w.low, 20, 90)).toBeGreaterThan(mean(w.low, 120, 190) * 4);
    expect(mean(w.high, 120, 190)).toBeGreaterThan(mean(w.high, 20, 90) * 4);
  });
});
