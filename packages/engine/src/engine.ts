// The audio graph: two decks into a mix bus, a brick-wall-ish limiter, master
// volume and master metering. The UI talks to this object only; it never
// creates audio nodes itself.

import { Deck, type DeckId } from "./deck";
import { type MeterState, newMeter, stepMeter } from "./meter";

export class Engine {
  readonly ctx: AudioContext;
  readonly decks: Record<DeckId, Deck>;
  master: MeterState = newMeter();
  private readonly mix: GainNode;
  private readonly volume: GainNode;
  private readonly analyser: AnalyserNode;
  private readonly buf: Float32Array<ArrayBuffer>;

  constructor(ctx = new AudioContext({ latencyHint: "interactive" })) {
    this.ctx = ctx;
    this.mix = ctx.createGain();
    const limiter = ctx.createDynamicsCompressor();
    limiter.threshold.value = -3;
    limiter.knee.value = 1;
    limiter.ratio.value = 20;
    limiter.attack.value = 0.002;
    limiter.release.value = 0.1;
    this.volume = ctx.createGain();
    this.volume.gain.value = 0.85;
    this.analyser = ctx.createAnalyser();
    this.analyser.fftSize = 1024;
    this.buf = new Float32Array(this.analyser.fftSize);
    this.mix.connect(limiter).connect(this.volume).connect(ctx.destination);
    this.volume.connect(this.analyser);
    this.decks = { A: new Deck(ctx, "A", this.mix), B: new Deck(ctx, "B", this.mix) };
  }

  /** Browsers start contexts suspended until a user gesture. */
  async resume(): Promise<void> {
    if (this.ctx.state !== "running") await this.ctx.resume();
  }

  decode(bytes: ArrayBuffer): Promise<AudioBuffer> {
    return this.ctx.decodeAudioData(bytes);
  }

  setVolume(v: number): void {
    this.volume.gain.setTargetAtTime(v, this.ctx.currentTime, 0.01);
  }

  tickMeters(nowMs: number, dtMs: number): void {
    this.analyser.getFloatTimeDomainData(this.buf);
    this.master = stepMeter(this.master, this.buf, nowMs, dtMs);
    this.decks.A.tickMeter(nowMs, dtMs);
    this.decks.B.tickMeter(nowMs, dtMs);
  }

  /** Output latency the user actually hears, for the status bar. */
  get latencyMs(): number {
    return Math.round(((this.ctx.outputLatency || 0) + this.ctx.baseLatency) * 1000);
  }
}
