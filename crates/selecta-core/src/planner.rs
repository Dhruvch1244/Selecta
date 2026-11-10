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

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    /// Outgoing spins backwards to a stop; incoming drops on the next downbeat.
    Backspin,
    /// Outgoing slows to a halt like a powered-down turntable.
    TapeStop,
    /// "Pull up": the outgoing is wheeled back, then the incoming slams in.
    Rewind,
    /// Outgoing loop-rolls 1 -> 1/16 beat into the incoming's downbeat.
    LoopRoll,
    /// Stems: the incoming's vocal rides the outgoing's last phrase (its own
    /// vocal muted), then the incoming's beat drops in under it.
    AcapellaBuild,
    /// Stems: a live bootleg. The incoming's vocal over the outgoing's drop
    /// for a phrase or two, then the incoming's own drop takes over.
    Bootleg,
    /// Stems: double drop with one singer. Both drops land together, the
    /// outgoing keeps the bass, the incoming brings its vocal and top end.
    Mashup,
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
            "backspin" => Self::Backspin,
            "tape_stop" => Self::TapeStop,
            "rewind" => Self::Rewind,
            "loop_roll" => Self::LoopRoll,
            "acapella_build" => Self::AcapellaBuild,
            "bootleg" => Self::Bootleg,
            "mashup" => Self::Mashup,
            _ => return None,
        })
    }
    pub fn beatmatched(self) -> bool {
        matches!(self, Self::Blend | Self::BassSwap | Self::DoubleDrop | Self::Filter) || self.stems()
    }
    /// Needs vocal stems on both tracks.
    pub fn stems(self) -> bool {
        matches!(self, Self::AcapellaBuild | Self::Bootleg | Self::Mashup)
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
    /// Outgoing-track bar where the outgoing's vocal stem is muted, so two
    /// vocals never clash (stem tracks only).
    #[serde(default)]
    pub vox_out: Option<u32>,
    /// Stem moves: incoming-track bar of a sung hook that loops (phrase-locked
    /// to the outgoing) from `enter_at` until the swap, instead of playing the
    /// incoming's pre-drop bars straight through.
    #[serde(default)]
    pub loop_from: Option<u32>,
    /// Length of that hook loop in incoming bars (4 or 8).
    #[serde(default)]
    pub loop_bars: Option<u32>,
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
    /// Effects the director placed on this track, in track bars.
    pub fx: Vec<crate::fx::FxCue>,
    /// Semitones to shift this track's key (keylock on): -2..=2, 0 = as recorded.
    #[serde(default)]
    pub key_shift: i32,
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
    /// Vocal presence per bar (0..1) from the vocal stem, when separated.
    pub vocals: Option<Vec<f32>>,
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

/// Camelot code of a key shifted by `semi` semitones: +7 semitones is one
/// step clockwise on the wheel, so +1 semitone is 7 steps.
pub fn shift_camelot(c: &str, semi: i32) -> String {
    let (Some(n), Some(l)) = (c.get(..c.len().saturating_sub(1)).and_then(|s| s.parse::<i32>().ok()), c.chars().last()) else {
        return c.to_string();
    };
    format!("{}{l}", (n - 1 + 7 * semi).rem_euclid(12) + 1)
}

/// The smallest key shift (|s| <= 2, keylock only) that makes `b` mix
/// harmonically after `a`, with its score. 0 when none is needed or possible.
pub fn best_key_shift(a: &str, b: &str, keylock: bool) -> (i32, f64) {
    let base = camelot_score(a, b);
    if !keylock || base >= 0.85 {
        return (0, base);
    }
    for s in [1, -1, 2, -2] {
        let sc = camelot_score(a, &shift_camelot(b, s));
        if sc >= 0.85 {
            return (s, sc);
        }
    }
    (0, base)
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

/// Largest tempo change a beatmatched move may use. With keylock the
/// stretcher keeps the key, so a wider window still sounds natural.
fn match_window(keylock: bool) -> f64 {
    if keylock {
        0.10
    } else {
        0.06
    }
}

/// Identity of a *song*, not a file: the same song often exists twice (an
/// .opus rip and an .m4a, "(Official Video)" vs "[Lyric Video]", a YouTube
/// title "'RED CUP' | PENTHOUSE TAPES | SEEDHE MAUT" vs the tagged "RED CUP").
/// Returns (title core, artist names). Unicode-aware, so Devanagari titles
/// don't collapse to nothing.
pub fn song_key(artist: &str, title: &str) -> Option<(String, Vec<String>)> {
    let mut t = String::new();
    let mut depth = 0;
    for c in title.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth - 1).max(0),
            _ if depth == 0 => t.push(c),
            _ => {}
        }
    }
    let clean = |s: &str| -> String {
        s.chars()
            .flat_map(char::to_lowercase)
            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    // The song is the first segment: after " - ", "|" or "｜" come the uploader's extras.
    let first = t.split(" - ").next().unwrap_or(&t).split(['|', '｜']).next().unwrap_or(&t);
    let mut core = clean(first);
    for junk in ["official music video", "official video", "lyric video", "official audio", "lyrics", "audio", "video", "hd", "4k"] {
        if core.ends_with(junk) {
            core = core[..core.len() - junk.len()].trim().to_string();
        }
    }
    let artists: Vec<String> = artist.split([',', '&', '/']).map(clean).filter(|a| !a.is_empty()).collect();
    (core.chars().count() >= 2).then_some((core, artists))
}

