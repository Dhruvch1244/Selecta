//! Whole-track analysis: tempo, a constant beat grid, downbeats, phrases,
//! drops, key, loudness and a 3-band waveform overview.
//!
//! The target is *DJ-usable*, not MIR-paper accuracy:
//! - One constant grid (bpm + phase), because beatmatching and loop lengths
//!   need a single period. `grid_conf` says how well it fits, so the planner
//!   can refuse to beatmatch a live-drummer track and cut instead.
//! - Bars and phrases counted from the first bar with music in it.
//! - Per-bar energy, which is what finds drops, breakdowns and outros.
//!
//! Ported from the Python prototype, where each heuristic was measured on
//! synthetic tracks with known tempo/offset/structure (numbers in comments).
//! A neural beat tracker replaces the tempo/grid stage in a later PR; the rest
//! keeps working on its output unchanged.

use std::sync::Arc;

use rayon::prelude::*;
use rustfft::{num_complex::Complex32, FftPlanner};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::decode::Pcm;

pub const NFFT: usize = 2048;
pub const HOP: usize = 512;
const BINS: usize = NFFT / 2 + 1;
/// Spectral flux fires when an attack *enters* the 93 ms window, i.e. before
/// the frame centre. Measured on synthetic tracks with known offsets: a
/// consistent -36..-42 ms, corrected so cue points land on the transient.
const ONSET_LAG: f64 = 0.039;
/// Bump when analysis output changes; stored tracks with an older version are re-analysed.
/// 2: peak-aware gain (`peak_db`), soft genre tempo prior.
pub const ANALYSIS_VERSION: u32 = 5;

/// Loudness every track's loud parts are brought to (LUFS, short-term 3 s,
/// 90th percentile). Measured on the real library: lossy files overshoot
/// 0 dBFS, so the old peak-capped RMS gain left drops 11 dB apart.
pub const TARGET_LUFS: f64 = -10.0;
/// How far a boost may push a track's sample peak past 0 dBFS; the master's
/// lookahead limiter takes the rest cleanly.
const BOOST_PEAK_CEIL_DB: f64 = 3.0;

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Section {
    pub start: u32,
    pub end: u32,
    /// Mean energy 0..1, relative to the track's own loud parts.
    pub energy: f32,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq)]
#[ts(export)]
pub struct WaveOverview {
    pub low: Vec<u8>,
    pub mid: Vec<u8>,
    pub high: Vec<u8>,
    pub peak: Vec<u8>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Analysis {
    pub version: u32,
    pub duration: f64,
    pub bpm: f64,
    /// Seconds per beat at the original tempo.
    pub beat: f64,
    pub first_beat: f64,
    /// Time of bar 0's downbeat (earliest one at or after 0 s).
    pub downbeat0: f64,
    /// Mean onset strength on the grid / mean overall. ~1 = no grid, >2 = trustworthy.
    pub grid_conf: f64,
    pub key: String,
    pub camelot: String,
    pub key_conf: f64,
    pub bars: u32,
    pub start_bar: u32,
    pub sections: Vec<Section>,
    pub drops: Vec<u32>,
    pub breaks: Vec<u32>,
    pub outro_bar: u32,
    pub bar_energy: Vec<f32>,
    /// Playback gain bringing the loud parts to `TARGET_LUFS`.
    pub gain_db: f64,
    /// Short-term (3 s) BS.1770 loudness of the track's loud parts (90th percentile), LUFS.
    #[serde(default)]
    pub loudness: f64,
    /// Sample peak of the decoded audio, dBFS.
    pub peak_db: f64,
    pub centroid: f64,
    pub onset_rate: f64,
    pub wave: WaveOverview,
}

struct Spectrum {
    /// Row-major frames x BINS magnitudes.
    mag: Vec<f32>,
    frames: usize,
    rate: f64,
}

impl Spectrum {
    fn row(&self, f: usize) -> &[f32] {
        &self.mag[f * BINS..(f + 1) * BINS]
    }
    fn fps(&self) -> f64 {
        self.rate / HOP as f64
    }
    fn bin_of(&self, hz: f64) -> usize {
        ((hz * NFFT as f64 / self.rate) as usize).min(BINS - 1)
    }
}

/// Gain that brings the loud parts to `TARGET_LUFS`, boosting no further than
/// a peak of `BOOST_PEAK_CEIL_DB`.
pub fn norm_gain(loudness: f64, peak_db: f64) -> f64 {
    let want = TARGET_LUFS - loudness;
    // Cutting is always fine; only a boost is limited by the peak.
    if want <= 0.0 {
        want.max(-15.0)
    } else {
        want.min((BOOST_PEAK_CEIL_DB - peak_db).max(0.0)).min(15.0)
    }
}

/// Sub-bass level (< ~150 Hz, dB) over each beat starting at `beats_t`.
fn sub_db_per_beat(sp: &Spectrum, beats_t: &[f64], beat: f64, low_bins: usize) -> Vec<f64> {
    let fps = sp.fps();
    beats_t
        .iter()
        .map(|t| {
            let a = ((t * fps) as usize).min(sp.frames.saturating_sub(1));
            let b = (((t + beat) * fps) as usize).clamp(a + 1, sp.frames.max(a + 1));
            let e: f64 =
                (a..b.min(sp.frames)).map(|f| sp.row(f)[1..low_bins.max(2)].iter().map(|v| (*v as f64).powi(2)).sum::<f64>()).sum();
            10.0 * (e / (b - a).max(1) as f64 + 1e-12).log10()
        })
        .collect()
}

/// Power mean of dB values, in dB.
fn pmean_db(v: &[f64]) -> f64 {
    10.0 * (v.iter().map(|d| 10f64.powf(d / 10.0)).sum::<f64>() / v.len().max(1) as f64 + 1e-12).log10()
}

/// How hard the bass returns at beat `i`: the next 2 bars' sub level over
/// the median of the 2 bars before, when the sub also jumps >= 10 dB on
/// that very beat (a pickup or a rhythmic bassline doesn't count).
fn slam_at(sub: &[f64], full: f64, i: i64) -> f64 {
    if i < 8 || i as usize + 8 > sub.len() {
        return 0.0;
    }
    let i = i as usize;
    // ...and it reaches the track's full bass level: a riser or snare roll
    // coming out of a silent break jumps too, but stays well below it.
    if sub[i] - sub[i - 1] < 10.0 || pmean_db(&sub[i..i + 8]) < full - 6.0 {
        return 0.0;
    }
    let mut pre = sub[i - 8..i].to_vec();
    pre.sort_by(|a, b| a.total_cmp(b));
    pmean_db(&sub[i..i + 8]) - 0.5 * (pre[3] + pre[4])
}

/// Drops placed where the sub-bass actually slams back in, to the beat.
/// Each energy drop moves to the hardest slam within 8 bars before to 4
/// after it (full-band energy put DnB drops 4-8 bars late: the build is as
/// loud as the drop); clear slams the energy curve missed are added. Also
/// returns, per beat of the bar (0..4), how strongly the slams land there:
/// they should land on the one, so a consistent other beat means the bar
/// line is off by that much.
fn refine_drops(drops: &[u32], sub: &[f64], first_beat: i64, energy: &[f32], start_bar: usize) -> (Vec<u32>, [f64; 4]) {
    let nb = energy.len();
    let mut votes = [0f64; 4];
    let full = {
        let mut v = sub.to_vec();
        v.sort_by(|a, b| a.total_cmp(b));
        v.get(v.len() * 4 / 5).copied().unwrap_or(0.0)
    };
    let loud = |b: usize| {
        let e = &energy[b.min(nb)..(b + 8).min(nb)];
        !e.is_empty() && e.iter().sum::<f32>() / e.len() as f32 >= 0.55
    };
    // A slam at beat `i` -> (bar, beat within bar); a slam a beat before the
    // line counts as that bar's (an early bar line), as does one up to 2 after.
    let place = |i: i64| -> (i64, usize) {
        let k = (i - first_beat).rem_euclid(4) as usize;
        let bar = (i - first_beat).div_euclid(4) + if k == 3 { 1 } else { 0 };
        (bar, k)
    };
    let mut out: Vec<u32> = vec![];
    for &d in drops {
        let lo = first_beat + 4 * (d as i64 - 8).max(start_bar as i64);
        let hi = first_beat + 4 * (d as i64 + 4);
        let best = (lo..=hi).map(|i| (i, slam_at(sub, full, i))).filter(|(_, s)| *s >= 12.0).max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((i, sc)) => {
                let (bar, k) = place(i);
                votes[k] += sc;
                out.push(bar.max(0) as u32);
            }
            None => out.push(d),
        }
    }
    for i in (first_beat + 4 * start_bar.max(2) as i64)..(first_beat + 4 * nb.saturating_sub(4) as i64) {
        let sc = slam_at(sub, full, i);
        let (bar, k) = place(i);
        if sc >= 15.0 && bar >= 0 && loud(bar as usize) && out.iter().all(|d| (*d as i64 - bar).abs() >= 12) {
            votes[k] += sc;
            out.push(bar as u32);
        }
    }
    out.sort_unstable();
    let mut spaced: Vec<u32> = vec![];
    for d in out {
        if spaced.last().is_none_or(|l| d >= l + 16) {
            spaced.push(d);
        }
    }
    (spaced, votes)
}

