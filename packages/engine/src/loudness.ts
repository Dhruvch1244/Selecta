// Loudness normalisation, measured on the exact audio the deck plays.
//
// ITU-R BS.1770 short-term loudness (K-weighted, 3 s windows, 1 s hop) over
// the decoded stereo buffer; a track's level is the 90th percentile of its
// non-silent windows, i.e. how loud its drops/choruses are, so quiet intros
// and breakdowns don't drag it up. Every track's loud parts are brought to
// TARGET_LUFS. The analyser's mono-downmix estimate was off by up to 3 dB on
// Opus files, hence measuring here.

export const TARGET_LUFS = -10;
/** How far a boost may push a sample peak past 0 dBFS; the master's lookahead limiter takes the rest. */
export const BOOST_PEAK_CEIL_DB = 3;

export interface ChannelSource {
  numberOfChannels: number;
  sampleRate: number;
  getChannelData(ch: number): Float32Array;
}

/** K-weighting: +4 dB high shelf at 1.5 kHz, then a 38 Hz high-pass (RBJ forms, as pyloudnorm). */
function kWeighted(x: Float32Array, rate: number): Float64Array {
  const y = new Float64Array(x.length);
  const stage = (src: ArrayLike<number>, dst: Float64Array, b: number[], a: number[]) => {
    let x1 = 0, x2 = 0, y1 = 0, y2 = 0;
    const [b0, b1, b2] = b.map((v) => v / a[0]!) as [number, number, number];
    const [a1, a2] = [a[1]! / a[0]!, a[2]! / a[0]!];
    for (let i = 0; i < src.length; i++) {
      const v = src[i]!;
      const o = b0 * v + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
      x2 = x1; x1 = v; y2 = y1; y1 = o;
      dst[i] = o;
    }
  };
  {
    const A = 10 ** (4 / 40);
    const w = (2 * Math.PI * 1500) / rate;
    const c = Math.cos(w), al = Math.sin(w) / (2 * Math.SQRT1_2), sa = 2 * Math.sqrt(A) * al;
    stage(
      x,
      y,
      [A * (A + 1 + (A - 1) * c + sa), -2 * A * (A - 1 + (A + 1) * c), A * (A + 1 + (A - 1) * c - sa)],
      [A + 1 - (A - 1) * c + sa, 2 * (A - 1 - (A + 1) * c), A + 1 - (A - 1) * c - sa],
    );
  }
  {
    const w = (2 * Math.PI * 38) / rate;
    const c = Math.cos(w), al = Math.sin(w) / (2 * 0.5);
    stage(y, y, [(1 + c) / 2, -(1 + c), (1 + c) / 2], [1 + al, -2 * c, 1 - al]);
  }
  return y;
}

/**
 * Loudness of the loud parts (LUFS) and sample peak (dBFS). A 4-channel stem
 * buffer (instrumental L/R + vocals L/R) is measured as the full mix; a mono
 * buffer as if on both speakers.
 */
export function measureLoudness(buf: ChannelSource): { lufs: number; peakDb: number } {
  const rate = buf.sampleRate;
  const n = buf.getChannelData(0).length;
  const sides: Float32Array[] = [];
  if (buf.numberOfChannels >= 4) {
    for (const [a, b] of [[0, 2], [1, 3]] as const) {
      const s = new Float32Array(n);
      const x = buf.getChannelData(a), v = buf.getChannelData(b);
      for (let i = 0; i < n; i++) s[i] = x[i]! + v[i]!;
      sides.push(s);
    }
  } else if (buf.numberOfChannels >= 2) sides.push(buf.getChannelData(0), buf.getChannelData(1));
  else sides.push(buf.getChannelData(0), buf.getChannelData(0));

  let peak = 0;
  const hop = Math.max(1, Math.round(rate));
  const nb = Math.ceil(n / hop);
  const power = new Float64Array(nb);
  for (const s of sides) {
    for (let i = 0; i < n; i++) {
      const a = Math.abs(s[i]!);
      if (a > peak) peak = a;
    }
    const k = kWeighted(s, rate);
    for (let b = 0; b < nb; b++) {
      let acc = 0;
      const end = Math.min(n, (b + 1) * hop);
      for (let i = b * hop; i < end; i++) acc += k[i]! * k[i]!;
      power[b]! += acc / (end - b * hop);
    }
  }
  const st: number[] = [];
  for (let b = 0; b + 3 <= nb; b++) {
    const l = -0.691 + 10 * Math.log10((power[b]! + power[b + 1]! + power[b + 2]!) / 3 + 1e-12);
    if (l > -60) st.push(l);
  }
  st.sort((a, b) => a - b);
  const lufs = st.length ? st[Math.round((st.length - 1) * 0.9)]! : -60;
  return { lufs, peakDb: 20 * Math.log10(peak + 1e-9) };
}

/** Gain bringing the loud parts to TARGET_LUFS; a boost stops at the peak ceiling, a cut never does. */
export function normGain(lufs: number, peakDb: number): number {
  const want = TARGET_LUFS - lufs;
  if (want <= 0) return Math.max(want, -15);
  return Math.min(want, Math.max(0, BOOST_PEAK_CEIL_DB - peakDb), 15);
}
