import * as React from "react";
import { EQ_KILL_DB, type Band, type DeckId, type Stem } from "@selecta/engine";
import { Fader } from "@/components/studio/fader";
import { Knob } from "@/components/studio/knob";
import { Lcd } from "@/components/studio/lcd";
import { Meter } from "@/components/studio/meter";
import { Panel } from "@/components/studio/panel";
import { onFrame } from "@/lib/frame";
import { hintProps } from "@/lib/hint";
import { getEngine, peekEngine } from "@/lib/store";
import { cn } from "@/lib/utils";
import { DECK_COLOR } from "./deck-lane";

const db = (v: number) => (v <= EQ_KILL_DB ? "kill" : `${v > 0 ? "+" : ""}${v.toFixed(1)} dB`);

/**
 * Mixer: one strip per deck plus master, left to right in signal order
 * (trim, EQ, filter, fader). Values are local UI state pushed into the
 * engine; the engine stays the source of truth for audio.
 */
export function Mixer() {
  // One grid for the whole console, flowing by column: 8 shared rows
  // (name, trim, hi, mid, low, filter, stems, fader). Every control sits on
  // the same row as its neighbour's, and all three faders start and end level
  // (the master's readouts span the knob rows).
  return (
    <Panel title="Mixer" bodyClassName="p-1.5">
      <div className="grid h-full grid-flow-col grid-cols-[repeat(2,4rem)_3.5rem] grid-rows-[repeat(7,auto)_minmax(6rem,1fr)] justify-center gap-x-1 gap-y-1">
        <Strip id="A" />
        <Strip id="B" />
        <MasterStrip />
      </div>
    </Panel>
  );
}

/** A console cell: one row of one channel. */
const cell = "flex justify-center items-center";

function Strip({ id }: { id: DeckId }) {
  const color = DECK_COLOR[id];
  const deck = getEngine().decks[id];
  const [trim, setTrim] = React.useState(0);
  const [eq, setEq] = React.useState<Record<Band, number>>({ high: 0, mid: 0, low: 0 });
  const [filter, setFilter] = React.useState(0);
  const [fader, setFader] = React.useState(1);
  const read = React.useCallback(() => (deck.track ? deck.meter : null), [deck]);
  const band = (b: Band, label: string) => (
    <Knob
      caption={label}
      size="sm"
      bipolar
      color={color}
      label={`Deck ${id} ${label} EQ`}
      min={EQ_KILL_DB}
      max={6}
      defaultValue={0}
      value={eq[b]}
      format={db}
      onChange={(v) => {
        setEq((s) => ({ ...s, [b]: v }));
        deck.setEq(b, v);
      }}
    />
  );
  return (
    <>
      <div className={cn(cell, "rounded-t-sm border-t-2 bg-[var(--glass)] py-1")} style={{ borderColor: color }}>
        <span className="font-display text-[13px] font-semibold tracking-wider" style={{ color }}>{id}</span>
      </div>
      <div className={cell}><Knob caption="Trim" size="sm" color={color} label={`Deck ${id} trim`} min={-12} max={12} defaultValue={0} value={trim} bipolar format={db} onChange={(v) => { setTrim(v); deck.setTrimDb(v); }} /></div>
      <div className={cell}>{band("high", "Hi")}</div>
      <div className={cell}>{band("mid", "Mid")}</div>
      <div className={cell}>{band("low", "Low")}</div>
      <div className={cell}><Knob
        caption="Filter"
        size="sm"
        bipolar
        color={color}
        label={`Deck ${id} filter`}
        min={-1}
        max={1}
        defaultValue={0}
        value={filter}
        format={(v) => (Math.abs(v) < 0.02 ? "off" : v < 0 ? `low-pass ${Math.round(-v * 100)}%` : `high-pass ${Math.round(v * 100)}%`)}
        onChange={(v) => { setFilter(v); deck.setFilter(v); }}
      /></div>
      <div className={cell}><StemKills id={id} color={color} /></div>
      <div className="flex min-h-0 items-stretch justify-center gap-1 rounded-b-sm bg-[var(--glass)] py-1.5">
        <Fader color={color} label={`Deck ${id} volume`} min={0} max={1} defaultValue={1} value={fader} onChange={(v) => { setFader(v); deck.setFader(v); }} />
        <Meter label={`Deck ${id}`} read={read} onResetClip={() => (deck.meter = { ...deck.meter, clipped: false })} />
      </div>
    </>
  );
}

