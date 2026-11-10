//! Prompt -> set intent, by rules. Instant and fully local; the AI layer (a
//! later PR) refines vague prompts *on top of* this and never overrides an
//! explicit ask: a prompt saying "DnB" stays DnB whatever a model guesses.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::genres::{self, has_phrase, pad};

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Energy {
    Chill,
    Normal,
    Party,
    Crazy,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Intent {
    pub prompt: String,
    /// Genre id -> weight 0..1.
    pub genres: BTreeMap<String, f64>,
    /// Normalised artist names the prompt asked for.
    pub artists: Vec<String>,
    pub energy: Energy,
    /// 0..1: how adventurous transitions are (and, later, how much FX).
    pub chaos: f64,
    /// Energy-arc shape: rave | party | journey | hype | chill.
    pub arc: String,
    pub duration_min: f64,
    pub duration_explicit: bool,
    pub title: String,
    /// One-line description, filled in by the AI layer when available.
    pub vibe: Option<String>,
    /// Quick-mix pacing: ~30-45 s per track, drop to drop, lots of double
    /// drops ("hedex style", "quick mix", "rapid fire").
    #[serde(default)]
    pub rapid: bool,
    /// 0..1 appetite for stem moves (acapella builds, bootlegs, mashups).
    #[serde(default)]
    pub mashups: f64,
    /// Long, seamless blends; cuts and turntable tricks kept to a minimum.
    #[serde(default)]
    pub smooth: bool,
    /// Turntable tricks wanted: rewinds, backspins, brakes, loop rolls.
    #[serde(default)]
    pub turntablism: bool,
}

const SYNONYMS: &[(&str, &[&str])] = &[
    ("dnb", &["dnb", "d&b", "drum and bass", "drum & bass", "drum n bass", "jungle", "neuro", "neurofunk", "liquid", "jump up"]),
    (
        "bollywood",
        &[
            "bollywood",
            "bolly",
            "hindi",
            "filmi",
            "shaadi",
            "wedding",
            "sangeet",
            "baraat",
            "mehendi",
            "dandiya",
            "garba",
            "desi party",
            "indian",
        ],
    ),
    ("punjabi", &["punjabi", "bhangra", "pind", "jatt"]),
    ("desi_hiphop", &["desi hip hop", "desi hiphop", "dhh", "indian hip hop", "hindi rap", "desi rap", "gully"]),
    ("hiphop", &["hip hop", "hiphop", "rap", "boom bap", "drill", "grime"]),
    ("trap", &["trap", "phonk", "rage"]),
    ("house", &["house", "tech house", "deep house", "garage", "disco"]),
    ("techno", &["techno", "warehouse", "berghain", "acid"]),
    ("trance", &["trance", "psytrance", "psy"]),
    ("edm", &["edm", "festival", "big room", "mainstage", "hardstyle"]),
    ("dubstep", &["dubstep", "riddim", "bass music", "brostep"]),
    ("pop", &["pop", "top 40", "hits", "radio"]),
    ("rnb", &["r&b", "rnb", "soul", "slow jams"]),
    ("lofi", &["lofi", "lo fi", "study", "chillhop", "ambient"]),
    ("afro", &["afrobeats", "amapiano", "afro", "reggaeton", "latin", "dancehall"]),
];
const CRAZY: &[&str] = &[
    "crazy",
    "insane",
    "mental",
    "hard",
    "heavy",
    "banger",
    "bangers",
    "rave",
    "wild",
    "savage",
    "filthy",
    "madness",
    "lit",
    "chaos",
    "psycho",
    "nuts",
    "aggressive",
    "hype",
    "turn up",
    "go off",
    "mosh",
    "peak",
];
const CHILL: &[&str] = &[
    "chill",
    "relax",
    "mellow",
    "soft",
    "calm",
    "study",
    "sunday",
    "late night",
    "sunset",
    "rainy",
    "sleep",
    "cozy",
    "smooth",
    "dinner",
    "background",
];
const PARTY: &[&str] = &[
    "friends",
    "party",
    "pregame",
    "pre game",
    "guests",
    "house party",
    "celebration",
    "birthday",
    "get together",
    "gathering",
    "drinks",
    "club",
    "night out",
];

/// Quick-mix pacing. Hedex is the reference: a tune rarely plays a minute
/// before the next one is dropped on top of it.
const RAPID: &[&str] = &[
    "hedex",
    "rapid",
    "rapid fire",
    "quick mix",
    "quick mixing",
    "quickfire",
    "quick fire",
    "fast mix",
    "fast mixing",
    "fast cuts",
    "quick cuts",
    "chop",
    "chopping",
    "double drops",
    "drop to drop",
    "no waiting",
];
const MASHUP: &[&str] = &[
    "mashup",
    "mashups",
    "mash up",
    "bootleg",
    "bootlegs",
    "acapella",
    "acapellas",
    "a cappella",
    "vocals over",
    "vocal over",
    "remix",
    "edits",
    "blend vocals",
    "stems",
];

fn duration(text: &str) -> Option<f64> {
    let toks: Vec<&str> = text.split_whitespace().collect();
    for w in toks.windows(2) {
        if let Ok(n) = w[0].parse::<f64>() {
            match w[1] {
                "h" | "hr" | "hrs" | "hour" | "hours" => return Some(n * 60.0),
                "m" | "min" | "mins" | "minute" | "minutes" => return Some(n),
                _ => {}
            }
        }
    }
    let any = |ws: &[&str]| ws.iter().any(|p| has_phrase(text, p));
    if any(&["full set", "long set", "all night", "marathon"]) {
        Some(60.0)
    } else if any(&["quick", "short", "mini"]) && !any(RAPID) {
        Some(15.0)
    } else {
        None
    }
}

pub fn parse(prompt: &str, library_artists: &[String]) -> Intent {
    let text = pad(prompt);
    let mut genres: BTreeMap<String, f64> = BTreeMap::new();
    for (id, words) in SYNONYMS {
        if words.iter().any(|w| has_phrase(&text, w)) {
            genres.insert((*id).into(), 1.0);
        }
    }
    // "desi hip hop" shouldn't also mean US hip hop at full weight.
    if genres.contains_key("desi_hiphop") {
        if let Some(w) = genres.get_mut("hiphop") {
            *w = 0.35;
        }
    }

    // Artists: the library's own names plus the knowledge base, longest first so
    // "seedhe maut" wins over a hypothetical "maut".
    let mut names: Vec<String> = library_artists.iter().flat_map(|a| genres::split_artists(a)).collect();
    names.extend(genres::PROFILES.iter().flat_map(|p| p.artists.iter().map(|a| genres::norm(a))));
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    names.dedup();
    let mut artists = vec![];
    for n in names {
        if n.len() >= 3 && has_phrase(&text, &n) && !artists.iter().any(|a: &String| a.contains(&n)) {
            for g in genres::genres_of_artist(&n) {
                let w = genres.entry(g.into()).or_insert(0.0);
                *w = w.max(0.9);
            }
            artists.push(n);
        }
    }

    let any = |ws: &[&str]| ws.iter().any(|p| has_phrase(&text, p));
    let energy = if any(CRAZY) {
        Energy::Crazy
    } else if any(CHILL) {
        Energy::Chill
    } else if any(PARTY) {
        Energy::Party
    } else {
        Energy::Normal
    };
    let rapid = any(RAPID);
    let mut chaos: f64 = match energy {
        Energy::Crazy => 0.9,
        Energy::Party => 0.65,
        Energy::Normal => 0.5,
        Energy::Chill => 0.18,
    };
    if rapid {
        chaos = chaos.max(0.75);
    }
    let mashups = if any(MASHUP) {
        0.9
    } else {
        match energy {
            Energy::Crazy => 0.55,
            Energy::Party => 0.5,
            Energy::Normal => 0.35,
            Energy::Chill => 0.15,
        }
    };
    let top = genres.iter().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).map(|(g, _)| g.clone());
    let arc = match energy {
        Energy::Crazy => "rave".to_string(),
        Energy::Chill => "chill".to_string(),
        _ => {
            let base = top.as_deref().and_then(genres::profile).map(|p| p.arc).unwrap_or("journey");
            if energy == Energy::Party && base != "rave" {
                "party".into()
            } else {
                base.into()
            }
        }
    };
    let dur = duration(&text);
    let mut it = Intent {
        prompt: prompt.trim().to_string(),
        genres,
        artists,
        energy,
        chaos,
        arc,
        duration_min: dur.unwrap_or(if energy == Energy::Crazy { 45.0 } else { 30.0 }),
        duration_explicit: dur.is_some(),
        title: String::new(),
        vibe: None,
        rapid,
        mashups,
        smooth: any(&["smooth", "seamless", "long blends", "no cuts"]),
        turntablism: any(&["scratch", "scratchy", "turntablism", "rewinds", "pull ups", "pull up", "backspins"]),
    };
    // A style reference ("hedex style") with none of that artist's tracks
    // in the library is about how to mix, not whose tunes to play.
    let lib: Vec<String> = library_artists.iter().flat_map(|a| genres::split_artists(a)).collect();
    if rapid {
        it.artists.retain(|a| lib.iter().any(|l| l == a || l.contains(a.as_str())));
    }
    it.title = title_for(&it, top.as_deref());
    it
}

