use std::collections::HashSet;

use crate::patterns::{pattern_cells, Pattern};

// ── Bit-manipulation helpers ──────────────────────────────────────────────────

/// Returns `true` if the bit at `(row, col)` is set in the bit-packed slice.
///
/// # Arguments
/// * `cells`         — flat bit-packed row-major buffer (`u64` words)
/// * `words_per_row` — number of `u64` words per row
/// * `row`, `col`    — cell coordinates
#[inline]
fn get_bit(cells: &[u64], words_per_row: usize, row: usize, col: usize) -> bool {
    (cells[row * words_per_row + col / 64] >> (col % 64)) & 1 != 0
}

/// Sets or clears the bit at `(row, col)` in the bit-packed slice.
///
/// # Arguments
/// * `cells`         — flat bit-packed row-major buffer (mutable)
/// * `words_per_row` — number of `u64` words per row
/// * `row`, `col`    — cell coordinates
/// * `alive`         — `true` to set the bit, `false` to clear it
#[inline]
fn set_bit(cells: &mut [u64], words_per_row: usize, row: usize, col: usize, alive: bool) {
    let idx = row * words_per_row + col / 64;
    let bit = col % 64;
    if alive {
        cells[idx] |= 1u64 << bit;
    } else {
        cells[idx] &= !(1u64 << bit);
    }
}

/// Inserts every cell in the 3×3 neighbourhood of `(row, col)` into `frontier`,
/// clamping to the grid bounds given by `width` and `height`.
///
/// # Arguments
/// * `frontier` — destination set
/// * `row`, `col` — centre cell
/// * `width`, `height` — grid dimensions (for bounds checking)
fn add_neighborhood(
    frontier: &mut HashSet<(usize, usize)>,
    row: usize,
    col: usize,
    width: usize,
    height: usize,
) {
    for dr in [-1i32, 0, 1] {
        for dc in [-1i32, 0, 1] {
            let r = row as i32 + dr;
            let c = col as i32 + dc;
            if r >= 0 && c >= 0 && (r as usize) < height && (c as usize) < width {
                frontier.insert((r as usize, c as usize));
            }
        }
    }
}

/// A 2-D grid of cells for Conway's Game of Life with dead-cell boundaries.
///
/// ## Storage layout
/// Cells are stored in a flat bit-packed `Vec<u64>` in row-major order.
/// Each row occupies `words_per_row = ⌈width / 64⌉` words.
/// Within each word, bit `col % 64` corresponds to column `col`
/// (LSB = leftmost column of the word group).  Unused high bits in the
/// last word of each row are always zero.
///
/// ## Double-buffer
/// A pre-allocated `next` scratch buffer of the same size avoids heap
/// allocation on every step.  After computing the new generation into `next`,
/// the two buffers are swapped with a pointer flip.
///
/// ## Active-cell frontier
/// `frontier` holds every cell that is alive or adjacent to a live cell —
/// the only positions where a birth or death can occur.  Each step evaluates
/// only the frontier cells (`O(live + border)` instead of `O(width × height)`
/// or even `O(bounding_box_area)`), making sparse and multi-cluster patterns
/// very efficient.  `prev_written` records which cells were written to `next`
/// during the previous step so stale double-buffer values can be zeroed.
pub struct Grid {
    /// Number of columns.
    pub width: usize,
    /// Number of rows.
    pub height: usize,
    /// Number of `u64` words per row: `⌈width / 64⌉`.
    words_per_row: usize,
    /// Current generation cell states (read during step), bit-packed.
    cells: Vec<u64>,
    /// Scratch buffer for the next generation (written during step, then swapped).
    next: Vec<u64>,
    /// Tight bounding box of live cells: `[row_min, col_min, row_max, col_max]`
    /// (all inclusive).  `None` when the grid is empty.  Used by the renderer.
    pub live_bbox: Option<[usize; 4]>,
    /// Cells to evaluate in the next `step()` call: every cell that is alive
    /// or adjacent to a live cell.  Rebuilt after each step.
    frontier: HashSet<(usize, usize)>,
    /// Cells written to `next` in the most recent `step()` call.  Zeroed at
    /// the start of the following step to clear stale double-buffer values.
    prev_written: HashSet<(usize, usize)>,
}

