// One deck: buffer playback on a beat grid, plus its mixer channel strip
// (trim, 3-band kill EQ, sweep filter, fader). Everything is scheduled on the
// AudioContext clock; nothing here uses setTimeout for audio timing.

import type { Analysis } from "@selecta/protocol";
import { type BaseLine, type GridSpec, barOfPos, nextGrid, posAt, posOfBar, timeOfPos } from "./grid";
import { type MeterState, newMeter, stepMeter } from "./meter";
import { computeWaveform, type Waveform } from "./waveform";
import type { Move, Target } from "./schedule";

export type DeckId = "A" | "B";
export type Band = "low" | "mid" | "high";

export interface LoadedTrack {
  id: string;
  artist: string;
  title: string;
  /** 2 channels, or 4 for a stem track (instrumental L/R, vocals L/R; see stems.ts). */
  buffer: AudioBuffer;
  analysis: Analysis;
}

export type Stem = "vox" | "inst";

const DECLICK = 0.004;
/** dB to linear for isolator bands; at or below the kill level the band is fully off. */
export const dbToGain = (db: number): number => (db <= EQ_KILL_DB ? 0 : Math.pow(10, db / 20));
/** Full kill: -40 dB is inaudible under a playing deck and avoids denormal tails. */
export const EQ_KILL_DB = -40;

export class Deck {
  readonly id: DeckId;
  track: LoadedTrack | null = null;
  wave: Waveform | null = null;
  meter: MeterState = newMeter();

  private readonly ctx: BaseAudioContext;
  private src: AudioBufferSourceNode | null = null;
  private base: BaseLine = { t: 0, pos: 0, rate: 1 };
  private stoppedAt = Infinity;
  private readonly vca: GainNode;
  private readonly trimNode: GainNode;
  /** Stem gains (linear). On a 2-channel track `inst` is the whole track and `vox` has nothing to do. */
  private readonly stem: Record<Stem, GainNode>;
  /** Isolator band gains (linear; 0 = full kill). */
  private readonly eq: Record<Band, GainNode>;
  private readonly eqIn: GainNode;
  readonly slotIn: GainNode;
  readonly slotOut: GainNode;
  readonly tail: GainNode;
  private readonly slotDry: GainNode;
  private insertWindows: [number, number][] = [];
  private rev: AudioBuffer | null = null;
  /** End of the current playback override (loop roll, backspin...): another can't start before it. */
  busyUntil = 0;
  private ov: { t0: number; t1: number; posFn: ((t: number) => number) | null } | null = null;
  private readonly bandSum: GainNode;
  private readonly filterHp: BiquadFilterNode;
  private readonly filterLp: BiquadFilterNode;
  readonly fader: GainNode;
  private readonly echoSend: GainNode;
  private readonly echoDelay: DelayNode;
  private readonly analyser: AnalyserNode;
  private readonly meterBuf: Float32Array<ArrayBuffer>;

