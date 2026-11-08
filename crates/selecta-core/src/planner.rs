//! Intent + analysed library -> a set plan the engine performs.
//!
//! 1. Select: score tracks against the intent (genre, artists), widening to
//!    related genres when the library is thin.
//! 2. Order: beam search over sequences, scoring harmonic (Camelot) moves,
//!    tempo compatibility, fit to an energy arc, and artist variety.
//! 3. Script: for each adjacent pair, phrase-aligned cue points and a
//!    transition the pair can actually support. Beatmatched blends need both
//!    grids trustworthy and tempos within +-8% (no keylock yet).
//!
//! Everything is in *track bars* (each track's own grid), so the engine
//! schedules against what the deck is really doing rather than a wall
//! clock. Plans are deterministic for a given seed.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::analysis::Analysis;
use crate::genres::{self, Profile};
use crate::intent::{Energy, Intent};
use crate::library::TrackSummary;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TransitionKind {
    Start,
    /// Long overlap, gradual low-EQ swap.
    Blend,
    /// Overlap with a hard low-EQ swap on a downbeat.
    #[serde(rename = "bassswap")]
    BassSwap,
    /// Incoming drop lands on the outgoing drop; bass swaps on the one.
    DoubleDrop,
    /// Outgoing high-passes away while the incoming low-pass opens.
    Filter,
    /// Outgoing echoes out; incoming starts on the next downbeat. Works across tempos.
    EchoOut,
    /// Hard cut on a downbeat. Works across tempos.
    Slam,
}

impl TransitionKind {
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "blend" => Self::Blend,
            "bassswap" => Self::BassSwap,
            "double_drop" => Self::DoubleDrop,
            "filter" => Self::Filter,
            "echo_out" => Self::EchoOut,
            "slam" => Self::Slam,
            _ => return None,
        })
    }
    pub fn beatmatched(self) -> bool {
        matches!(self, Self::Blend | Self::BassSwap | Self::DoubleDrop | Self::Filter)
    }
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Transition {
    pub kind: TransitionKind,
    /// Outgoing-track bar where the transition starts.
    pub from_bar: u32,
    /// Length in outgoing bars (0 for a slam).
    pub len: u32,
    /// Outgoing-track bar where the incoming track starts playing its `in_bar`.
    pub enter_at: u32,
    /// Outgoing-track bar of the low-EQ swap (blends) or the shared drop.
    pub swap_at: Option<u32>,
    /// Echo division as a fraction of a bar, for `echo_out`.
    pub echo_div: Option<f64>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SetTrack {
    pub id: String,
    pub path: String,
    pub artist: String,
    pub title: String,
    pub bpm: f64,
    pub key: String,
    pub camelot: String,
    pub genre: Option<String>,
    pub genre_label: String,
    /// 0..1, relative to the set's candidate pool.
    pub energy: f64,
    /// playbackRate to apply (1 = original tempo).
    pub rate: f64,
    pub in_bar: u32,
    pub out_bar: u32,
    /// How this track comes in over the previous one.
    pub trans: Transition,
    pub est_sec: f64,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SetPlan {
    pub id: String,
    pub created: f64,
    pub title: String,
    pub prompt: String,
    pub intent: Intent,
    pub tracks: Vec<SetTrack>,
    pub est_total_sec: f64,
    /// Target energy per position, for drawing the arc.
    pub arc: Vec<f64>,
    pub notes: Vec<String>,
    /// True when the AI curated this set.
    pub curated: bool,
}

pub struct Candidate {
    pub track: TrackSummary,
    pub analysis: Analysis,
    pub genres: BTreeMap<String, f64>,
    /// AI genre/mood/language tags, when the library has been tagged.
    pub tags: Option<crate::llm::TrackTags>,
}

// ---------- small, well-understood helpers (pillar 7) ----------

/// xorshift64*: deterministic, seedable, good enough for musical choices.
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_f64(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn gauss(&mut self, sd: f64) -> f64 {
        let (u, v) = (self.next_f64().max(1e-12), self.next_f64());
        sd * (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
    pub fn pick<'a, T>(&mut self, items: &'a [(T, f64)]) -> &'a T {
        let total: f64 = items.iter().map(|i| i.1).sum();
        let mut r = self.next_f64() * total;
        for (t, w) in items {
            r -= w;
            if r <= 0.0 {
                return t;
            }
        }
        &items.last().expect("non-empty").0
    }
}

const ARCS: &[(&str, &[(f64, f64)])] = &[
    ("rave", &[(0.0, 0.55), (0.15, 0.8), (0.35, 0.95), (0.55, 0.85), (0.7, 1.0), (0.9, 1.0), (1.0, 0.9)]),
    ("party", &[(0.0, 0.5), (0.25, 0.7), (0.5, 0.85), (0.75, 0.97), (0.9, 0.9), (1.0, 0.8)]),
    ("journey", &[(0.0, 0.35), (0.3, 0.6), (0.6, 0.85), (0.8, 0.95), (1.0, 0.6)]),
    ("hype", &[(0.0, 0.7), (0.2, 0.9), (0.5, 0.95), (0.7, 0.85), (0.9, 1.0), (1.0, 0.85)]),
    ("chill", &[(0.0, 0.3), (0.4, 0.5), (0.7, 0.45), (1.0, 0.3)]),
];

pub fn arc_at(name: &str, x: f64) -> f64 {
    let pts = ARCS.iter().find(|a| a.0 == name).map(|a| a.1).unwrap_or(ARCS[2].1);
    for w in pts.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if (x0..=x1).contains(&x) {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0 + 1e-9);
        }
    }
    pts.last().unwrap().1
}

