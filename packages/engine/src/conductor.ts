// Performs a set plan on the two decks.
//
// Bar-driven, not wall-clock-driven: every tick it asks the playing deck
// "when is bar N?" and schedules what falls inside the lookahead, so the
// script follows whatever the deck actually does (a manual seek, a later
// PR's loop roll). Track audio is fetched through an injected `loadTrack`,
// so this package never touches Tauri or the network itself.

import type { Layer, SetPlan, SetTrack } from "@selecta/protocol";
import { type Deck, type DeckId, EQ_KILL_DB, type LayerDeckId, type LoadedTrack } from "./deck";
import type { Engine } from "./engine";
import { matchRate, posOfBar } from "./grid";
import { fire, type FxContext } from "./fx";
import { keylockSemitones, shiftPitch } from "./keylock";
import { scheduleTransition, type TransitionSchedule } from "./schedule";

const LOOKAHEAD = 1.2; // seconds: enough to schedule a transition sample-accurately
const TICK_MS = 40;

export type ConductorEvent =
  | { type: "track"; index: number; deck: DeckId }
  | { type: "transition"; index: number; deck: DeckId; kind: string; rate: number }
  | { type: "postponed"; index: number }
  | { type: "fx"; index: number; fx: string; reason: string }
  | { type: "layer"; index: number; deck: LayerDeckId; layer: Layer }
  | { type: "end" }
  | { type: "error"; message: string };

export interface ConductorHost {
  /** Audio for a set track, or for a layer's source (only these four fields are set). */
  loadTrack: (t: Pick<SetTrack, "id" | "path" | "artist" | "title">) => Promise<LoadedTrack>;
  onEvent?: (e: ConductorEvent) => void;
}

/** A layer's loop, rendered to the host's tempo and key, ready for a loop deck. */
interface LayerAudio {
  track: LoadedTrack;
  /** Loop start and length in the rendered buffer (seconds). */
  a: number;
  len: number;
}

/** A/B deck id (conductor events about the main decks). */
const ab = (d: Deck): DeckId => d.id as DeckId;

interface Pending {
  out: Deck;
  inn: Deck;
  index: number;
  sched: TransitionSchedule;
  handedOver: boolean;
}

export class Conductor {
  plan: SetPlan | null = null;
  index = 0;
  private live: Deck | null = null;
  private loaded = new Map<number, Promise<LoadedTrack>>();
  private pending: Pending | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private ending = false;
  /** Cue keys already fired for the current track ("index:position"). */
  private fired = new Set<string>();
  private readonly fx: FxContext;
  /** Pitch-correct tempo changes (and apply planned key shifts) while preparing tracks. */
  keylock = false;
  /** Tracks whose audio failed to load: skipped when their turn comes. */
  private failed = new Set<SetTrack>();
  /** Each track's layer loops, rendered while the track itself is prepared (null = unavailable). */
  private layerAudio = new WeakMap<SetTrack, (LayerAudio | null | undefined)[]>();
  /** Until when each loop deck is taken (a layer is booked a lookahead before it sounds). */
  private layerBusy = new Map<Deck, number>();

  constructor(
    private readonly engine: Engine,
    private readonly host: ConductorHost,
  ) {
    this.fx = { ctx: engine.ctx, master: engine.fxBus };
  }

  get running(): boolean {
    return this.timer !== null;
  }
  /** The deck the audience is hearing as "the" track (switches at the handover point). */
  get leadDeck(): Deck | null {
    return this.pending?.handedOver ? this.pending.inn : this.live;
  }

