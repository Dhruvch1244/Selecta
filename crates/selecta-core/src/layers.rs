//! Layers: a loop of one song played over another on the extra decks (C, D).
//!
//! What a DJ with more than two decks does: a sung hook from an earlier
//! track comes back over a later one (callback), the next track's hook is
//! teased over the current one before it arrives (tease), a signature loop
//! keeps returning through the set (anthem), or a loop from a song that is
//! not even in the set rides several tracks (tool: one famous bassline over
//! a run of drum & bass). Vocal layers loop the vocal stem; riff layers loop
//! a drop's instrumental and swap out the host's own bass. The engine
//! time-stretches and key-shifts every loop to the host as it plays.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::intent::Intent;
use crate::planner::{camelot_score, phrases, shift_camelot, vocal_hook, vox, Candidate, Rng, SetTrack};

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LayerKind {
    /// From a track that already played.
    Callback,
    /// From the next track, before it comes in.
    Tease,
    /// The set's recurring loop (the same source over several tracks).
    Anthem,
    /// From a library track that isn't in the set.
    Tool,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LayerPart {
    /// The source's vocal stem; the host's vocals are ducked.
    Vocal,
    /// The source's drop, vocals removed; the host's bass is swapped out.
    Riff,
}

impl LayerPart {
    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "vocal" | "vocals" | "hook" | "acapella" => Some(Self::Vocal),
            "riff" | "bass" | "bassline" | "instrumental" | "beat" => Some(Self::Riff),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Layer {
    pub kind: LayerKind,
    pub part: LayerPart,
    /// Source track.
    pub id: String,
    pub path: String,
    pub artist: String,
    pub title: String,
    /// Loop in source bars: `src_bars` bars from `src_bar`.
    pub src_bar: u32,
    pub src_bars: u32,
    /// Host bars the loop plays over: [from_bar, to_bar).
    pub from_bar: u32,
    pub to_bar: u32,
    /// Host beats per source beat (1, or 2 / 0.5 for a half/double-time vocal).
    pub beat_ratio: f64,
    /// Semitones the source is shifted to sit in the host's key.
    pub key_shift: i32,
    pub reason: String,
}

/// An AI request: loop `src`'s `part` over the tracks in `over`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiLayer {
    pub src: String,
    pub over: Vec<String>,
    pub part: Option<LayerPart>,
}

/// The loop a source offers for a part: (bar, bars).
pub fn material(c: &Candidate, part: LayerPart) -> Option<(u32, u32)> {
    let a = &c.analysis;
    match part {
        // A 4-bar piece of its sung hook (vocal stem required).
        LayerPart::Vocal => vocal_hook(c).map(|(s, _)| (s, 4)),
        // The first 4 bars of a drop that is energetic and (when known) not sung over.
        LayerPart::Riff => a
            .drops
            .iter()
            .copied()
            .find(|d| {
                let e = &a.bar_energy;
                let (lo, hi) = (*d as usize, (*d as usize + 4).min(e.len()));
                *d + 8 < a.bars
                    && hi > lo
                    && e[lo..hi].iter().sum::<f32>() / (hi - lo) as f32 >= 0.6
                    && c.vocals.as_ref().is_none_or(|_| vox(c, *d, *d + 4) < 0.3)
            })
            .map(|d| (d, 4)),
    }
}

/// (host beats per source beat, playback rate of the source) to sit on the
/// host's played tempo, or None when the stretch would be audible.
pub fn fit_tempo(src_bpm: f64, host_bpm: f64, part: LayerPart) -> Option<(f64, f64)> {
    let (ratios, win): (&[f64], f64) = match part {
        LayerPart::Vocal => (&[1.0, 2.0, 0.5], 0.12),
        // Drums and bass smear when stretched far, and don't sit half-time.
        LayerPart::Riff => (&[1.0], 0.08),
    };
    ratios
        .iter()
        .map(|m| (*m, host_bpm / (m * src_bpm)))
        .filter(|(_, r)| (r - 1.0).abs() <= win)
        .min_by(|a, b| (a.1 - 1.0).abs().total_cmp(&(b.1 - 1.0).abs()))
}

/// (semitones, harmonic score) for the source over the host's key. Riffs
/// carry a bassline, so they need confident keys and a real match.
pub fn fit_key(src: &Candidate, host: &SetTrack, host_conf: f64, part: LayerPart) -> Option<(i32, f64)> {
    let hk = shift_camelot(&host.camelot, host.key_shift);
    let confident = src.analysis.key_conf >= 0.6 && host_conf >= 0.6;
    if !confident {
        return (part == LayerPart::Vocal).then_some((0, 0.5));
    }
    let need = if part == LayerPart::Riff { 0.85 } else { 0.7 };
    (-2..=2)
        .map(|ks| (ks, camelot_score(&shift_camelot(&src.analysis.camelot, ks), &hk) - 0.06 * (ks as f64).abs()))
        .filter(|(ks, s)| *s + 0.06 * (*ks as f64).abs() >= need)
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// Host bars free of transitions, for track `i`: after the previous track
/// has gone and a bar before the next transition starts.
pub fn solo_range(tracks: &[SetTrack], i: usize) -> (u32, u32) {
    let t = &tracks[i];
    let lo = if i == 0 { t.in_bar + 16 } else { t.in_bar + (t.trans.from_bar + t.trans.len).saturating_sub(t.trans.enter_at) + 2 };
    let hi = tracks.get(i + 1).map(|n| n.trans.from_bar.saturating_sub(1)).unwrap_or(t.out_bar.saturating_sub(8));
    (lo, hi)
}

struct Opt {
    host: usize,
    src: usize,
    part: LayerPart,
    kind: LayerKind,
    from: u32,
    to: u32,
    ratio: f64,
    shift: i32,
    loop_at: (u32, u32),
    score: f64,
}

/// Effects that take over the host deck's playback (rolls, stutters, stops):
/// a loop riding on top would keep going while the host stutters.
fn takes_over_playback(fx: &str) -> bool {
    ["roll:", "stutter:", "tape_stop:", "gate:", "brake:", "backspin:"].iter().any(|p| fx.starts_with(p))
}

/// Place layers on a scripted set. `hosts[i]` is the candidate behind
/// `tracks[i]`; `tools` are library tracks outside the set that fit the
/// prompt (with their fit). Returns notes on what was layered.
pub fn place(
    tracks: &mut [SetTrack],
    hosts: &[&Candidate],
    tools: &[(&Candidate, f64)],
    intent: &Intent,
    ai: &[AiLayer],
    rng: &mut Rng,
) -> Vec<String> {
    let n = tracks.len();
    if n < 2 {
        return vec![];
    }
    // Sources: the set's own tracks, then the tools.
    let srcs: Vec<(&Candidate, Option<usize>, f64)> =
        hosts.iter().enumerate().map(|(i, c)| (*c, Some(i), 1.0)).chain(tools.iter().map(|(c, f)| (*c, None, *f))).collect();
    let ai_wants = |src: &str, host: &str, part: LayerPart| {
        ai.iter().any(|l| l.src == src && l.over.iter().any(|h| h == host) && l.part.is_none_or(|p| p == part))
    };
    // "Bassline"/"riff" in the prompt asks for riff layers; "vocals"/"hook" for vocal ones.
    let said = |words: &[&str]| words.iter().any(|w| intent.prompt.to_lowercase().contains(w));
    let want_riff = said(&["bassline", "bass line", "riff", "basslines"]);
    let want_vocal = said(&["vocal", "acapella", "hook", "voice"]);
    let mut opts: Vec<Opt> = vec![];
    // Why an AI-requested (source, host) pair couldn't be layered, for the notes.
    let mut why_not: std::collections::BTreeMap<(String, String), &'static str> = Default::default();
    let mut refuse = |src: &Candidate, host: &Candidate, part: LayerPart, why: &'static str| {
        if ai_wants(&src.track.id, &host.track.id, part) {
            why_not.entry((src.track.title.clone(), host.track.title.clone())).or_insert(why);
        }
    };
    for i in 0..n {
        let (lo, hi) = solo_range(tracks, i);
        if hi < lo + 8 {
            for (s, _, _) in &srcs {
                refuse(s, hosts[i], LayerPart::Vocal, "its solo part is too short");
                refuse(s, hosts[i], LayerPart::Riff, "its solo part is too short");
            }
            continue;
        }
        let h = hosts[i];
        let ha = &h.analysis;
        let host_bpm = tracks[i].bpm * tracks[i].rate;
        let len = if intent.chaos >= 0.55 && hi >= lo + 16 { 16 } else { 8 };
        let starts: Vec<u32> = phrases(ha, lo, hi.saturating_sub(len)).into_iter().filter(|b| b + len <= hi).collect();
        if starts.is_empty() {
            continue;
        }
        for (si, (s, set_at, fit)) in srcs.iter().enumerate() {
            let kind = match set_at {
                Some(j) if *j + 2 <= i => LayerKind::Callback,
                Some(j) if *j == i + 1 && !tracks[i + 1].trans.kind.stems() => LayerKind::Tease,
                Some(_) => continue,
                None => LayerKind::Tool,
            };
            if s.track.id == h.track.id {
                continue;
            }
            for part in [LayerPart::Vocal, LayerPart::Riff] {
                // A tease is the next track's voice, not its beat.
                if kind == LayerKind::Tease && part == LayerPart::Riff {
                    continue;
                }
                let Some(loop_at) = material(s, part) else {
                    refuse(s, h, part, if part == LayerPart::Vocal { "no separated sung hook" } else { "no clean instrumental drop" });
                    continue;
                };
                let Some((ratio, rate)) = fit_tempo(s.analysis.bpm, host_bpm, part) else {
                    refuse(s, h, part, "tempos too far apart");
                    continue;
                };
                let Some((shift, key)) = fit_key(s, &tracks[i], ha.key_conf, part) else {
                    refuse(s, h, part, "keys clash");
                    continue;
                };
                // The best window: over the host's energetic bars, and for a
                // vocal where the host isn't singing; a tease sits as late as possible.
                let energy_at = |b: u32| -> f64 {
                    let e = &ha.bar_energy;
                    let (x, y) = ((b as usize).min(e.len()), ((b + len) as usize).min(e.len()));
                    if y > x {
                        e[x..y].iter().sum::<f32>() as f64 / (y - x) as f64
                    } else {
                        0.0
                    }
                };
                let window = |b: u32| -> f64 {
                    let energy = energy_at(b);
                    let free = match (part, h.vocals.is_some()) {
                        (LayerPart::Vocal, true) => 1.0 - (1.6 * vox(h, b, b + len)).min(1.0),
                        (LayerPart::Vocal, false) => 0.45,
                        (LayerPart::Riff, _) => 1.0,
                    };
                    let late = if kind == LayerKind::Tease { (b + len) as f64 / hi as f64 } else { 0.5 };
                    energy * if part == LayerPart::Riff { 1.4 } else { 0.8 } + free + 0.6 * late
                };
                let Some((from, wscore)) = starts.iter().map(|b| (*b, window(*b))).max_by(|a, b| a.1.total_cmp(&b.1)) else { continue };
                // A riff needs a drop to ride.
                if part == LayerPart::Riff && energy_at(from) < 0.55 {
                    refuse(s, h, part, "no drop to ride");
                    continue;
                }
                let bonus = match kind {
                    LayerKind::Callback => 0.3,
                    LayerKind::Tease => 0.4,
                    LayerKind::Tool => 0.5 * fit.min(1.0) - 0.1,
                    LayerKind::Anthem => 0.0,
                };
                let ai_bonus = if ai_wants(&s.track.id, &h.track.id, part) { 2.0 } else { 0.0 };
                let asked = match part {
                    LayerPart::Riff if want_riff => 0.6,
                    LayerPart::Vocal if want_vocal => 0.4,
                    _ => 0.0,
                };
                let score = wscore + 0.8 * key + 0.5 * (1.0 - 6.0 * (rate - 1.0).abs()) + bonus + ai_bonus + asked + rng.gauss(0.08);
                opts.push(Opt { host: i, src: si, part, kind, from, to: from + len, ratio, shift, loop_at, score });
            }
        }
    }

    // How many: a few per set by default, more for chaotic/mashup prompts.
    // Measured on a real library: 0.8 per track turned a house set into
    // constant riff-on-riff mud; a loop should be an event.
    let dens = (0.15 + 0.25 * intent.chaos + 0.15 * intent.mashups) * if intent.smooth { 0.5 } else { 1.0 };
    let ai_count = ai.iter().map(|l| l.over.len()).sum::<usize>();
    let target = ((n as f64 * dens).round() as usize).min(n / 2).max(ai_count.min(n - 1));
    // Vocal and riff from different sources may stack over one host (a
    // 3-4 deck mashup: host drums, one track's bass, another's voice).
    let stack = intent.chaos >= 0.75 || intent.mashups >= 0.7;
    let mut chosen: Vec<Opt> = vec![];
    while chosen.len() < target {
        // Re-scored every round: a source already layered gets a bonus (the
        // same part coming back is what makes a motif), but never on
        // neighbouring tracks and at most three times.
        let best = opts
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                let on_host: Vec<&Opt> = chosen.iter().filter(|c| c.host == o.host).collect();
                let host_ok = on_host.is_empty() || (stack && on_host.len() == 1 && on_host[0].part != o.part && on_host[0].src != o.src);
                let uses: Vec<&Opt> = chosen.iter().filter(|c| c.src == o.src).collect();
                host_ok && uses.len() < 3 && uses.iter().all(|u| u.host.abs_diff(o.host) >= 2 && u.part == o.part)
            })
            .map(|(k, o)| {
                let reuse = chosen.iter().any(|c| c.src == o.src && c.loop_at == o.loop_at);
                (k, o.score + if reuse { 0.6 } else { 0.0 })
            })
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let Some((k, _)) = best else { break };
        let mut o = opts.swap_remove(k);
        // Stacked layers share the window of the first one on that host.
        if let Some(first) = chosen.iter().find(|c| c.host == o.host) {
            (o.from, o.to) = (first.from, first.to);
        }
        chosen.push(o);
    }

    let mut notes = vec![];
    let mut motifs: Vec<usize> = vec![];
    for o in &chosen {
        if chosen.iter().filter(|c| c.src == o.src).count() >= 2 && !motifs.contains(&o.src) {
            motifs.push(o.src);
        }
    }
    chosen.sort_by_key(|o| (o.host, o.from));
    for o in chosen {
        let s = srcs[o.src].0;
        let kind = if motifs.contains(&o.src) { LayerKind::Anthem } else { o.kind };
        let what = match o.part {
            LayerPart::Vocal => "vocal hook",
            LayerPart::Riff => "riff",
        };
        let reason = match kind {
            LayerKind::Callback => format!("{}'s {what} comes back over this one", s.track.title),
            LayerKind::Tease => format!("teases {}'s {what} before it drops", s.track.title),
            LayerKind::Anthem => format!("{}'s {what} again: the set's recurring loop", s.track.title),
            LayerKind::Tool => format!("{}'s {what} as a tool over this drop", s.track.title),
        };
        let host = &mut tracks[o.host];
        // The host's own playback effects would fight the loop.
        let (a, b) = (o.from as f64 - 1.0, o.to as f64);
        host.fx.retain(|c| !(c.bar >= a && c.bar < b && takes_over_playback(&c.fx)));
        host.layers.push(Layer {
            kind,
            part: o.part,
            id: s.track.id.clone(),
            path: s.track.path.clone(),
            artist: s.track.artist.clone(),
            title: s.track.title.clone(),
            src_bar: o.loop_at.0,
            src_bars: o.loop_at.1,
            from_bar: o.from,
            to_bar: o.to,
            beat_ratio: o.ratio,
            key_shift: o.shift,
            reason,
        });
    }
    for ((src, host), why) in why_not {
        if !tracks.iter().any(|t| t.title == host && t.layers.iter().any(|l| l.title == src)) {
            notes.push(format!("Couldn't loop {src} over {host}: {why}."));
        }
    }
    let total: usize = tracks.iter().map(|t| t.layers.len()).sum();
    if total > 0 {
        let names: Vec<String> = motifs.iter().map(|m| srcs[*m].0.track.title.clone()).collect();
        notes.push(format!(
            "{total} layered loops on the extra decks{}.",
            if names.is_empty() { String::new() } else { format!("; recurring: {}", names.join(", ")) }
        ));
    }
    notes
}
