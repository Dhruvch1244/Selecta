// Effect families. The catalog (ids, categories, default lengths) lives in
// Rust (crates/selecta-core/src/fx.rs) and is exported as fx-catalog.json; each
// family is implemented once here. Three shapes:
//   insert   processes the deck for [t0,t1] with its dry path muted
//   send     taps the deck into a delay/reverb whose tail rings on, post-fader
//   override replaces playback for a window in slip mode (rolls, tape, spins)
// plus master one-shots synthesised from oscillators and noise.
// Everything is scheduled on the AudioContext clock; nodes are released
// after the effect (and its tail) has finished.

import type { Deck } from "./deck";

export interface FxContext {
  ctx: BaseAudioContext;
  /** Where master one-shots go (joins the mix before the limiter). */
  master: AudioNode;
}

export interface FireOptions {
  deck: Deck;
  /** Context time to start (callers quantise to the grid). */
  at: number;
  /** Length in beats; defaults to the catalog's. */
  beats: number;
  /** For rewinds: bar to drop back in at. */
  jumpTo?: number;
}

type Impl = (fx: FxContext, o: FireOptions & { param: string; t0: number; t1: number; beat: number }) => boolean;

const frac = (p: string): number => {
  const [a, b] = p.split("/");
  return Number(a) / Number(b ?? 1);
};
const release = (ctx: BaseAudioContext, at: number, nodes: AudioNode[]) => {
  const ms = Math.max(0, (at - ctx.currentTime) * 1000 + 300);
  setTimeout(() => nodes.forEach((n) => { try { (n as AudioScheduledSourceNode).stop?.(); } catch { /* already stopped */ } n.disconnect(); }), ms);
};
const bq = (ctx: BaseAudioContext, type: BiquadFilterType, freq: number, q = 0) => {
  const f = ctx.createBiquadFilter();
  f.type = type;
  f.frequency.value = freq;
  f.Q.value = q;
  return f;
};
function lfo(ctx: BaseAudioContext, hz: number, depth: number, target: AudioParam, t0: number, t1: number, type: OscillatorType = "sine"): AudioNode[] {
  const o = ctx.createOscillator();
  o.type = type;
  o.frequency.value = hz;
  const g = ctx.createGain();
  g.gain.value = depth;
  o.connect(g).connect(target);
  o.start(t0);
  o.stop(t1 + 0.05);
  return [o, g];
}
let noiseCache: AudioBuffer | null = null;
function noise(ctx: BaseAudioContext): AudioBuffer {
  if (noiseCache && noiseCache.sampleRate === ctx.sampleRate) return noiseCache;
  const b = ctx.createBuffer(1, ctx.sampleRate * 2, ctx.sampleRate);
  const d = b.getChannelData(0);
  let seed = 1;
  for (let i = 0; i < d.length; i++) {
    seed = (seed * 16807) % 2147483647;
    d[i] = seed / 1073741823.5 - 1;
  }
  return (noiseCache = b);
}
const irCache = new Map<string, AudioBuffer>();
function impulse(ctx: BaseAudioContext, kind: string): AudioBuffer {
  const key = `${kind}@${ctx.sampleRate}`;
  const hit = irCache.get(key);
  if (hit) return hit;
  const [len, decay, bright] = ({ hall: [2.8, 2.2, 0.4], plate: [1.8, 2.6, 0.9], wash: [6, 1.3, 0.5] } as Record<string, number[]>)[kind] ?? [2, 2, 0.5];
  const n = Math.floor((len ?? 2) * ctx.sampleRate);
  const b = ctx.createBuffer(2, n, ctx.sampleRate);
  for (let ch = 0; ch < 2; ch++) {
    const d = b.getChannelData(ch);
    let lp = 0;
    let seed = 7 + ch;
    for (let i = 0; i < n; i++) {
      seed = (seed * 48271) % 2147483647;
      const x = seed / 1073741823.5 - 1;
      lp += (1 - (bright ?? 0.5) * 0.9) * (x - lp);
      d[i] = ((bright ?? 0.5) * x + (1 - (bright ?? 0.5)) * lp) * Math.pow(1 - i / n, decay ?? 2);
    }
  }
  irCache.set(key, b);
  return b;
}

