// Lookahead brickwall limiter (stereo-linked). Plain JS on purpose: worklet
// modules load by URL and must run without a build step.
//
// Why not DynamicsCompressorNode: browsers apply automatic makeup gain and
// a soft knee to it, so it raises the level, pumps, and still lets
// overshoots through. Here the gain is reduced *before* a peak arrives:
// the signal is delayed by the lookahead, and the gain follows the minimum
// required gain over that window, smoothed so it never steps. A final hard
// clip at the ceiling is only a safety net and stays idle in practice (the
// self-test measures that).

class Limiter extends AudioWorkletProcessor {
  static get parameterDescriptors() {
    return [
      { name: "ceiling", defaultValue: -1, minValue: -24, maxValue: 0, automationRate: "k-rate" },
      { name: "release", defaultValue: 0.08, minValue: 0.005, maxValue: 1, automationRate: "k-rate" },
    ];
  }

  constructor() {
    super();
    this.look = Math.max(16, Math.round(0.005 * sampleRate)); // 5 ms
    this.buf = [new Float32Array(this.look), new Float32Array(this.look)];
    this.req = new Float32Array(this.look).fill(1); // required gain per sample in the window
    this.pos = 0;
    this.gain = 1;
    this.attack = 1 - Math.exp(-1 / (this.look / 4)); // reaches the target well inside the lookahead
    this.reduction = 0; // deepest gain reduction this block (dB), for metering
    this.port.onmessage = () => {};
  }

  process(inputs, outputs, params) {
    const inp = inputs[0];
    const out = outputs[0];
    if (!out || !out.length) return true;
    const ceil = Math.pow(10, params.ceiling[0] / 20);
    const rel = 1 - Math.exp(-1 / (params.release[0] * sampleRate));
    const n = out[0].length;
    const L = this.look;
    let minGain = 1;
    for (let i = 0; i < n; i++) {
      const a = inp[0] ? inp[0][i] : 0;
      const b = inp[1] ? inp[1][i] : a;
      const peak = Math.max(Math.abs(a), Math.abs(b));
      this.req[this.pos] = peak > ceil ? ceil / peak : 1;
      // Window minimum (L is ~240 samples, so a plain scan is cheap and obviously correct).
      let target = 1;
      for (let k = 0; k < L; k++) if (this.req[k] < target) target = this.req[k];
      // Smooth both ways: attack time constant L/4 reaches ~98% of the target
      // within the lookahead; the hard clip below catches the last ~2%.
      this.gain += (target < this.gain ? this.attack : rel) * (target - this.gain);
      const g = this.gain;
      if (g < minGain) minGain = g;
      // Output the delayed sample with the gain computed for it.
      const da = this.buf[0][this.pos];
      const db = this.buf[1][this.pos];
      this.buf[0][this.pos] = a;
      this.buf[1][this.pos] = b;
      for (let ch = 0; ch < out.length; ch++) {
        let v = (ch === 0 ? da : db) * g;
        if (v > ceil) v = ceil;
        else if (v < -ceil) v = -ceil;
        out[ch][i] = v;
      }
      this.pos = (this.pos + 1) % L;
    }
    this.reduction = 20 * Math.log10(Math.max(minGain, 1e-6));
    // Report the deepest reduction ~20x a second, for the master readout.
    this.worst = Math.min(this.worst ?? 0, this.reduction);
    if ((this.blocks = (this.blocks ?? 0) + 1) % 18 === 0) {
      this.port.postMessage(this.worst);
      this.worst = 0;
    }
    return true;
  }
}

registerProcessor("selecta-limiter", Limiter);