/// One candidate per song: same title core and at least one shared artist
/// (two different songs called "Intro" stay apart). Keeps the copy with a
/// vocal stem, else the first.
fn dedupe(library: &[Candidate]) -> Vec<&Candidate> {
    let mut seen: HashMap<String, Vec<(Vec<String>, usize)>> = HashMap::new();
    let mut out: Vec<&Candidate> = vec![];
    for c in library {
        let Some((core, artists)) = song_key(&c.track.artist, &c.track.title) else {
            out.push(c);
            continue;
        };
        let group = seen.entry(core).or_default();
        match group.iter().find(|(a, _)| a.iter().any(|x| artists.contains(x))) {
            Some(&(_, i)) => {
                if c.vocals.is_some() && out[i].vocals.is_none() {
                    out[i] = c;
                }
            }
            None => {
                group.push((artists, out.len()));
                out.push(c);
            }
        }
    }
    out
}

// ---------- planner ----------

pub struct Options {
    pub seed: u64,
    pub exclude: Vec<String>,
    /// AI curation of `shortlist(...)`: weighed as data, never obeyed blindly.
    pub curation: Option<crate::llm::Curation>,
    /// The engine will keylock (tempo changes keep pitch), so tracks may also
    /// be key-shifted up to 2 semitones to mix harmonically.
    pub keylock: bool,
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

/// Per-track planning facts for a pool: energy (pool-relative, nudged by
/// the curator's role), primary genre, intended play time, curator rank.
fn make_works<'a>(intent: &Intent, pool: &[(&'a Candidate, f64)], cur: Option<&crate::llm::Curation>) -> Vec<Work<'a>> {
    let cands: Vec<&Candidate> = pool.iter().map(|p| p.0).collect();
    let energy = energy_index(&cands);
    let pace = match intent.energy {
        Energy::Crazy => 0.85,
        Energy::Party => 0.9,
        Energy::Chill => 1.25,
        Energy::Normal => 1.0,
    };
    pool.iter()
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
            // Rapid: ~40 bars a tune (45-55 s at DnB tempo), drop to drop.
            let play = if intent.rapid { 40.0 * 4.0 * c.analysis.beat } else { (lo + hi) / 2.0 * pace };
            let share = if intent.rapid { 0.6 } else { 0.85 };
            Work { c, fit: *f, energy: e, genre, est_play: (c.analysis.duration * share).min(play), rank }
        })
        .collect()
}

/// Director cues, energies and play times for scripted tracks. Returns the total seconds.
fn finish(tracks: &mut [SetTrack], seq: &[&Work], intent: &Intent, rng: &mut Rng) -> f64 {
    // FX direction last: it needs every track's final cue points.
    for i in 0..tracks.len() {
        let until = tracks.get(i + 1).map(|n| n.trans.from_bar).unwrap_or(tracks[i].out_bar);
        let genre = tracks[i].genre.clone();
        tracks[i].fx = crate::fx::direct(seq[i].a(), genre.as_deref(), tracks[i].in_bar, until, intent.chaos, rng);
        tracks[i].energy = (seq[i].energy * 1000.0).round() / 1000.0;
    }
    let mut total = 0.0;
    for i in 0..tracks.len() {
        let end_bar = tracks.get(i + 1).map(|n| n.trans.enter_at).unwrap_or(tracks[i].out_bar);
        let bar_sec = seq[i].bar_sec(tracks[i].rate);
        tracks[i].est_sec = ((end_bar.saturating_sub(tracks[i].in_bar)) as f64 * bar_sec * 10.0).round() / 10.0;
        total += tracks[i].est_sec;
    }
    total
}

