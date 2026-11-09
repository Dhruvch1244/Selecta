// High-resolution display waveform from decoded audio: 100 columns per second,
// peak plus low/high band energy via one-pole filters. Computed once per load
// (~30 ms for a 5-minute track) so drawing is a plain array lookup per pixel.

export interface Waveform {
  fps: number;
  peak: Float32Array;
  low: Float32Array;
  high: Float32Array;
}

export function computeWaveform(channels: Float32Array[], sampleRate: number, fps = 100): Waveform {
  const a = channels[0] ?? new Float32Array(0);
  const b = channels[1] ?? a;
  // Stem buffers (see stems.ts) carry instrumental + vocals; the display shows their sum, the track.
  const c = channels[2];
  const d = channels[3] ?? c;
  const per = Math.max(1, Math.floor(sampleRate / fps));
  const n = Math.floor(a.length / per);
  const peak = new Float32Array(n);
  const low = new Float32Array(n);
  const high = new Float32Array(n);
  const kL = 1 - Math.exp((-2 * Math.PI * 180) / sampleRate);
  const kH = 1 - Math.exp((-2 * Math.PI * 3000) / sampleRate);
  let lp = 0;
  let lp2 = 0;
  for (let i = 0, j = 0; i < n; i++) {
    let pk = 0;
    let el = 0;
    let eh = 0;
    for (let k = 0; k < per; k++, j++) {
      let x = 0.5 * ((a[j] ?? 0) + (b[j] ?? 0));
      if (c && d) x += 0.5 * ((c[j] ?? 0) + (d[j] ?? 0));
      lp += kL * (x - lp);
      lp2 += kH * (x - lp2);
      const h = x - lp2;
      const ax = x < 0 ? -x : x;
      if (ax > pk) pk = ax;
      el += lp * lp;
      eh += h * h;
    }
    peak[i] = pk;
    low[i] = Math.sqrt(el / per);
    high[i] = Math.sqrt(eh / per);
  }
  for (const arr of [peak, low, high]) {
    let m = 1e-6;
    for (const v of arr) if (v > m) m = v;
    for (let i = 0; i < arr.length; i++) arr[i] = (arr[i] ?? 0) / m;
  }
  return { fps, peak, low, high };
}
