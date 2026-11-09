import * as React from "react";
<<<<<<< HEAD
import type { AiConfig, AiStatus, StemsStatus } from "@selecta/protocol";
=======
import type { AiConfig, AiStatus } from "@selecta/protocol";
>>>>>>> origin/main
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/dsgn/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/dsgn/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/dsgn/select";
import { Switch } from "@/components/dsgn/switch";
import { api } from "@/lib/api";
import { useApp } from "@/lib/store";

/**
 * Settings: music folders and the AI brain. Changes save immediately (no
 * Save button to forget); the dialog only gathers what's configurable.
 */
export function SettingsDialog({ open: isOpen, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const settings = useApp((s) => s.settings);
  const tagging = useApp((s) => s.tagging);
  const { saveSettings, setMusicDirs, watchJobs } = useApp.getState();
  const [status, setStatus] = React.useState<AiStatus | null>(null);
  const [test, setTest] = React.useState<{ busy: boolean; text: string; ok?: boolean }>({ busy: false, text: "" });

  React.useEffect(() => {
    if (isOpen) api.aiStatus().then(setStatus).catch(() => setStatus(null));
  }, [isOpen, settings?.ai]);

  if (!settings) return null;
  const ai = settings.ai;
  const setAi = (patch: Partial<AiConfig>) => void saveSettings({ ...settings, ai: { ...ai, ...patch } });

  const addFolder = async () => {
    const dir = await open({ directory: true, multiple: false, title: "Add a music folder" });
    if (typeof dir === "string" && !settings.musicDirs.includes(dir)) await setMusicDirs([...settings.musicDirs, dir]);
  };

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="font-display text-lg uppercase tracking-wider">Settings</DialogTitle>
          <DialogDescription>Everything runs on this machine. The AI uses your own signed-in Claude CLI.</DialogDescription>
        </DialogHeader>

        <Section title="Music folders">
          {settings.musicDirs.length === 0 && <p className="text-[11px] text-muted-foreground">No folders yet.</p>}
          <ul className="flex flex-col gap-1">
            {settings.musicDirs.map((d) => (
              <li key={d} className="flex items-center gap-2 rounded-sm border border-border bg-muted px-2 py-1">
                <span className="min-w-0 flex-1 truncate font-mono text-[11px]" data-selectable>{d}</span>
                <Button size="xs" variant="ghost" onClick={() => void setMusicDirs(settings.musicDirs.filter((x) => x !== d))} aria-label={`Remove ${d}`}>
                  Remove
                </Button>
              </li>
            ))}
          </ul>
          <Button size="xs" variant="secondary" onClick={addFolder} className="self-start">
            Add folder
          </Button>
        </Section>

        <Section title="AI brain">
          <Row label="Provider" hint={status?.detail}>
            <Select value={ai.provider} onValueChange={(v) => setAi({ provider: v })}>
              <SelectTrigger className="h-8 w-44 text-xs" aria-label="AI provider"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="claude">Claude CLI</SelectItem>
                <SelectItem value="off">Off (rules only)</SelectItem>
              </SelectContent>
            </Select>
          </Row>
          {ai.provider === "claude" && (
            <>
              <Row label="Model" hint="Sonnet curates best; Haiku is fastest; Opus is deepest and slowest.">
                <Select value={ai.model} onValueChange={(v) => setAi({ model: v })}>
                  <SelectTrigger className="h-8 w-44 text-xs" aria-label="Claude model"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="sonnet">Sonnet</SelectItem>
                    <SelectItem value="haiku">Haiku</SelectItem>
                    <SelectItem value="opus">Opus</SelectItem>
                  </SelectContent>
                </Select>
              </Row>
              <Row label="Curate every set" hint="Picks bangers, vetoes wrong-mood tracks, suggests a running order and DJ moments.">
                <Switch checked={ai.curate} onCheckedChange={(v) => setAi({ curate: v })} aria-label="Curate every set" />
              </Row>
              <Row label="Tag new tracks" hint="Genre, mood and language for every track after each scan, 60 per call.">
                <Switch checked={ai.tagLibrary} onCheckedChange={(v) => setAi({ tagLibrary: v })} aria-label="Tag new tracks" />
              </Row>
              <div className="flex flex-wrap items-center gap-2 pt-1">
                <Button
                  size="xs"
                  variant="secondary"
                  disabled={test.busy || !status?.available}
                  onClick={async () => {
                    setTest({ busy: true, text: "Asking…" });
                    try {
                      setTest({ busy: false, ok: true, text: await api.aiTest() });
                    } catch (e) {
                      setTest({ busy: false, ok: false, text: String(e instanceof Error ? e.message : e) });
                    }
                  }}
                >
                  Test AI
                </Button>
                <Button
                  size="xs"
                  variant="soft"
                  disabled={!status?.available || !!tagging}
                  onClick={async () => {
                    try {
                      const n = await api.tagLibrary();
                      setTest({ busy: false, ok: true, text: n ? `Tagging ${n} tracks in the background.` : "Every track is already tagged." });
                      watchJobs();
                    } catch (e) {
                      setTest({ busy: false, ok: false, text: String(e instanceof Error ? e.message : e) });
                    }
                  }}
                >
                  {tagging ? `Tagging ${tagging.done}/${tagging.total}…` : "Tag library now"}
                </Button>
                {test.text && <span role="status" className={`text-[11px] ${test.ok === false ? "text-destructive" : test.ok ? "text-[var(--cyan)]" : "text-muted-foreground"}`}>{test.text}</span>}
              </div>
            </>
          )}
        </Section>
<<<<<<< HEAD

        <StemsSection />
=======
>>>>>>> origin/main
      </DialogContent>
    </Dialog>
  );
}

