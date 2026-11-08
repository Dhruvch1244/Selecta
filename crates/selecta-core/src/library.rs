//! The track library: folder scan, tag reading, and one SQLite file.
//!
//! A track's id is FNV-1a over its canonical path (hand-rolled, pillar 7), so
//! it is stable across rescans. `mtime` decides whether analysis is stale.
//! Analysis is stored as one JSON column: its shape belongs to `analysis.rs`
//! and evolves with it, and `ANALYSIS_VERSION` triggers re-analysis when it does.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use lofty::prelude::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use ts_rs::TS;

use crate::analysis::{Analysis, ANALYSIS_VERSION};
use crate::genres;
use crate::llm::{TagRow, TrackTags};
use crate::planner::{Candidate, SetPlan};
use crate::Result;

const AUDIO_EXTS: [&str; 10] = ["mp3", "flac", "m4a", "aac", "ogg", "opus", "wav", "aiff", "aif", "alac"];

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TrackSummary {
    pub id: String,
    pub path: String,
    pub artist: String,
    pub title: String,
    pub album: String,
    pub genre_tag: String,
    pub duration: f64,
    pub analyzed: bool,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub camelot: Option<String>,
    /// AI-tagged mood (party, sad, ...), when the library has been tagged.
    pub mood: Option<String>,
    /// Why analysis failed, if it did (shown in the browser; retried when the file changes).
    pub error: Option<String>,
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SetSummary {
    pub id: String,
    pub created: f64,
    pub title: String,
    pub prompt: String,
    pub tracks: u32,
}

#[derive(Serialize, TS, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScanReport {
    pub found: u32,
    pub added: u32,
    pub updated: u32,
    pub removed: u32,
}

/// (track id, file path, BPM range hint for octave folding).
pub type PendingAnalysis = (String, PathBuf, Option<(f64, f64)>);

