import * as React from "react";
import type { DeckId } from "@selecta/engine";
import { Button } from "@/components/dsgn/button";
import { Panel } from "@/components/studio/panel";
import { DetailWave, OverviewWave } from "@/components/studio/waveform";
import { onFrame } from "@/lib/frame";
import { hintProps } from "@/lib/hint";
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
        const pos = deck.position();
        el.textContent = `${fmt(pos)} / -${fmt(Math.max(0, (tr.analysis.duration - pos) / deck.rate))} · bar ${Math.floor(deck.bar())}`;
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
      <div className="relative min-h-0 flex-1">
        <DetailWave deck={deck} color={color} className="h-full" />
        {view.loading && <Overlay>Analysing and decoding…</Overlay>}
        {view.error && <Overlay tone="error">{view.error}</Overlay>}
      </div>
      <OverviewWave deck={deck} color={color} className="h-6" onSeek={(pos) => deck.seek(pos)} />
    </Panel>
  );
}

function Overlay({ children, tone }: { children: React.ReactNode; tone?: "error" }) {
  return (
    <div role={tone === "error" ? "alert" : "status"} className="absolute inset-0 flex items-center justify-center rounded-sm bg-background/70 text-[12px]">
      <span className={tone === "error" ? "text-destructive" : "text-muted-foreground"}>{children}</span>
    </div>
  );
}
