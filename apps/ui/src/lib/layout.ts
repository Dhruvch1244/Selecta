// Layout preferences (per machine, so localStorage; never needed by Claude
// or other viewers). The library collapses to a rail to give the decks room,
// and by default hides itself while a set plays.

import { create } from "zustand";

const read = (k: string, d: boolean) => {
  try {
    const v = localStorage.getItem(k);
    return v === null ? d : v === "1";
  } catch {
    return d;
  }
};
const write = (k: string, v: boolean) => {
  try {
    localStorage.setItem(k, v ? "1" : "0");
  } catch {
    /* storage unavailable: preference lasts this session */
  }
};

interface Layout {
  libraryOpen: boolean;
  autoHideLibrary: boolean;
  /** The library was hidden by a set starting (so stopping the set reopens it). */
  hiddenBySet: boolean;
  toggleLibrary: () => void;
  setAutoHide: (v: boolean) => void;
  setStarted: () => void;
  setStopped: () => void;
}

export const useLayout = create<Layout>((set, get) => ({
  libraryOpen: read("selecta.libraryOpen", true),
  autoHideLibrary: read("selecta.autoHideLibrary", true),
  hiddenBySet: false,
  toggleLibrary: () => {
    const open = !get().libraryOpen;
    write("selecta.libraryOpen", open);
    set({ libraryOpen: open, hiddenBySet: false });
  },
  setAutoHide: (v) => {
    write("selecta.autoHideLibrary", v);
    set({ autoHideLibrary: v });
  },
  setStarted: () => {
    if (get().autoHideLibrary && get().libraryOpen) set({ libraryOpen: false, hiddenBySet: true });
  },
  setStopped: () => {
    if (get().hiddenBySet) set({ libraryOpen: true, hiddenBySet: false });
  },
}));