pub fn track_id(path: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in path.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

pub struct Library {
    conn: Mutex<Connection>,
}

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS tracks (
               id TEXT PRIMARY KEY, path TEXT UNIQUE NOT NULL, mtime INTEGER NOT NULL,
               artist TEXT NOT NULL DEFAULT '', title TEXT NOT NULL DEFAULT '', album TEXT NOT NULL DEFAULT '',
               genre_tag TEXT NOT NULL DEFAULT '', duration REAL NOT NULL DEFAULT 0,
               analysis TEXT, analysis_mtime INTEGER NOT NULL DEFAULT 0, analysis_version INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS sets (
               id TEXT PRIMARY KEY, created REAL NOT NULL, title TEXT NOT NULL, prompt TEXT NOT NULL, plan TEXT NOT NULL
             );",
        )?;
        // Migrations for libraries created by earlier versions: add columns, never drop data.
        let cols: Vec<String> =
            conn.prepare("PRAGMA table_info(tracks)")?.query_map([], |r| r.get::<_, String>(1))?.collect::<std::result::Result<_, _>>()?;
        if !cols.iter().any(|c| c == "llm_tags") {
            conn.execute("ALTER TABLE tracks ADD COLUMN llm_tags TEXT", [])?;
        }
        if !cols.iter().any(|c| c == "analysis_error") {
            conn.execute("ALTER TABLE tracks ADD COLUMN analysis_error TEXT", [])?;
        }
        Ok(Library { conn: Mutex::new(conn) })
    }

    pub fn in_memory() -> Result<Self> {
        Self::open(Path::new(":memory:"))
    }

    pub fn scan(&self, dirs: &[PathBuf]) -> Result<ScanReport> {
        let mut files = vec![];
        for d in dirs {
            walk(d, &mut files);
        }
        let mut rep = ScanReport { found: files.len() as u32, ..Default::default() };
        let conn = self.conn.lock().unwrap();
        let mut known: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
        {
            let mut st = conn.prepare("SELECT path, mtime FROM tracks")?;
            for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (p, m) = row?;
                known.insert(p, m);
            }
        }
        let mut seen = std::collections::HashSet::new();
        for f in &files {
            let path = f.to_string_lossy().to_string();
            seen.insert(path.clone());
            let mtime = std::fs::metadata(f)
                .and_then(|m| m.modified())
                .map(|t| t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
                .unwrap_or(0);
            if known.get(&path) == Some(&mtime) {
                continue;
            }
            let t = read_tags(f);
            conn.execute(
                "INSERT INTO tracks (id, path, mtime, artist, title, album, genre_tag, duration) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
                 ON CONFLICT(path) DO UPDATE SET mtime=?3, artist=?4, title=?5, album=?6, genre_tag=?7, duration=?8",
                params![track_id(&path), path, mtime, t.0, t.1, t.2, t.3, t.4],
            )?;
            if known.contains_key(&path) {
                rep.updated += 1;
            } else {
                rep.added += 1;
            }
        }
        // Only forget files under the roots we just scanned (an unplugged drive
        // elsewhere shouldn't wipe its tracks).
        let roots: Vec<String> = dirs.iter().filter(|d| d.exists()).map(|d| d.to_string_lossy().to_string()).collect();
        for p in known.keys() {
            if !seen.contains(p) && roots.iter().any(|r| p.starts_with(r)) {
                conn.execute("DELETE FROM tracks WHERE path=?1", [p])?;
                rep.removed += 1;
            }
        }
        Ok(rep)
    }

    pub fn tracks(&self) -> Result<Vec<TrackSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT id, path, artist, title, album, genre_tag, duration, analysis, analysis_mtime >= mtime AND analysis_version = ?1,
                    json_extract(llm_tags, '$.mood'),
                    CASE WHEN analysis_mtime >= mtime THEN analysis_error END
             FROM tracks ORDER BY artist COLLATE NOCASE, title COLLATE NOCASE",
        )?;
        let rows = st.query_map([ANALYSIS_VERSION], |r| {
            let a: Option<String> = r.get(7)?;
            let parsed = a.as_deref().and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
            Ok(TrackSummary {
                id: r.get(0)?,
                path: r.get(1)?,
                artist: r.get(2)?,
                title: r.get(3)?,
                album: r.get(4)?,
                genre_tag: r.get(5)?,
                duration: r.get(6)?,
                analyzed: r.get::<_, bool>(8)? && parsed.is_some(),
                bpm: parsed.as_ref().and_then(|v| v["bpm"].as_f64()),
                key: parsed.as_ref().and_then(|v| v["key"].as_str().map(String::from)),
                camelot: parsed.as_ref().and_then(|v| v["camelot"].as_str().map(String::from)),
                mood: r.get::<_, Option<String>>(9)?.filter(|m| !m.is_empty()),
                error: r.get(10)?,
            })
        })?;
        rows.collect::<std::result::Result<_, _>>().map_err(Into::into)
    }

    /// Tracks whose analysis is missing or stale, with a BPM range to fold
    /// octave errors into when tags/artist/folder name a beatmatched genre.
    pub fn needs_analysis(&self) -> Result<Vec<PendingAnalysis>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT id, path, genre_tag, artist FROM tracks
             WHERE (analysis IS NULL OR analysis_mtime < mtime OR analysis_version != ?1)
               AND NOT (analysis_error IS NOT NULL AND analysis_mtime >= mtime)",
        )?;
        let rows = st.query_map([ANALYSIS_VERSION], |r| {
            let (path, tag, artist): (String, String, String) = (r.get(1)?, r.get(2)?, r.get(3)?);
            Ok((r.get::<_, String>(0)?, PathBuf::from(&path), genres::bpm_hint(&tag, &artist, &path)))
        })?;
        rows.collect::<std::result::Result<_, _>>().map_err(Into::into)
    }

    /// Every analysed track with its genre scores, ready for the planner.
    /// Genres are classified on read (cheap string work), so they always
    /// reflect the current profiles without a migration.
    pub fn candidates(&self) -> Result<Vec<Candidate>> {
        let analysed: Vec<TrackSummary> = self.tracks()?.into_iter().filter(|t| t.analyzed).collect();
        let mut out = Vec::with_capacity(analysed.len());
        for t in analysed {
            if let Some(a) = self.analysis(&t.id)? {
                let tags = self.tags(&t.id)?;
                let genres = genres::classify_with(&t.genre_tag, &t.artist, &t.path, Some(a.bpm), tags.as_ref());
                out.push(Candidate { track: t, analysis: a, genres, tags });
            }
        }
        Ok(out)
    }

    /// Remember a failed analysis so it isn't retried every scan, and so the UI
    /// can say why. Changing the file (new mtime) makes it eligible again.
    pub fn save_analysis_error(&self, id: &str, msg: &str) -> Result<()> {
        self.conn.lock().unwrap().execute("UPDATE tracks SET analysis_error=?1, analysis_mtime=mtime WHERE id=?2", params![msg, id])?;
        Ok(())
    }

    pub fn tags(&self, id: &str) -> Result<Option<TrackTags>> {
        let conn = self.conn.lock().unwrap();
        let s: Option<Option<String>> = conn.query_row("SELECT llm_tags FROM tracks WHERE id=?1", [id], |r| r.get(0)).optional()?;
        Ok(s.flatten().and_then(|j| serde_json::from_str(&j).ok()))
    }

    pub fn save_tags(&self, id: &str, t: &TrackTags) -> Result<()> {
        self.conn.lock().unwrap().execute("UPDATE tracks SET llm_tags=?1 WHERE id=?2", params![serde_json::to_string(t)?, id])?;
        Ok(())
    }

    /// Tracks the AI hasn't tagged yet: (id, artist, title, album, genre tag, bpm).
    pub fn untagged(&self) -> Result<Vec<TagRow>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn.prepare(
            "SELECT id, artist, title, album, genre_tag, json_extract(analysis, '$.bpm') FROM tracks WHERE llm_tags IS NULL ORDER BY artist, title",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
        rows.collect::<std::result::Result<_, _>>().map_err(Into::into)
    }

    pub fn save_set(&self, p: &SetPlan) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO sets (id, created, title, prompt, plan) VALUES (?1,?2,?3,?4,?5)",
            params![p.id, p.created, p.title, p.prompt, serde_json::to_string(p)?],
        )?;
        Ok(())
    }

    pub fn sets(&self) -> Result<Vec<SetSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn
            .prepare("SELECT id, created, title, prompt, json_array_length(plan, '$.tracks') FROM sets ORDER BY created DESC LIMIT 200")?;
        let rows = st.query_map([], |r| {
            Ok(SetSummary { id: r.get(0)?, created: r.get(1)?, title: r.get(2)?, prompt: r.get(3)?, tracks: r.get(4)? })
        })?;
        rows.collect::<std::result::Result<_, _>>().map_err(Into::into)
    }

    pub fn set(&self, id: &str) -> Result<Option<SetPlan>> {
        let conn = self.conn.lock().unwrap();
        let s: Option<String> = conn.query_row("SELECT plan FROM sets WHERE id=?1", [id], |r| r.get(0)).optional()?;
        Ok(match s {
            Some(j) => Some(serde_json::from_str(&j)?),
            None => None,
        })
    }

    pub fn delete_set(&self, id: &str) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM sets WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn save_analysis(&self, id: &str, a: &Analysis) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tracks SET analysis=?1, analysis_mtime=mtime, analysis_version=?2, duration=?3, analysis_error=NULL WHERE id=?4",
            params![serde_json::to_string(a)?, ANALYSIS_VERSION, a.duration, id],
        )?;
        Ok(())
    }

    pub fn analysis(&self, id: &str) -> Result<Option<Analysis>> {
        let conn = self.conn.lock().unwrap();
        let s: Option<Option<String>> = conn.query_row("SELECT analysis FROM tracks WHERE id=?1", [id], |r| r.get(0)).optional()?;
        Ok(match s.flatten() {
            Some(j) => Some(serde_json::from_str(&j)?),
            None => None,
        })
    }

    pub fn path_of(&self, id: &str) -> Result<Option<PathBuf>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("SELECT path FROM tracks WHERE id=?1", [id], |r| r.get::<_, String>(0)).optional()?.map(PathBuf::from))
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if !e.file_name().to_string_lossy().starts_with('.') {
                walk(&p, out);
            }
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| AUDIO_EXTS.contains(&x.to_ascii_lowercase().as_str())) {
            out.push(p.canonicalize().unwrap_or(p));
        }
    }
}

