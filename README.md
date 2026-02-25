# newlife

A Conway's Game of Life desktop app written in Rust, built with
[egui](https://github.com/emilk/egui) /
[eframe](https://github.com/emilk/egui/tree/master/crates/eframe)
and rendered via wgpu on Wayland.

---

## Features

### Painting & interaction
- **Paint & erase** — left-click or drag to set cells alive; right-click or drag to erase
- **Coordinate tooltip** — hover over any cell to see its `(row, col)` coordinates
- **Grid lines** — toggle with `G` when zoom ≥ 4 px per cell; useful for precise editing

### Simulation control
- **Run / Pause / Step** — `Space` toggles play/pause; `S` advances one generation at a
  time while paused
- **Configurable speed** — 1–60 generations per second via the top-panel speed slider
- **Steps per frame** — batch 1–1024 simulation steps per visual frame for watching
  fast-evolving patterns
- **Random fill** — 🎲 button fills the grid at a configurable density (1–100 %)

### Grid
- **Auto-expanding grid** — the canvas grows automatically (20-cell dead margin) whenever
  live cells reach any edge
- **Auto-resize on load** — loading a pattern larger than the current grid creates a new
  grid with 40-cell margins; no cells are ever silently clipped

### Zoom & navigation
- **Smooth zoom** — `+` / `=` / `-` keys and the ＋/− toolbar buttons animate the zoom
  level; `0` resets to 100 %
- **Mouse-anchored zoom** — `Ctrl+scroll` or pinch zooms towards the cursor

### Population tracking
- **Live counter** — current live-cell count displayed in the top panel
- **Sparkline** — rolling 128-sample population bar chart for spotting growth or extinction

### Pattern library (25 built-in)
- **Browser panel** (left side) — filterable by category, searchable by name, each entry
  shows a 40×40 miniature preview
- *Still lifes* (8): Block, Beehive, Loaf, Boat, Tub, Pond, Ship, Long Boat
- *Oscillators* (7): Blinker, Toad, Beacon, Pulsar, Pentadecathlon, Figure Eight, Queen Bee Shuttle
- *Guns* (1): Gosper Glider Gun
- *Spaceships* (6): Glider, LWSS, MWSS, HWSS, Copperhead, Canada Goose
- *Methuselahs* (3): R-Pentomino, Acorn, Diehard

### User patterns
- **Save via browser** — "💾 Save…" popup writes a named `.cells` file to
  `~/.config/newlife/patterns/`
- **Native file dialogs** — 💾 and 📂 toolbar buttons open OS-native choosers for
  exporting or importing `.cells` / `.rle` files
- **Auto-reload** — user patterns are rescanned after every save and loaded at startup

### UI extras
- **Keyboard cheat-sheet** — `F1` toggles an overlay listing all shortcuts

---

## Prerequisites

- **Rust toolchain** (edition 2021) — install via [rustup](https://rustup.rs)
- **Wayland compositor** — required by the wgpu/winit backend
- **`make`** — optional, for the convenience targets in `Makefile`

---

## Build & Run

```bash
cargo run                      # debug build + launch
cargo build --release          # optimised binary → target/release/newlife
./target/release/newlife       # run the release binary

cargo test                     # run all tests
cargo test test_blinker        # run a specific test by name prefix
cargo clippy -- -D warnings    # lint (warnings are errors)
cargo fmt                      # format source code
cargo bench --bench step       # micro-benchmarks for the step kernel

make release bump=patch        # bump Cargo.toml patch version, commit, tag
make release bump=minor        # … minor version
make release bump=major        # … major version
```

---

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Space` | Toggle play / pause |
| `S` | Step one generation (paused only) |
| `R` | Clear the grid |
| `G` | Toggle grid lines |
| `+` / `=` | Zoom in |
| `-` | Zoom out |
| `0` | Reset zoom to 100 % |
| `Ctrl+scroll` | Zoom in / out (mouse-anchored) |
| `F1` | Show / hide keyboard cheat-sheet |

---

## Architecture

`newlife` is built with **egui 0.33 / eframe 0.33** (wgpu renderer, Wayland).
The codebase separates a pure simulation core (no UI dependency) from an egui frontend.

### Module map

| File | Role |
|------|------|
| `main.rs` | Entry point; creates the eframe window |
| `app.rs` | `GameOfLifeApp` — top-level `eframe::App`; owns `Simulation`, `Camera`, browser state, user patterns |
| `simulation.rs` | `Simulation` — pure state (no egui): grid, timing, speed, `steps_per_frame` |
| `grid.rs` | `Grid` — core data structure; all perf-critical logic lives here |
| `camera.rs` | `Camera` — cell size (zoom), scroll offset, viewport rect |
| `input.rs` | Keyboard shortcuts and Ctrl+scroll / pinch-to-zoom handling |
| `build.rs` | Build script: scans `src/patterns/<category>/` and generates `$OUT_DIR/library_entries.rs` |
| `patterns.rs` | `Pattern` enum with const `(Δrow, Δcol)` slices (used in tests) |
| `library.rs` | Built-in pattern library: `Category`, `LibraryEntry`, `decoded_library()`; `LIBRARY` via generated `include!` |
| `rle.rs` | RLE and `.cells` parser/serialiser: `parse_rle`, `parse_cells`, `write_cells`, `load_user_patterns` |
| `ui/panel.rs` | Top control panel (play/pause, speed, generation counter, zoom, sparkline, file dialogs) |
| `ui/browser.rs` | Left-side pattern browser: category filter, search, previews, save popup |
| `ui/grid_view.rs` | Grid canvas: mouse paint/erase, viewport-culled cell rendering, coordinate tooltip |

### Frame loop (`app.rs::update`)

Every egui frame executes in this fixed order:

```
handle_keyboard → handle_zoom → tick_zoom (smooth-zoom animation)
    → advance_simulation → draw_top_panel → draw_pattern_browser
    → draw_grid → draw_help_overlay (F1)
```

`advance_simulation` caps `dt` at 0.1 s to avoid a large first-frame spike, then calls
`sim.advance(dt)`.  `advance` runs `steps_per_frame` simulation steps per timer tick,
accumulating the `(add_top, add_left)` expansion returned by `expand_if_needed` so
`Camera::apply_expansion` can shift `scroll_offset` to keep the view stable.

### Grid internals (`grid.rs`) — three interleaved optimisations

#### 1 — Bit-packed storage

Cells are stored as `Vec<u64>`, row-major, 64 cells per word (LSB = leftmost cell).

```
words_per_row = ⌈width / 64⌉
```

This gives an **8× memory reduction** over `Vec<bool>` and keeps neighbour data in
cache-friendly 64-bit chunks.

#### 2 — Word-level frontier

`frontier: Vec<(row, word_index)>` lists every word that contains a live cell **or** is
adjacent (horizontally, vertically, diagonally) to one.  `step()` visits only those words
— **O(live + border)** per generation instead of O(width × height).

The frontier is kept sorted and deduplicated lazily via a merge-scan.  `prev_written`
tracks which scratch words need zeroing at the start of the next step (no hash-set
overhead).

#### 3 — SWAR kernel (`step_word`)

Each frontier word is evaluated with a **carry-save adder tree** (~30 bitwise ops) that
computes all 64 cell transitions simultaneously, avoiding 64 individual neighbour-count
loops and keeping the hot path branch-free.

**Double-buffer**: `cells` (current state) and `next` (scratch) swap each step.

**Auto-expand**: after every step, if any live cell touches an edge, `MARGIN = 20` dead
rows/cols are prepended on that side and the scroll offset is compensated to keep the
visible region stable.

### Pattern library (`build.rs` + `library.rs` + `rle.rs` + `src/patterns/`)

```
src/patterns/
├── still_life/    *.rle   (8 patterns)
├── oscillator/    *.rle   (7 patterns)
├── gun/           *.rle   (1 pattern)
├── spaceship/     *.rle   (6 patterns)
└── methuselah/    *.rle   (3 patterns)
```

- **Compile-time discovery** — `build.rs` scans these directories at build time, derives
  each display name from the file stem, and writes `$OUT_DIR/library_entries.rs`.
  Adding a pattern requires only dropping a `.rle` file in the correct subfolder.
- **`LIBRARY: &[LibraryEntry]`** — loaded at compile time via
  `include!(concat!(env!("OUT_DIR"), "/library_entries.rs"))`.
- **`decoded_library()`** — parses and centres all entries once via `OnceLock`; the
  result is shared across frames.
- **User patterns** — stored in `~/.config/newlife/patterns/*.cells`; loaded at startup
  and rescanned after every save.  `Category::Custom` distinguishes them from built-ins.

### Key invariants

| Invariant | Why it matters |
|-----------|----------------|
| `Simulation` has no `egui` import | Keeps simulation logic independently unit-testable without a display |
| `drag_paint_state: Option<bool>` lives on `GameOfLifeApp`, not `Grid` | Set on pointer-press, cleared on pointer-release; `Grid` remains input-stateless |
| Unused high bits in the last word of each row are always zero | The SWAR kernel reads these bits; a stray `1` would create phantom live neighbours |
| `live_bbox` is only expanded, never shrunk (except by `clear()` / `step()`) | Conservative over-estimate of the frontier — never misses live cells |

---

## License

MIT
