# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build           # compile (debug)
cargo run             # run the application
cargo test            # run all tests
cargo test <name>     # run a single test, e.g. cargo test test_blinker_oscillates
cargo clippy          # lint
cargo fmt             # format
cargo build --release # optimised build
cargo bench --bench step   # microbenchmarks; run before/after any performance change
```

## Workflow

- **Always plan first**: for any non-trivial change (new feature, multi-file edit, refactor)
  use `EnterPlanMode`, design the approach, and get user approval before writing any code.
- **Commit after every source code change**: once tests, linter, and formatter all pass,
  create a git commit.  Do not bundle unrelated changes into a single commit.  The commit
  message body must list every modified file with a concise explanation of what changed in
  it and why (not just "updated X" — describe the actual change).
- **Summarise changes**: after completing any modification, provide a summary listing every
  modified file and a brief description of what changed in each.
- **Benchmark after perf changes**: for any change intended as a performance improvement,
  capture a baseline *before* making changes with
  `cargo bench --bench step -- --save-baseline before`, implement the change, then run
  `cargo bench --bench step -- --save-baseline after` and compare with
  `cargo bench --bench step -- --load-baseline before --baseline after`.
  A regression on any existing benchmark must be justified or fixed before the commit is
  complete.  This applies to any module, not just `grid.rs`.
- **Unit tests are mandatory**: every source code change must include corresponding test
  updates — add new tests for new behaviour, update existing tests when behaviour changes,
  and delete tests that cover removed functionality.  A change without an appropriate test
  delta is incomplete.

## Architecture

`newlife` is a Conway's Game of Life desktop app built with egui 0.33 / eframe 0.33 (wgpu renderer, Wayland).

### Module map

| File | Role |
|---|---|
| `main.rs` | Entry point; creates the eframe window |
| `app.rs` | `GameOfLifeApp` — top-level `eframe::App`; owns `Simulation`, `Camera`, browser state, user patterns |
| `simulation.rs` | `Simulation` — pure state (no egui): grid, timing, speed, `steps_per_frame` |
| `grid.rs` | `Grid` — core data structure; all perf-critical logic lives here |
| `camera.rs` | `Camera` — cell size (zoom), scroll offset, viewport rect |
| `input.rs` | Keyboard shortcuts and Ctrl+scroll / pinch-to-zoom |
| `build.rs` | Build script: scans `src/patterns/<category>/` and generates `$OUT_DIR/library_entries.rs` |
| `patterns.rs` | `Pattern` enum with const `(Δrow, Δcol)` slices (used in tests) |
| `library.rs` | Built-in pattern library: `Category`, `LibraryEntry`, `decoded_library()`; `LIBRARY` via generated `include!` |
| `rle.rs` | RLE and `.cells` parser/serialiser: `parse_rle`, `parse_cells`, `write_cells`, `load_user_patterns` |
| `ui/panel.rs` | Top control panel (play/pause, speed, generation counter, zoom) |
| `ui/browser.rs` | Left-side pattern browser panel: category filter, search, previews, save popup |
| `ui/grid_view.rs` | Grid canvas: mouse paint/erase, viewport-culled cell rendering |

### Frame loop (`app.rs::update`)

```
handle_keyboard → handle_zoom → advance_simulation → draw_top_panel → draw_pattern_browser → draw_grid
```

`advance_simulation` caps `dt` at 0.1 s to avoid a first-frame spike, then calls `sim.advance(dt)` which steps in multiples of `1/speed`. Each step also calls `expand_if_needed`, returning `(add_top, add_left)` rows/cols so `Camera::apply_expansion` can compensate `scroll_offset`.

### Grid internals (`grid.rs`) — three interleaved optimisations

1. **Bit-packed storage** — `Vec<u64>`, row-major, 64 cells per word (LSB = leftmost). `words_per_row = ⌈width/64⌉`. 8× memory reduction vs `Vec<bool>`.
2. **Word-level frontier** — `frontier: FxHashSet<(row, wi)>` covers every word that contains a live cell or is adjacent to one. `step()` materialises it into `frontier_vec` and evaluates only those words — O(live + border) per step.
3. **SWAR kernel** (`step_word`) — evaluates all 64 bit positions of a word simultaneously via carry-save adder trees (~30 bitwise ops) instead of 64 individual neighbour loops.  **AVX2 fast path** (`step_4words_avx2` / `compute_4words`) — processes four consecutive words in a single 256-bit pass using AVX2 lane-wise intrinsics (`_mm256_{slli,srli,or,and,xor,andnot}_si256`); dispatched from `step()` when `is_x86_feature_detected!("avx2")` is true and `frontier_vec.len() ≥ AVX2_SORT_THRESHOLD` (64).  Below that threshold the scalar `compute_word` path is used with no sorting overhead.

Double-buffer: `cells` (current) and `next` (scratch) swap each step. `prev_written: Vec<(row, wi)>` tracks which `next` words need zeroing at the start of the following step.  When `frontier_vec.len() ≥ AVX2_SORT_THRESHOLD`, the vec is sorted (`sort_unstable`) before evaluation so that consecutive `(row, wi)` runs are adjacent — enabling AVX2 4-word batching in the sequential path and improving cache locality for Rayon word loads in the parallel path.

Auto-expand: after each step, if live cells touch any edge, `MARGIN = 20` dead rows/cols are prepended on that side and the scroll offset is shifted to keep the view stable.

### Pattern library (`build.rs` + `library.rs` + `rle.rs` + `src/patterns/`)

- `src/patterns/<category>/*.rle` — 25 RLE files in 5 subfolders (`still_life/`, `oscillator/`, `gun/`, `spaceship/`, `methuselah/`)
- `build.rs` scans these dirs at compile time, derives each pattern's name from the file stem, and writes `$OUT_DIR/library_entries.rs`; adding a pattern = dropping a `.rle` file in the right folder
- `LIBRARY: &[LibraryEntry]` — 25 entries loaded via `include!(concat!(env!("OUT_DIR"), "/library_entries.rs"))`
- `decoded_library()` — decodes all entries once via `OnceLock`, centres each with `center_cells`
- User patterns stored in `~/.config/newlife/patterns/*.cells`; loaded at startup and after saves via `load_user_patterns`
- `Category::Custom` distinguishes user patterns from built-ins in the browser

### Key invariants to preserve

- `Simulation` has no egui dependency — keep simulation logic independently testable.
- `drag_paint_state: Option<bool>` lives on `GameOfLifeApp` (not `Grid`): set on `drag_started`, cleared on `drag_stopped`.
- Unused high bits in the last word of each row must always be zero (enforced by the mask in `step()` / `compute_4words()` and by `set_bit`).
- `live_bbox` is expanded conservatively (never shrunk except by `clear()` or `step()`).
- `step_4words_avx2` is `unsafe` and `#[target_feature(enable = "avx2")]`; it must only be called inside an `is_x86_feature_detected!("avx2")` runtime guard — never unconditionally.

## Documentation

When modifying source files, keep **README.md** and **CLAUDE.md** in sync:

- `README.md` — update if features, keyboard shortcuts, or build instructions change
- `CLAUDE.md` — update if modules are added/removed, the frame loop changes, or architectural invariants change
