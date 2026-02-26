# TODO — Newlife Improvement Backlog

Items are grouped by theme. Within each section, entries are roughly ordered from
highest to lowest impact / easiest to hardest.

---

## 0. Profiling results (measured 2026-02-27)

### Methodology
- CPU: `cargo flamegraph --bench step --profile-time 30` (perf, 997 Hz)
- Cache: `perf stat -e cache-misses,cache-references,instructions,cycles,L1-dcache-load-misses`
- Memory: `heaptrack` on the bench binary, 12–15 s per engine

### SWAR — `grid_step/large_soup` (1024×1024, 20% density ≈ 209 k live cells)

**CPU flamegraph top symbols**
| Symbol | % of samples | Notes |
|--------|-------------|-------|
| `core::slice::sort::unstable::quicksort` | **38%** | frontier sort+dedup — #1 hotspot |
| `[unknown]` / inlined | ~70% | includes `step_word` kernel (fully inlined) |
| `grid::add_word_neighborhood` | 18% | frontier expansion per live word |
| `Vec::push` → `grow_amortized` | 16% | `next_frontier` buffer growing |
| `make_random_grid` + `Grid::set` (setup) | ~21% | benchmark setup, not hot path |

**Cache stats (10 s window)**
| Counter | Value | Notes |
|---------|-------|-------|
| Cache-miss rate | **3.84%** | low — data is mostly cache-friendly |
| L1-dcache-load-misses | 995 M | from random frontier access pattern |
| IPC (insn/cycle) | **2.23** | good — CPU is compute-bound |

**Memory (heaptrack, 14.7 s)**
| Metric | Value |
|--------|-------|
| Peak heap | 38.65 MB |
| Allocation rate | 642 allocs/s |
| Temporary allocs | 6923 / 471 per s |
| Main allocation site | `add_word_neighborhood` → `Vec::push` → `grow_amortized` |

**Key SWAR findings**
- Sort accounts for **38% of CPU time**; replacing sort+dedup with a hash-set
  frontier would eliminate it entirely.
- The `step_word` SWAR kernel is fully inlined and does not appear by name —
  it is fast enough to hide inside `[unknown]`. It is likely not the bottleneck.
- `add_word_neighborhood` (18%) is the next target after sort.
- Memory pressure is low (3.84% cache-miss rate, 2.23 IPC) — the bottleneck
  is algorithmic (sort) not memory bandwidth.
- Reuse buffers (`results_buf`, `next_frontier`) work: allocation rate is low
  (642/s), but `next_frontier` still triggers `grow_amortized` when the frontier
  size exceeds the retained capacity from the previous step.

---

### HashLife — `hashlife/cordership_gun_step_universe`

**CPU flamegraph top symbols**
| Symbol | % of samples | Notes |
|--------|-------------|-------|
| `[unknown]` / inlined | ~62% | heavily inlined node operations |
| `HashLife::make_node` | **29%** | HashMap insert for new nodes — #1 hotspot |
| `HashLife::step_recursive` | ~34% | appears twice (recursive calls) |
| `HashMap::get` / `hashbrown::find_inner` | **14% / 12%** | memo cache lookup |
| `Vec::collect` / `from_iter` (setup) | ~11% | `live_cells_in_viewport` warmup |

**Cache stats (10 s window)**
| Counter | Value | Notes |
|---------|-------|-------|
| Cache-miss rate | **32.66%** | very high — 8.5× worse than SWAR |
| L1-dcache-load-misses | 554 M | random HashMap probing pattern |
| IPC (insn/cycle) | **0.76** | poor — CPU is heavily memory-stalled |

**Memory (heaptrack, 12.4 s)**
| Metric | Value |
|--------|-------|
| Peak heap | **241.29 MB** | 6× more than SWAR for the same pattern |
| Allocation rate | **3338 allocs/s** | 5× more than SWAR |
| Temporary allocs | 37 675 / 3041 per s |
| HashMap resizes | 68 `prepare_resize` calls | per pattern load |
| Nodes Vec resizes | 68 `grow_amortized` calls | matching HashMap growth |

**Key HashLife findings**
- HashLife is **memory-bound, not compute-bound** (IPC 0.76 vs SWAR 2.23).
  The CPU spends most of its time waiting for cache misses from random HashMap
  probing, not executing instructions.
- Cache-miss rate (32.66%) is the root cause: each `HashMap::get` and `make_node`
  insert jumps to a pseudo-random memory location, causing LLC misses.
- **Switching from `std::HashMap` to an open-addressing table with better locality
  (e.g. a purpose-built flat array) is the single highest-impact HashLife change.**