impl Grid {
    /// Creates a new all-dead grid with the given dimensions.
    ///
    /// # Arguments
    /// * `width`  — number of columns
    /// * `height` — number of rows
    pub fn new(width: usize, height: usize) -> Self {
        let words_per_row = width.div_ceil(64);
        let n = height * words_per_row;
        Self {
            width,
            height,
            words_per_row,
            cells: vec![0u64; n],
            next: vec![0u64; n],
            live_bbox: None,
            frontier: HashSet::new(),
            prev_written: HashSet::new(),
        }
    }

    /// Returns the alive/dead state of the cell at `(row, col)`.
    ///
    /// Returns `false` for out-of-bounds coordinates.
    pub fn get(&self, row: usize, col: usize) -> bool {
        if row >= self.height || col >= self.width {
            return false;
        }
        get_bit(&self.cells, self.words_per_row, row, col)
    }

    /// Sets the alive/dead state of the cell at `(row, col)`.
    ///
    /// Does nothing for out-of-bounds coordinates.
    /// Always adds the 3×3 neighbourhood of `(row, col)` to the frontier so
    /// that the change is accounted for in the next `step()` call.
    pub fn set(&mut self, row: usize, col: usize, alive: bool) {
        if row < self.height && col < self.width {
            set_bit(&mut self.cells, self.words_per_row, row, col, alive);
            if alive {
                self.include_in_bbox(row, col);
            }
            add_neighborhood(&mut self.frontier, row, col, self.width, self.height);
        }
    }

    /// Toggles the alive/dead state of the cell at `(row, col)`.
    ///
    /// Does nothing for out-of-bounds coordinates.
    /// Expands `live_bbox` when the cell becomes alive (conservative — does not
    /// shrink when the cell dies).  Always adds the 3×3 neighbourhood to the
    /// frontier.
    pub fn toggle(&mut self, row: usize, col: usize) {
        if row < self.height && col < self.width {
            let wpr = self.words_per_row;
            let idx = row * wpr + col / 64;
            let bit = col % 64;
            self.cells[idx] ^= 1u64 << bit;
            let new_alive = (self.cells[idx] >> bit) & 1 != 0;
            if new_alive {
                self.include_in_bbox(row, col);
            }
            add_neighborhood(&mut self.frontier, row, col, self.width, self.height);
        }
    }

    /// Sets every cell to dead and resets both buffers, bounding box, and
    /// the frontier tracking sets.
    pub fn clear(&mut self) {
        self.cells.fill(0);
        self.next.fill(0);
        self.live_bbox = None;
        self.frontier.clear();
        self.prev_written.clear();
    }

