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

export async function installMockBackend(): Promise<void> {
  const fixtures: Fixture[] = await fetch("/__fixtures__/library.json").then((r) => (r.ok ? r.json() : []));
  const byId = new Map<string, Fixture>();
  const tracks: TrackSummary[] = fixtures.map((f, i) => {
    const file = f.path.split("/").pop() ?? f.path;
    const [artist, title] = file.replace(/\.[^.]+$/, "").split(" - ");
    const id = `mock${i}`;
    byId.set(id, f);
    return { id, path: `/__fixtures__/${file}`, artist: artist ?? "", title: title ?? file, album: "", genreTag: "", duration: f.analysis.duration, analyzed: true, bpm: f.analysis.bpm, key: f.analysis.key, camelot: f.analysis.camelot, mood: null, error: null };
  });
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
    settings_get: () => ({ musicDirs: ["/__fixtures__"], ai: { provider: "claude", model: "sonnet", binary: "claude", curate: true, tagLibrary: true } }),
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
