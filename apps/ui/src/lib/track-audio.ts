// Loads a library track for a deck: analysis, audio and (if separated) the
// vocal stem, all in parallel. With a stem the deck gets the 4-channel stem
// buffer (see packages/engine/src/stems.ts); a missing or unreadable stem is
// never an error, the track just plays without stem control.

import { type LoadedTrack, measureLoudness, normGain, stemBuffer } from "@selecta/engine";
import { api, readAudio } from "./api";
import { getEngine } from "./store";

export async function loadTrackAudio(t: { id: string; path: string; artist: string; title: string }): Promise<LoadedTrack> {
  const eng = getEngine();
  const vocalBytes = api
    .trackVocals(t.id)
    .then((p) => (p ? readAudio(p) : null))
    .catch(() => null);
  const [raw, bytes] = await Promise.all([api.analysis(t.id), readAudio(t.path)]);
  const [buffer, vb] = await Promise.all([eng.decode(bytes), vocalBytes]);
  // Normalise on the audio that will actually play (see engine/loudness.ts).
  const { lufs, peakDb } = measureLoudness(buffer);
  const analysis = { ...raw, loudness: lufs, peakDb, gainDb: normGain(lufs, peakDb) };
  if (vb) {
    try {
      const vocals = await eng.decode(vb);
      return { id: t.id, artist: t.artist, title: t.title, analysis, buffer: stemBuffer(eng.ctx, buffer, vocals).buffer };
    } catch (e) {
      console.warn("vocal stem unreadable, playing without stems:", e);
    }
  }
  return { id: t.id, artist: t.artist, title: t.title, analysis, buffer };
}
