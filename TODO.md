# TODO — Newlife Improvement Backlog

Items are grouped by theme. Within each section, entries are roughly ordered from
highest to lowest impact / easiest to hardest.

---

## 1. Performance — Simulation Engine

- [ ] **HashLife algorithm** — implement the quadtree-based HashLife algorithm
  (see <https://johnhw.github.io/hashlife/index.md.html>) for O(log N) amortised steps per
  generation on periodic or highly-repetitive patterns.  HashLife memoises 2^k × 2^k
  quadtree nodes by content hash, enabling exponential time-leaps; it is the standard
  algorithm for long-running complex patterns such as guns, methuselahs, and
  self-replicators where the current SWAR frontier approach scales linearly.

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

