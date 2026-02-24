use std::collections::HashSet;

use crate::patterns::{pattern_cells, Pattern};

/// Dead-cell margin added on each side when the grid expands.
const MARGIN: usize = 20;

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
/// * `frontier`       — destination set
/// * `row`, `col`     — centre cell
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

// ── SWAR step kernel ──────────────────────────────────────────────────────────

/// Computes one Conway's GoL step for all 64 bit positions in a single word
/// simultaneously, using SWAR (SIMD Within A Register) bitwise arithmetic.
///
/// Each call replaces 64 individual `count_neighbors` + rule checks with a
/// fixed sequence of ~30 bitwise operations, independent of grid density.
///
/// ## Bit/column convention
/// Bit `b` (0 = LSB) represents the cell at column `wi * 64 + b`.
/// "Left" means decreasing column index; "right" means increasing.
///
/// ## Boundary words
/// The caller passes `0` for any adjacent word that lies outside the grid
/// (dead-cell boundary).
///
/// ## Arguments
/// * `ap`, `a`, `an`  — above row: left-adjacent word, center word, right-adjacent word
/// * `cp`, `c`, `cn`  — current row (same order)
/// * `bp`, `b`, `bn`  — below row (same order)
///
/// Returns the new alive word for the center position.
#[inline]
#[allow(clippy::too_many_arguments)]
fn step_word(ap: u64, a: u64, an: u64, cp: u64, c: u64, cn: u64, bp: u64, b: u64, bn: u64) -> u64 {
    // ── 8 neighbour contributions (one bit per cell position) ─────────────────
    //
    // left-shift  (c << 1) | (cp >> 63):
    //   result[b] = c[b-1]  for b > 0,   result[0] = cp[63]
    //   → left neighbour in the same row.
    //
    // right-shift (c >> 1) | (cn << 63):
    //   result[b] = c[b+1]  for b < 63,  result[63] = cn[0]
    //   → right neighbour in the same row.
    let n0 = (c << 1) | (cp >> 63); // left  (same row)
    let n1 = (c >> 1) | (cn << 63); // right (same row)
    let n2 = (a << 1) | (ap >> 63); // above-left
    let n3 = a; // above
    let n4 = (a >> 1) | (an << 63); // above-right
    let n5 = (b << 1) | (bp >> 63); // below-left
    let n6 = b; // below
    let n7 = (b >> 1) | (bn << 63); // below-right

    // ── Bit-parallel addition of 8 one-bit values → 4-bit sum per position ───
    //
    // Uses carry-save adders (CSA) and half-adders (HA):
    //   CSA(a,b,c) → (sum = a^b^c,  carry = (a&b)|(b&c)|(a&c))
    //   HA(a,b)    → (sum = a^b,    carry = a&b)
    //
    // The tree reduces 8 one-bit inputs to a (bit2, bit1, bit0) triplet.
    // bit3 (only set when n=8) is computed but not needed: the Conway formula
    // !bit2 & bit1 & (bit0 | alive) already yields 0 for n=8 because bit1=0.

    // Stage 1 — reduce 8 → 6 values
    let s0 = n0 ^ n1 ^ n2;
    let c0 = (n0 & n1) | (n1 & n2) | (n0 & n2); // CSA(n0,n1,n2)

    let s1 = n3 ^ n4 ^ n5;
    let c1 = (n3 & n4) | (n4 & n5) | (n3 & n5); // CSA(n3,n4,n5)

    let s2 = n6 ^ n7;
    let c2 = n6 & n7; // HA(n6,n7)

    // Stage 2 — reduce weight-1 triple and weight-2 triple
    // s0,s1,s2 at weight 1 → s3 (bit0, final), c3 at weight 2
    let s3 = s0 ^ s1 ^ s2;
    let c3 = (s0 & s1) | (s1 & s2) | (s0 & s2); // CSA

    // c0,c1,c2 at weight 2 → s4 at weight 2, c4 at weight 4
    let s4 = c0 ^ c1 ^ c2;
    let c4 = (c0 & c1) | (c1 & c2) | (c0 & c2); // CSA

    // Stage 3 — merge weight-2 pair → s5 (bit1, final), c5 at weight 4
    let s5 = s4 ^ c3; // HA
    let c5 = s4 & c3;

    // Stage 4 — merge weight-4 pair → s6 (bit2)
    let s6 = c5 ^ c4; // HA (carry = bit3, implicit)

    // ── Conway's rule: new = (n==3) | (alive & n==2)
    //                       = !bit2 & bit1 & (bit0 | alive)
    !s6 & s5 & (s3 | c)
}

