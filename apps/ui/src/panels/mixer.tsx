import * as React from "react";
import { EQ_KILL_DB, type Band, type DeckId } from "@selecta/engine";
import { Fader } from "@/components/studio/fader";
import { Knob } from "@/components/studio/knob";
import { Meter } from "@/components/studio/meter";
import { Panel } from "@/components/studio/panel";
import { getEngine, peekEngine } from "@/lib/store";
import { DECK_COLOR } from "./deck-lane";

const db = (v: number) => (v <= EQ_KILL_DB ? "kill" : `${v > 0 ? "+" : ""}${v.toFixed(1)} dB`);

/**
 * Mixer: one strip per deck plus master, left to right in signal order
 * (trim, EQ, filter, fader). Values are local UI state pushed into the
 * engine; the engine stays the source of truth for audio.
 */
export function Mixer() {
  return (
    <Panel title="Mixer" bodyClassName="flex gap-1 p-1.5">
      <Strip id="A" />
      <Strip id="B" />
      <MasterStrip />
    </Panel>
  );
}

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
    <div className="flex w-16 flex-col items-center gap-1 rounded-sm border border-border bg-[var(--glass)] py-1.5">
      <span className="font-display text-[12px] font-semibold tracking-wider" style={{ color }}>{id}</span>
      <Knob caption="Trim" size="sm" color={color} label={`Deck ${id} trim`} min={-12} max={12} defaultValue={0} value={trim} bipolar format={db} onChange={(v) => { setTrim(v); deck.setTrimDb(v); }} />
      {band("high", "Hi")}
      {band("mid", "Mid")}
      {band("low", "Low")}
      <Knob
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
      />
      <div className="flex min-h-24 flex-1 items-stretch gap-1 pt-1">
        <Fader color={color} label={`Deck ${id} volume`} min={0} max={1} defaultValue={1} value={fader} onChange={(v) => { setFader(v); deck.setFader(v); }} />
        <Meter label={`Deck ${id}`} read={read} onResetClip={() => (deck.meter = { ...deck.meter, clipped: false })} />
      </div>
    </div>
  );
}

function MasterStrip() {
  const [vol, setVol] = React.useState(0.85);
  const read = React.useCallback(() => peekEngine()?.master ?? null, []);
  return (
    <div className="flex w-14 flex-col items-center gap-1 rounded-sm border border-border bg-[var(--glass)] py-1.5">
      <span className="font-display text-[12px] font-semibold tracking-wider text-accent">MST</span>
      <div className="flex min-h-24 flex-1 items-stretch gap-1 pt-1">
        <Fader label="Master volume" min={0} max={1} defaultValue={0.85} value={vol} onChange={(v) => { setVol(v); getEngine().setVolume(v); }} />
        <Meter label="Master" read={read} onResetClip={() => { const e = peekEngine(); if (e) e.master = { ...e.master, clipped: false }; }} />
      </div>
    </div>
  );
}
