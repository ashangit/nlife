use crate::patterns::{pattern_cells, Pattern};

/// A 2-D grid of cells for Conway's Game of Life with dead-cell boundaries.
///
/// Cells are stored in a flat `Vec<bool>` in row-major order:
/// `index = row * width + col`.
pub struct Grid {
    /// Number of columns.
    pub width: usize,
    /// Number of rows.
    pub height: usize,
    cells: Vec<bool>,
}

impl Grid {
    /// Creates a new all-dead grid with the given dimensions.
    ///
    /// # Arguments
    /// * `width`  — number of columns
    /// * `height` — number of rows
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![false; width * height],
        }
    }

    /// Returns the alive/dead state of the cell at `(row, col)`.
    ///
    /// Returns `false` for out-of-bounds coordinates.
    pub fn get(&self, row: usize, col: usize) -> bool {
        if row >= self.height || col >= self.width {
            return false;
        }
        self.cells[row * self.width + col]
    }

    /// Sets the alive/dead state of the cell at `(row, col)`.
    ///
    /// Does nothing for out-of-bounds coordinates.
    pub fn set(&mut self, row: usize, col: usize, alive: bool) {
        if row < self.height && col < self.width {
            self.cells[row * self.width + col] = alive;
        }
    }

    /// Toggles the alive/dead state of the cell at `(row, col)`.
    ///
    /// Does nothing for out-of-bounds coordinates.
    pub fn toggle(&mut self, row: usize, col: usize) {
        if row < self.height && col < self.width {
            let idx = row * self.width + col;
            self.cells[idx] = !self.cells[idx];
        }
    }

    /// Sets every cell to dead.
    pub fn clear(&mut self) {
        self.cells.fill(false);
    }

    /// Advances the simulation by one generation using Conway's rules:
    /// - A live cell with 2 or 3 live neighbours survives.
    /// - A dead cell with exactly 3 live neighbours becomes alive.
    /// - All other cells die or stay dead.
    ///
    /// Out-of-bounds neighbours are treated as dead (finite, non-wrapping boundary).
    pub fn step(&mut self) {
        let mut next = vec![false; self.width * self.height];
        for row in 0..self.height {
            for col in 0..self.width {
                let n = self.count_live_neighbors(row, col);
                let alive = self.cells[row * self.width + col];
                next[row * self.width + col] =
                    matches!((alive, n), (true, 2) | (true, 3) | (false, 3));
            }
        }
        self.cells = next;
    }

    /// Clears the grid and places `pattern` centred at `(height/2, width/2)`.
    ///
    /// Cells whose computed position falls outside the grid bounds are silently skipped.
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

    /// Returns the number of live neighbours of `(row, col)`.
    ///
    /// Out-of-bounds neighbours are treated as dead (dead-cell / finite boundary).
    fn count_live_neighbors(&self, row: usize, col: usize) -> u8 {
        let mut count = 0u8;
        for dr in [-1i32, 0, 1] {
            for dc in [-1i32, 0, 1] {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let r = row as i32 + dr;
                let c = col as i32 + dc;
                if r >= 0
                    && c >= 0
                    && (r as usize) < self.height
                    && (c as usize) < self.width
                    && self.cells[r as usize * self.width + c as usize]
                {
                    count += 1;
                }
            }
        }
        count
    }

    /// Checks all four edges for live cells and, for each edge that has one, adds
    /// `MARGIN` dead rows/columns on that side.  The cells buffer is rebuilt in place.
    ///
    /// Returns `(added_top_rows, added_left_cols)` so the caller can compensate its
    /// scroll offset and keep the viewport centred on the same region.
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
        let mut new_cells = vec![false; new_w * new_h];
        for row in 0..self.height {
            for col in 0..self.width {
                new_cells[(row + add_top) * new_w + (col + add_left)] =
                    self.cells[row * self.width + col];
            }
        }
        self.width = new_w;
        self.height = new_h;
        self.cells = new_cells;
        (add_top, add_left)
    }
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

    /// Collect all live `(row, col)` pairs from a grid.
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
        // Horizontal blinker at row 5, cols 4-5-6
        let mut g = make_grid(20, 20, &[(5, 4), (5, 5), (5, 6)]);
        g.step();
        // Should become vertical: rows 4-5-6, col 5
        assert!(g.get(4, 5));
        assert!(g.get(5, 5));
        assert!(g.get(6, 5));
        assert_eq!(live_cells(&g).len(), 3);

        g.step();
        // Back to horizontal
        assert!(g.get(5, 4));
        assert!(g.get(5, 5));
        assert!(g.get(5, 6));
        assert_eq!(live_cells(&g).len(), 3);
    }

    #[test]
    fn test_block_still_life() {
        // 2×2 block is a still life
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
        // Standard glider placed well away from edges (dead-cell boundary, not toroidal).
        // After 4 steps it shifts (+1 row, +1 col).
        let mut g = make_grid(40, 40, &[(10, 11), (11, 12), (12, 10), (12, 11), (12, 12)]);
        for _ in 0..4 {
            g.step();
        }
        // Glider shifted one row down and one col right
        assert!(g.get(11, 12));
        assert!(g.get(12, 13));
        assert!(g.get(13, 11));
        assert!(g.get(13, 12));
        assert!(g.get(13, 13));
        assert_eq!(live_cells(&g).len(), 5);
    }

    #[test]
    fn test_expand_if_needed() {
        // No expansion when all live cells are in the interior.
        let mut g = make_grid(20, 20, &[(5, 5), (5, 6), (6, 5), (6, 6)]);
        let (t, l) = g.expand_if_needed();
        assert_eq!((t, l), (0, 0));
        assert_eq!(g.width, 20);
        assert_eq!(g.height, 20);

        // A live cell on the top row → expand at top and left.
        let mut g2 = make_grid(20, 20, &[(0, 10)]);
        let (t2, l2) = g2.expand_if_needed();
        assert_eq!(t2, 20); // MARGIN = 20 rows added at top
        assert_eq!(l2, 0); // no live cell on left edge
                           // The cell that was at (0, 10) should now be at (20, 10).
        assert!(g2.get(20, 10));
        assert_eq!(g2.height, 40);

        // A live cell on the left edge → expand at left.
        let mut g3 = make_grid(20, 20, &[(10, 0)]);
        let (t3, l3) = g3.expand_if_needed();
        assert_eq!(t3, 0);
        assert_eq!(l3, 20); // MARGIN = 20 cols added at left
                            // The cell that was at (10, 0) should now be at (10, 20).
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
        // A single isolated live cell dies
        let mut g = make_grid(10, 10, &[(5, 5)]);
        g.step();
        assert!(!g.get(5, 5));
    }

    #[test]
    fn test_overpopulation() {
        // A live cell surrounded by 4+ live neighbours dies.
        // Centre cell (2,2) has 4 neighbours.
        let mut g = make_grid(10, 10, &[(1, 2), (2, 1), (2, 2), (2, 3), (3, 2)]);
        g.step();
        assert!(!g.get(2, 2), "centre cell should die from overpopulation");
    }

    #[test]
    fn test_set_pattern_clears_first() {
        let mut g = Grid::new(40, 40);
        // Place a sentinel cell far from centre
        g.set(0, 0, true);
        g.set_pattern(Pattern::Glider);
        assert!(
            !g.get(0, 0),
            "sentinel cell should be cleared after set_pattern"
        );
    }
}
