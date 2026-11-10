import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { TooltipProvider } from "@/components/dsgn/tooltip";
import "./styles/base.css";
import { App } from "./App";

// Theme before first paint, so there is no flash of the wrong palette.
try {
  const t = localStorage.getItem("selecta.theme");
  if (t === "light" || t === "dark") document.documentElement.dataset.theme = t;
} catch {
  /* storage unavailable: default dark */
}

if (import.meta.env.DEV) {
  // Console/CDP handle for measuring the audio chain: `await selectaSelfTest()`.
  const { selfTest } = await import("@selecta/engine");
  Object.assign(window, { selectaSelfTest: selfTest });
}

if (import.meta.env.DEV && new URLSearchParams(location.search).has("mock")) {
  const { installMockBackend } = await import("./dev/mock-backend");
  await installMockBackend();
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <TooltipProvider delayDuration={400}>
      <App />
    </TooltipProvider>
  </StrictMode>,
);

// `SELECTA_SELFTEST=1 selecta`: run the audio self-test inside the real app's
// webview (WebKitGTK/WebView2, not the dev browser), print it and exit.
// It's how the shipped audio path gets measured, not assumed. Deliberately
// *not* a top-level await: in the production bundle the engine chunk imports
// from this entry chunk, so awaiting its import here deadlocks (measured: the
// release app hung while the dev app passed).
if ("__TAURI_INTERNALS__" in window) {
  void runSelfTestIfAsked();
  void runAutoplayIfAsked();
}

async function runSelfTestIfAsked() {
  const { invoke } = await import("@tauri-apps/api/core");
  if (!(await invoke<boolean>("self_test_mode").catch(() => false))) return;
  const log = (line: string) => void invoke("self_test_log", { line }).catch(() => {});
  const fmt = (x: { pass: boolean; name: string; value: number; expect: string }) => `${x.pass ? "PASS" : "FAIL"} ${x.name}: ${x.value} (want ${x.expect})`;
  log("self-test: webview up, starting");
  const { outputCheck, selfTest } = await import("@selecta/engine");
  const results = await selfTest((x) => log(fmt(x))).catch((e) => {
    const r = { name: `selfTest threw: ${e instanceof Error ? `${e.name}: ${e.message}` : String(e)}`, value: 0, pass: false, expect: "" };
    log(fmt(r));
    return [r];
  });
  const out = await outputCheck().catch((e) => ({ name: `output check threw: ${e}`, value: 0, pass: false, expect: "" }));
  log(fmt(out));
  results.push(out);
  // The live engine, as the app builds it: is the limiter really in the path?
  const { Engine } = await import("@selecta/engine");
  const eng = new Engine();
  await eng.limiterReady;
  const lim = { name: "live engine limiter in the signal path", value: eng.limiterActive ? 1 : 0, pass: eng.limiterActive, expect: "1" };
  log(fmt(lim));
  results.push(lim);
  void eng.ctx.close();
  await invoke("self_test_report", { lines: [], ok: results.every((x) => x.pass) });
}

/** `SELECTA_AUTOPLAY="prompt"`: rescan, plan by rules, play for N minutes, log events, exit. */
async function runAutoplayIfAsked() {
  const { invoke } = await import("@tauri-apps/api/core");
  const cfg = await invoke<[string, number, boolean] | null>("autoplay_config").catch(() => null);
  if (!cfg) return;
  const [prompt, minutes, withAi] = cfg;
  const log = (line: string) => void invoke("self_test_log", { line: `[${(performance.now() / 1000).toFixed(1)}s] ${line}` }).catch(() => {});
  const { useApp, getEngine } = await import("@/lib/store");
  const { useSet } = await import("@/lib/set-store");
  useApp.subscribe((s, p) => {
    if (s.status !== p.status) log(`status: ${s.status}`);
  });
  await useApp.getState().refreshLibrary();
  await useApp.getState().scan();
  for (let i = 0; i < 600; i++) {
    await new Promise((r) => setTimeout(r, 500));
    const st = useApp.getState();
    if (!st.analysing && st.tracks.every((t) => t.analyzed || t.error)) break;
  }
  log(`library ready: ${useApp.getState().tracks.length} tracks`);
  await useSet.getState().build(prompt, withAi ? {} : { noAi: true });
  const plan = useSet.getState().plan;
  if (!plan) return void invoke("self_test_report", { lines: [], ok: false });
  log(`title: ${plan.title} | vibe: ${plan.intent.vibe ?? ""}`);
  plan.tracks.forEach((t, i) => log(`plan ${i}: ${t.trans.kind}${t.trans.loopFrom != null ? " (hook loop)" : ""} ${t.artist} - ${t.title} rate ${t.rate.toFixed(3)} keyShift ${t.keyShift} in ${t.inBar}`));
  plan.notes.forEach((n) => log(`note: ${n}`));
  const t0 = performance.now();
  await useSet.getState().play();
  log(`playing; latency ${getEngine().latencyMs} ms`);
  while (performance.now() - t0 < minutes * 60000 && useSet.getState().playing) await new Promise((r) => setTimeout(r, 1000));
  useSet.getState().stop();
  log("autoplay done");
  await invoke("self_test_report", { lines: [], ok: true });
}
