# TODO — Newlife Improvement Backlog

Items are grouped by theme. Within each section, entries are roughly ordered from
highest to lowest impact / easiest to hardest.

---

## 1. Performance — CPU / Memory

---

## 2. Faster Generation Computing

- [ ] **Active-cell frontier** — maintain a `HashSet` of cells that have at least one
  live neighbour. Only evaluate those cells each step (O(live + border) instead of
  O(width × height)). Ideal for sparse patterns like glider guns.

- [ ] **SIMD neighbour counting** — with bit-packed rows, use SIMD shifts and bitwise
  operations to count all 8 neighbours for 64 cells simultaneously (one `u64` word
  per instruction group).

- [ ] **HashLife / Gosper algorithm** — recursive quad-tree memoisation that can advance
  a pattern by 2^n generations in O(unique-quad-trees) time. Essential for
  methuselahs like Acorn (5206 gen) and gun patterns. Consider integrating the
  `hashlife` crate or porting the algorithm.

- [ ] **Configurable step-ahead** — expose a "steps per frame" integer in the UI (e.g.
  1, 2, 4, 8 …) so users can trade visual smoothness for raw throughput without
  having to max out the speed slider.

---

## 3. Code Refactoring / Maintainability

- [ ] **Split `app.rs` into sub-modules** — the file already handles rendering, input,
  simulation timing, and the top panel. Extract into:
  - `src/ui/panel.rs`   — top control panel
  - `src/ui/grid_view.rs` — grid painter + mouse handler
  - `src/input.rs`      — keyboard & zoom handlers
  - `src/simulation.rs` — `advance_simulation`, `step_once`

- [ ] **Separate simulation state from render state** — introduce a plain `Simulation`
  struct (`grid`, `generation`, `running`, `speed`) with no egui dependency, and keep
  egui-specific fields (`scroll_offset`, `viewport_rect`, `cell_size`, …) in the app
  layer. Makes unit-testing the logic trivial.

- [ ] **Load pattern coordinates from data, not code** — move pattern cell lists to
  static data (e.g. `const` arrays or a `patterns/` directory of `.cells` files
  parsed at startup). Eliminates the need to recompile when adding new patterns.

- [ ] **Introduce a `Camera` type** — encapsulate `cell_size`, `scroll_offset`, and
  `viewport_rect` together with `apply_zoom`, `apply_expansion`, and
  screen-to-cell conversion. Reduces parameter passing and makes zoom logic testable
  in isolation.

- [ ] **Replace magic numbers with named constants** — grid `MARGIN` (20) in `grid.rs`,
  gap size (1.0) in `paint_cells`, `DEFAULT_CELL_SIZE` ratio already exists but
  other numbers are inline.

- [ ] **Add integration tests for `set_pattern`** — verify that each pattern lands at the
  expected grid coordinates, does not overflow bounds, and produces the correct cell
  count after `set_pattern`.

---

## 4. UI / UX Improvements

- [ ] **Cell coordinate tooltip** — show `(row, col)` in a tooltip or status bar when
  hovering over the grid.

- [ ] **Population counter & live-cell history graph** — display the current live-cell
  count next to the generation counter; optionally plot the last N values as a
  sparkline.

- [ ] **Pan with right-click drag** — allow panning the viewport by right-click-dragging
  (or middle-click) without painting cells.

- [ ] **Undo / redo** — maintain a ring buffer of recent grid snapshots (e.g. 20 steps)
  and expose Ctrl+Z / Ctrl+Y to restore them.

- [ ] **Cell selection, copy & paste** — rubber-band select a rectangular region, copy
  it to a clipboard buffer, and paste it at a new position (like a pattern stamp).

- [ ] **Grid lines toggle** — keyboard shortcut `G` or a checkbox to draw grid lines
  between cells at higher zoom levels.

- [ ] **Colour theme picker** — let the user choose from a few preset palettes (classic
  green-on-black, blue-on-dark, high-contrast, …) stored in a `Theme` struct.

- [ ] **Keyboard shortcut cheat-sheet** — pressing `?` or `F1` opens a modal overlay
  listing all key bindings.

- [ ] **Wrap-around (toroidal) mode** — add a toggle that makes the grid wrap at the
  edges so patterns leaving one side re-enter from the other.

- [ ] **Save / load custom grids** — serialise the current grid state (e.g. as `.cells`
  plaintext) and restore it from disk. Use `rfd` for a native file-picker dialog.

- [ ] **Random fill** — a "Random" button that seeds the grid with a configurable density
  (0–100 %, default 30 %) of randomly alive cells.

- [ ] **Smooth zoom animation** — interpolate `cell_size` toward a target value over a
  few frames instead of applying it instantly, giving a polished feel.

---

## 5. Pattern Library

- [ ] **RLE / `.cells` file parser** — implement a parser for the two most common GoL
  file formats so any pattern from LifeWiki can be loaded without hand-coding
  coordinates.

- [ ] **Embed all LifeWiki "small" patterns** — add the ~200 well-known patterns (still
  lifes up to 14 cells, all common oscillators, all known period spaceships) as
  built-in presets loaded from embedded RLE strings.

- [ ] **Pattern browser panel** — a scrollable side panel (or modal) with categories,
  a search field, a miniature preview of each pattern, and a single-click to load.

- [ ] **User-defined pattern library** — let users save the current selection as a named
  pattern stored in `~/.config/newlife/patterns/`, which then appears in the browser
  alongside the built-ins.

- [ ] **More oscillators** — add at minimum:
  - Clock (p2, 8 cells)
  - Figure-eight (p8, 20 cells)
  - Queen Bee Shuttle (p30)
  - Gosper Glider Gun (p30, 36 cells)

- [ ] **More spaceships** — add at minimum:
  - Copperhead (c/10 orthogonal)
  - Canada Goose (c/4 diagonal, 26 cells)

- [ ] **More methuselahs** — add at minimum:
  - Die Hard extended (many variants)
  - Rabbits (17331 gen)
  - Bunnies (17489 gen)
