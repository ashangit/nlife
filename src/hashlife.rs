//! HashLife — quadtree-memoised Conway's Game of Life engine.
//!
//! Each canonical node is interned once and identified by a [`NodeId`] (`u32`
//! index into [`HashLife::nodes`]).  The two level-0 leaf nodes occupy fixed
//! slots: `DEAD = 0` and `ALIVE = 1`.
//!
//! The step advances the universe by **2^(level−2)** generations per call —
//! an exponential speed-up for repetitive/periodic patterns.

use rustc_hash::{FxHashMap, FxHasher};
use std::hash::Hasher;
use std::sync::{Arc, Mutex, OnceLock};

/// Index into the `HashLife::nodes` arena.  0 = DEAD leaf, 1 = ALIVE leaf.
pub(crate) type NodeId = u32;

// ── CanonTable ────────────────────────────────────────────────────────────────

/// Sentinel value for an empty slot in [`CanonTable`].
const CANON_EMPTY: u32 = u32::MAX;

/// A single slot in the open-addressing intern table.
///
/// All five fields are packed contiguously (20 bytes), so sequential
/// linear-probe steps stay within the same or adjacent cache lines.
#[derive(Copy, Clone)]
#[repr(C)]
struct CanonEntry {
    nw: u32,
    ne: u32,
    sw: u32,
    se: u32,
    /// `CANON_EMPTY` (u32::MAX) when the slot is vacant.
    id: u32,
}

/// Purpose-built flat open-addressing hash table for node canonicalisation.
///
/// Key = `(nw, ne, sw, se)` → `NodeId`.  Load factor is kept ≤ 75 % before
/// doubling.  Linear probing keeps all probe steps within contiguous memory.
struct CanonTable {
    entries: Vec<CanonEntry>,
    /// `capacity - 1`; capacity is always a power of two.
    mask: usize,
    len: usize,
}

impl CanonTable {
    /// Creates a new table with at least `cap` slots, rounded up to a power of 2.
    fn with_capacity(cap: usize) -> Self {
        let capacity = cap.next_power_of_two().max(8);
        let empty = CanonEntry {
            nw: 0,
            ne: 0,
            sw: 0,
            se: 0,
            id: CANON_EMPTY,
        };
        Self {
            entries: vec![empty; capacity],
            mask: capacity - 1,
            len: 0,
        }
    }

    /// Computes the slot index for `(nw, ne, sw, se)`.
    #[inline]
    fn hash_slot(&self, nw: u32, ne: u32, sw: u32, se: u32) -> usize {
        let mut h = FxHasher::default();
        h.write_u64((nw as u64) | ((ne as u64) << 32));
        h.write_u64((sw as u64) | ((se as u64) << 32));
        (h.finish() as usize) & self.mask
    }

    /// Looks up `(nw, ne, sw, se)` and returns the interned `NodeId`, or
    /// `None` if not present.
    #[inline]
    fn get(&self, nw: u32, ne: u32, sw: u32, se: u32) -> Option<u32> {
        let mut slot = self.hash_slot(nw, ne, sw, se);
        loop {
            let e = self.entries[slot];
            if e.id == CANON_EMPTY {
                return None;
            }
            if e.nw == nw && e.ne == ne && e.sw == sw && e.se == se {
                return Some(e.id);
            }
            slot = (slot + 1) & self.mask;
        }
    }

    /// Inserts `(nw, ne, sw, se) → id`.
    ///
    /// Resizes (double capacity + rehash) when the load factor exceeds 75 %.
    /// The caller guarantees the key is not already present.
    fn insert(&mut self, nw: u32, ne: u32, sw: u32, se: u32, id: u32) {
        // Grow before insertion so the probe always terminates.
        if self.len * 4 >= self.entries.len() * 3 {
            self.grow();
        }
        let mut slot = self.hash_slot(nw, ne, sw, se);
        loop {
            if self.entries[slot].id == CANON_EMPTY {
                self.entries[slot] = CanonEntry { nw, ne, sw, se, id };
                self.len += 1;
                return;
            }
            slot = (slot + 1) & self.mask;
        }
    }

    /// Clears all entries by refilling with the empty sentinel.
    fn clear(&mut self) {
        let empty = CanonEntry {
            nw: 0,
            ne: 0,
            sw: 0,
            se: 0,
            id: CANON_EMPTY,
        };
        self.entries.fill(empty);
        self.len = 0;
    }

    /// Doubles capacity and rehashes all live entries.
    fn grow(&mut self) {
        let new_cap = (self.entries.len() * 2).max(8);
        let new_mask = new_cap - 1;
        let empty = CanonEntry {
            nw: 0,
            ne: 0,
            sw: 0,
            se: 0,
            id: CANON_EMPTY,
        };
        let mut new_entries = vec![empty; new_cap];
        for e in &self.entries {
            if e.id == CANON_EMPTY {
                continue;
            }
            let mut h = FxHasher::default();
            h.write_u64((e.nw as u64) | ((e.ne as u64) << 32));
            h.write_u64((e.sw as u64) | ((e.se as u64) << 32));
            let mut slot = (h.finish() as usize) & new_mask;
            loop {
                if new_entries[slot].id == CANON_EMPTY {
                    new_entries[slot] = *e;
                    break;
                }
                slot = (slot + 1) & new_mask;
            }
        }
        self.entries = new_entries;
        self.mask = new_mask;
    }
}

/// Level-0 dead leaf (always slot 0).
const DEAD: NodeId = 0;
/// Level-0 alive leaf (always slot 1).
const ALIVE: NodeId = 1;

/// Default grid level on creation: level 8 = 256 × 256 cells.
const DEFAULT_LEVEL: u8 = 8;
/// Minimum level enforced before each step (provides enough dead padding).
const MIN_STEP_LEVEL: u8 = 4;
/// Dead-cell margin (rows/cols) added around a loaded pattern.
const LOAD_MARGIN: usize = 40;
/// Trigger GC when the arena exceeds this many nodes (≈ 28 MB at 28 B/node).
const GC_THRESHOLD: usize = 1 << 20; // 1 048 576

// ── Level-2 lookup table ──────────────────────────────────────────────────────

/// Precomputed 4×4 → 2×2 GoL step table.
///
/// Index: 16 cells of a level-2 node packed into a `u16` (row-major, bit
/// `r*4+c`). Value: 4-bit result (bit0 = r_nw, bit1 = r_ne, bit2 = r_sw,
/// bit3 = r_se) after applying one generation of Conway's rules to the four
/// centre cells (rows/cols 1–2 of the 4×4 grid).
static STEP_LEVEL2_TABLE: OnceLock<Box<[u8; 65536]>> = OnceLock::new();

fn build_step_level2_table() -> Box<[u8; 65536]> {
    let mut table = Box::new([0u8; 65536]);
    for idx in 0u32..65536 {
        let cell = |r: usize, c: usize| -> bool { (idx >> (r * 4 + c)) & 1 != 0 };
        let gol = |r: usize, c: usize| -> u8 {
            let alive = cell(r, c);
            let mut nbrs = 0u32;
            for dr in [-1i32, 0, 1] {
                for dc in [-1i32, 0, 1] {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nr = r as i32 + dr;
                    let nc = c as i32 + dc;
                    if (0..4).contains(&nr) && (0..4).contains(&nc) {
                        nbrs += cell(nr as usize, nc as usize) as u32;
                    }
                }
            }
            (nbrs == 3 || (alive && nbrs == 2)) as u8
        };
        table[idx as usize] = gol(1, 1) | (gol(1, 2) << 1) | (gol(2, 1) << 2) | (gol(2, 2) << 3);
    }
    table
}

// ── Node ─────────────────────────────────────────────────────────────────────

/// An interned quadtree node.
///
/// Level-0 nodes are leaf cells (DEAD / ALIVE). Level-k nodes (`k ≥ 1`) cover
/// a 2^k × 2^k region and have four level-(k-1) children.
///
/// The `pop` field caches the total live-cell count rooted here; it is always
/// exact and is computed at construction time.
#[derive(Copy, Clone, Debug)]
struct Node {
    /// Quadtree level: 0 = leaf, k ≥ 1 = 2^k × 2^k region.
    level: u8,
    /// NW child (or 0 for level-0 nodes).
    nw: NodeId,
    /// NE child.
    ne: NodeId,
    /// SW child.
    sw: NodeId,
    /// SE child.
    se: NodeId,
    /// Exact live-cell count for this subtree.
    pop: u64,
}

// ── HashLife ─────────────────────────────────────────────────────────────────

/// Minimum node level at which `step_recursive` splits into parallel Rayon tasks.
///
/// Level 6 corresponds to a 64×64 cell region — large enough that Rayon task
/// overhead is amortised by the work performed in each subtree.
pub(crate) const PARALLEL_THRESHOLD: u8 = 6;

/// Shared mutable state for a HashLife instance, protected behind `Mutex` locks
/// so that parallel Rayon tasks spawned by `step_recursive` can safely share access.
///
/// Lock ordering (when acquiring both): `nodes` before `canon` before `step_cache`.
struct HashLifeStore {
    /// Arena of all interned nodes; `nodes[0]` = DEAD, `nodes[1]` = ALIVE.
    nodes: Mutex<Vec<Node>>,
    /// Canonicalisation map: `(nw, ne, sw, se)` → `NodeId`.
    canon: Mutex<CanonTable>,
    /// Memoisation cache for `step_recursive`.
    step_cache: Mutex<FxHashMap<NodeId, NodeId>>,
}

