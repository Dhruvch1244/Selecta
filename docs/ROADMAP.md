# Roadmap

Each item is one feature branch and one pull request. The order puts what
makes everything after it better first.

| # | PR | What lands | Why in this order |
|---|---|---|---|
| 1 | **Foundation** (this PR) | Stack, design system, CI/CD, Rust analysis port, scheduler, two decks with sync/EQ/filter/meters | Everything else builds on it |
| 2 | **Neural beat tracking** | Beat + downbeat model via ONNX Runtime (`ort`) in the inference lane, replacing the tempo/grid stage; grid confidence per section | Every transition and effect depends on a correct grid; live-drummer Bollywood is where the DSP grid is weakest |
| 3 | **Stem separation** | Demucs (htdemucs ONNX) on GPU when available, CPU fallback; stems cached per track; the engine plays 4 stems per deck with per-stem gain | The biggest jump in quality: acapella intros, vocal-only throws, drum-only blends |
| 4 | **Keylock** | WASM time-stretcher (Signalsmith Stretch, MIT) in an AudioWorklet; tempo changes without pitch changes; widens the beatmatch window from ±8% | Lets the planner mix across genres without chipmunked vocals |
| 5 | **Set planner** | Prompt → intent → track selection → beam-search ordering (harmonic, tempo, energy arc) → phrase-aligned cue points; set timeline panel | The "describe a set" core, now on solid grids and stems |
| 6 | **FX director** | Effects chosen by intent, not dice: a tension/release model over sections, stems and lyrics (builds into drops, vocal throws on line ends, drum-only breakdowns), with a rule for every move and a budget per phrase | Fixes the prototype's random-FX problem by design |
| 7 | **AI curation (Claude CLI)** | Intent parsing, track vetting by real song knowledge, running order and DJ moments; library genre/mood tagging; all optional, rules work without it | Proven in the prototype, now with better inputs |
| 8 | **Lyrics + Lyric Overlay link** | LRCLIB/local LRC, stage view synced to the real deck position, MPRIS on Linux / SMTC on Windows | Lyric-linked FX need stems + director first |
| 9 | **Spotify** | PKCE import, local matching, store links for missing tracks, export sets as playlists | Library growth, independent of the engine |
| 10 | **Recording & release polish** | Record the master to file, auto-updater, signed builds | Ship it |
