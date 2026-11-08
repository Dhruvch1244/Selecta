// One deck: buffer playback on a beat grid, plus its mixer channel strip
// (trim, 3-band kill EQ, sweep filter, fader). Everything is scheduled on the
// AudioContext clock; nothing here uses setTimeout for audio timing.

import type { Analysis } from "@selecta/protocol";
import { type BaseLine, type GridSpec, barOfPos, nextGrid, posAt, posOfBar, timeOfPos } from "./grid";
import { type MeterState, newMeter, stepMeter } from "./meter";
import { computeWaveform, type Waveform } from "./waveform";

export type DeckId = "A" | "B";
export type Band = "low" | "mid" | "high";

export interface LoadedTrack {
  id: string;
  artist: string;
  title: string;
  buffer: AudioBuffer;
  analysis: Analysis;
}

const DECLICK = 0.004;
/** Full kill: -40 dB is inaudible under a playing deck and avoids denormal tails. */
export const EQ_KILL_DB = -40;

export class Deck {
  readonly id: DeckId;
  track: LoadedTrack | null = null;
  wave: Waveform | null = null;
  playing = false;
  meter: MeterState = newMeter();

  private readonly ctx: AudioContext;
  private src: AudioBufferSourceNode | null = null;
  private base: BaseLine = { t: 0, pos: 0, rate: 1 };
  private stoppedAt = Infinity;
  private readonly vca: GainNode;
  private readonly trimNode: GainNode;
  private readonly eq: Record<Band, BiquadFilterNode>;
  private readonly filterHp: BiquadFilterNode;
  private readonly filterLp: BiquadFilterNode;
  readonly fader: GainNode;
  private readonly analyser: AnalyserNode;
  private readonly meterBuf: Float32Array<ArrayBuffer>;

  constructor(ctx: AudioContext, id: DeckId, out: AudioNode) {
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
    // Isolator-style corner frequencies common on DJ mixers.
    this.eq = { low: band("lowshelf", 220), mid: band("peaking", 1000), high: band("highshelf", 3200) };
    this.eq.mid.Q.value = 0.7;
    this.filterHp = band("highpass", 10);
    this.filterLp = band("lowpass", 22000);
    this.filterHp.Q.value = this.filterLp.Q.value = 0.9;
    this.fader = ctx.createGain();
    this.analyser = ctx.createAnalyser();
    this.analyser.fftSize = 1024;
    this.meterBuf = new Float32Array(this.analyser.fftSize);
    this.vca
      .connect(this.trimNode)
      .connect(this.eq.low)
      .connect(this.eq.mid)
      .connect(this.eq.high)
      .connect(this.filterHp)
      .connect(this.filterLp)
      .connect(this.fader)
      .connect(out);
    this.fader.connect(this.analyser);
  }

  get grid(): GridSpec | null {
    return this.track ? { beat: this.track.analysis.beat, downbeat0: this.track.analysis.downbeat0 } : null;
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
    this.track = track;
    const chans = Array.from({ length: Math.min(2, track.buffer.numberOfChannels) }, (_, i) => track.buffer.getChannelData(i));
    this.wave = computeWaveform(chans, track.buffer.sampleRate);
    this.base = { t: this.ctx.currentTime, pos: posOfBar(this.gridOrThrow(), track.analysis.startBar), rate: 1 };
    this.stoppedAt = this.ctx.currentTime;
    // Loudness normalisation from analysis; the trim knob adds on top.
    this.setTrimDb(0);
  }

  /** Position in track seconds at context time `t` (defaults to now). Frozen while stopped. */
  position(t = this.ctx.currentTime): number {
    return posAt(this.base, Math.min(t, this.stoppedAt));
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
    if (!this.track || this.playing) return;
    const when = Math.max(at, this.ctx.currentTime);
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
        this.playing = false;
        this.stoppedAt = this.ctx.currentTime;
      }
    };
    this.src = s;
    this.playing = true;
    this.stoppedAt = Infinity;
  }

  stop(at = this.ctx.currentTime): void {
    if (!this.src) return;
    const when = Math.max(at, this.ctx.currentTime);
    this.vca.gain.setValueAtTime(1, when);
    this.vca.gain.linearRampToValueAtTime(0, when + DECLICK);
    this.src.stop(when + DECLICK);
    this.src = null;
    this.playing = false;
    this.stoppedAt = when;
  }

  /** Jump to a track position, keeping play state (quantised callers pass a grid time). */
  seek(pos: number, at = this.ctx.currentTime): void {
    const was = this.playing;
    this.stop(at);
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
    this.eq[band].gain.setTargetAtTime(Math.max(EQ_KILL_DB, db), this.ctx.currentTime, 0.008);
  }
  /** -1..1: negative sweeps a low-pass down, positive a high-pass up, 0 is bypass. */
  setFilter(v: number): void {
    const t = this.ctx.currentTime;
    const lp = v < 0 ? 22000 * Math.pow(250 / 22000, -v) : 22000;
    const hp = v > 0 ? 10 * Math.pow(6000 / 10, v) : 10;
    this.filterLp.frequency.setTargetAtTime(lp, t, 0.01);
    this.filterHp.frequency.setTargetAtTime(hp, t, 0.01);
  }
  setFader(v: number): void {
    // Equal-power-ish curve: a fader at 50% should sound half as loud, not silent.
    this.fader.gain.setTargetAtTime(Math.pow(v, 1.6), this.ctx.currentTime, 0.008);
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
