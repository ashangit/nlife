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
| `simulation.rs` | `Simulation` — pure state (no egui): engine selection, timing, speed, `steps_per_frame` |
| `grid.rs` | `Grid` — SWAR bit-packed engine; all perf-critical SWAR logic lives here |
| `hashlife.rs` | `HashLife` — quadtree-memoised engine; canonical node interning, `step_recursive`, auto-expansion |
| `camera.rs` | `Camera` — cell size (zoom), scroll offset, viewport rect |
| `input.rs` | Keyboard shortcuts and Ctrl+scroll / pinch-to-zoom |
| `build.rs` | Build script: scans `src/patterns/<category>/` and generates `$OUT_DIR/library_entries.rs` |
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

1. **Bit-packed tiled storage** — `Vec<u64>`, 64 cells per word (LSB = leftmost). `words_per_row = ⌈width/64⌉`. 8× memory reduction vs `Vec<bool>`. Stored in an **8-row tile layout** (`TILE_HEIGHT = 8`): `tiled_idx(row, wi, wpr) = (row/8)*(wpr*8) + wi*8 + (row%8)`. Eight consecutive row values for the same word-column share a 64-byte cache line, so `compute_word`'s vertical neighbour loads (row±1, wi) hit the same cache line as (row, wi). Buffer size is `tiled_size(height, wpr) = ⌈height/8⌉ × 8 × wpr`; padding slots beyond `height-1` are always zero.
2. **Word-level frontier** — `frontier: FxHashSet<(row, wi)>` covers every word that contains a live cell or is adjacent to one. `step()` materialises it into `frontier_vec` and evaluates only those words — O(live + border) per step.
3. **SWAR kernel** (`step_word`) — evaluates all 64 bit positions of a word simultaneously via carry-save adder trees (~30 bitwise ops) instead of 64 individual neighbour loops.  **AVX2 fast path** (`step_4words_avx2` / `compute_4words`) — processes four consecutive words in a single 256-bit pass using AVX2 lane-wise intrinsics (`_mm256_{slli,srli,or,and,xor,andnot}_si256`); dispatched from `step()` when `is_x86_feature_detected!("avx2")` is true and `frontier_vec.len() ≥ AVX2_SORT_THRESHOLD` (64).  Below that threshold the scalar `compute_word` path is used with no sorting overhead.

Double-buffer: `cells` (current) and `next` (scratch) swap each step. `prev_written: Vec<(row, wi)>` tracks which `next` words need zeroing at the start of the following step.  When `frontier_vec.len() ≥ AVX2_SORT_THRESHOLD`, the vec is sorted (`sort_unstable`) before evaluation so that consecutive `(row, wi)` runs are adjacent — enabling AVX2 4-word batching in the sequential path and improving cache locality for Rayon word loads in the parallel path.

Auto-expand: after each step, if live cells touch any edge, `MARGIN = 20` dead rows/cols are prepended on that side and the scroll offset is shifted to keep the view stable.

### HashLife engine (`hashlife.rs`)

- `HashLife` stores the universe as a canonical quadtree; nodes are identified by `NodeId` (`u32` arena index)
- `CanonTable` — purpose-built open-addressing intern map; 20-byte `CanonEntry {nw,ne,sw,se,id}`, linear probing, 75% load factor, FxHasher on two packed `u64` words; replaces `FxHashMap<(u32,u32,u32,u32),u32>` for better cache locality
- `step_recursive` — 9-submacrocell algorithm advancing `2^(level−2)` gens, memoised in `step_cache: FxHashMap<NodeId,NodeId>`
- `step_universe` expansion loop checks **two conditions** before each step (both evaluated under a single `nodes` lock per iteration via `needs_expansion_inner` / `needs_expansion_deep_inner`):
  1. `needs_expansion_inner()` — all 12 outer grandchildren must be empty (cells within `[N/4, 3N/4)`)
  2. `needs_expansion_deep_inner()` — all 12 near-boundary great-grandchildren must also be empty (cells within `[3N/8, 5N/8)`); prevents cells near the result-window boundary from being silently dropped during the step for patterns moving at up to c/2
- Re-centering after each step: result `(level k−1)` is split into 4 quadrants and assembled into a same-level root with dead padding, preserving absolute cell coordinates

### Pattern library (`build.rs` + `library.rs` + `rle.rs` + `src/patterns/`)

- `src/patterns/<category>/*.rle` — 1284 RLE files in 7 subfolders (`still_life/`×366, `oscillator/`×582, `gun/`×80, `spaceship/`×179, `methuselah/`×50, `puffer/`×23, `wick/`×4)
- `build.rs` scans these dirs at compile time, derives each pattern's name from the file stem, and writes `$OUT_DIR/library_entries.rs`; adding a pattern = dropping a `.rle` file in the right folder
- `LIBRARY: &[LibraryEntry]` — all entries loaded via `include!(concat!(env!("OUT_DIR"), "/library_entries.rs"))`
- `decoded_library()` — decodes all entries once via `OnceLock`, centres each with `center_cells`
- User patterns stored in `~/.config/newlife/patterns/*.cells`; loaded at startup and after saves via `load_user_patterns`
- `Category::Custom` distinguishes user patterns from built-ins in the browser

### Key invariants to preserve

- `Simulation` has no egui dependency — keep simulation logic independently testable.
- `drag_paint_state: Option<bool>` lives on `GameOfLifeApp` (not `Grid`): set on `drag_started`, cleared on `drag_stopped`.
- Unused high bits in the last word of each row must always be zero (enforced by the mask in `step()` / `compute_4words()` and by `set_bit`).
- Unused padding slots in the last partial tile (rows `height..⌈height/8⌉×8`) are always zero (allocated by `tiled_size` via `vec![0u64; n]`, never written by `set_bit`).
- `live_bbox` is expanded conservatively (never shrunk except by `clear()` or `step()`).
- `step_4words_avx2` is `unsafe` and `#[target_feature(enable = "avx2")]`; it must only be called inside an `is_x86_feature_detected!("avx2")` runtime guard — never unconditionally.
- `CANON_EMPTY = u32::MAX` is the `CanonTable` empty-slot sentinel; `NodeId` `u32::MAX` is never a valid node index.
- `needs_expansion_deep_inner()` must be checked alongside `needs_expansion_inner()` in `step_universe`'s expansion loop; omitting it allows cells near the result-window boundary to be silently dropped for expanding patterns (e.g. cordership guns).

## Documentation

When modifying source files, keep **README.md** and **CLAUDE.md** in sync:

- `README.md` — update if features, keyboard shortcuts, or build instructions change
- `CLAUDE.md` — update if modules are added/removed, the frame loop changes, or architectural invariants change
