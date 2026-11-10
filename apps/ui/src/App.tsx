import * as React from "react";
import { HintBar } from "@/components/studio/hint-bar";
import { inTauri } from "@/lib/api";
import { onFrame } from "@/lib/frame";
import { startNowPlaying } from "@/lib/now-playing";
import { getEngine, useApp } from "@/lib/store";
import { Browser } from "@/panels/browser";
import { DeckLane } from "@/panels/deck-lane";
import { LayerStrip } from "@/panels/layer-strip";
import { Mixer } from "@/panels/mixer";
import { SetPanel } from "@/panels/set-panel";
import { FxPad } from "@/panels/fx-pad";
import { useSet } from "@/lib/set-store";
import { useLayout } from "@/lib/layout";
import { hintProps } from "@/lib/hint";
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
    const stopNowPlaying = startNowPlaying();
    return () => {
      stop();
      stopNowPlaying();
    };
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
      l: () => useLayout.getState().toggleLibrary(),
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

  const libraryOpen = useLayout((s) => s.libraryOpen);
  return (
    <div className="flex h-full flex-col">
      <Toolbar />
      <HintBar />
      <main className={`grid min-h-0 flex-1 gap-1.5 p-1.5 ${libraryOpen ? "grid-cols-[minmax(240px,300px)_1fr_auto]" : "grid-cols-[1.75rem_1fr_auto]"}`}>
        {libraryOpen ? (
          <Browser />
        ) : (
          <button
            type="button"
            onClick={() => useLayout.getState().toggleLibrary()}
            className="flex items-center justify-center rounded-sm border border-border bg-[var(--glass)] text-muted-foreground hover:text-foreground"
            {...hintProps({ label: "Show the library", keys: "L" })}
          >
            <span className="font-display text-[11px] uppercase tracking-[0.25em] [writing-mode:vertical-rl]">Library ▸</span>
          </button>
        )}
        <div className="grid min-h-0 grid-rows-[auto_minmax(120px,1fr)_minmax(120px,1fr)_auto_auto] gap-1.5">
          <SetPanel />
          <DeckLane id="A" />
          <DeckLane id="B" />
          <LayerStrip />
          <FxPad />
        </div>
        <Mixer />
      </main>
    </div>
  );
}