<<<<<<< HEAD
/**
 * The stem engine: a one-time install (PyTorch + Demucs in Selecta's own
 * env), then vocal stems for the whole library in the background, the set
 * you're about to play first.
 */
function StemsSection() {
  const settings = useApp((s) => s.settings);
  const stemming = useApp((s) => s.stemming);
  const { saveSettings, watchJobs, scan } = useApp.getState();
  const [st, setSt] = React.useState<StemsStatus | null>(null);
  React.useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      const s = await api.stemsStatus().catch(() => null);
      if (!alive) return;
      // Install just finished: the backend has queued the library; show its progress.
      setSt((prev) => {
        if (prev?.installing && s?.installed) watchJobs();
        return s;
      });
      timer = setTimeout(poll, s?.installing ? 1000 : 4000);
    };
    void poll();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [watchJobs]);
  if (!settings) return null;
  const state = !st ? "…" : st.installing ? "Installing" : st.installed ? `Ready${st.device ? ` · ${st.device === "cuda" ? "GPU" : "CPU"}` : ""}` : "Not installed";
  return (
    <Section title="Stems">
      <Row
        label="Vocal / instrumental stems"
        hint="Powers mashups, acapella builds, bootleg moves and the VOX/INS kills. Uses the Demucs model on your GPU (CPU works, slower)."
      >
        <span className="rounded-sm border border-border bg-muted px-2 py-0.5 font-mono text-[11px]" role="status">{state}</span>
      </Row>
      {st && !st.installed && (
        <div className="flex flex-wrap items-center gap-2">
          <Button
            size="xs"
            variant="secondary"
            disabled={st.installing}
            onClick={async () => {
              await api.stemsInstall().catch(() => {});
              setSt({ ...st, installing: true, detail: "Starting…" });
            }}
          >
            {st.installing ? "Installing…" : "Install stem engine"}
          </Button>
          <span className="text-[11px] text-muted-foreground">One-time download: about 3 GB with an NVIDIA GPU, 300 MB without.</span>
        </div>
      )}
      {st?.detail && (st.installing || !st.installed || st.detail.startsWith("Install failed")) && (
        <p className="truncate font-mono text-[10.5px] text-muted-foreground" data-selectable title={st.detail}>{st.detail}</p>
      )}
      {st?.installed && (
        <Row label="Separate in the background" hint={stemming ? `${stemming.done} of ${stemming.total} tracks done (~15 s each on a GPU)` : "New tracks are separated after each scan."}>
          <Switch
            checked={settings.stems}
            onCheckedChange={async (v) => {
              await saveSettings({ ...settings, stems: v });
              if (v) {
                await scan();
                watchJobs();
              }
            }}
            aria-label="Separate stems in the background"
          />
        </Row>
      )}
    </Section>
  );
}

=======
>>>>>>> origin/main
function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-2 border-t border-border pt-3">
      <h3 className="font-display text-[12px] font-semibold uppercase tracking-[0.1em] text-muted-foreground">{title}</h3>
      {children}
    </section>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string | undefined; children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-3">
      <div className="min-w-0 flex-1">
        <div className="text-[12px] text-foreground">{label}</div>
        {hint && <div className="text-[11px] leading-snug text-muted-foreground">{hint}</div>}
      </div>
      {children}
    </div>
  );
}
