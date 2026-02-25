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

