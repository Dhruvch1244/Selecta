// Beat-grid maths for one deck, with no Web Audio in sight so it can be
// tested headless. A deck's timeline is a *base line*: at context time `t`
// the track position is `base.pos + (t - base.t) * rate`. Loops, rolls and
// spinbacks (later PRs) temporarily replace audio but resume on the same line
// (slip mode), so the grid never drifts and the conductor can keep
// scheduling the rest of the set against it.

export interface GridSpec {
  /** Seconds per beat at the track's original tempo. */
  beat: number;
  /** Track time of bar 0's downbeat. */
  downbeat0: number;
}

export interface BaseLine {
  t: number;
  pos: number;
  rate: number;
}

export const posAt = (b: BaseLine, t: number): number => b.pos + (t - b.t) * b.rate;
export const timeOfPos = (b: BaseLine, pos: number): number => b.t + (pos - b.pos) / b.rate;
export const posOfBar = (g: GridSpec, bar: number): number => g.downbeat0 + bar * 4 * g.beat;
export const barOfPos = (g: GridSpec, pos: number): number => (pos - g.downbeat0) / (4 * g.beat);

/** Context time of the next grid line at or after `t`, in units of `beats` (1 = beat, 4 = bar). */
export function nextGrid(g: GridSpec, b: BaseLine, t: number, beats = 1): number {
  const step = beats * g.beat;
  const k = Math.ceil((posAt(b, t) - g.downbeat0) / step - 1e-6);
  return timeOfPos(b, g.downbeat0 + k * step);
}

/**
 * playbackRate that makes `dst` play at `src`'s tempo, folding octaves so a
 * 87 BPM half-time reading still matches 174. Returns null beyond `maxShift`
 * (no keylock yet: past ~8% the pitch shift is audible, so callers cut instead).
 */
export function matchRate(srcBpm: number, dstBpm: number, maxShift = 0.08): number | null {
  let r = srcBpm / dstBpm;
  while (r > Math.SQRT2) r /= 2;
  while (r < Math.SQRT1_2) r *= 2;
  return Math.abs(r - 1) <= maxShift ? r : null;
}

/**
 * Beat-phase difference between two decks at the same instant, in beats,
 * wrapped to [-0.5, 0.5). 0 = locked. Used by tests and the phase meter.
 */
export function phaseError(ga: GridSpec, ba: BaseLine, gb: GridSpec, bb: BaseLine, t: number): number {
  const pa = barOfPos(ga, posAt(ba, t)) * 4;
  const pb = barOfPos(gb, posAt(bb, t)) * 4;
  const d = (pa - pb) % 1;
  return ((d + 1.5) % 1) - 0.5;
}
