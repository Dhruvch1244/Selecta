# Roadmap

Each item is one feature branch and one pull request. The order puts what
makes everything after it better first.

| # | PR | What lands | Status |
|---|---|---|---|
| 1 | **Foundation** | Stack, design system, CI/CD, Rust analysis, scheduler, two decks with sync/EQ/filter/meters | merged |
| 2 | **Set planner** | Prompt → intent → selection → beam-search ordering (key, tempo, energy arc) → phrase-aligned cues; conductor performs blend / bass swap / double drop / filter / echo out / slam; Set panel timeline | this PR |
| 3 | **AI curation (Claude CLI)** | Intent refinement, track vetting by real song knowledge (bangers vs ballads), running order, DJ moments, library genre/mood tagging. Optional: rules work without it | next |
| 4 | **FX director** | Effects with intent, not dice: tension/release over sections and lyrics (builds into drops, throws on line ends, rolls, rewinds), plus an FX pad. Adds backspin / tape stop / rewind / loop-roll transitions | |
| 5 | **Lyrics + Lyric Overlay link** | LRCLIB / local LRC, stage view synced to the real deck position, MPRIS on Linux | |
| 6 | **Spotify** | PKCE import, local matching, store links for missing tracks, export sets as playlists | |
| 7 | **Neural beat tracking** | Beat + downbeat model via ONNX Runtime, replacing the DSP grid stage; best on live-drummer tracks | |
| 8 | **Stem separation** | Demucs (ONNX) on GPU/CPU, cached stems, 4 stems per deck: acapella intros, vocal-only throws, drum-only blends | |
| 9 | **Keylock** | WASM time-stretcher in an AudioWorklet: tempo without pitch, wider beatmatch window | |
| 10 | **Recording & release polish** | Record the master, auto-updater, signed builds | |

Order changed after PR 1 at the user's request: the features you see and
use (planner, AI, FX, lyrics) come before the deeper audio upgrades.
