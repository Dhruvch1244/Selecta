//! `cargo run --release -p selecta-core --example analyze -- FILE...`
//! Prints `[{path, analysis}]` as JSON: handy for checking the analyser on
//! real music, and it generates the UI's mock-backend fixtures.

use selecta_core::analysis::analyze_pcm;
use selecta_core::decode::{decode_mono, to_analysis_rate};

fn main() {
    let out: Vec<serde_json::Value> = std::env::args()
        .skip(1)
        .filter_map(|p| {
            let t = std::time::Instant::now();
            let pcm = to_analysis_rate(decode_mono(std::path::Path::new(&p)).map_err(|e| eprintln!("{p}: {e}")).ok()?);
            // Same genre hint the app applies (artist from "Artist - Title", folder names).
            let stem = std::path::Path::new(&p).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let artist = stem.split_once(" - ").map(|(a, _)| a.to_string()).unwrap_or_default();
            let a = analyze_pcm(&pcm, selecta_core::genres::bpm_hint("", &artist, &p));
            eprintln!(
                "{:>6.0} ms  {:>7.2} BPM  {:>4}  grid {:>4.1}  drops {:?}  {p}",
                t.elapsed().as_secs_f64() * 1e3,
                a.bpm,
                a.camelot,
                a.grid_conf,
                a.drops
            );
            Some(serde_json::json!({ "path": p, "analysis": a }))
        })
        .collect();
    println!("{}", serde_json::to_string(&out).unwrap());
}
