# TODO — Newlife Improvement Backlog

Items are grouped by theme. Within each section, entries are roughly ordered from
highest to lowest impact / easiest to hardest.

---

## 0. Profiling — before optimising, measure

**0.1 — CPU flamegraph (SWAR, bench)**
Run `cargo flamegraph --bench step -- --bench large_soup --profile-time 30`
to produce `flamegraph.svg`. Validate which functions dominate: `step_word`,
frontier sort/dedup, `add_word_neighborhood`, or Rayon scheduling overhead.
Tools already installed: `flamegraph`, `perf`, `cargo-flamegraph`.

**0.2 — CPU flamegraph (HashLife, bench)**
Run `cargo flamegraph --bench step -- --bench cordership_gun_step_universe --profile-time 30`.
Expected hotspots: `step_recursive` cache lookup, `make_node` hashing,
`step_level2` bitops. Confirm before implementing 2.1–2.5.

**0.3 — CPU flamegraph (running app)**
Attach `perf record -F 997 -g -p $(pgrep newlife) -- sleep 10` to the app
while running a large pattern at max speed. Convert with
`perf script | inferno-collapse-perf | inferno-flamegraph > app_flamegraph.svg`.
Reveals rendering vs simulation split and UI hotspots.

**0.4 — Memory profile (SWAR)**
Use `heaptrack cargo bench --bench step -- --bench large_soup` (or
`valgrind --tool=massif`) to measure peak allocation and allocation rate for
`Grid::step()`. Confirms whether `results_buf`/`next_frontier` reuse fully
eliminates per-step heap traffic, or if other sources remain.

**0.5 — Memory profile (HashLife)**
Same tooling on `cordership_gun_step_universe`. Primary question: how fast
does the node table grow, and what fraction of memory is live (reachable)
vs dead (GC-collectable)? Informs priority of TODO 2.2.

**0.6 — Cache miss analysis**
Run `perf stat -e cache-misses,cache-references,instructions,cycles cargo bench
--bench step -- --bench large_soup` for SWAR and
`-- --bench cordership_gun_step_universe` for HashLife.
Cache-miss rate per instruction guides layout optimisations (1.4, 2.3).

---

## 1. Performance — SWAR Engine

**1.1 — AVX2 / SIMD kernel**
Replace the scalar `step_word` with an AVX2 implementation that processes 4×u64
words per SIMD instruction. The carry-save adder tree maps naturally to 256-bit
bitops. Use `#[cfg(target_feature = "avx2")]` with a scalar fallback.
Expected gain: ~4× throughput on supported hardware.

**1.2 — Parallel frontier rebuild**
Only the `compute_word` phase is Rayon-parallelised; the subsequent frontier
rebuild loop (`add_word_neighborhood` for each live result) is sequential.
Collect new frontier entries in parallel (par_iter + flatten into a scratch
buffer), then merge-sort and dedup as today.
Expected gain: removes the sequential bottleneck for large grids.

**1.3 — HashSet-based frontier for sparse grids**
For very sparse patterns, sorting + dedup of `Vec<(row, wi)>` is O(n log n).
A `FxHashSet<(usize, usize)>` frontier gives O(n) total work and avoids the
sort entirely. Switch representation when `frontier.len() < threshold`.

**1.4 — Tiled grid layout for cache locality**
Current row-major u64 layout causes cache misses when `step_word` reads
neighbours in adjacent rows. A tiled layout (e.g. 8-row × 1-word tiles)
keeps a cell and all its row-neighbours in the same cache line.

**1.5 — Multi-step per `advance()` call**
For tiny frontiers (blinker: 3 words) the per-call overhead dominates.
Run `k` SWAR steps inside a single `advance()` tick before returning to egui,
instead of relying on `steps_per_frame` to amortise this at the frame level.

---

## 2. Performance — HashLife Engine

**2.1 — 4×4 → 2×2 lookup table**
Replace the brute-force bitop loop in `step_level2` with a precomputed
`[u16; 65536]` table indexed by the 16-bit encoding of a 4×4 cell block.
One array access replaces ~60 operations; trivial to implement.

**2.2 — Garbage collection**
Nodes are never freed. For long-running simulations the node table grows
unboundedly, polluting the CPU cache and eventually exhausting RAM.
Add mark-and-sweep GC from the root after each `step_universe` call (or
when the table exceeds a size threshold), freeing unreachable nodes.

**2.3 — Open-addressing node intern table**
Replace `HashMap<(NodeId,NodeId,NodeId,NodeId), NodeId>` with a flat
open-addressing table (Robin Hood or quadratic probing). Better cache
locality, no heap allocation per entry, ~2× faster node lookup.

**2.4 — Variable step size**
Currently `step_universe` always advances by `2^(level-2)` generations.
Accept a target step count and expand/contract the root level on demand so
the user can request exactly 1, 8, or 1024 generations interactively.

**2.5 — Parallel subtree traversal**
Independent quadrants at a given level have no data dependencies.
Use Rayon `join` to evaluate the four quadrant `step_recursive` calls in
parallel. Benefit is greatest for large, highly structured patterns.

---

## 3. UI / User Experience

**3.1 — "Center / zoom to fit" (`F` key)**
Pan and zoom the camera so all live cells fill the viewport.
Compute `live_bbox` from SWAR's `grid.live_bbox` / HashLife's tree, derive
the required zoom level and scroll offset, and animate to it.

**3.2 — Actual throughput display**
Show the measured simulation rate next to the generation counter:
`Gen: 42 105 | 8 342 gen/s`. Compute as a rolling average over the last
~0.5 s of `advance()` calls. Helps users tune `speed` and `steps_per_frame`.

**3.3 — Drag-and-drop pattern loading**
Detect `egui::Event::DroppedFile` and load `.rle` / `.cells` files dropped
onto the window. Removes the friction of the file picker dialog.

**3.4 — Step N generations**
Add a numeric input + "⏭ ×N" button that advances exactly N steps while
paused. Complements the existing single-step button for jumping ahead.

**3.5 — Undo cell edits (`Ctrl+Z`)**
Keep a small ring buffer of pre-edit grid snapshots (or cell-delta lists).
Ctrl+Z restores the previous state; useful when drawing patterns interactively.

**3.6 — Rectangular selection + copy/paste**
Shift-drag to select a bounding box. `Ctrl+C` copies the live cells within it;
`Ctrl+V` pastes them at the cursor position. `Del` erases the selection.

**3.7 — Loaded pattern name in panel**
When a library or user pattern is loaded, display its name in the top panel
(e.g. "Gosper Glider Gun"). Store as `Simulation::pattern_name: Option<String>`.

**3.8 — Animated browser thumbnails**
When the user hovers a pattern entry in the browser, run 5–10 `step()` calls
on the preview grid and redraw it each frame to show the pattern is alive.
Pause the animation after a full period or 60 frames.

**3.9 — RLE export**
Add an "Export RLE" button alongside "💾 Save". Reuse the existing
`parse_rle` infrastructure in reverse (an `encode_rle` fn in `rle.rs`).

**3.10 — Middle-mouse-button pan**
Middle-click-drag to pan the grid view, as an alternative to the scroll-bar
panning already available. Standard in most 2-D canvas editors.

**3.11 — Toroidal boundary mode**
Toggle in the panel to make the grid wrap at both edges. Requires a small
change to `step_word` neighbour indexing (modular row/column fetch).

**3.12 — Resizable browser panel**
Replace the fixed-width left panel with an `egui::SidePanel` that the user
can drag to resize. Useful when browsing patterns with long names.
