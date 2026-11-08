//! Effects: the catalog (what exists) and the director (what plays, when, and why).
//!
//! The prototype fired effects by dice roll at phrase boundaries, and it
//! sounded random. Here every cue comes from a rule tied to the music, and
//! carries that rule as its `reason`:
//! - **Build**: the 4-8 bars before a drop get tension (riser, roll build,
//!   HP sweep), more and longer at higher chaos.
//! - **Hit**: the drop's downbeat gets an impact, sub drop or genre-
//!   appropriate horn/siren, not every drop, and never two in a row.
//! - **Throw**: an echo or reverb throw on the last beat before a phrase
//!   change into a *quieter* section, which is where real DJs throw.
//! - **Budget**: at most one cue per 8-bar phrase outside builds and drops,
//!   scaled by chaos, so effects punctuate rather than smother.
//!
//! An id is `family:param`; the engine implements each family once
//! (packages/engine/src/fx.ts) and a test there checks it covers this catalog.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::analysis::Analysis;
use crate::planner::Rng;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FxTarget {
    /// Processes one deck's audio.
    Deck,
    /// A synthesised one-shot over the whole mix.
    Master,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FxDef {
    pub id: String,
    pub family: String,
    pub label: String,
    /// build | hit | echo | space | roll | filter | gate | mod | dist | tape | horn
    pub category: String,
    /// Default length in beats.
    pub beats: f64,
    pub target: FxTarget,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FxCue {
    /// Track bar (fractional = beats within the bar) where the effect starts.
    pub bar: f64,
    pub fx: String,
    pub beats: f64,
    /// Why the director placed it, shown in the hint bar.
    pub reason: String,
}

/// (family, params, category, beats per param or fixed, target, label)
struct Family {
    family: &'static str,
    params: &'static [&'static str],
    category: &'static str,
    beats: fn(&str) -> f64,
    target: FxTarget,
    label: &'static str,
}

fn frac(p: &str) -> f64 {
    match p.split_once('/') {
        Some((a, b)) => a.parse::<f64>().unwrap_or(1.0) / b.parse::<f64>().unwrap_or(1.0),
        None => p.parse().unwrap_or(1.0),
    }
}

const FAMILIES: &[Family] = &[
    Family {
        family: "echo",
        params: &["1/8", "3/16", "1/4", "1/2"],
        category: "echo",
        beats: |_| 4.0,
        target: FxTarget::Deck,
        label: "Echo",
    },
    Family {
        family: "throw",
        params: &["1/8", "3/16", "1/4"],
        category: "echo",
        beats: |_| 1.0,
        target: FxTarget::Deck,
        label: "Echo Throw",
    },
    Family {
        family: "reverb",
        params: &["hall", "plate", "wash"],
        category: "space",
        beats: |_| 4.0,
        target: FxTarget::Deck,
        label: "Reverb",
    },
    Family {
        family: "rev_throw",
        params: &["hall", "plate"],
        category: "space",
        beats: |_| 1.0,
        target: FxTarget::Deck,
        label: "Reverb Throw",
    },
    Family {
        family: "roll",
        params: &["1", "1/2", "1/4", "1/8"],
        category: "roll",
        beats: |p| if frac(p) >= 1.0 { 4.0 } else { 2.0 },
        target: FxTarget::Deck,
        label: "Loop Roll",
    },
    Family {
        family: "roll_build",
        params: &["1", "2"],
        category: "build",
        beats: |p| frac(p) * 4.0,
        target: FxTarget::Deck,
        label: "Roll Build",
    },
    Family { family: "stutter", params: &["1/8", "1/16"], category: "roll", beats: |_| 1.0, target: FxTarget::Deck, label: "Stutter" },
    Family {
        family: "hp_sweep",
        params: &["2", "4", "8"],
        category: "build",
        beats: |p| frac(p) * 4.0,
        target: FxTarget::Deck,
        label: "HP Sweep",
    },
    Family {
        family: "lp_sweep",
        params: &["2", "4"],
        category: "filter",
        beats: |p| frac(p) * 4.0,
        target: FxTarget::Deck,
        label: "LP Sweep",
    },
    Family {
        family: "gate",
        params: &["trance", "half", "offbeat", "chop"],
        category: "gate",
        beats: |_| 8.0,
        target: FxTarget::Deck,
        label: "Gate",
    },
    Family { family: "flanger", params: &["slow", "jet"], category: "mod", beats: |_| 8.0, target: FxTarget::Deck, label: "Flanger" },
    Family { family: "phaser", params: &["slow", "fast"], category: "mod", beats: |_| 8.0, target: FxTarget::Deck, label: "Phaser" },
    Family { family: "crush", params: &["8", "4"], category: "dist", beats: |_| 4.0, target: FxTarget::Deck, label: "Bitcrush" },
    Family { family: "tape_stop", params: &["1", "2"], category: "tape", beats: |p| frac(p), target: FxTarget::Deck, label: "Tape Stop" },
    Family { family: "backspin", params: &["1", "2"], category: "tape", beats: |p| frac(p), target: FxTarget::Deck, label: "Backspin" },
    Family { family: "rewind", params: &["8"], category: "tape", beats: |_| 4.0, target: FxTarget::Deck, label: "Rewind" },
    Family { family: "riser", params: &["4", "8"], category: "build", beats: |p| frac(p) * 4.0, target: FxTarget::Master, label: "Riser" },
    Family { family: "impact", params: &["boom", "crash"], category: "hit", beats: |_| 2.0, target: FxTarget::Master, label: "Impact" },
    Family { family: "sub_drop", params: &["2"], category: "hit", beats: |p| frac(p), target: FxTarget::Master, label: "Sub Drop" },
    Family { family: "horn", params: &["triple", "single"], category: "horn", beats: |_| 4.0, target: FxTarget::Master, label: "Air Horn" },
    Family { family: "siren", params: &["dub"], category: "horn", beats: |_| 8.0, target: FxTarget::Master, label: "Dub Siren" },
];

pub fn catalog() -> Vec<FxDef> {
    FAMILIES
        .iter()
        .flat_map(|f| {
            f.params.iter().map(move |p| FxDef {
                id: format!("{}:{p}", f.family),
                family: f.family.into(),
                label: format!("{} {p}", f.label),
                category: f.category.into(),
                beats: (f.beats)(p),
                target: f.target,
            })
        })
        .collect()
}

pub fn def(id: &str) -> Option<FxDef> {
    catalog().into_iter().find(|d| d.id == id)
}

/// Genre style for hits: which drop hits suit it (weights).
fn hits_for(genre: Option<&str>) -> &'static [(&'static str, f64)] {
    match genre {
        Some("dnb" | "dubstep") => &[("impact:boom", 3.0), ("sub_drop:2", 3.0), ("siren:dub", 2.0), ("horn:triple", 1.0)],
        Some("bollywood" | "punjabi" | "edm") => &[("horn:triple", 4.0), ("impact:boom", 2.0), ("sub_drop:2", 1.0)],
        Some("desi_hiphop" | "hiphop" | "trap") => &[("impact:boom", 2.0), ("horn:single", 2.0), ("sub_drop:2", 2.0)],
        Some("house" | "techno" | "trance") => &[("impact:crash", 3.0), ("sub_drop:2", 1.0)],
        _ => &[("impact:boom", 2.0), ("impact:crash", 1.0)],
    }
}