- The node table never shrinks: 241 MB peak vs 38 MB for SWAR. GC would
  reduce memory pressure and improve cache locality for long-running sessions.
- `make_node` at 29% handles both new-node insertion and step-result memoisation.
  A faster intern table would cut both costs simultaneously.

---

## 1. Performance — SWAR Engine

**1.1 — Replace sort+dedup frontier with FxHashSet** ★ *Confirmed #1 by profiling*
Sort accounts for **38%** of SWAR step time for large patterns. Replace the
`Vec<(row, wi)>` + `sort_unstable` + `dedup` frontier with a
`FxHashSet<(usize, usize)>`. Insertion is O(1) average; iteration is O(n).
No sort needed. The overhead of hashing a `(usize, usize)` pair is negligible
vs a quicksort of 14 000 elements.
Expected gain: ~35–40% reduction in step time for large patterns.

**1.2 — Reduce `add_word_neighborhood` cost** ★ *#2 hotspot at 18%*
`add_word_neighborhood` pushes up to 9 words per live word into `next_frontier`,
which causes `Vec::push` → `grow_amortized` to fire. With a `FxHashSet` frontier
(TODO 1.1), insertion already becomes O(1) with no reallocation. If sticking
with `Vec`, pre-size `next_frontier` to `frontier.len() * 9` before the loop.

**1.3 — Parallelise frontier rebuild**
Only the `compute_word` phase is Rayon-parallelised; the subsequent frontier
rebuild loop (`add_word_neighborhood` for each live result) is sequential.
Collect new frontier entries in parallel (par_iter + flatten into a scratch
buffer), then merge-sort and dedup as today, or insert into a concurrent set.

**1.4 — AVX2 / SIMD kernel** *(lower priority — kernel already fast)*
Profiling shows `step_word` is fully inlined and absent from CPU samples —
it is already not the bottleneck. Implement AVX2 only after items 1.1–1.3 are
done. Replace scalar `step_word` with a 4×u64 AVX2 path under
`#[cfg(target_feature = "avx2")]`.

**1.5 — Tiled grid layout for cache locality**
L1-dcache-load-misses: 995M for the large soup. A tiled layout (8-row × 1-word
tiles) keeps a word and its row-neighbours in the same cache line. Lower
priority than 1.1 given the already-acceptable 3.84% miss rate.

---

## 2. Performance — HashLife Engine

**2.1 — Open-addressing node intern table** ★ *Root cause of 32% cache-miss rate*
Replace `HashMap<(NodeId,NodeId,NodeId,NodeId), NodeId>` with a flat
open-addressing table (Robin Hood or quadratic probing). Better spatial locality
means fewer LLC misses per node lookup. This directly attacks the 32.66%
cache-miss rate and 0.76 IPC.
A purpose-built table avoids the `hashbrown` overhead visible in flamegraph
(`find_inner` 12%, `make_node` 29%).

**2.2 — Garbage collection** ★ *Root cause of 241 MB peak heap*
The node table grows to 241 MB and never shrinks (vs 38 MB for SWAR). For
long-running sessions memory continues to grow unboundedly, worsening cache
pressure over time. Add mark-and-sweep GC from the root after each
`step_universe` call (or when table exceeds a threshold), freeing unreachable
nodes and compacting the table.

**2.3 — 4×4 → 2×2 lookup table for `step_level2`**
Replace the brute-force bitop loop in `step_level2` with a precomputed
`[u16; 65536]` table. One array access replaces ~60 bitops. Easy to implement;
reduces `step_recursive` leaf call cost. Profiling shows `step_recursive`
at ~34%; the level-2 base case is called millions of times per `step_universe`.

**2.4 — `FxHashMap` instead of `std::HashMap`**
The flamegraph shows `hashbrown::find_inner` at 12% — using `rustc-hash`'s
`FxHashMap` (already a dependency) instead of `std::HashMap` reduces hashing
cost. This is a one-line change in `hashlife.rs` and a free win before
implementing the full open-addressing table (2.1).

**2.5 — Variable step size**
Currently `step_universe` always advances by `2^(level-2)` generations.
Accept a target step count and expand/contract the root level on demand so
the user can step by exactly 1, 8, or 1024 generations interactively.

**2.6 — Parallel subtree traversal**
Independent quadrants at a given level have no data dependencies.
Use Rayon `join` to evaluate the four quadrant `step_recursive` calls in
parallel. However, given the current 0.76 IPC (memory-bound), parallelism
will not help until cache locality is improved (2.1, 2.2).

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
