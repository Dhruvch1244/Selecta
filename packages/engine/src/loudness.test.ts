import { describe, expect, it } from "vitest";
import { measureLoudness, normGain, TARGET_LUFS } from "./loudness";

const buf = (chs: Float32Array[], rate: number) => ({ numberOfChannels: chs.length, sampleRate: rate, getChannelData: (c: number) => chs[c]! });
const sine = (amp: number, rate: number, secs: number) => Float32Array.from({ length: rate * secs }, (_, i) => amp * Math.sin((2 * Math.PI * 997 * i) / rate));

describe("loudness", () => {
  it("reads a -20 dBFS 997 Hz sine on both channels as -20 LUFS", () => {
    const s = sine(0.1, 48000, 10);
    expect(measureLoudness(buf([s, s], 48000)).lufs).toBeCloseTo(-20, 0.5);
    // Mono plays on both speakers: same reading.
    expect(measureLoudness(buf([s], 48000)).lufs).toBeCloseTo(-20, 0.5);
  });
  it("measures a stem buffer as the full mix", () => {
    const s = sine(0.1, 48000, 10);
    const half = s.map((v) => v / 2);
    expect(measureLoudness(buf([half, half, half, half], 48000)).lufs).toBeCloseTo(-20, 0.5);
  });
  it("ignores quiet intros: the loud part sets the level", () => {
    const rate = 24000;
    const quiet = sine(0.01, rate, 30), loud = sine(0.1, rate, 30);
    const s = new Float32Array(quiet.length + loud.length);
    s.set(quiet);
    s.set(loud, quiet.length);
    expect(measureLoudness(buf([s, s], rate)).lufs).toBeCloseTo(-20, 0.5);
  });
  it("cuts hot masters freely and boosts only up to the peak ceiling", () => {
    expect(normGain(-5, 2.5)).toBeCloseTo(TARGET_LUFS + 5);
    expect(normGain(-16, 0)).toBeCloseTo(3);
    expect(normGain(-14, -10)).toBeCloseTo(4);
  });
});