fn throws_for(genre: Option<&str>) -> &'static [(&'static str, f64)] {
    match genre {
        Some("house" | "techno" | "trance" | "lofi" | "rnb") => &[("rev_throw:plate", 2.0), ("throw:3/16", 2.0), ("rev_throw:hall", 1.0)],
        _ => &[("throw:1/4", 2.0), ("throw:3/16", 2.0), ("rev_throw:plate", 1.0)],
    }
}

fn flavour_for(genre: Option<&str>) -> &'static [(&'static str, f64)] {
    match genre {
        Some("dnb" | "dubstep") => &[("roll:1/4", 2.0), ("stutter:1/16", 2.0), ("gate:chop", 1.0), ("phaser:fast", 1.0), ("crush:8", 1.0)],
        Some("house" | "techno") => &[("gate:trance", 2.0), ("flanger:slow", 2.0), ("lp_sweep:2", 2.0), ("phaser:slow", 1.0)],
        Some("trance" | "edm") => &[("gate:trance", 3.0), ("roll:1/2", 1.0), ("flanger:jet", 1.0)],
        Some("desi_hiphop" | "hiphop" | "trap") => &[("stutter:1/8", 2.0), ("tape_stop:1", 1.0), ("crush:8", 1.0), ("roll:1/2", 1.0)],
        Some("bollywood" | "punjabi") => &[("roll:1/2", 2.0), ("echo:3/16", 1.0), ("gate:half", 1.0)],
        _ => &[("echo:3/16", 1.0), ("lp_sweep:2", 1.0), ("flanger:slow", 1.0)],
    }
}

