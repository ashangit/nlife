# TODO — Newlife Improvement Backlog

Items are grouped by theme. Within each section, entries are roughly ordered from
highest to lowest impact / easiest to hardest.

---

## 1. Performance — Simulation Engine

- [ ] **Word-level frontier** — `frontier` currently stores per-cell `(row, col)` pairs and
  rebuilds a `HashSet<(row, word_index)>` inside `step()` every generation.  Maintaining
  the frontier directly at word granularity eliminates this per-step conversion and shrinks
  the set size up to 64×.  The frontier would store `(row, wi)` triples and pre-expand to
  include the three adjacent word columns (wi-1, wi, wi+1) for each active row, matching
  exactly the 3×3 word neighbourhood read by `step_word`.

- [ ] **Rayon parallel step loop** — the inner loop `for &(row, wi) in &word_set { … }`
  in `step()` is embarrassingly parallel: each word depends only on its 8 neighbours from
  the *previous* generation (`self.cells`, read-only) and writes to a distinct slot in
  `self.next`.  Switching to `word_set.par_iter()` with Rayon would give near-linear
  speedup for large patterns (guns, large methuselahs).  The `new_frontier` accumulation
  would need a `Mutex<HashSet>` or a per-thread staging vec merged at the end.

- [ ] **Replace HashSet with sorted Vec for frontier** — `HashSet<(usize, usize)>` has poor
  cache locality due to bucket pointer-chasing.  The frontier entries are spatially
  correlated (clustered around live regions), so a `Vec<(usize, usize)>` that is sorted
  and deduped after each step would be more cache-friendly and cheaper to iterate.  Profile
  first to confirm this is a net win over the amortised O(1) hash insertions.

- [ ] **Word-copy in `expand_if_needed`** — the expansion loop copies cells via individual
  `get_bit` / `set_bit` calls (O(W × H) iterations).  The new grid is a bit-shifted
  version of the old one: each row can be copied by shifting whole `u64` words left by
  `add_left % 64` bits and OR-ing with the neighbouring word, reducing the copy to
  O(H × wpr) word operations — up to 64× fewer iterations.

- [ ] **Replace frontier `HashSet` with `FxHashSet`** — profiling shows ~48 % of CPU is
  spent on `HashSet<(usize, usize)>` hashing via SipHash-13 (`add_neighborhood` 19.6 %,
  `hash_one` 14.1 %, `Hasher::write` 10.4 %).  Switching to `FxHashSet` from the
  `rustc-hash` crate (multiplicative hash, ~3 ns vs ~15 ns per lookup for integer keys)
  is a 2-line change (`Cargo.toml` + type alias) and should give a ~2–3× speedup on the
  frontier-heavy hot path.

- [ ] **HashLife algorithm** — implement the quadtree-based HashLife algorithm
  (see <https://johnhw.github.io/hashlife/index.md.html>) for O(log N) amortised steps per
  generation on periodic or highly-repetitive patterns.  HashLife memoises 2^k × 2^k
  quadtree nodes by content hash, enabling exponential time-leaps; it is the standard
  algorithm for long-running complex patterns such as guns, methuselahs, and
  self-replicators where the current SWAR frontier approach scales linearly.

---

## 2. Performance — Pattern Browser

- [ ] **Virtual scrolling** — all matching patterns (potentially 1 000+) are currently
  allocated in the egui layout every frame, even when only ~10 rows are visible.  Replace
  the inner `for` loops with `egui::ScrollArea::show_rows(row_height, total_rows, |ui, range| { … })`
  so egui only invokes the closure for the visible row range.  This requires knowing the
  total filtered count and mapping row indices back to library entries, so build a
  `Vec<BrowserRow>` index (built-in + custom, post-filter) once per filter change.

- [ ] **Cached preview images** — `draw_preview()` recomputes the bounding box, scale
  factor, and origin for every visible pattern on every frame, then issues O(live_cells)
  `painter.rect_filled` calls.  Pre-render each pattern to an `egui::ColorImage` (40×40
  RGBA) once, upload it as a retained `egui::TextureHandle` (stored in a
  `HashMap<String, TextureHandle>` on `GameOfLifeApp`), and display with a single
  `ui.image()` call.  Cache invalidation: re-render when user patterns change.

- [ ] **Pre-filtered index with change detection** — the browser re-filters the full library
  on every frame by iterating all entries and checking category + name.  Instead, cache a
  `Vec<usize>` of matching indices and only recompute it when `browser_category` or
  `browser_search` change (compare against the values used to build the last cache).  With
  virtual scrolling this cache is the total-row count needed by `show_rows`.

- [ ] **Lazy `decoded_library` access** — `decoded_library()` decodes all entries once into
  a `Vec` via `OnceLock`, but the browser still walks the full slice.  Once virtual
  scrolling + a pre-filtered index are in place, only O(visible_rows) entries will be
  accessed per frame, so the remaining cost is dominated by the texture lookup, which is O(1).

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

## 5. Pattern Format / Metadata

- [ ] **Parse `#C` / `#O` comment lines from RLE files** — the RLE format uses `#C text`
  for a description and `#O author` for attribution.  Currently `parse_rle` silently skips
  all `#` lines.  Extract these into `LibraryEntry` fields (e.g. `description: Option<String>`,
  `author: Option<String>`) and display them as a tooltip or info panel in the pattern
  browser so users know what each pattern does and who created it.

- [ ] **Respect the `rule = …` field in RLE headers** — the `x = N, y = M, rule = B3/S23`
  header line is currently skipped entirely by `parse_rle`.  The `rule` field encodes
  birth/survival conditions in Golly's B/S notation (e.g. `B36/S23` for HighLife,
  `B368/S245` for Move).  Parsing it would let the app warn the user when a loaded pattern
  targets a non-standard rule and will not behave as described under the default B3/S23
  simulation.  The `x` / `y` bounding-box values can also be used to pre-size the
  `Vec<(i32, i32)>` returned by the parser (minor allocation win).