  constructor(ctx: BaseAudioContext, id: DeckId, out: AudioNode) {
    this.ctx = ctx;
    this.id = id;
    this.vca = ctx.createGain();
    this.trimNode = ctx.createGain();
    const band = (type: BiquadFilterType, freq: number) => {
      const f = ctx.createBiquadFilter();
      f.type = type;
      f.frequency.value = freq;
      return f;
    };
    // 3-band isolator, as on club DJ mixers: the signal is split by 4th-order
    // Linkwitz-Riley crossovers (two cascaded Butterworth biquads) at 250 Hz
    // and 2.5 kHz, each band gets its own gain, and the bands are summed.
    // Unlike shelving EQs, a kill really removes its band (the old
    // -40 dB low shelf also dulled the low mids, so bass swaps sounded muddy).
    const lr4 = (type: BiquadFilterType, freq: number): [BiquadFilterNode, BiquadFilterNode] => {
      const a = band(type, freq);
      const b = band(type, freq);
      // Butterworth. Web Audio gives lowpass/highpass Q *in dB*: a linear 0.7071
      // here is read as +0.7 dB resonance, and measured as +2..4 dB bumps
      // around both crossovers. 20*log10(1/sqrt 2) = -3.0103 dB is the real thing.
      a.Q.value = b.Q.value = 20 * Math.log10(Math.SQRT1_2);
      a.connect(b);
      return [a, b];
    };
    const split = ctx.createGain();
    this.bandSum = ctx.createGain();
    this.eq = { low: ctx.createGain(), mid: ctx.createGain(), high: ctx.createGain() };
    const [l1, l2] = lr4("lowpass", 250);
    split.connect(l1);
    l2.connect(this.eq.low).connect(this.bandSum);
    const [m1, m2] = lr4("highpass", 250);
    const [m3, m4] = lr4("lowpass", 2500);
    split.connect(m1);
    m2.connect(m3);
    m4.connect(this.eq.mid).connect(this.bandSum);
    const [h1, h2] = lr4("highpass", 2500);
    split.connect(h1);
    h2.connect(this.eq.high).connect(this.bandSum);
    this.eqIn = split;
    this.filterHp = band("highpass", 10);
    this.filterLp = band("lowpass", 22000);
    this.filterHp.Q.value = this.filterLp.Q.value = 0.9;
    this.fader = ctx.createGain();
    this.analyser = ctx.createAnalyser();
    this.analyser.fftSize = 1024;
    this.meterBuf = new Float32Array(this.analyser.fftSize);
    // Stems: split the (up to) 4-channel source into instrumental and vocal
    // pairs, each with its own gain, then sum. A 2-channel track lands
    // entirely on the instrumental pair (discrete up-mix zero-fills 2-3).
    const split4 = ctx.createChannelSplitter(4);
    split4.channelInterpretation = "discrete";
    this.stem = { inst: ctx.createGain(), vox: ctx.createGain() };
    const instM = ctx.createChannelMerger(2);
    const voxM = ctx.createChannelMerger(2);
    split4.connect(instM, 0, 0);
    split4.connect(instM, 1, 1);
    split4.connect(voxM, 2, 0);
    split4.connect(voxM, 3, 1);
    instM.connect(this.stem.inst).connect(this.trimNode);
    voxM.connect(this.stem.vox).connect(this.trimNode);
    this.vca.channelCountMode = "explicit";
    this.vca.channelCount = 4;
    this.vca.channelInterpretation = "discrete";
    this.vca.connect(split4);
    this.trimNode.connect(this.eqIn);
    // FX slot after the filters: inserts mute `slotDry` for their window and
    // feed `slotOut` themselves; sends tap `slotIn` and return on `tail`,
    // which is post-fader so echo/reverb tails ring on after a fader cut.
    this.slotIn = ctx.createGain();
    this.slotDry = ctx.createGain();
    this.slotOut = ctx.createGain();
    this.tail = ctx.createGain();
    this.bandSum.connect(this.filterHp).connect(this.filterLp).connect(this.slotIn);
    this.slotIn.connect(this.slotDry).connect(this.slotOut).connect(this.fader).connect(out);
    this.tail.connect(out);
    this.fader.connect(this.analyser);
    // Echo send taps pre-fader and returns straight to the mix bus, so an
    // echo-out tail keeps ringing after this deck's fader is cut.
    this.echoSend = ctx.createGain();
    this.echoSend.gain.value = 0;
    this.echoDelay = ctx.createDelay(4);
    const fb = ctx.createGain();
    fb.gain.value = 0.58;
    const tone = ctx.createBiquadFilter();
    tone.type = "lowpass";
    tone.frequency.value = 5000;
    this.filterLp.connect(this.echoSend).connect(this.echoDelay);
    this.echoDelay.connect(tone).connect(fb).connect(this.echoDelay);
    this.echoDelay.connect(out);
  }

  get grid(): GridSpec | null {
    return this.track ? { beat: this.track.analysis.beat, downbeat0: this.track.analysis.downbeat0 } : null;
  }
  /** True when the loaded track has a vocal stem (vox/inst are separable). */
  get hasStems(): boolean {
    return (this.track?.buffer.numberOfChannels ?? 0) >= 4;
  }
  /**
   * Audible right now. A deck whose stop is *scheduled* (the outgoing deck
   * of a transition) is still playing until then: FX and the FX pad must
   * keep working on it, and an earlier stop must still be able to reach it.
   */
  get playing(): boolean {
    return this.src !== null && this.ctx.currentTime < this.stoppedAt;
  }
  get rate(): number {
    return this.base.rate;
  }
  /** Current tempo after pitch/rate, in BPM. */
  get bpm(): number {
    return this.track ? this.track.analysis.bpm * this.base.rate : 0;
  }

  load(track: LoadedTrack): void {
    this.stop();
    this.src = null;
    if (track.buffer.numberOfChannels === 1) {
      // The stem path maps channels discretely, so a mono file must be made
      // stereo here or it would play on the left only.
      const st = this.ctx.createBuffer(2, track.buffer.length, track.buffer.sampleRate);
      const m = track.buffer.getChannelData(0);
      st.copyToChannel(m, 0);
      st.copyToChannel(m, 1);
      track = { ...track, buffer: st };
    }
    this.track = track;
    this.rev = null;
    this.busyUntil = 0;
    this.ov = null;
    const chans = Array.from({ length: Math.min(4, track.buffer.numberOfChannels) }, (_, i) => track.buffer.getChannelData(i));
    this.wave = computeWaveform(chans, track.buffer.sampleRate);
    this.base = { t: this.ctx.currentTime, pos: posOfBar(this.gridOrThrow(), track.analysis.startBar), rate: 1 };
    this.stoppedAt = this.ctx.currentTime;
    // Loudness normalisation from analysis; the trim knob adds on top.
    this.setTrimDb(0);
  }

