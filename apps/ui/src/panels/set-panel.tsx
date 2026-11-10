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
  ["Rapid-fire DnB", "hedex style rapid fire dnb, double drops"],
  ["Bollywood mashups", "bollywood mashups and bootlegs for friends"],
];

type StyleKey = "rapid" | "mashups" | "smooth" | "turntablism";
const STYLES: [StyleKey, string, string][] = [
  ["rapid", "Rapid fire", "Quick mixing: ~40 bars a tune, drop to drop, stacked double drops"],
  ["mashups", "Mashups", "Vocals over other beats: looped hooks, bootlegs, acapella builds"],
  ["smooth", "Smooth", "Long seamless blends, hardly any cuts"],
  ["turntablism", "Scratchy", "Rewinds, backspins, brakes and loop rolls"],
];

const KIND_LABEL: Record<string, string> = {
  start: "",
  blend: "BLEND",
  bassswap: "BASS SWAP",
  double_drop: "DOUBLE DROP",
  filter: "FILTER",
  echo_out: "ECHO OUT",
  slam: "SLAM",
  backspin: "BACKSPIN",
  tape_stop: "TAPE STOP",
  rewind: "REWIND",
  loop_roll: "LOOP ROLL",
  acapella_build: "ACAPELLA",
  bootleg: "BOOTLEG",
  mashup: "MASHUP",
};
/** Stem moves get the accent: they're the moments a set is remembered for. */
const STEM_KINDS = new Set(["acapella_build", "bootleg", "mashup"]);

const fmt = (s: number) => `${Math.floor(s / 60)}:${String(Math.round(s % 60)).padStart(2, "0")}`;

/**
 * The set: FL's Playlist idea applied to a DJ set. Describe it in the
 * composer; the plan appears as clips on one timeline, sized by play time and
 * coloured by the deck they'll play on, with each transition marked where
 * clips meet.
 */
export function SetPanel() {
  const { plan, planning, playing, index, error, recent, progress, draft } = useSet();
  const { build, play, stop, skip, open, refreshRecent } = useSet.getState();
  const [prompt, setPrompt] = React.useState("");
  const [minutes, setMinutes] = React.useState(0);
  const [chaos, setChaos] = React.useState(-0.05);

  React.useEffect(() => {
    void refreshRecent();
  }, [refreshRecent]);

  const [style, setStyle] = React.useState<Record<StyleKey, boolean>>({ rapid: false, mashups: false, smooth: false, turntablism: false });
  // Only switched-on styles are sent: off means "let the prompt decide".
  const opts = () => ({
    ...(minutes > 0 ? { durationMin: Math.round(minutes) } : {}),
    ...(chaos >= 0 ? { chaos } : {}),
    ...Object.fromEntries(Object.entries(style).filter(([, on]) => on)),
  });
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
      <div className="flex flex-wrap items-center gap-1" role="group" aria-label="Set style">
        <span className="font-display text-[10px] uppercase tracking-wider text-[var(--ink-faint)]">Style</span>
        {STYLES.map(([key, label, hint]) => (
          <button
            key={key}
            type="button"
            aria-pressed={style[key]}
            onClick={() => setStyle((s) => ({ ...s, [key]: !s[key], ...(key === "smooth" && !s.smooth ? { rapid: false, turntablism: false } : {}), ...((key === "rapid" || key === "turntablism") && !s[key] ? { smooth: false } : {}) }))}
            className={cn(
              "rounded-sm border px-2 py-0.5 font-display text-[11px] uppercase tracking-wider",
              style[key] ? "border-accent bg-accent text-accent-foreground" : "border-border bg-secondary text-muted-foreground hover:text-foreground",
            )}
            {...hintProps({ label, value: `${hint}${style[key] ? " · on" : " · off: the prompt decides"}` })}
          >
            {label}
          </button>
        ))}
      </div>
      <div className="flex flex-wrap gap-1">
        {CHIPS.map(([label, p]) => (
          <button key={label} type="button" onClick={() => setPrompt(p)} className="rounded-sm border border-border bg-secondary px-2 py-0.5 text-[11px] text-muted-foreground hover:border-accent hover:text-foreground" {...hintProps({ label: "Use this prompt", value: p })}>
            {label}
          </button>
        ))}
      </div>

      {error && <p role="alert" className="text-[11px] text-destructive">{error}</p>}
      {planning && (
        <p role="status" className="flex items-center gap-2 font-mono text-[11px] text-muted-foreground">
          <span className="inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-accent" aria-hidden />
          {progress.at(-1) ?? "Starting…"}
        </p>
      )}

      {plan ? (
        <>
          <div className="flex items-center gap-2">
            <span className="font-display text-[15px] font-semibold tracking-wide">{plan.title}</span>
            {plan.curated && (
              <span className="rounded-sm border border-accent/50 px-1.5 font-display text-[10px] uppercase tracking-wider text-accent" {...hintProps({ label: "Curated by Claude", value: "Tracks scored, vetoed and ordered from real song knowledge" })}>
                AI curated
              </span>
            )}
            {draft && (
              <span
                className="animate-pulse rounded-sm border border-border px-1.5 font-display text-[10px] uppercase tracking-wider text-muted-foreground"
                {...hintProps({ label: "Instant draft", value: "Planned by the rules in a moment. Press Play now: Claude's curated order takes over from the next track when it's ready." })}
              >
                Draft · AI curating
              </span>
            )}
            {plan.intent.rapid && (
              <span className="rounded-sm border border-[var(--magenta)]/50 px-1.5 font-display text-[10px] uppercase tracking-wider text-[var(--magenta)]" {...hintProps({ label: "Rapid fire", value: "Quick mixing: ~40 bars a tune, drop to drop, lots of double drops" })}>
                Rapid
              </span>
            )}
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
          {plan.intent.vibe && <p className="truncate text-[11px] italic text-muted-foreground">{plan.intent.vibe}</p>}
          <Timeline tracks={plan.tracks} current={playing ? index : -1} />
          <Notes notes={plan.notes} />
        </>
      ) : (
        <p className="py-3 text-center text-[11px] text-[var(--ink-faint)]">Describe a set and press Build. Selecta picks and orders tracks from your library by genre, key, tempo and an energy arc, then performs the transitions.</p>
      )}
    </Panel>
  );
}

