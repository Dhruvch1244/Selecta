import * as React from "react";
import type { LayerDeckId } from "@selecta/engine";
import { onFrame } from "@/lib/frame";
import { hintProps } from "@/lib/hint";
import { useSet } from "@/lib/set-store";
import { getEngine } from "@/lib/store";
import { cn } from "@/lib/utils";

export const LAYER_COLOR: Record<LayerDeckId, string> = { C: "var(--violet)", D: "var(--warm)" };
const KIND_LABEL = { callback: "Callback", tease: "Tease", anthem: "Anthem", tool: "Tool" } as const;

/**
 * The two loop decks under A/B: what each is layering (whose hook or riff,
 * and why), a 16-step cursor on its 4-bar loop, bars left, and a mute.
 * Idle decks stay as thin, quiet rows so the layout never jumps.
 */
export function LayerStrip() {
  return (
    <div className="grid grid-cols-2 gap-1.5">
      <LayerSlot id="C" index={0} />
      <LayerSlot id="D" index={1} />
    </div>
  );
}

function LayerSlot({ id, index }: { id: LayerDeckId; index: 0 | 1 }) {
  const slot = useSet((s) => s.layers[id]);
  const deck = getEngine().layers[index];
  const steps = React.useRef<HTMLDivElement>(null);
  const left = React.useRef<HTMLSpanElement>(null);
  const [muted, setMuted] = React.useState(false);
  const color = LAYER_COLOR[id];

  React.useEffect(
    () =>
      onFrame(() => {
        if (!useSet.getState().layers[id]) return;
        const t = deck.heardTime();
        // Ended (or the set stopped): free the slot.
        if (!deck.playing && t > deck.stopsAt) {
          useSet.setState((s) => ({ layers: { ...s.layers, [id]: undefined } }));
          return;
        }
        const lp = deck.loop;
        if (!deck.playing || !lp) return; // booked, not sounding yet
        const step = Math.floor(((deck.position(t) - lp.a) / lp.len) * 16) % 16;
        const cells = steps.current?.children;
        if (cells) for (let i = 0; i < cells.length; i++) (cells[i] as HTMLElement).dataset.on = String(i === step);
        const barSec = (4 * (lp.len / lp.beats)) / deck.rate;
        if (left.current) left.current.textContent = Number.isFinite(deck.stopsAt) ? `${Math.ceil((deck.stopsAt - t) / barSec)} bars` : "";
      }),
    [deck, id],
  );
  // A new layer starts unmuted (the conductor resets the deck's mix).
  React.useEffect(() => setMuted(false), [slot]);

  const toggleMute = () => {
    const m = !muted;
    setMuted(m);
    deck.setFader(m ? 0 : 0.95);
  };

  const l = slot?.layer;
  return (
    <div
      className={cn(
        "flex h-9 min-w-0 items-center gap-2 rounded-md border border-border bg-card px-2 shadow-ambient",
        !l && "opacity-60",
      )}
      {...hintProps({ label: `Loop deck ${id}`, value: l ? l.reason : "Idle: the set layers other songs' hooks and riffs here" })}
    >
      <span aria-hidden className="h-4 w-1 shrink-0 rounded-sm" style={{ background: color }} />
      <span className="shrink-0 whitespace-nowrap font-display text-[12px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">Loop {id}</span>
      {l ? (
        <>
          <span className="shrink-0 rounded-sm border border-border px-1 font-display text-[10px] uppercase tracking-wider" style={{ color }}>
            {KIND_LABEL[l.kind]} · {l.part === "vocal" ? "Vocal" : "Riff"}
          </span>
          <span className="min-w-0 flex-1 truncate text-[12px] text-foreground">
            {l.title}
          </span>
          <div ref={steps} className="flex shrink-0 gap-px" aria-hidden>
            {Array.from({ length: 16 }, (_, i) => (
              <span
                key={i}
                className={cn("h-3 w-1 rounded-[1px] bg-muted data-[on=true]:bg-[var(--slot)]", i % 4 === 0 && "h-3.5")}
                style={{ ["--slot" as string]: color }}
              />
            ))}
          </div>
          <span ref={left} className="shrink-0 text-right font-mono text-[11px] tabular-nums text-muted-foreground" />
          <button
            type="button"
            onClick={toggleMute}
            aria-pressed={muted}
            className={cn(
              "h-6 shrink-0 rounded-sm border border-border px-1.5 font-display text-[10px] uppercase tracking-wider",
              muted ? "bg-destructive text-destructive-foreground" : "bg-secondary text-muted-foreground hover:text-foreground",
            )}
            {...hintProps({ label: muted ? "Unmute this loop" : "Mute this loop" })}
          >
            {muted ? "Muted" : "Mute"}
          </button>
        </>
      ) : (
        <span className="truncate text-[12px] text-muted-foreground">idle</span>
      )}
    </div>
  );
}
