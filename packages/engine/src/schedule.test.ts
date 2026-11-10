import type { Transition } from "@selecta/protocol";
import { describe, expect, it } from "vitest";
import { KILL_DB, scheduleTransition, type Move } from "./schedule";

const barSec = 1.379; // 174 BPM
const barTime = (b: number) => 10 + b * barSec;
const tr = (p: Partial<Transition>): Transition => ({ kind: "blend", fromBar: 48, len: 16, enterAt: 48, swapAt: 56, echoDiv: null, voxOut: null, loopFrom: null, loopBars: null, ...p });
const last = (moves: Move[], side: string, target: string) => moves.filter((m) => m.side === side && m.target === target).at(-1)!;

describe("scheduleTransition", () => {
  it("blend: incoming enters on the from bar, bass crosses over around the swap, outgoing is gone by the end", () => {
    const s = scheduleTransition(tr({}), barTime, barSec);
    expect(s.startIn).toBeCloseTo(barTime(48), 9);
    expect(s.outEndsAt).toBeCloseTo(barTime(64), 9);
    expect(s.handoverAt).toBeCloseTo(barTime(56), 9);
    expect(last(s.moves, "out", "fader")).toMatchObject({ value: 0, until: barTime(64) });
    expect(last(s.moves, "out", "low").value).toBe(KILL_DB);
    expect(last(s.moves, "in", "low").value).toBe(0);
    // Every move happens inside the transition window.
    for (const m of s.moves) {
      expect(m.at).toBeGreaterThanOrEqual(barTime(48) - barSec - 1e-9);
      expect(m.until ?? m.at).toBeLessThanOrEqual(barTime(64) + 1e-9);
    }
  });

  it("bass swap and double drop swap the low end exactly on the swap downbeat", () => {
    for (const kind of ["bassswap", "double_drop"] as const) {
      const s = scheduleTransition(tr({ kind, swapAt: 60 }), barTime, barSec);
      const swaps = s.moves.filter((m) => m.target === "low" && m.at === barTime(60));
      expect(swaps).toHaveLength(2);
      expect(swaps.find((m) => m.side === "out")!.value).toBe(KILL_DB);
      expect(swaps.find((m) => m.side === "in")!.value).toBe(0);
      expect(swaps.every((m) => m.until === undefined)).toBe(true);
    }
  });

  it("echo out: echo runs in the bar before the cut, incoming lands on the downbeat", () => {
    const s = scheduleTransition(tr({ kind: "echo_out", fromBar: 63, len: 1, enterAt: 64, echoDiv: 0.25 }), barTime, barSec);
    expect(s.echo).toEqual({ at: barTime(63), until: barTime(64), delaySec: 0.25 * barSec });
    expect(s.startIn).toBeCloseTo(barTime(64), 9);
    expect(last(s.moves, "out", "fader")).toMatchObject({ value: 0, until: barTime(64) });
  });

  it("fx exits: the move ends exactly on the incoming downbeat", () => {
    for (const kind of ["backspin", "tape_stop", "rewind", "loop_roll"] as const) {
      const fromBar = kind === "loop_roll" ? 62 : 63;
      const s = scheduleTransition(tr({ kind, fromBar, len: 64 - fromBar, enterAt: 64 }), barTime, barSec);
      expect(s.exitFx!.until).toBeCloseTo(barTime(64), 9);
      expect(s.exitFx!.at).toBeLessThan(barTime(64));
      expect(s.exitFx!.at).toBeGreaterThanOrEqual(barTime(fromBar) - 1e-9);
      expect(s.startIn).toBeCloseTo(barTime(64), 9);
    }
  });

  it("slam: a declicked cut on the downbeat", () => {
    const s = scheduleTransition(tr({ kind: "slam", fromBar: 64, len: 0, enterAt: 64 }), barTime, barSec);
    const fade = last(s.moves, "out", "fader");
    expect(fade.until! - fade.at).toBeLessThan(0.01);
    expect(s.startIn).toBeCloseTo(barTime(64), 9);
  });

  it("acapella build and bootleg: vocal only until the swap, then the incoming beat drops and the outgoing is cut", () => {
    for (const kind of ["acapella_build", "bootleg"] as const) {
      const s = scheduleTransition(tr({ kind, fromBar: 48, len: 16, enterAt: 48, swapAt: 64, voxOut: 48 }), barTime, barSec);
      const at = (side: string, target: string, t: number) => s.moves.filter((m) => m.side === side && m.target === target && Math.abs(m.at - t) < 1e-9);
      expect(at("in", "inst", barTime(48))[0]!.value).toBe(0);
      expect(at("in", "inst", barTime(64))[0]!.value).toBe(1);
      expect(last(s.moves, "in", "vox").value).toBe(1);
      // The outgoing's own vocal is gone by the time the acapella starts.
      expect(last(s.moves, "out", "vox")).toMatchObject({ value: 0, until: barTime(48) });
      expect(last(s.moves, "out", "fader")).toMatchObject({ value: 0, until: barTime(64) });
      expect(s.handoverAt).toBeCloseTo(barTime(64), 9);
      expect(s.echo!.until).toBeCloseTo(barTime(64), 9);
    }
  });

  it("mashup: both drops together, bass stays with the outgoing until 4 bars from the end", () => {
    const s = scheduleTransition(tr({ kind: "mashup", fromBar: 44, len: 32, enterAt: 44, swapAt: 60, voxOut: 60 }), barTime, barSec);
    expect(s.handoverAt).toBeCloseTo(barTime(60), 9);
    expect(s.moves.find((m) => m.side === "in" && m.target === "low")!).toMatchObject({ at: barTime(44), value: KILL_DB });
    const bass = s.moves.filter((m) => m.target === "low" && Math.abs(m.at - barTime(72)) < 1e-9);
    expect(bass.map((m) => [m.side, m.value]).sort()).toEqual([["in", 0], ["out", KILL_DB]]);
    expect(last(s.moves, "out", "vox")).toMatchObject({ value: 0, until: barTime(60) });
    expect(s.outEndsAt).toBeCloseTo(barTime(76), 9);
  });

  it("vocal clash guard applies to plain blends too", () => {
    const s = scheduleTransition(tr({ kind: "bassswap", swapAt: 56, voxOut: 56 }), barTime, barSec);
    expect(last(s.moves, "out", "vox")).toMatchObject({ value: 0, until: barTime(56) });
    expect(scheduleTransition(tr({ kind: "bassswap" }), barTime, barSec).moves.some((m) => m.target === "vox")).toBe(false);
  });
});
