# Design: the "Studio" voice

Selecta follows **FL Studio's design principles**, applied through the
[dsgn](https://github.com/dhruvch1244/design) token system and component
registry. It is an original implementation of the principles, not a copy of
FL Studio's skin or assets.

## Principles

1. **One workspace, no pages.** Browser, decks and mixer are panels that are
   always visible. Nothing navigates away from the music.
2. **The hint bar.** Pointing at or dragging any control shows its name,
   live value and shortcut in a fixed strip under the toolbar, instead of
   tooltips covering what you're adjusting. Every control registers a hint.
3. **Direct manipulation.** Knobs and faders drag vertically (Shift = fine),
   take the mouse wheel, reset on double-click, and support arrow, PageUp
   and Home keys. They are ARIA sliders, so screen readers get value text.
4. **Colour is information.** Deck A is always cyan and deck B always
   magenta: its waveform, swatch, knob arcs and fader cap. The accent
   (orange) means selection, focus and the primary action. Red appears only
   for clipping and errors.
5. **Readouts are instruments.** Tempo, time, key and latency sit in
   recessed monospace LCDs. Values change in place, written straight to the
   DOM by the frame loop, so React never re-renders at 60 Hz.
6. **Dense but legible.** 12 px body text, 28 px panel headers, a
   condensed display face for labels, and every number in tabular figures.
7. **Flat with a bevel.** Near-zero radius (`--radius-scale: 0.3`), a
   1 px light top edge and dark bottom edge (`--shadow-ambient`), slate
   rather than black so coloured waveforms don't glare in a dark room.
8. **Motion is feedback, not decoration.** 80–150 ms state changes; panels
   never animate in; `prefers-reduced-motion` turns transitions off.
9. **Meters you can trust.** Peak and RMS with peak-hold, a latching clip
   LED, and DAW ballistics (instant attack, 20 dB/s fall, 1.5 s hold).

## Tokens

`apps/ui/src/styles/tokens.css` sets values on dsgn's existing token names
(raw → semantic → Tailwind `@theme`) and adds no new names, so every registry
component reskins without source edits. Dark and light palettes are both
defined; the light palette darkens the hues for AA contrast instead of just
swapping the ground.

| Role | Token | Dark | Light |
|---|---|---|---|
| App ground | `--void` | `#1b1f22` | `#d9dde0` |
| Panel | `--surface` | `#262b2f` | `#eceff1` |
| Accent / selection | `--warm` | `#f5a04b` | `#b85f00` |
| Deck A | `--cyan` | `#4fc6da` | `#0b7f92` |
| Deck B | `--magenta` | `#e8628f` | `#b8235a` |
| Phrase lines | `--violet` | `#a08cff` | `#5b3fd6` |

Fonts (bundled locally, no network): Inter for UI text, Barlow Condensed for
display labels, and JetBrains Mono for readouts.

## Components

- From the dsgn registry (`apps/ui/src/components/dsgn/`, installed with
  `npx @dhruvchoudhary/dsgn add …`): button, context-menu, dropdown-menu,
  input, progress, tooltip, tabs, select, slider, scroll-area, toggle,
  switch, separator, table.
- DAW controls the registry doesn't have (`components/studio/`): Panel,
  Knob, Fader, Meter, Lcd, HintBar, DetailWave and OverviewWave. They are
  built the registry's way: CVA for variants, `cn()`, tokens only.
