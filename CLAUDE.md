# CLAUDE.md

Guidance for Claude Code (and humans) working in this repo.

## Workflow (non-negotiable)

- `main` is protected: PRs only, no force-push, linear history, squash-merge.
- Every change goes on a branch named `feat/…`, `fix/…` or `chore/…` and
  becomes a pull request against `main`, filled in from the PR template.
  Never push to `main`.
- Before opening a PR: `npm run check`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`,
  and if the UI changed, render it (dark and light) and look at it.

## Commands

```sh
npm run dev            # desktop app (Tauri + Vite)
npm run dev:ui         # UI only; open http://localhost:1420/?mock
npm run check          # eslint + typecheck + vitest
npm run protocol       # regenerate TS types from Rust (commit the result)
cargo test --workspace
cargo run --release -p selecta-core --example analyze -- FILE...   # analyse real files, prints JSON
```

## Architecture rules

Read docs/ARCHITECTURE.md. In short: `selecta-core` never imports Tauri;
`packages/engine` and `packages/protocol` never import React, Tauri or app
code; all background work goes through `jobs::Scheduler`; all
I/O/DSP commands are `#[tauri::command(async)]`; UI frame work goes through
`lib/frame.ts`; style values come from tokens only (docs/DESIGN.md).

## Footguns already hit (don't rediscover them)

- **TypeScript 7 vs typescript-eslint:** typescript-eslint 8.x accepts TS
  `<6.1`, so TS is pinned to `~6.0`. `npm install` fails with ERESOLVE otherwise.
- **dsgn import paths:** `dsgn add` writes `@/` + `utilsDir`, which gives
  `@/src/lib/utils` here. tsconfig `paths` and the Vite alias resolve both
  `@/src/*` and `@/*` to `src/`. Don't hand-edit registry files to "fix"
  it: that breaks `dsgn diff`/`update`.
- **Canvas and CSS variables:** `ctx.fillStyle = "var(--cyan)"` is silently
  ignored (it keeps the previous colour). Resolve with `getComputedStyle`
  first (`resolveColor` in `components/studio/waveform.tsx`).
- **Base font size:** set the 12 px UI size on `body`, never `html`.
  Changing the root size shrinks the rem unit and scales every Tailwind/dsgn
  dimension by 0.75 (it truncated every LCD).
- **symphonia 0.6** changed its decode API: `codec_params.audio()`,
  `get_codecs().make_audio_decoder(...)`, `next_packet()` returns
  `Result<Option<Packet>>`, and buffers expose `spec().rate()` and
  `copy_to_slice_interleaved`. See `crates/selecta-core/src/decode.rs`.
- **Tauri 3 is alpha** (as of Nov 2026); this repo is on Tauri 2 stable.
- **`pkill -f <name>` in a shell whose command line contains `<name>`**
  kills that shell. Use `pgrep -f '^node .*vite'` or kill by PID.
