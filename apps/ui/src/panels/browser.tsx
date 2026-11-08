import * as React from "react";
import type { TrackSummary } from "@selecta/protocol";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/dsgn/button";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuShortcut, ContextMenuTrigger } from "@/components/dsgn/context-menu";
import { Input } from "@/components/dsgn/input";
import { Progress } from "@/components/dsgn/progress";
import { Panel } from "@/components/studio/panel";
import { hintProps } from "@/lib/hint";
import { useApp } from "@/lib/store";
import { cn } from "@/lib/utils";

const ROW = 30;

/**
 * Library browser. The list is windowed by hand (only visible rows are in the
 * DOM): libraries run to tens of thousands of files, and a fixed row height
 * makes this a few lines rather than a virtualisation dependency.
 */
export function Browser() {
  const tracks = useApp((s) => s.tracks);
  const settings = useApp((s) => s.settings);
  const scanning = useApp((s) => s.scanning);
  const analysing = useApp((s) => s.analysing);
  const tagging = useApp((s) => s.tagging);
  const { scan, load, setMusicDirs } = useApp.getState();
  const [q, setQ] = React.useState("");
  const [scrollTop, setScrollTop] = React.useState(0);
  const [height, setHeight] = React.useState(400);
  const [selected, setSelected] = React.useState<string | null>(null);
  const listRef = React.useRef<HTMLDivElement>(null);

  React.useLayoutEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setHeight(el.clientHeight));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const rows = React.useMemo(() => {
    const n = q.trim().toLowerCase();
    return n ? tracks.filter((t) => `${t.artist} ${t.title} ${t.album} ${t.genreTag}`.toLowerCase().includes(n)) : tracks;
  }, [tracks, q]);
  const first = Math.max(0, Math.floor(scrollTop / ROW) - 4);
  const visible = rows.slice(first, first + Math.ceil(height / ROW) + 8);
  const analysed = tracks.filter((t) => t.analyzed).length;
  const failed = tracks.filter((t) => t.error).length;

  const pickFolder = async () => {
    const dir = await open({ directory: true, multiple: false, title: "Add a music folder" });
    if (typeof dir === "string") await setMusicDirs([...(settings?.musicDirs ?? []), dir].filter((d, i, a) => a.indexOf(d) === i));
  };

  return (
    <Panel
      title="Browser"
      tools={
        <span className="font-mono text-[10px] tabular-nums text-muted-foreground" {...hintProps({ label: "Analysed tracks", value: `${analysed} of ${tracks.length}${failed ? `, ${failed} unreadable (point at the ⚠ for why)` : ""}` })}>
          {analysed}/{tracks.length}
          {failed > 0 && <span className="ml-1 text-destructive">⚠{failed}</span>}
        </span>
      }
      bodyClassName="flex flex-col"
    >
      <div className="flex gap-1 border-b border-border p-1.5">
        <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search artist, title, genre" aria-label="Search library" className="h-7 text-xs" {...hintProps({ label: "Search library", keys: "Ctrl+F" })} />
      </div>
      <div ref={listRef} className="relative min-h-0 flex-1 overflow-y-auto" onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)} role="listbox" aria-label="Tracks">
        {rows.length === 0 ? (
          <EmptyLibrary hasDirs={!!settings?.musicDirs.length} onAdd={pickFolder} onScan={scan} scanning={scanning} />
        ) : (
          <div style={{ height: rows.length * ROW }} className="relative">
            {visible.map((t, k) => (
              <TrackRow key={t.id} t={t} top={(first + k) * ROW} selected={selected === t.id} onSelect={() => setSelected(t.id)} onLoad={(d) => void load(d, t)} />
            ))}
          </div>
        )}
      </div>
      <footer className="flex flex-col gap-1.5 border-t border-border p-1.5">
        {analysing && (
          <div className="flex items-center gap-2" {...hintProps({ label: "Background analysis", value: `${analysing.done} of ${analysing.total} tracks` })}>
            <Progress value={(analysing.done / Math.max(1, analysing.total)) * 100} className="h-1" />
            <span className="font-mono text-[10px] tabular-nums text-muted-foreground">{analysing.done}/{analysing.total}</span>
          </div>
        )}
        {tagging && (
          <div className="flex items-center gap-2" {...hintProps({ label: "AI tagging", value: `${tagging.done} of ${tagging.total} tracks: genre, mood, language` })}>
            <Progress value={(tagging.done / Math.max(1, tagging.total)) * 100} className="h-1" />
            <span className="font-mono text-[10px] tabular-nums text-muted-foreground">AI {tagging.done}/{tagging.total}</span>
          </div>
        )}
        <div className="flex gap-1">
          <Button size="xs" variant="secondary" onClick={pickFolder} {...hintProps({ label: "Add a music folder" })}>
            Add folder
          </Button>
          <Button size="xs" variant="soft" onClick={() => void scan()} disabled={scanning} {...hintProps({ label: "Rescan folders", value: settings?.musicDirs.join(", ") || "no folders yet" })}>
            {scanning ? "Scanning…" : "Rescan"}
          </Button>
        </div>
      </footer>
    </Panel>
  );
}

