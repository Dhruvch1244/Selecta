import * as React from "react";
import { meterPos, type MeterState } from "@selecta/engine";
import { onFrame } from "@/lib/frame";
import { hintProps } from "@/lib/hint";
import { cn } from "@/lib/utils";

export interface MeterProps {
  /** Read every frame; return null when there is nothing to meter. */
  read: () => MeterState | null;
  onResetClip?: () => void;
  label: string;
  className?: string;
}

/**
 * Peak + RMS bar with a peak-hold tick and a latching clip LED, drawn on a
 * canvas from the shared frame loop. Colours run cool -> warm -> clip using
 * the theme's own hues, so the meter reskins with everything else.
 */
export function Meter({ read, onResetClip, label, className }: MeterProps) {
  const ref = React.useRef<HTMLCanvasElement>(null);
  const [clipped, setClipped] = React.useState(false);

  React.useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const css = getComputedStyle(cv);
    const col = (v: string) => css.getPropertyValue(v).trim();
    return onFrame(() => {
      const dpr = Math.min(2, devicePixelRatio || 1);
      const w = cv.clientWidth * dpr;
      const h = cv.clientHeight * dpr;
      if (cv.width !== w || cv.height !== h) {
        cv.width = w;
        cv.height = h;
      }
      const c = cv.getContext("2d");
      if (!c) return;
      c.clearRect(0, 0, w, h);
      const m = read();
      if (!m) return;
      const grad = c.createLinearGradient(0, h, 0, 0);
      grad.addColorStop(0, col("--cyan"));
      grad.addColorStop(0.72, col("--cyan"));
      grad.addColorStop(0.88, col("--warm"));
      grad.addColorStop(1, col("--destructive"));
      const pk = meterPos(m.peakDb) * h;
      const rms = meterPos(m.rmsDb) * h;
      c.globalAlpha = 0.45;
      c.fillStyle = grad;
      c.fillRect(0, h - pk, w, pk);
      c.globalAlpha = 1;
      c.fillRect(0, h - rms, w, rms);
      const hold = meterPos(m.holdDb) * h;
      c.fillStyle = col("--ink");
      c.fillRect(0, h - hold - dpr, w, dpr * 1.5);
      if (m.clipped !== clipped) setClipped(m.clipped);
    });
  }, [read, clipped]);

  return (
    <div className={cn("flex h-full w-2.5 flex-col gap-1", className)}>
      <button
        type="button"
        aria-label={`${label} clip indicator${clipped ? ", clipped" : ""}`}
        onClick={onResetClip}
        {...hintProps({ label: `${label} clip LED`, value: clipped ? "Clipped (click to reset)" : "OK" })}
        className={cn("h-1.5 w-full shrink-0 rounded-sm", clipped ? "bg-destructive" : "bg-muted")}
      />
      <canvas ref={ref} aria-hidden className="min-h-0 w-full flex-1 rounded-sm bg-muted" />
    </div>
  );
}
