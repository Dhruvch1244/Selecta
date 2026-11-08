//! Genre profiles: how to recognise a genre, and how a DJ plays it.
//!
//! Each profile is both a *classifier* (tag keywords, artists, BPM range) and
//! a *style guide* for the planner: how long a track stays on before the next
//! comes in, whether tracks are beatmatched or cut, which transitions fit.
//! Real DnB DJs blend 16-32 bars and double-drop; Bollywood/desi party sets
//! cut hook to hook; techno rides long blends.
//!
//! Matching is plain token search on normalised text (no regex crate,
//! pillar 7): text is lower-cased, non-alphanumerics become spaces, and a
//! phrase matches when " phrase " occurs in " text ".

use std::collections::BTreeMap;

pub struct Profile {
    pub id: &'static str,
    pub label: &'static str,
    pub bpm: (f64, f64),
    pub beatmatch: bool,
    /// Typical seconds a track plays before the next one takes over.
    pub play: (f64, f64),
    pub keywords: &'static [&'static str],
    pub artists: &'static [&'static str],
    pub related: &'static [&'static str],
    /// Transition id -> relative weight. Only ids the engine supports are used.
    pub transitions: &'static [(&'static str, f64)],
    pub arc: &'static str,
}

pub static PROFILES: &[Profile] = &[
    Profile {
        id: "dnb",
        label: "Drum & Bass",
        bpm: (160.0, 180.0),
        beatmatch: true,
        play: (75.0, 150.0),
        keywords: &["drum and bass", "drum & bass", "drum n bass", "dnb", "d&b", "jungle", "neurofunk", "liquid", "jump up", "liquid funk"],
        artists: &[
            "pendulum",
            "sub focus",
            "chase & status",
            "chase and status",
            "netsky",
            "wilkinson",
            "dimension",
            "andy c",
            "noisia",
            "culture shock",
            "hybrid minds",
            "camo & krooked",
            "friction",
            "delta heavy",
            "high contrast",
            "rudimental",
            "shy fx",
            "goldie",
            "london elektricity",
            "metrik",
            "sigma",
            "fred v",
            "grafix",
            "kanine",
            "bou",
            "hedex",
            "dub phizix",
            "calibre",
            "spectrasoul",
            "alix perez",
            "mefjus",
            "black sun empire",
            "the prototypes",
            "macky gee",
            "turno",
            "loadstar",
            "danny byrd",
            "dj fresh",
            "nu tone",
            "logistics",
            "kings of the rollers",
            "bensley",
            "koven",
            "maduk",
            "feint",
            "sully",
            "nia archives",
            "pola & bryson",
            "sammy virji",
            "mozey",
            "crissy criss",
        ],
        related: &["dubstep", "edm"],
        transitions: &[("blend", 3.0), ("bassswap", 4.0), ("double_drop", 3.0), ("echo_out", 1.0), ("filter", 2.0), ("slam", 1.0)],
        arc: "rave",
    },
    Profile {
        id: "dubstep",
        label: "Dubstep / Bass",
        bpm: (138.0, 152.0),
        beatmatch: true,
        play: (75.0, 140.0),
        keywords: &["dubstep", "riddim", "brostep", "bass music", "future bass"],
        artists: &[
            "skrillex",
            "excision",
            "subtronics",
            "virtual riot",
            "zeds dead",
            "rusko",
            "flux pavilion",
            "doctor p",
            "nero",
            "knife party",
            "svdden death",
            "datsik",
            "illenium",
            "seven lions",
            "porter robinson",
        ],
        related: &["dnb", "edm"],
        transitions: &[("bassswap", 3.0), ("double_drop", 3.0), ("slam", 2.0), ("echo_out", 1.0), ("filter", 2.0)],
        arc: "rave",
    },
    Profile {
        id: "house",
        label: "House / Tech House",
        bpm: (118.0, 130.0),
        beatmatch: true,
        play: (120.0, 220.0),
        keywords: &[
            "house",
            "tech house",
            "deep house",
            "progressive house",
            "garage",
            "uk garage",
            "disco",
            "nu disco",
            "afro house",
            "melodic house",
        ],
        artists: &[
            "fisher",
            "chris lake",
            "john summit",
            "dom dolla",
            "fred again",
            "disclosure",
            "calvin harris",
            "duke dumont",
            "purple disco machine",
            "black coffee",
            "peggy gou",
            "keinemusik",
            "kaytranada",
            "gorgon city",
            "camelphat",
            "solardo",
            "eli brown",
            "vintage culture",
            "meduza",
            "joel corry",
            "jax jones",
            "anyma",
        ],
        related: &["techno", "edm", "pop"],
        transitions: &[("blend", 4.0), ("bassswap", 4.0), ("filter", 3.0), ("echo_out", 1.0)],
        arc: "journey",
    },
    Profile {
        id: "techno",
        label: "Techno",
        bpm: (124.0, 150.0),
        beatmatch: true,
        play: (150.0, 260.0),
        keywords: &["techno", "hard techno", "industrial", "minimal", "acid", "hardgroove", "peak time"],
        artists: &[
            "charlotte de witte",
            "amelie lens",
            "adam beyer",
            "carl cox",
            "reinier zonneveld",
            "i hate models",
            "sara landry",
            "kobosil",
            "enrico sangiuliano",
            "boris brejcha",
            "nina kraviz",
            "richie hawtin",
            "jeff mills",
        ],
        related: &["house", "trance"],
        transitions: &[("blend", 5.0), ("bassswap", 4.0), ("filter", 3.0)],
        arc: "journey",
    },
    Profile {
        id: "trance",
        label: "Trance",
        bpm: (128.0, 142.0),
        beatmatch: true,
        play: (150.0, 240.0),
        keywords: &["trance", "psytrance", "psy", "uplifting", "goa"],
        artists: &[
            "armin van buuren",
            "above & beyond",
            "tiesto",
            "ferry corsten",
            "paul van dyk",
            "infected mushroom",
            "vini vici",
            "astrix",
        ],
        related: &["techno", "edm"],
        transitions: &[("blend", 4.0), ("bassswap", 3.0), ("filter", 3.0), ("echo_out", 1.0)],
        arc: "journey",
    },
    Profile {
        id: "edm",
        label: "EDM / Big Room",
        bpm: (124.0, 132.0),
        beatmatch: true,
        play: (90.0, 160.0),
        keywords: &["edm", "big room", "electro house", "festival", "dance", "electronic", "hardstyle", "electro"],
        artists: &[
            "martin garrix",
            "david guetta",
            "avicii",
            "alesso",
            "hardwell",
            "dimitri vegas",
            "steve aoki",
            "afrojack",
            "marshmello",
            "alan walker",
            "zedd",
            "the chainsmokers",
            "swedish house mafia",
            "kshmr",
            "nicky romero",
            "r3hab",
            "don diablo",
        ],
        related: &["house", "dubstep", "pop"],
        transitions: &[("bassswap", 3.0), ("double_drop", 2.0), ("slam", 2.0), ("filter", 2.0), ("echo_out", 2.0)],
        arc: "party",
    },
    Profile {
        id: "bollywood",
        label: "Bollywood",
        bpm: (85.0, 135.0),
        beatmatch: false,
        play: (70.0, 130.0),
        keywords: &["bollywood", "hindi", "filmi", "indian", "desi", "hindi pop", "bhangra", "sufi", "indipop", "filmy", "bolly"],
        artists: &[
            "arijit singh",
            "pritam",
            "vishal shekhar",
            "vishal-shekhar",
            "shreya ghoshal",
            "sunidhi chauhan",
            "neha kakkar",
            "atif aslam",
            "a r rahman",
            "ar rahman",
            "shankar ehsaan loy",
            "amit trivedi",
            "sachin jigar",
            "sachin-jigar",
            "tanishk bagchi",
            "mika singh",
            "benny dayal",
            "kk",
            "sonu nigam",
            "shaan",
            "udit narayan",
            "alka yagnik",
            "kumar sanu",
            "jubin nautiyal",
            "darshan raval",
            "badshah",
            "yo yo honey singh",
            "honey singh",
            "guru randhawa",
            "b praak",
            "neeti mohan",
            "vishal dadlani",
            "anirudh ravichander",
            "devi sri prasad",
            "mithoon",
            "salim sulaiman",
            "himesh reshammiya",
            "armaan malik",
            "dhvani bhanushali",
            "asees kaur",
            "nucleya",
            "lata mangeshkar",
            "kishore kumar",
            "asha bhosle",
            "mohammed rafi",
            "r d burman",
            "shilpa rao",
            "nikhita gandhi",
        ],
        related: &["punjabi", "desi_hiphop", "pop"],
        transitions: &[("slam", 3.0), ("echo_out", 3.0), ("filter", 2.0), ("blend", 1.0)],
        arc: "party",
    },
    Profile {
        id: "punjabi",
        label: "Punjabi",
        bpm: (85.0, 110.0),
        beatmatch: false,
        play: (70.0, 120.0),
        keywords: &["punjabi", "bhangra", "punjabi pop"],
        artists: &[
            "diljit dosanjh",
            "sidhu moose wala",
            "karan aujla",
            "ap dhillon",
            "shubh",
            "imran khan",
            "gurinder gill",
            "jazzy b",
            "panjabi mc",
            "ammy virk",
            "babbu maan",
            "garry sandhu",
            "harrdy sandhu",
            "jasmine sandlas",
            "parmish verma",
            "the prophec",
            "ikka",
            "bohemia",
        ],
        related: &["bollywood", "desi_hiphop"],
        transitions: &[("slam", 3.0), ("echo_out", 3.0), ("filter", 1.0)],
        arc: "party",
    },
    Profile {
        id: "desi_hiphop",
        label: "Desi Hip Hop",
        bpm: (75.0, 105.0),
        beatmatch: false,
        play: (65.0, 120.0),
        keywords: &["desi hip hop", "dhh", "indian hip hop", "hindi rap", "gully", "desi rap", "hindi hip hop", "urdu rap"],
        artists: &[
            "seedhe maut",
            "encore abj",
            "calm",
            "krsna",
            "kr$na",
            "divine",
            "raftaar",
            "emiway bantai",
            "emiway",
            "mc stan",
            "prabh deep",
            "naezy",
            "brodha v",
            "dino james",
            "kaam bhaari",
            "yungsta",
            "sez on the beat",
            "karma",
            "talha anjum",
            "talhah yunus",
            "young stunners",
            "hanumankind",
            "mc altaf",
            "ritviz",
            "fotty seven",
            "raga",
            "chaar diwaari",
            "shah rule",
            "sikander kahlon",
            "king",
            "panther",
            "harjas",
            "mxrci",
            "nanku",
            "mc insane",
            "frappe ash",
            "tsumyoki",
            "ahmer",
            "yashraj",
        ],
        related: &["hiphop", "punjabi", "bollywood"],
        transitions: &[("echo_out", 3.0), ("slam", 3.0), ("filter", 1.0)],
        arc: "hype",
    },
    Profile {
        id: "hiphop",
        label: "Hip Hop",
        bpm: (70.0, 105.0),
        beatmatch: false,
        play: (65.0, 120.0),
        keywords: &["hip hop", "hip-hop", "rap", "boom bap", "gangsta", "east coast", "west coast", "grime", "uk drill", "drill"],
        artists: &[
            "kendrick lamar",
            "j cole",
            "drake",
            "travis scott",
            "kanye west",
            "jay z",
            "eminem",
            "nas",
            "50 cent",
            "tupac",
            "2pac",
            "notorious b i g",
            "a$ap rocky",
            "tyler the creator",
            "lil wayne",
            "future",
            "21 savage",
            "metro boomin",
            "playboi carti",
            "dr dre",
            "snoop dogg",
            "ice cube",
            "wu tang clan",
            "mf doom",
            "denzel curry",
            "jid",
            "central cee",
            "dave",
            "stormzy",
            "skepta",
            "pop smoke",
            "lil uzi vert",
            "nicki minaj",
            "cardi b",
            "lil baby",
            "gunna",
            "young thug",
            "mobb deep",
            "outkast",
            "busta rhymes",
        ],
        related: &["desi_hiphop", "trap", "rnb"],
        transitions: &[("echo_out", 3.0), ("slam", 3.0), ("filter", 1.0)],
        arc: "hype",
    },
    Profile {
        id: "trap",
        label: "Trap / Phonk",
        bpm: (130.0, 160.0),
        beatmatch: false,
        play: (60.0, 110.0),
        keywords: &["trap", "rage", "phonk", "drift phonk", "memphis"],
        artists: &["kordhell", "dvrst", "kslv noh", "ghostface playa", "baauer", "rl grime"],
        related: &["hiphop", "dubstep"],
        transitions: &[("slam", 3.0), ("echo_out", 2.0)],
        arc: "hype",
    },
    Profile {
        id: "pop",
        label: "Pop",
        bpm: (90.0, 130.0),
        beatmatch: false,
        play: (75.0, 140.0),
        keywords: &["pop", "dance pop", "synthpop", "k pop", "kpop", "electropop", "indie pop"],
        artists: &[
            "dua lipa",
            "the weeknd",
            "taylor swift",
            "ed sheeran",
            "ariana grande",
            "billie eilish",
            "harry styles",
            "bruno mars",
            "rihanna",
            "lady gaga",
            "katy perry",
            "justin bieber",
            "olivia rodrigo",
            "sabrina carpenter",
            "bts",
            "blackpink",
            "charli xcx",
            "doja cat",
            "post malone",
            "coldplay",
            "imagine dragons",
            "maroon 5",
            "sia",
        ],
        related: &["edm", "rnb", "house"],
        transitions: &[("echo_out", 3.0), ("filter", 3.0), ("blend", 2.0), ("slam", 2.0)],
        arc: "party",
    },
    Profile {
        id: "rnb",
        label: "R&B / Soul",
        bpm: (60.0, 110.0),
        beatmatch: false,
        play: (80.0, 150.0),
        keywords: &["r&b", "rnb", "soul", "neo soul", "funk", "motown"],
        artists: &[
            "sza",
            "frank ocean",
            "daniel caesar",
            "brent faiyaz",
            "usher",
            "beyonce",
            "summer walker",
            "chris brown",
            "alicia keys",
            "marvin gaye",
            "steve lacy",
        ],
        related: &["hiphop", "pop", "lofi"],
        transitions: &[("echo_out", 3.0), ("filter", 3.0), ("blend", 2.0)],
        arc: "chill",
    },
    Profile {
        id: "lofi",
        label: "Lo-fi / Chill",
        bpm: (60.0, 95.0),
        beatmatch: false,
        play: (90.0, 180.0),
        keywords: &["lofi", "lo fi", "chill", "chillhop", "ambient", "downtempo", "study", "jazz hop", "chillout"],
        artists: &[
            "nujabes",
            "j dilla",
            "tom misch",
            "jinsang",
            "idealism",
            "kupla",
            "potsu",
            "tomppabeats",
            "bonobo",
            "khruangbin",
            "prateek kuhad",
            "anuv jain",
            "lifafa",
        ],
        related: &["rnb", "pop"],
        transitions: &[("blend", 3.0), ("filter", 3.0), ("echo_out", 2.0)],
        arc: "chill",
    },
    Profile {
        id: "afro",
        label: "Afrobeats / Latin",
        bpm: (95.0, 118.0),
        beatmatch: true,
        play: (100.0, 180.0),
        keywords: &["afrobeats", "afrobeat", "amapiano", "afropop", "afro", "dancehall", "reggaeton", "latin", "moombahton"],
        artists: &[
            "burna boy",
            "wizkid",
            "davido",
            "rema",
            "tems",
            "asake",
            "ayra starr",
            "tyla",
            "kabza de small",
            "bad bunny",
            "j balvin",
            "daddy yankee",
            "sean paul",
            "karol g",
        ],
        related: &["house", "pop"],
        transitions: &[("blend", 3.0), ("bassswap", 2.0), ("echo_out", 2.0), ("filter", 2.0), ("slam", 1.0)],
        arc: "party",
    },
];

