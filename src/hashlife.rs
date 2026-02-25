//! HashLife — quadtree-memoised Conway's Game of Life engine.
//!
//! Each canonical node is interned once and identified by a [`NodeId`] (`u32`
//! index into [`HashLife::nodes`]).  The two level-0 leaf nodes occupy fixed
//! slots: `DEAD = 0` and `ALIVE = 1`.
//!
//! The step advances the universe by **2^(level−2)** generations per call —
//! an exponential speed-up for repetitive/periodic patterns.

use rustc_hash::FxHashMap;

/// Index into the `HashLife::nodes` arena.  0 = DEAD leaf, 1 = ALIVE leaf.
pub(crate) type NodeId = u32;

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
    /// Arena of all interned nodes; `nodes[0]` = DEAD, `nodes[1]` = ALIVE.
    nodes: Vec<Node>,
    /// Canonicalisation map: `(nw, ne, sw, se)` → `NodeId`.
    canon: FxHashMap<(NodeId, NodeId, NodeId, NodeId), NodeId>,
    /// Memoisation cache for `step_recursive`.
    step_cache: FxHashMap<NodeId, NodeId>,
    /// Current root node.
    root: NodeId,
    /// Current quadtree level (root covers 2^level × 2^level cells).
    pub(crate) level: u8,
    /// Total generations elapsed since last clear / load.
    pub(crate) generation: u64,
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
        let mut hl = HashLife {
            nodes: vec![dead_leaf, alive_leaf],
            canon: FxHashMap::default(),
            step_cache: FxHashMap::default(),
            root: 0,
            level: 0,
            generation: 0,
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
        self.step_cache.clear();
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
        self.nodes.truncate(2); // keep DEAD and ALIVE leaves
        self.canon.clear();
        self.step_cache.clear();
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
        self.step_cache.clear();
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
        self.nodes.truncate(2);
        self.canon.clear();
        self.step_cache.clear();
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
        self.step_cache.clear();
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
        self.nodes[self.root as usize].pop
    }

    /// Computes the tight bounding box `[row_min, col_min, row_max, col_max]`
    /// (all inclusive) of all live cells, or `None` if the universe is empty.
    ///
    /// Traverses the tree once, pruning all-dead branches.
    #[allow(dead_code)]
    pub(crate) fn live_bbox(&self) -> Option<[usize; 4]> {
        self.compute_live_bbox(self.root, 0, 0, self.width())
    }

    /// Returns the number of generations advanced by a single call to
    /// [`step_universe`](HashLife::step_universe): `2^(level−2)`.
    pub(crate) fn step_size(&self) -> u64 {
        1u64 << (self.level as u32).saturating_sub(2)
    }

    /// Advances the universe by `2^(level−2)` generations.
    ///
    /// Expands the root as needed, runs `step_recursive`, re-centres the
    /// result, and returns `(gens_advanced, expansion_per_side)` where
    /// `expansion_per_side` is the number of cells added to each of the four
    /// sides due to `expand_root` calls (used for camera scroll compensation).
    pub(crate) fn step_universe(&mut self) -> (u64, usize) {
        let mut expansion: usize = 0;

        // Ensure at least MIN_STEP_LEVEL and that outer quadrants are empty.
        while self.level < MIN_STEP_LEVEL || self.needs_expansion() {
            expansion = expansion.saturating_add(1usize << (self.level.saturating_sub(1)));
            self.expand_root();
        }

        let gens = self.step_size();

        // Compute the center half of the universe advanced by gens generations.
        let result = self.step_recursive(self.root);

        // Re-centre: wrap result back into a same-level root with dead padding.
        let result_level = self.nodes[result as usize].level;
        let (r_nw, r_ne, r_sw, r_se) = {
            let n = self.nodes[result as usize];
            (n.nw, n.ne, n.sw, n.se)
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
        let key = (nw, ne, sw, se);
        if let Some(&id) = self.canon.get(&key) {
            return id;
        }
        let level = self.nodes[nw as usize].level + 1;
        let pop = self.nodes[nw as usize].pop
            + self.nodes[ne as usize].pop
            + self.nodes[sw as usize].pop
            + self.nodes[se as usize].pop;
        let id = self.nodes.len() as NodeId;
        self.nodes.push(Node {
            level,
            nw,
            ne,
            sw,
            se,
            pop,
        });
        self.canon.insert(key, id);
        id
    }

    /// Returns the canonical all-dead node at `level` (recursively constructed).
    ///
    /// Memoised via `canon` — each level's dead node is created at most once.
    fn make_dead_node(&mut self, level: u8) -> NodeId {
        if level == 0 {
            return DEAD;
        }
        let child = self.make_dead_node(level - 1);
        self.make_node(child, child, child, child)
    }

    /// Doubles the root level, centering the current content with dead padding.
    ///
    /// Old level-k root → new level-(k+1) root where old content occupies the
    /// center 2^k × 2^k of the new 2^(k+1) × 2^(k+1) grid.
    fn expand_root(&mut self) {
        let old_level = self.level;
        let d = self.make_dead_node(old_level - 1);
        let (nw, ne, sw, se) = {
            let r = self.nodes[self.root as usize];
            (r.nw, r.ne, r.sw, r.se)
        };
        let new_nw = self.make_node(d, d, d, nw);
        let new_ne = self.make_node(d, d, ne, d);
        let new_sw = self.make_node(d, sw, d, d);
        let new_se = self.make_node(se, d, d, d);
        self.root = self.make_node(new_nw, new_ne, new_sw, new_se);
        self.level = old_level + 1;
    }

    /// Returns `true` if any outer grandchild of the root contains live cells,
    /// indicating the pattern is too close to the boundary for a correct step.
    ///
    /// For `step_recursive(root)` to produce a correct center result, the outer
    /// three sub-quadrants of every root child must be all-dead.  The only safe
    /// sub-quadrants are `nw.se`, `ne.sw`, `sw.ne`, and `se.nw` (the four inner
    /// corners that together form the center half of the root).  All 12 outer
    /// grandchildren must therefore be empty.
    ///
    /// The previous implementation only checked the 4 corner grandchildren
    /// (`nw.nw`, `ne.ne`, `sw.sw`, `se.se`), missing patterns that reach the
    /// edge-but-not-corner regions (e.g. `nw.ne`, `ne.nw`, `sw.se`, etc.).
    fn needs_expansion(&self) -> bool {
        let r = self.nodes[self.root as usize];
        let nw = self.nodes[r.nw as usize];
        let ne = self.nodes[r.ne as usize];
        let sw = self.nodes[r.sw as usize];
        let se = self.nodes[r.se as usize];
        // All 12 outer grandchildren must be empty.
        // Only nw.se / ne.sw / sw.ne / se.nw are safe interior quadrants.
        self.nodes[nw.nw as usize].pop > 0
            || self.nodes[nw.ne as usize].pop > 0
            || self.nodes[nw.sw as usize].pop > 0
            || self.nodes[ne.nw as usize].pop > 0
            || self.nodes[ne.ne as usize].pop > 0
            || self.nodes[ne.se as usize].pop > 0
            || self.nodes[sw.nw as usize].pop > 0
            || self.nodes[sw.sw as usize].pop > 0
            || self.nodes[sw.se as usize].pop > 0
            || self.nodes[se.ne as usize].pop > 0
            || self.nodes[se.sw as usize].pop > 0
            || self.nodes[se.se as usize].pop > 0
    }

    /// Returns the canonical level-(k-1) node whose NE quadrant is `a.NE` and
    /// whose NW quadrant is `b.NW` (horizontal centre between two level-k nodes).
    ///
    /// Used to build the 9 sub-macrocells for `step_recursive`.
    fn horiz(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let (a_ne, a_se) = {
            let n = self.nodes[a as usize];
            (n.ne, n.se)
        };
        let (b_nw, b_sw) = {
            let n = self.nodes[b as usize];
            (n.nw, n.sw)
        };
        self.make_node(a_ne, b_nw, a_se, b_sw)
    }

    /// Returns the canonical level-(k-1) node from the bottom half of `a` and
    /// the top half of `b` (vertical centre between two level-k nodes).
    fn vert(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let (a_sw, a_se) = {
            let n = self.nodes[a as usize];
            (n.sw, n.se)
        };
        let (b_nw, b_ne) = {
            let n = self.nodes[b as usize];
            (n.nw, n.ne)
        };
        self.make_node(a_sw, a_se, b_nw, b_ne)
    }

    /// Returns the canonical level-(k-1) node from the shared inner corners of
    /// four level-k quadrant nodes (the "center4" sub-macrocell).
    fn center4(&mut self, nw: NodeId, ne: NodeId, sw: NodeId, se: NodeId) -> NodeId {
        let nw_se = self.nodes[nw as usize].se;
        let ne_sw = self.nodes[ne as usize].sw;
        let sw_ne = self.nodes[sw as usize].ne;
        let se_nw = self.nodes[se as usize].nw;
        self.make_node(nw_se, ne_sw, sw_ne, se_nw)
    }

    /// Brute-force 4×4 → 2×2 step for level-2 nodes.
    ///
    /// Extracts the 16 cells from the four level-1 children, applies one
    /// generation of Conway's rules to the 4 centre cells, and returns the
    /// canonical level-1 result node.
    fn step_level2(&mut self, node: NodeId) -> NodeId {
        let n = self.nodes[node as usize];
        let nw = self.nodes[n.nw as usize];
        let ne = self.nodes[n.ne as usize];
        let sw = self.nodes[n.sw as usize];
        let se = self.nodes[n.se as usize];

        // 4×4 grid; `true` = alive.
        let cells: [[bool; 4]; 4] = [
            [
                nw.nw == ALIVE,
                nw.ne == ALIVE,
                ne.nw == ALIVE,
                ne.ne == ALIVE,
            ],
            [
                nw.sw == ALIVE,
                nw.se == ALIVE,
                ne.sw == ALIVE,
                ne.se == ALIVE,
            ],
            [
                sw.nw == ALIVE,
                sw.ne == ALIVE,
                se.nw == ALIVE,
                se.ne == ALIVE,
            ],
            [
                sw.sw == ALIVE,
                sw.se == ALIVE,
                se.sw == ALIVE,
                se.se == ALIVE,
            ],
        ];

        let gol = |r: usize, c: usize| -> bool {
            let alive = cells[r][c];
            let mut nbrs = 0u32;
            for dr in [-1i32, 0, 1] {
                for dc in [-1i32, 0, 1] {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nr = r as i32 + dr;
                    let nc = c as i32 + dc;
                    if (0..4).contains(&nr) && (0..4).contains(&nc) {
                        nbrs += cells[nr as usize][nc as usize] as u32;
                    }
                }
            }
            nbrs == 3 || (alive && nbrs == 2)
        };

        let r_nw = if gol(1, 1) { ALIVE } else { DEAD };
        let r_ne = if gol(1, 2) { ALIVE } else { DEAD };
        let r_sw = if gol(2, 1) { ALIVE } else { DEAD };
        let r_se = if gol(2, 2) { ALIVE } else { DEAD };

        self.make_node(r_nw, r_ne, r_sw, r_se)
    }

    /// Memoised recursive step: advances `node` by `2^(level−2)` generations
    /// and returns the canonical level-(k-1) result.
    ///
    /// For level 2 this delegates to `step_level2`; for level k ≥ 3 it uses
    /// the 9-submacrocell algorithm (two rounds of recursive stepping on 4×4
    /// half-size overlapping tiles).
    fn step_recursive(&mut self, node: NodeId) -> NodeId {
        if let Some(&cached) = self.step_cache.get(&node) {
            return cached;
        }

        let level = self.nodes[node as usize].level;
        let result = if level == 2 {
            self.step_level2(node)
        } else {
            let (nw, ne, sw, se) = {
                let n = self.nodes[node as usize];
                (n.nw, n.ne, n.sw, n.se)
            };

            // ── 9 sub-macrocells (level k-1) ─────────────────────────────────
            let t0 = nw;
            let t1 = self.horiz(nw, ne);
            let t2 = ne;
            let t3 = self.vert(nw, sw);
            let t4 = self.center4(nw, ne, sw, se);
            let t5 = self.vert(ne, se);
            let t6 = sw;
            let t7 = self.horiz(sw, se);
            let t8 = se;

            // ── Step each → level k-2, advanced 2^(k-3) gens ─────────────────
            let s0 = self.step_recursive(t0);
            let s1 = self.step_recursive(t1);
            let s2 = self.step_recursive(t2);
            let s3 = self.step_recursive(t3);
            let s4 = self.step_recursive(t4);
            let s5 = self.step_recursive(t5);
            let s6 = self.step_recursive(t6);
            let s7 = self.step_recursive(t7);
            let s8 = self.step_recursive(t8);

            // ── Combine into 4 level-(k-1) nodes ─────────────────────────────
            let u0 = self.make_node(s0, s1, s3, s4);
            let u1 = self.make_node(s1, s2, s4, s5);
            let u2 = self.make_node(s3, s4, s6, s7);
            let u3 = self.make_node(s4, s5, s7, s8);

            // ── Step each again → another 2^(k-3) gens ───────────────────────
            let r0 = self.step_recursive(u0);
            let r1 = self.step_recursive(u1);
            let r2 = self.step_recursive(u2);
            let r3 = self.step_recursive(u3);

            self.make_node(r0, r1, r2, r3)
        };

        self.step_cache.insert(node, result);
        result
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
        let n = self.nodes[node as usize];
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
            let n = self.nodes[node as usize];
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
        self.make_node(new_nw, new_ne, new_sw, new_se)
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
        if self.nodes[node as usize].pop == 0 {
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
        let n = self.nodes[node as usize];
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

    /// Recursively computes the tight bounding box of all live cells within
    /// `node`'s region `[node_row, node_row+node_size) × [node_col, …)`.
    ///
    /// Returns `None` for all-dead subtrees.
    #[allow(dead_code)]
    fn compute_live_bbox(
        &self,
        node: NodeId,
        node_row: usize,
        node_col: usize,
        node_size: usize,
    ) -> Option<[usize; 4]> {
        if self.nodes[node as usize].pop == 0 {
            return None;
        }
        if node_size == 1 {
            return Some([node_row, node_col, node_row, node_col]);
        }
        let half = node_size / 2;
        let n = self.nodes[node as usize];
        let bboxes = [
            self.compute_live_bbox(n.nw, node_row, node_col, half),
            self.compute_live_bbox(n.ne, node_row, node_col + half, half),
            self.compute_live_bbox(n.sw, node_row + half, node_col, half),
            self.compute_live_bbox(n.se, node_row + half, node_col + half, half),
        ];
        bboxes.into_iter().flatten().reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Internals ─────────────────────────────────────────────────────────────

    /// DEAD and ALIVE are always at fixed indices 0 and 1 with the correct pop.
    #[test]
    fn test_leaf_nodes() {
        let hl = HashLife::new();
        assert_eq!(hl.nodes[DEAD as usize].pop, 0);
        assert_eq!(hl.nodes[ALIVE as usize].pop, 1);
        assert_eq!(hl.nodes[DEAD as usize].level, 0);
        assert_eq!(hl.nodes[ALIVE as usize].level, 0);
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
        assert_eq!(hl.nodes[dn as usize].pop, 0);
        assert_eq!(hl.nodes[dn as usize].level, 1);
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

        let result = hl.step_level2(block);
        // Result should be a level-1 node with all four cells alive.
        assert_eq!(hl.nodes[result as usize].pop, 4);
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

    /// live_bbox returns None for empty grid, Some for live cells.
    #[test]
    fn test_live_bbox() {
        let mut hl = HashLife::new();
        assert_eq!(hl.live_bbox(), None);
        let (r, c) = (10, 20);
        hl.set(r, c, true);
        let bbox = hl.live_bbox().unwrap();
        assert_eq!(bbox, [r, c, r, c]);
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

    /// step_size returns 2^(level-2).
    #[test]
    fn test_step_size() {
        let hl = HashLife::new();
        assert_eq!(hl.step_size(), 1u64 << (DEFAULT_LEVEL as u32 - 2));
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
        assert!(
            hl.needs_expansion(),
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
}