function Notes({ notes }: { notes: string[] }) {
  const ideas = notes.filter((n) => n.startsWith("DJ idea · ")).map((n) => n.slice("DJ idea · ".length));
  const other = notes.filter((n) => !n.startsWith("DJ idea · "));
  if (!ideas.length && !other.length) return null;
  return (
    <div className="flex flex-col gap-0.5 text-[11px]">
      {other.length > 0 && <p className="truncate text-[var(--warm)]">{other.join(" ")}</p>}
      {ideas.length > 0 && (
        <ul className="flex flex-wrap gap-x-3 gap-y-0.5 text-muted-foreground" aria-label="DJ ideas from the curator">
          {ideas.map((i) => (
            <li key={i} className="truncate" {...hintProps({ label: "DJ idea", value: i })}>
              <span className="text-accent">✦</span> {i}
            </li>
          ))}
        </ul>
      )}
    </div>
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
              value: `${t.genreLabel || "?"} · ${t.bpm.toFixed(1)} BPM${t.rate !== 1 ? ` → ${(t.bpm * t.rate).toFixed(1)}` : ""} · ${t.camelot}${t.keyShift ? ` → shifted ${t.keyShift > 0 ? "+" : ""}${t.keyShift} st to mix in key` : ""} · bars ${t.inBar}–${t.outBar} · ${fmt(t.estSec)}${t.trans.kind !== "start" ? ` · in by ${t.trans.kind.replace("_", " ")}` : ""}${t.fx.length ? ` · ${t.fx.length} FX: ${t.fx.map((f) => f.fx.split(":")[0]).join(", ")}` : ""}`,
            })}
          >
            <span className="truncate text-[11px] font-medium text-foreground">{t.title}</span>
            <span className="flex items-center gap-1 truncate font-mono text-[9.5px] text-muted-foreground">
              {(t.bpm * t.rate).toFixed(0)} · {t.camelot}
              {t.keyShift ? <span className="text-accent"> {t.keyShift > 0 ? "+" : ""}{t.keyShift}st</span> : null}
              {t.trans.kind !== "start" && (
                <span
                  className={cn(
                    "truncate rounded-[2px] px-1 font-display text-[9px] tracking-wider",
                    STEM_KINDS.has(t.trans.kind) ? "bg-accent text-accent-foreground" : "bg-background/60 text-foreground",
                  )}
                >
                  {KIND_LABEL[t.trans.kind]}
                </span>
              )}
            </span>
            <span aria-hidden className="absolute inset-x-0 bottom-0 h-0.5" style={{ background: `linear-gradient(90deg, transparent, ${deck} ${t.energy * 100}%, transparent ${t.energy * 100}%)` }} />
          </div>
        );
      })}
      <div ref={head} aria-hidden className="pointer-events-none absolute inset-y-0 w-0.5 bg-foreground opacity-0" />
    </div>
  );
}
