// Bitcrusher + sample-rate reducer: the one effect family that needs per-sample code.
class Crusher extends AudioWorkletProcessor {
  static get parameterDescriptors() {
    return [
      { name: "bits", defaultValue: 16, minValue: 1, maxValue: 16, automationRate: "k-rate" },
      { name: "factor", defaultValue: 1, minValue: 1, maxValue: 64, automationRate: "k-rate" },
    ];
  }
  constructor() {
    super();
    this.hold = [0, 0];
    this.n = 0;
  }
  process(inputs, outputs, p) {
    const inp = inputs[0];
    const out = outputs[0];
    if (!inp || !inp.length) return true;
    const step = Math.pow(0.5, p.bits[0] - 1);
    const f = Math.max(1, Math.round(p.factor[0]));
    let n = this.n;
    for (let ch = 0; ch < out.length; ch++) {
      const i = inp[ch] || inp[0];
      const o = out[ch];
      let h = this.hold[ch] || 0;
      n = this.n;
      for (let k = 0; k < o.length; k++, n++) {
        if (n % f === 0) h = step * Math.round(i[k] / step);
        o[k] = h;
      }
      this.hold[ch] = h;
    }
    this.n = n;
    return true;
  }
}
registerProcessor("selecta-crusher", Crusher);