/// RBJ biquad, direct form I, in place (the two ITU-R BS.1770 K-weighting stages).
fn biquad(x: &mut [f64], b: [f64; 3], a: [f64; 3]) {
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    for v in x.iter_mut() {
        let y = (b[0] * *v + b[1] * x1 + b[2] * x2 - a[1] * y1 - a[2] * y2) / a[0];
        x2 = x1;
        x1 = *v;
        y2 = y1;
        y1 = y;
        *v = y;
    }
}

/// BS.1770 short-term loudness (3 s windows, 1 s hop) of a mono downmix,
/// expressed as the equivalent stereo LUFS; the 90th percentile of the
/// non-silent windows, so quiet intros and breakdowns don't count.
pub fn loud_parts_lufs(x: &[f32], rate: f64) -> f64 {
    let mut y: Vec<f64> = x.iter().map(|v| *v as f64).collect();
    let (g, q, fc) = (4.0f64, std::f64::consts::FRAC_1_SQRT_2, 1500.0);
    let a = 10f64.powf(g / 40.0);
    let w = 2.0 * std::f64::consts::PI * fc / rate;
    let (c, al) = (w.cos(), w.sin() / (2.0 * q));
    let sa = 2.0 * a.sqrt() * al;
    biquad(
        &mut y,
        [a * ((a + 1.0) + (a - 1.0) * c + sa), -2.0 * a * ((a - 1.0) + (a + 1.0) * c), a * ((a + 1.0) + (a - 1.0) * c - sa)],
        [(a + 1.0) - (a - 1.0) * c + sa, 2.0 * ((a - 1.0) - (a + 1.0) * c), (a + 1.0) - (a - 1.0) * c - sa],
    );
    let w = 2.0 * std::f64::consts::PI * 38.0 / rate;
    let (c, al) = (w.cos(), w.sin() / (2.0 * 0.5));
    biquad(&mut y, [(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + al, -2.0 * c, 1.0 - al]);
    let hop = rate as usize;
    let blocks: Vec<f64> = y.chunks(hop.max(1)).map(|b| b.iter().map(|v| v * v).sum::<f64>() / b.len() as f64).collect();
    // Mono summed to two identical channels reads +3.01 dB in BS.1770.
    let mut st: Vec<f64> =
        blocks.windows(3).map(|w| -0.691 + 10.0 * ((w[0] + w[1] + w[2]) / 3.0 + 1e-12).log10() + 3.0103).filter(|l| *l > -60.0).collect();
    if st.is_empty() {
        return -60.0;
    }
    st.sort_by(|a, b| a.total_cmp(b));
    st[((st.len() - 1) as f64 * 0.9).round() as usize]
}

fn stft(x: &[f32], rate: u32) -> Spectrum {
    let pad = NFFT / 2;
    let mut padded = vec![0f32; pad];
    padded.extend_from_slice(x);
    padded.extend(std::iter::repeat_n(0f32, pad));
    let frames = 1 + (padded.len() - NFFT) / HOP;
    let win: Vec<f32> = (0..NFFT).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / NFFT as f32).cos()).collect();
    let fft: Arc<dyn rustfft::Fft<f32>> = FftPlanner::new().plan_fft_forward(NFFT);
    let mut mag = vec![0f32; frames * BINS];
    mag.par_chunks_mut(BINS * 256).enumerate().for_each(|(chunk, out)| {
        let mut buf = vec![Complex32::new(0.0, 0.0); NFFT];
        let mut scratch = vec![Complex32::new(0.0, 0.0); fft.get_inplace_scratch_len()];
        for (k, row) in out.chunks_mut(BINS).enumerate() {
            let f = chunk * 256 + k;
            let s = &padded[f * HOP..f * HOP + NFFT];
            for i in 0..NFFT {
                buf[i] = Complex32::new(s[i] * win[i], 0.0);
            }
            fft.process_with_scratch(&mut buf, &mut scratch);
            for (r, c) in row.iter_mut().zip(&buf[..BINS]) {
                *r = c.norm();
            }
        }
    });
    Spectrum { mag, frames, rate: rate as f64 }
}

