# TODO — Newlife Improvement Backlog

Items are grouped by theme. Within each section, entries are roughly ordered from
highest to lowest impact / easiest to hardest.

---

## 4. UI / UX Improvements

- [ ] **Cell coordinate tooltip** — show `(row, col)` in a tooltip or status bar when
  hovering over the grid.

- [ ] **Population counter & live-cell history graph** — display the current live-cell
  count next to the generation counter; optionally plot the last N values as a
  sparkline.

- [ ] **Grid lines toggle** — keyboard shortcut `G` or a checkbox to draw grid lines
  between cells at higher zoom levels.

- [ ] **Keyboard shortcut cheat-sheet** — pressing `?` or `F1` opens a modal overlay
  listing all key bindings.

- [ ] **Save / load custom grids** — serialise the current grid state (e.g. as `.cells`
  plaintext) and restore it from disk. Use `rfd` for a native file-picker dialog.

- [ ] **Random fill** — a "Random" button that seeds the grid with a configurable density
  (0–100 %, default 30 %) of randomly alive cells.

- [ ] **Smooth zoom animation** — interpolate `cell_size` toward a target value over a
  few frames instead of applying it instantly, giving a polished feel.

---

## 5. Pattern Library

- [x] **RLE / `.cells` file parser** — implement a parser for the two most common GoL
  file formats so any pattern from LifeWiki can be loaded without hand-coding
  coordinates.

- [x] **Embed all LifeWiki "small" patterns** — add the ~200 well-known patterns (still
  lifes up to 14 cells, all common oscillators, all known period spaceships) as
  built-in presets loaded from embedded RLE strings.

- [x] **Pattern browser panel** — a scrollable side panel (or modal) with categories,
  a search field, a miniature preview of each pattern, and a single-click to load.

- [x] **User-defined pattern library** — let users save the current selection as a named
  pattern stored in `~/.config/newlife/patterns/`, which then appears in the browser
  alongside the built-ins.

- [ ] **More oscillators** — still missing:
  - Clock (p2, 8 cells) — RLE not yet verified

- [ ] **More spaceships** — still missing:
  - Canada Goose (c/4 diagonal, 26 cells) — RLE not yet verified

- [ ] **More methuselahs** — still missing:
  - Die Hard extended (many variants)
  - Rabbits (17331 gen) — RLE not yet verified
  - Bunnies (17489 gen) — RLE not yet verified
