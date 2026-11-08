import * as React from "react";
import type { SetTrack } from "@selecta/protocol";
import { Button } from "@/components/dsgn/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@/components/dsgn/dropdown-menu";
import { Input } from "@/components/dsgn/input";
import { Knob } from "@/components/studio/knob";
import { Panel } from "@/components/studio/panel";
import { onFrame } from "@/lib/frame";
import { hintProps } from "@/lib/hint";
import { getConductor, useSet } from "@/lib/set-store";
import { cn } from "@/lib/utils";

const CHIPS: [string, string][] = [
  ["Crazy DnB", "crazy DnB set, double drops"],
  ["Friends over: Bollywood", "my friends are coming, make a bollywood party set"],
  ["Seedhe Maut", "hip hop set, Seedhe Maut and desi hip hop"],
  ["Punjabi bangers", "punjabi party bangers"],
  ["House party", "house party, groovy tech house"],
  ["Liquid sunset", "chill liquid drum and bass for sunset"],
];

const KIND_LABEL: Record<string, string> = {
  start: "",
  blend: "BLEND",
  bassswap: "BASS SWAP",
  double_drop: "DOUBLE DROP",
  filter: "FILTER",
  echo_out: "ECHO OUT",
  slam: "SLAM",
};

const fmt = (s: number) => `${Math.floor(s / 60)}:${String(Math.round(s % 60)).padStart(2, "0")}`;

/**
 * The set: FL's Playlist idea applied to a DJ set. Describe it in the
 * composer; the plan appears as clips on one timeline, sized by play time and
 * coloured by the deck they'll play on, with each transition marked where
 * clips meet.
 */