fn log1p10(v: f32) -> f32 {
    (1.0 + 10.0 * v).ln()
}

fn flux(sp: &Spectrum, lo: usize, hi: usize) -> Vec<f64> {
    std::iter::once(0.0)
        .chain((1..sp.frames).map(|f| {
            let (a, b) = (sp.row(f - 1), sp.row(f));
            (lo..hi).map(|k| (log1p10(b[k]) - log1p10(a[k])).max(0.0) as f64).sum()
        }))
        .collect()
}

fn detrend(x: &[f64], win: usize) -> Vec<f64> {
    // max(x - centred moving average, 0); prefix sums keep it O(n).
    let n = x.len();
    let mut pre = vec![0f64; n + 1];
    for i in 0..n {
        pre[i + 1] = pre[i] + x[i];
    }
    let h = win / 2;
    (0..n)
        .map(|i| {
            let a = i.saturating_sub(h);
            let b = (i + win - h).min(n);
            (x[i] - (pre[b] - pre[a]) / win as f64).max(0.0)
        })
        .collect()
}

fn interp(xs: &[f64], pos: f64) -> f64 {
    if pos <= 0.0 {
        return xs[0];
    }
    let i = pos.floor() as usize;
    if i + 1 >= xs.len() {
        return *xs.last().unwrap_or(&0.0);
    }
    let t = pos - i as f64;
    xs[i] * (1.0 - t) + xs[i + 1] * t
}

fn autocorr(x: &[f64]) -> Vec<f64> {
    let n = x.len();
    let m = (2 * n).next_power_of_two();
    let mean = x.iter().sum::<f64>() / n as f64;
    let mut buf: Vec<rustfft::num_complex::Complex64> =
        (0..m).map(|i| rustfft::num_complex::Complex64::new(if i < n { x[i] - mean } else { 0.0 }, 0.0)).collect();
    let mut planner = FftPlanner::<f64>::new();
    planner.plan_fft_forward(m).process(&mut buf);
    buf.iter_mut().for_each(|c| *c = rustfft::num_complex::Complex64::new(c.norm_sqr(), 0.0));
    planner.plan_fft_inverse(m).process(&mut buf);
    let z = buf[0].re.max(1e-12);
    buf[..n].iter().map(|c| c.re / z).collect()
}

/// Tempo by comb-weighted autocorrelation over 70-190 BPM, weighted by a
/// log-Gaussian prior (`center` BPM, `width` in octaves).
fn estimate_tempo(onset: &[f64], fps: f64, center: f64, width: f64) -> f64 {
    let ac = autocorr(onset);
    let mut best = (f64::MIN, center);
    let mut bpm = 70.0;
    while bpm < 190.0 {
        let lag = 60.0 * fps / bpm;
        // Comb over multiples: a true period also scores at 2x, 3x, 4x its lag.
        let mut s: f64 = (1..=4).map(|m| interp(&ac, lag * m as f64) / m as f64).sum();
        s *= (-0.5 * ((bpm / center).log2() / width).powi(2)).exp();
        if s > best.0 {
            best = (s, bpm);
        }
        bpm += 0.05;
    }
    best.1
}

fn grid_search(onset: &[f64], lo: f64, hi: f64, step: f64, phase_step: f64, fps: f64) -> (f64, f64, f64) {
    let n = onset.len() as f64;
    let steps = ((hi - lo) / step).ceil() as usize;
    (0..steps)
        .into_par_iter()
        .map(|i| {
            let b = lo + i as f64 * step;
            let period = 60.0 * fps / b;
            let mut best = (f64::MIN, b, 0.0);
            let mut ph = 0.0;
            while ph < period {
                let (mut sum, mut cnt, mut t) = (0.0, 0usize, ph);
                while t < n - period {
                    sum += interp(onset, t);
                    cnt += 1;
                    t += period;
                }
                let s = sum / cnt.max(1) as f64;
                if s > best.0 {
                    best = (s, b, ph);
                }
                ph += phase_step;
            }
            best
        })
        .reduce(|| (f64::MIN, lo, 0.0), |a, b| if b.0 > a.0 { b } else { a })
}

