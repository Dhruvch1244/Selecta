import * as React from "react";
import { fire, type Deck, type DeckId } from "@selecta/engine";
import { Button } from "@/components/dsgn/button";
import { Panel } from "@/components/studio/panel";
import { DetailWave, OverviewWave } from "@/components/studio/waveform";
import { onFrame } from "@/lib/frame";
import { hintProps, useHintStore } from "@/lib/hint";
import { cn } from "@/lib/utils";
import { getEngine, useApp } from "@/lib/store";

export const DECK_COLOR: Record<DeckId, string> = { A: "var(--cyan)", B: "var(--magenta)" };
const KEYS: Record<DeckId, { play: string; cue: string; sync: string }> = {
  A: { play: "Q", cue: "A", sync: "Z" },
  B: { play: "W", cue: "S", sync: "X" },
};

const fmt = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

/** One deck as a playlist lane: transport, live readouts, detail + overview waveforms. Drop a track on it to load. */
export function DeckLane({ id }: { id: DeckId }) {
  const view = useApp((s) => s.decks[id]);
  const tracks = useApp((s) => s.tracks);
  const { togglePlay, cue, sync, load } = useApp.getState();
  const deck = getEngine().decks[id];
  const color = DECK_COLOR[id];
  const readout = React.useRef<HTMLSpanElement>(null);
  const [over, setOver] = React.useState(false);

  React.useEffect(
    () =>
      onFrame(() => {
        const el = readout.current;
        const tr = deck.track;
        if (!el || !tr) return;
        const t = deck.heardTime();
        const pos = deck.position(t);
        el.textContent = `${fmt(pos)} / -${fmt(Math.max(0, (tr.analysis.duration - pos) / deck.rate))} · bar ${Math.floor(deck.bar(t))}`;
      }),
    [deck],
  );

  const a = view.analysis;
  const pct = (view.rate - 1) * 100;
  return (
    <Panel
      title={`Deck ${id}`}
      swatch={color}
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes("application/x-selecta-track")) {
          e.preventDefault();
          setOver(true);
        }
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        setOver(false);
        const t = tracks.find((x) => x.id === e.dataTransfer.getData("application/x-selecta-track"));
        if (t) void load(id, t);
      }}
      className={over ? "border-accent shadow-glow" : undefined}
      tools={
        a && (
          <span className="font-mono text-[11px] tabular-nums text-muted-foreground">
            <span className="text-foreground">{(a.bpm * view.rate).toFixed(2)}</span> BPM
            {Math.abs(pct) > 0.005 && <span className={pct > 0 ? "text-[var(--warm)]" : "text-[var(--cyan)]"}> {pct > 0 ? "+" : ""}{pct.toFixed(2)}%</span>}
            {" · "}
            <span className="text-[var(--cyan)]">{a.camelot}</span> {a.key}
            {a.gridConf < 2 && <span className="text-[var(--warm)]" {...hintProps({ label: "Unsteady beat grid", value: `confidence ${a.gridConf.toFixed(1)}: beatmatching may drift` })}> · grid?</span>}
          </span>
        )
      }
      bodyClassName="flex flex-col gap-1.5 p-1.5"
    >
      <div className="flex items-center gap-2">
        <div className="flex gap-1">
          <Button size="xs" variant={view.playing ? "accent" : "secondary"} className="w-14" disabled={!a} onClick={() => void togglePlay(id)} {...hintProps({ label: view.playing ? `Pause deck ${id}` : `Play deck ${id}`, keys: KEYS[id].play })}>
            {view.playing ? "Pause" : "Play"}
          </Button>
          <Button size="xs" variant="secondary" disabled={!a} onClick={() => cue(id)} {...hintProps({ label: `Cue deck ${id}`, value: "Back to the first bar with music", keys: KEYS[id].cue })}>
            Cue
          </Button>
          <Button size="xs" variant="secondary" disabled={!a} onClick={() => sync(id)} {...hintProps({ label: `Sync deck ${id}`, value: "Match tempo and land on the other deck's next downbeat", keys: KEYS[id].sync })}>
            Sync
          </Button>
        </div>
        <div className="min-w-0 flex-1 truncate">
          {view.trackId ? (
            <>
              <span className="font-medium text-foreground">{view.title}</span>
              <span className="text-muted-foreground"> · {view.artist}</span>
            </>
          ) : (
            <span className="text-[var(--ink-faint)]">Drop a track here, or double-click one in the browser</span>
          )}
        </div>
        <span ref={readout} className="font-mono text-[11px] tabular-nums text-muted-foreground" />
      </div>
      <PerformRow deck={deck} color={color} enabled={!!a} />
      <div className="relative min-h-0 flex-1">
        <DetailWave deck={deck} color={color} className="h-full" />
        {view.loading && <Overlay>Analysing and decoding…</Overlay>}
        {view.error && <Overlay tone="error">{view.error}</Overlay>}
      </div>
      <OverviewWave deck={deck} color={color} className="h-6" onSeek={(pos) => deck.seek(pos)} />
    </Panel>
  );
}

const LOOPS: [string, number][] = [["½", 0.5], ["1", 1], ["2", 2], ["4", 4], ["8", 8], ["16", 16]];