pub fn profile(id: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.id == id)
}

/// Lower-case, strip accents' ASCII-incompatible bytes, turn punctuation into
/// spaces (keeping `&` and `$`, which carry meaning in "d&b" and "kr$na"), split
/// digit/letter runs ("45min" -> "45 min"), and collapse whitespace.
pub fn norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev: Option<char> = None;
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        let keep = c.is_ascii_alphanumeric() || c == '&' || c == '$';
        let ch = if keep { c } else { ' ' };
        if let Some(p) = prev {
            if (p.is_ascii_digit() && ch.is_ascii_alphabetic()) || (p.is_ascii_alphabetic() && ch.is_ascii_digit()) {
                out.push(' ');
            }
        }
        if !(ch == ' ' && out.ends_with(' ')) {
            out.push(ch);
        }
        prev = Some(ch);
    }
    out.trim().to_string()
}

/// True when the normalised `phrase` occurs as whole words in normalised `text`.
pub fn has_phrase(text_padded: &str, phrase: &str) -> bool {
    let p = norm(phrase);
    !p.is_empty() && text_padded.contains(&format!(" {p} "))
}

pub fn pad(s: &str) -> String {
    format!(" {} ", norm(s))
}

/// Split "A feat. B, C & D" into individual normalised names (plus the whole string).
pub fn split_artists(artist: &str) -> Vec<String> {
    let mut parts: Vec<String> = vec![norm(artist)];
    let lowered = format!(" {} ", artist.to_lowercase());
    let mut buf = lowered;
    for sep in [",", ";", "/", " feat. ", " feat ", " ft. ", " ft ", " x ", " & ", " and ", " with "] {
        buf = buf.replace(sep, "|");
    }
    parts.extend(buf.split('|').map(norm).filter(|s| !s.is_empty()));
    parts.sort();
    parts.dedup();
    parts
}

