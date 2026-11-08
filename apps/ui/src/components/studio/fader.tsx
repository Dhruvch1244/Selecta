import { cn } from "@/lib/utils";
import { useDragValue, type DragValueOptions } from "./use-drag-value";

export interface FaderProps extends Omit<DragValueOptions, "format" | "travel"> {
  format?: (v: number) => string;
  color?: string;
  className?: string;
}

/** Vertical channel fader: groove, unity tick, flat cap with the channel colour. */
export function Fader({ color = "var(--accent)", className, format = (v) => `${Math.round(v * 100)}%`, ...opts }: FaderProps) {
  const drag = useDragValue({ ...opts, format, travel: 140 });
  const t = (opts.value - opts.min) / (opts.max - opts.min);
  return (
    <div className={cn("relative h-full w-7 cursor-ns-resize outline-none", className)} {...drag}>
      <div className="absolute inset-y-1 left-1/2 w-1 -translate-x-1/2 rounded-sm bg-muted shadow-[inset_0_1px_2px_rgb(0_0_0/0.6)]" />
      {[0, 0.25, 0.5, 0.75, 1].map((m) => (
        <div key={m} className="absolute left-0 h-px w-1.5 bg-[var(--ink-faint)]" style={{ bottom: `calc(${m} * (100% - 18px) + 9px)` }} />
      ))}
      <div
        className="absolute left-1/2 h-[18px] w-6 -translate-x-1/2 rounded-sm border border-border bg-secondary shadow-ambient"
        style={{ bottom: `calc(${t} * (100% - 18px))` }}
      >
        <div className="mx-auto mt-[7px] h-0.5 w-4 rounded-full" style={{ background: color }} />
      </div>
    </div>
  );
}
