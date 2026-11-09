//! "Now playing" for the rest of the desktop, over MPRIS (Linux).
//!
//! Selecta plays through Web Audio, which no desktop media integration can
//! see. This publishes `org.mpris.MediaPlayer2.selecta` so the Lyric Overlay
//! (and GNOME/KDE media controls, playerctl, media keys) knows what's on.
//!
//! The UI pushes the lead deck's track, play state and rate on every change
//! and a position correction about once a second. `Position` is computed at
//! read time by projecting from that anchor: MPRIS clients poll it (the
//! overlay calls GetAll each cycle) rather than wait for change signals.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Deserialize;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};
use zbus::{connection, interface, Connection};

const PATH: &str = "/org/mpris/MediaPlayer2";

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub length_sec: f64,
    /// Track position the listener hears right now (output latency already taken off).
    pub position_sec: f64,
    pub playing: bool,
    /// Playback rate of the deck (track seconds per second).
    pub rate: f64,
}

#[derive(Default)]
struct State {
    np: Option<NowPlaying>,
    at: Option<Instant>,
}

impl State {
    fn position_sec(&self) -> f64 {
        let Some(n) = &self.np else { return 0.0 };
        let mut p = n.position_sec;
        if n.playing {
            p += self.at.map(|a| a.elapsed().as_secs_f64()).unwrap_or(0.0) * n.rate;
        }
        p.clamp(0.0, n.length_sec.max(0.0))
    }
}

type Shared = Arc<Mutex<State>>;

struct Root;

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {}
    fn quit(&self) {}
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        "Selecta"
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "selecta"
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec![]
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        vec![]
    }
}

/// Where media keys go (the UI, in the app).
pub type OnKey = Arc<dyn Fn(&str) + Send + Sync>;

struct Player {
    st: Shared,
    on_key: OnKey,
}

fn ov(v: Value<'_>) -> OwnedValue {
    v.try_to_owned().expect("plain values always convert")
}