/// Quadtree-memoised Game of Life engine.
///
/// Nodes are canonicalised (interned) by their four children; the same
/// quadrant layout is represented by exactly one `NodeId`.  `step_recursive`
/// is memoised in `step_cache`, enabling exponential time-leaps for periodic
/// and repetitive patterns.
///
/// The universe always starts at `DEFAULT_LEVEL` (256×256) and is expanded
/// automatically via [`expand_root`](HashLife::expand_root) as the pattern
/// grows.  The caller should use [`step_universe`](HashLife::step_universe)
/// and adjust the camera by the returned `(gens, expansion)` pair.
pub(crate) struct HashLife {
    /// Shared mutable state (nodes, canon, step_cache) wrapped in `Arc` so
    /// parallel Rayon tasks can borrow it across thread boundaries.
    store: Arc<HashLifeStore>,
    /// Current root node.
    root: NodeId,
    /// Current quadtree level (root covers 2^level × 2^level cells).
    pub(crate) level: u8,
    /// Total generations elapsed since last clear / load.
    pub(crate) generation: u64,
    /// Log₂ of the number of generations to advance per step.
    ///
    /// Each call to [`step_universe`](HashLife::step_universe) advances
    /// `2^step_log2` generations (clamped to `level - 2` when level is too
    /// small).  Default `0` → 1 generation per step.
    pub(crate) step_log2: u8,
}

impl HashLife {
    /// Creates a new `HashLife` with a dead grid at the default level (256×256).
    pub(crate) fn new() -> Self {
        let dead_leaf = Node {
            level: 0,
            nw: 0,
            ne: 0,
            sw: 0,
            se: 0,
            pop: 0,
        };
        let alive_leaf = Node {
            level: 0,
            nw: 0,
            ne: 0,
            sw: 0,
            se: 0,
            pop: 1,
        };
        let store = Arc::new(HashLifeStore {
            nodes: Mutex::new(vec![dead_leaf, alive_leaf]),
            canon: Mutex::new(CanonTable::with_capacity(4096)),
            step_cache: Mutex::new(FxHashMap::default()),
        });
        let mut hl = HashLife {
            store,
            root: 0,
            level: 0,
            generation: 0,
            step_log2: 0,
        };
        let root = hl.make_dead_node(DEFAULT_LEVEL);
        hl.root = root;
        hl.level = DEFAULT_LEVEL;
        hl
    }

    /// Returns the width of the grid in cells: `2^level`.
    #[inline]
    pub(crate) fn width(&self) -> usize {
        1usize << self.level
    }

    /// Returns the height of the grid in cells: `2^level` (always equals width).
    #[inline]
    pub(crate) fn height(&self) -> usize {
        1usize << self.level
    }

    /// Returns the alive/dead state of the cell at absolute `(row, col)`.
    ///
    /// Returns `false` for out-of-bounds coordinates.
    pub(crate) fn get(&self, row: usize, col: usize) -> bool {
        let size = self.width();
        if row >= size || col >= size {
            return false;
        }
        self.get_cell_in(self.root, row, col, self.level)
    }

    /// Sets the alive/dead state of the cell at absolute `(row, col)`.
    ///
    /// Does nothing for out-of-bounds coordinates.  Invalidates the step cache
    /// because previously memoised results no longer hold.
    pub(crate) fn set(&mut self, row: usize, col: usize, alive: bool) {
        let size = self.width();
        if row >= size || col >= size {
            return;
        }
        let level = self.level;
        let root = self.root;
        self.root = self.set_cell_in(root, row, col, alive, level);
        self.store.step_cache.lock().unwrap().clear();
    }

    /// Toggles the alive/dead state of the cell at absolute `(row, col)`.
    ///
    /// Does nothing for out-of-bounds coordinates.
    pub(crate) fn toggle(&mut self, row: usize, col: usize) {
        let current = self.get(row, col);
        self.set(row, col, !current);
    }

    /// Resets the universe to a fresh dead grid at the default level.
    pub(crate) fn clear(&mut self) {
        self.store.nodes.lock().unwrap().truncate(2); // keep DEAD and ALIVE leaves
        self.store.canon.lock().unwrap().clear();
        self.store.step_cache.lock().unwrap().clear();
        let root = self.make_dead_node(DEFAULT_LEVEL);
        self.root = root;
        self.level = DEFAULT_LEVEL;
        self.generation = 0;
    }

    /// Fills the grid randomly using an xorshift64 PRNG seeded with `seed`.
    ///
    /// Resets to the default level first.  Cells are set alive with probability
    /// `density_pct / 100`.
    ///
    /// # Arguments
    /// * `density_pct` — target live-cell percentage (0–100)
    /// * `seed`        — PRNG seed; 0 is silently treated as 1
    pub(crate) fn fill_random(&mut self, density_pct: u8, seed: u64) {
        self.clear();
        let density = density_pct.min(100) as u128;
        let threshold = density * (u64::MAX as u128 + 1) / 100;
        let size = self.width();
        let mut state = if seed == 0 { 1 } else { seed };
        for row in 0..size {
            for col in 0..size {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                if (state as u128) < threshold {
                    self.set(row, col, true);
                }
            }
        }
        self.store.step_cache.lock().unwrap().clear();
    }

    /// Clears the grid and centres the given cell offsets, auto-sizing the
    /// universe to fit the pattern with `LOAD_MARGIN` dead cells on each side.
    ///
    /// Compatible with `Grid::set_cells`: offsets are added to `(half, half)`
    /// where `half = 2^(level−1)`.
    ///
    /// # Arguments
    /// * `cells` — centred `(row_offset, col_offset)` pairs
    pub(crate) fn set_cells(&mut self, cells: &[(i32, i32)]) {
        // Compute the level required to fit all cells with margin.
        let level = if cells.is_empty() {
            DEFAULT_LEVEL
        } else {
            let max_abs = cells
                .iter()
                .flat_map(|&(r, c)| [r.unsigned_abs() as usize, c.unsigned_abs() as usize])
                .max()
                .unwrap_or(0);
            let required_half = max_abs + LOAD_MARGIN + 1;
            let mut lv = DEFAULT_LEVEL;
            while (1usize << lv.saturating_sub(1)) < required_half {
                lv += 1;
            }
            lv
        };

        // Reset to a fresh dead root at the required level.
        self.store.nodes.lock().unwrap().truncate(2);
        self.store.canon.lock().unwrap().clear();
        self.store.step_cache.lock().unwrap().clear();
        let root = self.make_dead_node(level);
        self.root = root;
        self.level = level;
        self.generation = 0;

        let half = self.width() / 2;
        for &(dr, dc) in cells {
            let row = (half as i64 + dr as i64) as usize;
            let col = (half as i64 + dc as i64) as usize;
            let size = self.width();
            if row < size && col < size {
                // Use direct tree-update to avoid redundant cache clears.
                let lv = self.level;
                let rt = self.root;
                self.root = self.set_cell_in(rt, row, col, true, lv);
            }
        }
        self.store.step_cache.lock().unwrap().clear();
    }

    /// Returns all live cells as centred `(row_offset, col_offset)` pairs.
    ///
    /// Offsets are relative to `(height/2, width/2)`, compatible with
    /// [`set_cells`](HashLife::set_cells).
    pub(crate) fn live_cells_offsets(&self) -> Vec<(i32, i32)> {
        let size = self.width();
        let half = size / 2;
        let mut raw = Vec::new();
        self.collect_live_in_rect(self.root, 0, 0, size, 0, 0, size, size, &mut raw);
        raw.iter()
            .map(|&(r, c)| (r as i32 - half as i32, c as i32 - half as i32))
            .collect()
    }

    /// Returns all live cells as `(row, col)` pairs normalised to top-left = `(0, 0)`.
    ///
    /// Intended for use when saving patterns to disk.
    pub(crate) fn live_cells_for_save(&self) -> Vec<(usize, usize)> {
        let size = self.width();
        let mut cells = Vec::new();
        self.collect_live_in_rect(self.root, 0, 0, size, 0, 0, size, size, &mut cells);
        if cells.is_empty() {
            return cells;
        }
        let row_min = cells.iter().map(|&(r, _)| r).min().unwrap();
        let col_min = cells.iter().map(|&(_, c)| c).min().unwrap();
        cells
            .iter()
            .map(|&(r, c)| (r - row_min, c - col_min))
            .collect()
    }

    /// Returns all live cells within the given viewport rectangle (inclusive bounds).
    ///
    /// Efficiently prunes branches with `pop == 0` or no bounding-box
    /// intersection, giving O(live cells in viewport + tree depth) performance.
    ///
    /// # Arguments
    /// * `row_min`, `col_min` — inclusive start of the query rectangle
    /// * `row_max`, `col_max` — exclusive end (i.e. the range is `[min, max)`)
    pub(crate) fn live_cells_in_viewport(
        &self,
        row_min: usize,
        col_min: usize,
        row_max: usize,
        col_max: usize,
    ) -> Vec<(usize, usize)> {
        let size = self.width();
        let mut out = Vec::new();
        self.collect_live_in_rect(
            self.root, 0, 0, size, row_min, col_min, row_max, col_max, &mut out,
        );
        out
    }

    /// Returns the total live-cell count (`nodes[root].pop`).
    #[inline]
    pub(crate) fn population(&self) -> u64 {
        self.store.nodes.lock().unwrap()[self.root as usize].pop
    }

    /// Sets the log₂ of the step size and clears the step cache if it changed.
    ///
    /// The value is clamped to 62 to avoid shifting past `u64` range.
    /// Because `j` is baked into the memoised step results, the cache must be
    /// invalidated whenever it changes.
    pub(crate) fn set_step_log2(&mut self, j: u8) {
        let j = j.min(62);
        if j != self.step_log2 {
            self.step_log2 = j;
            self.store.step_cache.lock().unwrap().clear();
        }
    }

