import { describe, expect, it } from "vitest";
import { barOfPos, matchRate, nextGrid, phaseError, posAt, posOfBar, timeOfPos } from "./grid";

const g = { beat: 60 / 174, downbeat0: 0.25 };

describe("grid", () => {
  it("round-trips bars and positions", () => {
    expect(barOfPos(g, posOfBar(g, 37))).toBeCloseTo(37, 9);
  });

  it("maps context time through a rate-shifted base line", () => {
    const b = { t: 10, pos: 30, rate: 1.04 };
    expect(posAt(b, 12)).toBeCloseTo(32.08, 9);
    expect(timeOfPos(b, posAt(b, 17.3))).toBeCloseTo(17.3, 9);
  });

  it("finds the next beat and bar on the grid", () => {
    const b = { t: 0, pos: 0, rate: 1 };
    const t = nextGrid(g, b, 1.0, 1);
    expect(((t - g.downbeat0) / g.beat) % 1).toBeCloseTo(0, 6);
    expect(t).toBeGreaterThanOrEqual(1.0);
    const bar = nextGrid(g, b, 1.0, 4);
    expect(barOfPos(g, bar) % 1).toBeCloseTo(0, 6);
  });

  it("folds octaves when matching tempo and refuses big shifts", () => {
    expect(matchRate(174, 87)).toBeCloseTo(1, 9);
    expect(matchRate(174, 172)).toBeCloseTo(174 / 172, 9);
    expect(matchRate(128, 96)).toBeNull();
  });

  it("reports zero phase error for decks started on the same downbeat", () => {
    const gb = { beat: 60 / 172, downbeat0: 0.6 };
    const rate = matchRate(174, 172)!;
    const ba = { t: 0, pos: posOfBar(g, 8), rate: 1 };
    const bb = { t: 0, pos: posOfBar(gb, 0), rate };
    for (const t of [0.5, 7.3, 31.1]) expect(Math.abs(phaseError(g, ba, gb, bb, t))).toBeLessThan(1e-9);
  });
});
