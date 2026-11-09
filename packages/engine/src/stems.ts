// Stem decks. A track with a vocal stem plays from one 4-channel buffer:
// channels 0-1 the instrumental, 2-3 the vocals, where
//
//     instrumental = original - vocals
//
// so with both stems up the deck outputs the untouched original, bit for
// bit, and every playback trick (loops, slip, backspin) keeps working on a
// single source node. Only the vocal stem is stored; this file derives the
// rest when the track loads.
//
// The original and the stem are decoded by different decoders (the browser
// for the file, ffmpeg + Vorbis for the stem), which can disagree by an
// encoder delay (AAC priming is 1024-2112 samples). A subtraction that is off
// by even a few samples leaves the vocal in the "instrumental", so the stem is
// aligned to the original first, by cross-correlation on a loud passage.

/** Lag (in samples) to shift `voc` by so it lines up with `full`: voc[i - lag] ~ part of full[i]. */
export function alignLag(full: Float32Array, voc: Float32Array, maxLag = 4096): number {
  const n = Math.min(full.length, voc.length);
  if (n < maxLag * 4) return 0;
  // Analyse the loudest stretch of the vocal: correlation needs vocal energy.
  const win = Math.min(32768, Math.floor(n / 4));
  let best = maxLag;
  let bestE = -1;
  for (let s = maxLag; s + win + maxLag < n; s += win >> 1) {
    let e = 0;
    for (let i = s; i < s + win; i += 16) e += (voc[i] ?? 0) * (voc[i] ?? 0);
    if (e > bestE) {
      bestE = e;
      best = s;
    }
  }
  if (bestE <= 1e-9) return 0;
  // Coarse: correlate 4x-decimated (box-averaged, so low-passed) copies, whose
  // correlation peak is ~4 samples wide and can't fall between coarse lags.
  const D = 4;
  const dec = (x: Float32Array, from: number, len: number) => {
    const o = new Float32Array(Math.floor(len / D));
    for (let k = 0; k < o.length; k++) {
      let acc = 0;
      for (let q = 0; q < D; q++) acc += x[from + k * D + q] ?? 0;
      o[k] = acc;
    }
    return o;
  };
  const fd = dec(full, best, win);
  const vd = dec(voc, best - maxLag, win + 2 * maxLag);
  const off = maxLag / D;
  let lag = 0;
  let top = -Infinity;
  for (let l = -off; l <= off; l++) {
    let c = 0;
    for (let k = 0; k < fd.length; k++) c += (fd[k] ?? 0) * (vd[k + off - l] ?? 0);
    if (c > top) {
      top = c;
      lag = l * D;
    }
  }
  const corr = (lg: number) => {
    let c = 0;
    for (let i = best; i < best + win; i++) c += (full[i] ?? 0) * (voc[i - lg] ?? 0);
    return c;
  };
  let fine = lag;
  top = -Infinity;
  for (let l = lag - 6; l <= lag + 6; l++) {
    const c = corr(l);
    if (c > top) {
      top = c;
      fine = l;
    }
  }
  return fine;
}

/** The 4 stem-deck channels (instrumental L/R, vocals L/R): pure, so it's unit-tested. */
export function buildStemChannels(full: Float32Array[], voc: Float32Array[]): { channels: Float32Array[]; lag: number } {
  const fl = full[0]!;
  const fr = full[1] ?? fl;
  const vl0 = voc[0]!;
  const vr0 = voc[1] ?? vl0;
  const n = fl.length;
  const monoFull = new Float32Array(n);
  const monoVoc = new Float32Array(vl0.length);
  for (let i = 0; i < n; i++) monoFull[i] = 0.5 * ((fl[i] ?? 0) + (fr[i] ?? 0));
  for (let i = 0; i < vl0.length; i++) monoVoc[i] = 0.5 * ((vl0[i] ?? 0) + (vr0[i] ?? 0));
  const lag = alignLag(monoFull, monoVoc);
  const il = new Float32Array(n);
  const ir = new Float32Array(n);
  const vl = new Float32Array(n);
  const vr = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const j = i - lag;
    const a = j >= 0 && j < vl0.length ? (vl0[j] ?? 0) : 0;
    const b = j >= 0 && j < vr0.length ? (vr0[j] ?? 0) : 0;
    vl[i] = a;
    vr[i] = b;
    il[i] = (fl[i] ?? 0) - a;
    ir[i] = (fr[i] ?? 0) - b;
  }
  return { channels: [il, ir, vl, vr], lag };
}

/** Combine a decoded track and its decoded vocal stem into a stem-deck buffer. */
export function stemBuffer(ctx: BaseAudioContext, full: AudioBuffer, vocals: AudioBuffer): { buffer: AudioBuffer; lag: number } {
  const ch = (b: AudioBuffer) => Array.from({ length: Math.min(2, b.numberOfChannels) }, (_, i) => b.getChannelData(i));
  const { channels, lag } = buildStemChannels(ch(full), ch(vocals));
  const out = ctx.createBuffer(4, full.length, full.sampleRate);
  channels.forEach((c, i) => out.copyToChannel(c as Float32Array<ArrayBuffer>, i));
  return { buffer: out, lag };
}
