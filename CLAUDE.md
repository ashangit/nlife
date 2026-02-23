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
```

## Architecture

`newlife` is a Conway's Game of Life desktop app built with egui 0.31 / eframe 0.31 (wgpu renderer, Wayland).

### Module map

| File | Role |
|---|---|
| `main.rs` | Entry point; creates the eframe window |
| `app.rs` | `GameOfLifeApp` — top-level `eframe::App`; owns `Simulation`, `Camera`, `drag_paint_state` |
| `simulation.rs` | `Simulation` — pure state (no egui): grid, timing, speed, `steps_per_frame` |
| `grid.rs` | `Grid` — core data structure; all perf-critical logic lives here |
| `camera.rs` | `Camera` — cell size (zoom), scroll offset, viewport rect |
| `input.rs` | Keyboard shortcuts and Ctrl+scroll / pinch-to-zoom |
| `patterns.rs` | `Pattern` enum with const `(Δrow, Δcol)` slices for every preset |
| `ui/panel.rs` | Top control panel (buttons, sliders, pattern picker) |
| `ui/grid_view.rs` | Grid canvas: mouse paint/erase, viewport-culled cell rendering |

### Frame loop (`app.rs::update`)

```
handle_keyboard → handle_zoom → advance_simulation → draw_top_panel → draw_grid
```

`advance_simulation` caps `dt` at 0.1 s to avoid a first-frame spike, then calls `sim.advance(dt)` which steps in multiples of `1/speed`. Each step also calls `expand_if_needed`, returning `(add_top, add_left)` rows/cols so `Camera::apply_expansion` can compensate `scroll_offset`.

### Grid internals (`grid.rs`) — three interleaved optimisations

1. **Bit-packed storage** — `Vec<u64>`, row-major, 64 cells per word (LSB = leftmost). `words_per_row = ⌈width/64⌉`. 8× memory reduction vs `Vec<bool>`.
2. **Active-cell frontier** — `frontier: HashSet<(row, col)>` holds every live cell and its Moore neighbourhood. `step()` maps frontier entries to `(row, word_index)` pairs, evaluating only those words — O(live + border) per step.
3. **SWAR kernel** (`step_word`) — evaluates all 64 bit positions of a word simultaneously via carry-save adder trees (~30 bitwise ops) instead of 64 individual neighbour loops.

Double-buffer: `cells` (current) and `next` (scratch) swap each step. `prev_written_words` tracks which `next` words need zeroing at the start of the following step.

Auto-expand: after each step, if live cells touch any edge, `MARGIN = 20` dead rows/cols are prepended on that side and the scroll offset is shifted to keep the view stable.

### Key invariants to preserve

- `Simulation` has no egui dependency — keep simulation logic independently testable.
- `drag_paint_state: Option<bool>` lives on `GameOfLifeApp` (not `Grid`): set on `drag_started`, cleared on `drag_stopped`.
- Unused high bits in the last word of each row must always be zero (enforced by the mask in `step()` and by `set_bit`).
- `live_bbox` is expanded conservatively (never shrunk except by `clear()` or `step()`).
