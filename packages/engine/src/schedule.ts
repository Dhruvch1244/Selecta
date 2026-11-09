// Turns one planned transition into concrete, timed mixer moves. Pure: it
// takes "when is outgoing bar N?" as a function and returns automation
// events, so every transition's choreography is unit-tested without an
// AudioContext. The conductor applies the events to real AudioParams.

import type { Transition, TransitionKind } from "@selecta/protocol";

export type Target = "fader" | "low" | "high" | "hp" | "lp" | "vox" | "inst";
export type Side = "out" | "in";

export interface Move {
  side: Side;
  target: Target;
  /** Context time the move starts (a step) or the ramp begins. */
  at: number;
  value: number;
  /** If set, ramp linearly from the value in force at `at` to `value` by `until`. */
  until?: number;
}

export interface TransitionSchedule {
  kind: TransitionKind;
  /** When the incoming deck starts playing (its in_bar lands here). */
  startIn: number;
  /** When the incoming deck becomes "the" deck for readouts and lyrics. */
  handoverAt: number;
  /** When the outgoing deck is silent and can be stopped. */
  outEndsAt: number;
  moves: Move[];
  echo?: { at: number; until: number; delaySec: number };
  /** An effect on the outgoing deck that *is* the exit (backspin, rewind...). */
  exitFx?: { id: string; at: number; until: number };
}

export const KILL_DB = -40;
const LP_CLOSED = 280;
const HP_OPEN = 1600;

/**
 * @param t        transition from the plan, in outgoing-track bars
 * @param barTime  context time of an outgoing-track bar
 * @param barSec   outgoing bar length in seconds (after its playback rate)
 */
export function scheduleTransition(t: Transition, barTime: (bar: number) => number, barSec: number): TransitionSchedule {
  const tFrom = barTime(t.fromBar);
  const tEnter = barTime(t.enterAt);
  const tEnd = barTime(t.fromBar + t.len);
  const moves: Move[] = [];
  const m = (side: Side, target: Target, at: number, value: number, until?: number) => moves.push({ side, target, at, value, ...(until !== undefined ? { until } : {}) });

  switch (t.kind) {
    case "blend":
    case "bassswap":
    case "double_drop": {
      const L = tEnd - tEnter;
      const swap = barTime(t.swapAt ?? t.enterAt + Math.floor(t.len / 2));
      // Incoming rises without its bass; outgoing keeps the low end until the swap.
      m("in", "fader", tEnter, 0);
      m("in", "fader", tEnter, 1, tEnter + Math.min(L * 0.3, 4 * barSec));
      m("in", "low", tEnter, KILL_DB);
      if (t.kind === "blend") {
        // Gradual 2-bar crossover of the low band around the swap point.
        m("out", "low", swap - barSec, 0);
        m("out", "low", swap - barSec, KILL_DB, swap + barSec);
        m("in", "low", swap - barSec, KILL_DB);
        m("in", "low", swap - barSec, 0, swap + barSec);
        m("out", "high", swap, 0);
        m("out", "high", swap, -12, tEnd);
      } else {
        // Hard swap exactly on the downbeat (double drop: both drops land here).
        m("out", "low", swap, KILL_DB);
        m("in", "low", swap, 0);
      }
      const fadeLen = Math.min(L * 0.35, 8 * barSec);
      m("out", "fader", tEnd - fadeLen, 1);
      m("out", "fader", tEnd - fadeLen, 0, tEnd);
      return { kind: t.kind, startIn: tEnter, handoverAt: swap, outEndsAt: tEnd, moves };
    }
    case "filter": {
      m("out", "hp", tEnter, 10);
      m("out", "hp", tEnter, HP_OPEN, tEnd);
      m("in", "lp", tEnter, LP_CLOSED);
      m("in", "lp", tEnter, 22000, tEnd);
      m("in", "fader", tEnter, 0);
      m("in", "fader", tEnter, 1, tEnter + (tEnd - tEnter) * 0.45);
      const fadeAt = tEnter + (tEnd - tEnter) * 0.55;
      m("out", "fader", fadeAt, 1);
      m("out", "fader", fadeAt, 0, tEnd);
      return { kind: t.kind, startIn: tEnter, handoverAt: (tEnter + tEnd) / 2, outEndsAt: tEnd, moves };
    }
    case "echo_out": {
      // The echo tail is routed post-fader, so it rings on after the cut.
      m("out", "fader", tEnter - 0.01, 1);
      m("out", "fader", tEnter - 0.01, 0, tEnter);
      m("in", "fader", tEnter, 1);
      return {
        kind: t.kind,
        startIn: tEnter,
        handoverAt: tEnter,
        outEndsAt: tEnter + 0.05,
        moves,
        echo: { at: tFrom, until: tEnter, delaySec: (t.echoDiv ?? 0.25) * barSec },
      };
    }
    case "backspin":
    case "tape_stop":
    case "rewind":
    case "loop_roll": {
      const beatSec = barSec / 4;
      // Backspins are short and land right before the downbeat; the others use the whole lead-in.
      const at = t.kind === "backspin" ? tEnter - 2 * beatSec : tFrom;
      const id = { backspin: "backspin:2", tape_stop: "tape_stop:2", rewind: "rewind:8", loop_roll: "roll_build:2" }[t.kind];
      m("out", "fader", tEnter - 0.004, 1);
      m("out", "fader", tEnter - 0.004, 0, tEnter);
      m("in", "fader", tEnter, 1);
      return { kind: t.kind, startIn: tEnter, handoverAt: tEnter, outEndsAt: tEnter + 0.05, moves, exitFx: { id, at, until: tEnter } };
    }
    case "slam":
    case "start":
    default: {
      m("out", "fader", tEnter - 0.004, 1);
      m("out", "fader", tEnter - 0.004, 0, tEnter);
      m("in", "fader", tEnter, 1);
      return { kind: t.kind, startIn: tEnter, handoverAt: tEnter, outEndsAt: tEnter + 0.02, moves };
    }
  }
}
