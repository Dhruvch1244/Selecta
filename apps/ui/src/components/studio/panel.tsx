import * as React from "react";
import { cn } from "@/lib/utils";

/**
 * A tool window: FL's "everything is a panel in one workspace" principle.
 * Title strip in the display face, hairline bevel, body that owns its own
 * scrolling. Panels are placed by the layout, never animated in.
 */
export interface PanelProps extends Omit<React.HTMLAttributes<HTMLElement>, "title"> {
  title: React.ReactNode;
  /** Controls on the right of the title strip (view toggles, counts). */
  tools?: React.ReactNode;
  /** Colour chip left of the title, e.g. a deck's identity colour. */
  swatch?: string;
  bodyClassName?: string;
}

export const Panel = React.forwardRef<HTMLElement, PanelProps>(({ title, tools, swatch, className, bodyClassName, children, ...rest }, ref) => (
  <section ref={ref} className={cn("flex min-h-0 min-w-0 flex-col overflow-hidden rounded-md border border-border bg-card shadow-ambient", className)} {...rest}>
    <header className="flex h-7 shrink-0 items-center gap-2 border-b border-border bg-[var(--glass)] px-2">
      {swatch && <span aria-hidden className="h-3 w-1 rounded-sm" style={{ background: swatch }} />}
      <h2 className="font-display text-[13px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">{title}</h2>
      <div className="ml-auto flex items-center gap-1">{tools}</div>
    </header>
    <div className={cn("min-h-0 flex-1", bodyClassName)}>{children}</div>
  </section>
));
Panel.displayName = "Panel";