  /** Position in track seconds at context time `t` (defaults to now). Frozen while stopped. */
  position(t = this.ctx.currentTime): number {
    const tt = Math.min(t, this.stoppedAt);
    if (this.ov?.posFn && tt >= this.ov.t0 && tt < this.ov.t1) return this.ov.posFn(tt);
    return posAt(this.base, tt);
  }
  bar(t = this.ctx.currentTime): number {
    const g = this.grid;
    return g ? barOfPos(g, this.position(t)) : 0;
  }
  timeOfBar(bar: number): number {
    return timeOfPos(this.base, posOfBar(this.gridOrThrow(), bar));
  }
  nextBeat(beats = 1, after = this.ctx.currentTime): number {
    return nextGrid(this.gridOrThrow(), this.base, after, beats);
  }

  play(at = this.ctx.currentTime, from = this.position()): void {
    if (!this.track || (this.src && this.stoppedAt === Infinity)) return;
    const when = Math.max(at, this.ctx.currentTime);
    // A previous scheduled stop's fade must not land on this playback.
    this.vca.gain.cancelScheduledValues(when);
    this.base = { t: when, pos: from, rate: this.base.rate };
    const s = this.ctx.createBufferSource();
    s.buffer = this.track.buffer;
    s.playbackRate.value = this.base.rate;
    s.connect(this.vca);
    this.vca.gain.setValueAtTime(0, when);
    this.vca.gain.linearRampToValueAtTime(1, when + DECLICK);
    s.start(when, Math.max(0, from));
    s.onended = () => {
      if (this.src === s) {
        this.src = null;
        this.stoppedAt = Math.min(this.stoppedAt, this.ctx.currentTime);
      }
    };
    this.src = s;
    this.stoppedAt = Infinity;
  }

  stop(at = this.ctx.currentTime): void {
    if (!this.src) return;
    const when = Math.max(at, this.ctx.currentTime);
    if (when >= this.stoppedAt) return; // already stopping sooner
    // Replaces any later scheduled stop (an earlier stop always wins).
    this.vca.gain.cancelScheduledValues(when);
    this.vca.gain.setValueAtTime(1, when);
    this.vca.gain.linearRampToValueAtTime(0, when + DECLICK);
    try {
      this.src.stop(when + DECLICK);
    } catch {
      // already ended
    }
    this.stoppedAt = when;
  }

  /** Jump to a track position, keeping play state (quantised callers pass a grid time). */
  seek(pos: number, at = this.ctx.currentTime): void {
    const was = this.playing;
    this.stop(at);
    this.src = null;
    this.base = { t: Math.max(at, this.ctx.currentTime), pos, rate: this.base.rate };
    this.stoppedAt = this.base.t;
    if (was) this.play(this.base.t, pos);
  }

  /** Change playback rate at a grid-safe instant, re-anchoring the base line so the grid stays continuous. */
  setRate(rate: number, at = this.ctx.currentTime): void {
    const when = Math.max(at, this.ctx.currentTime);
    const pos = this.position(when);
    this.base = { t: when, pos, rate };
    this.src?.playbackRate.setValueAtTime(rate, when);
  }

  setTrimDb(db: number): void {
    const norm = this.track?.analysis.gainDb ?? 0;
    this.trimNode.gain.setTargetAtTime(Math.pow(10, (norm + db) / 20), this.ctx.currentTime, 0.01);
  }
  setEq(band: Band, db: number): void {
    this.eq[band].gain.setTargetAtTime(dbToGain(db), this.ctx.currentTime, 0.008);
  }
  /** -1..1: negative sweeps a low-pass down, positive a high-pass up, 0 is bypass. */
  setFilter(v: number): void {
    const t = this.ctx.currentTime;
    const lp = v < 0 ? 22000 * Math.pow(250 / 22000, -v) : 22000;
    const hp = v > 0 ? 10 * Math.pow(6000 / 10, v) : 10;
    this.filterLp.frequency.setTargetAtTime(lp, t, 0.01);
    this.filterHp.frequency.setTargetAtTime(hp, t, 0.01);
  }
  /** Stem level 0..1 (a kill is 0). */
  setStem(stem: Stem, v: number): void {
    this.stem[stem].gain.setTargetAtTime(v, this.ctx.currentTime, 0.008);
  }
  stemLevel(stem: Stem): number {
    return this.stem[stem].gain.value;
  }
  setFader(v: number): void {
    // Equal-power-ish curve: a fader at 50% should sound half as loud, not silent.
    this.fader.gain.setTargetAtTime(Math.pow(v, 1.6), this.ctx.currentTime, 0.008);
  }

