// The hint bar (FL Studio's signature): hovering or dragging any control
// shows its name, live value and shortcut in one fixed strip, instead of
// tooltips that pop up over the thing you are trying to see.

import { create } from "zustand";

export interface Hint {
  label: string;
  value?: string;
  keys?: string;
}

interface HintState {
  hint: Hint | null;
  set: (h: Hint | null) => void;
}

export const useHintStore = create<HintState>((set) => ({ hint: null, set: (hint) => set({ hint }) }));

/** Spread onto any element: `<button {...hintProps({label: "Play", keys: "Space"})}>`. */
export function hintProps(h: Hint) {
  return {
    onPointerEnter: () => useHintStore.getState().set(h),
    onPointerLeave: () => useHintStore.getState().set(null),
    onFocus: () => useHintStore.getState().set(h),
    onBlur: () => useHintStore.getState().set(null),
  };
}
