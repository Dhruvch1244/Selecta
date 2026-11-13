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
./scripts/install-local.sh   # build this checkout, install/update ~/.local/bin/selecta + menu entry
npm run check          # eslint + typecheck + vitest
npm run protocol       # regenerate TS types from Rust (commit the result)
cargo test --workspace
cargo run --release -p selecta-core --example analyze -- FILE...   # analyse real files, prints JSON
cargo run --release -p selecta-core --example grid_check -- FILE... # beat-grid error vs real kicks (ms), per third
cargo run --release -p selecta-core --example loudness_check -- FILE... # analyser LUFS/peak/gain per file
cargo run --release -p selecta-core --example plan_db -- COPY.db --keylock [--reanalyse] "PROMPT"...  # real plans
SELECTA_SELFTEST=1 ./target/release/selecta-desktop                  # audio self-test inside the real webview
```

### Testing against the real library (not synthetic fixtures)

1. `sqlite3 ~/.local/share/com.dhruvch1244.selecta/library.db ".backup /tmp/lib-copy.db"`
   (never point tools at the live DB: opening it runs migrations).
2. In `apps/ui/dev-fixtures/` (gitignored): symlink `lib -> ~/Music` and
   `libstems -> ~/.local/share/com.dhruvch1244.selecta/stems`, then write
   `real-library.json` (tracks + analysis + vocals paths) and `real-plans.json`
   (from `plan_db`, paths rewritten to `/__fixtures__/lib/...`).
3. `SELECTA_NO_HMR=1 npm run dev:ui`, open `/?mock=real`, and drive a whole set
   over CDP, sampling `getEngine().master.peakDb` for silence gaps and the
   conductor's status lines for holds. Without `SELECTA_NO_HMR`, a `cargo test`
   (it rewrites `fx-catalog.json`) reloads the page mid-set.

### Listening test of the real app (what the user actually hears)

Route the real app to a null sink, record it, analyse the recording; nothing
reaches the speakers and no other app is captured:

```sh
MOD=$(pactl load-module module-null-sink sink_name=selecta_test)
parecord -d selecta_test.monitor --file-format=wav rec.wav &
PULSE_SINK=selecta_test SELECTA_AUTOPLAY="prompt" SELECTA_AUTOPLAY_MIN=6 ./target/release/selecta-desktop
pactl unload-module $MOD   # always
```

`SELECTA_AUTOPLAY` rescans, plans by rules (no AI quota), plays, logs every
conductor event and exits. Check the recording for dropouts, clicks and
silences, and check that the logged transitions are what the prompt asked for.

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
- **Web Audio `lowpass`/`highpass` Q is in dB**, not linear: Butterworth is
  `Q = -3.0103`, not `0.7071` (which measured as +2..4 dB crossover bumps).
  Measure audio changes with `await selectaSelfTest()` in a dev build.
- **symphonia can't decode Opus**: `decode_mono` falls back to ffmpeg; most
  of a real library was `.opus`. Keep the fallback until symphonia has Opus.
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
- **CSP vs worklets/WASM:** the window CSP must allow `script-src 'self' blob:
  'wasm-unsafe-eval'`: signalsmith-stretch (keylock) loads its AudioWorklet
  from a blob URL and compiles WASM. Without it keylock silently falls back
  to the original pitch in the app while working fine in the dev browser.
- **Measure the real webview, not just Chrome:** `SELECTA_SELFTEST=1
  ./target/release/selecta-desktop` runs the audio self-test inside the app's
  own webview (WebKitGTK on Linux), prints PASS/FAIL lines and exits 0/1.
- **Offline worklet race:** messages to an AudioWorklet (e.g. stretch
  `schedule()`) can arrive after an OfflineAudioContext has already
  finished rendering. Await a reply on the same port before `startRendering()`
  (see `keylock.ts`).
- **Dev fixtures are synthetic:** `apps/ui/dev-fixtures/*.mp3` are generated
  beats with real song names, so their vocal stems are near-silent. Use real
  files (copied into `dev-fixtures/real/`, gitignored) for stem/vocal tests.
- **Vite HMR in CDP tests:** after editing UI files, restart `npm run dev:ui`
  before evaluating `import("/src/lib/store.ts")` from a test: hot-reload
  timestamps give the page a second store instance and the test sees an empty one.
- **zsh doesn't word-split `$VAR`:** a file list in a variable is one argument.
  Use `git diff --name-only --diff-filter=U | xargs -r git checkout --ours --`.
- **Beat grids must be pinned to the kicks:** the coarse tempo/phase fit
  (23 ms frames) was off by -60..+51 ms per track on a real library, so
  "beatmatched" decks flammed by up to 110 ms. `kick_phase_offset` refines
  the phase on a 2.9 ms envelope, scoring low-thump x high-click so a
  tech-house bassline a 16th after the kick can't capture it. Check with
  `--example grid_check` (every third of each track should read ~0 ms).
- **Headless test browsers must be muted:** start Chrome with `--mute-audio`
  (or route it to a null sink). An unmuted CDP test once played whole test sets
  through the user's Bluetooth speaker.
- **Bluetooth output delay is invisible to the app:** WebKit reported 3 ms and
  PipeWire 0 µs on an SBC speaker that is ~200 ms late, so the decks drew
  ahead of the sound (~1/6 bar). Visuals use `Engine.heardTime()`, which
  includes the calibrated `outputDelay` (Settings → Output; defaults from the
  detected codec).
- **Duplicate songs:** a real library held 33 duplicate files out of 157 (an
  .opus rip and a tagged .m4a). The planner dedupes by `song_key`; without it
  sets played the same song twice.
- **Loudness is measured on the playback buffer:** the old peak-capped RMS
  gain left drops 11 dB apart (lossy files overshoot 0 dBFS, so the cap cut
  them hard), and the analyser's mono ffmpeg downmix reads Opus ~3 dB hot.
  `engine/loudness.ts` measures BS.1770 short-term loudness (90th
  percentile) on the decoded stereo buffer at load and sets `gainDb` to reach
  -10 LUFS; it matches pyloudnorm within 0.15 dB on the real library.
- **Drops are where the sub-bass returns, not where it gets loud:** DnB
  builds are as loud as their drops, so the full-band energy curve put 52%
  of a real library's drops a bar or more off (double drops landed "4 bars
  early, while the other is still building"). `refine_drops` snaps each
  drop to the beat where the sub jumps back to full level after a bass-light
  stretch, and shifts the bar line when the drops consistently slam on
  another beat. Measure the sub on 20-100 Hz (up to 150 Hz caught risers
  and kick punch the beat before), only from frames whose whole STFT window
  is inside the beat (a centred frame hears ~40 ms of the next beat, which
  in power read as a 10 dB jump a beat early), and locate the hit on the
  beat's *first half*: DnB basses often push in an eighth early, and the
  full-beat level moved the bar line a beat short (test
  `a_bass_pushed_an_eighth_early_still_drops_on_the_one`). On the real
  library: 63% of drops exactly on the slam, 6% a bar or more off.
- **Never postpone a double drop by a phrase:** when the next track isn't
  decoded in time the conductor holds the current one, and pushing a
  drop-on-drop move 8 bars landed the incoming's drop in the middle of the
  outgoing's. It now waits for the outgoing's next drop, and tracks are
  prepared two ahead (one ahead held 7 times in a 9-minute rapid set).
- **Moves are scored, not rolled:** `script` places every move a pair allows
  (`place_move`) and scores it on both tracks' analysis (`quality`: the
  incoming's drop on the hand-off, the outgoing's drop or energy fall, no
  stacked full drops, no two singers, play length, phrase lines). The genre
  table is only the prior; a weighted dice roll cut into intros and paired
  drops that didn't line up.
- **Layers are events, not wallpaper:** a density of ~0.8 loops per track
  turned a house set into riff-on-riff mud. `layers.rs` caps it at half the
  tracks, keeps a motif off neighbouring tracks and at most 3 uses, and only
  places loops in a track's solo bars. The exception is a double drop where
  nobody really sings (vocal presence < 0.6: stems pick up synth bleed in
  loud drops): a third track's sung hook loops over both drops
  (`over_mix`), and both decks' vocals duck for it. Vocal loops are
  preferred over riffs unless the prompt asks for basslines.

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