  /**
   * Mute the dry path for [t0, t1] while an insert effect feeds `slotOut`.
   * Windows are merged and the automation rebuilt, so two overlapping
   * inserts never fight over the same AudioParam.
   */
  addInsert(t0: number, t1: number): void {
    const now = this.ctx.currentTime;
    this.insertWindows = this.insertWindows.filter((w) => w[1] > now - 0.05);
    this.insertWindows.push([t0, t1]);
    this.insertWindows.sort((a, b) => a[0] - b[0]);
    const merged: [number, number][] = [];
    for (const w of this.insertWindows) {
      const last = merged.at(-1);
      if (last && w[0] <= last[1]) last[1] = Math.max(last[1], w[1]);
      else merged.push([w[0], w[1]]);
    }
    const g = this.slotDry.gain;
    g.cancelScheduledValues(now);
    g.setValueAtTime(g.value, now);
    for (const [a, b] of merged) {
      g.setValueAtTime(1, Math.max(now, a - 0.006));
      g.linearRampToValueAtTime(0, Math.max(now, a));
      g.setValueAtTime(0, Math.max(now, b - 0.006));
      g.linearRampToValueAtTime(1, Math.max(now, b));
    }
  }

  /** The track reversed (for backspins and rewinds), computed once per load. */
  get reversed(): AudioBuffer | null {
    const b = this.track?.buffer;
    if (!b) return null;
    if (!this.rev) {
      const r = this.ctx.createBuffer(b.numberOfChannels, b.length, b.sampleRate);
      for (let ch = 0; ch < b.numberOfChannels; ch++) {
        const src = b.getChannelData(ch);
        const dst = r.getChannelData(ch);
        for (let i = 0, n = src.length; i < n; i++) dst[i] = src[n - 1 - i] ?? 0;
      }
      this.rev = r;
    }
    return this.rev;
  }

  /**
   * Replace the playing audio with an equivalent buffer of the same length
   * (a keylock render of this track), seamlessly: the new source picks up at
   * exactly the position the old one is at, on the next beat.
   */
  swapBuffer(buffer: AudioBuffer): void {
    const t = this.track;
    if (!t || buffer.length !== t.buffer.length) return;
    this.track = { ...t, buffer };
    this.rev = null;
    if (!this.src || this.stoppedAt !== Infinity) return;
    const at = Math.max(this.ctx.currentTime + 0.05, this.busyUntil);
    const pos = posAt(this.base, at);
    this.vca.gain.cancelScheduledValues(at - DECLICK);
    this.vca.gain.setValueAtTime(1, at - DECLICK);
    this.vca.gain.linearRampToValueAtTime(0, at);
    this.vca.gain.linearRampToValueAtTime(1, at + DECLICK);
    this.src.stop(at);
    const s = this.source(buffer);
    s.start(at, Math.max(0, pos));
    s.onended = () => {
      if (this.src === s) {
        this.src = null;
        this.stoppedAt = Math.min(this.stoppedAt, this.ctx.currentTime);
      }
    };
    this.src = s;
  }

  /** Head of the deck's chain: playback sources connect here. */
  get input(): AudioNode {
    return this.vca;
  }

  /** A source wired into this deck's chain (used by playback overrides). */
  source(buffer: AudioBuffer, rate = this.base.rate): AudioBufferSourceNode {
    const s = this.ctx.createBufferSource();
    s.buffer = buffer;
    s.playbackRate.value = rate;
    s.connect(this.vca);
    return s;
  }