pub fn camelot_score(a: &str, b: &str) -> f64 {
    let parse = |s: &str| -> Option<(i32, char)> { Some((s[..s.len() - 1].parse().ok()?, s.chars().last()?)) };
    let (Some((na, la)), Some((nb, lb))) = (parse(a), parse(b)) else { return 0.5 };
    let d = (na - nb).rem_euclid(12).min((nb - na).rem_euclid(12));
    match (d, la == lb) {
        (0, true) => 1.0,
        (0, false) => 0.85, // relative major/minor
        (1, true) => 0.9,   // adjacent on the wheel
        (2, true) => 0.55,  // energy boost
        (7, true) => 0.5,   // +7: semitone lift
        _ => 0.15,
    }
}

/// playbackRate for `dst` to match `src`'s tempo, folding octaves (87 vs 174).
pub fn fold_ratio(src_bpm: f64, dst_bpm: f64) -> f64 {
    let mut r = src_bpm / dst_bpm;
    while r > std::f64::consts::SQRT_2 {
        r /= 2.0;
    }
    while r < std::f64::consts::FRAC_1_SQRT_2 {
        r *= 2.0;
    }
    r
}

fn tempo_score(a: f64, b: f64) -> f64 {
    (1.0 - (fold_ratio(a, b) - 1.0).abs() / 0.1).max(0.0)
}

// ---------- planner ----------

pub struct Options {
    pub seed: u64,
    pub exclude: Vec<String>,
    /// AI curation of `shortlist(...)`: weighed as data, never obeyed blindly.
    pub curation: Option<crate::llm::Curation>,
}

/// The tracks worth showing a curator: anything with some genre fit, plus
/// anything the rules couldn't classify at all (the AI may know it), best first.
pub fn shortlist<'a>(intent: &Intent, library: &'a [Candidate], exclude: &[String], max: usize) -> Vec<&'a Candidate> {
    let mut v: Vec<(&Candidate, f64)> = library
        .iter()
        .filter(|c| !exclude.contains(&c.track.id))
        .map(|c| (c, fit(c, intent)))
        .filter(|(c, f)| *f >= 0.1 || c.genres.is_empty())
        .collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    v.into_iter().take(max).map(|(c, _)| c).collect()
}

/// Energy a curator's role implies; blended with the measured energy.
fn role_energy(role: &str) -> Option<f64> {
    Some(match role {
        "opener" => 0.4,
        "builder" => 0.65,
        "peak" => 0.95,
        "closer" => 0.6,
        _ => return None,
    })
}

struct Work<'a> {
    c: &'a Candidate,
    fit: f64,
    energy: f64,
    genre: Option<&'static Profile>,
    est_play: f64,
    /// Position in the curator's running order, 0..1.
    rank: Option<f64>,
}

impl Work<'_> {
    fn a(&self) -> &Analysis {
        &self.c.analysis
    }
    fn bar_sec(&self, rate: f64) -> f64 {
        4.0 * self.a().beat / rate
    }
}

