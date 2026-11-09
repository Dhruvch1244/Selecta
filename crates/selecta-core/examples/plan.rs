//! `cargo run --release -p selecta-core --example plan -- "PROMPT" FILE...`
//! Analyses the files, plans a set for the prompt with the real planner, and
//! prints `{library, plan}` as JSON (the UI mock backend's fixture format).
//! Files named "Artist - Title.ext" get those as metadata.

use std::path::Path;

use selecta_core::analysis::analyze_pcm;
use selecta_core::decode::{decode_mono, to_analysis_rate};
use selecta_core::library::{track_id, TrackSummary};
use selecta_core::planner::{plan, Candidate, Options};
use selecta_core::{genres, intent};

fn main() {
    let mut args = std::env::args().skip(1);
    let prompt = args.next().expect("usage: plan \"PROMPT\" FILE...");
    let files: Vec<String> = args.collect();
    let cands: Vec<Candidate> = files
        .iter()
        .filter_map(|p| {
            let stem = Path::new(p).file_stem()?.to_string_lossy().to_string();
            let (artist, title) = stem.split_once(" - ").map(|(a, t)| (a.to_string(), t.to_string())).unwrap_or((String::new(), stem));
            let hint = genres::bpm_hint("", &artist, p);
            let a = analyze_pcm(&to_analysis_rate(decode_mono(Path::new(p)).ok()?), hint);
            let track = TrackSummary {
                id: track_id(p),
                path: p.clone(),
                artist: artist.clone(),
                title,
                album: String::new(),
                genre_tag: String::new(),
                duration: a.duration,
                analyzed: true,
                bpm: Some(a.bpm),
                key: Some(a.key.clone()),
                camelot: Some(a.camelot.clone()),
                mood: None,
                error: None,
<<<<<<< HEAD
                stems: false,
            };
            Some(Candidate { genres: genres::classify("", &artist, p, Some(a.bpm)), track, analysis: a, tags: None, vocals: None })
=======
            };
            Some(Candidate { genres: genres::classify("", &artist, p, Some(a.bpm)), track, analysis: a, tags: None })
>>>>>>> origin/main
        })
        .collect();
    let artists: Vec<String> = cands.iter().map(|c| c.track.artist.clone()).collect();
    let it = intent::parse(&prompt, &artists);
    let p = plan(&it, &cands, &Options { seed: 42, exclude: vec![], curation: None }).unwrap_or_else(|e| panic!("{e}"));
    for t in &p.tracks {
        eprintln!(
            "{:<20} {:<26} {:>6.1}→{:>6.1} {:>4}  bars {:>3}-{:<3} {:?}",
            t.artist,
            t.title,
            t.bpm,
            t.bpm * t.rate,
            t.camelot,
            t.in_bar,
            t.out_bar,
            t.trans.kind
        );
    }
    let lib: Vec<_> = cands.iter().map(|c| serde_json::json!({ "path": c.track.path, "analysis": c.analysis })).collect();
    println!("{}", serde_json::json!({ "library": lib, "plan": p }));
}
