import * as React from "react";
import { fire, type DeckId } from "@selecta/engine";
import catalog from "@selecta/protocol/fx-catalog.json";
import { Panel } from "@/components/studio/panel";
import { hintProps, useHintStore } from "@/lib/hint";
import { getEngine, useApp } from "@/lib/store";
import { useSet } from "@/lib/set-store";
import { cn } from "@/lib/utils";

const CATEGORIES = ["build", "hit", "horn", "roll", "echo", "space", "filter", "gate", "mod", "dist", "tape"] as const;
const QUANT: [string, number][] = [["Beat", 1], ["Bar", 4], ["Off", 0]];

/**
 * The FX pad: every effect the director can use, playable live. Pads are
 * quantised to the playing deck's grid (beat or bar) so a hand-fired roll
 * lands like a scripted one. Targets the deck you pick, or the one leading
 * the mix.
 */
export function FxPad() {
  const [cat, setCat] = React.useState<(typeof CATEGORIES)[number]>("build");
  const [target, setTarget] = React.useState<"lead" | DeckId>("lead");
  const [quant, setQuant] = React.useState(1);
  const [flash, setFlash] = React.useState<string | null>(null);
  const lead = useSet((s) => s.leadDeck);
  const decks = useApp((s) => s.decks);

  const play = (id: string, beats: number) => {
    const eng = getEngine();
    const want: DeckId | null = target === "lead" ? (lead ?? (decks.A.playing ? "A" : decks.B.playing ? "B" : null)) : target;
    const deck = want ? eng.decks[want] : null;
    if (!deck?.track || !deck.playing) {
      useHintStore.getState().set({ label: "Nothing playing on that deck", value: "Play a deck or a set first" });
      return;
    }
    const at = quant ? deck.nextBeat(quant, eng.ctx.currentTime + 0.03) : eng.ctx.currentTime + 0.03;
    const ok = fire({ ctx: eng.ctx, master: eng.fxBus }, id, { deck, at, beats });
    setFlash(id);
    setTimeout(() => setFlash((f) => (f === id ? null : f)), 220);
    if (!ok) useHintStore.getState().set({ label: "Deck busy", value: "Another loop or turntable move is still running" });
  };

  const items = catalog.filter((c) => c.category === cat);
  return (
    <Panel
      title="FX"
      tools={
        <div className="flex items-center gap-2 text-[11px]">
          <Segmented label="Target" value={target} options={[["Lead", "lead"], ["A", "A"], ["B", "B"]]} onChange={(v) => setTarget(v as typeof target)} />
          <Segmented label="Quantise" value={String(quant)} options={QUANT.map(([l, v]) => [l, String(v)])} onChange={(v) => setQuant(Number(v))} />
        </div>
      }
      bodyClassName="flex flex-col gap-1.5 p-1.5"
    >
      <div className="flex flex-wrap gap-1" role="tablist" aria-label="Effect categories">
        {CATEGORIES.map((c) => (
          <button
            key={c}
            type="button"
            role="tab"
            aria-selected={c === cat}
            onClick={() => setCat(c)}
            className={cn("rounded-sm border px-2 py-0.5 font-display text-[11px] uppercase tracking-wider", c === cat ? "border-accent bg-accent text-accent-foreground" : "border-border bg-secondary text-muted-foreground hover:text-foreground")}
          >
            {c} <span className="opacity-60">{catalog.filter((x) => x.category === c).length}</span>
          </button>
        ))}
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(104px,1fr))] gap-1">
        {items.map((f) => (
          <button
            key={f.id}
            type="button"
            onClick={() => play(f.id, f.beats)}
            {...hintProps({ label: f.label, value: `${f.beats} beat${f.beats === 1 ? "" : "s"}${f.target === "master" ? " · synth over the mix" : ""} · quantised to ${quant === 4 ? "the bar" : quant ? "the beat" : "nothing"}` })}
            className={cn(
              "h-11 rounded-sm border border-border bg-secondary px-1.5 text-left shadow-ambient transition-colors duration-100 active:translate-y-px",
              flash === f.id ? "border-accent bg-accent/25" : "hover:border-[var(--ink-faint)]",
            )}
          >
            <span className="block truncate text-[11.5px] font-medium text-foreground">{f.label}</span>
            <span className="block font-mono text-[9.5px] text-muted-foreground">{f.beats}b{f.target === "master" ? " · synth" : ""}</span>
          </button>
        ))}
      </div>
    </Panel>
  );
}

function Segmented({ label, value, options, onChange }: { label: string; value: string; options: [string, string][]; onChange: (v: string) => void }) {
  return (
    <div className="flex items-center gap-1" role="radiogroup" aria-label={label}>
      <span className="font-display uppercase tracking-wider text-[var(--ink-faint)]">{label}</span>
      <div className="flex overflow-hidden rounded-sm border border-border">
        {options.map(([l, v]) => (
          <button key={v} type="button" role="radio" aria-checked={value === v} onClick={() => onChange(v)} className={cn("px-1.5 py-px font-mono text-[10px]", value === v ? "bg-accent text-accent-foreground" : "bg-muted text-muted-foreground hover:text-foreground")}>
            {l}
          </button>
        ))}
      </div>
    </div>
  );
}
