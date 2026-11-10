//! Desktop shell. Owns the window, settings file, and the bridge from UI
//! commands to `selecta-core`. No domain logic lives here.
//!
//! Every command doing I/O or DSP is `async` and runs its work on a blocking
//! thread (pillar 9): a plain `#[tauri::command] fn` runs on the main thread
//! and freezes the whole window for its duration. This bit the Lyric Overlay
//! for 40-85 s per local file before being fixed there.

#[cfg(target_os = "linux")]
mod mpris;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use selecta_core::analysis::{analyze_pcm, Analysis};
use selecta_core::compose;
use selecta_core::decode::{decode_mono, to_analysis_rate};
use selecta_core::intent;
use selecta_core::jobs::{GroupProgress, Priority, Scheduler};
use selecta_core::library::{Library, ScanReport, SetSummary, TrackSummary};
use selecta_core::llm::{self, AiConfig, AiStatus};
use selecta_core::planner::{self, SetPlan};
use selecta_core::stems::{self, Separator, StemPaths, StemQueue, StemsStatus};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct Settings {
    music_dirs: Vec<String>,
    #[serde(default)]
    ai: AiConfig,
    /// Separate vocal stems in the background once the stem engine is installed.
    #[serde(default = "yes")]
    stems: bool,
    /// Tempo changes keep the key, and sets may shift keys to mix harmonically.
    #[serde(default = "yes")]
    keylock: bool,
    /// How late the speakers are behind the audio clock (ms), calibrated by
    /// the user. None: use the detected output's typical delay.
    #[serde(default)]
    output_delay_ms: Option<f64>,
}

/// The audio output the desktop is using, and how late it typically is.
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct AudioOutput {
    name: String,
    description: String,
    bluetooth: bool,
    codec: Option<String>,
    /// Typical delay of this output on top of the engine's own (ms).
    suggested_delay_ms: f64,
}

/// Bluetooth audio is 100-300 ms late (codec buffering), and neither WebKit
/// nor PipeWire's pulse layer report it: WebKit says 3 ms, `pactl` says 0.
/// Measured as a ~1/6 bar gap between what the decks showed and what a user
/// heard on an SBC speaker. The codec's typical delay is the starting point;
/// the sync test in Settings calibrates the rest.
fn detect_output() -> AudioOutput {
    let run = |args: &[&str]| {
        std::process::Command::new("pactl")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
    };
    let Some(name) = run(&["get-default-sink"]).map(|s| s.trim().to_string()) else { return AudioOutput::default() };
    let sinks = run(&["list", "sinks"]).unwrap_or_default();
    let block = sinks.split("Sink #").find(|b| b.contains(&format!("Name: {name}"))).unwrap_or("");
    let prop = |key: &str| {
        block.lines().find_map(|l| {
            let l = l.trim();
            l.strip_prefix(key).and_then(|r| r.trim_start().strip_prefix('=')).map(|v| v.trim().trim_matches('"').to_string())
        })
    };
    let bluetooth = name.starts_with("bluez") || prop("device.api").as_deref() == Some("bluez5");
    let codec = prop("api.bluez5.codec");
    let suggested_delay_ms = if !bluetooth {
        0.0
    } else {
        match codec.as_deref() {
            Some("aptx_ll" | "aptx_ll_duplex" | "faststream") => 70.0,
            Some("lc3" | "opus_05" | "opus_g") => 130.0,
            Some("aptx" | "aptx_hd") => 180.0,
            Some("aac") => 250.0,
            Some("ldac") => 280.0,
            _ => 220.0, // SBC and unknown
        }
    };
    AudioOutput { description: prop("device.description").unwrap_or_else(|| name.clone()), name, bluetooth, codec, suggested_delay_ms }
}

#[tauri::command(async)]
async fn audio_output() -> Res<AudioOutput> {
    Ok(tauri::async_runtime::spawn_blocking(detect_output).await.unwrap_or_default())
}

fn yes() -> bool {
    true
}

/// (done, total) for library tagging, shown alongside scheduler groups.
#[derive(Default)]
struct TagProgress {
    done: AtomicU64,
    total: AtomicU64,
}

