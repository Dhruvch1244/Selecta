// Keylock and key shifting, rendered offline.
//
// A deck changes tempo by playback rate, which also changes pitch (+4% tempo
// = +0.68 semitones: vocals go chipmunk). Real-time keylock would put a
// pitch shifter with ~100 ms of latency into the deck, and every grid,
// loop and FX time would have to compensate. Instead the track is
// pitch-shifted *before* it plays, by -12*log2(rate) (plus any harmonic key
// shift), keeping its length: played at `rate`, it has the new tempo and
// its own key. The deck, grid and FX never know. The cost is a render when
// the track is prepared (a set prepares the next track a whole track ahead).
//
// Signalsmith Stretch (MIT, WASM) does the shifting: it's one of the best
// open-source stretchers and runs in an AudioWorklet, so it works inside an
// OfflineAudioContext at faster than real time.

import SignalsmithStretch from "signalsmith-stretch";

/** Semitones a track must be shifted so that, played at `rate`, it sounds at its original pitch plus `keyShift`. */
export function keylockSemitones(rate: number, keyShift = 0): number {
  return -12 * Math.log2(rate) + keyShift;
}

/** Below this the shift is inaudible (1/100 semitone) and the original is used as is. */
const EPS = 0.01;

/**
 * The buffer pitch-shifted by `semitones`, same length and channel count
 * (stem buffers keep their 4 channels, so stem kills still work).
 */
export async function shiftPitch(buffer: AudioBuffer, semitones: number, region?: { from: number; to: number }): Promise<AudioBuffer> {
  if (Math.abs(semitones) < EPS) return buffer;
  if (region) return shiftRegion(buffer, semitones, region);
  const ch = buffer.numberOfChannels;
  const ctx = new OfflineAudioContext(ch, buffer.length, buffer.sampleRate);
  const node = await SignalsmithStretch(ctx, { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [ch] });
  node.connect(ctx.destination);
  // Tonality limit: above ~8 kHz the stretcher treats content as noise, which
  // keeps hats and air crisp instead of phasey.
  node.schedule({ output: 0, input: 0, rate: 1, semitones, tonalityHz: 8000, active: true });
  // schedule() is a message to the worklet; an offline render can finish
  // before it arrives (measured: a silent render, 76 ms). addBuffers replies
  // over the same ordered port, so once it resolves both have landed.
  await node.addBuffers(Array.from({ length: ch }, (_, c) => buffer.getChannelData(c)));
  return ctx.startRendering();
}

/**
 * Shift only [from, to] seconds (what a set will actually play, plus a
 * margin), copied into the original: several times faster than the whole
 * track. Outside the region the track keeps its original pitch, which only
 * a big manual jump would ever reach.
 */
async function shiftRegion(buffer: AudioBuffer, semitones: number, region: { from: number; to: number }): Promise<AudioBuffer> {
  const sr = buffer.sampleRate;
  const a = Math.max(0, Math.floor(region.from * sr));
  const b = Math.min(buffer.length, Math.ceil(region.to * sr));
  if (b - a < sr) return buffer;
  const ch = buffer.numberOfChannels;
  const part = new AudioBuffer({ numberOfChannels: ch, length: b - a, sampleRate: sr });
  for (let c = 0; c < ch; c++) part.copyToChannel(buffer.getChannelData(c).subarray(a, b), c);
  const shifted = await shiftPitch(part, semitones);
  const out = new AudioBuffer({ numberOfChannels: ch, length: buffer.length, sampleRate: sr });
  // 20 ms crossfades at the region edges, so reaching one never clicks.
  const fade = Math.min(Math.floor(0.02 * sr), (b - a) >> 2);
  for (let c = 0; c < ch; c++) {
    const src = buffer.getChannelData(c);
    const dst = new Float32Array(src);
    const sh = shifted.getChannelData(c);
    for (let i = 0; i < b - a; i++) {
      const w = i < fade ? i / fade : i > b - a - fade ? (b - a - i) / fade : 1;
      dst[a + i] = w * (sh[i] ?? 0) + (1 - w) * (src[a + i] ?? 0);
    }
    out.copyToChannel(dst, c);
  }
  return out;
}