  async start(plan: SetPlan): Promise<void> {
    this.stop();
    if (!plan.tracks.length) throw new Error("This set has no tracks.");
    await this.engine.resume();
    this.plan = plan;
    this.index = 0;
    this.loaded.clear();
    this.fired.clear();
    this.ending = false;
    // A first track that won't load is dropped, not a dead set.
    let track: LoadedTrack | null = null;
    while (plan.tracks.length && !track) {
      track = await this.prepare(0).catch(() => null);
      if (!track) {
        plan.tracks.splice(0, 1);
        this.loaded.clear();
      }
    }
    if (!track) throw new Error("None of this set's tracks could be loaded.");
    const first = plan.tracks[0]!;
    first.trans = { ...first.trans, kind: "start" };
    const deck = this.engine.decks.A;
    this.engine.decks.B.stop();
    deck.load(track);
    deck.resetMix();
    deck.setRate(first.rate);
    const t = this.engine.ctx.currentTime + 0.15;
    deck.fader.gain.setValueAtTime(0, t);
    deck.fader.gain.linearRampToValueAtTime(1, t + 1.5);
    deck.play(t, posOfBar(deck.grid!, first.inBar));
    this.live = deck;
    this.host.onEvent?.({ type: "track", index: 0, deck: ab(deck) });
    void this.prepare(1);
    this.timer = setInterval(() => this.tick(), TICK_MS);
  }

  /**
   * Swap in a new plan from track `from` onwards while playing (the AI's
   * curated plan replacing the instant draft). Tracks up to `from` must be
   * the same ones; `from`'s cue points may change (its exit), later tracks
   * are replaced. Refused if the set already moved past `from`.
   */
  adopt(plan: SetPlan, from: number): boolean {
    const cur = this.plan;
    if (!cur || this.index > from || (this.pending && this.pending.index > from)) return false;
    for (let i = 0; i <= from; i++) if (cur.tracks[i]?.id !== plan.tracks[i]?.id) return false;
    // Same objects up to `from` (pending callbacks compare identity); fresh data.
    Object.assign(cur.tracks[from]!, plan.tracks[from]);
    cur.tracks.splice(from + 1, cur.tracks.length, ...plan.tracks.slice(from + 1));
    Object.assign(cur, { ...plan, tracks: cur.tracks });
    for (const i of [...this.loaded.keys()]) if (i > from) this.loaded.delete(i);
    void this.prepare(this.index + 1).catch(() => {});
    return true;
  }

  stop(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    this.pending = null;
    for (const d of [...Object.values(this.engine.decks), ...this.engine.layers]) d.stop();
    this.layerBusy.clear();
  }

  /** Jump to the next track now: an echo-out on the next bar. */
  async skip(): Promise<void> {
    const plan = this.plan;
    const live = this.live;
    if (!plan || !live || this.pending || this.index + 1 >= plan.tracks.length) return;
    await this.prepare(this.index + 1);
    const b = Math.ceil(live.bar() + 0.3);
    const cur = plan.tracks[this.index]!;
    const next = plan.tracks[this.index + 1]!;
    cur.outBar = b + 1;
    next.trans = { kind: "echo_out", fromBar: b, len: 1, enterAt: b + 1, swapAt: null, echoDiv: 0.25, voxOut: null, loopFrom: null, loopBars: null };
  }

  private prepare(i: number): Promise<LoadedTrack> {
    const t = this.plan?.tracks[i];
    if (!t) return Promise.reject(new Error("no such track"));
    let p = this.loaded.get(i);
    if (!p) {
      p = this.host.loadTrack(t).then((track) => this.keylocked(t, track));
      // Its layers' loops render alongside (a whole track ahead of use).
      void p.then(() => this.prepareLayers(t)).catch(() => {});
      p.catch((e) => {
        this.failed.add(t);
        this.host.onEvent?.({ type: "error", message: `Couldn't load ${t.title}: ${e instanceof Error ? e.message : e}. Skipping it.` });
      });
      this.loaded.set(i, p);
    }
    return p;
  }

