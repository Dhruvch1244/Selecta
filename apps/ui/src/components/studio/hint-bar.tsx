import { useHintStore } from "@/lib/hint";
import { useApp } from "@/lib/store";

/** One line under the toolbar: what you're pointing at, its value, its shortcut; otherwise the last status message. */
export function HintBar() {
  const hint = useHintStore((s) => s.hint);
  const status = useApp((s) => s.status);
  return (
    <div role="status" aria-live="polite" className="flex h-6 shrink-0 items-center gap-3 border-b border-border bg-muted px-3 text-[11px]">
      {hint ? (
        <>
          <span className="font-medium text-foreground">{hint.label}</span>
          {hint.value && <span className="font-mono tabular-nums text-[var(--cyan)]">{hint.value}</span>}
          {hint.keys && <kbd className="ml-auto rounded-sm border border-border bg-secondary px-1.5 font-mono text-[10px] text-muted-foreground">{hint.keys}</kbd>}
        </>
      ) : (
        <span className="text-muted-foreground">{status || "Point at any control to see what it does."}</span>
      )}
    </div>
  );
}
