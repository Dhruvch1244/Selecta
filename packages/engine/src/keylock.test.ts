import { describe, expect, it } from "vitest";
import { keylockSemitones } from "./keylock";

describe("keylockSemitones", () => {
  it("cancels the pitch change a playback rate causes", () => {
    expect(keylockSemitones(1)).toBeCloseTo(0, 12);
    // +4% tempo raises pitch by 0.679 semitones; the render lowers it by the same.
    expect(keylockSemitones(1.04)).toBeCloseTo(-0.679, 3);
    expect(keylockSemitones(2 ** (1 / 12))).toBeCloseTo(-1, 9);
  });
  it("adds the planned harmonic key shift on top", () => {
    expect(keylockSemitones(1, 2)).toBe(2);
    expect(keylockSemitones(1.04, -1)).toBeCloseTo(-1.679, 3);
  });
});
