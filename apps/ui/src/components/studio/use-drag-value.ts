import * as React from "react";
import { useHintStore } from "@/lib/hint";

/**
 * Shared direct-manipulation behaviour for knobs and faders, matching DAW
 * conventions: vertical drag (200 px = full range), Shift for fine control,
 * wheel steps, double-click resets, arrow/PageUp/Home keys for keyboard
 * users. While dragging, the live value shows in the hint bar instead of
 * a tooltip covering the control.
 */
export interface DragValueOptions {
  value: number;
  min: number;
  max: number;
  defaultValue: number;
  onChange: (v: number) => void;
  label: string;
  format: (v: number) => string;
  keys?: string;
  /** Pixels of vertical drag for the full range. */
  travel?: number;
}

export function useDragValue({ value, min, max, defaultValue, onChange, label, format, keys, travel = 200 }: DragValueOptions) {
  const drag = React.useRef<{ y: number; v: number } | null>(null);
  const span = max - min;
  const clamp = (v: number) => Math.min(max, Math.max(min, v));
  const show = (v: number) => useHintStore.getState().set({ label, value: format(v), ...(keys ? { keys } : {}) });
  const commit = (v: number) => {
    const c = clamp(v);
    onChange(c);
    show(c);
  };

  return {
    role: "slider" as const,
    tabIndex: 0,
    "aria-label": label,
    "aria-valuemin": min,
    "aria-valuemax": max,
    "aria-valuenow": Number(value.toFixed(3)),
    "aria-valuetext": format(value),
    onPointerDown: (e: React.PointerEvent<HTMLElement>) => {
      if (e.button !== 0) return;
      e.currentTarget.setPointerCapture(e.pointerId);
      drag.current = { y: e.clientY, v: value };
      show(value);
    },
    onPointerMove: (e: React.PointerEvent<HTMLElement>) => {
      const d = drag.current;
      if (!d) return;
      const fine = e.shiftKey ? 0.15 : 1;
      commit(d.v + ((d.y - e.clientY) / travel) * span * fine);
    },
    onPointerUp: (e: React.PointerEvent<HTMLElement>) => {
      drag.current = null;
      e.currentTarget.releasePointerCapture(e.pointerId);
    },
    onPointerEnter: () => show(value),
    onPointerLeave: () => {
      if (!drag.current) useHintStore.getState().set(null);
    },
    onDoubleClick: () => commit(defaultValue),
    onWheel: (e: React.WheelEvent<HTMLElement>) => commit(value - Math.sign(e.deltaY) * span * (e.shiftKey ? 0.005 : 0.03)),
    onKeyDown: (e: React.KeyboardEvent<HTMLElement>) => {
      const step = span * (e.shiftKey ? 0.005 : 0.02);
      const map: Record<string, number> = {
        ArrowUp: value + step, ArrowRight: value + step, ArrowDown: value - step, ArrowLeft: value - step,
        PageUp: value + span * 0.1, PageDown: value - span * 0.1, Home: min, End: max,
      };
      const next = map[e.key];
      if (next !== undefined) {
        e.preventDefault();
        commit(next);
      } else if (e.key === "Delete" || e.key === "Backspace") commit(defaultValue);
    },
  };
}