// ── Grid ──────────────────────────────────────────────────────────────────────

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
/// A pre-allocated `next` scratch buffer avoids heap allocation per step.
/// After computing the new generation, the two buffers are swapped.
///
/// ## Active-cell frontier + SWAR neighbour counting
/// `frontier` holds every cell that is alive or adjacent to a live cell.
/// `step()` maps these to `(row, word_index)` pairs, then calls `step_word`
/// which uses SWAR bitwise arithmetic to evaluate all 64 positions in a word
/// simultaneously — replacing 64 individual `count_neighbors` calls with
/// ~30 bitwise operations.  `prev_written_words` tracks which words were
/// written to `next` last step so stale values can be zeroed efficiently.
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
    /// Per-cell set of candidates for the next step: every cell that is alive
    /// or adjacent to a live cell.  Updated from newly-alive cells after each step.
    frontier: HashSet<(usize, usize)>,
    /// `(row, word_index)` pairs written to `next` in the most recent step.
    /// Zeroed at the start of the following step to clear stale double-buffer values.
    prev_written_words: HashSet<(usize, usize)>,
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
            prev_written_words: HashSet::new(),
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
    /// Always adds the 3×3 neighbourhood to the frontier so the change is
    /// accounted for in the next `step()` call.
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
    /// frontier tracking.
    pub fn clear(&mut self) {
        self.cells.fill(0);
        self.next.fill(0);
        self.live_bbox = None;
        self.frontier.clear();
        self.prev_written_words.clear();
    }

    /// Advances the simulation by one generation using Conway's rules:
    /// - A live cell with 2 or 3 live neighbours survives.
    /// - A dead cell with exactly 3 live neighbours becomes alive.
    /// - All other cells die or stay dead.
    ///
    /// Out-of-bounds neighbours are treated as dead (finite, non-wrapping boundary).
    ///
    /// ## Optimisations
    /// - **Active-cell frontier**: only words that contain a frontier cell are
    ///   evaluated — `O(live + border)` amortised.
    /// - **SWAR neighbour counting**: `step_word` evaluates 64 cells per ~30
    ///   bitwise operations instead of 64 individual neighbour-count loops.
    /// - **Bit-packed storage**: 8× less memory; improved cache utilisation.
    /// - **Double-buffer**: writes to `next` and swaps — no heap allocation.
    pub fn step(&mut self) {
        if self.frontier.is_empty() {
            return;
        }
        let width = self.width;
        let height = self.height;
        let wpr = self.words_per_row;

        // Map per-cell frontier to (row, word_index) pairs.
        let mut word_set: HashSet<(usize, usize)> = HashSet::with_capacity(self.frontier.len());
        for &(row, col) in &self.frontier {
            word_set.insert((row, col / 64));
        }

        // Zero words written last step that won't be overwritten this step.
        // This clears stale double-buffer values from two generations ago.
        for &(row, wi) in &self.prev_written_words {
            if !word_set.contains(&(row, wi)) {
                self.next[row * wpr + wi] = 0;
            }
        }

        // Evaluate each word in the frontier using the SWAR kernel.
        let mut new_frontier: HashSet<(usize, usize)> = HashSet::new();
        let mut new_live_bbox: Option<[usize; 4]> = None;

        for &(row, wi) in &word_set {
            // Read the 3×3 word neighbourhood from the current cells buffer.
            let ap = if row > 0 && wi > 0 {
                self.cells[(row - 1) * wpr + wi - 1]
            } else {
                0
            };
            let a = if row > 0 {
                self.cells[(row - 1) * wpr + wi]
            } else {
                0
            };
            let an = if row > 0 && wi + 1 < wpr {
                self.cells[(row - 1) * wpr + wi + 1]
            } else {
                0
            };
            let cp = if wi > 0 {
                self.cells[row * wpr + wi - 1]
            } else {
                0
            };
            let c = self.cells[row * wpr + wi];
            let cn = if wi + 1 < wpr {
                self.cells[row * wpr + wi + 1]
            } else {
                0
            };
            let bp = if row + 1 < height && wi > 0 {
                self.cells[(row + 1) * wpr + wi - 1]
            } else {
                0
            };
            let b = if row + 1 < height {
                self.cells[(row + 1) * wpr + wi]
            } else {
                0
            };
            let bn = if row + 1 < height && wi + 1 < wpr {
                self.cells[(row + 1) * wpr + wi + 1]
            } else {
                0
            };

            let mut new_word = step_word(ap, a, an, cp, c, cn, bp, b, bn);

            // Mask off unused high bits in the last word of each row.
            if wi + 1 == wpr && !width.is_multiple_of(64) {
                new_word &= (1u64 << (width % 64)) - 1;
            }

            self.next[row * wpr + wi] = new_word;

            // Extract alive bit positions and build the next frontier.
            let mut bits = new_word;
            while bits != 0 {
                let b_pos = bits.trailing_zeros() as usize;
                let col = wi * 64 + b_pos;
                new_live_bbox = Some(match new_live_bbox {
                    None => [row, col, row, col],
                    Some([rmin, cmin, rmax, cmax]) => {
                        [rmin.min(row), cmin.min(col), rmax.max(row), cmax.max(col)]
                    }
                });
                add_neighborhood(&mut new_frontier, row, col, width, height);
                bits &= bits - 1; // clear lowest set bit
            }
        }

        std::mem::swap(&mut self.cells, &mut self.next);
        self.live_bbox = new_live_bbox;
        self.prev_written_words = word_set;
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
    #[allow(dead_code)]
    pub fn set_pattern(&mut self, pattern: Pattern) {
        self.clear();
        let origin_row = (self.height / 2) as i32;
        let origin_col = (self.width / 2) as i32;
        for &(dr, dc) in pattern_cells(pattern) {
            let r = origin_row + dr;
            let c = origin_col + dc;
            if r >= 0 && c >= 0 && (r as usize) < self.height && (c as usize) < self.width {
                self.set(r as usize, c as usize, true);
            }
        }
    }

    /// Clears the grid and places `cells` (already-centred offsets) at the grid centre.
    ///
    /// Equivalent to [`set_pattern`] but accepts arbitrary `(row_offset, col_offset)`
    /// pairs instead of a [`Pattern`] enum value.  Offsets are added to
    /// `(height/2, width/2)` and cells that fall outside the grid bounds are
    /// silently skipped.  `live_bbox` and `frontier` are rebuilt from the
    /// placed cells via repeated [`set`] calls.
    ///
    /// # Arguments
    /// * `cells` — centred `(row_offset, col_offset)` pairs, e.g. from
    ///   `decoded_library()` or `center_cells()`
    pub fn set_cells(&mut self, cells: &[(i32, i32)]) {
        self.clear();
        let origin_row = (self.height / 2) as i32;
        let origin_col = (self.width / 2) as i32;
        for &(dr, dc) in cells {
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
    /// compensate its scroll offset.  `live_bbox` and `frontier` are shifted;
    /// `prev_written_words` is cleared (the fresh `next` buffer has no stale data).
    pub fn expand_if_needed(&mut self) -> (usize, usize) {
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
        self.next = vec![0u64; n]; // fresh zero buffer — no stale data

        fn shift(bbox: [usize; 4], dr: usize, dc: usize) -> [usize; 4] {
            [bbox[0] + dr, bbox[1] + dc, bbox[2] + dr, bbox[3] + dc]
        }
        self.live_bbox = self.live_bbox.map(|b| shift(b, add_top, add_left));

        self.frontier = self
            .frontier
            .iter()
            .map(|&(r, c)| (r + add_top, c + add_left))
            .collect();

        // next is freshly zeroed, so prev_written_words is irrelevant.
        self.prev_written_words.clear();

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
/// Retained as a utility for tests; production `step()` uses `step_word` instead.
///
/// # Arguments
/// * `cells`         — flat bit-packed row-major buffer (`u64` words)
/// * `words_per_row` — number of `u64` words per row
/// * `width`         — grid width in columns
/// * `height`        — grid height in rows
/// * `row`, `col`    — cell to evaluate
#[cfg(test)]
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
    fn test_step_word_all_neighbor_counts() {
        // Verify step_word against count_neighbors for all n in 0..=8.
        // Place n live cells in specific positions around a center and check
        // the center bit of the result.
        let wpr = 1usize;
        let w = 64usize;
        let h = 3usize;

        // Center cell: row 1, col 32 (bit 32 of word 0).
        // We'll put neighbors at positions around it in a 3-row, 64-col grid.
        // Neighbors of (1, 32): (0,31),(0,32),(0,33),(1,31),(1,33),(2,31),(2,32),(2,33)
        let neighbor_positions: &[(usize, usize)] = &[
            (0, 31),
            (0, 32),
            (0, 33),
            (1, 31),
            (1, 33),
            (2, 31),
            (2, 32),
            (2, 33),
        ];

        for n in 0u8..=8 {
            // Build cell words with exactly `n` of the 8 neighbors alive.
            let mut cells = vec![0u64; h * wpr];
            for &(r, c) in neighbor_positions.iter().take(n as usize) {
                cells[r * wpr + c / 64] |= 1u64 << (c % 64);
            }

            // Center alive
            let center_alive = cells[1 * wpr + 32 / 64] & (1u64 << 32) != 0;
            let expected_alive = n == 3 || (center_alive && n == 2);

            let a = cells[0 * wpr];
            let c = cells[1 * wpr];
            let b = cells[2 * wpr];
            let result = step_word(0, a, 0, 0, c, 0, 0, b, 0);
            let got = (result >> 32) & 1 != 0;

            assert_eq!(
                got, expected_alive,
                "step_word: n={n}, center_alive={center_alive}: expected={expected_alive} got={got}"
            );

            // Also cross-check with count_neighbors for the center cell.
            let cn = count_neighbors(&cells, wpr, w, h, 1, 32);
            assert_eq!(cn, n, "count_neighbors disagrees: expected {n}, got {cn}");
        }
    }

    #[test]
    fn test_step_word_word_boundary() {
        // Live cell at bit 63 of word 0 should influence bit 0 of word 1.
        // Three cells: (0,63), (0,64), (0,65) — a horizontal triplet spanning 2 words.
        // Cell (1, 64) is the center of the triplet → should become alive.
        let width = 128usize;
        let height = 3usize;
        let wpr = width.div_ceil(64);
        let mut cells = vec![0u64; height * wpr];

        // Row 0: bits 63, 64, 65 set (spanning words 0 and 1).
        cells[0 * wpr + 0] |= 1u64 << 63; // col 63
        cells[0 * wpr + 1] |= 1u64 << 0; // col 64
        cells[0 * wpr + 1] |= 1u64 << 1; // col 65

        // Compute word 1 of row 1 using step_word.
        let ap = cells[0 * wpr + 0]; // above-left word (word 0 of row 0)
        let a = cells[0 * wpr + 1]; // above word (word 1 of row 0)
        let an = 0u64; // above-right word (word 2, off-grid for col 128)
        let cp = cells[1 * wpr + 0];
        let c = cells[1 * wpr + 1];
        let cn = 0u64;
        let bp = cells[2 * wpr + 0];
        let b = cells[2 * wpr + 1];
        let bn = 0u64;

        let result = step_word(ap, a, an, cp, c, cn, bp, b, bn);
        // Cell (1, 64) = bit 0 of word 1: has 3 alive neighbors (0,63),(0,64),(0,65).
        assert!(
            result & 1 != 0,
            "cell (1,64) should be alive (3 above-neighbours)"
        );
        // Cell (1, 65) = bit 1: has 2 alive neighbors (0,64),(0,65) → stays dead.
        assert!(
            result & 2 == 0,
            "cell (1,65) should be dead (only 2 neighbours)"
        );
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
        let mut g = Grid::new(10, 10);
        assert!(g.frontier.is_empty(), "new grid frontier should be empty");

        g.set(5, 5, true);
        assert!(
            !g.frontier.is_empty(),
            "frontier should be non-empty after set"
        );
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

        g.clear();
        assert!(
            g.frontier.is_empty(),
            "frontier should be empty after clear"
        );
    }

    /// Asserts that `set_pattern(pattern)` on a 100×60 grid places exactly
    /// `expected_count` live cells, all within bounds, and that the centroid of
    /// those cells lies within ±1 cell of the grid centre `(height/2, width/2)`.
    ///
    /// # Arguments
    /// * `pattern`        — the pattern variant to test
    /// * `expected_count` — expected number of live cells after loading
    fn assert_pattern_valid(pattern: Pattern, expected_count: usize) {
        let width = 100usize;
        let height = 60usize;
        let mut g = Grid::new(width, height);
        g.set_pattern(pattern);

        let cells: Vec<(usize, usize)> = (0..height)
            .flat_map(|r| (0..width).map(move |c| (r, c)))
            .filter(|&(r, c)| g.get(r, c))
            .collect();

        // 1. Cell-count check.
        assert_eq!(
            cells.len(),
            expected_count,
            "{pattern:?}: expected {expected_count} live cells, got {}",
            cells.len()
        );

        // 2. Bounds check: every live cell is inside [0, height) × [0, width).
        for &(r, c) in &cells {
            assert!(
                r < height,
                "{pattern:?}: row {r} out of bounds [0, {height})"
            );
            assert!(c < width, "{pattern:?}: col {c} out of bounds [0, {width})");
        }

        // 3. Centroid within ±1 cell of (height/2, width/2).
        if !cells.is_empty() {
            let sum_r: f64 = cells.iter().map(|&(r, _)| r as f64).sum();
            let sum_c: f64 = cells.iter().map(|&(_, c)| c as f64).sum();
            let centroid_r = sum_r / cells.len() as f64;
            let centroid_c = sum_c / cells.len() as f64;
            let center_r = height as f64 / 2.0;
            let center_c = width as f64 / 2.0;
            assert!(
                (centroid_r - center_r).abs() <= 1.0,
                "{pattern:?}: centroid row {centroid_r:.2} not within ±1 of centre {center_r}"
            );
            assert!(
                (centroid_c - center_c).abs() <= 1.0,
                "{pattern:?}: centroid col {centroid_c:.2} not within ±1 of centre {center_c}"
            );
        }
    }

    #[test]
    fn test_set_pattern_block() {
        assert_pattern_valid(Pattern::Block, 4);
    }
    #[test]
    fn test_set_pattern_beehive() {
        assert_pattern_valid(Pattern::Beehive, 6);
    }
    #[test]
    fn test_set_pattern_loaf() {
        assert_pattern_valid(Pattern::Loaf, 7);
    }
    #[test]
    fn test_set_pattern_boat() {
        assert_pattern_valid(Pattern::Boat, 5);
    }
    #[test]
    fn test_set_pattern_blinker() {
        assert_pattern_valid(Pattern::Blinker, 3);
    }
    #[test]
    fn test_set_pattern_toad() {
        assert_pattern_valid(Pattern::Toad, 6);
    }
    #[test]
    fn test_set_pattern_beacon() {
        assert_pattern_valid(Pattern::Beacon, 8);
    }
    #[test]
    fn test_set_pattern_pulsar() {
        assert_pattern_valid(Pattern::Pulsar, 48);
    }
    #[test]
    fn test_set_pattern_pentadecathlon() {
        assert_pattern_valid(Pattern::Pentadecathlon, 10);
    }
    #[test]
    fn test_set_pattern_glider() {
        assert_pattern_valid(Pattern::Glider, 5);
    }
    #[test]
    fn test_set_pattern_lwss() {
        assert_pattern_valid(Pattern::Lwss, 9);
    }
    #[test]
    fn test_set_pattern_mwss() {
        assert_pattern_valid(Pattern::Mwss, 11);
    }
    #[test]
    fn test_set_pattern_hwss() {
        assert_pattern_valid(Pattern::Hwss, 13);
    }
    #[test]
    fn test_set_pattern_rpentomino() {
        assert_pattern_valid(Pattern::RPentomino, 5);
    }
    #[test]
    fn test_set_pattern_acorn() {
        assert_pattern_valid(Pattern::Acorn, 7);
    }
    #[test]
    fn test_set_pattern_diehard() {
        assert_pattern_valid(Pattern::Diehard, 7);
    }

    #[test]
    fn test_frontier_step_correctness() {
        // Run a 50-step glider and compare with a brute-force reference.
        let live: &[(usize, usize)] = &[(10, 11), (11, 12), (12, 10), (12, 11), (12, 12)];
        let mut optimised = make_grid(60, 60, live);
        let mut reference = make_grid(60, 60, live);

        for _ in 0..50 {
            optimised.step();

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
            "SWAR frontier-based and brute-force states diverged after 50 steps"
        );
    }
}