pub fn genres_of_artist(name: &str) -> Vec<&'static str> {
    let n = norm(name);
    PROFILES.iter().filter(|p| p.artists.iter().any(|a| norm(a) == n)).map(|p| p.id).collect()
}

/// Soft multi-label scores in 0..1. Evidence adds up; BPM only confirms.
pub fn classify(tag_genre: &str, artist: &str, path: &str, bpm: Option<f64>) -> BTreeMap<String, f64> {
    let mut sc: BTreeMap<String, f64> = BTreeMap::new();
    for (text, w) in [(pad(tag_genre), 0.7), (pad(&path.replace(['/', '\\'], " ")), 0.35)] {
        if text.trim().is_empty() {
            continue;
        }
        for p in PROFILES {
            if let Some(kw) = p.keywords.iter().find(|k| has_phrase(&text, k)) {
                *sc.entry(p.id.into()).or_default() += w * if kw.len() > 4 { 1.0 } else { 0.8 };
            }
        }
    }
    for a in split_artists(artist) {
        for g in genres_of_artist(&a) {
            *sc.entry(g.into()).or_default() += 0.9;
        }
    }
    if let Some(b) = bpm {
        let fits = |p: &Profile| {
            let (lo, hi) = p.bpm;
            [b, b * 2.0, b / 2.0].iter().any(|x| (lo..=hi).contains(x))
        };
        for p in PROFILES {
            match sc.get_mut(p.id) {
                Some(v) => *v *= if fits(p) { 1.15 } else { 0.6 },
                // Narrow-window electronic genres can be guessed from tempo alone, weakly.
                None if p.beatmatch && (p.bpm.0..=p.bpm.1).contains(&b) && p.bpm.1 - p.bpm.0 <= 25.0 => {
                    sc.insert(p.id.into(), 0.12);
                }
                None => {}
            }
        }
    }
    sc.into_iter().filter(|(_, v)| *v > 0.05).map(|(k, v)| (k, (v.min(1.0) * 1000.0).round() / 1000.0)).collect()
}