export function SetPanel() {
  const { plan, planning, playing, index, error, recent } = useSet();
  const { build, play, stop, skip, open, refreshRecent } = useSet.getState();
  const [prompt, setPrompt] = React.useState("");
  const [minutes, setMinutes] = React.useState(0);
  const [chaos, setChaos] = React.useState(-0.05);

  React.useEffect(() => {
    void refreshRecent();
  }, [refreshRecent]);

  const opts = () => ({ ...(minutes > 0 ? { durationMin: Math.round(minutes) } : {}), ...(chaos >= 0 ? { chaos } : {}) });
  const submit = (reroll = false) => void build(prompt || plan?.prompt || "", opts(), reroll);

  return (
    <Panel
      title="Set"
      tools={
        <DropdownMenu onOpenChange={(o) => o && void refreshRecent()}>
          <DropdownMenuTrigger className="rounded-sm px-1.5 text-[11px] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring" {...hintProps({ label: "Recent sets" })}>
            Recent ▾
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="max-h-80 overflow-y-auto">
            <DropdownMenuLabel>Recent sets</DropdownMenuLabel>
            {recent.length === 0 && <DropdownMenuItem disabled>No sets yet</DropdownMenuItem>}
            {recent.map((s) => (
              <DropdownMenuItem key={s.id} onSelect={() => void open(s.id)}>
                <span className="truncate">{s.title}</span>
                <span className="ml-auto pl-3 font-mono text-[10px] text-muted-foreground">{s.tracks} trk</span>
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      }
      bodyClassName="flex flex-col gap-1.5 p-1.5"
    >
      <div className="flex items-center gap-2">
        <form
          className="flex min-w-0 flex-1 gap-1"
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
        >
          <Input
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder="Describe the set: crazy DnB, friends over for Bollywood, Seedhe Maut takeover…"
            aria-label="Describe the set"
            className="h-8 text-[13px]"
            {...hintProps({ label: "Describe the set", value: "Genre, artists, mood, length", keys: "Enter" })}
          />
          <Button type="submit" size="xs" variant="accent" className="h-8 px-4" disabled={planning} {...hintProps({ label: "Build the set", keys: "Enter" })}>
            {planning ? "Building…" : "Build"}
          </Button>
        </form>
        <Knob caption="Length" size="sm" label="Set length" min={0} max={120} defaultValue={0} value={minutes} onChange={setMinutes} format={(v) => (v < 2.5 ? "auto (from prompt)" : `${Math.round(v)} min`)} />
        <Knob caption="Chaos" size="sm" label="Chaos" min={-0.05} max={1} defaultValue={-0.05} value={chaos} onChange={setChaos} format={(v) => (v < 0 ? "auto (from prompt)" : `${Math.round(v * 100)}%: ${v > 0.7 ? "slams and double drops" : v > 0.35 ? "mixed" : "smooth blends"}`)} />
      </div>
      <div className="flex flex-wrap gap-1">
        {CHIPS.map(([label, p]) => (
          <button key={label} type="button" onClick={() => setPrompt(p)} className="rounded-sm border border-border bg-secondary px-2 py-0.5 text-[11px] text-muted-foreground hover:border-accent hover:text-foreground" {...hintProps({ label: "Use this prompt", value: p })}>
            {label}
          </button>
        ))}
      </div>

      {error && <p role="alert" className="text-[11px] text-destructive">{error}</p>}

      {plan ? (
        <>
          <div className="flex items-center gap-2">
            <span className="font-display text-[15px] font-semibold tracking-wide">{plan.title}</span>
            <span className="font-mono text-[11px] text-muted-foreground">
              {plan.tracks.length} tracks · {fmt(plan.estTotalSec)} · {plan.intent.energy} · chaos {Math.round(plan.intent.chaos * 100)}%
            </span>
            <div className="ml-auto flex gap-1">
              {playing ? (
                <>
                  <Button size="xs" variant="secondary" onClick={skip} {...hintProps({ label: "Next track", value: "Echo out on the next bar", keys: "N" })}>
                    Next
                  </Button>
                  <Button size="xs" variant="secondary" onClick={stop} {...hintProps({ label: "Stop the set" })}>
                    Stop
                  </Button>
                </>
              ) : (
                <>
                  <Button size="xs" variant="ghost" onClick={() => submit(true)} disabled={planning} {...hintProps({ label: "Re-roll", value: "Same prompt, different choices" })}>
                    Re-roll
                  </Button>
                  <Button size="xs" variant="accent" onClick={() => void play()} {...hintProps({ label: "Play the set", value: "Takes over both decks" })}>
                    Play set
                  </Button>
                </>
              )}
            </div>
          </div>
          <Timeline tracks={plan.tracks} current={playing ? index : -1} />
          {plan.notes.length > 0 && <p className="truncate text-[11px] text-[var(--warm)]">{plan.notes.join(" ")}</p>}
        </>
      ) : (
        <p className="py-3 text-center text-[11px] text-[var(--ink-faint)]">Describe a set and press Build. Selecta picks and orders tracks from your library by genre, key, tempo and an energy arc, then performs the transitions.</p>
      )}
    </Panel>
  );
}

function Timeline({ tracks, current }: { tracks: SetTrack[]; current: number }) {
  const total = tracks.reduce((s, t) => s + Math.max(20, t.estSec), 0);
  const head = React.useRef<HTMLDivElement>(null);
  const clips = React.useRef<HTMLDivElement>(null);

  React.useEffect(
    () =>
      onFrame(() => {
        const el = head.current;
        const c = getConductor();
        const deck = c?.leadDeck;
        if (!el || !c?.running || !deck?.track || current < 0) {
          if (el) el.style.opacity = "0";
          return;
        }
        const t = tracks[current];
        if (!t) return;
        const before = tracks.slice(0, current).reduce((s, x) => s + Math.max(20, x.estSec), 0);
        const span = Math.max(1, t.outBar - t.inBar);
        const frac = Math.min(1, Math.max(0, (deck.bar() - t.inBar) / span));
        el.style.opacity = "1";
        el.style.left = `${((before + frac * Math.max(20, t.estSec)) / total) * 100}%`;
      }),
    [tracks, current, total],
  );

  return (
    <div ref={clips} className="relative flex h-14 gap-px overflow-hidden rounded-sm bg-muted p-px" role="list" aria-label="Set timeline">
      {tracks.map((t, i) => {
        const deck = i % 2 === 0 ? "var(--cyan)" : "var(--magenta)";
        return (
          <div
            key={`${t.id}-${i}`}
            role="listitem"
            className={cn("relative flex min-w-0 flex-col justify-between overflow-hidden rounded-[2px] px-1.5 py-1", i === current ? "ring-1 ring-accent" : "")}
            style={{ flex: `${Math.max(20, t.estSec)} 1 0`, background: `color-mix(in srgb, ${deck} ${i === current ? 32 : 18}%, var(--card))`, borderTop: `2px solid ${deck}` }}
            {...hintProps({
              label: `${i + 1}. ${t.artist} – ${t.title}`,
              value: `${t.genreLabel || "?"} · ${t.bpm.toFixed(1)} BPM${t.rate !== 1 ? ` → ${(t.bpm * t.rate).toFixed(1)}` : ""} · ${t.camelot} · bars ${t.inBar}–${t.outBar} · ${fmt(t.estSec)}${t.trans.kind !== "start" ? ` · in by ${t.trans.kind.replace("_", " ")}` : ""}`,
            })}
          >
            <span className="truncate text-[11px] font-medium text-foreground">{t.title}</span>
            <span className="flex items-center gap-1 truncate font-mono text-[9.5px] text-muted-foreground">
              {(t.bpm * t.rate).toFixed(0)} · {t.camelot}
              {t.trans.kind !== "start" && <span className="truncate rounded-[2px] bg-background/60 px-1 font-display text-[9px] tracking-wider text-foreground">{KIND_LABEL[t.trans.kind]}</span>}
            </span>
            <span aria-hidden className="absolute inset-x-0 bottom-0 h-0.5" style={{ background: `linear-gradient(90deg, transparent, ${deck} ${t.energy * 100}%, transparent ${t.energy * 100}%)` }} />
          </div>
        );
      })}
      <div ref={head} aria-hidden className="pointer-events-none absolute inset-y-0 w-0.5 bg-foreground opacity-0" />
    </div>
  );
}