/** Insert: route slotIn -> chain -> slotOut for [t0,t1] with a short crossfade, dry muted. */
function insert(fx: FxContext, deck: Deck, t0: number, t1: number, build: (input: AudioNode, keep: AudioNode[]) => AudioNode): boolean {
  const { ctx } = fx;
  const input = ctx.createGain();
  const wet = ctx.createGain();
  wet.gain.setValueAtTime(0, Math.max(ctx.currentTime, t0 - 0.006));
  wet.gain.linearRampToValueAtTime(1, t0);
  wet.gain.setValueAtTime(1, t1 - 0.006);
  wet.gain.linearRampToValueAtTime(0, t1);
  deck.slotIn.connect(input);
  const keep: AudioNode[] = [];
  build(input, keep).connect(wet).connect(deck.slotOut);
  deck.addInsert(t0, t1);
  release(ctx, t1, [input, wet, ...keep]);
  return true;
}

/** Send: tap slotIn for [t0, close] into an effect whose output rings on the post-fader tail. */
function send(fx: FxContext, deck: Deck, t0: number, close: number, tailSecs: number, level: number, build: (input: AudioNode, keep: AudioNode[]) => AudioNode): boolean {
  const { ctx } = fx;
  const tap = ctx.createGain();
  tap.gain.setValueAtTime(0, Math.max(ctx.currentTime, t0 - 0.005));
  tap.gain.linearRampToValueAtTime(level, t0);
  tap.gain.setValueAtTime(level, close);
  tap.gain.linearRampToValueAtTime(0, close + 0.01);
  deck.slotIn.connect(tap);
  const keep: AudioNode[] = [];
  build(tap, keep).connect(deck.tail);
  release(ctx, close + tailSecs, [tap, ...keep]);
  return true;
}

function delayNet(ctx: BaseAudioContext, input: AudioNode, keep: AudioNode[], time: number, feedback: number): AudioNode {
  const d = ctx.createDelay(4);
  d.delayTime.value = Math.min(3.9, Math.max(0.02, time));
  const fb = ctx.createGain();
  fb.gain.value = feedback;
  const tone = bq(ctx, "lowpass", 5500);
  const hp = bq(ctx, "highpass", 120);
  input.connect(d);
  d.connect(tone).connect(hp).connect(fb).connect(d);
  keep.push(d, fb, tone, hp);
  return d;
}

function convolver(ctx: BaseAudioContext, input: AudioNode, keep: AudioNode[], kind: string): AudioNode {
  const hp = bq(ctx, "highpass", 220);
  const cv = ctx.createConvolver();
  cv.buffer = impulse(ctx, kind);
  input.connect(hp).connect(cv);
  keep.push(hp, cv);
  return cv;
}

function loop(deck: Deck, t0: number, t1: number, lenBeats: number, gate = 0): boolean {
  const tr = deck.track;
  if (!tr) return false;
  const L = lenBeats * tr.analysis.beat;
  return deck.override(
    t0,
    t1,
    (a, b, p0) => {
      const s = deck.source(tr.buffer);
      s.loop = true;
      s.loopStart = p0;
      s.loopEnd = p0 + L;
      s.start(a, p0);
      if (gate) {
        // Stutter: chop each repetition with a short gap.
        const per = L / deck.rate;
        const g = deck.input.context.createGain();
        s.disconnect();
        s.connect(g).connect(deck.input);
        for (let t = a; t < b; t += per) {
          g.gain.setValueAtTime(1, t);
          g.gain.setValueAtTime(0, t + per * gate);
        }
      }
      return [s];
    },
    { posFn: (t, p0) => p0 + (((t - t0) * deck.rate) % L) },
  );
}

