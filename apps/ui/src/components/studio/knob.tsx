import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";
import { useDragValue, type DragValueOptions } from "./use-drag-value";

const knob = cva("group relative flex select-none flex-col items-center gap-0.5 outline-none", {
  variants: { size: { sm: "w-8", md: "w-10", lg: "w-12" } },
  defaultVariants: { size: "md" },
});

const SWEEP = 270;
const START = 135; // degrees, clockwise from +x: the gap sits at the bottom

function arc(r: number, a0: number, a1: number) {
  const p = (a: number) => [20 + r * Math.cos((a * Math.PI) / 180), 20 + r * Math.sin((a * Math.PI) / 180)];
  const [x0, y0] = p(a0);
  const [x1, y1] = p(a1);
  return `M ${x0} ${y0} A ${r} ${r} 0 ${a1 - a0 > 180 ? 1 : 0} 1 ${x1} ${y1}`;
}

export interface KnobProps extends Omit<DragValueOptions, "format">, VariantProps<typeof knob> {
  /** Short caption under the knob, display face. */
  caption: string;
  format?: (v: number) => string;
  /** Arc grows from the centre (EQ, filter, pan) instead of from the minimum. */
  bipolar?: boolean;
  color?: string;
  className?: string;
}

/**
 * Rotary control. Arc colour is the owning channel's colour, so a glance at
 * a strip says which deck it belongs to. The value lives in the hint bar.
 */
export function Knob({ caption, size, bipolar, color = "var(--accent)", className, format = (v) => v.toFixed(2), ...opts }: KnobProps) {
  const drag = useDragValue({ ...opts, format });
  const { value, min, max } = opts;
  const t = (value - min) / (max - min);
  const angle = START + t * SWEEP;
  const mid = START + ((bipolar ? (0 - min) / (max - min) : 0) * SWEEP);
  const [a0, a1] = bipolar ? (angle < mid ? [angle, mid] : [mid, angle]) : [START, angle];
  return (
    <div className={cn(knob({ size }), className)} {...drag}>
      <svg viewBox="0 0 40 40" className="w-full cursor-ns-resize">
        <path d={arc(16, START, START + SWEEP)} fill="none" stroke="var(--muted)" strokeWidth="3.5" strokeLinecap="butt" />
        {Math.abs(a1 - a0) > 0.5 && <path d={arc(16, a0, a1)} fill="none" stroke={color} strokeWidth="3.5" strokeLinecap="butt" />}
        <circle cx="20" cy="20" r="11" fill="var(--secondary)" stroke="var(--rule)" strokeWidth="1" />
        <line
          x1={20 + 4 * Math.cos((angle * Math.PI) / 180)}
          y1={20 + 4 * Math.sin((angle * Math.PI) / 180)}
          x2={20 + 10 * Math.cos((angle * Math.PI) / 180)}
          y2={20 + 10 * Math.sin((angle * Math.PI) / 180)}
          stroke="var(--ink)"
          strokeWidth="2"
          strokeLinecap="round"
        />
      </svg>
      <span className="font-display text-[10px] font-medium uppercase tracking-wider text-muted-foreground group-focus-visible:text-foreground">{caption}</span>
    </div>
  );
}
