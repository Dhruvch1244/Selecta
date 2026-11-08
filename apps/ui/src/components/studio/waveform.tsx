import * as React from "react";
import { barOfPos, posOfBar, type Deck } from "@selecta/engine";
import { onFrame } from "@/lib/frame";
import { getConductor } from "@/lib/set-store";
import { hintProps } from "@/lib/hint";
import { cn } from "@/lib/utils";

const SPAN = 6; // seconds visible each side of the playhead

/** Canvas can't resolve CSS custom properties; `fillStyle = "var(--cyan)"` is silently ignored. */
function resolveColor(css: CSSStyleDeclaration, color: string): string {
  const m = /^var\((--[\w-]+)\)$/.exec(color);
  return m?.[1] ? css.getPropertyValue(m[1]).trim() : color;
}

function fit(cv: HTMLCanvasElement) {
  const dpr = Math.min(2, devicePixelRatio || 1);
  const w = Math.round(cv.clientWidth * dpr);
  const h = Math.round(cv.clientHeight * dpr);
  if (cv.width !== w || cv.height !== h) {
    cv.width = w;
    cv.height = h;
  }
  return { w, h, dpr };
}

/**
 * Scrolling detail waveform. FL-style single-hue body in the deck's colour
 * (low band solid, highs as a lighter core) over a beat grid where bars and
 * 8-bar phrases are visibly heavier: phrase lines are where a DJ mixes.
 */
export function DetailWave({ deck, color, className }: { deck: Deck; color: string; className?: string }) {
  const ref = React.useRef<HTMLCanvasElement>(null);
  React.useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const css = getComputedStyle(cv);
    const col = (v: string) => css.getPropertyValue(v).trim();
    return onFrame(() => {
      const { w, h, dpr } = fit(cv);
      const c = cv.getContext("2d");
      if (!c) return;
      c.clearRect(0, 0, w, h);
      const deckColor = resolveColor(css, color);
      const wv = deck.wave;
      const tr = deck.track;
      if (!wv || !tr) return;
      const a = tr.analysis;
      const pos = deck.position();
      const x0 = pos - SPAN;
      const pxs = w / (2 * SPAN);
      const mid = h / 2;
      const ink = col("--ink");
      for (let x = 0; x < w; x += dpr) {
        const i = Math.floor((x0 + x / pxs) * wv.fps);
        if (i < 0 || i >= wv.peak.length) continue;
        const lo = (wv.low[i] ?? 0) * mid * 0.92;
        const pk = (wv.peak[i] ?? 0) * mid * 0.92;
        const hi = (wv.high[i] ?? 0) * mid * 0.7;
        c.globalAlpha = 0.28;
        c.fillStyle = deckColor;
        c.fillRect(x, mid - pk, dpr, 2 * pk);
        c.globalAlpha = 0.95;
        c.fillRect(x, mid - lo, dpr, 2 * lo);
        c.globalAlpha = 0.55;
        c.fillStyle = ink;
        c.fillRect(x, mid - hi / 2, dpr, hi);
      }
      c.globalAlpha = 1;
      const g = { beat: a.beat, downbeat0: a.downbeat0 };
      const b0 = Math.floor(barOfPos(g, x0) * 4);
      const b1 = Math.ceil(barOfPos(g, x0 + 2 * SPAN) * 4);
      c.font = `${10 * dpr}px ${col("--font-mono")}`;
      for (let b = b0; b <= b1; b++) {
        const x = (posOfBar(g, b / 4) - x0) * pxs;
        const isBar = b % 4 === 0;
        const isPhrase = b % 32 === 0;
        c.fillStyle = isPhrase ? col("--violet") : ink;
        c.globalAlpha = isPhrase ? 0.9 : isBar ? 0.38 : 0.12;
        c.fillRect(x, 0, isPhrase ? 2 * dpr : dpr, isBar ? h : h * 0.16);
        if (isBar) {
          c.globalAlpha = 0.55;
          c.fillText(String(b / 4), x + 3 * dpr, 11 * dpr);
        }
      }
      c.globalAlpha = 1;
      c.fillStyle = col("--warm");
      for (const d of a.drops) {
        const x = (posOfBar(g, d) - x0) * pxs;
        if (x > -8 && x < w + 8) {
          c.beginPath();
          c.moveTo(x - 5 * dpr, h);
          c.lineTo(x + 5 * dpr, h);
          c.lineTo(x, h - 7 * dpr);
          c.fill();
        }
      }
      // The director's effect cues for this track: a bar along the top for each.
      const plan = getConductor()?.plan;
      const cues = plan?.tracks.find((t) => t.id === tr.id)?.fx ?? [];
      c.font = `${9 * dpr}px ${col("--font-mono")}`;
      for (const cue of cues) {
        const x = (posOfBar(g, cue.bar) - x0) * pxs;
        const xe = (posOfBar(g, cue.bar + cue.beats / 4) - x0) * pxs;
        if (xe < 0 || x > w) continue;
        c.fillStyle = col("--accent");
        c.globalAlpha = 0.85;
        c.fillRect(x, 14 * dpr, Math.max(2 * dpr, xe - x), 3 * dpr);
        c.globalAlpha = 0.9;
        c.fillText(cue.fx.replace(":", " "), x + 2 * dpr, 26 * dpr);
      }
      c.globalAlpha = 1;
      c.fillStyle = ink;
      c.fillRect(w / 2 - dpr, 0, 2 * dpr, h);
    });
  }, [deck, color]);
  return <canvas ref={ref} aria-hidden className={cn("block w-full rounded-sm bg-muted", className)} />;
}

