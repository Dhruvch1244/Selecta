//! How far each track's beat grid sits from its real kick transients.
//!
//!   cargo run --release -p selecta-core --example grid_check -- FILE...
//!
//! For every file: analyse (as the app does), then measure the offset
//! (-60..+60 ms) at which the grid's beats best line up with the kicks
//! (`kick_phase_offset`). That is the grid's error: two decks with errors of
//! opposite sign flam by their difference. After the kick refinement it should
//! be ~0 on every track with clear kicks.

use std::path::Path;

use selecta_core::analysis::{analyze_pcm, kick_phase_offset};
use selecta_core::decode::{decode_mono, to_analysis_rate};
use selecta_core::genres;

fn main() {
    for f in std::env::args().skip(1) {
        let p = Path::new(&f);
        let Ok(pcm) = decode_mono(p) else { continue };
        let pcm = to_analysis_rate(pcm);
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        let a = analyze_pcm(&pcm, genres::bpm_hint("", &name, &f));
        // Residual error of the final grid (wider window than the analysis uses, to see outliers).
        let (off, sharp) = kick_phase_offset(&pcm.samples, pcm.rate as f64, a.beat, a.first_beat, 0.06);
        let off = off * 1000.0;
        // Drift: the same measurement on each third of the track (a wrong BPM shows as a trend).
        let n = pcm.samples.len();
        let thirds: Vec<String> = (0..3)
            .map(|k| {
                let (s0, s1) = (k * n / 3, (k + 1) * n / 3);
                let t0 = s0 as f64 / pcm.rate as f64;
                let ph = (a.first_beat - t0).rem_euclid(a.beat);
                let (o, _) = kick_phase_offset(&pcm.samples[s0..s1], pcm.rate as f64, a.beat, ph, 0.06);
                format!("{:+4.0}", o * 1000.0)
            })
            .collect();
        print!("thirds [{}]  ", thirds.join(" "));
        println!("{off:+6.1} ms  sharp {sharp:4.2}  {:6.1} BPM  {}", a.bpm, name.chars().take(50).collect::<String>());
    }
}