    /// Advances the universe by `2^effective_j` generations, where
    /// `effective_j = step_log2.min(level - 2)`.
    ///
    /// Expands the root as needed, runs `step_recursive`, re-centres the
    /// result, and returns `(gens_advanced, expansion_per_side)` where
    /// `expansion_per_side` is the number of cells added to each of the four
    /// sides due to `expand_root` calls (used for camera scroll compensation).
    pub(crate) fn step_universe(&mut self) -> (u64, usize) {
        if self.store.nodes.lock().unwrap().len() > GC_THRESHOLD {
            self.gc();
        }

        let mut expansion: usize = 0;

        // Ensure at least MIN_STEP_LEVEL (and enough room for the requested
        // step size), that outer quadrants are empty, and that live cells are
        // not in the near-boundary band of the safe zone.
        // The deeper check (`needs_expansion_deep_inner`) prevents cells from
        // drifting out of the step-result window during a step; see its
        // doc-comment.  All three conditions are checked under a single lock
        // acquisition per iteration to reduce mutex overhead.
        let min_level = (self.step_log2 as u32 + 2).max(MIN_STEP_LEVEL as u32) as u8;
        loop {
            let needs_expand = {
                let nodes = self.store.nodes.lock().unwrap();
                self.level < min_level
                    || needs_expansion_inner(&nodes, self.root)
                    || needs_expansion_deep_inner(&nodes, self.root, self.level)
            };
            if !needs_expand {
                break;
            }
            expansion = expansion.saturating_add(1usize << self.level.saturating_sub(1));
            self.expand_root();
        }

        // Recompute effective_j now that level may have grown from expansion.
        let effective_j = self.step_log2.min(self.level.saturating_sub(2));
        let gens = 1u64 << effective_j;

        // Compute the center half of the universe advanced by gens generations.
        let result = step_recursive(&self.store, self.root, effective_j);

        // Re-centre: wrap result back into a same-level root with dead padding.
        let (result_level, r_nw, r_ne, r_sw, r_se) = {
            let nodes = self.store.nodes.lock().unwrap();
            let n = nodes[result as usize];
            (n.level, n.nw, n.ne, n.sw, n.se)
        };
        let d = self.make_dead_node(result_level.saturating_sub(1));
        let new_nw = self.make_node(d, d, d, r_nw);
        let new_ne = self.make_node(d, d, r_ne, d);
        let new_sw = self.make_node(d, r_sw, d, d);
        let new_se = self.make_node(r_se, d, d, d);
        self.root = self.make_node(new_nw, new_ne, new_sw, new_se);
        // level stays the same.

        self.generation += gens;
        (gens, expansion)
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

impl HashLife {
    /// Canonicalises a level-k node by interning it in `canon`.
    ///
    /// If the `(nw, ne, sw, se)` tuple already exists, returns its existing
    /// `NodeId`; otherwise creates a new node, computes `pop = sum(children)`,
    /// and inserts into the arena and canon map.
    fn make_node(&mut self, nw: NodeId, ne: NodeId, sw: NodeId, se: NodeId) -> NodeId {
        make_node_in_store(&self.store, nw, ne, sw, se)
    }

    /// Returns the canonical all-dead node at `level` (recursively constructed).
    ///
    /// Memoised via `canon` — each level's dead node is created at most once.
    fn make_dead_node(&mut self, level: u8) -> NodeId {
        make_dead_node_in_store(&self.store, level)
    }

    /// Doubles the root level, centering the current content with dead padding.
    ///
    /// Old level-k root → new level-(k+1) root where old content occupies the
    /// center 2^k × 2^k of the new 2^(k+1) × 2^(k+1) grid.
    fn expand_root(&mut self) {
        let old_level = self.level;
        let d = make_dead_node_in_store(&self.store, old_level - 1);
        let (nw, ne, sw, se) = {
            let r = self.store.nodes.lock().unwrap()[self.root as usize];
            (r.nw, r.ne, r.sw, r.se)
        };
        let new_nw = make_node_in_store(&self.store, d, d, d, nw);
        let new_ne = make_node_in_store(&self.store, d, d, ne, d);
        let new_sw = make_node_in_store(&self.store, d, sw, d, d);
        let new_se = make_node_in_store(&self.store, se, d, d, d);
        self.root = make_node_in_store(&self.store, new_nw, new_ne, new_sw, new_se);
        self.level = old_level + 1;
    }

    // ── Garbage collection ────────────────────────────────────────────────────

    /// Collects unreachable nodes from the arena, compacting it.
    ///
    /// Performs a mark-sweep-compact cycle:
    ///
    /// 1. **Mark** — BFS from `self.root`; `DEAD` and `ALIVE` are always live.
    /// 2. **Remap** — assign consecutive new IDs to reachable nodes.
    /// 3. **Compact** — build a new `nodes` vec with only reachable entries;
    ///    rewrite child pointers of non-leaf nodes through `remap`.
    /// 4. **Rebuild canon** — clear and re-insert every reachable non-leaf node
    ///    using the already-remapped child IDs.
    /// 5. **Remap step_cache** — keep only entries where both the source node
    ///    and the result node survived; translate IDs through `remap`.
    /// 6. **Commit** — update `self.root`, replace `self.nodes`.
    fn gc(&mut self) {
        // Lock all three stores for the duration of GC to prevent concurrent access.
        let mut nodes = self.store.nodes.lock().unwrap();
        let mut canon = self.store.canon.lock().unwrap();
        let mut step_cache = self.store.step_cache.lock().unwrap();

        let n = nodes.len();

        // 1. Mark phase: BFS from root.
        let mut reachable = vec![false; n];
        reachable[DEAD as usize] = true;
        reachable[ALIVE as usize] = true;
        let mut stack: Vec<NodeId> = Vec::new();
        if !reachable[self.root as usize] {
            stack.push(self.root);
        }
        while let Some(id) = stack.pop() {
            if reachable[id as usize] {
                continue;
            }
            reachable[id as usize] = true;
            let node = nodes[id as usize];
            if node.level > 0 {
                for child in [node.nw, node.ne, node.sw, node.se] {
                    if !reachable[child as usize] {
                        stack.push(child);
                    }
                }
            }
        }

        // 2. Build remap: old NodeId → new NodeId (u32::MAX = unreachable).
        let mut remap = vec![u32::MAX; n];
        let mut next_id = 0u32;
        for (old_id, &is_reachable) in reachable.iter().enumerate() {
            if is_reachable {
                remap[old_id] = next_id;
                next_id += 1;
            }
        }

        // 3. Compact nodes: copy only reachable entries, then rewrite children.
        let mut new_nodes: Vec<Node> = Vec::with_capacity(next_id as usize);
        for (&is_reachable, node) in reachable.iter().zip(nodes.iter()) {
            if is_reachable {
                new_nodes.push(*node);
            }
        }
        for node in &mut new_nodes {
            if node.level > 0 {
                node.nw = remap[node.nw as usize];
                node.ne = remap[node.ne as usize];
                node.sw = remap[node.sw as usize];
                node.se = remap[node.se as usize];
            }
        }

        // 4. Rebuild canon from the compacted, remapped nodes.
        canon.clear();
        for (new_id, node) in new_nodes.iter().enumerate() {
            if node.level > 0 {
                canon.insert(node.nw, node.ne, node.sw, node.se, new_id as u32);
            }
        }

        // 5. Remap step_cache: keep entries where both source and result survived.
        let old_cache = std::mem::take(&mut *step_cache);
        *step_cache = old_cache
            .into_iter()
            .filter_map(|(old_k, old_v)| {
                let new_k = remap[old_k as usize];
                let new_v = remap[old_v as usize];
                if new_k == u32::MAX || new_v == u32::MAX {
                    None
                } else {
                    Some((new_k, new_v))
                }
            })
            .collect();

        // 6. Commit.
        self.root = remap[self.root as usize];
        new_nodes.shrink_to_fit();
        *nodes = new_nodes;
    }

    // ── Cell access helpers ───────────────────────────────────────────────────

    /// Recursively reads the cell at `(row, col)` within `node`.
    ///
    /// `node_size = 2^level` is passed explicitly to avoid repeated `pow` calls.
    fn get_cell_in(&self, node: NodeId, row: usize, col: usize, level: u8) -> bool {
        if level == 0 {
            return node == ALIVE;
        }
        let half = 1usize << (level - 1);
        let n = self.store.nodes.lock().unwrap()[node as usize];
        if row < half {
            if col < half {
                self.get_cell_in(n.nw, row, col, level - 1)
            } else {
                self.get_cell_in(n.ne, row, col - half, level - 1)
            }
        } else if col < half {
            self.get_cell_in(n.sw, row - half, col, level - 1)
        } else {
            self.get_cell_in(n.se, row - half, col - half, level - 1)
        }
    }

    /// Recursively sets the cell at `(row, col)` within `node`, returning the
    /// new canonical `NodeId` for the modified subtree.
    fn set_cell_in(
        &mut self,
        node: NodeId,
        row: usize,
        col: usize,
        alive: bool,
        level: u8,
    ) -> NodeId {
        if level == 0 {
            return if alive { ALIVE } else { DEAD };
        }
        let half = 1usize << (level - 1);
        let (nw, ne, sw, se) = {
            let n = self.store.nodes.lock().unwrap()[node as usize];
            (n.nw, n.ne, n.sw, n.se)
        };
        let (new_nw, new_ne, new_sw, new_se) = if row < half {
            if col < half {
                (self.set_cell_in(nw, row, col, alive, level - 1), ne, sw, se)
            } else {
                (
                    nw,
                    self.set_cell_in(ne, row, col - half, alive, level - 1),
                    sw,
                    se,
                )
            }
        } else if col < half {
            (
                nw,
                ne,
                self.set_cell_in(sw, row - half, col, alive, level - 1),
                se,
            )
        } else {
            (
                nw,
                ne,
                sw,
                self.set_cell_in(se, row - half, col - half, alive, level - 1),
            )
        };
        make_node_in_store(&self.store, new_nw, new_ne, new_sw, new_se)
    }

    // ── Tree traversal helpers ────────────────────────────────────────────────

    /// Recursively collects live cells within the intersection of `node`'s
    /// bounding box `[node_row, node_row+node_size) × [node_col, node_col+node_size)`
    /// and the query rectangle `[row_min, row_max) × [col_min, col_max)`.
    ///
    /// Branches with `pop == 0` or no intersection are pruned immediately.
    #[allow(clippy::too_many_arguments)]
    fn collect_live_in_rect(
        &self,
        node: NodeId,
        node_row: usize,
        node_col: usize,
        node_size: usize,
        row_min: usize,
        col_min: usize,
        row_max: usize,
        col_max: usize,
        out: &mut Vec<(usize, usize)>,
    ) {
        // Prune dead subtrees.
        if self.store.nodes.lock().unwrap()[node as usize].pop == 0 {
            return;
        }
        // Prune out-of-range subtrees.
        let node_row_max = node_row + node_size;
        let node_col_max = node_col + node_size;
        if node_row >= row_max
            || node_row_max <= row_min
            || node_col >= col_max
            || node_col_max <= col_min
        {
            return;
        }
        // Emit leaf.
        if node_size == 1 {
            out.push((node_row, node_col));
            return;
        }
        let half = node_size / 2;
        let n = self.store.nodes.lock().unwrap()[node as usize];
        self.collect_live_in_rect(
            n.nw, node_row, node_col, half, row_min, col_min, row_max, col_max, out,
        );
        self.collect_live_in_rect(
            n.ne,
            node_row,
            node_col + half,
            half,
            row_min,
            col_min,
            row_max,
            col_max,
            out,
        );
        self.collect_live_in_rect(
            n.sw,
            node_row + half,
            node_col,
            half,
            row_min,
            col_min,
            row_max,
            col_max,
            out,
        );
        self.collect_live_in_rect(
            n.se,
            node_row + half,
            node_col + half,
            half,
            row_min,
            col_min,
            row_max,
            col_max,
            out,
        );
    }
}

// ── Free functions operating on HashLifeStore ─────────────────────────────────

/// Returns `true` if any outer grandchild of `root` contains live cells,
/// indicating the pattern is too close to the boundary for a correct step.
///
/// For `step_recursive(root)` to produce a correct center result, the outer
/// three sub-quadrants of every root child must be all-dead.  The only safe
/// sub-quadrants are `nw.se`, `ne.sw`, `sw.ne`, and `se.nw` (the four inner
/// corners that together form the center half of the root).  All 12 outer
/// grandchildren must therefore be empty.
///
/// Takes the `nodes` slice directly so the caller can hold the lock for the
/// duration of all expansion checks (no redundant lock acquisitions).
fn needs_expansion_inner(nodes: &[Node], root: NodeId) -> bool {
    let r = nodes[root as usize];
    let nw = nodes[r.nw as usize];
    let ne = nodes[r.ne as usize];
    let sw = nodes[r.sw as usize];
    let se = nodes[r.se as usize];
    // All 12 outer grandchildren must be empty.
    // Only nw.se / ne.sw / sw.ne / se.nw are safe interior quadrants.
    nodes[nw.nw as usize].pop > 0
        || nodes[nw.ne as usize].pop > 0
        || nodes[nw.sw as usize].pop > 0
        || nodes[ne.nw as usize].pop > 0
        || nodes[ne.ne as usize].pop > 0
        || nodes[ne.se as usize].pop > 0
        || nodes[sw.nw as usize].pop > 0
        || nodes[sw.sw as usize].pop > 0
        || nodes[sw.se as usize].pop > 0
        || nodes[se.ne as usize].pop > 0
        || nodes[se.sw as usize].pop > 0
        || nodes[se.se as usize].pop > 0
}

/// Returns `true` if live cells are too close to the inner boundary of the
/// step result window to survive the next step without escaping.
///
/// `step_recursive(root)` returns the center half `[N/4, 3N/4)` advanced by
/// `2^(level−2)` generations.  Any live cell that drifts beyond `[N/4, 3N/4)`
/// during that interval is silently lost from the result.  After re-centring
/// the result is placed back at `[N/4, 3N/4)`, so `needs_expansion_inner()`
/// always sees an empty outer ring — the level would never grow on its own.
///
/// This deeper check inspects the **outer three great-grandchildren** of each
/// of the four safe grandchildren (`nw.se`, `ne.sw`, `sw.ne`, `se.nw`).
/// Those 12 slots cover the band `[N/4, 3N/8) ∪ (5N/8, 3N/4)` — the half of
/// the safe zone that is closest to the result boundary.  If anything lives
/// there, one more `expand_root()` call is needed before the step.
///
/// Requires `level ≥ 4` (great-grandchildren must be at least level 1); returns
/// `false` immediately for smaller levels.
///
/// Takes the `nodes` slice directly so the caller can hold the lock for the
/// duration of all expansion checks (no redundant lock acquisitions).
fn needs_expansion_deep_inner(nodes: &[Node], root: NodeId, level: u8) -> bool {
    if level < 4 {
        return false;
    }
    let r = nodes[root as usize];
    let nw = nodes[r.nw as usize];
    let ne = nodes[r.ne as usize];
    let sw = nodes[r.sw as usize];
    let se = nodes[r.se as usize];

    // Safe grandchildren (inner corners of the root).
    let nw_se = nodes[nw.se as usize]; // inner = nw_se.se
    let ne_sw = nodes[ne.sw as usize]; // inner = ne_sw.sw
    let sw_ne = nodes[sw.ne as usize]; // inner = sw_ne.ne
    let se_nw = nodes[se.nw as usize]; // inner = se_nw.nw

    // Outer 3 great-grandchildren of each safe grandchild must be empty.
    nodes[nw_se.nw as usize].pop > 0
        || nodes[nw_se.ne as usize].pop > 0
        || nodes[nw_se.sw as usize].pop > 0
        || nodes[ne_sw.nw as usize].pop > 0
        || nodes[ne_sw.ne as usize].pop > 0
        || nodes[ne_sw.se as usize].pop > 0
        || nodes[sw_ne.nw as usize].pop > 0
        || nodes[sw_ne.sw as usize].pop > 0
        || nodes[sw_ne.se as usize].pop > 0
        || nodes[se_nw.ne as usize].pop > 0
        || nodes[se_nw.sw as usize].pop > 0
        || nodes[se_nw.se as usize].pop > 0
}

/// Canonicalises a level-k node by interning it in the store's `canon`.
///
/// Lock ordering: `nodes` first (to push the new entry), then `canon` (to insert the key).
/// Both locks are released between the fast-path check and the slow-path insertion.
fn make_node_in_store(
    store: &HashLifeStore,
    nw: NodeId,
    ne: NodeId,
    sw: NodeId,
    se: NodeId,
) -> NodeId {
    // Fast path: check canon first.
    {
        let canon = store.canon.lock().unwrap();
        if let Some(id) = canon.get(nw, ne, sw, se) {
            return id;
        }
    }
    // Slow path: create new node. We must check again after acquiring both locks
    // to handle the case where another thread inserted the same node concurrently.
    let mut nodes = store.nodes.lock().unwrap();
    let mut canon = store.canon.lock().unwrap();
    // Double-check after acquiring both locks.
    if let Some(id) = canon.get(nw, ne, sw, se) {
        return id;
    }
    let level = nodes[nw as usize].level + 1;
    let pop = nodes[nw as usize].pop
        + nodes[ne as usize].pop
        + nodes[sw as usize].pop
        + nodes[se as usize].pop;
    let id = nodes.len() as NodeId;
    nodes.push(Node {
        level,
        nw,
        ne,
        sw,
        se,
        pop,
    });
    canon.insert(nw, ne, sw, se, id);
    id
}

/// Returns the canonical all-dead node at `level` (recursively constructed).
fn make_dead_node_in_store(store: &HashLifeStore, level: u8) -> NodeId {
    if level == 0 {
        return DEAD;
    }
    let child = make_dead_node_in_store(store, level - 1);
    make_node_in_store(store, child, child, child, child)
}

/// Returns the canonical level-(k-1) node whose NE quadrant is `a.NE` and
/// whose NW quadrant is `b.NW` (horizontal centre between two level-k nodes).
fn horiz_in_store(store: &HashLifeStore, a: NodeId, b: NodeId) -> NodeId {
    let (a_ne, a_se, b_nw, b_sw) = {
        let nodes = store.nodes.lock().unwrap();
        let na = nodes[a as usize];
        let nb = nodes[b as usize];
        (na.ne, na.se, nb.nw, nb.sw)
    };
    make_node_in_store(store, a_ne, b_nw, a_se, b_sw)
}

/// Returns the canonical level-(k-1) node from the bottom half of `a` and
/// the top half of `b` (vertical centre between two level-k nodes).
fn vert_in_store(store: &HashLifeStore, a: NodeId, b: NodeId) -> NodeId {
    let (a_sw, a_se, b_nw, b_ne) = {
        let nodes = store.nodes.lock().unwrap();
        let na = nodes[a as usize];
        let nb = nodes[b as usize];
        (na.sw, na.se, nb.nw, nb.ne)
    };
    make_node_in_store(store, a_sw, a_se, b_nw, b_ne)
}

/// Returns the canonical level-(k-1) node from the shared inner corners of
/// four level-k quadrant nodes (the "center4" sub-macrocell).
fn center4_in_store(
    store: &HashLifeStore,
    nw: NodeId,
    ne: NodeId,
    sw: NodeId,
    se: NodeId,
) -> NodeId {
    let (nw_se, ne_sw, sw_ne, se_nw) = {
        let nodes = store.nodes.lock().unwrap();
        (
            nodes[nw as usize].se,
            nodes[ne as usize].sw,
            nodes[sw as usize].ne,
            nodes[se as usize].nw,
        )
    };
    make_node_in_store(store, nw_se, ne_sw, sw_ne, se_nw)
}

/// 4×4 → 2×2 step for level-2 nodes via a precomputed lookup table.
///
/// The 16 cells of the 4×4 node are packed into a `u16` index (row-major,
/// bit `r*4+c`), looked up in [`STEP_LEVEL2_TABLE`], and the 4-bit result
/// maps to the four canonical level-1 child `NodeId`s.
fn step_level2_in_store(store: &HashLifeStore, node: NodeId) -> NodeId {
    let table = STEP_LEVEL2_TABLE.get_or_init(build_step_level2_table);

    let (n_nw, n_ne, n_sw, n_se) = {
        let nodes = store.nodes.lock().unwrap();
        let n = nodes[node as usize];
        (n.nw, n.ne, n.sw, n.se)
    };
    let (nw, ne, sw, se) = {
        let nodes = store.nodes.lock().unwrap();
        (
            nodes[n_nw as usize],
            nodes[n_ne as usize],
            nodes[n_sw as usize],
            nodes[n_se as usize],
        )
    };

    // Pack 16 cells into a u16 index (row-major, bit r*4+c):
    //   Row 0: nw.nw nw.ne ne.nw ne.ne  → bits  0– 3
    //   Row 1: nw.sw nw.se ne.sw ne.se  → bits  4– 7
    //   Row 2: sw.nw sw.ne se.nw se.ne  → bits  8–11
    //   Row 3: sw.sw sw.se se.sw se.se  → bits 12–15
    let idx = ((nw.nw == ALIVE) as u16)
        | ((nw.ne == ALIVE) as u16) << 1
        | ((ne.nw == ALIVE) as u16) << 2
        | ((ne.ne == ALIVE) as u16) << 3
        | ((nw.sw == ALIVE) as u16) << 4
        | ((nw.se == ALIVE) as u16) << 5
        | ((ne.sw == ALIVE) as u16) << 6
        | ((ne.se == ALIVE) as u16) << 7
        | ((sw.nw == ALIVE) as u16) << 8
        | ((sw.ne == ALIVE) as u16) << 9
        | ((se.nw == ALIVE) as u16) << 10
        | ((se.ne == ALIVE) as u16) << 11
        | ((sw.sw == ALIVE) as u16) << 12
        | ((sw.se == ALIVE) as u16) << 13
        | ((se.sw == ALIVE) as u16) << 14
        | ((se.se == ALIVE) as u16) << 15;

    // 4-bit result: bit0=r_nw, bit1=r_ne, bit2=r_sw, bit3=r_se
    let result = table[idx as usize];
    let r_nw = if result & 1 != 0 { ALIVE } else { DEAD };
    let r_ne = if result & 2 != 0 { ALIVE } else { DEAD };
    let r_sw = if result & 4 != 0 { ALIVE } else { DEAD };
    let r_se = if result & 8 != 0 { ALIVE } else { DEAD };

    make_node_in_store(store, r_nw, r_ne, r_sw, r_se)
}

/// Memoised recursive step.
///
/// Advances `node` (level `k`) by `2^j` generations and returns the
/// canonical level-(k-1) result.
///
/// * `j == level - 2` → **full step**: two waves of recursion, each
///   advancing `2^(j-1)` gens, for a total of `2^j` gens.
/// * `j < level - 2` → **partial step**: one wave only, advancing `2^j`
///   gens into a level-(k-1) intermediate that is returned directly
///   (no second wave).
///
/// For level 2 (`j` must be 0), delegates to `step_level2_in_store`.
///
/// When `level > PARALLEL_THRESHOLD`, the wave-2 computations are parallelised
/// using `rayon::join`, allowing subtrees to be computed concurrently.
///
/// The cache key is `node` alone — valid because `j` is constant throughout a
/// single top-level `step_universe` call and the cache is cleared by
/// `set_step_log2` whenever `j` changes.
fn step_recursive(store: &Arc<HashLifeStore>, node: NodeId, j: u8) -> NodeId {
    // Check cache first (without holding the lock during recursion).
    {
        let cache = store.step_cache.lock().unwrap();
        if let Some(&cached) = cache.get(&node) {
            return cached;
        }
    }

    let level = store.nodes.lock().unwrap()[node as usize].level;
    let result = if level == 2 {
        debug_assert_eq!(j, 0, "j must be 0 at level-2 base case");
        step_level2_in_store(store, node)
    } else {
        let (nw, ne, sw, se) = {
            let nodes = store.nodes.lock().unwrap();
            let n = nodes[node as usize];
            (n.nw, n.ne, n.sw, n.se)
        };

        // ── 9 sub-macrocells (level k-1) ─────────────────────────────────────
        let t0 = nw;
        let t1 = horiz_in_store(store, nw, ne);
        let t2 = ne;
        let t3 = vert_in_store(store, nw, sw);
        let t4 = center4_in_store(store, nw, ne, sw, se);
        let t5 = vert_in_store(store, ne, se);
        let t6 = sw;
        let t7 = horiz_in_store(store, sw, se);
        let t8 = se;

        if j == level - 2 {
            // ── Full step: two recursive waves ────────────────────────────────
            // Wave 1: each sub-macrocell (level k-1) stepped by j-1 gens.
            let j1 = j - 1;
            let s0 = step_recursive(store, t0, j1);
            let s1 = step_recursive(store, t1, j1);
            let s2 = step_recursive(store, t2, j1);
            let s3 = step_recursive(store, t3, j1);
            let s4 = step_recursive(store, t4, j1);
            let s5 = step_recursive(store, t5, j1);
            let s6 = step_recursive(store, t6, j1);
            let s7 = step_recursive(store, t7, j1);
            let s8 = step_recursive(store, t8, j1);

            // ── Combine into 4 level-(k-1) nodes ─────────────────────────────
            let u0 = make_node_in_store(store, s0, s1, s3, s4);
            let u1 = make_node_in_store(store, s1, s2, s4, s5);
            let u2 = make_node_in_store(store, s3, s4, s6, s7);
            let u3 = make_node_in_store(store, s4, s5, s7, s8);

            // Wave 2: step the 4 combined nodes by another j-1 gens.
            // Parallelise when the level is large enough to amortise Rayon overhead.
            if level > PARALLEL_THRESHOLD {
                let store1 = Arc::clone(store);
                let store2 = Arc::clone(store);
                let store3 = Arc::clone(store);
                let store4 = Arc::clone(store);
                let ((r0, r1), (r2, r3)) = rayon::join(
                    || {
                        rayon::join(
                            || step_recursive(&store1, u0, j1),
                            || step_recursive(&store2, u1, j1),
                        )
                    },
                    || {
                        rayon::join(
                            || step_recursive(&store3, u2, j1),
                            || step_recursive(&store4, u3, j1),
                        )
                    },
                );
                make_node_in_store(store, r0, r1, r2, r3)
            } else {
                let r0 = step_recursive(store, u0, j1);
                let r1 = step_recursive(store, u1, j1);
                let r2 = step_recursive(store, u2, j1);
                let r3 = step_recursive(store, u3, j1);
                make_node_in_store(store, r0, r1, r2, r3)
            }
        } else {
            // ── Partial step: one wave + center4 assembly ─────────────────────
            // Each sub-macrocell (level k-1) stepped by j gens, returning
            // level-(k-2) results.  (When j == (k-1)-2, this is a full
            // step on the sub-macrocell; otherwise it recurses as partial.)
            let s0 = step_recursive(store, t0, j);
            let s1 = step_recursive(store, t1, j);
            let s2 = step_recursive(store, t2, j);
            let s3 = step_recursive(store, t3, j);
            let s4 = step_recursive(store, t4, j);
            let s5 = step_recursive(store, t5, j);
            let s6 = step_recursive(store, t6, j);
            let s7 = step_recursive(store, t7, j);
            let s8 = step_recursive(store, t8, j);

            // Extract the 4 quadrants of the level-(k-1) result by applying
            // center4 to each overlapping group of 4 stepped sub-macrocells.
            // center4(a, b, c, d) = make_node(a.se, b.sw, c.ne, d.nw), which
            // picks one inner corner from each, so there is no double-counting
            // of the overlapping s4.  Total time advance = 2^j (wave 1 only).
            let r0 = center4_in_store(store, s0, s1, s3, s4);
            let r1 = center4_in_store(store, s1, s2, s4, s5);
            let r2 = center4_in_store(store, s3, s4, s6, s7);
            let r3 = center4_in_store(store, s4, s5, s7, s8);

            make_node_in_store(store, r0, r1, r2, r3)
        }
    };

    // Insert into cache (double-checked: another thread may have computed this
    // concurrently; NodeId results are deterministic so either value is correct).
    store.step_cache.lock().unwrap().insert(node, result);
    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    // `super::*` resolves differently when included via #[path] in the bench binary
    use super::*;

    // ── Internals ─────────────────────────────────────────────────────────────

    /// DEAD and ALIVE are always at fixed indices 0 and 1 with the correct pop.
    #[test]
    fn test_leaf_nodes() {
        let hl = HashLife::new();
        assert_eq!(hl.store.nodes.lock().unwrap()[DEAD as usize].pop, 0);
        assert_eq!(hl.store.nodes.lock().unwrap()[ALIVE as usize].pop, 1);
        assert_eq!(hl.store.nodes.lock().unwrap()[DEAD as usize].level, 0);
        assert_eq!(hl.store.nodes.lock().unwrap()[ALIVE as usize].level, 0);
    }

    /// make_node must canonicalise: calling with the same children returns the
    /// same NodeId.
    #[test]
    fn test_make_node_canonical() {
        let mut hl = HashLife::new();
        let n1 = hl.make_node(DEAD, DEAD, DEAD, ALIVE);
        let n2 = hl.make_node(DEAD, DEAD, DEAD, ALIVE);
        assert_eq!(n1, n2);
    }

    /// make_dead_node at level 1 must produce a node with pop=0.
    #[test]
    fn test_make_dead_node() {
        let mut hl = HashLife::new();
        let dn = hl.make_dead_node(1);
        assert_eq!(hl.store.nodes.lock().unwrap()[dn as usize].pop, 0);
        assert_eq!(hl.store.nodes.lock().unwrap()[dn as usize].level, 1);
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// After construction, width and height both equal 2^DEFAULT_LEVEL.
    #[test]
    fn test_dimensions() {
        let hl = HashLife::new();
        assert_eq!(hl.width(), 1 << DEFAULT_LEVEL);
        assert_eq!(hl.height(), 1 << DEFAULT_LEVEL);
    }

    /// get/set round-trip: set a cell alive, then read it back.
    #[test]
    fn test_get_set_roundtrip() {
        let mut hl = HashLife::new();
        let (r, c) = (10, 20);
        assert!(!hl.get(r, c));
        hl.set(r, c, true);
        assert!(hl.get(r, c));
        hl.set(r, c, false);
        assert!(!hl.get(r, c));
    }

    /// toggle: dead→alive→dead.
    #[test]
    fn test_toggle() {
        let mut hl = HashLife::new();
        hl.toggle(5, 5);
        assert!(hl.get(5, 5));
        hl.toggle(5, 5);
        assert!(!hl.get(5, 5));
    }

    /// population counts set bits correctly.
    #[test]
    fn test_population() {
        let mut hl = HashLife::new();
        assert_eq!(hl.population(), 0);
        hl.set(0, 0, true);
        hl.set(1, 1, true);
        assert_eq!(hl.population(), 2);
    }

    /// clear resets the population to zero.
    #[test]
    fn test_clear() {
        let mut hl = HashLife::new();
        hl.set(100, 100, true);
        assert_eq!(hl.population(), 1);
        hl.clear();
        assert_eq!(hl.population(), 0);
    }

    /// step_level2: a 2×2 block centred in a 4×4 grid should become
    /// a 2×2 block again after one step (still life).
    #[test]
    fn test_step_level2_still_life_block() {
        // Build a level-2 node with a 2×2 block at (1,1)..(2,2).
        // In level-2 layout:
        //   NW.SE = alive, NE.SW = alive
        //   SW.NE = alive, SE.NW = alive
        let mut hl = HashLife::new();
        let nw = hl.make_node(DEAD, DEAD, DEAD, ALIVE); // se=alive
        let ne = hl.make_node(DEAD, DEAD, ALIVE, DEAD); // sw=alive
        let sw = hl.make_node(DEAD, ALIVE, DEAD, DEAD); // ne=alive
        let se = hl.make_node(ALIVE, DEAD, DEAD, DEAD); // nw=alive
        let block = hl.make_node(nw, ne, sw, se);

        let result = step_level2_in_store(&hl.store, block);
        // Result should be a level-1 node with all four cells alive.
        assert_eq!(hl.store.nodes.lock().unwrap()[result as usize].pop, 4);
    }

    /// A blinker oscillates with period 2: horizontal then vertical.
    #[test]
    fn test_blinker_oscillates() {
        let mut hl = HashLife::new();
        // Place a horizontal blinker at the center.
        let half = hl.width() / 2;
        hl.set(half, half - 1, true);
        hl.set(half, half, true);
        hl.set(half, half + 1, true);
        assert_eq!(hl.population(), 3);

        let init_offsets: std::collections::HashSet<_> =
            hl.live_cells_offsets().into_iter().collect();

        // Advance two steps (one period) — must restore original state.
        // We may need multiple step_universe calls since each advances 2^(level-2) gens.
        let mut total_gens = 0u64;
        while total_gens < 2 {
            let (gens, _) = hl.step_universe();
            total_gens += gens;
        }

        // After an even number of generations the population should still be 3.
        assert_eq!(hl.population(), 3);
        let after_offsets: std::collections::HashSet<_> =
            hl.live_cells_offsets().into_iter().collect();
        assert_eq!(
            init_offsets, after_offsets,
            "blinker should return to original state after even number of gens"
        );
    }

    /// A 2×2 block is a still life: population stays constant after stepping.
    #[test]
    fn test_block_still_life() {
        let mut hl = HashLife::new();
        let half = hl.width() / 2;
        hl.set(half, half, true);
        hl.set(half, half + 1, true);
        hl.set(half + 1, half, true);
        hl.set(half + 1, half + 1, true);
        assert_eq!(hl.population(), 4);

        hl.step_universe();
        assert_eq!(hl.population(), 4, "block must be a still life");
    }

    /// live_cells_in_viewport returns only cells inside the query rectangle.
    #[test]
    fn test_live_cells_in_viewport() {
        let mut hl = HashLife::new();
        hl.set(10, 10, true);
        hl.set(20, 20, true);
        let vp = hl.live_cells_in_viewport(0, 0, 15, 15);
        assert_eq!(
            vp.len(),
            1,
            "only (10,10) should be in viewport [0,15)x[0,15)"
        );
        assert_eq!(vp[0], (10, 10));
    }

    /// set_cells centers the pattern and auto-sizes the grid.
    #[test]
    fn test_set_cells_centered() {
        let mut hl = HashLife::new();
        hl.set_cells(&[(0, 0), (0, 1), (0, -1)]);
        assert_eq!(hl.population(), 3);
    }

    /// set_cells for a large pattern must expand the grid.
    #[test]
    fn test_set_cells_auto_resize() {
        let mut hl = HashLife::new();
        // Pattern far outside default 256×256 grid.
        hl.set_cells(&[(-500, -500), (500, 500)]);
        assert_eq!(hl.population(), 2);
        assert!(
            hl.width() > 512 * 2,
            "grid should expand to fit ±500 + margin"
        );
    }

    /// live_cells_offsets → set_cells round-trip preserves pattern.
    #[test]
    fn test_offsets_roundtrip() {
        let mut hl = HashLife::new();
        hl.set_cells(&[(0, 0), (0, 1), (1, 0)]);
        let offsets = hl.live_cells_offsets();
        assert_eq!(offsets.len(), 3);

        let mut hl2 = HashLife::new();
        hl2.set_cells(&offsets);
        assert_eq!(hl2.population(), 3);
    }

    /// A live cell in nw.ne (top-centre-left, NOT the corner nw.nw) must trigger
    /// needs_expansion.  The old four-corner check missed this region, causing
    /// step_recursive to run with a dirty boundary and produce wrong results for
    /// patterns like the 6-engine cordership gun.
    #[test]
    fn test_needs_expansion_catches_edge_not_corner() {
        let mut hl = HashLife::new();
        // nw child covers rows [0..half), cols [0..half).
        // nw.ne covers rows [0..quarter), cols [quarter..half) — NOT the corner.
        let quarter = hl.width() / 4;
        hl.set(0, quarter + 1, true); // row=0, col=quarter+1 → in nw.ne
        let nodes = hl.store.nodes.lock().unwrap();
        assert!(
            needs_expansion_inner(&nodes, hl.root),
            "cell in nw.ne must trigger needs_expansion (edge-but-not-corner bug)"
        );
    }

    /// HashLife and SWAR must agree on population after the same number of
    /// generations for a blinker.  Any boundary-expansion bug that corrupts
    /// the step result would cause a population mismatch.
    #[test]
    fn test_hashlife_swar_agree_60_gens() {
        use crate::grid::Grid;

        // Vertical blinker centred in a 200×200 SWAR grid.
        let mut swar = Grid::new(200, 200);
        swar.set(99, 100, true);
        swar.set(100, 100, true);
        swar.set(101, 100, true);
        for _ in 0..60 {
            swar.step();
            swar.expand_if_needed();
        }

        // Same blinker in HashLife (offsets relative to centre).
        let mut hl = HashLife::new();
        hl.set_cells(&[(-1, 0), (0, 0), (1, 0)]);
        // Advance until generation ≥ 60.  step_universe may overshoot, but any
        // even multiple of the blinker period (2) restores population to 3.
        let mut total = 0u64;
        while total < 60 {
            let (g, _) = hl.step_universe();
            total += g;
        }

        assert_eq!(
            swar.live_count(),
            hl.population(),
            "SWAR and HashLife must agree after ≥60 blinker generations (total HL gens: {total})"
        );
    }

    /// A cell in nw.se.nw (outer great-grandchild of the safe zone, NOT the safe
    /// great-grandchild nw.se.se) must trigger `needs_expansion_deep_inner`.
    /// Without this deeper check the cordership gun diverges from SWAR around
    /// gen 10000+ because corderships drift out of the step-result window and
    /// are silently dropped.
    #[test]
    fn test_needs_expansion_deep_catches_near_boundary() {
        let mut hl = HashLife::new();
        // nw.se covers [N/4, N/2) × [N/4, N/2).
        // nw.se.nw (outer GGC) covers [N/4, 3N/8) × [N/4, 3N/8) — near boundary.
        // nw.se.se (safe  GGC) covers [3N/8, N/2) × [3N/8, N/2) — inner quarter.
        let n = hl.width();
        let quarter = n / 4;
        let eighth = n / 8;
        // Place a cell in nw.se.nw (near-boundary), which is inside the safe
        // grandchild but outside the safe great-grandchild.
        hl.set(quarter + 1, quarter + 1, true); // in [N/4, 3N/8) × [N/4, 3N/8)
        {
            let nodes = hl.store.nodes.lock().unwrap();
            assert!(
                !needs_expansion_inner(&nodes, hl.root),
                "cell in nw.se (inner half) must NOT trigger needs_expansion"
            );
            assert!(
                needs_expansion_deep_inner(&nodes, hl.root, hl.level),
                "cell in nw.se.nw (outer GGC of safe zone) must trigger needs_expansion_deep"
            );
        }

        // A cell in nw.se.se (the safe great-grandchild, inner quarter) must NOT
        // trigger the deep check.
        let mut hl2 = HashLife::new();
        hl2.set(quarter + eighth + 1, quarter + eighth + 1, true); // in [3N/8, N/2)
        {
            let nodes = hl2.store.nodes.lock().unwrap();
            assert!(
                !needs_expansion_deep_inner(&nodes, hl2.root, hl2.level),
                "cell in nw.se.se (inner great-grandchild) must NOT trigger needs_expansion_deep"
            );
        }
        let _ = eighth;
    }

    // ── Garbage collection tests ──────────────────────────────────────────────

    /// After GC, population and subsequent evolution are unchanged.
    #[test]
    fn test_gc_preserves_state() {
        let mut hl = HashLife::new();
        // Standard SE glider (period 4, speed c/4 diagonal).
        hl.set_cells(&[(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)]);
        assert_eq!(hl.population(), 5);

        // Advance at least 256 generations to build up arena nodes.
        // Cap by total gens (not call count) to avoid u64 overflow when the
        // level grows and step_size becomes very large.
        let mut total = 0u64;
        while total < 256 {
            let (g, _) = hl.step_universe();
            total += g;
        }
        assert_eq!(
            hl.population(),
            5,
            "glider population must stay 5 before GC"
        );

        // GC must not change observable state.
        hl.gc();
        assert_eq!(hl.population(), 5, "GC must preserve population");

        // Engine must still produce correct results after GC.
        while total < 512 {
            let (g, _) = hl.step_universe();
            total += g;
        }
        assert_eq!(
            hl.population(),
            5,
            "glider population must stay 5 after post-GC steps"
        );
    }

    /// GC must reduce the arena after many steps have accumulated stale nodes.
    #[test]
    fn test_gc_reclaims_nodes() {
        let mut hl = HashLife::new();
        // Gosper glider gun (period 30, emits one glider every 30 gens).
        let gosper: &[(i32, i32)] = &[
            (0, 24),
            (1, 22),
            (1, 24),
            (2, 12),
            (2, 13),
            (2, 20),
            (2, 21),
            (2, 34),
            (2, 35),
            (3, 11),
            (3, 15),
            (3, 20),
            (3, 21),
            (3, 34),
            (3, 35),
            (4, 0),
            (4, 1),
            (4, 10),
            (4, 16),
            (4, 20),
            (4, 21),
            (5, 0),
            (5, 1),
            (5, 10),
            (5, 14),
            (5, 16),
            (5, 17),
            (5, 22),
            (5, 24),
            (6, 10),
            (6, 16),
            (6, 24),
            (7, 11),
            (7, 15),
            (8, 12),
            (8, 13),
        ];
        hl.set_cells(gosper);

        // Run until at least 2000 generations to fill the arena with stale
        // intermediate nodes.  Cap by total gens to avoid overflow.
        let mut total = 0u64;
        while total < 2000 {
            let (g, _) = hl.step_universe();
            total += g;
        }
        let before = hl.store.nodes.lock().unwrap().len();

        // GC must reclaim unreachable nodes.
        hl.gc();
        assert!(
            hl.store.nodes.lock().unwrap().len() < before,
            "GC must reclaim nodes: before={before}, after={}",
            hl.store.nodes.lock().unwrap().len()
        );
    }

    // ── Variable step-size (step_log2) tests ─────────────────────────────────
    //
    // These tests reference `HashLife::set_step_log2` and the updated
    // `step_size()` / `step_universe()` behaviour that will be added by the
    // TODO 2.5 implementation.  They are intentionally written *before* the
    // implementation exists and will fail to compile until it is complete.

    /// With step_log2=0, a single step_universe call must advance exactly
    /// 1 generation and keep blinker population at 3.
    #[test]
    fn test_step_log2_zero_advances_1_gen() {
        let mut hl = HashLife::new();
        let half = hl.width() / 2;
        hl.set(half, half - 1, true);
        hl.set(half, half, true);
        hl.set(half, half + 1, true);
        assert_eq!(hl.population(), 3);

        hl.set_step_log2(0);
        let (gens, _) = hl.step_universe();

        assert_eq!(gens, 1, "step_log2=0 must advance exactly 1 generation");
        assert_eq!(
            hl.population(),
            3,
            "blinker population must remain 3 after 1 gen"
        );
    }

    /// With step_log2 = level - 2, step_universe must advance exactly
    /// 2^(level-2) generations (the maximum for that level).
    #[test]
    fn test_step_log2_full_matches_level_minus_2() {
        let mut hl = HashLife::new();
        let half = hl.width() / 2;
        hl.set(half, half - 1, true);
        hl.set(half, half, true);
        hl.set(half, half + 1, true);

        // Prime expansion: one step_universe call ensures the level has settled
        // to at least MIN_STEP_LEVEL after internal expansion.
        hl.set_step_log2(0);
        hl.step_universe();

        // Now set step_log2 to the full level-2 value for the current level.
        let level_before = hl.level;
        let j = level_before - 2;
        hl.set_step_log2(j);

        let (gens, _) = hl.step_universe();
        let expected = 1u64 << (j as u32);
        assert_eq!(
            gens, expected,
            "step_log2={j} at level {level_before} must advance {expected} gens, got {gens}"
        );
    }

    /// step_log2 field must equal j after set_step_log2(j), for j in [0, 1, 5].
    #[test]
    fn test_step_size_returns_2_pow_step_log2() {
        let mut hl = HashLife::new();
        for j in [0u8, 1, 5] {
            hl.set_step_log2(j);
            assert_eq!(
                hl.step_log2, j,
                "step_log2 must equal {j} after set_step_log2({j})"
            );
        }
    }

    /// With step_log2=0, HashLife and SWAR must agree on population at every
    /// individual step for 20 generations of a blinker.
    #[test]
    fn test_step_log2_zero_agrees_with_swar() {
        use crate::grid::Grid;

        // Blinker in SWAR (200×200 grid).
        let mut swar = Grid::new(200, 200);
        swar.set(100, 99, true);
        swar.set(100, 100, true);
        swar.set(100, 101, true);

        // Same blinker in HashLife with step_log2=0.
        let mut hl = HashLife::new();
        hl.set_cells(&[(0, -1), (0, 0), (0, 1)]);
        hl.set_step_log2(0);

        for step in 0..20 {
            swar.step();
            swar.expand_if_needed();
            let (gens, _) = hl.step_universe();
            assert_eq!(
                gens, 1,
                "step {step}: step_log2=0 must advance exactly 1 gen"
            );
            assert_eq!(
                swar.live_count(),
                hl.population(),
                "step {step}: SWAR and HashLife populations must match"
            );
        }
    }

    /// Stepping with step_log2=3 once (8 gens) must give the same final
    /// population as stepping 8 times with step_log2=0.
    /// The blinker has period 2, so 8 steps (even) always leaves pop=3.
    #[test]
    fn test_step_log2_large_matches_repeated_small() {
        let blinker: &[(i32, i32)] = &[(0, -1), (0, 0), (0, 1)];

        // hl_big: one step at step_log2=3 (8 gens).
        let mut hl_big = HashLife::new();
        hl_big.set_cells(blinker);
        hl_big.set_step_log2(3);
        let (big_gens, _) = hl_big.step_universe();
        assert_eq!(big_gens, 8, "step_log2=3 must advance 8 gens");

        // hl_small: eight steps at step_log2=0 (1 gen each).
        let mut hl_small = HashLife::new();
        hl_small.set_cells(blinker);
        hl_small.set_step_log2(0);
        let mut small_total = 0u64;
        for _ in 0..8 {
            let (g, _) = hl_small.step_universe();
            small_total += g;
        }
        assert_eq!(small_total, 8, "8 × step_log2=0 must total 8 gens");

        assert_eq!(
            hl_big.population(),
            hl_small.population(),
            "population after 8 gens must match regardless of step granularity"
        );
    }

    /// Setting step_log2=7 and calling step_universe must not panic and must
    /// leave hl.level >= j + 2 = 9, since expansion is required before stepping.
    #[test]
    fn test_expansion_enforces_level_ge_j_plus_2() {
        let mut hl = HashLife::new();
        let half = hl.width() / 2;
        hl.set(half, half - 1, true);
        hl.set(half, half, true);
        hl.set(half, half + 1, true);

        hl.set_step_log2(7);
        // Must not panic even though j=7 requires level >= 9.
        hl.step_universe();

        assert!(
            hl.level >= 9,
            "after step_log2=7 the level must be >= 9 (j+2), got {}",
            hl.level
        );
    }

    /// A single glider (c/4 diagonal spaceship) run long enough to trigger
    /// `needs_expansion_deep` must agree with SWAR.  Without the deeper check
    /// the glider drifts into the near-boundary zone, escapes the step-result
    /// window on the following step, and is silently dropped — causing HL
    /// population to fall to 0 while SWAR still shows 5.
    #[test]
    fn test_hashlife_swar_agree_glider_long() {
        use crate::grid::Grid;

        // Standard SE glider (period 4, speed c/4 diagonal).
        let cells: &[(i32, i32)] = &[(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)];

        let mut hl = HashLife::new();
        hl.set_cells(cells);

        // Run until ≥ 500 gens.  HL step size may grow due to expansion, so
        // `total` might overshoot; we then run SWAR to the same `total`.
        let mut total = 0u64;
        while total < 500 {
            let (g, _) = hl.step_universe();
            total += g;
        }

        let mut swar = Grid::new(2000, 2000);
        let origin = 1000i32;
        for &(dr, dc) in cells {
            swar.set((origin + dr) as usize, (origin + dc) as usize, true);
        }
        for _ in 0..total {
            swar.step();
            swar.expand_if_needed();
        }

        assert_eq!(
            swar.live_count(),
            hl.population(),
            "SWAR and HashLife must agree at gen {total} for a glider"
        );
    }

    // ── Parallel subtree traversal (TODO 2.6) tests ───────────────────────────
    //
    // These tests reference `PARALLEL_THRESHOLD`, which will be added by the
    // TODO 2.6 implementation (Rayon parallel join on wave-2 step_recursive
    // calls when node level > PARALLEL_THRESHOLD).  They are intentionally
    // written before the implementation exists and will fail to compile until
    // `PARALLEL_THRESHOLD` is exported from this module.

    /// PARALLEL_THRESHOLD must be an accessible constant in this module.
    /// This test verifies the constant exists and has a sensible value:
    /// it must be at least 4 (to avoid parallelising tiny subtrees) and
    /// at most 16 (practically: large enough to amortise Rayon overhead).
    #[test]
    fn test_parallel_threshold_is_exported_and_sane() {
        // PARALLEL_THRESHOLD does not exist yet — this line will fail to
        // compile until TODO 2.6 adds the constant.
        assert!(
            PARALLEL_THRESHOLD >= 4,
            "PARALLEL_THRESHOLD must be >= 4 to avoid trivial parallelism overhead, got {PARALLEL_THRESHOLD}"
        );
        assert!(
            PARALLEL_THRESHOLD <= 16,
            "PARALLEL_THRESHOLD must be <= 16 (sanity cap), got {PARALLEL_THRESHOLD}"
        );
    }

    /// The Gosper glider gun runs entirely below PARALLEL_THRESHOLD (it fits
    /// in a ~36×36 bounding box, well below the 128×128 level-7 threshold).
    /// After 300 generations the population must grow monotonically (new
    /// gliders are emitted every 30 gens) and agree between two independently
    /// constructed instances — confirming the sequential path is unaffected.
    #[test]
    fn test_parallel_small_pattern_gosper_gun_correctness() {
        let gosper: &[(i32, i32)] = &[
            (0, 24),
            (1, 22),
            (1, 24),
            (2, 12),
            (2, 13),
            (2, 20),
            (2, 21),
            (2, 34),
            (2, 35),
            (3, 11),
            (3, 15),
            (3, 20),
            (3, 21),
            (3, 34),
            (3, 35),
            (4, 0),
            (4, 1),
            (4, 10),
            (4, 16),
            (4, 20),
            (4, 21),
            (5, 0),
            (5, 1),
            (5, 10),
            (5, 14),
            (5, 16),
            (5, 17),
            (5, 22),
            (5, 24),
            (6, 10),
            (6, 16),
            (6, 24),
            (7, 11),
            (7, 15),
            (8, 12),
            (8, 13),
        ];

        // Two independent HashLife instances must agree exactly.
        let mut hl1 = HashLife::new();
        hl1.set_cells(gosper);
        let mut hl2 = HashLife::new();
        hl2.set_cells(gosper);

        let mut total1 = 0u64;
        let mut total2 = 0u64;

        // Advance both past 300 generations.
        while total1 < 300 {
            let (g, _) = hl1.step_universe();
            total1 += g;
        }
        while total2 < 300 {
            let (g, _) = hl2.step_universe();
            total2 += g;
        }

        // Both should have taken the same path and reached the same generation.
        assert_eq!(
            total1, total2,
            "two independent gosper gun runs must reach the same total generation"
        );
        assert_eq!(
            hl1.population(),
            hl2.population(),
            "two independent gosper gun runs must have identical population at gen {total1}"
        );
        // Gosper gun emits one glider every 30 gens: after 300 gens there are
        // 10 gliders + the gun body (36 cells) ≥ 86 live cells.
        assert!(
            hl1.population() > 36,
            "gosper gun must have emitted gliders after 300 gens, pop={}",
            hl1.population()
        );
    }

    /// A large random soup (seeded, 50 % density) placed in a grid large enough
    /// to exceed PARALLEL_THRESHOLD in level must produce the same population
    /// when run twice from identical seeds.  This exercises the parallel path
    /// (if level > PARALLEL_THRESHOLD) without requiring SWAR for the ground
    /// truth — determinism is the invariant.
    #[test]
    fn test_parallel_large_pattern_determinism() {
        // fill_random at 50 % density will set many cells across the full
        // DEFAULT_LEVEL (256×256) grid, giving a root at level 8 > any
        // reasonable PARALLEL_THRESHOLD.  Two instances seeded identically
        // must reach the same population after the same number of steps.
        let seed = 0xDEAD_BEEF_u64;

        let mut hl1 = HashLife::new();
        hl1.fill_random(50, seed);
        let mut hl2 = HashLife::new();
        hl2.fill_random(50, seed);

        // Advance both to at least 16 generations.
        let mut total1 = 0u64;
        let mut total2 = 0u64;
        while total1 < 16 {
            let (g, _) = hl1.step_universe();
            total1 += g;
        }
        while total2 < 16 {
            let (g, _) = hl2.step_universe();
            total2 += g;
        }

        assert_eq!(
            total1, total2,
            "determinism: both runs must reach the same total generation"
        );
        assert_eq!(
            hl1.population(),
            hl2.population(),
            "determinism: both runs must have identical population at gen {total1} \
             (parallel path must be deterministic)"
        );
    }

    /// A large pattern (level ≥ PARALLEL_THRESHOLD + 1) must produce the same
    /// result whether run on one instance or compared to a fresh clone of the
    /// initial state.  Uses a known-good large still-life seed so that the
    /// population is stable and any corruption is immediately visible.
    ///
    /// Specifically: a 256×256 grid filled at 37 % density is chaotic for the
    /// first ~500 gens, then stabilises.  We run two fresh instances to 64 gens
    /// and compare; if the parallel code introduces non-determinism or a wrong
    /// result, the populations will differ.
    #[test]
    fn test_parallel_large_pattern_matches_independent_run() {
        let seed = 0x1234_5678_u64;

        // Instance A
        let mut hl_a = HashLife::new();
        hl_a.fill_random(37, seed);

        // Instance B — identical starting state
        let mut hl_b = HashLife::new();
        hl_b.fill_random(37, seed);

        // Both instances advance step_log2=0 (1 gen per call) for 64 steps,
        // so total generations are exactly equal and comparable.
        hl_a.set_step_log2(0);
        hl_b.set_step_log2(0);

        for step in 0..64 {
            let (ga, _) = hl_a.step_universe();
            let (gb, _) = hl_b.step_universe();
            assert_eq!(
                ga, gb,
                "step {step}: both instances must advance the same number of gens"
            );
            assert_eq!(
                hl_a.population(),
                hl_b.population(),
                "step {step}: parallel and sequential runs must agree on population \
                 (level={}, PARALLEL_THRESHOLD={PARALLEL_THRESHOLD})",
                hl_a.level
            );
        }
    }

    /// After running a large soup past the PARALLEL_THRESHOLD level, the live
    /// cell positions (not just population) must be identical between two
    /// independent instances.  Checks that parallelism does not reorder writes
    /// or introduce race conditions.
    #[test]
    fn test_parallel_large_pattern_cell_positions_match() {
        let seed = 0xCAFE_BABE_u64;

        let mut hl1 = HashLife::new();
        hl1.fill_random(30, seed);
        let mut hl2 = HashLife::new();
        hl2.fill_random(30, seed);

        hl1.set_step_log2(0);
        hl2.set_step_log2(0);

        // Advance exactly 8 generations with 1-gen steps.
        for _ in 0..8 {
            hl1.step_universe();
            hl2.step_universe();
        }

        // live_cells_offsets returns offsets relative to the centre; collect
        // into sorted vecs for comparison.
        let mut pos1 = hl1.live_cells_offsets();
        let mut pos2 = hl2.live_cells_offsets();
        pos1.sort_unstable();
        pos2.sort_unstable();

        assert_eq!(
            pos1, pos2,
            "cell positions must be identical between two independent runs \
             at level {} (PARALLEL_THRESHOLD={PARALLEL_THRESHOLD})",
            hl1.level
        );
    }

    /// When the universe level exceeds PARALLEL_THRESHOLD, HashLife and SWAR
    /// must agree on the live-cell count after the same number of generations.
    /// Uses a small, well-known pattern (blinker) scaled via many step_log2=0
    /// steps so the HL tree grows large enough to trigger the parallel path.
    ///
    /// Note: The blinker has period 2, so after any even number of gens the
    /// population is exactly 3.  After an odd number it is also 3 (blinker
    /// keeps pop=3 at all times).
    #[test]
    fn test_parallel_agrees_with_swar_above_threshold() {
        use crate::grid::Grid;

        // Standard horizontal blinker.
        let blinker: &[(i32, i32)] = &[(0, -1), (0, 0), (0, 1)];

        let mut hl = HashLife::new();
        hl.set_cells(blinker);
        hl.set_step_log2(0);

        // Step until the root level exceeds PARALLEL_THRESHOLD.  Each
        // step_universe call with step_log2=0 advances exactly 1 gen; the
        // level grows as the universe expands.
        let mut total_gens = 0u64;
        while hl.level <= PARALLEL_THRESHOLD {
            let (g, _) = hl.step_universe();
            total_gens += g;
        }
        // Now level > PARALLEL_THRESHOLD.  Step a few more gens.
        for _ in 0..4 {
            let (g, _) = hl.step_universe();
            total_gens += g;
        }

        // Run SWAR for exactly the same number of generations.
        let mut swar = Grid::new(4096, 4096);
        let origin = 2048usize;
        for &(_dr, dc) in blinker {
            swar.set(origin, (origin as i64 + dc as i64) as usize, true);
        }
        for _ in 0..total_gens {
            swar.step();
            swar.expand_if_needed();
        }

        assert_eq!(
            swar.live_count(),
            hl.population(),
            "SWAR and HashLife must agree at gen {total_gens} when hl.level={} > PARALLEL_THRESHOLD={PARALLEL_THRESHOLD}",
            hl.level
        );
    }
}