/** Whole-track strip: click jumps to the nearest bar, so a seek never lands off-grid. */
export function OverviewWave({ deck, color, onSeek, className }: { deck: Deck; color: string; onSeek: (pos: number) => void; className?: string }) {
  const ref = React.useRef<HTMLCanvasElement>(null);
  React.useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const css = getComputedStyle(cv);
    const col = (v: string) => css.getPropertyValue(v).trim();
    return onFrame(() => {
      const { w, h, dpr } = fit(cv);
      const c = cv.getContext("2d");
      if (!c) return;
      c.clearRect(0, 0, w, h);
      const tr = deck.track;
      if (!tr) return;
      const a = tr.analysis;
      const peak = a.wave.peak;
      const n = peak.length;
      c.fillStyle = resolveColor(css, color);
      c.globalAlpha = 0.75;
      for (let x = 0; x < w; x++) {
        const v = ((peak[Math.floor((x / w) * n)] ?? 0) / 255) * h;
        c.fillRect(x, (h - v) / 2, 1, v);
      }
      c.globalAlpha = 1;
      const bx = (bar: number) => (posOfBar({ beat: a.beat, downbeat0: a.downbeat0 }, bar) / a.duration) * w;
      c.fillStyle = col("--warm");
      for (const d of a.drops) c.fillRect(bx(d), 0, dpr, h);
      const px = (deck.position() / a.duration) * w;
      c.fillStyle = col("--background");
      c.globalAlpha = 0.45;
      c.fillRect(0, 0, px, h);
      c.globalAlpha = 1;
      c.fillStyle = col("--ink");
      c.fillRect(px - dpr / 2, 0, 1.5 * dpr, h);
    });
  }, [deck, color]);
  return (
    <canvas
      ref={ref}
      {...hintProps({ label: "Track overview", value: "Click to jump (snaps to the nearest bar)" })}
      onClick={(e) => {
        const tr = deck.track;
        if (!tr) return;
        const r = e.currentTarget.getBoundingClientRect();
        const pos = ((e.clientX - r.left) / r.width) * tr.analysis.duration;
        const g = { beat: tr.analysis.beat, downbeat0: tr.analysis.downbeat0 };
        onSeek(posOfBar(g, Math.max(0, Math.round(barOfPos(g, pos)))));
      }}
      className={cn("block w-full cursor-pointer rounded-sm bg-muted", className)}
    />
  );
}
