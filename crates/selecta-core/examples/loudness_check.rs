//! `cargo run --release -p selecta-core --example loudness_check -- FILE...`
//! Prints `path<TAB>loudness LUFS<TAB>peak dBFS<TAB>gain dB` per file, as the
//! analyser measures them: compare with a reference meter (pyloudnorm), and
//! check the spread of `loudness + gain` across a library.

use selecta_core::analysis::{loud_parts_lufs, norm_gain};
use selecta_core::decode::{decode_mono, to_analysis_rate};

fn main() {
    for p in std::env::args().skip(1) {
        let Ok(pcm) = decode_mono(std::path::Path::new(&p)).map(to_analysis_rate) else {
            eprintln!("{p}: can't decode");
            continue;
        };
        let l = loud_parts_lufs(&pcm.samples, pcm.rate as f64);
        let peak = 20.0 * (pcm.samples.iter().fold(0f32, |m, v| m.max(v.abs())) as f64 + 1e-9).log10();
        println!("{p}\t{l:.2}\t{peak:.2}\t{:.2}", norm_gain(l, peak));
    }
}