/// Below this the low band has no clear kick pattern to pin a grid to.
pub const KICK_SHARPNESS: f64 = 1.6;

/// Offset (s) that best aligns a grid (`beat`, first beat `phase`) with the
/// track's kicks, searched over +-`range`, and how clear that optimum is
/// (best / mean score; ~1 means no kick structure).
///
/// A kick is a low thump *and* a high click at the same instant; a bass note
/// is only the first, a hi-hat only the second. Scoring the product of the
/// low-band (< 150 Hz) and high-band (> 2 kHz) energy rises keeps the grid
/// off a tech-house bassline that hits a 16th after the kick (measured: a
/// low-band-only detector locked 49-60 ms off on such tracks). 64-sample
/// hops (2.9 ms at 22 kHz) are fine enough to beatmatch by. Near-ties go to
/// the smaller correction.
pub fn kick_phase_offset(samples: &[f32], rate: f64, beat: f64, phase: f64, range: f64) -> (f64, f64) {
    let kl = 1.0 - (-2.0 * std::f64::consts::PI * 150.0 / rate).exp();
    let kh = 1.0 - (-2.0 * std::f64::consts::PI * 2000.0 / rate).exp();
    const HOP: usize = 64;
    let (mut lp, mut lp2) = (0.0, 0.0);
    let (mut lo, mut hi) = (Vec::with_capacity(samples.len() / HOP + 1), Vec::with_capacity(samples.len() / HOP + 1));
    let (mut al, mut ah) = (0.0, 0.0);
    for (i, s) in samples.iter().enumerate() {
        let x = *s as f64;
        lp += kl * (x - lp);
        lp2 += kh * (x - lp2);
        al += lp * lp;
        ah += (x - lp2) * (x - lp2);
        if (i + 1) % HOP == 0 {
            lo.push(al.sqrt());
            hi.push(ah.sqrt());
            al = 0.0;
            ah = 0.0;
        }
    }
    let rise = |e: &[f64]| -> Vec<f64> {
        let r: Vec<f64> = e.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect();
        let m = r.iter().sum::<f64>() / r.len().max(1) as f64;
        r.into_iter().map(|v| v / (m + 1e-12)).collect()
    };
    let (rl, rh) = (rise(&lo), rise(&hi));
    let kick: Vec<f64> = rl.iter().zip(&rh).map(|(a, b)| a * (0.25 + b)).collect();
    if kick.is_empty() {
        return (0.0, 0.0);
    }
    let fps = rate / HOP as f64;
    let end = kick.len() as f64 / fps - 0.01;
    let score = |d: f64| {
        let (mut s, mut t) = (0.0, phase + d);
        while t < end {
            if t >= 0.0 {
                // A 3-hop window (8.7 ms) forgives tiny jitter in the transient.
                let i = (t * fps) as usize;
                s += kick[i.saturating_sub(1)..(i + 2).min(kick.len())].iter().cloned().fold(0.0, f64::max);
            }
            t += beat;
        }
        s
    };
    let steps = (range / 0.001).round() as i64;
    let scores: Vec<(f64, f64)> = (-steps..=steps).map(|i| (i as f64 * 0.001, score(i as f64 * 0.001))).collect();
    let top = scores.iter().map(|s| s.1).fold(f64::MIN, f64::max);
    let mean = scores.iter().map(|s| s.1).sum::<f64>() / scores.len() as f64;
    // Within 3% of the top, the smallest correction wins.
    let best = scores.iter().filter(|s| s.1 >= 0.97 * top).min_by(|a, b| a.0.abs().total_cmp(&b.0.abs())).map(|s| s.0).unwrap_or(0.0);
    (best, if mean > 0.0 { top / mean } else { 0.0 })
}

/// Joint (bpm, phase) search for one grid over the whole track. The peak is
/// very sharp (measured: 706 at 174.0 vs 220 at 173.8 on a synthetic DnB
/// track), so a coarse +-2.5% pass at 0.05 BPM finds it and a fine pass pins
/// it; a narrower first window missed whenever the autocorrelation guess was
/// ~1% off.
fn fit_grid(onset: &[f64], bpm: f64, fps: f64) -> (f64, f64, f64) {
    let (_, b1, _) = grid_search(onset, bpm * 0.975, bpm * 1.025, 0.05, 1.0, fps);
    let (score, b, phase) = grid_search(onset, b1 - 0.06, b1 + 0.06, 0.005, 0.1, fps);
    let mean = onset.iter().sum::<f64>() / onset.len() as f64;
    (b, phase / fps + ONSET_LAG, score / (mean + 1e-9))
}

