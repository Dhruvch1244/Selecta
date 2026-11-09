// The audio graph: two decks into a mix bus, a brick-wall-ish limiter, master
// volume and master metering. The UI talks to this object only; it never
// creates audio nodes itself.

import { Deck, type DeckId } from "./deck";
import { type MeterState, newMeter, stepMeter } from "./meter";

export class Engine {
  readonly ctx: AudioContext;
  readonly decks: Record<DeckId, Deck>;
  readonly limiterReady: Promise<void>;
  /** The limiter worklet is in the signal path (false: it failed to load and the mix runs unlimited). */
  limiterActive = false;
  /** Master one-shots (horns, risers, impacts) join the mix before the limiter. */
  readonly fxBus: GainNode;
  master: MeterState = newMeter();
  private readonly mix: GainNode;
  private readonly volume: GainNode;
  private readonly analyser: AnalyserNode;
  private readonly buf: Float32Array<ArrayBuffer>;

  constructor(ctx = new AudioContext({ latencyHint: "interactive" })) {
    this.ctx = ctx;
    this.mix = ctx.createGain();
    this.volume = ctx.createGain();
    this.volume.gain.value = 0.85;
    this.analyser = ctx.createAnalyser();
    this.analyser.fftSize = 1024;
    this.buf = new Float32Array(this.analyser.fftSize);
    // Mix -> brickwall limiter (-1 dBFS ceiling) -> master volume. Until the
    // worklet module has loaded (a few ms) the mix runs straight to the
    // volume, so nothing is ever silent waiting on it.
    this.mix.connect(this.volume).connect(ctx.destination);
    this.fxBus = ctx.createGain();
    this.fxBus.gain.value = 0.7;
    this.fxBus.connect(this.mix);
    this.limiterReady = loadLimiter(ctx).then((lim) => {
      this.mix.disconnect(this.volume);
      this.mix.connect(lim).connect(this.volume);
      this.limiterActive = true;
    }).catch((e) => console.warn("limiter unavailable, running unlimited:", e));
    this.volume.connect(this.analyser);
    this.decks = { A: new Deck(ctx, "A", this.mix), B: new Deck(ctx, "B", this.mix) };
  }

  /** Browsers start contexts suspended until a user gesture. */
  async resume(): Promise<void> {
    if (this.ctx.state !== "running") await this.ctx.resume();
    await this.limiterReady;
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

/** Load the limiter worklet module and create a node (works for offline contexts too). */
export async function loadLimiter(ctx: BaseAudioContext, ceilingDb = -1): Promise<AudioWorkletNode> {
  await ctx.audioWorklet.addModule(new URL("./limiter-worklet.js", import.meta.url));
  await ctx.audioWorklet.addModule(new URL("./fx-worklets.js", import.meta.url));
  const node = new AudioWorkletNode(ctx, "selecta-limiter", { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [2] });
  node.parameters.get("ceiling")!.value = ceilingDb;
  return node;
}
