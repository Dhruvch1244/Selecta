// Set state: planning, the current plan, and the conductor performing it.
// Kept apart from the deck store so manual DJing and set playback stay
// independent; while a set plays the conductor owns both decks.

import { Conductor, type ConductorEvent, type DeckId, type LayerDeckId } from "@selecta/engine";
import type { Layer, SetPlan, SetSummary } from "@selecta/protocol";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { api, type PlanOptions } from "./api";
import { loadTrackAudio } from "./track-audio";
import { useLayout } from "./layout";
import { getEngine, useApp } from "./store";

interface SetState {
  plan: SetPlan | null;
  /** The plan on screen is the instant rules draft; Claude's curated plan is still coming. */
  draft: boolean;
  planning: boolean;
  /** Live lines from the backend while a set is being planned. */
  progress: string[];
  playing: boolean;
  index: number;
  leadDeck: DeckId | null;
  /** What each loop deck is layering right now (cleared by the strip when it ends). */
  layers: Partial<Record<LayerDeckId, { layer: Layer; host: number }>>;
  recent: SetSummary[];
  error: string | null;
  build: (prompt: string, opts: PlanOptions, reroll?: boolean) => Promise<void>;
  open: (id: string) => Promise<void>;
  refreshRecent: () => Promise<void>;
  play: () => Promise<void>;
  stop: () => void;
  skip: () => void;
}

let conductor: Conductor | null = null;
export const getConductor = (): Conductor | null => conductor;

export const useSet = create<SetState>((set, get) => {
  const onEvent = (e: ConductorEvent) => {
    const status = useApp.getState().setStatus;
    const plan = get().plan;
    if (e.type === "track") {
      set({ index: e.index, leadDeck: e.deck });
      const t = plan?.tracks[e.index];
      if (t) syncDeckView(e.deck, t.id, t.artist, t.title);
    } else if (e.type === "transition") {
      const t = plan?.tracks[e.index];
      // The incoming deck is audible from now on, so its lane header updates now, not at the handover.
      if (t) syncDeckView(e.deck, t.id, t.artist, t.title);
      const pct = (e.rate - 1) * 100;
      status(`${e.kind.replace("_", " ")} → ${t?.title ?? ""}${Math.abs(pct) > 0.01 ? ` (${pct > 0 ? "+" : ""}${pct.toFixed(2)}%)` : ""}`);
    } else if (e.type === "fx") {
      const label = e.fx.replace(":", " ").replace(/_/g, " ");
      status(`✦ ${label}: ${e.reason}`);
    } else if (e.type === "layer") {
      set({ layers: { ...get().layers, [e.deck]: { layer: e.layer, host: e.index } } });
      const what = e.layer.part === "vocal" ? "vocal hook" : "riff";
      status(`⟲ ${e.layer.kind}: ${e.layer.title} ${what} on deck ${e.deck}`);
    } else if (e.type === "postponed") status("Next track still decoding; holding this one for another phrase.");
    else if (e.type === "end") {
      set({ playing: false, leadDeck: null });
      useLayout.getState().setStopped();
      status("Set complete.");
    } else if (e.type === "error") status(e.message);
  };

  const ensureConductor = () => {
    conductor ??= new Conductor(getEngine(), {
      loadTrack: (t) => loadTrackAudio(t),
      onEvent,
    });
    return conductor;
  };

  return {
    plan: null,
    draft: false,
    planning: false,
    progress: [],
    playing: false,
    index: 0,
    leadDeck: null,
    layers: {},
    recent: [],
    error: null,

    build: async (prompt, opts, reroll) => {
      if (!prompt.trim()) return set({ error: "Describe the set you want first." });
      if (get().planning) return;
      set({ planning: true, error: null, progress: [] });
      const unlisten = await listen<string>("plan-progress", (e) => set((s) => ({ progress: [...s.progress, e.payload] }))).catch(() => () => {});
      try {
        let plan = await api.planSet(prompt, { ...opts, ...(reroll ? { seed: Date.now() } : {}) });
        if (get().draft && get().playing && conductor?.plan) plan = (await spliceIntoPlaying(plan)) ?? conductor.plan;
        set({ plan, draft: false, index: get().playing ? get().index : 0 });
        // Separate this set's stems first, so its mashup moves have their vocals in time.
        void api.stemsPrioritise(plan.tracks.map((t) => t.id)).catch(() => {});
        useApp.getState().setStatus(`${plan.title}: ${plan.tracks.length} tracks, about ${Math.round(plan.estTotalSec / 60)} min.`);
        void get().refreshRecent();
      } catch (e) {
        set({ error: e instanceof Error ? e.message : String(e) });
      } finally {
        unlisten();
        set({ planning: false, draft: false });
      }
    },

    open: async (id) => {
      const plan = await api.set(id);
      set({ plan, index: 0 });
      void api.stemsPrioritise(plan.tracks.map((t) => t.id)).catch(() => {});
    },
    refreshRecent: async () => set({ recent: await api.sets().catch(() => []) }),

    play: async () => {
      const plan = get().plan;
      if (!plan) return;
      try {
        set({ playing: true, error: null });
        // The conductor mutates cue points (skips, postponements); give it its own copy.
        useLayout.getState().setStarted();
        const c = ensureConductor();
        c.keylock = useApp.getState().settings?.keylock ?? true;
        await c.start(structuredClone(plan));
      } catch (e) {
        set({ playing: false, error: e instanceof Error ? e.message : String(e) });
      }
    },
    stop: () => {
      conductor?.stop();
      useLayout.getState().setStopped();
      set({ playing: false, leadDeck: null });
      for (const id of ["A", "B"] as const) useApp.setState((s) => ({ decks: { ...s.decks, [id]: { ...s.decks[id], playing: false } } }));
    },
    skip: () => void conductor?.skip(),
  };
});

/**
 * The curated plan arrived while the draft is playing: keep everything up to
 * the next (already loading) track, re-script the curator's order after it,
 * and hand the result to the conductor. Returns the plan now playing, or
 * null if the conductor moved on in the meantime (the draft keeps playing).
 */
async function spliceIntoPlaying(final: SetPlan): Promise<SetPlan | null> {
  const c = conductor;
  if (!c?.plan) return null;
  const k = Math.min(c.index + 1, c.plan.tracks.length - 1);
  const locked = c.plan.tracks.slice(0, k + 1);
  const ids = new Set(locked.map((t) => t.id));
  const rest = final.tracks.map((t) => t.id).filter((id) => !ids.has(id));
  try {
    const tail = await api.planSplice(final.intent, locked[k]!, rest);
    const merged: SetPlan = { ...final, tracks: [...locked.slice(0, k), ...tail] };
    merged.estTotalSec = merged.tracks.reduce((s, t) => s + t.estSec, 0);
    if (!c.adopt(structuredClone(merged), k)) return null;
    useApp.getState().setStatus(`Claude's curation took over after “${locked[k]!.title}”.`);
    return merged;
  } catch (e) {
    console.warn("couldn't splice the curated plan in:", e);
    return null;
  }
}

/** Mirror conductor-loaded tracks into the deck lanes' header state. */
function syncDeckView(id: DeckId, trackId: string, artist: string, title: string) {
  const d = getEngine().decks[id];
  useApp.setState((s) => ({
    decks: { ...s.decks, [id]: { ...s.decks[id], trackId, artist, title, analysis: d.track?.analysis ?? null, loading: false, playing: true, rate: d.rate, error: null } },
  }));
}