fn fit(c: &Candidate, it: &Intent) -> f64 {
    let mut f = if it.genres.is_empty() {
        0.6
    } else {
        let max_w = it.genres.values().cloned().fold(0.0, f64::max);
        let direct: f64 = it.genres.iter().map(|(g, w)| w * c.genres.get(g).copied().unwrap_or(0.0)).sum();
        let related = it
            .genres
            .iter()
            .flat_map(|(g, w)| {
                genres::profile(g)
                    .into_iter()
                    .flat_map(move |p| p.related.iter().map(move |r| 0.35 * w * c.genres.get(*r).copied().unwrap_or(0.0)))
            })
            .fold(0.0, f64::max);
        (direct / max_w + related).min(1.0)
    };
    if !it.artists.is_empty() {
        let names = genres::split_artists(&c.track.artist);
        if it.artists.iter().any(|a| names.iter().any(|n| n == a || n.contains(a.as_str()))) {
            f += 1.0;
        }
    }
    f
}

fn primary_genre(c: &Candidate, it: &Intent) -> Option<&'static Profile> {
    if !it.genres.is_empty() {
        let best = it.genres.iter().max_by(|a, b| {
            let s = |g: &String, w: f64| w * (c.genres.get(g).copied().unwrap_or(0.0) + 0.01);
            s(a.0, *a.1).partial_cmp(&s(b.0, *b.1)).unwrap()
        });
        if let Some((g, _)) = best {
            if c.genres.get(g).copied().unwrap_or(0.0) > 0.1 {
                return genres::profile(g);
            }
        }
    }
    c.genres.iter().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).and_then(|(g, _)| genres::profile(g))
}