/**
 * Performance controls, as on a CDJ/Mixxx deck: beat loops, beat jumps and
 * the turntable moves. Everything quantises to the next beat, so a hand
 * move lands like a planned one. Loops are native buffer loops on the grid,
 * so a looped deck stays locked to the other one.
 */
function PerformRow({ deck, color, enabled }: { deck: Deck; color: string; enabled: boolean }) {
  const [loop, setLoop] = React.useState<number | null>(null);
  React.useEffect(
    () =>
      onFrame(() => {
        const b = deck.loop?.beats ?? null;
        setLoop((p) => (p === b ? p : b));
      }),
    [deck],
  );
  const busy = (msg: string) => useHintStore.getState().set({ label: msg, value: "Wait for the current move to finish" });
  const move = (id: string, beats: number, jumpTo?: number) => {
    if (!deck.playing) return useHintStore.getState().set({ label: "Play the deck first" });
    const eng = getEngine();
    const at = deck.nextBeat(1, eng.ctx.currentTime + 0.03);
    if (!fire({ ctx: eng.ctx, master: eng.fxBus }, id, { deck, at, beats, ...(jumpTo !== undefined ? { jumpTo } : {}) })) busy("Deck busy");
  };
  // "Pull up": back to the build before the last drop, so the drop comes round again.
  const rewind = () => {
    const a = deck.track?.analysis;
    if (!a) return;
    const bar = Math.floor(deck.bar());
    const drop = [...a.drops].reverse().find((d) => d <= bar - 2);
    move("rewind:8", 8, drop !== undefined ? Math.max(a.startBar, drop - 8) : Math.max(a.startBar, bar - 16));
  };
  const pad = "h-6 rounded-[2px] border px-1.5 font-mono text-[10px] tracking-wide transition-colors duration-100 disabled:opacity-30 active:translate-y-px";
  return (
    <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label={`Deck ${deck.id} performance`}>
      <span className="font-display text-[10px] uppercase tracking-wider text-[var(--ink-faint)]">Loop</span>
      <div className="flex gap-0.5">
        {LOOPS.map(([label, beats]) => {
          const on = loop === beats;
          return (
            <button
              key={label}
              type="button"
              disabled={!enabled}
              aria-pressed={on}
              onClick={() => (deck.setLoop(on ? null : beats) ? undefined : busy(on ? "Can't exit now" : "Can't loop now"))}
              {...hintProps({ label: on ? `Release the ${label}-beat loop` : `Loop ${label} beat${beats === 1 ? "" : "s"}`, value: "Starts on the next beat; locked to the grid" })}
              className={cn(pad, "min-w-7", on ? "border-transparent text-background" : "border-border bg-secondary text-foreground hover:border-[var(--ink-faint)]")}
              style={on ? { background: color } : undefined}
            >
              {label}
            </button>
          );
        })}
      </div>
      <span className="ml-1 font-display text-[10px] uppercase tracking-wider text-[var(--ink-faint)]">Jump</span>
      <div className="flex gap-0.5">
        <button type="button" disabled={!enabled} onClick={() => deck.beatJump(-4)} className={cn(pad, "border-border bg-secondary")} {...hintProps({ label: "Jump back 4 bars", value: "On the next beat, in phase" })}>
          −4
        </button>
        <button type="button" disabled={!enabled} onClick={() => deck.beatJump(4)} className={cn(pad, "border-border bg-secondary")} {...hintProps({ label: "Jump ahead 4 bars", value: "On the next beat, in phase" })}>
          +4
        </button>
      </div>
      <div className="ml-auto flex gap-0.5">
        <button type="button" disabled={!enabled} onClick={rewind} className={cn(pad, "border-accent/60 bg-accent/15 text-accent hover:bg-accent/25")} {...hintProps({ label: "Rewind (pull up!)", value: "Wheels the track back and drops in again at the build before the last drop" })}>
          REWIND
        </button>
        <button type="button" disabled={!enabled} onClick={() => move("backspin:2", 2)} className={cn(pad, "border-border bg-secondary")} {...hintProps({ label: "Backspin", value: "Spins back 2 beats, then plays on in place" })}>
          SPIN
        </button>
        <button type="button" disabled={!enabled} onClick={() => move("tape_stop:2", 2)} className={cn(pad, "border-border bg-secondary")} {...hintProps({ label: "Brake", value: "Turntable power-down over 2 beats, then plays on in place" })}>
          BRAKE
        </button>
        <button type="button" disabled={!enabled} onClick={() => move("echo:1/2", 4)} className={cn(pad, "border-border bg-secondary")} {...hintProps({ label: "Echo throw", value: "Half-beat echo on the last beat of the phrase" })}>
          ECHO
        </button>
      </div>
    </div>
  );
}

function Overlay({ children, tone }: { children: React.ReactNode; tone?: "error" }) {
  return (
    <div role={tone === "error" ? "alert" : "status"} className="absolute inset-0 flex items-center justify-center rounded-sm bg-background/70 text-[12px]">
      <span className={tone === "error" ? "text-destructive" : "text-muted-foreground"}>{children}</span>
    </div>
  );
}
