//! Prompt + library -> set: the whole pipeline, AI-led when the AI is on.
//!
//! 1. The AI reads the request (genres, energy, pacing, mashup appetite).
//! 2. The AI places the songs: a running order with a transition per pair,
//!    from per-track facts (tempo, key, energy, vocals, hooks, drops).
//! 3. The planner scripts that order exactly: cue points, beatmatching, stem
//!    moves, FX (falling back only where a pair can't support a move).
//! 4. The AI writes the notes from the final set, so they match what plays.
//!
//! Lives in the core (not the desktop shell) so tools and tests run exactly
//! what the app runs.

use crate::intent::Intent;
use crate::llm::{self, AiConfig};
use crate::planner::{self, Candidate, SetPlan};

/// `restyle` re-applies hand-set switches after the AI's reading (they win);
/// `say` reports progress; `notes` collects AI failures (never fatal).
#[allow(clippy::too_many_arguments)]
pub fn build_set(
    it: &mut Intent,
    cands: &[Candidate],
    ai: &AiConfig,
    ai_on: bool,
    seed: u64,
    keylock: bool,
    restyle: &dyn Fn(&mut Intent),
    say: &dyn Fn(&str),
    notes: &mut Vec<String>,
) -> Result<SetPlan, String> {
    // The AI builds the set, end to end, on the user's model (no fast-model
    // shortcut, no rules drafts: a good set is worth a few minutes).
    if ai_on {
        say(&format!("Claude ({}) is reading the request", ai.model));
        match llm::refine_intent(ai, it) {
            Ok(()) => restyle(it), // switches and knobs set by hand win over the AI's reading
            Err(e) => notes.push(format!("AI couldn't read the prompt ({e}); used rules.")),
        }
    }
    let mut curation = None;
    if ai_on && ai.curate {
        let wide = planner::shortlist(it, cands, &[], 220);
        if !wide.is_empty() {
            let per = if it.rapid { 60.0 } else { 110.0 };
            let need = ((it.duration_min * 60.0 / per).round() as usize).max(3);
            say(&format!(
                "Claude ({}) is placing the songs: {} candidates with tempo, key, energy, vocals, hooks and drops",
                ai.model,
                wide.len()
            ));
            match llm::curate(ai, it, &wide, need) {
                Ok(c) => {
                    say(&format!("Claude placed {} songs and chose {} transitions", c.order.len(), c.moves.len()));
                    curation = Some(c);
                }
                Err(e) => notes.push(format!("AI curation unavailable ({e}); planned by rules.")),
            }
        }
    }
    say(if curation.is_some() {
        "Scripting Claude's running order: cue points, beatmatching, vocal hooks"
    } else {
        "Ordering by key, tempo and energy arc"
    });
    let mut plan = planner::plan(it, cands, &planner::Options { seed, exclude: vec![], curation, keylock })?;
    if ai_on {
        // Written last, from the set as it will actually play.
        say(&format!("Claude ({}) is writing the set's notes from the final running order", ai.model));
        match llm::liner_notes(ai, &plan) {
            Ok(n) => {
                if let Some(t) = n.title {
                    plan.title = t;
                }
                if n.vibe.is_some() {
                    plan.intent.vibe = n.vibe;
                }
                plan.notes.retain(|x| !x.starts_with("DJ idea"));
                for (i, note) in n.moments {
                    plan.notes.push(format!("{} · {}: {note}", i + 1, plan.tracks[i].title));
                }
            }
            Err(e) => notes.push(format!("AI notes unavailable ({e}).")),
        }
    }
    Ok(plan)
}