    /// Advances the simulation by one generation using Conway's rules:
    /// - A live cell with 2 or 3 live neighbours survives.
    /// - A dead cell with exactly 3 live neighbours becomes alive.
    /// - All other cells die or stay dead.
    ///
    /// Out-of-bounds neighbours are treated as dead (finite, non-wrapping boundary).
    ///
    /// ## Optimisations
    /// - **Active-cell frontier**: only `frontier` cells are evaluated —
    ///   `O(live + border)` instead of `O(width × height)`.
    /// - **Bit-packed storage**: 8× less memory; improved cache utilisation.
    /// - **Double-buffer**: writes to `next` and swaps — no heap allocation.
    pub fn step(&mut self) {
        if self.frontier.is_empty() {
            return;
        }
        let width = self.width;
        let height = self.height;
        let wpr = self.words_per_row;

        // Collect the current frontier into a Vec to free the borrow on
        // `self.frontier` and allow `self.next` to be mutated below.
        let frontier: Vec<(usize, usize)> = self.frontier.iter().copied().collect();

        // Zero stale values left in `next` from two steps ago.
        for &(row, col) in &self.prev_written {
            set_bit(&mut self.next, wpr, row, col, false);
        }

        // Evaluate every frontier cell and build the next frontier.
        let mut new_frontier: HashSet<(usize, usize)> = HashSet::new();
        let mut new_live_bbox: Option<[usize; 4]> = None;

        for &(row, col) in &frontier {
            let n = count_neighbors(&self.cells, wpr, width, height, row, col);
            let alive = get_bit(&self.cells, wpr, row, col);
            let new_alive = matches!((alive, n), (true, 2) | (true, 3) | (false, 3));
            set_bit(&mut self.next, wpr, row, col, new_alive);
            if new_alive {
                new_live_bbox = Some(match new_live_bbox {
                    None => [row, col, row, col],
                    Some([rmin, cmin, rmax, cmax]) => {
                        [rmin.min(row), cmin.min(col), rmax.max(row), cmax.max(col)]
                    }
                });
                add_neighborhood(&mut new_frontier, row, col, width, height);
            }
        }

        std::mem::swap(&mut self.cells, &mut self.next);
        self.live_bbox = new_live_bbox;

        // Record which cells we wrote so stale values can be zeroed next step.
        self.prev_written = frontier.into_iter().collect();
        self.frontier = new_frontier;
    }

    /// Clears the grid and places `pattern` centred at `(height/2, width/2)`.
    ///
    /// Cells whose computed position falls outside the grid bounds are silently
    /// skipped.  `live_bbox` and `frontier` are rebuilt from the pattern cells
    /// via repeated `set` calls.
    ///
    /// # Arguments
    /// * `pattern` — the preset pattern to load
    pub fn set_pattern(&mut self, pattern: Pattern) {
        self.clear();
        let origin_row = (self.height / 2) as i32;
        let origin_col = (self.width / 2) as i32;
        for (dr, dc) in pattern_cells(pattern) {
            let r = origin_row + dr;
            let c = origin_col + dc;
            if r >= 0 && c >= 0 && (r as usize) < self.height && (c as usize) < self.width {
                self.set(r as usize, c as usize, true);
            }
        }
    }