/// (artist, title, album, genre, duration). Untagged files fall back to the
/// common "Artist - Title.ext" name, and the parent folder doubles as a genre
/// hint ("Music/DnB/...").
fn read_tags(path: &Path) -> (String, String, String, String, f64) {
    let (mut artist, mut title, mut album, mut genre, mut dur) = (String::new(), String::new(), String::new(), String::new(), 0.0);
    if let Ok(tf) = lofty::read_from_path(path) {
        dur = tf.properties().duration().as_secs_f64();
        if let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) {
            artist = tag.artist().map(|s| s.to_string()).unwrap_or_default();
            title = tag.title().map(|s| s.to_string()).unwrap_or_default();
            album = tag.album().map(|s| s.to_string()).unwrap_or_default();
            genre = tag.genre().map(|s| s.to_string()).unwrap_or_default();
        }
    }
    if title.is_empty() {
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        match stem.split_once(" - ") {
            Some((a, t)) => {
                if artist.is_empty() {
                    artist = a.trim().to_string();
                }
                title = t.trim().to_string();
            }
            None => title = stem,
        }
    }
    if genre.is_empty() {
        if let Some(folder) = path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()) {
            if !matches!(folder.to_lowercase().as_str(), "music" | "downloads" | "desktop") {
                genre = folder;
            }
        }
    }
    (artist, title, album, genre, dur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_round_trip() {
        let lib = Library::in_memory().unwrap();
        let it = crate::intent::parse("dnb", &[]);
        let plan = SetPlan {
            id: "abc".into(),
            created: 1.0,
            title: "T".into(),
            prompt: "dnb".into(),
            intent: it,
            tracks: vec![],
            est_total_sec: 0.0,
            arc: vec![],
            notes: vec![],
            curated: false,
        };
        lib.save_set(&plan).unwrap();
        assert_eq!(lib.sets().unwrap()[0].id, "abc");
        assert_eq!(lib.set("abc").unwrap().unwrap(), plan);
        lib.delete_set("abc").unwrap();
        assert!(lib.sets().unwrap().is_empty());
    }

    #[test]
    fn ids_are_stable_and_distinct() {
        assert_eq!(track_id("/a/b.mp3"), track_id("/a/b.mp3"));
        assert_ne!(track_id("/a/b.mp3"), track_id("/a/c.mp3"));
        assert_eq!(track_id("").len(), 16);
    }

    #[test]
    fn scan_reads_names_and_folder_genre() {
        let dir = std::env::temp_dir().join(format!("selecta-scan-{}", std::process::id()));
        let sub = dir.join("DnB");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("Pendulum - Watercolour.wav"), b"not really audio").unwrap();
        let lib = Library::in_memory().unwrap();
        let rep = lib.scan(std::slice::from_ref(&dir)).unwrap();
        assert_eq!((rep.found, rep.added), (1, 1));
        let t = &lib.tracks().unwrap()[0];
        assert_eq!((t.artist.as_str(), t.title.as_str(), t.genre_tag.as_str()), ("Pendulum", "Watercolour", "DnB"));
        assert_eq!(lib.scan(std::slice::from_ref(&dir)).unwrap().added, 0, "unchanged files are skipped");
        let id = t.id.clone();
        assert_eq!(lib.untagged().unwrap().len(), 1);
        let tags = TrackTags { genres: [("dnb".to_string(), 1.0)].into_iter().collect(), mood: "uplifting".into(), lang: "en".into() };
        lib.save_tags(&id, &tags).unwrap();
        assert_eq!(lib.tags(&id).unwrap(), Some(tags));
        assert!(lib.untagged().unwrap().is_empty());
        assert_eq!(lib.tracks().unwrap()[0].mood.as_deref(), Some("uplifting"));
        assert_eq!(lib.needs_analysis().unwrap().len(), 1);
        lib.save_analysis_error(&id, "no audio decoded").unwrap();
        assert_eq!(lib.tracks().unwrap()[0].error.as_deref(), Some("no audio decoded"));
        assert!(lib.needs_analysis().unwrap().is_empty(), "a failed file isn't retried until it changes");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
