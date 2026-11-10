// Dev-only stand-in for the Rust shell, so the UI can be built and checked
// in a plain browser: `npm run dev:ui`, then open http://localhost:1420/?mock.
// Fixtures live in apps/ui/dev-fixtures/ (gitignored audio, served at /__fixtures__ by a dev-only
// Vite middleware, never bundled), made by:
//   cargo run --release -p selecta-core --example analyze -- *.mp3 > library.json
// Never bundled: main.tsx only imports this when import.meta.env.DEV is true.

import type { Analysis, SetPlan, TrackSummary } from "@selecta/protocol";

interface Fixture {
  path: string;
  analysis: Analysis;
}

/**
 * `?mock=real`: the user's real library, exported from a copy of their
 * library.db with real analysis, stems and planner output (see
 * `examples/plan_db.rs`). dev-fixtures/lib and dev-fixtures/libstems are symlinks
 * to the music and stem folders. Gitignored, never bundled.
 */
async function installRealLibrary(): Promise<boolean> {
  interface RealTrack { id: string; path: string; artist: string; title: string; album: string; genreTag: string; duration: number; analysis: Analysis; vocals: string | null; mood: string | null }
  const lib: RealTrack[] | null = await fetch("/__fixtures__/real-library.json").then((r) => (r.ok ? r.json() : null)).catch(() => null);
  const plans: Record<string, SetPlan> = await fetch("/__fixtures__/real-plans.json").then((r) => (r.ok ? r.json() : {})).catch(() => ({}));
  if (!lib) return false;
  const byId = new Map(lib.map((t) => [t.id, t]));
  const tracks: TrackSummary[] = lib.map((t) => ({ id: t.id, path: t.path, artist: t.artist, title: t.title, album: t.album, genreTag: t.genreTag, duration: t.duration, analyzed: true, bpm: t.analysis.bpm, key: t.analysis.key, camelot: t.analysis.camelot, mood: t.mood, error: null, stems: !!t.vocals }));
  const planList = Object.entries(plans).map(([prompt, p], i) => ({ ...p, id: `real${i}`, prompt }));
  const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
    settings_get: () => ({ musicDirs: ["/__fixtures__/lib"], ai: { provider: "off", model: "sonnet", binary: "claude", curate: false, tagLibrary: false }, stems: true, keylock: true }),
    ai_status: () => ({ available: false, provider: "off", model: "", detail: "AI off (real-library mock)" }),
    library_tracks: () => tracks,
    library_scan: () => ({ found: tracks.length, added: 0, updated: 0, removed: 0 }),
    track_analysis: (a) => byId.get(String(a.id))?.analysis,
    track_vocals: (a) => byId.get(String(a.id))?.vocals ?? null,
    jobs_progress: () => [],
    plan_set: (a) => structuredClone(planList.find((p) => p.prompt === a.prompt) ?? planList[0]),
    sets_list: () => planList.map((p) => ({ id: p.id, created: p.created, title: p.title, prompt: p.prompt, tracks: p.tracks.length })),
    set_get: (a) => structuredClone(planList.find((p) => p.id === a.id) ?? planList[0]),
    stems_status: () => ({ installed: true, installing: false, device: "cuda", detail: "", done: 0, total: 0 }),
    now_playing: (a) => ((window as unknown as { __nowPlaying: unknown }).__nowPlaying = a.np),
  };
  const noop = () => null;
  for (const k of ["settings_set", "set_delete", "library_tag", "stems_install", "stems_prioritise", "plugin:event|unlisten"]) handlers[k] = noop;
  handlers["plugin:event|listen"] = () => 1;
  install(handlers);
  return true;
}

function install(handlers: Record<string, (args: Record<string, unknown>) => unknown>) {
  Object.assign(window, {
    __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
    __TAURI_INTERNALS__: {
      invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
        const h = handlers[cmd];
        if (!h) throw new Error(`mock backend: no handler for ${cmd}`);
        return h(args);
      },
      convertFileSrc: (p: string) => p,
      transformCallback: () => 0,
    },
  });
}