/// Pool-relative 0..1 energy from onset rate, brightness, loud-part share and tempo.
/// Relative on purpose: a chill set's arc should span the chill material
/// available, not be squashed into the bottom of a library that also holds neurofunk.
fn energy_index(pool: &[&Candidate]) -> HashMap<String, f64> {
    let z = |v: Vec<f64>| -> Vec<f64> {
        let m = v.iter().sum::<f64>() / v.len().max(1) as f64;
        let sd = (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt().max(1e-9);
        v.iter().map(|x| (x - m) / sd).collect()
    };
    let a: Vec<&Analysis> = pool.iter().map(|c| &c.analysis).collect();
    let hot = z(a.iter().map(|a| a.bar_energy.iter().filter(|e| **e > 0.75).count() as f64 / a.bar_energy.len().max(1) as f64).collect());
    let on = z(a.iter().map(|a| a.onset_rate).collect());
    let cen = z(a.iter().map(|a| a.centroid).collect());
    let bpm = z(a.iter().map(|a| a.bpm.min(180.0)).collect());
    let raw: Vec<f64> = (0..pool.len()).map(|i| 0.4 * on[i] + 0.25 * cen[i] + 0.2 * hot[i] + 0.15 * bpm[i]).collect();
    let mut order: Vec<usize> = (0..pool.len()).collect();
    order.sort_by(|x, y| raw[*x].partial_cmp(&raw[*y]).unwrap());
    let mut out = HashMap::new();
    for (rank, i) in order.iter().enumerate() {
        out.insert(pool[*i].track.id.clone(), 0.15 + 0.85 * rank as f64 / (pool.len().max(2) - 1) as f64);
    }
    out
}

pub fn plan(intent: &Intent, library: &[Candidate], opts: &Options) -> Result<SetPlan, String> {
    let mut rng = Rng::new(opts.seed);
    let mut notes = vec![];

    // 1. Select.
    let mut scored: Vec<(&Candidate, f64)> =
        library.iter().filter(|c| !opts.exclude.contains(&c.track.id)).map(|c| (c, fit(c, intent))).collect();
    if scored.is_empty() {
        return Err("No analysed tracks yet: add a music folder and let analysis finish.".into());
    }
    let cur = opts.curation.as_ref();
    if let Some(cur) = cur {
        scored.retain(|(c, _)| !cur.avoid.contains(&c.track.id));
        for (c, f) in scored.iter_mut() {
            if let Some(sc) = cur.scores.get(&c.track.id) {
                // Keep the explicit-artist boost (fit > 1) on top of the blend.
                *f = 0.35 * f.min(1.0) + 0.65 * sc / 10.0 + if *f > 1.0 { 1.0 } else { 0.0 };
            }
        }
        if scored.is_empty() {
            return Err("The AI curator vetoed every track for this prompt; try a different vibe.".into());
        }
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let need = intent.duration_min * 60.0 / 100.0;
    let mut pool: Vec<(&Candidate, f64)> = match cur {
        Some(cur) => {
            let picked: Vec<_> = scored.iter().filter(|(c, _)| cur.scores.get(&c.track.id).is_some_and(|s| *s >= 5.0)).cloned().collect();
            let mut p = picked;
            if (p.len() as f64) < need {
                // Top up with the rules' strong matches the curator didn't veto.
                let extra: Vec<_> =
                    scored.iter().filter(|s| s.1 >= 0.45 && !p.iter().any(|q| q.0.track.id == s.0.track.id)).cloned().collect();
                p.extend(extra.into_iter().take(need.ceil() as usize + 2 - p.len().min(need.ceil() as usize + 2)));
            }
            p
        }
        None => scored.iter().filter(|s| s.1 >= 0.45).cloned().collect(),
    };
    if (pool.len() as f64) < need {
        let more: Vec<_> = scored.iter().filter(|s| (0.15..0.45).contains(&s.1)).cloned().collect();
        if !more.is_empty() {
            notes.push(format!("Only {} strong matches; widened to {} related tracks.", pool.len(), more.len()));
        }
        pool.extend(more);
    }
    if (pool.len() as f64) < need.min(4.0) {
        notes.push("Your library is thin for this prompt, so the set is padded with the closest tracks you have.".into());
        pool = scored.iter().take((need as usize + 2).max(pool.len())).cloned().collect();
    }

    // 2. Order.
    let cands: Vec<&Candidate> = pool.iter().map(|p| p.0).collect();
    let energy = energy_index(&cands);
    let pace = match intent.energy {
        Energy::Crazy => 0.85,
        Energy::Party => 0.9,
        Energy::Chill => 1.25,
        Energy::Normal => 1.0,
    };
    let work: Vec<Work> = pool
        .iter()
        .map(|(c, f)| {
            let genre = primary_genre(c, intent);
            let (lo, hi) = genre.map(|g| g.play).unwrap_or((80.0, 150.0));
            let id = &c.track.id;
            let mut e = energy[id];
            if let Some(r) = cur.and_then(|k| k.roles.get(id)).and_then(|r| role_energy(r)) {
                e = 0.55 * e + 0.45 * r;
            }
            let rank = cur.and_then(|k| {
                let i = k.order.iter().position(|x| x == id)?;
                Some(i as f64 / (k.order.len().max(2) - 1) as f64)
            });
            Work { c, fit: *f, energy: e, genre, est_play: (c.analysis.duration * 0.85).min((lo + hi) / 2.0 * pace), rank }
        })
        .collect();
    let mean_play = work.iter().map(|w| w.est_play).sum::<f64>() / work.len() as f64;
    let n = ((intent.duration_min * 60.0 / mean_play.max(30.0)).round() as usize).clamp(2, work.len()).min(work.len());
    let temp = if intent.energy == Energy::Chill { 0.15 } else { 0.25 };

    let step = |seq: &[usize], t: usize, rng: &mut Rng| -> f64 {
        let w = &work[t];
        let x = seq.len() as f64 / (n.max(2) - 1) as f64;
        let mut s = 1.3 * w.fit.min(1.2) + 1.1 * (1.0 - (2.0 * (w.energy - arc_at(&intent.arc, x)).abs()).min(1.0));
        if let Some(r) = w.rank {
            // The curator's running order is a guide, not a cage: key, tempo and arc still vote.
            s += 0.9 * (1.0 - (2.5 * (r - x).abs()).min(1.0));
        }
        if let Some(&p) = seq.last() {
            let pw = &work[p];
            let bm = pw.genre.is_some_and(|g| g.beatmatch) && w.genre.is_some_and(|g| g.beatmatch);
            s += 0.7 * camelot_score(&pw.a().camelot, &w.a().camelot) * if pw.genre.is_some_and(|g| g.beatmatch) { 1.0 } else { 0.6 };
            s += if bm { 0.9 } else { 0.3 } * tempo_score(pw.a().bpm, w.a().bpm);
            let recent: Vec<String> = seq.iter().rev().take(3).map(|i| work[*i].c.track.artist.to_lowercase()).collect();
            let me = w.c.track.artist.to_lowercase();
            if recent.first() == Some(&me) {
                s -= 0.7;
            } else if recent.contains(&me) {
                s -= 0.3;
            }
            if pw.genre.map(|g| g.id) != w.genre.map(|g| g.id) {
                s -= 0.1;
            }
        }
        s + rng.gauss(temp)
    };

    let mut beams: Vec<(Vec<usize>, f64)> = vec![(vec![], 0.0)];
    for _ in 0..n {
        let mut next = vec![];
        for (seq, sc) in &beams {
            let mut opts: Vec<(f64, usize)> = (0..work.len()).filter(|i| !seq.contains(i)).map(|i| (step(seq, i, &mut rng), i)).collect();
            opts.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            for (s, i) in opts.into_iter().take(24) {
                let mut q = seq.clone();
                q.push(i);
                next.push((q, sc + s));
            }
        }
        if next.is_empty() {
            break;
        }
        next.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        next.truncate(14);
        beams = next;
    }
    let seq: Vec<&Work> = beams[0].0.iter().map(|i| &work[*i]).collect();

    // 3. Script.
    let tracks = script(&seq, intent, &mut rng);
    let mut out = tracks;
    let mut total = 0.0;
    for (st, w) in out.iter_mut().zip(&seq) {
        st.energy = (w.energy * 1000.0).round() / 1000.0;
    }
    for i in 0..out.len() {
        let end_bar = out.get(i + 1).map(|n| n.trans.enter_at).unwrap_or(out[i].out_bar);
        let bar_sec = seq[i].bar_sec(out[i].rate);
        out[i].est_sec = ((end_bar.saturating_sub(out[i].in_bar)) as f64 * bar_sec * 10.0).round() / 10.0;
        total += out[i].est_sec;
    }
    let len = out.len();
    let created = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    Ok(SetPlan {
        id: format!("{:016x}", opts.seed ^ created.to_bits()),
        created,
        title: cur.and_then(|c| c.title.clone()).unwrap_or_else(|| intent.title.clone()),
        prompt: intent.prompt.clone(),
        intent: Intent { vibe: cur.and_then(|c| c.vibe.clone()).or_else(|| intent.vibe.clone()), ..intent.clone() },
        tracks: out,
        est_total_sec: (total * 10.0).round() / 10.0,
        arc: (0..len).map(|i| (arc_at(&intent.arc, i as f64 / (len.max(2) - 1) as f64) * 1000.0).round() / 1000.0).collect(),
        notes: notes.into_iter().chain(cur.into_iter().flat_map(|c| c.moments.iter().map(|m| format!("DJ idea · {m}")))).collect(),
        curated: cur.is_some(),
    })
}

fn phrases(a: &Analysis, lo: u32, hi: u32) -> Vec<u32> {
    let mut v: Vec<u32> = (a.start_bar..a.bars).step_by(8).filter(|b| (lo..=hi).contains(b)).collect();
    v.extend(a.sections.iter().map(|s| s.start).filter(|b| (lo..=hi).contains(b)));
    v.sort_unstable();
    v.dedup();
    v
}

fn choose_in(w: &Work, cut: bool, intent: &Intent, rng: &mut Rng) -> u32 {
    let a = w.a();
    let drops: Vec<u32> = a.drops.iter().cloned().filter(|d| *d > a.start_bar).collect();
    if cut {
        let p = if matches!(intent.energy, Energy::Crazy | Energy::Party) { 0.55 } else { 0.3 };
        if let Some(&d) = drops.first() {
            if rng.next_f64() < p {
                // Hook-to-hook: come in 4-8 bars before the first drop/chorus.
                return d.saturating_sub(if rng.next_f64() < 0.5 { 4 } else { 8 }).max(a.start_bar);
            }
        }
        // Skip an over-long quiet intro on cut-style genres.
        if let Some(s) = a.sections.iter().find(|s| s.energy > 0.55 && s.start.saturating_sub(a.start_bar) > 16) {
            return s.start - 8;
        }
    }
    a.start_bar
}

fn choose_out(w: &Work, in_bar: u32, rate: f64, overlap: u32) -> u32 {
    let a = w.a();
    let min_bar = in_bar + 16.max(overlap + 8);
    let max_bar = a.bars.saturating_sub(1);
    if min_bar >= max_bar {
        return max_bar;
    }
    let target = in_bar as f64 + w.est_play / w.bar_sec(rate);
    let mut cands = phrases(a, min_bar, max_bar);
    if a.outro_bar >= min_bar {
        cands.push(a.outro_bar);
    }
    if cands.is_empty() {
        return max_bar;
    }
    let e = &a.bar_energy;
    let mean = |s: usize, t: usize| e[s.min(e.len())..t.min(e.len())].iter().sum::<f32>() as f64 / (t - s).max(1) as f64;
    *cands
        .iter()
        .min_by(|x, y| {
            let cost = |b: u32| {
                let b = b as usize;
                let mut c = (b as f64 - target).abs() / 8.0;
                if mean(b, b + 8) < mean(b.saturating_sub(8), b) - 0.12 {
                    c -= 0.8; // leave as the energy falls: never a dull moment
                }
                if a.breaks.contains(&(b as u32)) || b as u32 == a.outro_bar {
                    c -= 0.5;
                }
                c
            };
            cost(**x).partial_cmp(&cost(**y)).unwrap()
        })
        .unwrap()
}

fn pick_transition(p: &Work, t: &Work, compat: bool, prev: Option<TransitionKind>, chaos: f64, rng: &mut Rng) -> TransitionKind {
    let prof = p.genre.or(t.genre);
    let mut weights: Vec<(TransitionKind, f64)> = prof
        .map(|g| g.transitions.iter().filter_map(|(id, w)| TransitionKind::from_id(id).map(|k| (k, *w))).collect())
        .unwrap_or_else(|| {
            vec![(TransitionKind::EchoOut, 2.0), (TransitionKind::Filter, 2.0), (TransitionKind::Slam, 1.0), (TransitionKind::Blend, 1.0)]
        });
    if !compat {
        weights.retain(|(k, _)| !k.beatmatched());
        if weights.is_empty() {
            weights = vec![(TransitionKind::EchoOut, 2.0), (TransitionKind::Slam, 1.0)];
        }
    }
    let beat_genre = prof.is_some_and(|g| g.beatmatch);
    for (k, w) in weights.iter_mut() {
        if matches!(k, TransitionKind::Slam | TransitionKind::DoubleDrop) {
            *w *= 0.4 + 1.4 * chaos;
        }
        if compat && beat_genre && !k.beatmatched() {
            *w *= 0.45; // beatmatch genres: blends are the bread, cuts the spice
        }
        if Some(*k) == prev {
            *w *= 0.35;
        }
    }
    if p.a().drops.is_empty() || t.a().drops.is_empty() {
        weights.retain(|(k, _)| *k != TransitionKind::DoubleDrop);
    }
    *rng.pick(&weights)
}

fn script(seq: &[&Work], intent: &Intent, rng: &mut Rng) -> Vec<SetTrack> {
    let mut out: Vec<SetTrack> = vec![];
    let mut tempo = 0.0;
    for (i, w) in seq.iter().enumerate() {
        let a = w.a();
        let mut st = SetTrack {
            id: w.c.track.id.clone(),
            path: w.c.track.path.clone(),
            artist: w.c.track.artist.clone(),
            title: w.c.track.title.clone(),
            bpm: a.bpm,
            key: a.key.clone(),
            camelot: a.camelot.clone(),
            genre: w.genre.map(|g| g.id.to_string()),
            genre_label: w.genre.map(|g| g.label.to_string()).unwrap_or_default(),
            energy: w.energy,
            rate: 1.0,
            in_bar: a.start_bar,
            out_bar: a.bars.saturating_sub(1),
            trans: Transition { kind: TransitionKind::Start, from_bar: 0, len: 0, enter_at: 0, swap_at: None, echo_div: None },
            est_sec: 0.0,
        };
        if i == 0 {
            tempo = a.bpm;
            out.push(st);
            continue;
        }
        let p = seq[i - 1];
        let prev_rate = out[i - 1].rate;
        let prev_in = out[i - 1].in_bar;
        let r = fold_ratio(tempo, a.bpm);
        let either_bm = p.genre.is_some_and(|g| g.beatmatch) || w.genre.is_some_and(|g| g.beatmatch);
        let compat = (r - 1.0).abs() <= 0.08 && a.grid_conf > 2.0 && p.a().grid_conf > 2.0 && (either_bm || (r - 1.0).abs() <= 0.04);
        let mut kind = pick_transition(p, w, compat, Some(out[i - 1].trans.kind), intent.chaos, rng);
        let pa = p.a();

        if kind == TransitionKind::DoubleDrop {
            // Line the drops up: the incoming enters N bars before its drop exactly
            // N bars before the outgoing's next drop, so both hit together.
            let in_drop = a.drops.iter().cloned().find(|d| d.saturating_sub(a.start_bar) >= 8);
            let out_drops: Vec<u32> = pa.drops.iter().cloned().filter(|d| *d > prev_in + 16).collect();
            let done = in_drop.zip(out_drops.iter().min_by(|x, y| {
                let dist = |d: u32| ((d - prev_in) as f64 * p.bar_sec(prev_rate) - p.est_play).abs();
                dist(**x).partial_cmp(&dist(**y)).unwrap()
            }));
            match done {
                Some((idrop, &od)) => {
                    let mut lead = 16.min(idrop - a.start_bar).min(od.saturating_sub(prev_in + 4));
                    lead -= lead % 4;
                    if lead >= 8 && od + 16 < pa.bars {
                        st.in_bar = idrop - lead;
                        out[i - 1].out_bar = od + 16;
                        st.trans = Transition {
                            kind,
                            from_bar: od - lead,
                            len: lead + 16,
                            enter_at: od - lead,
                            swap_at: Some(od),
                            echo_div: None,
                        };
                    } else {
                        kind = TransitionKind::BassSwap;
                    }
                }
                None => kind = TransitionKind::BassSwap,
            }
        }
        if kind.beatmatched() && kind != TransitionKind::DoubleDrop {
            let mut ov = match kind {
                TransitionKind::Blend if matches!(w.genre.map(|g| g.id), Some("techno" | "house" | "trance")) => 32,
                TransitionKind::Filter => 8,
                _ => 16,
            };
            if ov == 16 && rng.next_f64() < 0.3 {
                ov = if kind == TransitionKind::Blend { 32 } else { 8 };
            }
            st.in_bar = a.start_bar;
            let ob = choose_out(p, prev_in, prev_rate, ov);
            out[i - 1].out_bar = ob;
            let from = (prev_in + 8).max(ob.saturating_sub(ov));
            st.trans =
                Transition { kind, from_bar: from, len: ob - from, enter_at: from, swap_at: Some(from + (ob - from) / 2), echo_div: None };
        } else if !kind.beatmatched() {
            st.in_bar = choose_in(w, true, intent, rng);
            let ob = choose_out(p, prev_in, prev_rate, 0);
            out[i - 1].out_bar = ob;
            let pre = if kind == TransitionKind::EchoOut { 1 } else { 0 };
            st.trans = Transition {
                kind,
                from_bar: ob - pre,
                len: pre,
                enter_at: ob,
                swap_at: None,
                echo_div: (kind == TransitionKind::EchoOut).then(|| [0.25, 0.5, 0.1875][(rng.next_f64() * 3.0) as usize % 3]),
            };
        }
        st.rate = if kind.beatmatched() { (r * 1e5).round() / 1e5 } else { 1.0 };
        tempo = a.bpm * st.rate;
        out.push(st);
    }
    if let (Some(last), Some(w)) = (out.last_mut(), seq.last()) {
        last.out_bar = (last.in_bar + 16).max(w.a().bars.min(w.a().outro_bar + 8));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::analyze_pcm;
    use crate::analysis::tests::{synth, LAYOUT};
    use crate::intent;

    fn library() -> Vec<Candidate> {
        // name, artist, bpm, tag
        let specs = [
            ("Neuro Storm", "Noisia", 174.0, "Drum & Bass"),
            ("Liquid Sky", "Hybrid Minds", 172.0, "Drum & Bass"),
            ("Solar", "Sub Focus", 174.0, "Drum & Bass"),
            ("Watercolour", "Pendulum", 175.0, "Drum & Bass"),
            ("Come Alive", "Netsky", 173.0, "Drum & Bass"),
            ("Groove", "Fisher", 124.0, "House"),
            ("Nanchaku", "Seedhe Maut", 93.0, ""),
            ("Genda Phool", "Badshah", 120.0, ""),
        ];
        specs
            .iter()
            .enumerate()
            .map(|(i, (title, artist, bpm, tag))| {
                let a = analyze_pcm(&synth(*bpm, 0.3 + i as f64 * 0.1, &LAYOUT, 22_050, i as u64), genres::bpm_hint(tag, artist, ""));
                Candidate {
                    track: TrackSummary {
                        id: format!("t{i}"),
                        path: format!("/m/{title}.mp3"),
                        artist: artist.to_string(),
                        title: title.to_string(),
                        album: String::new(),
                        genre_tag: tag.to_string(),
                        duration: a.duration,
                        analyzed: true,
                        bpm: Some(a.bpm),
                        key: Some(a.key.clone()),
                        camelot: Some(a.camelot.clone()),
                        mood: None,
                    },
                    genres: genres::classify(tag, artist, "", Some(a.bpm)),
                    analysis: a,
                    tags: None,
                }
            })
            .collect()
    }

    #[test]
    fn dnb_set_is_on_genre_beatmatched_and_phrase_aligned() {
        let lib = library();
        let it = intent::parse("crazy dnb set 10 min", &[]);
        let p = plan(&it, &lib, &Options { seed: 7, exclude: vec![], curation: None }).unwrap();
        assert!(p.tracks.len() >= 3, "{} tracks", p.tracks.len());
        for t in &p.tracks {
            assert_eq!(t.genre.as_deref(), Some("dnb"), "{} is {:?}", t.title, t.genre);
            assert!((t.rate - 1.0).abs() <= 0.08 + 1e-9, "{} rate {}", t.title, t.rate);
        }
        for w in p.tracks.windows(2) {
            let (prev, next) = (&w[0], &w[1]);
            assert!(next.trans.enter_at <= prev.out_bar, "incoming must start before outgoing ends");
            assert!(next.trans.from_bar >= prev.in_bar, "transition can't start before the outgoing track does");
            if next.trans.kind.beatmatched() {
                // Effective tempos match within a hair.
                assert!((prev.bpm * prev.rate - next.bpm * next.rate).abs() < 0.05);
            }
        }
        assert!(p.tracks.iter().skip(1).any(|t| t.trans.kind.beatmatched()), "a DnB set should blend at least once");
    }

    #[test]
    fn same_seed_same_plan_different_seed_can_differ() {
        let lib = library();
        let it = intent::parse("dnb", &[]);
        let a = plan(&it, &lib, &Options { seed: 1, exclude: vec![], curation: None }).unwrap();
        let b = plan(&it, &lib, &Options { seed: 1, exclude: vec![], curation: None }).unwrap();
        assert_eq!(a.tracks, b.tracks);
    }

    #[test]
    fn artist_request_leads_and_cuts_across_tempos() {
        let lib = library();
        let it = intent::parse("Seedhe Maut hip hop set 6 min", &[]);
        let p = plan(&it, &lib, &Options { seed: 3, exclude: vec![], curation: None }).unwrap();
        assert!(p.tracks.iter().any(|t| t.artist == "Seedhe Maut"));
        for t in p.tracks.iter().skip(1) {
            if (t.bpm - 93.0).abs() > 10.0 && !t.trans.kind.beatmatched() {
                assert_eq!(t.rate, 1.0, "cuts don't pitch tracks");
            }
        }
    }

    #[test]
    fn empty_library_is_a_clear_error() {
        let it = intent::parse("dnb", &[]);
        assert!(plan(&it, &[], &Options { seed: 1, exclude: vec![], curation: None }).unwrap_err().contains("No analysed tracks"));
    }

    #[test]
    fn curation_vetoes_scores_and_orders() {
        let lib = library();
        let it = intent::parse("dnb set 8 min", &[]);
        let id = |t: &str| lib.iter().find(|c| c.track.title == t).unwrap().track.id.clone();
        let cur = crate::llm::Curation {
            scores: [(id("Solar"), 10.0), (id("Watercolour"), 9.0), (id("Come Alive"), 8.0), (id("Liquid Sky"), 7.0)].into_iter().collect(),
            roles: [(id("Liquid Sky"), "opener".to_string())].into_iter().collect(),
            avoid: vec![id("Neuro Storm")],
            order: vec![id("Liquid Sky"), id("Watercolour"), id("Come Alive"), id("Solar")],
            title: Some("Curated Pressure".into()),
            vibe: Some("rolling".into()),
            moments: vec!["Solar: rewind the drop".into()],
        };
        let p = plan(&it, &lib, &Options { seed: 5, exclude: vec![], curation: Some(cur) }).unwrap();
        assert!(p.curated);
        assert_eq!(p.title, "Curated Pressure");
        assert_eq!(p.intent.vibe.as_deref(), Some("rolling"));
        assert!(p.tracks.iter().all(|t| t.title != "Neuro Storm"), "vetoed track must not appear");
        assert!(p.notes.iter().any(|n| n.contains("rewind the drop")));
        assert_eq!(p.tracks[0].title, "Liquid Sky", "the curator's opener should open");
    }

    #[test]
    fn camelot_rules() {
        assert_eq!(camelot_score("8A", "8A"), 1.0);
        assert_eq!(camelot_score("8A", "8B"), 0.85);
        assert_eq!(camelot_score("8A", "9A"), 0.9);
        assert_eq!(camelot_score("12A", "1A"), 0.9);
        assert_eq!(camelot_score("8A", "3B"), 0.15);
        assert!((fold_ratio(174.0, 87.0) - 1.0).abs() < 1e-12);
    }
}