struct App {
    tagging: Arc<TagProgress>,
    lib: Arc<Library>,
    jobs: Arc<Scheduler>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    stems: Arc<StemQueue>,
    stem_paths: StemPaths,
}

type Res<T> = Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?
}

fn analyse_file(path: &std::path::Path, bpm_hint: Option<(f64, f64)>) -> Res<Analysis> {
    let pcm = to_analysis_rate(decode_mono(path).map_err(err)?);
    Ok(analyze_pcm(&pcm, bpm_hint))
}

#[tauri::command(async)]
async fn settings_get(app: State<'_, App>) -> Res<Settings> {
    Ok(app.settings.lock().unwrap().clone())
}

#[tauri::command(async)]
async fn settings_set(handle: tauri::AppHandle, app: State<'_, App>, settings: Settings) -> Res<()> {
    std::fs::write(&app.settings_path, serde_json::to_vec_pretty(&settings).map_err(err)?).map_err(err)?;
    allow_dirs(&handle, &settings.music_dirs);
    *app.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command(async)]
async fn library_tracks(app: State<'_, App>) -> Res<Vec<TrackSummary>> {
    let lib = app.lib.clone();
    blocking(move || lib.tracks().map_err(err)).await
}

/// Scan the music folders, then queue analysis of anything new or stale as
/// background (`Idle`) work. Returns as soon as the scan itself is done.
#[tauri::command(async)]
async fn library_scan(app: State<'_, App>) -> Res<ScanReport> {
    let dirs: Vec<PathBuf> = app.settings.lock().unwrap().music_dirs.iter().map(PathBuf::from).collect();
    let (lib, jobs) = (app.lib.clone(), app.jobs.clone());
    let ai = app.settings.lock().unwrap().ai.clone();
    let tagging = app.tagging.clone();
    let want_stems = app.settings.lock().unwrap().stems && app.stem_paths.installed();
    let (stem_queue, stem_paths) = (app.stems.clone(), app.stem_paths.clone());
    blocking(move || {
        let rep = lib.scan(&dirs).map_err(err)?;
        if ai.tag_library && llm::status(&ai).available {
            queue_tagging(&lib, &jobs, &ai, &tagging);
        }
        if want_stems {
            if let Ok(todo) = lib.needs_stems() {
                stem_queue.push(todo, false);
                drain_stems(&lib, &jobs, &stem_queue, &stem_paths);
            }
        }
        for (id, path, hint) in lib.needs_analysis().map_err(err)? {
            let lib = lib.clone();
            jobs.submit(&format!("analyse:{id}"), "analysis", Priority::Idle, move |cancel| {
                if cancel.is_cancelled() {
                    return;
                }
                match analyse_file(&path, hint) {
                    Ok(a) => {
                        let _ = lib.save_analysis(&id, &a);
                    }
                    Err(e) => {
                        let _ = lib.save_analysis_error(&id, &e);
                    }
                }
            });
        }
        Ok(rep)
    })
    .await
}

