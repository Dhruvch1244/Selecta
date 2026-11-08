import * as React from "react";
import { hintProps } from "@/lib/hint";
import { cn } from "@/lib/utils";

export interface LcdProps {
  label: string;
  /** Either a static value, or a ref the frame loop writes into (no React re-render). */
  children?: React.ReactNode;
  valueRef?: React.Ref<HTMLSpanElement>;
  width?: string;
  hint?: string;
  className?: string;
}

/** Recessed monospace readout: tempo, time, key. FL's top-bar LCDs. */
export function Lcd({ label, children, valueRef, width = "w-20", hint, className }: LcdProps) {
  return (
    <div className={cn("flex h-8 shrink-0 flex-col justify-center rounded-sm border border-border bg-muted px-2 shadow-[inset_0_1px_3px_rgb(0_0_0/0.5)]", width, className)} {...hintProps({ label, ...(hint ? { value: hint } : {}) })}>
      <span className="font-display text-[9px] uppercase leading-none tracking-wider text-[var(--ink-faint)]">{label}</span>
      <span ref={valueRef} className="truncate font-mono text-[13px] leading-tight tabular-nums text-[var(--cyan)]">
        {children}
      </span>
    </div>
  );
}