/// Before analysis: a BPM range to fold octave errors into, from tags/folders/artist.
pub fn bpm_hint(tag_genre: &str, artist: &str, path: &str) -> Option<(f64, f64)> {
    let sc = classify(tag_genre, artist, path, None);
    let (id, v) = sc.iter().max_by(|a, b| a.1.partial_cmp(b.1).unwrap())?;
    let p = profile(id)?;
    (*v >= 0.5 && p.beatmatch).then_some(p.bpm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_splits_and_keeps_meaningful_symbols() {
        assert_eq!(norm("Crazy D&B set, 45min!"), "crazy d&b set 45 min");
        assert_eq!(norm("KR$NA"), "kr$na");
    }

    #[test]
    fn classifies_by_artist_tag_and_folder() {
        assert!(classify("", "Seedhe Maut", "", Some(93.0)).get("desi_hiphop").copied().unwrap_or(0.0) > 0.9);
        assert!(classify("Drum & Bass", "Unknown", "", Some(174.0)).get("dnb").copied().unwrap_or(0.0) > 0.7);
        assert!(classify("", "", "/music/DnB/track.mp3", None).contains_key("dnb"));
        let feat = classify("", "Badshah feat. Diljit Dosanjh", "", None);
        assert!(feat.contains_key("bollywood") && feat.contains_key("punjabi"));
    }

    #[test]
    fn bpm_hint_only_for_beatmatched_genres() {
        assert_eq!(bpm_hint("", "Netsky", ""), Some((160.0, 180.0)));
        assert_eq!(bpm_hint("", "Arijit Singh", ""), None);
    }

    #[test]
    fn every_transition_id_is_known() {
        for p in PROFILES {
            for (t, _) in p.transitions {
                assert!(crate::planner::TransitionKind::from_id(t).is_some(), "{} uses unknown transition {t}", p.id);
            }
        }
    }
}
