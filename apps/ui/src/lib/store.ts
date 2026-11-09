// App state that React renders (library, deck metadata, settings). Anything
// that changes every frame (positions, meters) is read straight from the
// engine in the frame loop instead, so React never re-renders at 60 Hz.

import { Engine, type DeckId, matchRate, posOfBar } from "@selecta/engine";
import type { Analysis, TrackSummary } from "@selecta/protocol";
import { create } from "zustand";
import { api, type Settings } from "./api";
import { loadTrackAudio } from "./track-audio";

let engine: Engine | null = null;
/** The engine is created on first use, which is always a user gesture (autoplay policy). */
export function getEngine(): Engine {
  engine ??= new Engine();
  return engine;
}
export const peekEngine = (): Engine | null => engine;

export interface DeckView {
  trackId: string | null;
  artist: string;
  title: string;
  analysis: Analysis | null;
  loading: boolean;
  playing: boolean;
  rate: number;
  error: string | null;
}

const emptyDeck = (): DeckView => ({ trackId: null, artist: "", title: "", analysis: null, loading: false, playing: false, rate: 1, error: null });

interface State {
  settings: Settings | null;
  tracks: TrackSummary[];
  scanning: boolean;
  analysing: { done: number; total: number } | null;
  tagging: { done: number; total: number } | null;
  stemming: { done: number; total: number } | null;
  status: string;
  decks: Record<DeckId, DeckView>;
  refreshLibrary: () => Promise<void>;
  scan: () => Promise<void>;
  setMusicDirs: (dirs: string[]) => Promise<void>;
  saveSettings: (s: Settings) => Promise<void>;
  watchJobs: () => void;
  load: (deck: DeckId, track: TrackSummary) => Promise<void>;
  togglePlay: (deck: DeckId) => Promise<void>;
  cue: (deck: DeckId) => void;
  sync: (deck: DeckId) => void;
  setStatus: (s: string) => void;
}

export const useApp = create<State>((set, get) => {
  const patchDeck = (id: DeckId, p: Partial<DeckView>) => set((s) => ({ decks: { ...s.decks, [id]: { ...s.decks[id], ...p } } }));

  return {
    settings: null,
    tracks: [],
    scanning: false,
    analysing: null,
    tagging: null,
    stemming: null,
    status: "",
    decks: { A: emptyDeck(), B: emptyDeck() },

    setStatus: (status) => set({ status }),

    refreshLibrary: async () => {
      const [settings, tracks] = await Promise.all([api.settings(), api.tracks()]);
      set({ settings, tracks });
    },

    setMusicDirs: async (musicDirs) => {
      const settings = { ...get().settings!, musicDirs };
      await api.saveSettings(settings);
      set({ settings });
      await get().scan();
    },

    saveSettings: async (settings) => {
      await api.saveSettings(settings);
      set({ settings });
    },

    scan: async () => {
      set({ scanning: true, status: "Scanning music folders…" });
      try {
        const r = await api.scan();
        set({ status: `Found ${r.found} files · ${r.added} new · ${r.updated} changed · ${r.removed} removed` });
        await get().refreshLibrary();
        get().watchJobs();
      } catch (e) {
        set({ status: String(e instanceof Error ? e.message : e) });
      } finally {
        set({ scanning: false });
      }
    },

    /** Poll background analysis + AI tagging until both drain, refreshing as tracks land. */
    watchJobs: () => {
      const poll = async () => {
        const groups = await api.progress().catch(() => []);
        const pick = (name: string) => {
          const g = groups.find((x) => x.group === name);
          return g ? { done: Number(g.done), total: Number(g.total) } : null;
        };
        const analysing = pick("analysis");
        const tagging = pick("tagging");
        const stemming = pick("stems");
        const before = get();
        set({ analysing, tagging, stemming });
        if (analysing || tagging || stemming) {
          if (
            (analysing && analysing.done % 10 === 0) ||
            (tagging && tagging.done !== before.tagging?.done) ||
            (stemming && stemming.done !== before.stemming?.done)
          )
            void get().refreshLibrary();
          setTimeout(poll, analysing || tagging ? 800 : 2500);
        } else void get().refreshLibrary();
      };
      void poll();
    },

    load: async (id, track) => {
      const eng = getEngine();
      await eng.resume();
      patchDeck(id, { ...emptyDeck(), trackId: track.id, artist: track.artist, title: track.title, loading: true });
      try {
        // Analysis may run now (Rust, off the UI thread) while the browser decodes in parallel.
        const loaded = await loadTrackAudio(track);
        const analysis = loaded.analysis;
        if (get().decks[id].trackId !== track.id) return; // a newer load won
        eng.decks[id].load(loaded);
        patchDeck(id, { analysis, loading: false, playing: false, rate: 1 });
        void get().refreshLibrary();
      } catch (e) {
        patchDeck(id, { loading: false, error: String(e instanceof Error ? e.message : e) });
      }
    },

    togglePlay: async (id) => {
      const eng = getEngine();
      await eng.resume();
      const d = eng.decks[id];
      if (!d.track) return;
      if (d.playing) d.stop();
      else d.play();
      patchDeck(id, { playing: d.playing });
    },

    cue: (id) => {
      const d = getEngine().decks[id];
      if (!d.track) return;
      d.seek(posOfBar(d.grid!, d.track.analysis.startBar));
      patchDeck(id, { playing: d.playing });
    },

    /**
     * Match tempo to the other deck and, if both are playing, land this deck's
     * next downbeat on the other's next downbeat. Refuses beyond +-8% (no
     * keylock yet), because the pitch shift would be audible.
     */
    sync: (id) => {
      const eng = getEngine();
      const me = eng.decks[id];
      const other = eng.decks[id === "A" ? "B" : "A"];
      if (!me.track || !other.track) return get().setStatus("Load both decks to sync.");
      const rate = matchRate(other.bpm, me.track.analysis.bpm);
      if (rate === null) return get().setStatus(`Too far apart to beatmatch (${other.bpm.toFixed(1)} vs ${me.track.analysis.bpm.toFixed(1)} BPM).`);
      if (other.playing && me.playing) {
        const t = other.nextBeat(4);
        me.setRate(rate, t);
        me.seek(posOfBar(me.grid!, Math.ceil(me.bar(t) - 0.5)), t);
      } else {
        me.setRate(rate);
      }
      patchDeck(id, { rate, playing: me.playing });
      get().setStatus(`Deck ${id} synced to ${other.bpm.toFixed(2)} BPM (${rate >= 1 ? "+" : ""}${((rate - 1) * 100).toFixed(2)}%).`);
    },
  };
});