/**
 * Stem kills, as on stem-capable DJ mixers: mute the vocal or the
 * instrumental of this deck. They read the engine's real gains each frame,
 * so they also light up when a set's mashup moves automate them.
 */
function StemKills({ id, color }: { id: DeckId; color: string }) {
  const deck = getEngine().decks[id];
  const [st, setSt] = React.useState({ has: false, vox: 1, inst: 1 });
  React.useEffect(
    () =>
      onFrame(() => {
        const next = { has: deck.hasStems, vox: deck.stemLevel("vox"), inst: deck.stemLevel("inst") };
        setSt((p) => (p.has === next.has && Math.abs(p.vox - next.vox) < 0.02 && Math.abs(p.inst - next.inst) < 0.02 ? p : next));
      }),
    [deck],
  );
  const btn = (stem: Stem, label: string) => {
    const on = st[stem] > 0.5;
    return (
      <button
        type="button"
        aria-pressed={!on}
        aria-label={`Deck ${id} ${stem === "vox" ? "vocals" : "instrumental"} ${on ? "on" : "muted"}`}
        disabled={!st.has}
        onClick={() => deck.setStem(stem, on ? 0 : 1)}
        {...hintProps({
          label: stem === "vox" ? "Vocal stem" : "Instrumental stem",
          value: !st.has ? "No stems for this track yet (Settings → Stems)" : on ? "Click to mute it" : "Muted: click to bring it back",
        })}
        className={cn(
          "h-5 flex-1 rounded-[2px] border font-mono text-[9px] tracking-wider transition-colors duration-100 disabled:opacity-30",
          on ? "border-border bg-secondary text-foreground" : "border-transparent text-background",
        )}
        style={on ? undefined : { background: color }}
      >
        {label}
      </button>
    );
  };
  return (
    <div className="flex w-full gap-0.5 px-1" role="group" aria-label={`Deck ${id} stems`}>
      {btn("vox", "VOX")}
      {btn("inst", "INS")}
    </div>
  );
}

function MasterStrip() {
  const [vol, setVol] = React.useState(0.85);
  const read = React.useCallback(() => peekEngine()?.master ?? null, []);
  const gr = React.useRef<HTMLSpanElement>(null);
  const lat = React.useRef<HTMLSpanElement>(null);
  React.useEffect(
    () =>
      onFrame(() => {
        const e = peekEngine();
        if (!e) return;
        if (gr.current) gr.current.textContent = e.limiterActive ? `${e.limiterReductionDb <= -0.05 ? e.limiterReductionDb.toFixed(1) : "0.0"}` : "off";
        if (lat.current) lat.current.textContent = `${e.latencyMs}`;
      }),
    [],
  );
  return (
    <>
      <div className={cn(cell, "rounded-t-sm border-t-2 border-accent bg-[var(--glass)] py-1")}>
        <span className="font-display text-[13px] font-semibold tracking-wider text-accent">MST</span>
      </div>
      <div className="row-span-6 flex flex-col items-center justify-start gap-3 pt-1">
        <Lcd label="Lim dB" valueRef={gr} width="w-[3.25rem]" hint="Limiter gain reduction: how hard the -1 dBFS brickwall is working">--</Lcd>
        <Lcd label="Out ms" valueRef={lat} width="w-[3.25rem]" hint="Output delay the visuals allow for (Settings → Output to calibrate)">--</Lcd>
      </div>
      <div className="flex min-h-0 items-stretch justify-center gap-1 rounded-b-sm bg-[var(--glass)] py-1.5">
        <Fader label="Master volume" min={0} max={1} defaultValue={0.85} value={vol} onChange={(v) => { setVol(v); getEngine().setVolume(v); }} />
        <Meter label="Master" read={read} onResetClip={() => { const e = peekEngine(); if (e) e.master = { ...e.master, clipped: false }; }} />
      </div>
    </>
  );
}
