//! Stem separation: the vocal stem of every track, made in the background.
//!
//! Why vocals only: mashups, acapella builds and bootlegs all come down to
//! "this vocal over that beat". The engine derives the instrumental as
//! (original - vocals) when it loads a track, so the full mix is always the
//! untouched original and one ~4 MB Ogg per track is all the disk it costs.
//!
//! Why a Python worker: the best open separator (Demucs, htdemucs) is a
//! PyTorch model with no maintained Rust/ONNX port that matches its quality.
//! It runs in a private env Selecta creates with `uv` (never the user's
//! system Python), as one long-lived process so the model loads once, and
//! uses the GPU when torch can see one. If anything about it fails, tracks
//! simply have no stems and every stem move degrades to a normal mix.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use ts_rs::TS;

use crate::analysis::Analysis;

const WORKER: &str = include_str!("stems_worker.py");
/// Bump when the worker's output changes meaning (forces re-separation).
pub const STEMS_VERSION: u32 = 1;

#[derive(Serialize, TS, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StemsStatus {
    pub installed: bool,
    pub installing: bool,
    /// "cuda" / "cpu" once the worker has started.
    pub device: Option<String>,
    /// Last install log line, or why it isn't available.
    pub detail: String,
    pub done: u64,
    pub total: u64,
}

/// Where the env, the worker script and the stem files live.
#[derive(Clone, Debug)]
pub struct StemPaths {
    pub env: PathBuf,
    pub out: PathBuf,
}

impl StemPaths {
    pub fn new(data_dir: &Path) -> Self {
        StemPaths { env: data_dir.join("stems-env"), out: data_dir.join("stems") }
    }
    pub fn python(&self) -> PathBuf {
        if cfg!(windows) {
            self.env.join("Scripts").join("python.exe")
        } else {
            self.env.join("bin").join("python")
        }
    }
    fn marker(&self) -> PathBuf {
        self.env.join(".selecta-ready")
    }
    pub fn installed(&self) -> bool {
        self.marker().exists() && self.python().exists()
    }
    pub fn vocals_file(&self, id: &str) -> PathBuf {
        self.out.join(format!("{id}.v{STEMS_VERSION}.vocals.ogg"))
    }
}