const F: Record<string, Impl> = {
  echo: (fx, o) => send(fx, o.deck, o.t0, o.t1, 6, 0.7, (i, k) => delayNet(fx.ctx, i, k, frac(o.param) * 4 * o.beat, 0.45)),
  throw: (fx, o) => send(fx, o.deck, o.t0, o.t0 + o.beat, 6, 0.9, (i, k) => delayNet(fx.ctx, i, k, frac(o.param) * 4 * o.beat, 0.55)),
  reverb: (fx, o) => send(fx, o.deck, o.t0, o.t1, 7, o.param === "wash" ? 1 : 0.6, (i, k) => convolver(fx.ctx, i, k, o.param)),
  rev_throw: (fx, o) => send(fx, o.deck, o.t0, o.t0 + o.beat, 7, 1.1, (i, k) => convolver(fx.ctx, i, k, o.param)),

  roll: (_fx, o) => loop(o.deck, o.t0, o.t1, frac(o.param)),
  stutter: (_fx, o) => loop(o.deck, o.t0, o.t1, frac(o.param) * 4, 0.55),
  roll_build: (fx, o) => {
    // 1 -> 1/2 -> 1/4 -> 1/8 -> 1/16 beat loops, accelerating into the drop, with a rising HP filter.
    const tr = o.deck.track;
    if (!tr) return false;
    const steps: [number, number][] = [[1, 0.4], [0.5, 0.25], [0.25, 0.15], [0.125, 0.1], [0.0625, 0.1]];
    const total = o.t1 - o.t0;
    const ok = o.deck.override(o.t0, o.t1, (a, _b, p0) => {
      let t = a;
      return steps.map(([len, share]) => {
        const s = o.deck.source(tr.buffer);
        s.loop = true;
        s.loopStart = p0;
        s.loopEnd = p0 + len * tr.analysis.beat;
        s.start(t, p0);
        s.stop(t + share * total);
        t += share * total;
        return s;
      });
    }, { posFn: (_t, p0) => p0 });
    if (ok) F.hp_sweep!(fx, { ...o, param: "x" });
    return ok;
  },

  hp_sweep: (fx, o) => insert(fx, o.deck, o.t0, o.t1, (i, k) => {
    const f = bq(fx.ctx, "highpass", 30, 6);
    f.frequency.setValueAtTime(30, o.t0);
    f.frequency.exponentialRampToValueAtTime(o.param === "x" ? 2500 : 5000, o.t1);
    i.connect(f);
    k.push(f);
    return f;
  }),
  lp_sweep: (fx, o) => insert(fx, o.deck, o.t0, o.t1, (i, k) => {
    const f = bq(fx.ctx, "lowpass", 20000, 6);
    const mid = (o.t0 + o.t1) / 2;
    f.frequency.setValueAtTime(20000, o.t0);
    f.frequency.exponentialRampToValueAtTime(300, mid);
    f.frequency.exponentialRampToValueAtTime(20000, o.t1);
    i.connect(f);
    k.push(f);
    return f;
  }),
  gate: (fx, o) => insert(fx, o.deck, o.t0, o.t1, (i, k) => {
    const pats: Record<string, string> = { trance: "x.x.x.x.x.x.x.x.", half: "xxxxxxxx........", offbeat: "..x...x...x...x.", chop: "x.xx.x.xx.x.xxx." };
    const pat = pats[o.param] ?? pats.trance!;
    const g = fx.ctx.createGain();
    const step = (4 * o.beat) / pat.length;
    let n = 0;
    for (let t = o.t0; t < o.t1 - 1e-4; t += step, n++) {
      const on = pat[n % pat.length] === "x" ? 1 : 0;
      g.gain.setValueAtTime(on, t);
      if (on) g.gain.setTargetAtTime(0, t + step * 0.75, 0.004);
    }
    i.connect(g);
    k.push(g);
    return g;
  }),
  flanger: (fx, o) => insert(fx, o.deck, o.t0, o.t1, (i, k) => {
    const out = fx.ctx.createGain();
    const dry = fx.ctx.createGain();
    dry.gain.value = 0.75;
    const d = fx.ctx.createDelay(0.05);
    const fb = fx.ctx.createGain();
    const [base, depth, hz, f] = o.param === "jet" ? [0.003, 0.0027, 1 / (8 * o.beat), 0.85] : [0.004, 0.0025, 0.2, 0.6];
    d.delayTime.value = base;
    fb.gain.value = f;
    i.connect(dry).connect(out);
    i.connect(d);
    d.connect(fb).connect(d);
    d.connect(out);
    k.push(out, dry, d, fb, ...lfo(fx.ctx, hz, depth, d.delayTime, o.t0, o.t1));
    return out;
  }),
  phaser: (fx, o) => insert(fx, o.deck, o.t0, o.t1, (i, k) => {
    const out = fx.ctx.createGain();
    const dry = fx.ctx.createGain();
    dry.gain.value = 0.8;
    i.connect(dry).connect(out);
    let node: AudioNode = i;
    const n = o.param === "fast" ? 6 : 4;
    for (let s = 0; s < n; s++) {
      const ap = bq(fx.ctx, "allpass", 900, 0.6);
      node.connect(ap);
      node = ap;
      k.push(ap, ...lfo(fx.ctx, o.param === "fast" ? 1.4 : 0.3, 700, ap.frequency, o.t0, o.t1));
    }
    node.connect(out);
    k.push(out, dry);
    return out;
  }),
  crush: (fx, o) => insert(fx, o.deck, o.t0, o.t1, (i, k) => {
    const n = new AudioWorkletNode(fx.ctx, "selecta-crusher", { outputChannelCount: [2] });
    n.parameters.get("bits")!.value = Number(o.param);
    n.parameters.get("factor")!.value = Number(o.param) <= 4 ? 6 : 2;
    const g = fx.ctx.createGain();
    g.gain.value = 0.8;
    i.connect(n).connect(g);
    k.push(n, g);
    return g;
  }),

  tape_stop: (_fx, o) => {
    const tr = o.deck.track;
    if (!tr) return false;
    return o.deck.override(o.t0, o.t1 + o.beat, (a, b, p0) => {
      const s = o.deck.source(tr.buffer);
      s.playbackRate.setValueAtTime(o.deck.rate, a);
      s.playbackRate.exponentialRampToValueAtTime(0.01, a + (o.t1 - o.t0));
      s.start(a, p0);
      s.stop(b);
      return [s];
    }, { posFn: (t, p0) => p0 + (t - o.t0) * 0.5 });
  },
  backspin: (_fx, o) => spin(o, "slip"),
  rewind: (_fx, o) => {
    // "Pull up!": wheel the record back with a rising-then-falling whirr, then
    // drop back in N bars earlier, on the grid (the build and drop come round again).
    const tr = o.deck.track;
    const rev = o.deck.reversed;
    if (!tr || !rev) return false;
    const back = o.jumpTo ?? Math.max(0, Math.floor(o.deck.bar(o.t0)) - frac(o.param));
    const dur = o.t1 - o.t0;
    const g = { beat: tr.analysis.beat, downbeat0: tr.analysis.downbeat0 };
    return o.deck.override(o.t0, o.t1, (a, _b, p0) => {
      const s = o.deck.source(rev, 1);
      s.playbackRate.setValueAtTime(0.3, a);
      s.playbackRate.exponentialRampToValueAtTime(5, a + dur * 0.55);
      s.playbackRate.exponentialRampToValueAtTime(0.05, a + dur * 0.95);
      s.start(a, Math.max(0, tr.buffer.duration - p0));
      s.stop(a + dur * 0.95);
      return [s];
    }, { after: { pos: g.downbeat0 + back * 4 * g.beat }, posFn: (t, p0) => p0 - (t - o.t0) * 2.5 });
  },

  riser: (fx, o) => {
    const { ctx } = fx;
    const n = ctx.createBufferSource();
    n.buffer = noise(ctx);
    n.loop = true;
    const f = bq(ctx, "bandpass", 300, 2.5);
    const g = ctx.createGain();
    f.frequency.setValueAtTime(250, o.t0);
    f.frequency.exponentialRampToValueAtTime(9000, o.t1);
    g.gain.setValueAtTime(0.0001, o.t0);
    g.gain.exponentialRampToValueAtTime(0.5, o.t1 - 0.02);
    g.gain.linearRampToValueAtTime(0, o.t1);
    n.connect(f).connect(g).connect(fx.master);
    n.start(o.t0);
    n.stop(o.t1 + 0.05);
    release(ctx, o.t1, [n, f, g]);
    return true;
  },
  impact: (fx, o) => {
    const { ctx } = fx;
    const t = o.t0;
    const nodes: AudioNode[] = [];
    if (o.param === "boom") nodes.push(...kick(ctx, fx.master, t, 1));
    const n = ctx.createBufferSource();
    n.buffer = noise(ctx);
    const f = bq(ctx, o.param === "crash" ? "highpass" : "lowpass", o.param === "crash" ? 3000 : 2500);
    const g = ctx.createGain();
    env(g.gain, t, 0.002, 0.02, o.param === "crash" ? 2.2 : 0.9, o.param === "crash" ? 0.45 : 0.4);
    const cv = ctx.createConvolver();
    cv.buffer = impulse(ctx, "hall");
    const wet = ctx.createGain();
    wet.gain.value = 0.3;
    n.connect(f).connect(g).connect(fx.master);
    g.connect(cv).connect(wet).connect(fx.master);
    n.start(t);
    n.stop(t + 2.5);
    release(ctx, t + 4, [n, f, g, cv, wet, ...nodes]);
    return true;
  },
  sub_drop: (fx, o) => {
    const { ctx } = fx;
    const osc = ctx.createOscillator();
    const g = ctx.createGain();
    osc.frequency.setValueAtTime(95, o.t0);
    osc.frequency.exponentialRampToValueAtTime(28, o.t1);
    env(g.gain, o.t0, 0.005, (o.t1 - o.t0) * 0.3, (o.t1 - o.t0) * 0.7, 0.9);
    osc.connect(g).connect(fx.master);
    osc.start(o.t0);
    osc.stop(o.t1 + 0.05);
    release(ctx, o.t1, [osc, g]);
    return true;
  },
  horn: (fx, o) => {
    const b = o.beat;
    if (o.param === "single") horn(fx, o.t0, b * 2);
    else {
      horn(fx, o.t0, b / 2.6);
      horn(fx, o.t0 + b / 2, b / 2.6);
      horn(fx, o.t0 + b, b * 1.6);
    }
    return true;
  },
  siren: (fx, o) => {
    const { ctx } = fx;
    const dur = o.t1 - o.t0;
    const osc = ctx.createOscillator();
    osc.type = "square";
    osc.frequency.value = 600;
    const sweep = lfo(ctx, 1 / o.beat, -350, osc.frequency, o.t0, o.t1, "sawtooth");
    const g = ctx.createGain();
    env(g.gain, o.t0, 0.05, dur - 0.3, 0.25, 0.09);
    const d = ctx.createDelay(2);
    d.delayTime.value = o.beat * 0.75;
    const fb = ctx.createGain();
    fb.gain.value = 0.5;
    const lp = bq(ctx, "lowpass", 3500);
    osc.connect(lp).connect(g).connect(fx.master);
    g.connect(d);
    d.connect(fb).connect(d);
    d.connect(fx.master);
    osc.start(o.t0);
    osc.stop(o.t1 + 0.05);
    release(ctx, o.t1 + 4, [osc, g, d, fb, lp, ...sweep]);
    return true;
  },
};