  /**
   * The track as it should sound in this set: pitch-shifted by the planned
   * rate (so the tempo change keeps its key) plus its harmonic key shift.
   * Only the part the set plays is rendered, during preparation, which
   * happens a whole track ahead. Any failure plays the original instead.
   */
  private async keylocked(t: SetTrack, track: LoadedTrack): Promise<LoadedTrack> {
    const semis = keylockSemitones(t.rate, t.keyShift ?? 0);
    if (!this.keylock || Math.abs(semis) < 0.01) return track;
    const g = { beat: track.analysis.beat, downbeat0: track.analysis.downbeat0 };
    const first = Math.min(t.inBar, t.trans.loopFrom ?? t.inBar);
    const from = posOfBar(g, Math.max(0, first - 2)) - 1;
    const to = posOfBar(g, t.outBar + 24);
    try {
      return { ...track, buffer: await shiftPitch(track.buffer, semis, { from, to }) };
    } catch (e) {
      console.warn("keylock render failed, playing the original:", e);
      return track;
    }
  }

  private prepareLayers(t: SetTrack): void {
    if (!t.layers?.length || this.layerAudio.has(t)) return;
    const slots: (LayerAudio | null | undefined)[] = t.layers.map(() => undefined);
    this.layerAudio.set(t, slots);
    // One at a time: renders share the CPU with decoding the next track.
    void t.layers.reduce<Promise<void>>(
      (chain, l, k) =>
        chain.then(async () => {
          try {
            slots[k] = await this.renderLayer(t, l, await this.host.loadTrack(l));
          } catch (e) {
            console.warn(`layer ${l.title} unavailable:`, e);
            slots[k] = null;
          }
        }),
      Promise.resolve(),
    );
  }

  /**
   * Cut a layer's loop (plus a margin) out of its source and render it to
   * the host's key: pitch-corrected for the rate it will play at, plus the
   * planned key shift. Always rendered (keylock or not): a loop riding a
   * track at the wrong pitch is a clash, not a tempo change.
   */
  private async renderLayer(host: SetTrack, l: Layer, src: LoadedTrack): Promise<LayerAudio | null> {
    const g = { beat: src.analysis.beat, downbeat0: src.analysis.downbeat0 };
    const a0 = posOfBar(g, l.srcBar);
    const len = posOfBar(g, l.srcBar + l.srcBars) - a0;
    const b = src.buffer;
    if (l.part === "vocal" && b.numberOfChannels < 4) return null; // the vocal stem is the point
    const pad = 0.5;
    const s0 = Math.max(0, Math.floor((a0 - pad) * b.sampleRate));
    const s1 = Math.min(b.length, Math.ceil((a0 + len + pad) * b.sampleRate));
    if (s1 - s0 < len * b.sampleRate) return null;
    const part = new AudioBuffer({ numberOfChannels: b.numberOfChannels, length: s1 - s0, sampleRate: b.sampleRate });
    for (let c = 0; c < b.numberOfChannels; c++) part.copyToChannel(b.getChannelData(c).subarray(s0, s1), c);
    const hostBeat = 60 / (host.bpm * host.rate);
    const rate = src.analysis.beat / (l.beatRatio * hostBeat);
    const buffer = await shiftPitch(part, keylockSemitones(rate, l.keyShift));
    const a = a0 - s0 / b.sampleRate;
    const bar = 4 * g.beat;
    return { track: { ...src, buffer, analysis: { ...src.analysis, downbeat0: a - Math.floor(a / bar) * bar } }, a, len };
  }

