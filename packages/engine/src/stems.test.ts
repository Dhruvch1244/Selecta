import { describe, expect, it } from "vitest";
import { alignLag, buildStemChannels } from "./stems";

/** Deterministic noise-ish "vocal" and "beat" (no Math.random: repeatable). */
function sig(n: number, seed: number, f: number): Float32Array {
  const out = new Float32Array(n);
  let s = seed;
  for (let i = 0; i < n; i++) {
    s = (Math.imul(s, 1103515245) + 12345) & 0x7fffffff;
    out[i] = 0.3 * Math.sin((i * f) / 1000) + 0.2 * (s / 0x7fffffff - 0.5);
  }
  return out;
}

describe("stem alignment", () => {
  it("finds the encoder delay between the original and the stem", () => {
    const n = 48000 * 4;
    const voc = sig(n, 7, 31);
    const beat = sig(n, 99, 3);
    for (const lag of [0, 1024, 2112, -37]) {
      const full = new Float32Array(n);
      for (let i = 0; i < n; i++) full[i] = (beat[i] ?? 0) + (voc[i - lag] ?? 0);
      expect(alignLag(full, voc)).toBe(lag);
    }
  });

  it("instrumental + vocals is exactly the original", () => {
    const n = 48000 * 2;
    const voc = sig(n, 3, 17);
    const beat = sig(n, 5, 2);
    const delayed = new Float32Array(n);
    for (let i = 0; i < n; i++) delayed[i] = voc[i - 1024] ?? 0;
    const full = beat.map((b, i) => b + (delayed[i] ?? 0));
    const { channels, lag } = buildStemChannels([full, full], [voc, voc]);
    expect(lag).toBe(1024);
    const [il, , vl] = channels;
    let resid = 0;
    let leak = 0;
    for (let i = 0; i < n; i++) {
      resid = Math.max(resid, Math.abs((il![i] ?? 0) + (vl![i] ?? 0) - (full[i] ?? 0)));
      leak = Math.max(leak, Math.abs((il![i] ?? 0) - (beat[i] ?? 0)));
    }
    expect(resid).toBeLessThan(1e-6); // full mix untouched
    expect(leak).toBeLessThan(1e-6); // no vocal left in the instrumental
  });
});