function spin(o: FireOptions & { t0: number; t1: number; beat: number }, after: "slip" | "stop"): boolean {
  const tr = o.deck.track;
  const rev = o.deck.reversed;
  if (!tr || !rev) return false;
  const dur = o.t1 - o.t0;
  return o.deck.override(o.t0, o.t1, (a, _b, p0) => {
    const s = o.deck.source(rev, o.deck.rate * 3);
    s.playbackRate.setValueAtTime(o.deck.rate * 3.2, a);
    s.playbackRate.exponentialRampToValueAtTime(0.02, a + dur);
    s.start(a, Math.max(0, tr.buffer.duration - p0));
    return [s];
  }, { after, posFn: (t, p0) => p0 - (t - o.t0) * 1.5 });
}

function env(p: AudioParam, t: number, a: number, hold: number, rel: number, peak: number) {
  p.setValueAtTime(0.0001, t);
  p.linearRampToValueAtTime(peak, t + a);
  p.setValueAtTime(peak, t + a + hold);
  p.exponentialRampToValueAtTime(0.0001, t + a + hold + rel);
}

function kick(ctx: BaseAudioContext, out: AudioNode, t: number, level: number): AudioNode[] {
  const o = ctx.createOscillator();
  const g = ctx.createGain();
  o.frequency.setValueAtTime(160, t);
  o.frequency.exponentialRampToValueAtTime(42, t + 0.15);
  env(g.gain, t, 0.002, 0.02, 0.3, 0.9 * level);
  o.connect(g).connect(out);
  o.start(t);
  o.stop(t + 0.6);
  return [o, g];
}

