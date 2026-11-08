import * as React from "react";
import { HintBar } from "@/components/studio/hint-bar";
import { inTauri } from "@/lib/api";
import { onFrame } from "@/lib/frame";
import { getEngine, useApp } from "@/lib/store";
import { Browser } from "@/panels/browser";
import { DeckLane } from "@/panels/deck-lane";
import { Mixer } from "@/panels/mixer";
import { SetPanel } from "@/panels/set-panel";
import { useSet } from "@/lib/set-store";
import { Toolbar } from "@/panels/toolbar";

/**
 * One workspace, no pages (FL principle): browser left, deck lanes centre,
 * mixer right, every panel visible at once. Keyboard-first: each deck's
 * transport has a key, shown in the hint bar when you point at it.
 */
export function App() {
  React.useEffect(() => {
    const eng = getEngine();
    const stop = onFrame((now, dt) => eng.tickMeters(now, dt));
    if (inTauri()) {
      useApp.getState().refreshLibrary().then(() => {
        if (!useApp.getState().tracks.length) void useApp.getState().scan();
      }).catch((e) => useApp.getState().setStatus(String(e)));
    } else {
      useApp.getState().setStatus("UI-only mode: start the full app with `npm run dev` to load your library.");
    }
    return stop;
  }, []);

  React.useEffect(() => {
    const keys: Record<string, () => void> = {
      q: () => void useApp.getState().togglePlay("A"),
      w: () => void useApp.getState().togglePlay("B"),
      a: () => useApp.getState().cue("A"),
      s: () => useApp.getState().cue("B"),
      z: () => useApp.getState().sync("A"),
      x: () => useApp.getState().sync("B"),
      n: () => useSet.getState().skip(),
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.ctrlKey || e.metaKey || e.altKey) return;
      const f = keys[e.key.toLowerCase()];
      if (f) {
        e.preventDefault();
        f();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full flex-col">
      <Toolbar />
      <HintBar />
      <main className="grid min-h-0 flex-1 grid-cols-[minmax(240px,300px)_1fr_auto] gap-1.5 p-1.5">
        <Browser />
        <div className="grid min-h-0 grid-rows-[auto_1fr_1fr] gap-1.5">
          <SetPanel />
          <DeckLane id="A" />
          <DeckLane id="B" />
        </div>
        <Mixer />
      </main>
    </div>
  );
}