fn title_for(it: &Intent, top: Option<&str>) -> String {
    let label = top.and_then(genres::profile).map(|p| p.label).unwrap_or("Selecta");
    if let Some(a) = it.artists.first() {
        let lead: String = a
            .split(' ')
            .map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ");
        return format!("{lead} Takeover");
    }
    if it.rapid {
        return format!("{label} Rapid Fire");
    }
    if it.mashups >= 0.9 {
        return format!("{label} Mashup Mix");
    }
    let word = match it.energy {
        Energy::Crazy => "Pressure",
        Energy::Party => "House Party",
        Energy::Chill => "After Hours",
        Energy::Normal => "Session",
    };
    format!("{label} {word}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crazy_dnb() {
        let it = parse("crazy DnB set", &[]);
        assert_eq!(it.genres.get("dnb"), Some(&1.0));
        assert_eq!(it.energy, Energy::Crazy);
        assert_eq!(it.arc, "rave");
        assert!(!it.duration_explicit);
    }

    #[test]
    fn friends_bollywood_with_length() {
        let it = parse("My friends are coming make a bollywood set, 1 hour", &[]);
        assert!(it.genres.contains_key("bollywood"));
        assert_eq!(it.energy, Energy::Party);
        assert_eq!(it.duration_min, 60.0);
        assert!(it.duration_explicit);
    }

    #[test]
    fn artist_pulls_in_genre() {
        let it = parse("I wanna hip hop Seedhe Maut set 45min", &[]);
        assert_eq!(it.artists, vec!["seedhe maut".to_string()]);
        assert!(it.genres.get("desi_hiphop").copied().unwrap_or(0.0) >= 0.9);
        assert_eq!(it.duration_min, 45.0);
        assert_eq!(it.title, "Seedhe Maut Takeover");
    }

    #[test]
    fn hedex_style_is_rapid_dnb_not_an_artist_filter() {
        let it = parse("hedex style double drops", &[]);
        assert!(it.rapid);
        assert!(it.genres.get("dnb").copied().unwrap_or(0.0) >= 0.9);
        assert!(it.artists.is_empty(), "{:?}", it.artists);
        assert_eq!(it.title, "Drum & Bass Rapid Fire");
        assert!(!it.duration_explicit);
        // ...but with Hedex tunes in the library, they're requested too.
        let it = parse("hedex set", &["Hedex".into()]);
        assert_eq!(it.artists, vec!["hedex".to_string()]);
    }

    #[test]
    fn quick_mix_is_pacing_not_a_short_set() {
        let it = parse("quick mix bollywood mashups for friends", &[]);
        assert!(it.rapid);
        assert_eq!(it.mashups, 0.9);
        assert!(!it.duration_explicit, "'quick mix' must not mean a 15 min set");
    }

    #[test]
    fn library_artists_are_recognised() {
        let it = parse("something by Obscure Local Band please", &["Obscure Local Band".into()]);
        assert_eq!(it.artists, vec!["obscure local band".to_string()]);
    }
}
