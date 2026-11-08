//! File -> mono f32 PCM at an analysis-friendly rate.
//!
//! Analysis runs at ~22-24 kHz: everything it measures (onsets, a kick's low
//! band, chroma up to 2.5 kHz) lives well below 11 kHz, and halving the rate
//! halves STFT cost. The decimator is a hand-rolled half-band FIR (pillar 7: a
//! 31-tap windowed-sinc lowpass is small, stable and fully understood) rather
//! than a resampling crate. Playback never uses this path: the UI decodes the
//! original file at full quality itself.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::sample::Sample;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymErr;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::Error;

#[derive(Debug)]
pub struct Pcm {
    pub samples: Vec<f32>,
    pub rate: u32,
}

/// Decode to mono. symphonia first (pure Rust, no process spawn); for codecs
/// it doesn't implement (Opus above all: ~70% of a real test library was
/// .opus, and every one failed silently before this fallback) ffmpeg decodes
/// instead. If both fail, the error says why, so the UI can show it.
pub fn decode_mono(path: &Path) -> Result<Pcm, Error> {
    match decode_symphonia(path) {
        Ok(p) => Ok(p),
        Err(first) => decode_ffmpeg(path).map_err(|second| Error::Decode(format!("{first}; ffmpeg fallback: {second}"))),
    }
}

/// Mono f32 at 24 kHz straight from ffmpeg (`-vn` skips embedded cover art).
fn decode_ffmpeg(path: &Path) -> Result<Pcm, String> {
    let out = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "24000", "-f", "f32le", "-"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "ffmpeg isn't installed (needed for Opus and other formats symphonia can't decode)".to_string()
            } else {
                e.to_string()
            }
        })?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("ffmpeg failed").chars().take(200).collect());
    }
    let samples: Vec<f32> = out.stdout.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
    if samples.is_empty() {
        return Err("no audio in file".into());
    }
    Ok(Pcm { samples, rate: 24_000 })
}

fn decode_symphonia(path: &Path) -> Result<Pcm, Error> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| Error::Decode(e.to_string()))?;
    let track = format.default_track(TrackType::Audio).ok_or_else(|| Error::Decode("no audio track".into()))?;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or_else(|| Error::Decode("no audio codec parameters".into()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| Error::Decode(e.to_string()))?;
    let track_id = track.id;

    let mut mono = Vec::new();
    let mut scratch: Vec<f32> = Vec::new();
    let mut rate = 0u32;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymErr::ResetRequired) => break,
            Err(e) => return Err(Error::Decode(e.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                rate = buf.spec().rate();
                let ch = buf.spec().channels().count().max(1);
                scratch.resize(buf.samples_interleaved(), f32::MID);
                buf.copy_to_slice_interleaved(&mut scratch);
                let inv = 1.0 / ch as f32;
                mono.extend(scratch.chunks_exact(ch).map(|f| f.iter().sum::<f32>() * inv));
            }
            // A corrupt frame mid-file is common in the wild; skip it, keep the track.
            Err(SymErr::DecodeError(_)) => continue,
            Err(e) => return Err(Error::Decode(e.to_string())),
        }
    }
    if rate == 0 || mono.is_empty() {
        return Err(Error::Decode("no audio decoded".into()));
    }
    Ok(Pcm { samples: mono, rate })
}

/// Halve the rate until it is at most 30 kHz (48k -> 24k, 44.1k -> 22.05k).
pub fn to_analysis_rate(mut pcm: Pcm) -> Pcm {
    while pcm.rate > 30_000 {
        pcm = Pcm { samples: halfband_decimate(&pcm.samples), rate: pcm.rate / 2 };
    }
    pcm
}

fn halfband_decimate(x: &[f32]) -> Vec<f32> {
    const TAPS: usize = 31;
    const MID: isize = (TAPS / 2) as isize;
    let mut h = [0f32; TAPS];
    for (i, v) in h.iter_mut().enumerate() {
        let n = i as f32 - MID as f32;
        let sinc = if n == 0.0 { 0.5 } else { (std::f32::consts::PI * n * 0.5).sin() / (std::f32::consts::PI * n) };
        let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (TAPS - 1) as f32).cos();
        *v = sinc * w;
    }
    let norm: f32 = h.iter().sum();
    h.iter_mut().for_each(|v| *v /= norm);
    let n = x.len();
    (0..n / 2)
        .map(|o| {
            let c = (o * 2) as isize;
            h.iter().enumerate().fold(0.0, |acc, (k, hk)| {
                let j = c + k as isize - MID;
                if j >= 0 && (j as usize) < n {
                    acc + hk * x[j as usize]
                } else {
                    acc
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_decodes_via_ffmpeg_fallback() {
        let dir = std::env::temp_dir().join(format!("selecta-opus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("tone.opus");
        let made = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=2", "-c:a", "libopus"])
            .arg(&f)
            .status();
        if !made.is_ok_and(|s| s.success()) {
            eprintln!("skipping: ffmpeg with libopus not available");
            return;
        }
        assert!(decode_symphonia(&f).is_err(), "symphonia can't do Opus; if this starts passing the fallback may be removable");
        let pcm = decode_mono(&f).expect("ffmpeg fallback decodes opus");
        let secs = pcm.samples.len() as f64 / pcm.rate as f64;
        assert!((secs - 2.0).abs() < 0.1, "decoded {secs}s");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unreadable_file_reports_both_reasons() {
        let f = std::env::temp_dir().join(format!("selecta-junk-{}.mp3", std::process::id()));
        std::fs::write(&f, b"definitely not audio").unwrap();
        let e = decode_mono(&f).unwrap_err().to_string();
        assert!(e.contains("ffmpeg"), "{e}");
        std::fs::remove_file(&f).unwrap();
    }

    #[test]
    fn decimation_keeps_low_tones_and_kills_high_ones() {
        let rate = 44_100.0;
        let tone = |f: f32| (0..44_100).map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / rate).sin()).collect::<Vec<_>>();
        let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
        let low = halfband_decimate(&tone(440.0));
        let high = halfband_decimate(&tone(18_000.0));
        assert!(rms(&low[100..]) > 0.65, "440 Hz should pass ({})", rms(&low[100..]));
        assert!(rms(&high[100..]) < 0.05, "18 kHz should be removed ({})", rms(&high[100..]));
    }
}
