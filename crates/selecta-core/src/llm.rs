//! The AI layer: the user's own signed-in Claude Code CLI, run non-interactively.
//!
//! `claude -p` reads the prompt on stdin and prints the reply. It runs in an
//! empty temp directory (so no project CLAUDE.md or files leak into the
//! prompt) with every tool disallowed (we only want text back) and a timeout.
//!
//! The AI is *optional everywhere*: every caller gets `Err` on any failure
//! (not installed, signed out, timeout, unparseable reply) and the rules carry
//! on. Results are data the planner weighs, never commands it obeys: an
//! explicit "DnB" in the prompt stays DnB whatever a model replies.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::genres;
use crate::intent::{Energy, Intent};
use crate::planner::Candidate;

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AiConfig {
    /// "claude" or "off".
    pub provider: String,
    /// Model alias passed to `claude --model`: sonnet | haiku | opus.
    pub model: String,
    /// Executable to run; "claude" resolves on PATH. Tests point this at a fake.
    pub binary: String,
    pub curate: bool,
    pub tag_library: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig { provider: "claude".into(), model: "sonnet".into(), binary: "claude".into(), curate: true, tag_library: true }
    }
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AiStatus {
    pub available: bool,
    pub provider: String,
    pub model: String,
    pub detail: String,
}

const DISALLOWED: &str = "Bash,Edit,Write,Read,WebFetch,WebSearch,Glob,Grep,Task,NotebookEdit";