const NOTES: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
const MAJ: [f64; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MIN: [f64; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];
const CAM_MAJ: [u8; 12] = [8, 3, 10, 5, 12, 7, 2, 9, 4, 11, 6, 1];
const CAM_MIN: [u8; 12] = [5, 12, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10];

/// Chroma -> Krumhansl correlation. Only spectral *peaks* above 100 Hz vote:
/// below that a 10.8 Hz bin spacing spans several semitones (the kick alone
/// read as A), and whitening each frame stops noisy hats from voting.
fn key(sp: &Spectrum) -> (String, String, f64) {
    let (lo, hi) = (sp.bin_of(100.0).max(1), sp.bin_of(2500.0));
    let pcs: Vec<usize> = (lo..hi)
        .map(|k| {
            let hz = k as f64 * sp.rate / NFFT as f64;
            ((12.0 * (hz / 440.0).log2()).round() as i64 + 9).rem_euclid(12) as usize
        })
        .collect();
    let mut chroma = [0f64; 12];
    let mut row: Vec<f32> = vec![0.0; hi - lo];
    for f in 0..sp.frames {
        let r = sp.row(f);
        for (i, k) in (lo..hi).enumerate() {
            row[i] = log1p10(r[k]);
        }
        let mut sorted = row.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = sorted[sorted.len() / 2];
        for i in 1..row.len() - 1 {
            let v = row[i] - med;
            if v > 0.0 && row[i] > row[i - 1] && row[i] > row[i + 1] {
                chroma[pcs[i]] += v as f64;
            }
        }
    }
    let norm = |v: &[f64; 12]| {
        let m = v.iter().sum::<f64>() / 12.0;
        let sd = (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / 12.0).sqrt().max(1e-9);
        v.map(|x| (x - m) / sd)
    };
    let c = norm(&chroma);
    let mut best = (f64::MIN, 0usize, false);
    for r in 0..12 {
        for (minor, prof) in [(false, &MAJ), (true, &MIN)] {
            let mut p = [0f64; 12];
            for i in 0..12 {
                p[(i + r) % 12] = prof[i];
            }
            let p = norm(&p);
            let corr = c.iter().zip(&p).map(|(a, b)| a * b).sum::<f64>() / 12.0;
            if corr > best.0 {
                best = (corr, r, minor);
            }
        }
    }
    let (conf, r, minor) = best;
    if minor {
        (format!("{}m", NOTES[r]), format!("{}A", CAM_MIN[r]), conf)
    } else {
        (NOTES[r].to_string(), format!("{}B", CAM_MAJ[r]), conf)
    }
}

/// Drops/breakdowns from 4-bar block energy, phrase-aligned to the music start.
/// A drop is the first block of a sustained loud region that a quieter block
/// leads into. Builds are ramps (measured 0.60 -> 0.72 -> 0.85 -> 0.97 on the
/// test house track), so "first block near the top after a step" lands on
/// the drop itself, not the start of the build.
fn drops_breaks(e: &[f32], start_bar: usize) -> (Vec<u32>, Vec<u32>) {
    let starts: Vec<usize> = (start_bar..e.len().saturating_sub(3)).step_by(4).collect();
    let blocks: Vec<f32> = starts.iter().map(|&i| e[i..i + 4].iter().sum::<f32>() / 4.0).collect();
    if blocks.len() < 3 {
        return (vec![], vec![]);
    }
    let hi = percentile(&blocks, 85.0);
    let (mut drops, mut breaks) = (Vec::<u32>::new(), Vec::<u32>::new());
    for k in 1..blocks.len() {
        let prev_max = blocks[k.saturating_sub(2)..k].iter().cloned().fold(f32::MIN, f32::max);
        if blocks[k] >= 0.92 * hi && blocks[k - 1] < blocks[k] - 0.08 && prev_max < blocks[k] - 0.05 {
            if drops.last().is_none_or(|&d| starts[k] as u32 - d >= 16) {
                drops.push(starts[k] as u32);
            }
        } else if blocks[k] < blocks[k - 1] - 0.15 && breaks.last().is_none_or(|&b| starts[k] as u32 - b >= 8) {
            breaks.push(starts[k] as u32);
        }
    }
    (drops, breaks)
}

fn sections(feats: &[[f64; 4]], energy: &[f32], start_bar: usize, extra: &[u32]) -> Vec<Section> {
    let nb = feats.len();
    let mut mean = [0f64; 4];
    let mut sd = [0f64; 4];
    for f in feats {
        for j in 0..4 {
            mean[j] += f[j] / nb as f64;
        }
    }
    for f in feats {
        for j in 0..4 {
            sd[j] += (f[j] - mean[j]).powi(2) / nb as f64;
        }
    }
    let z: Vec<[f64; 4]> = feats.iter().map(|f| std::array::from_fn(|j| (f[j] - mean[j]) / (sd[j].sqrt() + 1e-9))).collect();
    let w = 8usize;
    let mut nov = vec![0f64; nb];
    for (i, slot) in nov.iter_mut().enumerate().take(nb.saturating_sub(w / 2)).skip(w / 2) {
        let avg = |a: usize, b: usize| -> [f64; 4] {
            let n = (b - a).max(1) as f64;
            std::array::from_fn(|j| z[a..b].iter().map(|r| r[j]).sum::<f64>() / n)
        };
        let (a, b) = (avg(i.saturating_sub(w), i), avg(i, (i + w).min(nb)));
        *slot = (0..4).map(|j| (b[j] - a[j]).powi(2)).sum::<f64>().sqrt();
    }
    let nm = nov.iter().sum::<f64>() / nb as f64;
    let nsd = (nov.iter().map(|v| (v - nm).powi(2)).sum::<f64>() / nb as f64).sqrt();
    let thr = nm + 0.4 * nsd;
    let mut bounds = vec![start_bar as u32];
    let mut i = start_bar + 4;
    while i + 2 < nb {
        let local = nov[i.saturating_sub(4)..(i + 5).min(nb)].iter().cloned().fold(f64::MIN, f64::max);
        if nov[i] >= thr && nov[i] >= local - 1e-9 && i as u32 - bounds.last().unwrap() >= 4 {
            bounds.push(i as u32);
        }
        i += 4;
    }
    bounds.extend_from_slice(extra);
    bounds.sort_unstable();
    bounds.dedup();
    let mut merged: Vec<u32> = vec![];
    for b in bounds {
        if merged.last().is_none_or(|&m| b - m >= 4) {
            merged.push(b);
        }
    }
    merged.push(nb as u32);
    merged
        .windows(2)
        .map(|p| {
            let (s, e) = (p[0] as usize, p[1] as usize);
            Section { start: p[0], end: p[1], energy: energy[s..e].iter().sum::<f32>() / (e - s).max(1) as f32 }
        })
        .collect()
}

fn percentile(v: &[f32], p: f32) -> f32 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((p / 100.0) * (s.len() - 1) as f32).round() as usize;
    s[idx.min(s.len() - 1)]
}

