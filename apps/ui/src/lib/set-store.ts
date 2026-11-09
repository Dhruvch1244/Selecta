// Set state: planning, the current plan, and the conductor performing it.
// Kept apart from the deck store so manual DJing and set playback stay
// independent; while a set plays the conductor owns both decks.

import { Conductor, type ConductorEvent, type DeckId } from "@selecta/engine";
import type { SetPlan, SetSummary } from "@selecta/protocol";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { api } from "./api";
import { loadTrackAudio } from "./track-audio";
import { getEngine, useApp } from "./store";

interface SetState {
  plan: SetPlan | null;
  planning: boolean;
  /** Live lines from the backend while a set is being planned. */
  progress: string[];
  playing: boolean;
  index: number;
  leadDeck: DeckId | null;
  recent: SetSummary[];
  error: string | null;
  build: (prompt: string, opts: { durationMin?: number; chaos?: number }, reroll?: boolean) => Promise<void>;
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
    } else if (e.type === "postponed") status("Next track still decoding; holding this one for another phrase.");
    else if (e.type === "end") {
      set({ playing: false, leadDeck: null });
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
    planning: false,
    progress: [],
    playing: false,
    index: 0,
    leadDeck: null,
    recent: [],
    error: null,

    build: async (prompt, opts, reroll) => {
      if (!prompt.trim()) return set({ error: "Describe the set you want first." });
      set({ planning: true, error: null, progress: [] });
      const unlisten = await listen<string>("plan-progress", (e) => set((s) => ({ progress: [...s.progress, e.payload] }))).catch(() => () => {});
      try {
        const plan = await api.planSet(prompt, { ...opts, ...(reroll ? { seed: Date.now() } : {}) });
        set({ plan, index: 0 });
        // Separate this set's stems first, so its mashup moves have their vocals in time.
        void api.stemsPrioritise(plan.tracks.map((t) => t.id)).catch(() => {});
        useApp.getState().setStatus(`${plan.title}: ${plan.tracks.length} tracks, about ${Math.round(plan.estTotalSec / 60)} min.`);
        void get().refreshRecent();
      } catch (e) {
        set({ error: e instanceof Error ? e.message : String(e) });
      } finally {
        unlisten();
        set({ planning: false });
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
        await ensureConductor().start(structuredClone(plan));
      } catch (e) {
        set({ playing: false, error: e instanceof Error ? e.message : String(e) });
      }
    },
    stop: () => {
      conductor?.stop();
      set({ playing: false, leadDeck: null });
      for (const id of ["A", "B"] as const) useApp.setState((s) => ({ decks: { ...s.decks, [id]: { ...s.decks[id], playing: false } } }));
    },
    skip: () => void conductor?.skip(),
  };
});

/** Mirror conductor-loaded tracks into the deck lanes' header state. */
function syncDeckView(id: DeckId, trackId: string, artist: string, title: string) {
  const d = getEngine().decks[id];
  useApp.setState((s) => ({
    decks: { ...s.decks, [id]: { ...s.decks[id], trackId, artist, title, analysis: d.track?.analysis ?? null, loading: false, playing: true, rate: d.rate, error: null } },
  }));
}
