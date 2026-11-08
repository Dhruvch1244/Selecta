//! Desktop shell. Owns the window, settings file, and the bridge from UI
//! commands to `selecta-core`. No domain logic lives here.
//!
//! Every command doing I/O or DSP is `async` and runs its work on a blocking
//! thread (pillar 9): a plain `#[tauri::command] fn` runs on the main thread
//! and freezes the whole window for its duration. This bit the Lyric Overlay
//! for 40-85 s per local file before being fixed there.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use selecta_core::analysis::{analyze_pcm, Analysis};
use selecta_core::decode::{decode_mono, to_analysis_rate};
use selecta_core::intent;
use selecta_core::jobs::{GroupProgress, Priority, Scheduler};
use selecta_core::library::{Library, ScanReport, SetSummary, TrackSummary};
use selecta_core::llm::{self, AiConfig, AiStatus};
use selecta_core::planner::{self, SetPlan};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct Settings {
    music_dirs: Vec<String>,
    #[serde(default)]
    ai: AiConfig,
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
    blocking(move || {
        let rep = lib.scan(&dirs).map_err(err)?;
        if ai.tag_library && llm::status(&ai).available {
            queue_tagging(&lib, &jobs, &ai, &tagging);
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
            if let Some(c) = o.chaos {
                it.chaos = c.clamp(0.0, 1.0);
            }
            let mut ai_notes = vec![];
            let ai_on = llm::status(&ai).available;
            if ai_on {
                say(&format!("Asking Claude ({}) to read the prompt", ai.model));
                if let Err(e) = llm::refine_intent(&ai, &mut it) {
                    ai_notes.push(format!("AI couldn't read the prompt ({e}); used rules."));
                }
            }
            let cands = lib.candidates().map_err(err)?;
            let mut curation = None;
            if ai_on && ai.curate {
                let wide = planner::shortlist(&it, &cands, &[], 300);
                if !wide.is_empty() {
                    say(&format!("Claude is curating {} tracks: bangers, ballads, running order", wide.len()));
                    let need = ((it.duration_min * 60.0 / 110.0).round() as usize).max(3);
                    match llm::curate(&ai, &it, &wide, need) {
                        Ok(c) => {
                            say(&format!("Curator picked {} and vetoed {}", c.order.len(), c.avoid.len()));
                            curation = Some(c);
                        }
                        Err(e) => ai_notes.push(format!("AI curation unavailable ({e}); planned by rules.")),
                    }
                }
            }
            say("Ordering by key, tempo and energy arc");
            let seed = o.seed.unwrap_or_else(|| {
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
            });
            let mut plan = planner::plan(&it, &cands, &planner::Options { seed, exclude: vec![], curation })?;
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
                });
            allow_dirs(app.handle(), &settings.music_dirs);
            let lib = Arc::new(Library::open(&data.join("library.db")).map_err(|e| e.to_string())?);
            let workers = std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).max(1)).unwrap_or(2);
            app.manage(App {
                tagging: Arc::new(TagProgress::default()),
                lib,
                jobs: Scheduler::new(workers),
                settings: Mutex::new(settings),
                settings_path,
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
            ai_test
        ])
        .run(tauri::generate_context!())
        .expect("error while running Selecta");
}
