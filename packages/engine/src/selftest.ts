// In-browser self-test of the audio chain: renders real audio through the
// real Deck and limiter with an OfflineAudioContext and measures it. This is
// how sound-quality claims in PRs are backed by numbers rather than by ear.
// Run in a dev build: `await selectaSelfTest()` in the console (or via CDP).

import type { Analysis } from "@selecta/protocol";
import { Deck, type Band } from "./deck";
import { loadLimiter } from "./engine";
import { fire } from "./fx";
import catalog from "../../protocol/src/generated/fx-catalog.json";

const SR = 48000;
const db = (x: number) => 20 * Math.log10(Math.max(x, 1e-9));

function sine(ctx: BaseAudioContext, freq: number, amp: number, secs: number): AudioBuffer {
  const b = ctx.createBuffer(2, Math.round(secs * SR), SR);
  for (let ch = 0; ch < 2; ch++) {
    const d = b.getChannelData(ch);
    for (let i = 0; i < d.length; i++) d[i] = amp * Math.sin((2 * Math.PI * freq * i) / SR);
  }
  return b;
}

const fakeAnalysis = (secs: number): Analysis =>
  ({ beat: 0.5, downbeat0: 0, bpm: 120, startBar: 0, gainDb: 0, duration: secs, bars: Math.floor(secs / 2) }) as unknown as Analysis;

/** RMS of the steady middle of a rendered buffer (skips filter settling and fades). */
function rms(b: AudioBuffer): number {
  const d = b.getChannelData(0);
  const a = Math.floor(d.length * 0.4);
  const e = Math.floor(d.length * 0.9);
  let s = 0;
  for (let i = a; i < e; i++) s += (d[i] ?? 0) ** 2;
  return Math.sqrt(s / (e - a));
}

async function throughDeck(freq: number, eq: Partial<Record<Band, number>> = {}): Promise<number> {
  const secs = 0.6;
  const ctx = new OfflineAudioContext(2, secs * SR, SR);
  const deck = new Deck(ctx, "A", ctx.destination);
  deck.load({ id: "t", artist: "", title: "", buffer: sine(ctx, freq, 0.25, secs), analysis: fakeAnalysis(secs) });
  for (const [band, v] of Object.entries(eq)) deck.setEq(band as Band, v);
  deck.play(0, 0);
  const out = await ctx.startRendering();
  return db(rms(out)) - db(0.25 / Math.SQRT2);
}

async function throughLimiter(amp: number): Promise<{ peakDb: number; gainDb: number }> {
  const secs = 0.5;
  const ctx = new OfflineAudioContext(2, secs * SR, SR);
  const lim = await loadLimiter(ctx, -1);
  const src = ctx.createBufferSource();
  src.buffer = sine(ctx, 220, amp, secs);
  src.connect(lim).connect(ctx.destination);
  src.start();
  const out = await ctx.startRendering();
  let pk = 0;
  for (let ch = 0; ch < 2; ch++) for (const v of out.getChannelData(ch)) pk = Math.max(pk, Math.abs(v));
  return { peakDb: db(pk), gainDb: db(rms(out)) - db(amp / Math.SQRT2) };
}

/** A scheduled fade-in (step to 0, ramp to 1) must start silent: guards the automate() anchor bug. */
async function fadeInStartDb(): Promise<number> {
  const secs = 0.6;
  const ctx = new OfflineAudioContext(2, secs * SR, SR);
  const deck = new Deck(ctx, "A", ctx.destination);
  deck.load({ id: "t", artist: "", title: "", buffer: sine(ctx, 440, 0.25, secs), analysis: fakeAnalysis(secs) });
  deck.automate([
    { side: "in", target: "fader", at: 0, value: 0 },
    { side: "in", target: "fader", at: 0, value: 1, until: 0.5 },
  ]);
  deck.play(0, 0);
  const d = (await ctx.startRendering()).getChannelData(0);
  let s = 0;
  const n = Math.round(0.02 * SR); // first 20 ms
  for (let i = 0; i < n; i++) s += (d[i] ?? 0) ** 2;
  return db(Math.sqrt(s / n)) - db(0.25 / Math.SQRT2);
}

/**
 * Render one effect on a playing deck. Returns peak (NaN/∞ flagged), whether
 * it fired, and the RMS after the effect window, which must be audible again
 * for slip-mode overrides (the track resumes on its grid afterwards).
 */
