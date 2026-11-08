// One requestAnimationFrame loop for the whole UI. Meters, waveforms and
// readouts subscribe here instead of each running its own rAF, so frame
// work is batched, ordered, and stops entirely when nothing is listening.

type Tick = (nowMs: number, dtMs: number) => void;
const subs = new Set<Tick>();
let raf = 0;
let last = 0;

function loop(now: number) {
  const dt = last ? Math.min(100, now - last) : 16;
  last = now;
  for (const s of subs) s(now, dt);
  raf = subs.size ? requestAnimationFrame(loop) : 0;
  if (!raf) last = 0;
}

export function onFrame(tick: Tick): () => void {
  subs.add(tick);
  if (!raf) raf = requestAnimationFrame(loop);
  return () => {
    subs.delete(tick);
  };
}
