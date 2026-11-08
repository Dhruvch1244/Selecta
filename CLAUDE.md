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
- **`pkill -f <name>` / `pgrep -f <pattern>` match the shell running them**
  when its own command line contains the pattern, killing it. Find dev
  servers by port instead: `ss -ltnp | grep :1420`, then kill that PID.
- **No sound in the AppImage:** WebKitGTK plays Web Audio through GStreamer.
  An AppImage carries its own GStreamer libraries, so it also has to bundle
  the plugins (`bundle.linux.appimage.bundleMediaFramework: true`). Without
  that you get "GStreamer element appsink/appsrc/autoaudiosink not found"
  and silence. Building the AppImage **on Arch** additionally needs
  `patchelf` and `GSTREAMER_PLUGINS_DIR=/usr/lib/gstreamer-1.0
  GSTREAMER_HELPERS_DIR=/usr/lib/gstreamer-1.0` (the plugin guesses Debian
  paths). On Arch, the native binary (`target/release/selecta-desktop`) uses
  the system GStreamer and needs none of this.

## Dependencies

- Dependabot opens weekly grouped PRs. Patch/minor groups **auto-merge** once
  all required checks pass (`.github/workflows/dependabot-automerge.yml`);
  nothing merges red because branch protection still applies.
- **Majors are never auto-merged.** Read the changelog, check CI, test locally.
- Two branches of a Dependabot PR touching adjacent lines conflict after the
  first merges: comment `@dependabot rebase` instead of editing its branch.
- Held back on purpose (in `dependabot.yml` `ignore`): TypeScript `>=6.1`
  (typescript-eslint), `@types/node` majors (CI runs Node 24), Tauri majors
  (3.x is alpha). Remove an ignore when its reason no longer holds.

## Releases

Bump `version` in `apps/desktop/src-tauri/tauri.conf.json` and
`Cargo.toml` (workspace) via a PR, merge, then tag `main`:
`git tag v0.x.y && git push origin v0.x.y`. The Release workflow builds
Linux (AppImage/deb/rpm), Windows (NSIS) and macOS (universal dmg)
installers and attaches them to a **draft** release; publish it after testing.
Local build: `npm run build`; for just the AppImage:
`cd apps/desktop && npx tauri build --bundles appimage`.
