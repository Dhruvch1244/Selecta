//! Plan sets from a real library database, exactly as the app does (rules
//! only, no AI). For testing against a real library without the app:
//!
//!   cargo run --release -p selecta-core --example plan_db -- COPY_OF_library.db [--keylock] [--reanalyse] [--ai] "PROMPT"...
//!
//! Prints `{prompt: SetPlan}` JSON. Pass a *copy* of the database: opening
//! runs migrations.

use std::collections::BTreeMap;
use std::path::Path;

use selecta_core::analysis::analyze_pcm;
use selecta_core::compose;
use selecta_core::decode::{decode_mono, to_analysis_rate};
use selecta_core::intent;
use selecta_core::library::Library;
use selecta_core::planner::{plan, Options};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (db, rest) = args.split_first().expect("usage: plan_db DB [--keylock] PROMPT...");
    let keylock = rest.iter().any(|a| a == "--keylock");
    let use_ai = rest.iter().any(|a| a == "--ai");
    let lib = Library::open(Path::new(db)).expect("open library");
    if rest.iter().any(|a| a == "--reanalyse") {
        // Re-analyse stale tracks in this copy, in parallel, as the app's scan does.
        use rayon::prelude::*;
        let todo = lib.needs_analysis().expect("pending");
        let t = std::time::Instant::now();
        let done: Vec<_> = todo
            .par_iter()
            .map(|(id, path, hint)| (id.clone(), decode_mono(path).map(|p| analyze_pcm(&to_analysis_rate(p), *hint))))
            .collect();
        for (id, a) in done {
            match a {
                Ok(a) => lib.save_analysis(&id, &a).expect("save"),
                Err(e) => lib.save_analysis_error(&id, &e.to_string()).expect("save"),
            }
        }
        eprintln!("re-analysed {} tracks in {:.1}s", todo.len(), t.elapsed().as_secs_f64());
    }
    let cands = lib.candidates().expect("candidates");
    let artists: Vec<String> = cands.iter().map(|c| c.track.artist.clone()).collect();
    let mut out = BTreeMap::new();
    for (i, prompt) in rest.iter().filter(|a| !a.starts_with("--")).enumerate() {
        let mut it = intent::parse(prompt, &artists);
        let p = if use_ai {
            // Exactly the app's pipeline: Claude reads, places the songs, then writes the notes.
            let ai = selecta_core::llm::AiConfig::default();
            let say = |m: &str| eprintln!("  · {m}");
            let mut notes = vec![];
            let t = std::time::Instant::now();
            let mut p = compose::build_set(&mut it, &cands, &ai, true, 1000 + i as u64, keylock, &|_| {}, &say, &mut notes).expect("plan");
            eprintln!("  built in {:.0}s", t.elapsed().as_secs_f64());
            p.notes.extend(notes);
            p
        } else {
            plan(&it, &cands, &Options { seed: 1000 + i as u64, exclude: vec![], curation: None, keylock }).expect("plan")
        };
        out.insert(prompt.clone(), p);
    }
    println!("{}", serde_json::to_string(&out).unwrap());
}