export async function installMockBackend(): Promise<void> {
  if (new URLSearchParams(location.search).get("mock") === "real" && (await installRealLibrary())) return;
  const fixtures: Fixture[] = await fetch("/__fixtures__/library.json").then((r) => (r.ok ? r.json() : []));
  const byId = new Map<string, Fixture>();
  const tracks: TrackSummary[] = fixtures.map((f, i) => {
    const file = f.path.split("/").pop() ?? f.path;
    const [artist, title] = file.replace(/\.[^.]+$/, "").split(" - ");
    const id = `mock${i}`;
    byId.set(id, f);
    return { id, path: `/__fixtures__/${file}`, artist: artist ?? "", title: title ?? file, album: "", genreTag: "", duration: f.analysis.duration, analyzed: true, bpm: f.analysis.bpm, key: f.analysis.key, camelot: f.analysis.camelot, mood: null, error: null, stems: false };
  });
  // Vocal stems, if made for the fixtures: dev-fixtures/stems/<file name without extension>.ogg
  const stemOf = (t: TrackSummary) => `/__fixtures__/stems/${(t.path.split("/").pop() ?? "").replace(/\.[^.]+$/, "")}.ogg`;
  await Promise.all(tracks.map(async (t) => (t.stems = await fetch(stemOf(t), { method: "HEAD" }).then((r) => r.ok && r.headers.get("content-type") !== "text/html").catch(() => false))));
  // plan.json comes from `--example plan`; re-key its tracks to the mock ids/paths.
  const rawPlan: { plan: SetPlan } | null = await fetch("/__fixtures__/plan.json").then((r) => (r.ok ? r.json() : null)).catch(() => null);
  const plan = rawPlan?.plan ?? null;
  if (plan) {
    for (const t of plan.tracks) {
      const file = t.path.split("/").pop();
      const m = tracks.find((x) => x.path.endsWith(`/${file}`));
      if (m) Object.assign(t, { id: m.id, path: m.path });
    }
  }
  const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
    settings_get: () => ({ musicDirs: ["/__fixtures__"], ai: { provider: "claude", model: "sonnet", binary: "claude", curate: true, tagLibrary: true }, stems: true, keylock: true }),
    stems_status: () => ({ installed: true, installing: false, device: "cuda", detail: "", done: 0, total: 0 }),
    stems_install: () => null,
    now_playing: (a) => ((window as unknown as { __nowPlaying: unknown }).__nowPlaying = a.np),
    stems_prioritise: () => null,
    track_vocals: (a) => {
      const t = tracks.find((x) => x.id === a.id);
      return t?.stems ? stemOf(t) : null;
    },
    ai_status: () => ({ available: true, provider: "claude", model: "sonnet", detail: "Using your Claude CLI (sonnet). (mock)" }),
    ai_test: () => "sonnet answered in 3.2s: \"drop the bass\" (mock)",
    library_tag: () => 0,
    "plugin:event|listen": () => 1,
    "plugin:event|unlisten": () => null,
    settings_set: () => null,
    library_tracks: () => tracks,
    library_scan: () => ({ found: tracks.length, added: 0, updated: 0, removed: 0 }),
    track_analysis: (a) => byId.get(String(a.id))?.analysis,
    jobs_progress: () => [],
    plan_set: (a) => {
      if (!plan) throw new Error("mock backend: no plan.json fixture");
      return { ...structuredClone(plan), prompt: String(a.prompt), id: `mock-${Date.now()}` };
    },
    sets_list: () => (plan ? [{ id: plan.id, created: plan.created, title: plan.title, prompt: plan.prompt, tracks: plan.tracks.length }] : []),
    set_get: () => plan,
    set_delete: () => null,
  };
  Object.assign(window, {
    __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
    __TAURI_INTERNALS__: {
      invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
        const h = handlers[cmd];
        if (!h) throw new Error(`mock backend: no handler for ${cmd}`);
        return h(args);
      },
      convertFileSrc: (p: string) => p,
      transformCallback: () => 0,
    },
  });
}