/// `bpm_range` (from a tag/folder genre guess) folds octave errors: DnB read
/// at 86 becomes 172. Without it the estimate is left alone.
pub fn analyze_pcm(pcm: &Pcm, bpm_range: Option<(f64, f64)>) -> Analysis {
    let x = &pcm.samples;
    let rate = pcm.rate;
    let dur = x.len() as f64 / rate as f64;
    let sp = stft(x, rate);
    let fps = sp.fps();
    let nfr = sp.frames;

    let onset = detrend(&flux(&sp, 0, BINS), fps as usize);
    let low_bins = sp.bin_of(150.0) + 1;
    let low_on = detrend(&flux(&sp, 0, low_bins), fps as usize);

    // A genre hint is a *soft* prior: it tilts the scoring toward the genre's
    // tempo range without excluding the rest. Measured on a real 157-track
    // library against librosa: a hard restriction to the hinted range forced a
    // ~172 BPM collab into "house tempo" (116) just because one listed artist
    // is a house artist; octave folding alone missed 3:2 errors. The prior
    // keeps strong evidence winning and breaks ties the genre's way.
    let (center, width) = match bpm_range {
        Some((lo, hi)) => ((lo * hi).sqrt(), 0.45),
        None => (125.0, 1.0),
    };
    let bpm0 = estimate_tempo(&onset, fps, center, width);
    let (bpm, phase_t, grid_conf) = fit_grid(&onset, bpm0, fps);
    let beat = 60.0 / bpm;
    // Pin the grid to the kicks themselves. The fit above works on 23 ms
    // frames of a 93 ms-window flux, corrected by one average lag; measured on
    // a real library its error ran from -60 to +51 ms per track, so two
    // "beatmatched" decks could flam by 110 ms. Only where the kicks are
    // clear (sharpness), else the fit stands.
    let (d, sharp) = kick_phase_offset(x, rate as f64, beat, phase_t, (0.25 * beat).min(0.125));
    let mut phase_t = if sharp >= KICK_SHARPNESS { phase_t + d } else { phase_t };
    while phase_t < 0.0 {
        phase_t += beat;
    }

    // Downbeat: the beat phase (mod 4) where the kick lands hardest, plus where
    // energy rises (sections start on a "one").
    let frame_rms: Vec<f64> = (0..nfr).map(|f| (sp.row(f).iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / BINS as f64).sqrt()).collect();
    let mut beats_t = vec![];
    let mut t = phase_t;
    while t < dur {
        beats_t.push(t);
        t += beat;
    }
    let bf: Vec<usize> = beats_t.iter().map(|t| ((t * fps) as usize).min(nfr - 1)).collect();
    let low_at: Vec<f64> = bf.iter().map(|&f| low_on[f.saturating_sub(2)..(f + 3).min(nfr)].iter().cloned().fold(0.0, f64::max)).collect();
    let span = ((beat * fps) as usize).max(1);
    let rms_at: Vec<f64> = bf.iter().map(|&f| frame_rms[f..(f + span).min(nfr)].iter().sum::<f64>() / span as f64).collect();
    let rise: Vec<f64> =
        (0..rms_at.len()).map(|i| if i == 0 { 0.0 } else { ((rms_at[i] + 1e-6).ln() - (rms_at[i - 1] + 1e-6).ln()).max(0.0) }).collect();
    let avg = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    let (lm, rm) = (avg(&low_at), avg(&rise));
    let sub = sub_db_per_beat(&sp, &beats_t, beat, low_bins);
    let dphase = (0..4)
        .map(|p| {
            let l: Vec<f64> = low_at.iter().skip(p).step_by(4).cloned().collect();
            let r: Vec<f64> = rise.iter().skip(p).step_by(4).cloned().collect();
            (p, avg(&l) / (lm + 1e-9) + 1.5 * avg(&r) / (rm + 1e-9))
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(p, _)| p)
        .unwrap_or(0);
    let first_down = beats_t.get(dphase).copied().unwrap_or(phase_t);
    let bar = 4.0 * beat;
    let mut downbeat0 = first_down - (first_down / bar).floor() * bar;

    // Bar structure for a downbeat: features, energy, drops (snapped to where
    // the sub-bass returns: in DnB the build is as loud as the drop, and the
    // full-band curve put drops 4-8 bars late), breaks, sections.
    let hi_bin = sp.bin_of(4000.0);
    let beat_idx = |db0: f64| ((db0 - phase_t) / beat).round() as i64;
    let structure = |downbeat0: f64| {
        let mut feats: Vec<[f64; 4]> = vec![];
        let mut t = downbeat0;
        while t < dur - bar * 0.5 {
            let (a, b) = ((t * fps) as usize, (((t + bar) * fps) as usize).min(nfr));
            let rows = if b > a { a..b } else { nfr.saturating_sub(2)..nfr };
            let n = rows.len().max(1) as f64;
            let (mut e, mut lo, mut hi) = (0f64, 0f64, 0f64);
            for f in rows.clone() {
                let r = sp.row(f);
                e += r.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / BINS as f64;
                lo += r[..low_bins * 2].iter().map(|v| *v as f64).sum::<f64>() / (low_bins * 2) as f64;
                hi += r[hi_bin..].iter().map(|v| *v as f64).sum::<f64>() / (BINS - hi_bin) as f64;
            }
            let fl = if b > a { onset[a..b].iter().sum::<f64>() / (b - a) as f64 } else { 0.0 };
            feats.push([10.0 * (e / n + 1e-18).log10(), 20.0 * (lo / n + 1e-9).log10(), 20.0 * (hi / n + 1e-9).log10(), fl]);
            t += bar;
        }
        if feats.is_empty() {
            feats.push([-90.0, -90.0, -90.0, 0.0]);
        }
        let rms_db: Vec<f32> = feats.iter().map(|f| f[0] as f32).collect();
        let top = percentile(&rms_db, 95.0);
        // 0..1 relative energy; 0 = 24 dB below the loud parts.
        let rms_e: Vec<f32> = rms_db.iter().map(|d| ((d - (top - 24.0)) / 24.0).clamp(0.0, 1.0)).collect();
        let fmax = feats.iter().map(|f| f[3]).fold(0.0, f64::max) + 1e-9;
        let bar_energy: Vec<f32> =
            rms_e.iter().zip(&feats).map(|(r, f)| (0.65 * r + 0.35 * (f[3] / fmax) as f32).clamp(0.0, 1.0)).collect();
        let start_bar = rms_e.iter().position(|v| *v > 0.25).unwrap_or(0);
        let (drops, breaks) = drops_breaks(&bar_energy, start_bar);
        let (drops, votes) = refine_drops(&drops, &sub, beat_idx(downbeat0), &bar_energy, start_bar);
        (feats, bar_energy, start_bar, drops, breaks, votes)
    };
    let (mut feats, mut bar_energy, mut start_bar, mut drops, mut breaks, votes) = structure(downbeat0);
    // A drop slams on the "one". When the drops' bass consistently lands on
    // another beat (measured: a DnB track whose drop hit on beat 2, so its
    // double drops landed a beat off), the bar line is off by that much:
    // move it and measure again.
    let total: f64 = votes.iter().sum();
    if let Some((k, w)) = votes.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)) {
        if k != 0 && total > 0.0 && *w >= 0.6 * total && *w >= 25.0 {
            let shift = if k == 3 { -1.0 } else { k as f64 };
            downbeat0 = (downbeat0 + shift * beat).rem_euclid(bar);
            (feats, bar_energy, start_bar, drops, breaks, _) = structure(downbeat0);
        }
    }
    let nb = feats.len();
    let extra: Vec<u32> = drops.iter().chain(&breaks).cloned().collect();
    let secs = sections(&feats, &bar_energy, start_bar, &extra);
    let outro_bar =
        breaks.iter().rev().find(|b| **b as f64 > nb as f64 * 0.6).copied().unwrap_or((nb.saturating_sub(16)).max(start_bar) as u32);

    let (key_name, camelot, key_conf) = key(&sp);
    let peak_db = 20.0 * (x.iter().fold(0f32, |m, v| m.max(v.abs())) as f64 + 1e-9).log10();
    let loudness = loud_parts_lufs(x, rate as f64);
    let gain_db = norm_gain(loudness, peak_db);

    let mut spec_mean = vec![0f64; BINS];
    for f in 0..nfr {
        for (m, v) in spec_mean.iter_mut().zip(sp.row(f)) {
            *m += *v as f64;
        }
    }
    let centroid = spec_mean.iter().enumerate().map(|(k, v)| v * k as f64 * sp.rate / NFFT as f64).sum::<f64>()
        / (spec_mean.iter().sum::<f64>() + 1e-9);
    let om = avg(&onset);
    let osd = (onset.iter().map(|v| (v - om).powi(2)).sum::<f64>() / onset.len() as f64).sqrt();
    let onset_rate = onset.iter().filter(|v| **v > om + osd).count() as f64 / onset.len() as f64 * fps;

    Analysis {
        version: ANALYSIS_VERSION,
        duration: dur,
        bpm: (bpm * 1000.0).round() / 1000.0,
        beat,
        first_beat: phase_t,
        downbeat0,
        grid_conf,
        key: key_name,
        camelot,
        key_conf,
        bars: nb as u32,
        start_bar: start_bar as u32,
        sections: secs,
        drops,
        breaks,
        outro_bar,
        bar_energy: bar_energy.iter().map(|v| (v * 1000.0).round() / 1000.0).collect(),
        gain_db,
        loudness,
        peak_db,
        centroid,
        onset_rate,
        wave: overview(&sp, x, low_bins.max(1), hi_bin),
    }
}