fn has_nvidia() -> bool {
    Command::new("nvidia-smi").arg("-L").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Create the private env and install Demucs into it. `log` gets one line per
/// step. Torch is a large download (CUDA build ~2.5 GB, CPU build ~200 MB),
/// so this is only ever started by the user.
pub fn install(paths: &StemPaths, log: &mut dyn FnMut(&str)) -> Result<(), String> {
    let uv = which("uv").ok_or("Stem separation needs `uv` (https://docs.astral.sh/uv/). Install it, then try again.")?;
    let run = |args: &[&str], log: &mut dyn FnMut(&str)| -> Result<(), String> {
        log(&format!("uv {}", args.join(" ")));
        let out = Command::new(&uv).args(args).stdin(Stdio::null()).output().map_err(|e| format!("couldn't run uv: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            Err(err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("uv failed").trim().to_string())
        }
    };
    let env = paths.env.to_string_lossy().to_string();
    run(&["venv", "--quiet", "--python", "3.12", &env], log)?;
    let py = paths.python().to_string_lossy().to_string();
    if has_nvidia() {
        log("NVIDIA GPU found: installing the CUDA build of PyTorch (large download, once)");
        run(&["pip", "install", "--quiet", "--python", &py, "torch", "torchaudio"], log)?;
    } else {
        log("No NVIDIA GPU: installing the CPU build of PyTorch");
        run(
            &["pip", "install", "--quiet", "--python", &py, "--index-url", "https://download.pytorch.org/whl/cpu", "torch", "torchaudio"],
            log,
        )?;
    }
    log("Installing Demucs");
    run(&["pip", "install", "--quiet", "--python", &py, "demucs", "soundfile"], log)?;
    std::fs::write(paths.marker(), b"ok").map_err(|e| e.to_string())?;
    log("Stem engine ready");
    Ok(())
}

fn which(bin: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let extra = home.iter().flat_map(|h| [h.join(".local/bin"), h.join(".cargo/bin")]).collect::<Vec<_>>();
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(extra)
        .map(|d| d.join(if cfg!(windows) { format!("{bin}.exe") } else { bin.to_string() }))
        .find(|p| p.is_file())
}

/// A running worker process.
pub struct Separator {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    pub device: String,
}

#[derive(serde::Deserialize)]
struct Reply {
    ready: Option<bool>,
    device: Option<String>,
    ok: Option<bool>,
    env: Option<Vec<f32>>,
    error: Option<String>,
    /// Realtime factor of the device this track ran on.
    speed: Option<f64>,
}

impl Separator {
    pub fn spawn(paths: &StemPaths) -> Result<Self, String> {
        std::fs::create_dir_all(&paths.out).map_err(|e| e.to_string())?;
        let script = paths.env.join("selecta_stems.py");
        std::fs::write(&script, WORKER).map_err(|e| e.to_string())?;
        let mut child = Command::new(paths.python())
            .arg("-u")
            .arg(&script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("couldn't start the stem worker: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let mut stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let mut line = String::new();
        // First line: model loaded (or the process died trying).
        stdout.read_line(&mut line).map_err(|e| e.to_string())?;
        let r: Reply = serde_json::from_str(&line).map_err(|_| "the stem worker failed to start (is the env complete?)".to_string())?;
        if r.ready != Some(true) {
            return Err("the stem worker failed to load its model".into());
        }
        Ok(Separator { child, stdin, stdout, device: r.device.unwrap_or_else(|| "cpu".into()) })
    }

    /// Separate one file; returns the vocal envelope (dB, 10 frames/s).
    pub fn vocals(&mut self, id: &str, path: &Path, out: &Path) -> Result<Vec<f32>, String> {
        let job = serde_json::json!({"id": id, "path": path, "out": out});
        writeln!(self.stdin, "{job}").map_err(|e| format!("stem worker exited: {e}"))?;
        self.stdin.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        if self.stdout.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Err("stem worker exited".into());
        }
        let r: Reply = serde_json::from_str(&line).map_err(|e| format!("bad worker reply: {e}"))?;
        if let Some(d) = &r.device {
            self.device = match r.speed {
                Some(x) => format!("{d} {x:.1}x"),
                None => d.clone(),
            };
        }
        if r.ok == Some(true) {
            Ok(r.env.unwrap_or_default())
        } else {
            Err(r.error.unwrap_or_else(|| "separation failed".into()))
        }
    }
}

impl Drop for Separator {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The work list: one worker drains it; set tracks jump the queue.
#[derive(Default)]
pub struct StemQueue {
    q: Mutex<VecDeque<(String, PathBuf)>>,
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub running: AtomicBool,
    pub device: Mutex<Option<String>>,
    pub installing: AtomicBool,
    pub detail: Mutex<String>,
}

impl StemQueue {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    /// Add tracks at the back (library) or the front (`urgent`: the set about to play).
    pub fn push(&self, items: Vec<(String, PathBuf)>, urgent: bool) {
        let mut q = self.q.lock().unwrap();
        // Front-pushing in reverse keeps the urgent batch in its given order.
        let items: Vec<_> = if urgent { items.into_iter().rev().collect() } else { items };
        for it in items {
            if let Some(i) = q.iter().position(|x| x.0 == it.0) {
                if !urgent {
                    continue;
                }
                q.remove(i);
            } else {
                self.total.fetch_add(1, Ordering::Relaxed);
            }
            if urgent {
                q.push_front(it);
            } else {
                q.push_back(it);
            }
        }
    }
    pub fn is_empty(&self) -> bool {
        self.q.lock().unwrap().is_empty()
    }
    pub fn pop(&self) -> Option<(String, PathBuf)> {
        self.q.lock().unwrap().pop_front()
    }
    pub fn clear(&self) {
        self.q.lock().unwrap().clear();
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
    }
    pub fn status(&self, paths: &StemPaths) -> StemsStatus {
        let installed = paths.installed();
        let detail = self.detail.lock().unwrap().clone();
        StemsStatus {
            installed,
            installing: self.installing.load(Ordering::Relaxed),
            device: self.device.lock().unwrap().clone(),
            detail: if detail.is_empty() && !installed { "Not installed".into() } else { detail },
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
        }
    }
}

/// Per-bar vocal presence 0..1 from the worker's envelope and the track's grid.
/// 0 is silence (< -45 dB), 1 a lead vocal at full level (> -18 dB).
pub fn vocal_bars(env: &[f32], a: &Analysis) -> Vec<f32> {
    let bar = 4.0 * a.beat;
    (0..a.bars)
        .map(|b| {
            let t0 = a.downbeat0 + b as f64 * bar;
            let (i0, i1) = ((t0 * 10.0).max(0.0) as usize, (((t0 + bar) * 10.0).max(0.0) as usize).min(env.len()));
            if i1 <= i0 {
                return 0.0;
            }
            // 70th percentile: a vocal phrase, not one stray breath, makes a bar "vocal".
            let mut v: Vec<f32> = env[i0..i1].to_vec();
            v.sort_by(|x, y| x.partial_cmp(y).unwrap());
            let p = v[(v.len() * 7 / 10).min(v.len() - 1)];
            ((p + 45.0) / 27.0).clamp(0.0, 1.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urgent_items_jump_the_queue_without_double_counting() {
        let q = StemQueue::new();
        q.push(vec![("a".into(), "a".into()), ("b".into(), "b".into()), ("c".into(), "c".into())], false);
        q.push(vec![("c".into(), "c".into()), ("d".into(), "d".into())], true);
        assert_eq!(q.total.load(Ordering::Relaxed), 4);
        let order: Vec<String> = std::iter::from_fn(|| q.pop().map(|x| x.0)).collect();
        assert_eq!(order, ["c", "d", "a", "b"]);
    }

    #[test]
    fn vocal_bars_follow_the_envelope() {
        let base =
            crate::analysis::analyze_pcm(&crate::analysis::tests::synth(120.0, 0.5, &crate::analysis::tests::LAYOUT, 22_050, 1), None);
        let a = Analysis { beat: 0.5, downbeat0: 0.0, bars: 4, ..base };
        // 2 s bars at 10 fps: silent, loud vocal, quiet vocal, silent.
        let mut env = vec![-80.0; 20];
        env.extend(vec![-12.0; 20]);
        env.extend(vec![-31.5; 20]);
        env.extend(vec![-80.0; 20]);
        let v = vocal_bars(&env, &a);
        assert_eq!(v[0], 0.0);
        assert_eq!(v[1], 1.0);
        assert!((v[2] - 0.5).abs() < 0.01, "{}", v[2]);
        assert_eq!(v[3], 0.0);
    }
}