  /**
   * Start a layer on a free loop deck, phrase-locked to the host deck: its
   * rate is taken from what the host is really doing, so it can't drift.
   * Vocals come in through a quick high-pass sweep with the host's own
   * vocal ducked; riffs slam in on the one with the host's bass swapped
   * out. Both leave on an echo of their last beat.
   */
  private playLayer(index: number, host: Deck, l: Layer, audio: LayerAudio, t0: number, t1: number): void {
    const now = this.engine.ctx.currentTime;
    const d = this.engine.layers.find((x) => (this.layerBusy.get(x) ?? 0) <= Math.max(now, t0 - 0.01));
    if (!d || !host.grid || t1 <= t0) return;
    if (l.part === "vocal" && audio.track.buffer.numberOfChannels < 4) return;
    this.layerBusy.set(d, t1 + 0.05);
    const hostBeat = host.grid.beat / host.rate;
    d.load(audio.track);
    d.resetMix(now);
    d.setRate(audio.track.analysis.beat / (l.beatRatio * hostBeat));
    const vocal = l.part === "vocal";
    const moves: Parameters<Deck["automate"]>[0] = [];
    if (vocal) {
      moves.push({ side: "in", target: "inst", at: t0, value: 0 });
      moves.push({ side: "in", target: "hp", at: t0, value: 1800 }, { side: "in", target: "hp", at: t0, value: 120, until: t0 + hostBeat });
      moves.push({ side: "in", target: "fader", at: t0, value: 0.9 });
    } else {
      if (d.hasStems) moves.push({ side: "in", target: "vox", at: t0, value: 0 });
      // A riff is a bass swap: the loop brings its bass and mids, the host
      // keeps its drums' top. The loop's own hats are cut (two sets of hats
      // smeared the groove), and it sits under the host: at full level it
      // measured ~7 dB over the bass-swapped host and buried it.
      moves.push({ side: "in", target: "high", at: t0, value: EQ_KILL_DB }, { side: "in", target: "fader", at: t0, value: 0.7 });
    }
    d.automate(moves);
    d.playLoop(t0, audio.a, audio.len, t1);
    d.echo(t1 - hostBeat, t1, hostBeat * 0.75);
    // The host makes room: its bass for a riff, its vocal for a vocal.
    const hostMoves: Parameters<Deck["automate"]>[0] = vocal
      ? host.hasStems
        ? [
            { side: "out", target: "vox", at: t0 - hostBeat, value: 1 },
            { side: "out", target: "vox", at: t0 - hostBeat, value: 0.2, until: t0 },
            { side: "out", target: "vox", at: t1, value: 0.2 },
            { side: "out", target: "vox", at: t1, value: 1, until: t1 + hostBeat },
          ]
        : []
      : [
          { side: "out", target: "low", at: t0, value: EQ_KILL_DB },
          { side: "out", target: "low", at: t1, value: 0 },
        ];
    host.automate(hostMoves);
    this.host.onEvent?.({ type: "layer", index, deck: d.id as LayerDeckId, layer: l });
  }