fn overview(sp: &Spectrum, x: &[f32], _low: usize, hi_bin: usize) -> WaveOverview {
    const COLS: usize = 1200;
    let mid_bin = sp.bin_of(250.0);
    let band = |a: usize, b: usize| -> Vec<u8> {
        let per_frame: Vec<f64> =
            (0..sp.frames).map(|f| sp.row(f)[a..b].iter().map(|v| *v as f64).sum::<f64>() / (b - a).max(1) as f64).collect();
        let cols: Vec<f64> = (0..COLS)
            .map(|c| {
                let (s, e) = (c * sp.frames / COLS, ((c + 1) * sp.frames / COLS).max(c * sp.frames / COLS + 1).min(sp.frames));
                per_frame[s..e].iter().sum::<f64>() / (e - s).max(1) as f64
            })
            .collect();
        let mut sorted = cols.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p99 = sorted[(0.99 * (COLS - 1) as f64) as usize] + 1e-9;
        cols.iter().map(|v| ((v / p99).clamp(0.0, 1.0) * 255.0) as u8).collect()
    };
    let n = x.len();
    let peaks: Vec<f32> = (0..COLS)
        .map(|c| x[c * n / COLS..((c + 1) * n / COLS).max(c * n / COLS + 1).min(n)].iter().fold(0f32, |m, v| m.max(v.abs())))
        .collect();
    let pmax = peaks.iter().cloned().fold(1e-9, f32::max);
    WaveOverview {
        low: band(0, mid_bin),
        mid: band(mid_bin, hi_bin),
        high: band(hi_bin, BINS),
        peak: peaks.iter().map(|p| ((p / pmax) * 255.0) as u8).collect(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Synthetic track with known tempo/offset/structure; mirrors the
    /// prototype's generator so the measured numbers in comments still apply.
    pub fn synth(bpm: f64, offset: f64, layout: &[(&str, usize)], rate: u32, seed: u64) -> Pcm {
        let beat = 60.0 / bpm;
        let bar = 4.0 * beat;
        let total = offset + layout.iter().map(|l| l.1).sum::<usize>() as f64 * bar + 2.0;
        let mut y = vec![0f32; (total * rate as f64) as usize];
        let mut rng = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut noise = move || {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((rng >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
        };
        let sr = rate as f32;
        let add = |y: &mut Vec<f32>, start: usize, len: usize, f: &mut dyn FnMut(f32) -> f32| {
            for i in 0..len {
                if start + i < y.len() {
                    y[start + i] += f(i as f32 / sr);
                }
            }
        };
        let mut t0 = offset;
        for (sec, n) in layout {
            for b in 0..*n {
                for s in 0..16 {
                    let st = ((t0 + b as f64 * bar + s as f64 * beat / 4.0) * rate as f64) as usize;
                    if (*sec == "drop" || *sec == "intro" || *sec == "outro") && s % 2 == 0 {
                        add(&mut y, st, 3000, &mut |t| noise() * (-t * 60.0).exp() * 0.25);
                    }
                    let kick = |amp: f32| {
                        move |t: f32| {
                            let ph = 2.0 * std::f32::consts::PI * (50.0 * t + 120.0 / 35.0 * (1.0 - (-t * 35.0).exp()));
                            ph.sin() * (-t * 7.0).exp() * amp
                        }
                    };
                    if *sec == "drop" {
                        if s == 0 || s == 10 {
                            add(&mut y, st, 12000, &mut kick(0.9));
                        }
                        if s == 4 || s == 12 {
                            add(&mut y, st, 8000, &mut |t| {
                                (noise() * 0.6 + (2.0 * std::f32::consts::PI * 190.0 * t).sin() * 0.5) * (-t * 18.0).exp() * 0.6
                            });
                        }
                    }
                    if (*sec == "intro" || *sec == "outro") && s % 4 == 0 && b % 2 == 0 {
                        add(&mut y, st, 12000, &mut kick(0.5));
                    }
                    if *sec == "build" && s % (if b < 4 { 4 } else { 1 }) == 0 {
                        let amp = 0.2 + 0.5 * b as f32 / *n as f32;
                        add(&mut y, st, 4000, &mut |t| noise() * (-t * 18.0).exp() * amp);
                    }
                }
            }
            t0 += *n as f64 * bar;
        }
        let peak = y.iter().fold(1e-9f32, |m, v| m.max(v.abs()));
        y.iter_mut().for_each(|v| *v = *v / peak * 0.9);
        Pcm { samples: y, rate }
    }

    pub const LAYOUT: [(&str, usize); 7] =
        [("intro", 16), ("build", 8), ("drop", 32), ("break", 16), ("build", 8), ("drop", 32), ("outro", 16)];

    #[test]
    fn tempo_grid_and_downbeat_are_exact_on_dnb() {
        let a = analyze_pcm(&synth(174.0, 1.37, &LAYOUT, 22_050, 1), Some((160.0, 180.0)));
        assert!((a.bpm - 174.0).abs() < 0.02, "bpm {}", a.bpm);
        // Downbeat at 1.37 s folds to 1.37 mod bar (1.379 s) = 1.37.
        let bar = 4.0 * a.beat;
        let err = ((a.downbeat0 - 1.37 % bar + bar / 2.0).rem_euclid(bar) - bar / 2.0).abs();
        assert!(err < 0.03, "downbeat {} err {}", a.downbeat0, err);
        assert!(a.grid_conf > 2.0, "grid_conf {}", a.grid_conf);
    }

    #[test]
    fn structure_finds_both_drops_on_house() {
        let a = analyze_pcm(&synth(124.0, 0.5, &LAYOUT, 22_050, 3), None);
        assert!((a.bpm - 124.0).abs() < 0.02, "bpm {}", a.bpm);
        assert_eq!(a.drops, vec![24, 80], "drops {:?} sections {:?}", a.drops, a.sections);
        assert!(a.breaks.contains(&56), "breaks {:?}", a.breaks);
    }

    #[test]
    fn loudness_matches_bs1770_on_a_sine() {
        // A 997 Hz sine at -20 dBFS on both channels reads -20 LUFS.
        let rate = 22_050.0;
        let x: Vec<f32> = (0..(rate as usize * 10)).map(|i| 0.1 * (std::f64::consts::TAU * 997.0 * i as f64 / rate).sin() as f32).collect();
        let l = loud_parts_lufs(&x, rate);
        assert!((l + 20.0).abs() < 0.25, "{l} LUFS");
    }

    #[test]
    fn gain_brings_quiet_and_loud_masters_to_the_target() {
        let mut quiet = synth(124.0, 0.5, &LAYOUT, 22_050, 3);
        quiet.samples.iter_mut().for_each(|v| *v *= 0.5);
        let q = analyze_pcm(&quiet, None);
        // Lifted to the target, or as far as the peak ceiling allows.
        let at_target = (q.loudness + q.gain_db - TARGET_LUFS).abs() < 0.1;
        let at_ceiling = (q.peak_db + q.gain_db - BOOST_PEAK_CEIL_DB).abs() < 0.1;
        assert!(q.gain_db > 5.0 && (at_target || at_ceiling), "quiet: {} + {} (peak {})", q.loudness, q.gain_db, q.peak_db);
        assert!(q.peak_db + q.gain_db <= BOOST_PEAK_CEIL_DB + 1e-9);
        // A hot master is cut, never limited by its peak.
        assert!((norm_gain(-5.0, 2.5) - (TARGET_LUFS + 5.0)).abs() < 1e-9);
        // A boost stops where the peak would pass the ceiling.
        assert!((norm_gain(-16.0, 0.0) - BOOST_PEAK_CEIL_DB).abs() < 1e-9);
    }

    #[test]
    fn octave_hint_folds_half_time_reading() {
        let a = analyze_pcm(&synth(172.0, 0.2, &LAYOUT, 22_050, 2), Some((160.0, 180.0)));
        assert!((a.bpm - 172.0).abs() < 0.02, "bpm {}", a.bpm);
    }
}