  /**
   * Replace playback for [t0, t1] with sources from `build` (slip mode): the
   * base line keeps running, so afterwards the track resumes exactly where it
   * would have been, on the grid. `after: "stop"` ends the deck instead;
   * `{pos}` jumps (a rewind's pull-up). Returns false if the deck is busy.
   */
  override(
    t0: number,
    t1: number,
    build: (t0: number, t1: number, pos0: number) => AudioBufferSourceNode[],
    opts: { after?: "slip" | "stop" | { pos: number }; posFn?: (t: number, pos0: number) => number } = {},
  ): boolean {
    if (!this.track || !this.playing || !this.src || t0 < Math.max(this.ctx.currentTime, this.busyUntil)) return false;
    const pos0 = posAt(this.base, t0);
    const declick = (t: number, up: boolean) => {
      this.vca.gain.setValueAtTime(up ? 0 : 1, t - (up ? 0 : DECLICK));
      this.vca.gain.linearRampToValueAtTime(up ? 1 : 0, t + (up ? DECLICK : 0));
    };
    declick(t0, false);
    this.src.stop(t0);
    const srcs = build(t0, t1, pos0);
    declick(t0 + 0.0001, true);
    for (const s of srcs) s.stop(t1);
    this.ov = { t0, t1, posFn: opts.posFn ? (t) => opts.posFn!(t, pos0) : null };
    this.busyUntil = t1;
    const after = opts.after ?? "slip";
    if (after === "stop") {
      this.stoppedAt = t1;
      return true;
    }
    if (after !== "slip") this.base = { t: t1, pos: after.pos, rate: this.base.rate };
    const resume = posAt(this.base, t1);
    declick(t1, false);
    const main = this.source(this.track.buffer);
    main.start(t1, Math.max(0, resume));
    declick(t1 + 0.0001, true);
    this.src = main;
    return true;
  }

  /** Neutral mixer state for a freshly loaded track that a set is about to automate. */
  resetMix(at = this.ctx.currentTime): void {
    for (const b of ["low", "mid", "high"] as const) {
      this.eq[b].gain.cancelScheduledValues(at);
      this.eq[b].gain.setValueAtTime(1, at);
    }
    this.filterHp.frequency.cancelScheduledValues(at);
    this.filterHp.frequency.setValueAtTime(10, at);
    this.filterLp.frequency.cancelScheduledValues(at);
    this.filterLp.frequency.setValueAtTime(22000, at);
    this.fader.gain.cancelScheduledValues(at);
    this.fader.gain.setValueAtTime(1, at);
    for (const g of [this.stem.vox.gain, this.stem.inst.gain]) {
      g.cancelScheduledValues(at);
      g.setValueAtTime(1, at);
    }
  }

  /** Apply scheduled moves from a transition schedule (see schedule.ts). */
  automate(moves: Move[]): void {
    for (const m of [...moves].sort((a, b) => a.at - b.at)) {
      const p = this.param(m.target);
      const at = Math.max(m.at, this.ctx.currentTime);
      // EQ moves are written in dB (schedule.ts); the isolator bands take linear gain.
      const v = m.target === "low" || m.target === "high" ? dbToGain(m.value) : m.value;
      // A ramp starts from the previous event; schedules always place an
      // explicit step at the ramp's start. (Re-anchoring at `p.value` here used
      // to override that step with the value in force *now*, which silently
      // turned every incoming fade-in into "start at full volume".)
      if (m.until === undefined) p.setValueAtTime(v, at);
      else if (m.target === "hp" || m.target === "lp") p.exponentialRampToValueAtTime(Math.max(10, v), Math.max(at + 0.001, m.until));
      else p.linearRampToValueAtTime(v, Math.max(at + 0.001, m.until));
    }
  }

  /** Feed the echo for [at, until] with a delay of `delaySec`; the tail decays on its own. */
  echo(at: number, until: number, delaySec: number): void {
    const t0 = Math.max(at, this.ctx.currentTime);
    this.echoDelay.delayTime.setValueAtTime(Math.min(3.9, Math.max(0.02, delaySec)), t0);
    this.echoSend.gain.setValueAtTime(0, t0);
    this.echoSend.gain.linearRampToValueAtTime(0.9, t0 + 0.01);
    this.echoSend.gain.setValueAtTime(0.9, Math.max(t0 + 0.01, until - 0.01));
    this.echoSend.gain.linearRampToValueAtTime(0, until);
  }

  private param(t: Target): AudioParam {
    switch (t) {
      case "fader":
        return this.fader.gain;
      case "vox":
        return this.stem.vox.gain;
      case "inst":
        return this.stem.inst.gain;
      case "low":
        return this.eq.low.gain;
      case "high":
        return this.eq.high.gain;
      case "hp":
        return this.filterHp.frequency;
      case "lp":
        return this.filterLp.frequency;
    }
  }

  tickMeter(nowMs: number, dtMs: number): MeterState {
    this.analyser.getFloatTimeDomainData(this.meterBuf);
    this.meter = stepMeter(this.meter, this.meterBuf, nowMs, dtMs);
    return this.meter;
  }

  private gridOrThrow(): GridSpec {
    const g = this.grid;
    if (!g) throw new Error(`deck ${this.id} has no track`);
    return g;
  }
}
