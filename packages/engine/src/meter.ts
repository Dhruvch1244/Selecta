// Peak/RMS metering with peak-hold, decoupled from the analyser node so it is
// testable. Ballistics follow common DAW practice: instant attack, ~20 dB/s
// peak fall, 1.5 s hold on the peak marker, clip latch until clicked.

export interface MeterState {
  peakDb: number;
  rmsDb: number;
  holdDb: number;
  holdUntil: number;
  clipped: boolean;
}

export const FLOOR_DB = -60;
export const newMeter = (): MeterState => ({ peakDb: FLOOR_DB, rmsDb: FLOOR_DB, holdDb: FLOOR_DB, holdUntil: 0, clipped: false });
export const toDb = (v: number): number => (v > 1e-6 ? Math.max(FLOOR_DB, 20 * Math.log10(v)) : FLOOR_DB);

export function stepMeter(m: MeterState, block: Float32Array, nowMs: number, dtMs: number): MeterState {
  let pk = 0;
  let sq = 0;
  for (let i = 0; i < block.length; i++) {
    const v = block[i] ?? 0;
    const a = v < 0 ? -v : v;
    if (a > pk) pk = a;
    sq += v * v;
  }
  const inPeak = toDb(pk);
  const inRms = toDb(Math.sqrt(sq / Math.max(1, block.length)));
  const fall = (20 * dtMs) / 1000;
  const peakDb = Math.max(inPeak, m.peakDb - fall);
  const rmsDb = Math.max(inRms, m.rmsDb - fall);
  let { holdDb, holdUntil } = m;
  if (inPeak >= holdDb || nowMs > holdUntil) {
    holdDb = inPeak;
    holdUntil = nowMs + 1500;
  }
  return { peakDb, rmsDb, holdDb, holdUntil, clipped: m.clipped || pk >= 0.999 };
}

/** Map dB to 0..1 meter height with more resolution near 0 dB, like a DAW scale. */
export const meterPos = (db: number): number => Math.pow(Math.max(0, (db - FLOOR_DB) / -FLOOR_DB), 1.6);