function horn(fx: FxContext, t: number, dur: number) {
  const { ctx } = fx;
  const g = ctx.createGain();
  const f = bq(ctx, "bandpass", 1400, 0.6);
  const nodes: AudioNode[] = [g, f];
  for (const [mul, det] of [[1, 0], [1, 7], [1.5, -4], [2, 3]] as const) {
    const o = ctx.createOscillator();
    o.type = "sawtooth";
    o.detune.value = det;
    o.frequency.setValueAtTime(400 * mul, t);
    o.frequency.linearRampToValueAtTime(466 * mul, t + 0.06);
    o.connect(f);
    o.start(t);
    o.stop(t + dur + 0.1);
    nodes.push(o);
  }
  env(g.gain, t, 0.015, Math.max(0.01, dur - 0.06), 0.08, 0.2);
  f.connect(g).connect(fx.master);
  release(ctx, t + dur + 0.3, nodes);
}

/** Families implemented here, for the catalog-coverage test. */
export const FAMILIES = Object.keys(F);

/**
 * Fire one effect by catalog id (`family:param`). Returns false if the family
 * is unknown or the deck can't take it right now (e.g. already mid-roll).
 */
export function fire(fx: FxContext, id: string, o: FireOptions): boolean {
  const [family, param = ""] = id.split(":");
  const impl = F[family ?? ""];
  const tr = o.deck.track;
  if (!impl || !tr) return false;
  const beat = tr.analysis.beat / o.deck.rate;
  const t0 = Math.max(o.at, fx.ctx.currentTime + 0.01);
  try {
    return impl(fx, { ...o, param, t0, t1: t0 + o.beats * beat, beat });
  } catch (e) {
    console.warn("fx failed", id, e);
    return false;
  }
}