impl Player {
    /// Media keys and desktop controls go to the UI, which owns the decks.
    fn key(&self, k: &str) {
        (self.on_key)(k);
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        self.key("next");
    }
    fn previous(&self) {}
    fn pause(&self) {
        self.key("pause");
    }
    fn play_pause(&self) {
        self.key("playpause");
    }
    fn stop(&self) {
        self.key("stop");
    }
    fn play(&self) {
        self.key("play");
    }
    fn seek(&self, _offset: i64) {}
    fn set_position(&self, _track: ObjectPath<'_>, _position: i64) {}
    fn open_uri(&self, _uri: &str) {}

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> &str {
        match &self.st.lock().unwrap().np {
            Some(n) if n.playing => "Playing",
            Some(_) => "Paused",
            None => "Stopped",
        }
    }
    #[zbus(property)]
    fn loop_status(&self) -> &str {
        "None"
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        self.st.lock().unwrap().np.as_ref().map(|n| n.rate).unwrap_or(1.0)
    }
    #[zbus(property)]
    fn shuffle(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let st = self.st.lock().unwrap();
        let mut m = HashMap::new();
        let Some(n) = &st.np else {
            m.insert(
                "mpris:trackid".into(),
                ov(Value::from(ObjectPath::from_static_str_unchecked("/org/mpris/MediaPlayer2/TrackList/NoTrack"))),
            );
            return m;
        };
        // Track ids are hex, so always a valid object path element.
        if let Ok(p) = ObjectPath::try_from(format!("/org/selecta/track/t{}", n.id)) {
            m.insert("mpris:trackid".into(), ov(Value::from(p)));
        }
        m.insert("mpris:length".into(), ov(Value::from((n.length_sec * 1e6) as i64)));
        m.insert("xesam:title".into(), ov(Value::from(n.title.clone())));
        m.insert("xesam:artist".into(), ov(Value::from(vec![n.artist.clone()])));
        m.insert("xesam:album".into(), ov(Value::from(n.album.clone())));
        m
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        1.0
    }
    /// Live: projected from the last update at read time (no change signal, per spec).
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        (self.st.lock().unwrap().position_sec() * 1e6) as i64
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        0.5
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        2.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

/// The published player. Cheap to clone.
#[derive(Clone)]
pub struct Mpris {
    conn: Connection,
    st: Shared,
}

impl Mpris {
    pub async fn start(on_key: OnKey) -> zbus::Result<Self> {
        Self::start_named("org.mpris.MediaPlayer2.selecta", on_key).await
    }

    async fn start_named(name: &str, on_key: OnKey) -> zbus::Result<Self> {
        let st: Shared = Arc::default();
        let conn = connection::Builder::session()?
            .name(name.to_string())?
            .serve_at(PATH, Root)?
            .serve_at(PATH, Player { st: st.clone(), on_key })?
            .build()
            .await?;
        Ok(Mpris { conn, st })
    }

    /// New now-playing state from the UI; signals what changed.
    pub async fn update(&self, np: Option<NowPlaying>) -> zbus::Result<()> {
        let (changed_track, changed_state, jumped, pos) = {
            let mut st = self.st.lock().unwrap();
            let expected = st.position_sec();
            let old = st.np.clone();
            let changed_track = old.as_ref().map(|o| (&o.id, &o.title)) != np.as_ref().map(|n| (&n.id, &n.title));
            let changed_state = old.as_ref().map(|o| (o.playing, o.rate)) != np.as_ref().map(|n| (n.playing, n.rate));
            // A loop roll, rewind or manual seek: tell clients to re-sync now.
            let jumped = !changed_track && np.as_ref().is_some_and(|n| (n.position_sec - expected).abs() > 0.75);
            st.np = np;
            st.at = Some(Instant::now());
            (changed_track, changed_state, jumped, st.position_sec())
        };
        if !(changed_track || changed_state || jumped) {
            return Ok(());
        }
        let iface = self.conn.object_server().interface::<_, Player>(PATH).await?;
        let p = iface.get().await;
        let em = iface.signal_emitter();
        if changed_track {
            p.metadata_changed(em).await?;
        }
        if changed_state || changed_track {
            p.playback_status_changed(em).await?;
            p.rate_changed(em).await?;
        }
        if jumped || changed_track {
            Player::seeked(em, (pos * 1e6) as i64).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_projects_forward_at_the_deck_rate_and_stops_when_paused() {
        let np = NowPlaying { id: "1".into(), length_sec: 300.0, position_sec: 10.0, playing: true, rate: 1.04, ..Default::default() };
        let st = State { np: Some(np.clone()), at: Some(Instant::now() - std::time::Duration::from_secs(5)) };
        assert!((st.position_sec() - (10.0 + 5.0 * 1.04)).abs() < 0.05, "{}", st.position_sec());
        let paused = State { np: Some(NowPlaying { playing: false, ..np.clone() }), at: st.at };
        assert_eq!(paused.position_sec(), 10.0);
        let near_end = State { np: Some(NowPlaying { position_sec: 299.0, ..np }), at: st.at };
        assert_eq!(near_end.position_sec(), 300.0, "never past the end");
    }

    /// Publishes on the real session bus and reads it back the way the Lyric
    /// Overlay does (GetAll on the Player interface). Skips without a bus (CI).
    #[tokio::test]
    async fn lyric_overlay_sees_the_track_and_a_live_position() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("no session bus; skipped");
            return;
        }
        let keys = Arc::new(Mutex::new(vec![]));
        let k2 = keys.clone();
        let name = format!("org.mpris.MediaPlayer2.selecta_test{}", std::process::id());
        let m = Mpris::start_named(&name, Arc::new(move |k: &str| k2.lock().unwrap().push(k.to_string()))).await.unwrap();
        let np = NowPlaying {
            id: "00ab".into(),
            title: "Brown Munde".into(),
            artist: "AP Dhillon".into(),
            album: "Brown Munde".into(),
            length_sec: 267.0,
            position_sec: 42.0,
            playing: true,
            rate: 1.0,
        };
        m.update(Some(np)).await.unwrap();

        let client = Connection::session().await.unwrap();
        let get_all = || async {
            let r = client
                .call_method(
                    Some(name.as_str()),
                    PATH,
                    Some("org.freedesktop.DBus.Properties"),
                    "GetAll",
                    &("org.mpris.MediaPlayer2.Player",),
                )
                .await
                .unwrap();
            r.body().deserialize::<HashMap<String, OwnedValue>>().unwrap()
        };
        let mut p = get_all().await;
        assert_eq!(String::try_from(p.remove("PlaybackStatus").unwrap()).unwrap(), "Playing");
        let meta = <HashMap<String, OwnedValue>>::try_from(p.remove("Metadata").unwrap()).unwrap();
        assert_eq!(meta["xesam:title"].downcast_ref::<&str>().unwrap(), "Brown Munde");
        assert_eq!(meta["mpris:length"].downcast_ref::<i64>().unwrap(), 267_000_000);
        let pos1 = i64::try_from(p.remove("Position").unwrap()).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let pos2 = i64::try_from(get_all().await.remove("Position").unwrap()).unwrap();
        assert!((42_000_000..42_500_000).contains(&pos1), "{pos1}");
        assert!(pos2 - pos1 >= 250_000, "position must advance between polls: {pos1} -> {pos2}");

        // Media keys reach the app.
        client.call_method(Some(name.as_str()), PATH, Some("org.mpris.MediaPlayer2.Player"), "Next", &()).await.unwrap();
        assert_eq!(*keys.lock().unwrap(), vec!["next".to_string()]);
    }
}