function TrackRow({ t, top, selected, onSelect, onLoad }: { t: TrackSummary; top: number; selected: boolean; onSelect: () => void; onLoad: (d: "A" | "B") => void }) {
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          role="option"
          aria-selected={selected}
          tabIndex={selected ? 0 : -1}
          draggable
          onDragStart={(e) => e.dataTransfer.setData("application/x-selecta-track", t.id)}
          onClick={onSelect}
          onDoubleClick={() => onLoad("A")}
          onKeyDown={(e) => {
            if (e.key === "Enter") onLoad(e.shiftKey ? "B" : "A");
          }}
          {...hintProps({ label: `${t.artist} – ${t.title}`, value: t.error ? `Can't analyse: ${t.error}` : t.analyzed ? `${t.bpm?.toFixed(1)} BPM · ${t.key} (${t.camelot})${t.mood ? ` · ${t.mood}` : ""}` : "not analysed yet", keys: "Enter → A · Shift+Enter → B" })}
          style={{ top, height: 30 }}
          className={cn(
            "absolute inset-x-0 flex cursor-default items-center gap-2 border-b border-border/40 px-2 outline-none",
            selected ? "bg-accent/15" : "hover:bg-muted",
          )}
        >
          <div className="min-w-0 flex-1 leading-tight">
            <div className="truncate text-[12px] text-foreground">{t.title}</div>
            <div className="flex items-center gap-1 truncate text-[10px] text-muted-foreground">
              <span className="truncate">{t.artist || "Unknown artist"}</span>
              {t.mood && <span className="shrink-0 rounded-[2px] bg-muted px-1 text-[9px] uppercase tracking-wide">{t.mood}</span>}
            </div>
          </div>
          {t.error ? (
            <span className="w-10 text-right text-[12px] text-destructive" aria-label={`Can't analyse: ${t.error}`}>⚠</span>
          ) : (
            <span className="w-10 text-right font-mono text-[11px] tabular-nums text-muted-foreground">{t.bpm ? t.bpm.toFixed(1) : "···"}</span>
          )}
          <span className="w-7 text-right font-mono text-[11px] text-[var(--cyan)]">{t.camelot ?? ""}</span>
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={() => onLoad("A")}>
          Load to deck A<ContextMenuShortcut>Enter</ContextMenuShortcut>
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => onLoad("B")}>
          Load to deck B<ContextMenuShortcut>⇧Enter</ContextMenuShortcut>
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem disabled>{t.path.split(/[\\/]/).slice(-2).join("/")}</ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

function EmptyLibrary({ hasDirs, onAdd, onScan, scanning }: { hasDirs: boolean; onAdd: () => void; onScan: () => void; scanning: boolean }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 p-4 text-center">
      <p className="text-muted-foreground">{hasDirs ? "No tracks found yet." : "No music folders yet."}</p>
      <p className="text-[11px] text-[var(--ink-faint)]">Selecta reads files you own (MP3, FLAC, M4A, OGG, WAV) and analyses tempo, key and structure on your machine.</p>
      <Button size="xs" variant="accent" onClick={hasDirs ? onScan : onAdd} disabled={scanning}>
        {hasDirs ? (scanning ? "Scanning…" : "Scan now") : "Add a music folder"}
      </Button>
    </div>
  );
}