  private tick(): void {
    const plan = this.plan;
    const live = this.live;
    if (!plan || !live?.track) return;
    const now = this.engine.ctx.currentTime;

    if (this.pending) {
      const p = this.pending;
      if (!p.handedOver && now >= p.sched.handoverAt) {
        p.handedOver = true;
        this.host.onEvent?.({ type: "track", index: p.index, deck: ab(p.inn) });
      }
      if (now >= p.sched.outEndsAt + 0.1) {
        p.out.stop();
        this.live = p.inn;
        this.index = p.index;
        this.pending = null;
        this.loaded.delete(p.index - 1);
        void this.prepare(this.index + 1).catch(() => {});
      }
      return;
    }

    const cur = plan.tracks[this.index]!;
    // The director's cues for the playing track, scheduled within the lookahead.
    cur.fx.forEach((cue, k) => {
      const key = `${this.index}:${k}`;
      if (this.fired.has(key)) return;
      const t = live.timeOfBar(cue.bar);
      if (t - now > LOOKAHEAD) return;
      this.fired.add(key);
      if (t < now - 0.05) return; // missed (e.g. after a seek): skip rather than fire late
      if (fire(this.fx, cue.fx, { deck: live, at: t, beats: cue.beats })) this.host.onEvent?.({ type: "fx", index: this.index, fx: cue.fx, reason: cue.reason });
    });
    // Layers: other songs' loops over this track, scheduled like its cues.
    const audio = this.layerAudio.get(cur);
    cur.layers?.forEach((l, k) => {
      const key = `${this.index}:L${k}`;
      if (this.fired.has(key)) return;
      const t0 = live.timeOfBar(l.fromBar);
      if (t0 - now > LOOKAHEAD) return;
      this.fired.add(key);
      const got = audio?.[k];
      if (t0 < now - 0.05 || !got) {
        if (got === undefined) this.host.onEvent?.({ type: "error", message: `${l.title} loop wasn't ready in time; skipped it.` });
        return;
      }
      this.playLayer(this.index, live, l, got, t0, live.timeOfBar(l.toBar));
    });
    const next = plan.tracks[this.index + 1];
    if (!next) {
      if (!this.ending && live.timeOfBar(cur.outBar) - now < LOOKAHEAD) this.finish(live, cur);
      return;
    }
    // The next track couldn't be loaded: drop it and come into the one after
    // with an echo-out on the next phrase (any planned move assumed the dropped track).
    if (this.failed.has(next)) {
      plan.tracks.splice(this.index + 1, 1);
      this.loaded.delete(this.index + 1);
      for (const [k, v] of [...this.loaded.entries()].sort((a, b) => a[0] - b[0])) {
        if (k > this.index + 1) {
          this.loaded.delete(k);
          this.loaded.set(k - 1, v);
        }
      }
      const after = plan.tracks[this.index + 1];
      if (after) {
        const b = Math.ceil((live.bar() + 4) / 4) * 4;
        cur.outBar = b + 1;
        after.trans = { kind: "echo_out", fromBar: b, len: 1, enterAt: b + 1, swapAt: null, echoDiv: 0.25, voxOut: null, loopFrom: null, loopBars: null };
        void this.prepare(this.index + 1).catch(() => {});
      }
      return;
    }
    const tFrom = live.timeOfBar(next.trans.fromBar);
    if (tFrom - now > LOOKAHEAD) return;
    const ready = this.loaded.get(this.index + 1);
    void ready?.then((track) => {
      if (this.pending || this.plan !== plan || this.live !== live || plan.tracks[this.index + 1] !== next) return;
      this.transition(live, track, this.index + 1);
    });
    // Not decoded in time: give the current track another phrase rather than cut early.
    if (tFrom - now < 0.25) {
      for (const k of ["fromBar", "enterAt"] as const) next.trans[k] += 8;
      if (next.trans.swapAt !== null) next.trans.swapAt += 8;
      if (next.trans.voxOut != null) next.trans.voxOut += 8;
      cur.outBar += 8;
      // About to run out of track while waiting: hold the current phrase in
      // a 4-bar loop on the grid (as a DJ would) instead of playing into silence.
      const bars = live.track.analysis.bars;
      if (cur.outBar + 8 >= bars && !live.loop) live.setLoop(16);
      this.host.onEvent?.({ type: "postponed", index: this.index + 1 });
    }
  }

