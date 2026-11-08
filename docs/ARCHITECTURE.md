# Architecture

## The split

```
┌──────────────────────── apps/ui (React) ────────────────────────┐
│ panels, studio controls, dsgn components                         │
│   │ zustand store (what React renders)   frame loop (60 Hz reads)│
│   ▼                                                              │
│ packages/engine (Web Audio, no React)   packages/protocol (types)│
└───────────────┬──────────────────────────────────────▲──────────┘
                │ invoke() — typed, async               │ ts-rs
┌───────────────▼──── apps/desktop/src-tauri ───────────┴──────────┐
│ thin #[tauri::command(async)] wrappers, settings, asset scope    │
└───────────────┬──────────────────────────────────────────────────┘
                ▼
         crates/selecta-core (no Tauri): decode · analysis · library · jobs
```

**Why audio plays in the webview, not in Rust.** Web Audio gives
sample-accurate scheduling on one clock, AudioWorklets for custom DSP, and
mature EQ/filter/convolution nodes, with no FFI and no second audio clock to
keep in sync with the UI. Rust does the heavy, offline work (decode,
analysis, and later stems and neural beat tracking) where a native thread
pool and ONNX Runtime are far better than a browser. Keylock will be a
WASM time-stretcher inside an AudioWorklet, so it stays on the same clock.

## Rules the code follows

1. **Physical separation.** `selecta-core` cannot import Tauri: CI builds
   and tests it on a runner with no webkit/GTK installed. `packages/engine`
   and `packages/protocol` cannot import React, Tauri or app code: an ESLint
   rule fails CI on a stray import (npm hoisting makes React *resolvable*
   there, so a missing dependency alone wouldn't stop it).
2. **One scheduler.** All background work goes through
   `selecta_core::jobs::Scheduler`: dedup by key, `Now > Next > Idle`
   priority chosen when a worker frees up, cooperative cancellation by
   group. Nothing calls `thread::spawn` for work. In the UI, one
   `requestAnimationFrame` loop (`apps/ui/src/lib/frame.ts`) drives every
   meter, waveform and readout.
3. **Async commands.** Every Tauri command doing I/O or DSP is
   `#[tauri::command(async)]` and runs on a blocking thread. A plain `fn`
   command runs on the main thread and freezes the window.
4. **Types cross the boundary once.** Rust structs derive `ts_rs::TS`;
   `npm run protocol` regenerates `packages/protocol/src/generated`; CI
   fails if they're stale. A renamed field is a TypeScript error, not a
   runtime `undefined`.
5. **Measured, not assumed.** Analysis heuristics carry the numbers they
   were tuned against in their doc comments, and tests assert them on
   synthetic tracks with known tempo, offset and structure.

## Data

- Library: one SQLite file in the app data dir (`library.db`), with
  analysis as a JSON column and an `analysis_version` that triggers
  re-analysis when the algorithm changes.
- Settings: `settings.json` in the app config dir.
- Audio files are read by the webview through Tauri's asset protocol, scoped
  at runtime to the music folders the user picked, never globally.

## Analysis pipeline (`crates/selecta-core/src/analysis.rs`)

decode (symphonia) → mono → half-band decimate to ≤30 kHz → STFT 2048/512 →
spectral flux → tempo by comb-weighted autocorrelation → **one constant beat
grid** fitted over the whole track (coarse ±2.5% then fine 0.005 BPM) →
downbeat phase from low-band onsets and energy rises → per-bar energy →
drops/breaks from 4-bar block steps → sections → Krumhansl key on whitened,
peak-picked chroma → loudness gain → 1200-column 3-band overview.
About 0.35–0.6 s per track per track in release mode on a 12-thread laptop (measured with the `analyze` example).
