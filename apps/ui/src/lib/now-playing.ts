// Tells the desktop what the listener is hearing (MPRIS on Linux), so the
// Lyric Overlay can show the right lyrics in sync, and media keys work.
//
// "What's playing" is the lead deck: the set's lead while a set plays,
// otherwise the deck that's audible (the louder fader if both are). The
// position sent is what's *heard*, so the output latency is taken off. Rust
// projects the position between updates; this re-anchors it every second
// and immediately on any change (track, play/pause, rate, a jump).

import type { DeckId } from "@selecta/engine";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "./api";
import { useSet } from "./set-store";
import { getEngine, useApp } from "./store";

interface NowPlaying {
  id: string;
  title: string;
  artist: string;
  album: string;
  lengthSec: number;
  positionSec: number;
  playing: boolean;
  rate: number;
}

let lastLead: DeckId | null = null;

function leadDeck(): DeckId | null {
  const eng = getEngine();
  const set = useSet.getState();
  if (set.playing && set.leadDeck) return set.leadDeck;
  const { A, B } = eng.decks;
  if (A.playing && B.playing) return A.fader.gain.value >= B.fader.gain.value ? "A" : "B";
  if (A.playing) return "A";
  if (B.playing) return "B";
  // Nothing playing: report the last lead as paused, if it still has a track.
  return lastLead && eng.decks[lastLead].track ? lastLead : null;
}

export function snapshot(): NowPlaying | null {
  const id = leadDeck();
  if (!id) return null;
  lastLead = id;
  const eng = getEngine();
  const d = eng.decks[id];
  const t = d.track;
  if (!t) return null;
  const meta = useApp.getState().tracks.find((x) => x.id === t.id);
  // What the listener hears now (speaker delay included), so the Lyric Overlay's lyrics land on time.
  const heard = d.position(eng.heardTime());
  return {
    id: t.id,
    title: t.title,
    artist: t.artist,
    album: meta?.album ?? "",
    lengthSec: t.buffer.duration,
    positionSec: Math.max(0, heard),
    playing: d.playing,
    rate: d.rate,
  };
}

const key = (n: NowPlaying | null) => (n ? `${n.id}|${n.playing}|${n.rate.toFixed(4)}` : "none");

/** Start publishing. Returns a stop function. Desktop only. */
export function startNowPlaying(): () => void {
  if (!inTauri()) return () => {};
  let last = "";
  let lastSent = 0;
  const push = (force = false) => {
    const np = snapshot();
    const k = key(np);
    const now = performance.now();
    // On any change at once; while playing, a position correction each second.
    if (!force && k === last && (!np?.playing || now - lastSent < 1000)) return;
    last = k;
    lastSent = now;
    void invoke("now_playing", { np }).catch(() => {});
  };
  const timer = setInterval(() => push(), 250);
  const unsubs = [useSet.subscribe(() => push()), useApp.subscribe(() => push())];

  // Media keys / desktop controls arrive from MPRIS as events.
  const unlisten = listen<string>("media-key", (e) => {
    const set = useSet.getState();
    const app = useApp.getState();
    if (set.playing) {
      if (e.payload === "next") set.skip();
      else if (e.payload !== "play") set.stop();
      return;
    }
    const id = leadDeck() ?? (getEngine().decks.A.track ? "A" : null);
    if (!id) return;
    const playing = getEngine().decks[id].playing;
    if (e.payload === "playpause" || (e.payload === "play" && !playing) || ((e.payload === "pause" || e.payload === "stop") && playing)) void app.togglePlay(id);
    setTimeout(() => push(true), 50);
  });

  return () => {
    clearInterval(timer);
    unsubs.forEach((u) => u());
    void unlisten.then((f) => f());
    void invoke("now_playing", { np: null }).catch(() => {});
  };
}
