import * as React from "react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/dsgn/dropdown-menu";
import { Lcd } from "@/components/studio/lcd";
import { Meter } from "@/components/studio/meter";
import { onFrame } from "@/lib/frame";
import { hintProps } from "@/lib/hint";
import { getEngine, peekEngine, useApp } from "@/lib/store";
import { SettingsDialog } from "./settings-dialog";

/**
 * Top bar, FL-style: app menu, master tempo/time/key LCDs for whichever deck
 * is loudest in the mix right now, master meter, audio latency. LCD text is
 * written by the frame loop straight into the DOM, never through React state.
 */
export function Toolbar() {
  const bpm = React.useRef<HTMLSpanElement>(null);
  const time = React.useRef<HTMLSpanElement>(null);
  const key = React.useRef<HTMLSpanElement>(null);
  const lat = React.useRef<HTMLSpanElement>(null);
  const [theme, setTheme] = React.useState(() => document.documentElement.dataset.theme ?? "dark");
  const [settingsOpen, setSettingsOpen] = React.useState(false);
  const ai = useApp((s) => s.settings?.ai);
  const readMaster = React.useCallback(() => peekEngine()?.master ?? null, []);

  React.useEffect(
    () =>
      onFrame(() => {
        const e = peekEngine();
        if (!e) return;
        const live = (["A", "B"] as const).map((id) => e.decks[id]).filter((d) => d.playing && d.track);
        const lead = live.sort((x, y) => y.meter.rmsDb - x.meter.rmsDb)[0];
        if (bpm.current) bpm.current.textContent = lead ? lead.bpm.toFixed(2) : "---.--";
        if (key.current) key.current.textContent = lead?.track ? lead.track.analysis.camelot : "--";
        if (time.current && lead) {
          const p = lead.position();
          time.current.textContent = `${Math.floor(p / 60)}:${String(Math.floor(p % 60)).padStart(2, "0")}.${Math.floor((p % 1) * 10)}`;
        }
        if (lat.current) lat.current.textContent = `${e.latencyMs} ms`;
      }),
    [],
  );

  const applyTheme = (t: string) => {
    document.documentElement.dataset.theme = t;
    setTheme(t);
    try {
      localStorage.setItem("selecta.theme", t);
    } catch {
      /* private mode: the theme just won't persist */
    }
  };

  return (
    <div className="flex h-11 shrink-0 items-center gap-2 border-b border-border bg-card px-2 shadow-ambient">
      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
      <DropdownMenu>
        <DropdownMenuTrigger className="flex h-8 items-center gap-2 rounded-sm px-2 outline-none hover:bg-muted focus-visible:ring-1 focus-visible:ring-ring" {...hintProps({ label: "Selecta menu" })}>
          <Logo />
          <span className="font-display text-[15px] font-semibold uppercase tracking-[0.18em]">Selecta</span>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuItem onSelect={() => setSettingsOpen(true)}>Settings…</DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuLabel>Library</DropdownMenuLabel>
          <DropdownMenuItem onSelect={() => void useApp.getState().scan()}>Rescan music folders</DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuLabel>Theme</DropdownMenuLabel>
          <DropdownMenuRadioGroup value={theme} onValueChange={applyTheme}>
            <DropdownMenuRadioItem value="dark">Dark</DropdownMenuRadioItem>
            <DropdownMenuRadioItem value="light">Light</DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
      <div className="mx-1 h-6 w-px bg-border" />
      <Lcd label="Tempo" valueRef={bpm} hint="Tempo of the deck leading the mix">---.--</Lcd>
      <Lcd label="Time" valueRef={time} width="w-24" hint="Position of the deck leading the mix">0:00.0</Lcd>
      <Lcd label="Key" valueRef={key} width="w-14" hint="Camelot key of the deck leading the mix">--</Lcd>
      <div className="ml-auto flex h-full items-center gap-3 py-1.5">
        <div className="flex h-full items-center gap-1" {...hintProps({ label: "Master output" })}>
          <Meter label="Master" read={readMaster} className="w-2" />
        </div>
        <button type="button" onClick={() => setSettingsOpen(true)} className="text-left" aria-label="AI settings">
          <Lcd label="AI" width="w-20" hint={ai?.provider === "claude" ? `Claude CLI (${ai.model}). Click for settings` : "Off: rules only. Click for settings"}>
            {ai?.provider === "claude" ? ai.model : "off"}
          </Lcd>
        </button>
        <Lcd label="Latency" valueRef={lat} width="w-16" hint="Audio output latency you actually hear">-- ms</Lcd>
        <button
          type="button"
          className="h-8 rounded-sm border border-border bg-secondary px-2 font-display text-[11px] uppercase tracking-wider text-muted-foreground hover:text-foreground"
          onClick={() => void getEngine().resume()}
          {...hintProps({ label: "Audio engine", value: "Click if audio was blocked by the system" })}
        >
          Audio
        </button>
      </div>
    </div>
  );
}

function Logo() {
  return (
    <svg viewBox="0 0 24 24" className="h-5 w-5" aria-hidden>
      <circle cx="12" cy="12" r="10" fill="var(--secondary)" stroke="var(--rule)" />
      <path d="M12 2 A10 10 0 0 1 22 12" fill="none" stroke="var(--accent)" strokeWidth="2.5" strokeLinecap="round" />
      <circle cx="12" cy="12" r="3" fill="var(--accent)" />
    </svg>
  );
}