/// Re-script a set from a track that is already locked in (playing or
/// loaded) through `rest`, in that order. Used when the AI's curated plan
/// arrives while the instant draft is already playing: what the audience
/// has heard stays, the rest follows the curator. The first returned track
/// is `from` itself, with its new exit point and cues.
pub fn rescript(
    intent: &Intent,
    library: &[Candidate],
    from: &SetTrack,
    rest: &[String],
    seed: u64,
    keylock: bool,
) -> Result<Vec<SetTrack>, String> {
    let mut rng = Rng::new(seed);
    let find = |id: &str| library.iter().find(|c| c.track.id == id);
    let first = find(&from.id).ok_or("the playing track is no longer in the library")?;
    let mut pool: Vec<(&Candidate, f64)> = vec![(first, 1.0)];
    pool.extend(rest.iter().filter(|id| **id != from.id).filter_map(|id| find(id)).map(|c| (c, fit(c, intent))));
    let work = make_works(intent, &pool, None);
    let seq: Vec<&Work> = work.iter().collect();
    let mut tracks = script(&seq, intent, Some(from), keylock, &mut rng);
    finish(&mut tracks, &seq, intent, &mut rng);
    Ok(tracks)
}

pub fn plan(intent: &Intent, library: &[Candidate], opts: &Options) -> Result<SetPlan, String> {
    let mut rng = Rng::new(opts.seed);
    let mut notes = vec![];

    // 1. Select.
    let unique = dedupe(library);
    if unique.len() < library.len() {
        notes.push(format!("{} duplicate files of the same songs were set aside.", library.len() - unique.len()));
    }
    let mut scored: Vec<(&Candidate, f64)> =
        unique.into_iter().filter(|c| !opts.exclude.contains(&c.track.id)).map(|c| (c, fit(c, intent))).collect();
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
    let work = make_works(intent, &pool, cur);
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
            // With keylock a clash can be fixed by a small key shift (a little worse than a natural match).
            let (ks, key) = best_key_shift(&pw.a().camelot, &w.a().camelot, opts.keylock);
            let key = key - 0.08 * ks.abs() as f64;
            s += 0.7 * key * if pw.genre.is_some_and(|g| g.beatmatch) { 1.0 } else { 0.6 };
            // Tempo flow matters in every genre: neighbours within the match
            // window can be mixed instead of cut. (Was 0.3 for cut genres,
            // which ordered a hip-hop set 122 -> 90 -> 133 -> 155 BPM.)
            s += if bm { 1.1 } else { 0.9 } * tempo_score(pw.a().bpm, w.a().bpm);
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
    let mut out = script(&seq, intent, None, opts.keylock, &mut rng);
    let total = finish(&mut out, &seq, intent, &mut rng);
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

/// Mean vocal presence of a track over bars [lo, hi) (0 without a stem).
fn vox(c: &Candidate, lo: u32, hi: u32) -> f64 {
    let Some(v) = &c.vocals else { return 0.0 };
    let (lo, hi) = (lo as usize, (hi as usize).min(v.len()));
    if hi <= lo {
        return 0.0;
    }
    v[lo..hi].iter().map(|x| *x as f64).sum::<f64>() / (hi - lo) as f64
}

/// The track's strongest sung hook: a phrase-aligned 8-bar window (else 4)
/// past the intro, where the vocal is present almost throughout. Loops of
/// it ride another track in sync, the way a DJ loops an acapella.
fn vocal_hook(c: &Candidate) -> Option<(u32, u32)> {
    let a = &c.analysis;
    c.vocals.as_ref()?;
    for len in [8u32, 4] {
        let best = (a.start_bar + 8..a.bars.saturating_sub(len + 4))
            .step_by(4)
            .map(|s| {
                // Every bar of the loop should be sung: the weakest bar counts most.
                let min = (s..s + len).map(|b| vox(c, b, b + 1)).fold(1.0, f64::min);
                (s, 0.6 * min + 0.4 * vox(c, s, s + len))
            })
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((s, score)) = best.filter(|b| b.1 >= 0.5) {
            let _ = score;
            return Some((s, len));
        }
    }
    None
}

/// Where a track can "land": its drops and rising sections, at least `lead` bars in.
fn landings(a: &Analysis, lead: u32) -> Vec<u32> {
    let mut v: Vec<u32> = a
        .drops
        .iter()
        .cloned()
        .chain(a.sections.iter().filter(|s| s.energy > 0.55).map(|s| s.start))
        .filter(|b| *b >= a.start_bar + lead && *b + 8 < a.bars)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Place a stem move: (incoming in_bar, outgoing out_bar, transition), or
/// None when these two tracks don't have the material for it.
fn place_stem_move(
    kind: TransitionKind,
    p: &Work,
    w: &Work,
    prev_in: u32,
    prev_rate: f64,
    rng: &mut Rng,
) -> Option<(u32, u32, Transition)> {
    let (pa, a) = (p.a(), w.a());
    // The outgoing drop nearest to where the outgoing was meant to end anyway.
    let near = |d: &u32| ((d - prev_in) as f64 * p.bar_sec(prev_rate) - p.est_play).abs();
    let t = |from_bar, len, enter_at, swap_at, vox_out| Transition {
        kind,
        from_bar,
        len,
        enter_at,
        swap_at: Some(swap_at),
        echo_div: Some(0.125),
        vox_out,
        loop_from: None,
        loop_bars: None,
    };
    // Acapella builds and bootlegs loop the incoming's hook when it has a
    // clear one (one vocal, in sync, repeating), rather than whatever happens
    // to be sung in the bars before its drop.
    let hooked = |mut tr: Transition| {
        if let Some((from, len)) = vocal_hook(w.c) {
            if tr.len >= len {
                tr.loop_from = Some(from);
                tr.loop_bars = Some(len);
            }
        }
        tr
    };
    match kind {
        TransitionKind::AcapellaBuild => {
            for lead in [16u32, 8] {
                // A sung phrase of the incoming that ends exactly on one of its landings.
                let Some(land) = landings(a, lead).into_iter().find(|l| vox(w.c, l - lead, *l) >= 0.35) else { continue };
                let ob = choose_out(p, prev_in, prev_rate, lead);
                if ob < prev_in + 8 + lead {
                    continue;
                }
                let from = ob - lead;
                let mute = (vox(p.c, from, ob) > 0.12).then_some(from);
                return Some((land - lead, ob, hooked(t(from, lead, from, ob, mute))));
            }
            None
        }
        TransitionKind::Bootleg => {
            let lead = if rng.next_f64() < 0.35 { 32 } else { 16 };
            let land = landings(a, lead).into_iter().find(|l| vox(w.c, l - lead, *l) >= 0.4)?;
            let od = pa
                .drops
                .iter()
                .cloned()
                .filter(|d| *d >= prev_in + 8 && d + lead + 1 < pa.bars)
                .min_by(|x, y| near(x).total_cmp(&near(y)))?;
            Some((land - lead, od + lead, hooked(t(od, lead, od, od + lead, Some(od)))))
        }
        TransitionKind::Mashup => {
            // Double drop where the incoming sings over the outgoing's beat.
            let idrop = a.drops.iter().cloned().find(|d| d.saturating_sub(a.start_bar) >= 8 && vox(w.c, *d, d + 16) >= 0.3)?;
            let od =
                pa.drops.iter().cloned().filter(|d| *d > prev_in + 16 && d + 17 < pa.bars).min_by(|x, y| near(x).total_cmp(&near(y)))?;
            let mut lead = 16.min(idrop - a.start_bar).min(od.saturating_sub(prev_in + 4));
            lead -= lead % 4;
            if lead < 8 {
                return None;
            }
            Some((idrop - lead, od + 16, Transition { echo_div: None, ..t(od - lead, lead + 16, od - lead, od, Some(od)) }))
        }
        _ => None,
    }
}

/// What the set has done so far, for variety.
struct History {
    used: HashMap<TransitionKind, u32>,
    prev: Option<TransitionKind>,
}

fn pick_transition(p: &Work, t: &Work, compat: bool, stem_ok: bool, hist: &History, it: &Intent, rng: &mut Rng) -> TransitionKind {
    let (used, prev) = (&hist.used, hist.prev);
    let chaos = it.chaos;
    let prof = p.genre.or(t.genre);
    let mut weights: Vec<(TransitionKind, f64)> = prof
        .map(|g| g.transitions.iter().filter_map(|(id, w)| TransitionKind::from_id(id).map(|k| (k, *w))).collect())
        .unwrap_or_else(|| {
            vec![(TransitionKind::EchoOut, 2.0), (TransitionKind::Filter, 2.0), (TransitionKind::Slam, 1.0), (TransitionKind::Blend, 1.0)]
        });
    if compat {
        // Cut genres' tables have no blends; when the tempos line up, a real
        // DJ blends anyway: give them the beatmatched moves.
        for (k, w) in [
            (TransitionKind::BassSwap, 3.0),
            (TransitionKind::Blend, 1.5),
            (TransitionKind::DoubleDrop, 1.5),
            (TransitionKind::Filter, 1.0),
        ] {
            if !weights.iter().any(|x| x.0 == k) {
                weights.push((k, w));
            }
        }
    }
    if compat && stem_ok && it.mashups > 0.0 {
        // Stem moves aren't in the genre tables: any genre can ride a vocal.
        let w = 2.5 * it.mashups;
        weights.push((TransitionKind::AcapellaBuild, w));
        weights.push((TransitionKind::Bootleg, 0.8 * w));
        weights.push((TransitionKind::Mashup, w));
    }
    if !compat {
        weights.retain(|(k, _)| !k.beatmatched());
        if weights.is_empty() {
            weights = vec![(TransitionKind::EchoOut, 2.0), (TransitionKind::Slam, 1.0)];
        }
    }
    let beat_genre = prof.is_some_and(|g| g.beatmatch);
    for (k, w) in weights.iter_mut() {
        if matches!(
            k,
            TransitionKind::Slam
                | TransitionKind::DoubleDrop
                | TransitionKind::Backspin
                | TransitionKind::Rewind
                | TransitionKind::LoopRoll
        ) {
            *w *= 0.4 + 1.4 * chaos;
        }
        if compat && !k.beatmatched() {
            // Mixable pair: blends are the bread, cuts the spice (a little
            // more spice in cut-first genres like hip hop).
            *w *= if beat_genre { 0.45 } else { 0.4 };
        }
        if it.smooth {
            *w *= if k.beatmatched() { 1.6 } else { 0.25 };
        }
        if it.turntablism
            && matches!(k, TransitionKind::Rewind | TransitionKind::Backspin | TransitionKind::TapeStop | TransitionKind::LoopRoll)
        {
            *w *= 3.0;
        }
        if it.rapid {
            *w *= match k {
                TransitionKind::DoubleDrop | TransitionKind::Mashup => 2.5,
                TransitionKind::Slam | TransitionKind::Rewind | TransitionKind::EchoOut | TransitionKind::Backspin => 1.3,
                TransitionKind::Blend | TransitionKind::Filter => 0.35,
                _ => 1.0,
            };
        }
        if Some(*k) == prev {
            *w *= 0.35;
        }
        // Gimmick exits are seasoning: each use halves the next one's odds
        // (a set with four rewinds stops being exciting by the second).
        if matches!(k, TransitionKind::Rewind | TransitionKind::Backspin | TransitionKind::TapeStop | TransitionKind::LoopRoll) {
            *w *= (if it.turntablism { 0.75f64 } else { 0.45 }).powi(used.get(k).copied().unwrap_or(0) as i32);
        }
    }
    if p.a().drops.is_empty() || t.a().drops.is_empty() {
        weights.retain(|(k, _)| !matches!(k, TransitionKind::DoubleDrop | TransitionKind::Mashup | TransitionKind::Bootleg));
    }
    *rng.pick(&weights)
}

fn script(seq: &[&Work], intent: &Intent, first: Option<&SetTrack>, keylock: bool, rng: &mut Rng) -> Vec<SetTrack> {
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
            trans: Transition {
                kind: TransitionKind::Start,
                from_bar: 0,
                len: 0,
                enter_at: 0,
                swap_at: None,
                echo_div: None,
                vox_out: None,
                loop_from: None,
                loop_bars: None,
            },
            est_sec: 0.0,
            fx: vec![],
            key_shift: 0,
        };
        if i == 0 {
            // A locked-in first track keeps its cue, rate and entry; only its exit is re-planned.
            if let Some(f) = first {
                st = SetTrack { out_bar: a.bars.saturating_sub(1), ..f.clone() };
            }
            tempo = a.bpm * st.rate;
            out.push(st);
            continue;
        }
        let p = seq[i - 1];
        let prev_rate = out[i - 1].rate;
        let prev_in = out[i - 1].in_bar;
        let r = fold_ratio(tempo, a.bpm);
        // Any genre beatmatches when the tempos allow it (a hip-hop or
        // Bollywood DJ blends close tempos too); the genre only decides how
        // much it prefers blends over cuts.
        let compat = (r - 1.0).abs() <= match_window(keylock) && a.grid_conf > 2.0 && p.a().grid_conf > 2.0;
        // Vocals over another track's harmony only work in compatible keys.
        let pa_key = p.a().camelot.clone();
        // The outgoing's *heard* key: its own, plus any shift it was given.
        let prev_key = shift_camelot(&pa_key, out[i - 1].key_shift);
        // Only shift keys we trust: a shift computed from a shaky key estimate is a guess.
        let confident = a.key_conf >= 0.6 && p.a().key_conf >= 0.6;
        let (ks, ksc) = if confident { best_key_shift(&prev_key, &a.camelot, keylock) } else { (0, camelot_score(&prev_key, &a.camelot)) };
        let stem_ok = p.c.vocals.is_some() && w.c.vocals.is_some() && ksc >= 0.85;
        let mut hist = History { used: HashMap::new(), prev: Some(out[i - 1].trans.kind) };
        for t in &out {
            *hist.used.entry(t.trans.kind).or_default() += 1;
        }
        let mut kind = pick_transition(p, w, compat, stem_ok, &hist, intent, rng);
        let pa = p.a();

        if kind.stems() {
            match place_stem_move(kind, p, w, prev_in, prev_rate, rng) {
                Some((in_bar, out_bar, t)) => {
                    st.in_bar = in_bar;
                    out[i - 1].out_bar = out_bar;
                    st.trans = t;
                }
                // Not the material for it (no sung phrase into a drop, no drop to ride): a plain bass swap.
                None => kind = TransitionKind::BassSwap,
            }
        }

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
                            vox_out: None,
                            loop_from: None,
                            loop_bars: None,
                        };
                    } else {
                        kind = TransitionKind::BassSwap;
                    }
                }
                None => kind = TransitionKind::BassSwap,
            }
        }
        if kind.beatmatched() && kind != TransitionKind::DoubleDrop && !kind.stems() {
            let mut ov = match kind {
                TransitionKind::Blend if matches!(w.genre.map(|g| g.id), Some("techno" | "house" | "trance")) => 32,
                TransitionKind::Filter => 8,
                _ => 16,
            };
            if ov == 16 && rng.next_f64() < 0.3 {
                ov = if kind == TransitionKind::Blend { 32 } else { 8 };
            }
            if intent.rapid {
                ov = ov.min(16);
            } else if intent.smooth && kind == TransitionKind::Blend {
                ov = 32; // long, seamless
            }
            // Rapid mixing skips intros: come in on the build into the first drop.
            st.in_bar = if intent.rapid {
                a.drops.iter().cloned().find(|d| *d >= a.start_bar + 16).map(|d| d - 16).unwrap_or(a.start_bar)
            } else {
                a.start_bar
            };
            let ob = choose_out(p, prev_in, prev_rate, ov);
            out[i - 1].out_bar = ob;
            let from = (prev_in + 8).max(ob.saturating_sub(ov));
            st.trans = Transition {
                kind,
                from_bar: from,
                len: ob - from,
                enter_at: from,
                swap_at: Some(from + (ob - from) / 2),
                echo_div: None,
                vox_out: None,
                loop_from: None,
                loop_bars: None,
            };
        } else if !kind.beatmatched() {
            st.in_bar = choose_in(w, true, intent, rng);
            let ob = choose_out(p, prev_in, prev_rate, 0);
            out[i - 1].out_bar = ob;
            // Bars the outgoing's exit move takes before the incoming lands.
            let pre = match kind {
                TransitionKind::EchoOut | TransitionKind::Backspin | TransitionKind::TapeStop | TransitionKind::Rewind => 1,
                TransitionKind::LoopRoll => 2,
                _ => 0,
            };
            st.trans = Transition {
                kind,
                from_bar: ob - pre,
                len: pre,
                enter_at: ob,
                swap_at: None,
                echo_div: (kind == TransitionKind::EchoOut).then(|| [0.25, 0.5, 0.1875][(rng.next_f64() * 3.0) as usize % 3]),
                vox_out: None,
                loop_from: None,
                loop_bars: None,
            };
        }
        // Vocal clash guard: one singer at a time. The outgoing's vocal goes
        // on the 4-bar phrase where the incoming first sings, if the outgoing
        // is still singing then (it used to wait for the swap, and only when
        // both sang heavily, so two vocals could overlap for 8+ bars).
        if matches!(kind, TransitionKind::Blend | TransitionKind::BassSwap | TransitionKind::DoubleDrop | TransitionKind::Filter) {
            let t = &st.trans;
            let first_sung = (0..t.len).find(|k| vox(w.c, st.in_bar + k, st.in_bar + k + 1) >= 0.3);
            if let Some(k) = first_sung {
                let at = t.enter_at + k - k % 4;
                if vox(p.c, at, t.from_bar + t.len) > 0.12 {
                    st.trans.vox_out = Some(at);
                }
            }
        }
        st.rate = if kind.beatmatched() { (r * 1e5).round() / 1e5 } else { 1.0 };
        // Keylock: shift the key when that's what makes this pair harmonic.
        // Cuts don't need it (no overlap), so only overlapping moves shift.
        // +-1 for blends; +-2 only where a vocal must sit in key over another track.
        st.key_shift = if kind.beatmatched() && (ks.abs() <= 1 || kind.stems()) { ks } else { 0 };
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
                        error: None,
                        stems: false,
                    },
                    genres: genres::classify(tag, artist, "", Some(a.bpm)),
                    analysis: a,
                    tags: None,
                    vocals: None,
                }
            })
            .collect()
    }

    #[test]
    fn dnb_set_is_on_genre_beatmatched_and_phrase_aligned() {
        let lib = library();
        let it = intent::parse("crazy dnb set 10 min", &[]);
        let p = plan(&it, &lib, &Options { seed: 7, exclude: vec![], curation: None, keylock: false }).unwrap();
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
        let a = plan(&it, &lib, &Options { seed: 1, exclude: vec![], curation: None, keylock: false }).unwrap();
        let b = plan(&it, &lib, &Options { seed: 1, exclude: vec![], curation: None, keylock: false }).unwrap();
        assert_eq!(a.tracks, b.tracks);
    }

    #[test]
    fn artist_request_leads_and_cuts_across_tempos() {
        let lib = library();
        let it = intent::parse("Seedhe Maut hip hop set 6 min", &[]);
        let p = plan(&it, &lib, &Options { seed: 3, exclude: vec![], curation: None, keylock: false }).unwrap();
        assert!(p.tracks.iter().any(|t| t.artist == "Seedhe Maut"));
        for t in p.tracks.iter().skip(1) {
            if (t.bpm - 93.0).abs() > 10.0 && !t.trans.kind.beatmatched() {
                assert_eq!(t.rate, 1.0, "cuts don't pitch tracks");
            }
        }
    }

    /// The DnB part of the test library, every track "sung" after its intro and in one key.
    fn sung_dnb() -> Vec<Candidate> {
        library()
            .into_iter()
            .filter(|c| c.genres.get("dnb").copied().unwrap_or(0.0) > 0.5)
            .map(|mut c| {
                c.analysis.camelot = "8A".into();
                c.vocals = Some((0..c.analysis.bars).map(|b| if b < 8 { 0.0 } else { 0.8 }).collect());
                c
            })
            .collect()
    }

    #[test]
    fn stem_moves_are_placed_on_phrases_with_one_singer() {
        let lib = sung_dnb();
        let it = intent::parse("dnb mashups and bootlegs 20 min", &[]);
        let mut seen = std::collections::BTreeSet::new();
        for seed in 1..40 {
            let p = plan(&it, &lib, &Options { seed, exclude: vec![], curation: None, keylock: false }).unwrap();
            for w in p.tracks.windows(2) {
                let (prev, t) = (&w[0], &w[1].trans);
                if !t.kind.stems() {
                    continue;
                }
                seen.insert(format!("{:?}", t.kind));
                let swap = t.swap_at.expect("stem moves have a swap");
                assert!(t.vox_out.is_some() || t.kind == TransitionKind::AcapellaBuild, "{:?}: only one vocal at a time", t.kind);
                assert!(t.enter_at <= swap && swap <= t.from_bar + t.len, "{t:?}");
                assert!(
                    prev.out_bar >= t.from_bar + t.len.min(swap - t.from_bar),
                    "outgoing must still be playing: {t:?} out {}",
                    prev.out_bar
                );
                assert_eq!(t.enter_at % 4, prev.in_bar % 4, "phrase-aligned with the outgoing grid");
                assert!((prev.bpm * prev.rate - w[1].bpm * w[1].rate).abs() < 0.05, "stem moves are beatmatched");
            }
        }
        assert!(seen.len() >= 2, "expected several kinds of stem move over 40 seeds, saw {seen:?}");
    }

    #[test]
    fn no_stems_no_stem_moves() {
        let lib: Vec<Candidate> = sung_dnb().into_iter().map(|c| Candidate { vocals: None, ..c }).collect();
        let it = intent::parse("dnb mashups and bootlegs 20 min", &[]);
        for seed in 1..20 {
            let p = plan(&it, &lib, &Options { seed, exclude: vec![], curation: None, keylock: false }).unwrap();
            assert!(p.tracks.iter().all(|t| !t.trans.kind.stems() && t.trans.vox_out.is_none()));
        }
    }

    #[test]
    fn rapid_plays_each_tune_for_less_time() {
        let lib = library();
        let per = |prompt: &str| {
            let p = plan(&intent::parse(prompt, &[]), &lib, &Options { seed: 5, exclude: vec![], curation: None, keylock: false }).unwrap();
            p.est_total_sec / p.tracks.len() as f64
        };
        let (normal, rapid) = (per("dnb set 10 min"), per("hedex style dnb 10 min"));
        assert!(rapid < 0.75 * normal, "rapid {rapid:.0}s vs normal {normal:.0}s per tune");
    }

    #[test]
    fn rescript_keeps_the_locked_track_and_follows_the_new_order() {
        let lib = library();
        let it = intent::parse("dnb set 15 min", &[]);
        let draft = plan(&it, &lib, &Options { seed: 11, exclude: vec![], curation: None, keylock: false }).unwrap();
        assert!(draft.tracks.len() >= 3);
        let locked = &draft.tracks[1];
        // The "curator" wants the remaining DnB tracks in reverse library order.
        let rest: Vec<String> = lib
            .iter()
            .rev()
            .filter(|c| c.genres.get("dnb").copied().unwrap_or(0.0) > 0.5 && c.track.id != locked.id && c.track.id != draft.tracks[0].id)
            .map(|c| c.track.id.clone())
            .collect();
        let tail = rescript(&it, &lib, locked, &rest, 3, false).unwrap();
        assert_eq!(tail[0].id, locked.id);
        assert_eq!(
            (tail[0].in_bar, tail[0].rate, &tail[0].trans),
            (locked.in_bar, locked.rate, &locked.trans),
            "what's loaded stays as loaded"
        );
        let ids: Vec<&String> = tail[1..].iter().map(|t| &t.id).collect();
        assert_eq!(ids, rest.iter().collect::<Vec<_>>());
        for w in tail.windows(2) {
            assert!(w[1].trans.enter_at <= w[0].out_bar);
            assert!(w[1].trans.from_bar >= w[0].in_bar);
        }
    }

    #[test]
    fn key_shifts_move_around_the_camelot_wheel() {
        assert_eq!(shift_camelot("8A", 0), "8A");
        assert_eq!(shift_camelot("8A", 7), "9A", "a fifth up is one step clockwise");
        assert_eq!(shift_camelot("8A", 1), "3A", "a semitone up is 7 steps");
        assert_eq!(shift_camelot("12B", 1), "7B");
        assert_eq!(shift_camelot("1A", -1), "6A");
        assert_eq!(shift_camelot("?", 1), "?");
        // 8A -> 3A clashes; shifting 3A down a semitone gives 8A.
        assert_eq!(best_key_shift("8A", "3A", true), (-1, 1.0));
        assert_eq!(best_key_shift("8A", "3A", false).0, 0, "no keylock, no shifting");
        assert_eq!(best_key_shift("8A", "9A", true).0, 0, "already harmonic");
    }

    #[test]
    fn keylock_sets_mix_harmonically_through_key_shifts() {
        // Every DnB track in a different, mostly clashing key.
        let keys = ["8A", "3A", "11B", "5A", "1B"];
        let lib: Vec<Candidate> = library()
            .into_iter()
            .filter(|c| c.genres.get("dnb").copied().unwrap_or(0.0) > 0.5)
            .enumerate()
            .map(|(i, mut c)| {
                c.analysis.camelot = keys[i % keys.len()].into();
                c
            })
            .collect();
        let it = intent::parse("dnb set 15 min", &[]);
        let mut shifted = 0;
        for seed in 1..12 {
            let p = plan(&it, &lib, &Options { seed, exclude: vec![], curation: None, keylock: true }).unwrap();
            for w in p.tracks.windows(2) {
                let (a, b) = (&w[0], &w[1]);
                assert!(b.key_shift.abs() <= 2);
                if b.trans.kind.beatmatched() {
                    let heard = (shift_camelot(&a.camelot, a.key_shift), shift_camelot(&b.camelot, b.key_shift));
                    if b.key_shift != 0 {
                        shifted += 1;
                        assert!(camelot_score(&heard.0, &heard.1) >= 0.85, "{heard:?}");
                    }
                } else {
                    assert_eq!(b.key_shift, 0, "cuts don't shift keys");
                }
            }
            let off = plan(&it, &lib, &Options { seed, exclude: vec![], curation: None, keylock: false }).unwrap();
            assert!(off.tracks.iter().all(|t| t.key_shift == 0));
        }
        assert!(shifted > 0, "clashing keys should have been shifted at least once");
    }

    #[test]
    fn one_song_two_files_is_one_song() {
        let k = |a: &str, t: &str| song_key(a, t).map(|(core, _)| core);
        // Real duplicates from a library: a YouTube rip and the tagged file.
        assert_eq!(
            k("DL91 Era", "'RED CUP' ｜ PENTHOUSE TAPES VOL. 1 ｜ SEEDHE MAUT (Prod. CALM) - LYRIC VIDEO"),
            k("Calm, DL91 Era, Seedhe Maut", "RED CUP")
        );
        assert_eq!(k("Karan Aujla", "Mexico (Official Video)"), k("Karan Aujla", "Mexico"));
        assert_eq!(k("Seedhe Maut", "नalla Freestyle").unwrap(), "नalla freestyle", "Devanagari survives");
        assert_eq!(k("X", "(Intro)"), None, "nothing left: no identity, never merged");
        // Same title, no shared artist: two different songs.
        let mut lib = library();
        lib.truncate(3);
        lib[1].track.title = lib[0].track.title.clone();
        assert_eq!(dedupe(&lib).len(), 3);
        lib[1].track.artist = format!("{}, Someone", lib[0].track.artist);
        assert_eq!(dedupe(&lib).len(), 2, "shared artist + same title = duplicate");
    }

    #[test]
    fn empty_library_is_a_clear_error() {
        let it = intent::parse("dnb", &[]);
        assert!(plan(&it, &[], &Options { seed: 1, exclude: vec![], curation: None, keylock: false })
            .unwrap_err()
            .contains("No analysed tracks"));
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
        let p = plan(&it, &lib, &Options { seed: 5, exclude: vec![], curation: Some(cur), keylock: false }).unwrap();
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