/// Analysis for one track, computing it right now if missing: the user just
/// put this track on a deck and is waiting for its grid.
#[tauri::command(async)]
async fn track_analysis(app: State<'_, App>, id: String) -> Res<Analysis> {
    let lib = app.lib.clone();
    blocking(move || {
        if let Some(a) = lib.analysis(&id).map_err(err)? {
            return Ok(a);
        }
        let path = lib.path_of(&id).map_err(err)?.ok_or("unknown track")?;
        let a = analyse_file(&path, None)?;
        lib.save_analysis(&id, &a).map_err(err)?;
        Ok(a)
    })
    .await
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PlanOptions {
    duration_min: Option<f64>,
    chaos: Option<f64>,
    seed: Option<u64>,
    /// Style switches from the set builder; they override what the prompt implies.
    rapid: Option<bool>,
    mashups: Option<bool>,
    /// Plan by rules only, even with AI on (autoplay tests, offline use).
    #[serde(default)]
    no_ai: bool,
    smooth: Option<bool>,
    turntablism: Option<bool>,
}

impl PlanOptions {
    fn apply_style(&self, it: &mut intent::Intent) {
        if let Some(r) = self.rapid {
            it.rapid = r;
        }
        if let Some(m) = self.mashups {
            it.mashups = if m { 0.95 } else { it.mashups.min(0.2) };
        }
        if let Some(s) = self.smooth {
            it.smooth = s;
            if s && self.chaos.is_none() {
                it.chaos = it.chaos.min(0.35);
            }
        }
        if let Some(t) = self.turntablism {
            it.turntablism = t;
        }
        if let Some(c) = self.chaos {
            it.chaos = c.clamp(0.0, 1.0);
        }
    }
}

/// Prompt -> saved set plan. Runs through the scheduler at `Now` priority
/// (pillar 2), and only over tracks already analysed; anything still in the
/// analysis queue is mentioned in the plan's notes rather than waited on.
/// Prompt -> saved set plan. Runs through the scheduler at `Now` priority
/// (pillar 2), only over analysed tracks. With AI on, Claude refines the
/// intent and curates a shortlist first; any AI failure is a note on the plan,
/// never a failed set. Progress lines go to the UI as `plan-progress` events.
#[tauri::command(async)]
async fn plan_set(handle: tauri::AppHandle, app: State<'_, App>, prompt: String, options: Option<PlanOptions>) -> Res<SetPlan> {
    let (lib, jobs) = (app.lib.clone(), app.jobs.clone());
    let ai = app.settings.lock().unwrap().ai.clone();
    let keylock = app.settings.lock().unwrap().keylock;
    let o = options.unwrap_or_default();
    let rx = jobs
        .run_now(&format!("plan:{prompt}:{:?}", o.seed), move |_| -> Res<SetPlan> {
            let say = |m: &str| {
                let _ = handle.emit("plan-progress", m.to_string());
            };
            say("Reading the prompt");
            let lib_artists: Vec<String> = lib.tracks().map_err(err)?.into_iter().map(|t| t.artist).collect();
            let mut it = intent::parse(&prompt, &lib_artists);
            if let Some(d) = o.duration_min.filter(|d| *d > 0.0) {
                it.duration_min = d;
                it.duration_explicit = true;
            }
            o.apply_style(&mut it);
            let mut ai_notes = vec![];
            let ai_on = llm::status(&ai).available && !o.no_ai;
            let seed = o.seed.unwrap_or_else(|| {
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
            });
            let cands = lib.candidates().map_err(err)?;
            let mut plan = compose::build_set(&mut it, &cands, &ai, ai_on, seed, keylock, &|it| o.apply_style(it), &say, &mut ai_notes)?;
            plan.notes.extend(ai_notes);
            let pending = lib.needs_analysis().map_err(err)?.len();
            if pending > 0 {
                plan.notes.push(format!("{pending} tracks are still being analysed and weren't considered yet."));
            }
            lib.save_set(&plan).map_err(err)?;
            Ok(plan)
        })
        .ok_or("Already planning that set.")?;
    blocking(move || rx.recv().map_err(|_| "planning was cancelled".to_string())?).await
}

/// Re-script the rest of a set after a locked-in track (see `planner::rescript`).
#[tauri::command(async)]
async fn plan_splice(
    app: State<'_, App>,
    intent: intent::Intent,
    from: planner::SetTrack,
    rest: Vec<String>,
) -> Res<Vec<planner::SetTrack>> {
    let lib = app.lib.clone();
    let keylock = app.settings.lock().unwrap().keylock;
    blocking(move || {
        let cands = lib.candidates().map_err(err)?;
        planner::rescript(&intent, &cands, &from, &rest, 0x5e1ec7a, keylock)
    })
    .await
}

/// Tag every untagged track with the AI, 60 per call, one call at a time.
/// One sequential job rather than one job per batch: the scheduler runs a
/// worker per core, and eleven Claude processes at once helps nobody.
fn queue_tagging(lib: &Arc<Library>, jobs: &Arc<Scheduler>, ai: &AiConfig, progress: &Arc<TagProgress>) {
    let Ok(todo) = lib.untagged() else { return };
    if todo.is_empty() {
        return;
    }
    let (lib, ai, progress) = (lib.clone(), ai.clone(), progress.clone());
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(todo.len() as u64, Ordering::Relaxed);
    jobs.submit("ai-tag-library", "tagging", Priority::Idle, move |cancel| {
        for batch in todo.chunks(60) {
            if cancel.is_cancelled() {
                break;
            }
            match llm::tag_tracks(&ai, batch) {
                Ok(tags) => {
                    for (id, t) in tags {
                        let _ = lib.save_tags(&id, &t);
                    }
                }
                Err(_) => break, // signed out / offline: stop, retry on the next scan
            }
            progress.done.fetch_add(batch.len() as u64, Ordering::Relaxed);
        }
        progress.total.store(0, Ordering::Relaxed);
    });
}

#[tauri::command(async)]
async fn library_tag(app: State<'_, App>) -> Res<u64> {
    let ai = app.settings.lock().unwrap().ai.clone();
    let st = llm::status(&ai);
    if !st.available {
        return Err(st.detail);
    }
    queue_tagging(&app.lib, &app.jobs, &ai, &app.tagging);
    Ok(app.tagging.total.load(Ordering::Relaxed))
}

#[tauri::command(async)]
async fn ai_status(app: State<'_, App>) -> Res<AiStatus> {
    Ok(llm::status(&app.settings.lock().unwrap().ai))
}

/// One tiny round trip to prove the CLI works: signed in, model reachable.
#[tauri::command(async)]
async fn ai_test(app: State<'_, App>) -> Res<String> {
    let ai = app.settings.lock().unwrap().ai.clone();
    blocking(move || {
        let t = std::time::Instant::now();
        let reply =
            llm::ask(&ai, "Reply with ONLY this JSON: {\"ok\": true, \"hype\": \"<a 3-word DJ hype line>\"}", Duration::from_secs(90))?;
        let v = llm::parse_json(&reply).ok_or("the reply wasn't JSON")?;
        Ok(format!("{} answered in {:.1}s: \"{}\"", ai.model, t.elapsed().as_secs_f64(), v["hype"].as_str().unwrap_or("ok")))
    })
    .await
}

/// Run the stem worker until the queue is empty. One worker for the whole
/// app (it owns the GPU); a second call while it runs is a no-op, and the
/// running loop picks up whatever was queued meanwhile.
fn drain_stems(lib: &Arc<Library>, jobs: &Arc<Scheduler>, q: &Arc<StemQueue>, paths: &StemPaths) {
    if !paths.installed() || q.running.swap(true, Ordering::SeqCst) {
        return;
    }
    let (lib, q, paths) = (lib.clone(), q.clone(), paths.clone());
    jobs.submit("stems-worker", "stems", Priority::Idle, move |cancel| {
        let mut sep: Option<Separator> = None;
        loop {
            while let Some((id, path)) = q.pop() {
                if cancel.is_cancelled() {
                    break;
                }
                if sep.is_none() {
                    match Separator::spawn(&paths) {
                        Ok(s) => {
                            *q.device.lock().unwrap() = Some(s.device.clone());
                            sep = Some(s);
                        }
                        Err(e) => {
                            *q.detail.lock().unwrap() = e;
                            q.clear();
                            break;
                        }
                    }
                }
                let out = paths.vocals_file(&id);
                let res = sep.as_mut().unwrap().vocals(&id, &path, &out);
                if let Some(s) = &sep {
                    *q.device.lock().unwrap() = Some(s.device.clone());
                }
                match res {
                    Ok(env) => {
                        let _ = lib.save_stems(&id, &out, &env);
                    }
                    Err(e) => {
                        let _ = lib.save_stems_error(&id, &e);
                        if e.contains("exited") {
                            sep = None; // respawn for the next track
                        }
                    }
                }
                q.done.fetch_add(1, Ordering::Relaxed);
            }
            q.running.store(false, Ordering::SeqCst);
            // Something queued between the last pop and the flag? Take it, unless another drain did.
            if cancel.is_cancelled() || q.is_empty() || q.running.swap(true, Ordering::SeqCst) {
                break;
            }
        }
        if q.is_empty() {
            q.done.store(0, Ordering::Relaxed);
            q.total.store(0, Ordering::Relaxed);
        }
    });
}

#[tauri::command(async)]
async fn stems_status(app: State<'_, App>) -> Res<StemsStatus> {
    Ok(app.stems.status(&app.stem_paths))
}

/// Install the stem engine in the background; progress shows in `stems_status`.
#[tauri::command(async)]
async fn stems_install(app: State<'_, App>) -> Res<()> {
    if app.stems.installing.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let (q, paths, lib, jobs) = (app.stems.clone(), app.stem_paths.clone(), app.lib.clone(), app.jobs.clone());
    let enabled = app.settings.lock().unwrap().stems;
    std::thread::spawn(move || {
        let mut log = |m: &str| *q.detail.lock().unwrap() = m.to_string();
        let res = stems::install(&paths, &mut log);
        if let Err(e) = &res {
            *q.detail.lock().unwrap() = format!("Install failed: {e}");
        }
        q.installing.store(false, Ordering::SeqCst);
        if res.is_ok() && enabled {
            if let Ok(todo) = lib.needs_stems() {
                q.push(todo, false);
                drain_stems(&lib, &jobs, &q, &paths);
            }
        }
    });
    Ok(())
}

/// Separate these tracks next (the set that's about to play).
#[tauri::command(async)]
async fn stems_prioritise(app: State<'_, App>, ids: Vec<String>) -> Res<()> {
    if !app.stem_paths.installed() || !app.settings.lock().unwrap().stems {
        return Ok(());
    }
    let (lib, jobs, q, paths) = (app.lib.clone(), app.jobs.clone(), app.stems.clone(), app.stem_paths.clone());
    blocking(move || {
        let todo: Vec<_> = lib.needs_stems().map_err(err)?.into_iter().filter(|(id, _)| ids.contains(id)).collect();
        q.push(todo, true);
        drain_stems(&lib, &jobs, &q, &paths);
        Ok(())
    })
    .await
}

/// Path of a track's vocal stem, if separated.
#[tauri::command(async)]
async fn track_vocals(app: State<'_, App>, id: String) -> Res<Option<String>> {
    let lib = app.lib.clone();
    blocking(move || Ok(lib.vocals_path(&id).map_err(err)?.filter(|p| p.exists()).map(|p| p.to_string_lossy().to_string()))).await
}

/// What the listener hears now, for the Lyric Overlay and desktop media
/// controls (MPRIS on Linux; a no-op elsewhere for now).
#[derive(Default)]
struct NowPlayingBus {
    #[cfg(target_os = "linux")]
    mpris: tokio::sync::OnceCell<Option<mpris::Mpris>>,
}

#[cfg(target_os = "linux")]
#[tauri::command(async)]
async fn now_playing(handle: tauri::AppHandle, bus: State<'_, NowPlayingBus>, np: Option<mpris::NowPlaying>) -> Res<()> {
    let m = bus
        .mpris
        .get_or_init(|| async {
            let on_key: mpris::OnKey = std::sync::Arc::new(move |k: &str| {
                let _ = handle.emit("media-key", k);
            });
            match mpris::Mpris::start(on_key).await {
                Ok(m) => Some(m),
                Err(e) => {
                    eprintln!("MPRIS unavailable ({e}); the Lyric Overlay won't see Selecta");
                    None
                }
            }
        })
        .await;
    if let Some(m) = m {
        m.update(np).await.map_err(err)?;
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
#[tauri::command(async)]
async fn now_playing(_np: Option<serde_json::Value>) -> Res<()> {
    Ok(())
}

/// `SELECTA_AUTOPLAY="prompt"` (+ `SELECTA_AUTOPLAY_MIN`, default 5): rescan,
/// plan that set by rules and play it, logging every event, then exit. With
/// the output routed to a null sink and recorded, it's an end-to-end listening
/// test of the real app (webview, GStreamer, everything).
#[tauri::command(async)]
async fn autoplay_config() -> Res<Option<(String, f64, bool)>> {
    Ok(std::env::var("SELECTA_AUTOPLAY").ok().map(|p| {
        let min = std::env::var("SELECTA_AUTOPLAY_MIN").ok().and_then(|m| m.parse().ok()).unwrap_or(5.0);
        // SELECTA_AUTOPLAY_AI=1: the AI builds the set (uses the user's Claude quota).
        (p, min, std::env::var_os("SELECTA_AUTOPLAY_AI").is_some())
    }))
}

/// `SELECTA_SELFTEST=1`: the UI runs the audio self-test in this webview and reports back.
#[tauri::command(async)]
async fn self_test_mode() -> Res<bool> {
    Ok(std::env::var_os("SELECTA_SELFTEST").is_some())
}

#[tauri::command(async)]
async fn self_test_log(line: String) -> Res<()> {
    println!("{line}");
    Ok(())
}

#[tauri::command(async)]
async fn self_test_report(handle: tauri::AppHandle, lines: Vec<String>, ok: bool) -> Res<()> {
    for l in &lines {
        println!("{l}");
    }
    println!("{}", if ok { "SELF-TEST PASSED" } else { "SELF-TEST FAILED" });
    handle.exit(if ok { 0 } else { 1 });
    Ok(())
}

#[tauri::command(async)]
async fn sets_list(app: State<'_, App>) -> Res<Vec<SetSummary>> {
    let lib = app.lib.clone();
    blocking(move || lib.sets().map_err(err)).await
}

#[tauri::command(async)]
async fn set_get(app: State<'_, App>, id: String) -> Res<SetPlan> {
    let lib = app.lib.clone();
    blocking(move || lib.set(&id).map_err(err)?.ok_or_else(|| "set not found".to_string())).await
}

#[tauri::command(async)]
async fn set_delete(app: State<'_, App>, id: String) -> Res<()> {
    let lib = app.lib.clone();
    blocking(move || lib.delete_set(&id).map_err(err)).await
}

#[tauri::command(async)]
async fn jobs_progress(app: State<'_, App>) -> Res<Vec<GroupProgress>> {
    let mut groups: Vec<GroupProgress> = app.jobs.progress().into_iter().filter(|g| g.group != "tagging").collect();
    let total = app.tagging.total.load(Ordering::Relaxed);
    if total > 0 {
        groups.push(GroupProgress { group: "tagging".into(), done: app.tagging.done.load(Ordering::Relaxed), total });
    }
    groups.retain(|g| g.group != "stems");
    let st = app.stems.total.load(Ordering::Relaxed);
    if st > 0 {
        groups.push(GroupProgress { group: "stems".into(), done: app.stems.done.load(Ordering::Relaxed), total: st });
    }
    Ok(groups)
}

/// The webview may only read audio from folders the user chose.
fn allow_dirs(handle: &tauri::AppHandle, dirs: &[String]) {
    let scope = handle.asset_protocol_scope();
    for d in dirs {
        let _ = scope.allow_directory(d, true);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            let conf = app.path().app_config_dir()?;
            std::fs::create_dir_all(&conf)?;
            let settings_path = conf.join("settings.json");
            let settings: Settings =
                std::fs::read(&settings_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| Settings {
                    music_dirs: app.path().audio_dir().ok().map(|d| vec![d.to_string_lossy().to_string()]).unwrap_or_default(),
                    ai: AiConfig::default(),
                    stems: true,
                    keylock: true,
                    output_delay_ms: None,
                });
            allow_dirs(app.handle(), &settings.music_dirs);
            let lib = Arc::new(Library::open(&data.join("library.db")).map_err(|e| e.to_string())?);
            let stem_paths = StemPaths::new(&data);
            let _ = std::fs::create_dir_all(&stem_paths.out);
            let _ = app.asset_protocol_scope().allow_directory(&stem_paths.out, false);
            let workers = std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).max(1)).unwrap_or(2);
            app.manage(NowPlayingBus::default());
            app.manage(App {
                tagging: Arc::new(TagProgress::default()),
                lib,
                jobs: Scheduler::new(workers),
                settings: Mutex::new(settings),
                settings_path,
                stems: StemQueue::new(),
                stem_paths,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            settings_get,
            settings_set,
            library_tracks,
            library_scan,
            track_analysis,
            jobs_progress,
            plan_set,
            sets_list,
            set_get,
            set_delete,
            library_tag,
            ai_status,
            ai_test,
            plan_splice,
            self_test_mode,
            self_test_report,
            self_test_log,
            autoplay_config,
            audio_output,
            now_playing,
            stems_status,
            stems_install,
            stems_prioritise,
            track_vocals
        ])
        .run(tauri::generate_context!())
        .expect("error while running Selecta");
}