  private transition(out: Deck, track: LoadedTrack, index: number): void {
    const plan = this.plan!;
    const next = plan.tracks[index]!;
    const inn = out === this.engine.decks.A ? this.engine.decks.B : this.engine.decks.A;
    let trans = next.trans;
    const beatmatched = ["blend", "bassswap", "double_drop", "filter", "acapella_build", "bootleg", "mashup"].includes(trans.kind);
    // A stem move needs stems on both decks (a stem can fail to load, or the
    // plan predates a re-scan): degrade to the plain move with the same timing.
    if (["acapella_build", "bootleg", "mashup"].includes(trans.kind) && (track.buffer.numberOfChannels < 4 || !out.hasStems)) {
      trans = { ...trans, kind: trans.kind === "mashup" ? "double_drop" : "bassswap" };
    }
    // Mid-track effects on the outgoing deck must not fight its exit move.
    for (let k = 0; k < (plan.tracks[index - 1]?.fx.length ?? 0); k++) this.fired.add(`${index - 1}:${k}`);
    let rate = 1;
    if (beatmatched) {
      // Recomputed live from what the outgoing deck is really doing; beyond
      // +-10% (no keylock yet) the pitch shift is ugly, so cut instead.
      const r = matchRate(out.bpm, track.analysis.bpm, 0.1);
      if (r === null) {
        const end = trans.fromBar + Math.max(1, trans.len);
        trans = { ...trans, kind: "echo_out", fromBar: end - 1, len: 1, enterAt: end, echoDiv: 0.25 };
      } else rate = r;
    }
    const sched = scheduleTransition(trans, (b) => out.timeOfBar(b), (4 * out.grid!.beat) / out.rate);
    inn.load(track);
    inn.resetMix();
    inn.setRate(rate);
    out.automate(sched.moves.filter((m) => m.side === "out"));
    inn.automate(sched.moves.filter((m) => m.side === "in"));
    if (sched.echo) out.echo(sched.echo.at, sched.echo.until, sched.echo.delaySec);
    if (sched.exitFx) {
      const beat = out.grid!.beat / out.rate;
      fire(this.fx, sched.exitFx.id, { deck: out, at: sched.exitFx.at, beats: (sched.exitFx.until - sched.exitFx.at) / beat });
    }
    const g = inn.grid!;
    const stemMove = ["acapella_build", "bootleg"].includes(trans.kind);
    if (stemMove && trans.loopFrom != null && trans.loopBars && trans.swapAt != null) {
      // Hook loop: the incoming's sung hook repeats, phrase-locked to the
      // outgoing, until the swap; then the deck jumps to its drop (slip-mode
      // override, so the grid never moves). Native buffer looping is
      // sample-accurate, so the loop can't drift against the other deck.
      const a = posOfBar(g, trans.loopFrom);
      const len = posOfBar(g, trans.loopFrom + trans.loopBars) - a;
      const t0 = sched.startIn;
      const drop = posOfBar(g, next.inBar + (trans.swapAt - trans.enterAt));
      inn.play(t0, a);
      inn.override(
        t0,
        sched.handoverAt,
        (start) => {
          const s = inn.source(inn.track!.buffer);
          s.loop = true;
          s.loopStart = a;
          s.loopEnd = a + len;
          s.start(start, a);
          return [s];
        },
        { after: { pos: drop }, posFn: (t) => a + (((t - t0) * inn.rate) % len) },
      );
    } else {
      inn.play(sched.startIn, posOfBar(g, next.inBar));
    }
    if (stemMove || trans.kind === "mashup") this.buildInto(out, sched.handoverAt);
    out.stop(sched.outEndsAt + 0.05);
    this.pending = { out, inn, index, sched, handedOver: false };
    // Start preparing the track after this one now, not when this transition
    // ends: decoding, stems and the keylock render can take 10-35 s, and a
    // long mashup can end seconds before the next move is due.
    void this.prepare(index + 1).catch(() => {});
    this.host.onEvent?.({ type: "transition", index, deck: ab(inn), kind: trans.kind, rate });
  }

  /**
   * A proper build into a stem move's swap: a riser over the last bars, the
   * outgoing high-passed away, and an impact on the one. Same layering as the
   * FX director's builds into drops.
   */
  private buildInto(out: Deck, swap: number): void {
    const barSec = (4 * out.grid!.beat) / out.rate;
    const now = this.engine.ctx.currentTime;
    const bars = swap - now > 8 * barSec + 0.05 ? 8 : swap - now > 4 * barSec + 0.05 ? 4 : 0;
    if (bars) fire(this.fx, `riser:${bars}`, { deck: out, at: swap - bars * barSec, beats: bars * 4 });
    if (swap - now > 2 * barSec + 0.05) fire(this.fx, "hp_sweep:2", { deck: out, at: swap - 2 * barSec, beats: 8 });
    fire(this.fx, "impact:boom", { deck: out, at: swap, beats: 2 });
  }

  private finish(deck: Deck, cur: SetTrack): void {
    this.ending = true;
    const end = deck.timeOfBar(cur.outBar);
    const barSec = (4 * deck.grid!.beat) / deck.rate;
    deck.automate([
      { side: "out", target: "fader", at: end - 4 * barSec, value: 1 },
      { side: "out", target: "fader", at: end - 4 * barSec, value: 0, until: end },
    ]);
    deck.stop(end + 0.05);
    const ms = Math.max(0, (end - this.engine.ctx.currentTime) * 1000 + 200);
    setTimeout(() => {
      if (this.timer) clearInterval(this.timer);
      this.timer = null;
      this.host.onEvent?.({ type: "end" });
    }, ms);
  }
}