    /// Checks all four edges for live cells and, for each edge that has one,
    /// adds `MARGIN` dead rows/columns on that side.  The cells buffer is
    /// rebuilt in place.
    ///
    /// Returns `(added_top_rows, added_left_cols)` so the caller can
    /// compensate its scroll offset.  `live_bbox`, `frontier`, and
    /// `prev_written` are shifted to match the new layout.
    pub fn expand_if_needed(&mut self) -> (usize, usize) {
        const MARGIN: usize = 20;
        let top = (0..self.width).any(|c| self.get(0, c));
        let bottom = (0..self.width).any(|c| self.get(self.height - 1, c));
        let left = (0..self.height).any(|r| self.get(r, 0));
        let right = (0..self.height).any(|r| self.get(r, self.width - 1));

        let add_top = if top { MARGIN } else { 0 };
        let add_bottom = if bottom { MARGIN } else { 0 };
        let add_left = if left { MARGIN } else { 0 };
        let add_right = if right { MARGIN } else { 0 };

        if add_top == 0 && add_bottom == 0 && add_left == 0 && add_right == 0 {
            return (0, 0);
        }

        let new_w = self.width + add_left + add_right;
        let new_h = self.height + add_top + add_bottom;
        let new_wpr = new_w.div_ceil(64);
        let n = new_h * new_wpr;
        let mut new_cells = vec![0u64; n];
        for row in 0..self.height {
            for col in 0..self.width {
                if get_bit(&self.cells, self.words_per_row, row, col) {
                    set_bit(&mut new_cells, new_wpr, row + add_top, col + add_left, true);
                }
            }
        }
        self.width = new_w;
        self.height = new_h;
        self.words_per_row = new_wpr;
        self.cells = new_cells;
        self.next = vec![0u64; n];

        fn shift(bbox: [usize; 4], dr: usize, dc: usize) -> [usize; 4] {
            [bbox[0] + dr, bbox[1] + dc, bbox[2] + dr, bbox[3] + dc]
        }
        self.live_bbox = self.live_bbox.map(|b| shift(b, add_top, add_left));

        // Remap frontier and prev_written to the new coordinate space.
        self.frontier = self
            .frontier
            .iter()
            .map(|&(r, c)| (r + add_top, c + add_left))
            .collect();
        self.prev_written = self
            .prev_written
            .iter()
            .map(|&(r, c)| (r + add_top, c + add_left))
            .collect();

        (add_top, add_left)
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Expands `live_bbox` to include the cell at `(row, col)`.
    fn include_in_bbox(&mut self, row: usize, col: usize) {
        self.live_bbox = Some(match self.live_bbox {
            None => [row, col, row, col],
            Some([rmin, cmin, rmax, cmax]) => {
                [rmin.min(row), cmin.min(col), rmax.max(row), cmax.max(col)]
            }
        });
    }

    /// Scans `cells` within `[r0..=r1, c0..=c1]` and returns the tight
    /// bounding box of all live cells found, or `None` if all cells are dead.
    ///
    /// Used by tests for brute-force reference comparisons.
    ///
    /// # Arguments
    /// * `r0`, `c0` — inclusive start of scan region
    /// * `r1`, `c1` — inclusive end of scan region
    #[cfg(test)]
    fn scan_live_bbox(&self, r0: usize, c0: usize, r1: usize, c1: usize) -> Option<[usize; 4]> {
        let wpr = self.words_per_row;
        let mut rmin = usize::MAX;
        let mut cmin = usize::MAX;
        let mut rmax = 0usize;
        let mut cmax = 0usize;
        let mut any = false;
        for row in r0..=r1 {
            for col in c0..=c1 {
                if get_bit(&self.cells, wpr, row, col) {
                    any = true;
                    rmin = rmin.min(row);
                    cmin = cmin.min(col);
                    rmax = rmax.max(row);
                    cmax = cmax.max(col);
                }
            }
        }
        if any {
            Some([rmin, cmin, rmax, cmax])
        } else {
            None
        }
    }
}

/// Counts live neighbours of cell `(row, col)` in a flat bit-packed slice.
///
/// Out-of-bounds neighbours are treated as dead (finite, non-wrapping boundary).
/// Extracted as a free function so it can be called without borrowing the whole
/// `Grid` (enabling split borrows with the mutable `next` buffer).
///
/// # Arguments
/// * `cells`         — flat bit-packed row-major buffer (`u64` words)
/// * `words_per_row` — number of `u64` words per row
/// * `width`         — grid width in columns
/// * `height`        — grid height in rows
/// * `row`, `col`    — cell to evaluate
pub(crate) fn count_neighbors(
    cells: &[u64],
    words_per_row: usize,
    width: usize,
    height: usize,
    row: usize,
    col: usize,
) -> u8 {
    let mut count = 0u8;
    for dr in [-1i32, 0, 1] {
        for dc in [-1i32, 0, 1] {
            if dr == 0 && dc == 0 {
                continue;
            }
            let r = row as i32 + dr;
            let c = col as i32 + dc;
            if r >= 0 && c >= 0 && (r as usize) < height && (c as usize) < width {
                count += get_bit(cells, words_per_row, r as usize, c as usize) as u8;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a grid from a list of `(row, col)` live cells.
    fn make_grid(width: usize, height: usize, live: &[(usize, usize)]) -> Grid {
        let mut g = Grid::new(width, height);
        for &(r, c) in live {
            g.set(r, c, true);
        }
        g
    }

    /// Collect all live `(row, col)` pairs from a grid (sorted row-major).
    fn live_cells(g: &Grid) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for r in 0..g.height {
            for c in 0..g.width {
                if g.get(r, c) {
                    v.push((r, c));
                }
            }
        }
        v
    }

    #[test]
    fn test_empty_grid_stays_empty() {
        let mut g = Grid::new(10, 10);
        g.step();
        assert!(live_cells(&g).is_empty());
    }

    #[test]
    fn test_blinker_oscillates() {
        let mut g = make_grid(20, 20, &[(5, 4), (5, 5), (5, 6)]);
        g.step();
        assert!(g.get(4, 5));
        assert!(g.get(5, 5));
        assert!(g.get(6, 5));
        assert_eq!(live_cells(&g).len(), 3);

        g.step();
        assert!(g.get(5, 4));
        assert!(g.get(5, 5));
        assert!(g.get(5, 6));
        assert_eq!(live_cells(&g).len(), 3);
    }

    #[test]
    fn test_block_still_life() {
        let mut g = make_grid(10, 10, &[(4, 4), (4, 5), (5, 4), (5, 5)]);
        g.step();
        assert!(g.get(4, 4));
        assert!(g.get(4, 5));
        assert!(g.get(5, 4));
        assert!(g.get(5, 5));
        assert_eq!(live_cells(&g).len(), 4);
    }

    #[test]
    fn test_glider_moves() {
        let mut g = make_grid(40, 40, &[(10, 11), (11, 12), (12, 10), (12, 11), (12, 12)]);
        for _ in 0..4 {
            g.step();
        }
        assert!(g.get(11, 12));
        assert!(g.get(12, 13));
        assert!(g.get(13, 11));
        assert!(g.get(13, 12));
        assert!(g.get(13, 13));
        assert_eq!(live_cells(&g).len(), 5);
    }

    #[test]
    fn test_expand_if_needed() {
        let mut g = make_grid(20, 20, &[(5, 5), (5, 6), (6, 5), (6, 6)]);
        let (t, l) = g.expand_if_needed();
        assert_eq!((t, l), (0, 0));
        assert_eq!(g.width, 20);
        assert_eq!(g.height, 20);

        let mut g2 = make_grid(20, 20, &[(0, 10)]);
        let (t2, l2) = g2.expand_if_needed();
        assert_eq!(t2, 20);
        assert_eq!(l2, 0);
        assert!(g2.get(20, 10));
        assert_eq!(g2.height, 40);

        let mut g3 = make_grid(20, 20, &[(10, 0)]);
        let (t3, l3) = g3.expand_if_needed();
        assert_eq!(t3, 0);
        assert_eq!(l3, 20);
        assert!(g3.get(10, 20));
        assert_eq!(g3.width, 40);
    }

    #[test]
    fn test_toggle() {
        let mut g = Grid::new(5, 5);
        assert!(!g.get(2, 2));
        g.toggle(2, 2);
        assert!(g.get(2, 2));
        g.toggle(2, 2);
        assert!(!g.get(2, 2));
    }

    #[test]
    fn test_clear() {
        let mut g = make_grid(5, 5, &[(0, 0), (1, 1), (2, 2)]);
        g.clear();
        assert!(live_cells(&g).is_empty());
    }

    #[test]
    fn test_underpopulation() {
        let mut g = make_grid(10, 10, &[(5, 5)]);
        g.step();
        assert!(!g.get(5, 5));
    }

    #[test]
    fn test_overpopulation() {
        let mut g = make_grid(10, 10, &[(1, 2), (2, 1), (2, 2), (2, 3), (3, 2)]);
        g.step();
        assert!(!g.get(2, 2), "centre cell should die from overpopulation");
    }

    #[test]
    fn test_set_pattern_clears_first() {
        let mut g = Grid::new(40, 40);
        g.set(0, 0, true);
        g.set_pattern(Pattern::Glider);
        assert!(
            !g.get(0, 0),
            "sentinel cell should be cleared after set_pattern"
        );
    }

    #[test]
    fn test_live_bbox_tracks_cells() {
        let mut g = Grid::new(20, 20);
        assert!(g.live_bbox.is_none(), "new grid should have no bbox");

        g.set(5, 8, true);
        assert_eq!(g.live_bbox, Some([5, 8, 5, 8]));

        g.set(3, 2, true);
        assert_eq!(g.live_bbox, Some([3, 2, 5, 8]));

        g.clear();
        assert!(g.live_bbox.is_none());
    }

    #[test]
    fn test_bit_packing_roundtrip() {
        let mut g = Grid::new(130, 4);
        g.set(0, 63, true);
        g.set(0, 64, true);
        g.set(1, 0, true);
        g.set(1, 127, true);

        assert!(g.get(0, 63));
        assert!(g.get(0, 64));
        assert!(!g.get(0, 62));
        assert!(!g.get(0, 65));
        assert!(g.get(1, 0));
        assert!(g.get(1, 127));
        assert!(!g.get(1, 1));
        assert!(!g.get(1, 126));
    }

    #[test]
    fn test_frontier_tracks_state() {
        // A new grid has an empty frontier.
        let mut g = Grid::new(10, 10);
        assert!(g.frontier.is_empty(), "new grid frontier should be empty");

        // Setting a cell alive populates the frontier.
        g.set(5, 5, true);
        assert!(
            !g.frontier.is_empty(),
            "frontier should be non-empty after set"
        );
        // The cell itself and all 8 neighbours must be in the frontier.
        for dr in [-1i32, 0, 1] {
            for dc in [-1i32, 0, 1] {
                let r = (5i32 + dr) as usize;
                let c = (5i32 + dc) as usize;
                assert!(
                    g.frontier.contains(&(r, c)),
                    "({r},{c}) missing from frontier after set(5,5,true)"
                );
            }
        }

        // Clearing resets the frontier.
        g.clear();
        assert!(
            g.frontier.is_empty(),
            "frontier should be empty after clear"
        );
    }

    #[test]
    fn test_frontier_step_correctness() {
        // Run a 50-step glider and compare with a brute-force reference.
        // Catches any frontier tracking or bit-packing bugs.
        let live: &[(usize, usize)] = &[(10, 11), (11, 12), (12, 10), (12, 11), (12, 12)];
        let mut optimised = make_grid(60, 60, live);
        let mut reference = make_grid(60, 60, live);

        for _ in 0..50 {
            optimised.step();

            // Brute-force reference step using only the public API + a snapshot.
            let w = reference.width;
            let h = reference.height;
            let mut snapshot = Vec::with_capacity(w * h);
            for r in 0..h {
                for c in 0..w {
                    snapshot.push(reference.get(r, c));
                }
            }
            for row in 0..h {
                for col in 0..w {
                    let mut n = 0u8;
                    for dr in [-1i32, 0, 1] {
                        for dc in [-1i32, 0, 1] {
                            if dr == 0 && dc == 0 {
                                continue;
                            }
                            let r = row as i32 + dr;
                            let c = col as i32 + dc;
                            if r >= 0 && c >= 0 && (r as usize) < h && (c as usize) < w {
                                n += snapshot[r as usize * w + c as usize] as u8;
                            }
                        }
                    }
                    let alive = snapshot[row * w + col];
                    reference.set(
                        row,
                        col,
                        matches!((alive, n), (true, 2) | (true, 3) | (false, 3)),
                    );
                }
            }
            reference.live_bbox = reference.scan_live_bbox(0, 0, h - 1, w - 1);
        }

        assert_eq!(
            live_cells(&optimised),
            live_cells(&reference),
            "frontier-based and brute-force states diverged after 50 steps"
        );
    }
}