/// Place cues over the played region [in_bar, out_bar) of one track.
/// `busy_until`: bar where the outgoing transition starts (kept clean).
pub fn direct(a: &Analysis, genre: Option<&str>, in_bar: u32, transition_from: u32, chaos: f64, rng: &mut Rng) -> Vec<FxCue> {
    let mut cues: Vec<FxCue> = vec![];
    let lo = in_bar as f64 + 4.0; // let the track land first
    let hi = transition_from as f64 - 1.0; // keep the outgoing transition clean
    if hi - lo < 4.0 {
        return cues;
    }
    let mut taken: Vec<(f64, f64)> = vec![];
    let free = |taken: &Vec<(f64, f64)>, a: f64, b: f64| taken.iter().all(|(x, y)| b <= *x || a >= *y);
    let put = |cues: &mut Vec<FxCue>, taken: &mut Vec<(f64, f64)>, bar: f64, id: &str, reason: String| -> bool {
        let Some(d) = def(id) else { return false };
        let end = bar + d.beats / 4.0;
        if bar < lo || end > hi || !free(taken, bar, end) {
            return false;
        }
        taken.push((bar, end));
        cues.push(FxCue { bar, fx: id.into(), beats: d.beats, reason });
        true
    };

    // Builds and hits around drops.
    let mut last_hit = false;
    for &d in &a.drops {
        let d = d as f64;
        if !(lo + 4.0..hi).contains(&d) {
            continue;
        }
        if rng.next_f64() < 0.3 + 0.65 * chaos {
            let len = if chaos > 0.6 { 8.0 } else { 4.0 };
            let choice = *rng.pick(&[("riser", 3.0), ("roll_build", 2.0 * chaos + 0.5), ("hp_sweep", 2.0)]);
            let id = match choice {
                "riser" => format!("riser:{}", len as u32),
                "roll_build" => format!("roll_build:{}", if chaos > 0.6 { 2 } else { 1 }),
                _ => format!("hp_sweep:{}", len as u32),
            };
            let beats = def(&id).map(|x| x.beats).unwrap_or(16.0);
            put(&mut cues, &mut taken, d - beats / 4.0, &id, format!("Build into the drop at bar {d}"));
        }
        // Hit the one, but not every drop and never twice in a row.
        if !last_hit && rng.next_f64() < 0.25 + 0.6 * chaos {
            let id = *rng.pick(hits_for(genre));
            // Hits are master one-shots; they may overlap the build's last beat.
            if let Some(dd) = def(id) {
                cues.push(FxCue { bar: d, fx: id.into(), beats: dd.beats, reason: format!("Hit on the drop at bar {d}") });
                taken.push((d, d + 0.5));
                last_hit = true;
                continue;
            }
        }
        last_hit = false;
    }

    // Throws into quieter sections (phrase end -> lower energy).
    for w in a.sections.windows(2) {
        let (cur, next) = (&w[0], &w[1]);
        let b = next.start as f64;
        if next.energy < cur.energy - 0.12 && rng.next_f64() < 0.35 + 0.5 * chaos {
            let id = *rng.pick(throws_for(genre));
            put(&mut cues, &mut taken, b - 0.25, id, format!("Throw into the breakdown at bar {b}"));
        }
    }

    // Phrase flavour, on budget: at most one per 8-bar phrase, only in loud sections.
    let start = (lo / 8.0).ceil() as u32 * 8;
    let mut b = start;
    while (b as f64) < hi - 4.0 {
        let e = a.bar_energy.get(b as usize).copied().unwrap_or(0.0) as f64;
        // The 8 bars after a drop are the payoff: leave them alone.
        let after_drop = a.drops.iter().any(|&d| (d..d + 8).contains(&b) || (d..d + 8).contains(&(b + 7)));
        if !after_drop && e > 0.6 && rng.next_f64() < 0.12 + 0.45 * chaos {
            let id = *rng.pick(flavour_for(genre));
            if let Some(dd) = def(id) {
                // Short moves end on the phrase line; long ones start on it.
                let (at, reason) = if dd.beats <= 2.0 {
                    (b as f64 + 8.0 - dd.beats / 4.0, format!("Fill into the phrase at bar {}", b + 8))
                } else {
                    (b as f64, format!("Colour over the phrase at bar {b}"))
                };
                put(&mut cues, &mut taken, at, id, reason);
            }
        }
        b += 8;
    }
    cues.sort_by(|x, y| x.bar.partial_cmp(&y.bar).unwrap());
    cues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{
        analyze_pcm,
        tests::{synth, LAYOUT},
    };

    #[test]
    fn catalog_ids_are_unique_and_parse() {
        let c = catalog();
        let mut ids: Vec<&str> = c.iter().map(|d| d.id.as_str()).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(n, ids.len());
        assert!(c.iter().all(|d| d.beats > 0.0 && d.id.starts_with(&format!("{}:", d.family))));
        assert!(n >= 40, "{n} effects");
    }

    #[test]
    fn director_builds_into_drops_and_keeps_transitions_clean() {
        let a = analyze_pcm(&synth(174.0, 0.3, &LAYOUT, 22_050, 1), Some((160.0, 180.0)));
        let mut rng = Rng::new(9);
        let cues = direct(&a, Some("dnb"), 0, 110, 0.9, &mut rng);
        assert!(!cues.is_empty());
        let builds: Vec<&FxCue> = cues.iter().filter(|c| c.reason.starts_with("Build")).collect();
        assert!(!builds.is_empty(), "high chaos should build into a drop: {cues:?}");
        for c in builds {
            assert!(a.drops.iter().any(|&d| (c.bar + c.beats / 4.0 - d as f64).abs() < 1e-9), "build must end exactly on a drop: {c:?}");
        }
        for c in cues.iter().filter(|c| c.reason.starts_with("Fill") || c.reason.starts_with("Colour")) {
            assert!(
                !a.drops.iter().any(|&d| c.bar < d as f64 + 8.0 && c.bar + c.beats / 4.0 > d as f64),
                "flavour on a drop's first 8 bars: {c:?}"
            );
        }
        for c in &cues {
            assert!(c.bar >= 4.0 && c.bar + c.beats / 4.0 <= 109.0 + 1e-9, "cue outside the clean window: {c:?}");
            assert!(def(&c.fx).is_some(), "unknown fx {}", c.fx);
            assert!(!c.reason.is_empty());
        }
        // Chaos 0 is calm: never more cues than chaos 1 on the same track.
        let calm = direct(&a, Some("dnb"), 0, 110, 0.0, &mut Rng::new(9));
        assert!(calm.len() <= cues.len());
    }

    #[test]
    fn export_catalog_for_the_engine() {
        // The engine's test checks it implements every family in this file.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/protocol/src/generated");
        std::fs::write(dir.join("fx-catalog.json"), serde_json::to_string_pretty(&catalog()).unwrap() + "\n").unwrap();
    }
}
