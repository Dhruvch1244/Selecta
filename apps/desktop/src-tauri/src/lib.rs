//! Desktop shell. Owns the window, settings file, and the bridge from UI
//! commands to `selecta-core`. No domain logic lives here.
//!
//! Every command doing I/O or DSP is `async` and runs its work on a blocking
//! thread (pillar 9): a plain `#[tauri::command] fn` runs on the main thread
//! and freezes the whole window for its duration. This bit the Lyric Overlay
//! for 40-85 s per local file before being fixed there.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use selecta_core::analysis::{analyze_pcm, Analysis};
use selecta_core::decode::{decode_mono, to_analysis_rate};
use selecta_core::jobs::{GroupProgress, Priority, Scheduler};
use selecta_core::library::{Library, ScanReport, TrackSummary};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct Settings {
    music_dirs: Vec<String>,
}

struct App {
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

fn analyse_file(path: &std::path::Path) -> Res<Analysis> {
    let pcm = to_analysis_rate(decode_mono(path).map_err(err)?);
    Ok(analyze_pcm(&pcm, None))
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
    blocking(move || {
        let rep = lib.scan(&dirs).map_err(err)?;
        for (id, path, _genre) in lib.needs_analysis().map_err(err)? {
            let lib = lib.clone();
            jobs.submit(&format!("analyse:{id}"), "analysis", Priority::Idle, move |cancel| {
                if cancel.is_cancelled() {
                    return;
                }
                if let Ok(a) = analyse_file(&path) {
                    let _ = lib.save_analysis(&id, &a);
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
        let a = analyse_file(&path)?;
        lib.save_analysis(&id, &a).map_err(err)?;
        Ok(a)
    })
    .await
}

#[tauri::command(async)]
async fn jobs_progress(app: State<'_, App>) -> Res<Vec<GroupProgress>> {
    Ok(app.jobs.progress())
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
                });
            allow_dirs(app.handle(), &settings.music_dirs);
            let lib = Arc::new(Library::open(&data.join("library.db")).map_err(|e| e.to_string())?);
            let workers = std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).max(1)).unwrap_or(2);
            app.manage(App { lib, jobs: Scheduler::new(workers), settings: Mutex::new(settings), settings_path });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![settings_get, settings_set, library_tracks, library_scan, track_analysis, jobs_progress])
        .run(tauri::generate_context!())
        .expect("error while running Selecta");
}