async function renderFx(id: string, beats: number): Promise<{ fired: boolean; peak: number; afterDb: number; finite: boolean }> {
  const secs = 3;
  const ctx = new OfflineAudioContext(2, secs * SR, SR);
  await loadLimiter(ctx); // also loads the crusher worklet module
  const deck = new Deck(ctx, "A", ctx.destination);
  // The track must outlast the render, or "silent afterwards" means the file ran out, not that the effect broke it.
  deck.load({ id: "t", artist: "", title: "", buffer: sine(ctx, 220, 0.25, secs + 4), analysis: fakeAnalysis(secs + 4) });
  deck.play(0, 0.5);
  // 120 BPM: beat = 0.5 s. Effects start at 0.25 s; at most 2 s long here.
  const fired = fire({ ctx, master: ctx.destination }, id, { deck, at: 0.25, beats: Math.min(beats, 4) });
  const out = await ctx.startRendering();
  const d = out.getChannelData(0);
  let pk = 0;
  let finite = true;
  for (const v of d) {
    if (!Number.isFinite(v)) finite = false;
    else pk = Math.max(pk, Math.abs(v));
  }
  const a = Math.floor(2.6 * SR);
  let sq = 0;
  for (let i = a; i < d.length; i++) sq += (d[i] ?? 0) ** 2;
  return { fired, peak: pk, afterDb: db(Math.sqrt(sq / (d.length - a))) - db(0.25 / Math.SQRT2), finite };
}

export interface SelfTestResult {
  name: string;
  value: number;
  pass: boolean;
  expect: string;
}

export async function selfTest(): Promise<SelfTestResult[]> {
  const r: SelfTestResult[] = [];
  const add = (name: string, value: number, pass: boolean, expect: string) => r.push({ name, value: Math.round(value * 100) / 100, pass, expect });
  for (const f of [60, 150, 500, 1000, 4000, 10000]) {
    const g = await throughDeck(f);
    add(`isolator flat @ ${f} Hz (dB)`, g, Math.abs(g) <= 1.0, "within ±1 dB");
  }
  const lowKill = await throughDeck(60, { low: -40 });
  add("low kill @ 60 Hz (dB)", lowKill, lowKill <= -35, "≤ -35 dB");
  const midKill = await throughDeck(1000, { mid: -40 });
  add("mid kill @ 1 kHz (dB)", midKill, midKill <= -30, "≤ -30 dB");
  const highKill = await throughDeck(10000, { high: -40 });
  add("high kill @ 10 kHz (dB)", highKill, highKill <= -35, "≤ -35 dB");
  const lowKillMids = await throughDeck(1000, { low: -40 });
  add("low kill leaves 1 kHz (dB)", lowKillMids, Math.abs(lowKillMids) <= 1.5, "within ±1.5 dB");
  const fade = await fadeInStartDb();
  add("fade-in first 20 ms vs full (dB)", fade, fade <= -25, "≤ -25 dB (starts near silent)");
  const hot = await throughLimiter(4);
  add("limiter peak, +12 dB input (dBFS)", hot.peakDb, hot.peakDb <= -0.95, "≤ -0.95 dBFS (ceiling -1)");
  const clean = await throughLimiter(0.5);
  add("limiter transparent, -6 dB input (dB)", clean.gainDb, Math.abs(clean.gainDb) <= 0.1, "within ±0.1 dB");
  // Every effect in the catalog: fires, stays finite, and (except the ones
  // that end the deck) leaves the track playing afterwards.
  const ends = new Set(["tape_stop", "backspin", "rewind"]);
  let ok = 0;
  const bad: string[] = [];
  for (const f of catalog) {
    const res = await renderFx(f.id, f.beats);
    // "Still playing" = louder than -12 dB: a broken override measures ~-165 dB,
    // while an echo tail summing with the pure test tone can comb-cancel to ~-10.
    const resumes = ends.has(f.family) || f.target === "master" || res.afterDb > -12;
    if (res.fired && res.finite && res.peak < 4 && resumes) ok++;
    else bad.push(`${f.id}(fired=${res.fired} finite=${res.finite} peak=${res.peak.toFixed(2)} after=${res.afterDb.toFixed(1)}dB)`);
  }
  add(`effects render cleanly (${catalog.length})`, ok, ok === catalog.length, `all ${catalog.length}${bad.length ? `; failing: ${bad.join(", ")}` : ""}`);
  return r;
}