fn on_path(bin: &str) -> bool {
    if bin.contains('/') {
        return std::path::Path::new(bin).is_file();
    }
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

pub fn status(cfg: &AiConfig) -> AiStatus {
    let available = cfg.provider == "claude" && on_path(&cfg.binary);
    let detail = if cfg.provider != "claude" {
        "AI is off; sets are planned by rules only.".into()
    } else if available {
        format!("Using your Claude CLI ({}).", cfg.model)
    } else {
        format!("`{}` was not found on PATH. Install Claude Code, or turn AI off.", cfg.binary)
    };
    AiStatus { available, provider: cfg.provider.clone(), model: cfg.model.clone(), detail }
}

/// One prompt in, the model's text out.
pub fn ask(cfg: &AiConfig, prompt: &str, timeout: Duration) -> Result<String, String> {
    if cfg.provider != "claude" {
        return Err("AI is off".into());
    }
    let dir = std::env::temp_dir().join(format!("selecta-ai-{}-{}", std::process::id(), Instant::now().elapsed().as_nanos()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut child = Command::new(&cfg.binary)
        .args(["-p", "--model", &cfg.model, "--output-format", "text", "--disallowedTools", DISALLOWED])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't start {}: {e}", cfg.binary))?;
    child.stdin.take().expect("piped stdin").write_all(prompt.as_bytes()).map_err(|e| e.to_string())?;
    let started = Instant::now();
    let out = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => break child.wait_with_output().map_err(|e| e.to_string())?,
            None if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = std::fs::remove_dir_all(&dir);
                return Err(format!("no answer within {}s", timeout.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let _ = std::fs::remove_dir_all(&dir);
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().last().unwrap_or("claude exited with an error").chars().take(300).collect());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The first JSON object or array in a reply, tolerating code fences and chatter.
pub fn parse_json(text: &str) -> Option<serde_json::Value> {
    for (open, close) in [('{', '}'), ('[', ']')] {
        if let (Some(a), Some(b)) = (text.find(open), text.rfind(close)) {
            if b > a {
                if let Ok(v) = serde_json::from_str(&text[a..=b]) {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn ask_json(cfg: &AiConfig, prompt: &str, timeout: Duration) -> Result<serde_json::Value, String> {
    let reply = ask(cfg, prompt, timeout)?;
    parse_json(&reply).ok_or_else(|| "the reply wasn't JSON".to_string())
}

// ---------- intent ----------

/// Fill in what the rules couldn't infer (vibe, title, genres for a vague
/// prompt), never overriding what the prompt said explicitly.
pub fn refine_intent(cfg: &AiConfig, it: &mut Intent) -> Result<(), String> {
    let ids: Vec<&str> = genres::PROFILES.iter().map(|p| p.id).collect();
    let prompt = format!(
        "You are a DJ's assistant. Turn this request into JSON for planning a DJ set.\n\
         Allowed genre ids: {}.\n\
         Return ONLY JSON: {{\"genres\": {{\"<id>\": 0..1}}, \"energy\": \"chill|normal|party|crazy\", \"title\": \"short catchy set name\", \"vibe\": \"one sentence\"}}\n\
         Request: {}",
        ids.join(", "),
        it.prompt
    );
    let v = ask_json(cfg, &prompt, Duration::from_secs(90))?;
    if it.genres.is_empty() {
        if let Some(g) = v["genres"].as_object() {
            for (k, w) in g {
                if let (Some(p), Some(w)) = (genres::profile(k), w.as_f64()) {
                    if w > 0.0 {
                        it.genres.insert(p.id.into(), w.min(1.0));
                    }
                }
            }
        }
    }
    if it.energy == Energy::Normal {
        let (e, chaos) = match v["energy"].as_str() {
            Some("crazy") => (Energy::Crazy, 0.9),
            Some("party") => (Energy::Party, 0.65),
            Some("chill") => (Energy::Chill, 0.18),
            _ => (Energy::Normal, it.chaos),
        };
        it.energy = e;
        it.chaos = chaos;
    }
    if let Some(t) = v["title"].as_str().map(str::trim).filter(|t| (3..48).contains(&t.len())) {
        it.title = t.trim_matches('"').to_string();
    }
    if let Some(vibe) = v["vibe"].as_str() {
        it.vibe = Some(vibe.chars().take(200).collect());
    }
    Ok(())
}

// ---------- curation ----------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curation {
    /// Track id -> 0..10 fit for this request and crowd.
    pub scores: BTreeMap<String, f64>,
    /// Track id -> opener | builder | peak | closer.
    pub roles: BTreeMap<String, String>,
    /// Track ids that clearly don't fit (wrong mood or genre).
    pub avoid: Vec<String>,
    /// Suggested running order (track ids with score >= 5, first = opener).
    pub order: Vec<String>,
    pub title: Option<String>,
    pub vibe: Option<String>,
    /// Human-readable DJ ideas ("Badtameez Dil: let the crowd sing the hook").
    pub moments: Vec<String>,
}

pub fn curation_prompt(it: &Intent, wide: &[&Candidate], need: usize) -> String {
    let rows: Vec<String> = wide
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let g = c.genres.iter().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).map(|(g, _)| g.as_str()).unwrap_or("?");
            let mood = c.tags.as_ref().map(|t| t.mood.as_str()).unwrap_or("");
            format!(
                "{} | {} | {} | {:.0} | {} | {} | {}",
                i + 1,
                c.track.artist,
                c.track.title,
                c.analysis.bpm,
                c.analysis.camelot,
                g,
                mood
            )
        })
        .collect();
    format!(
        "You are a world-class DJ who knows Bollywood, Punjabi, desi hip hop, drum & bass, house, hip hop and pop deeply.\n\
         Curate a DJ set from MY library for this request: \"{prompt}\"\n\
         Parsed so far: genres={genres}; energy={energy:?}; about {mins:.0} minutes, so roughly {need} tracks.\n\n\
         Use your knowledge of the actual songs: which are bangers vs ballads, which a crowd sings along to, which fit this vibe.\n\
         List \"picks\" in your suggested RUNNING ORDER (first = opener). Score each plausible track 0-10 (10 = perfect for this\n\
         request and crowd) and give it a role. Put tracks that clearly don't fit (e.g. a sad ballad at a party, wrong genre) in \"avoid\".\n\
         Optionally suggest up to 5 DJ moments.\n\n\
         Return ONLY JSON:\n\
         {{\"title\": \"catchy set name\", \"vibe\": \"one line\", \"picks\": [{{\"n\": 12, \"score\": 9, \"role\": \"opener|builder|peak|closer\"}}],\n\
          \"avoid\": [3, 7], \"moments\": [{{\"n\": 12, \"idea\": \"...\"}}]}}\n\n\
         My library (n | artist | title | bpm | key | detected genre | mood):\n{rows}",
        prompt = it.prompt,
        genres = if it.genres.is_empty() { "open".to_string() } else { it.genres.keys().cloned().collect::<Vec<_>>().join(",") },
        energy = it.energy,
        mins = it.duration_min,
        rows = rows.join("\n"),
    )
}

/// Map a curation reply (1-based row numbers) back onto track ids.
pub fn parse_curation(v: &serde_json::Value, wide: &[&Candidate]) -> Curation {
    let id_of = |n: &serde_json::Value| n.as_u64().and_then(|n| wide.get((n as usize).checked_sub(1)?)).map(|c| c.track.id.clone());
    let mut cur = Curation {
        title: v["title"].as_str().map(|s| s.trim().to_string()).filter(|s| (3..48).contains(&s.len())),
        vibe: v["vibe"].as_str().map(|s| s.chars().take(200).collect()),
        ..Default::default()
    };
    for p in v["picks"].as_array().into_iter().flatten() {
        let Some(id) = id_of(&p["n"]) else { continue };
        let score = p["score"].as_f64().unwrap_or(5.0).clamp(0.0, 10.0);
        if let Some(r) = p["role"].as_str().filter(|r| matches!(*r, "opener" | "builder" | "peak" | "closer")) {
            cur.roles.insert(id.clone(), r.into());
        }
        if score >= 5.0 && !cur.order.contains(&id) {
            cur.order.push(id.clone());
        }
        cur.scores.insert(id, score);
    }
    cur.avoid = v["avoid"].as_array().into_iter().flatten().filter_map(id_of).collect();
    for m in v["moments"].as_array().into_iter().flatten().take(5) {
        if let (Some(id), Some(idea)) = (id_of(&m["n"]), m["idea"].as_str()) {
            let title = wide.iter().find(|c| c.track.id == id).map(|c| c.track.title.as_str()).unwrap_or("");
            cur.moments.push(format!("{title}: {}", idea.chars().take(140).collect::<String>()));
        }
    }
    cur
}

pub fn curate(cfg: &AiConfig, it: &Intent, wide: &[&Candidate], need: usize) -> Result<Curation, String> {
    let v = ask_json(cfg, &curation_prompt(it, wide, need), Duration::from_secs(300))?;
    Ok(parse_curation(&v, wide))
}

// ---------- library tagging ----------

#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TrackTags {
    /// Genre id -> 0..1 (only known profile ids are kept).
    pub genres: BTreeMap<String, f64>,
    /// party | hype | chill | romantic | sad | dark | uplifting
    pub mood: String,
    /// ISO-ish language code ("hi", "pa", "en") or "inst".
    pub lang: String,
}

/// A track to tag: (id, artist, title, album, genre tag, bpm).
pub type TagRow = (String, String, String, String, String, Option<f64>);

pub const MOODS: [&str; 7] = ["party", "hype", "chill", "romantic", "sad", "dark", "uplifting"];

/// Tag up to ~60 tracks in one call: (id, artist, title, album, genre tag, bpm).
pub fn tag_tracks(cfg: &AiConfig, rows: &[TagRow]) -> Result<BTreeMap<String, TrackTags>, String> {
    let ids: Vec<&str> = genres::PROFILES.iter().map(|p| p.id).collect();
    let list: Vec<String> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            format!("{} | {} | {} | {} | {} | {}", i + 1, r.1, r.2, r.3, r.4, r.5.map(|b| format!("{b:.0}")).unwrap_or("?".into()))
        })
        .collect();
    let prompt = format!(
        "You are a music librarian with deep knowledge of Indian (Bollywood, Punjabi, desi hip hop) and global music.\n\
         For each track, give genre weights using ONLY these ids: {}.\n\
         Also give mood (one of: {}) and main lyric language (ISO code like hi, pa, en, ur; \"inst\" for instrumental).\n\
         Use your knowledge of the real song/artist; if unknown, infer from artist, title, album and BPM.\n\
         Return ONLY a JSON object keyed by the track number: {{\"1\": {{\"genres\": {{\"<id>\": 0..1}}, \"mood\": \"...\", \"lang\": \"...\"}}}}\n\n\
         Tracks (number | artist | title | album | file tag | bpm):\n{}",
        ids.join(", "),
        MOODS.join(", "),
        list.join("\n")
    );
    let v = ask_json(cfg, &prompt, Duration::from_secs(240))?;
    let mut out = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        let t = &v[(i + 1).to_string()];
        if !t.is_object() {
            continue;
        }
        let genres = t["genres"]
            .as_object()
            .map(|g| g.iter().filter_map(|(k, w)| Some((genres::profile(k)?.id.to_string(), w.as_f64()?.clamp(0.0, 1.0)))).collect())
            .unwrap_or_default();
        let mood = t["mood"].as_str().filter(|m| MOODS.contains(m)).unwrap_or("").to_string();
        let lang = t["lang"].as_str().unwrap_or("").chars().take(8).collect();
        out.insert(r.0.clone(), TrackTags { genres, mood, lang });
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fake `claude` that ignores its prompt and prints `reply`.
    pub fn fake_claude(reply: &str) -> AiConfig {
        let dir = std::env::temp_dir().join(format!("selecta-fake-claude-{}-{}", std::process::id(), reply.len()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("claude");
        std::fs::write(&bin, format!("#!/bin/sh\ncat > /dev/null\ncat <<'EOF'\n{reply}\nEOF\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        AiConfig { binary: bin.to_string_lossy().into(), ..Default::default() }
    }

    #[test]
    fn parses_json_inside_chatter_and_fences() {
        assert_eq!(parse_json("Sure!\n```json\n{\"a\": 1}\n```").unwrap()["a"], 1);
        assert_eq!(parse_json("[1,2]").unwrap()[1], 2);
        assert!(parse_json("no json here").is_none());
    }

    #[test]
    #[cfg(unix)]
    fn runs_the_cli_and_survives_failures() {
        let cfg = fake_claude(r#"{"ok": true}"#);
        assert_eq!(parse_json(&ask(&cfg, "hi", Duration::from_secs(5)).unwrap()).unwrap()["ok"], true);
        let missing = AiConfig { binary: "/nonexistent/claude".into(), ..Default::default() };
        assert!(!status(&missing).available);
        assert!(ask(&missing, "hi", Duration::from_secs(5)).is_err());
        let off = AiConfig { provider: "off".into(), ..Default::default() };
        assert!(ask(&off, "hi", Duration::from_secs(5)).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn refine_fills_gaps_but_never_overrides_explicit_asks() {
        let cfg = fake_claude(r#"{"genres": {"house": 1, "nonsense": 1}, "energy": "chill", "title": "Rainy Drive", "vibe": "soft"}"#);
        let mut vague = crate::intent::parse("something for a rainy drive", &[]);
        refine_intent(&cfg, &mut vague).unwrap();
        assert!(vague.genres.contains_key("house") && !vague.genres.contains_key("nonsense"));
        assert_eq!(vague.title, "Rainy Drive");
        let mut explicit = crate::intent::parse("crazy dnb set", &[]);
        refine_intent(&cfg, &mut explicit).unwrap();
        assert_eq!(explicit.genres.keys().collect::<Vec<_>>(), vec!["dnb"], "explicit genre kept");
        assert_eq!(explicit.energy, Energy::Crazy, "explicit energy kept");
    }

    #[test]
    #[cfg(unix)]
    fn tags_keep_only_known_genres_and_moods() {
        let cfg = fake_claude(
            r#"{"1": {"genres": {"bollywood": 1, "made_up": 1}, "mood": "sad", "lang": "hi"}, "2": {"genres": {}, "mood": "angry", "lang": "en"}}"#,
        );
        let rows = vec![
            ("a".into(), "Arijit Singh".into(), "Channa Mereya".into(), String::new(), String::new(), Some(90.0)),
            ("b".into(), "X".into(), "Y".into(), String::new(), String::new(), None),
        ];
        let t = tag_tracks(&cfg, &rows).unwrap();
        assert_eq!(t["a"].mood, "sad");
        assert_eq!(t["a"].genres.keys().collect::<Vec<_>>(), vec!["bollywood"]);
        assert_eq!(t["b"].mood, "", "unknown moods are dropped, not guessed");
    }
}
