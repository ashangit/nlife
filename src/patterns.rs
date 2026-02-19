/// Available preset patterns for Conway's Game of Life, grouped by category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    // ── Still Lifes ────────────────────────────────────────────────────────────
    /// A 4-cell 2×2 still life — the simplest stable pattern.
    Block,
    /// A 6-cell still life shaped like a hexagon.
    Beehive,
    /// A 7-cell still life shaped like a bread loaf.
    Loaf,
    /// A 5-cell still life shaped like a boat.
    Boat,

    // ── Oscillators ───────────────────────────────────────────────────────────
    /// A 3-cell period-2 oscillator.
    Blinker,
    /// A 6-cell period-2 oscillator.
    Toad,
    /// An 8-cell period-2 oscillator formed by two diagonally adjacent 2×2 blocks.
    Beacon,
    /// A 48-cell period-3 symmetric oscillator.
    Pulsar,
    /// A 10-cell period-15 oscillator (row of 10 cells).
    Pentadecathlon,

    // ── Spaceships ────────────────────────────────────────────────────────────
    /// A 5-cell diagonal c/4 spaceship.
    Glider,
    /// A 9-cell c/2 orthogonal spaceship (lightweight).
    Lwss,
    /// An 11-cell c/2 orthogonal spaceship (middleweight).
    Mwss,
    /// A 13-cell c/2 orthogonal spaceship (heavyweight).
    Hwss,

    // ── Methuselahs ───────────────────────────────────────────────────────────
    /// A 5-cell methuselah with a lifespan of 1103 generations.
    RPentomino,
    /// A 7-cell methuselah that stabilises after 5206 generations.
    Acorn,
    /// A 7-cell methuselah that stabilises after 130 generations.
    Diehard,
}

/// Returns the list of `(row, col)` offsets from the grid centre for the given pattern.
///
/// Each entry is a `(row_offset, col_offset)` applied to `(height/2, width/2)`.
/// Coordinates are derived from canonical LifeWiki representations centred on the
/// bounding-box midpoint of the alive cells.
///
/// # Arguments
/// * `pattern` — the preset pattern to look up
///
/// # Returns
/// A `Vec<(i32, i32)>` of `(row_offset, col_offset)` pairs.
pub fn pattern_cells(pattern: Pattern) -> Vec<(i32, i32)> {
    match pattern {
        // ── Still Lifes ────────────────────────────────────────────────────────
        // Block: 2×2 square
        //  OO
        //  OO
        Pattern::Block => vec![(0, 0), (0, 1), (1, 0), (1, 1)],

        // Beehive: bounding box 3 rows × 4 cols, centred at (1, 1)
        //  .OO.
        //  O..O
        //  .OO.
        Pattern::Beehive => vec![(-1, 0), (-1, 1), (0, -1), (0, 2), (1, 0), (1, 1)],

        // Loaf: bounding box 4 rows × 4 cols, centred at (2, 2)
        //  .OO.
        //  O..O
        //  .O.O
        //  ..O.
        Pattern::Loaf => vec![
            (-2, -1),
            (-2, 0),
            (-1, -2),
            (-1, 1),
            (0, -1),
            (0, 1),
            (1, 0),
        ],

        // Boat: bounding box 3 rows × 3 cols, centred at (1, 1)
        //  OO.
        //  O.O
        //  .O.
        Pattern::Boat => vec![(-1, -1), (-1, 0), (0, -1), (0, 1), (1, 0)],

        // ── Oscillators ───────────────────────────────────────────────────────
        // Blinker (p2): 3 cells in a row
        Pattern::Blinker => vec![(0, -1), (0, 0), (0, 1)],

        // Toad (p2): two offset rows of 3, bounding box 2 rows × 4 cols
        //  .OOO
        //  OOO.
        Pattern::Toad => vec![(0, 0), (0, 1), (0, 2), (1, -1), (1, 0), (1, 1)],

        // Beacon (p2): two touching 2×2 blocks, bounding box 4 rows × 4 cols,
        // centred at (2, 2)
        //  OO..
        //  OO..
        //  ..OO
        //  ..OO
        Pattern::Beacon => vec![
            (-2, -2),
            (-2, -1),
            (-1, -2),
            (-1, -1),
            (0, 0),
            (0, 1),
            (1, 0),
            (1, 1),
        ],

        // Pulsar (p3): 48-cell symmetric oscillator.
        // Defined by cross-shaped arms at offsets ±2/±7 (rows) and ±4..±6 (cols)
        // plus the mirror image.
        Pattern::Pulsar => {
            let mut cells = Vec::new();
            for &r_sign in &[-1i32, 1] {
                for &c_sign in &[-1i32, 1] {
                    for &arm_col in &[4i32, 5, 6] {
                        cells.push((r_sign * 2, c_sign * arm_col));
                        cells.push((r_sign * 7, c_sign * arm_col));
                    }
                    for &arm_row in &[4i32, 5, 6] {
                        cells.push((r_sign * arm_row, c_sign * 2));
                        cells.push((r_sign * arm_row, c_sign * 7));
                    }
                }
            }
            cells
        }

        // Pentadecathlon (p15): row of 10 cells (the classic "polyomino" phase)
        Pattern::Pentadecathlon => vec![
            (0, -5),
            (0, -4),
            (0, -3),
            (0, -2),
            (0, -1),
            (0, 0),
            (0, 1),
            (0, 2),
            (0, 3),
            (0, 4),
        ],

        // ── Spaceships ────────────────────────────────────────────────────────
        Pattern::Glider => vec![(-1, 0), (0, 1), (1, -1), (1, 0), (1, 1)],

        // LWSS (c/2): bounding box 4 rows × 5 cols (cols 0-4), centred at (2, 2)
        //  .O..O
        //  O....
        //  O...O
        //  .OOOO
        Pattern::Lwss => vec![
            (-2, -1),
            (-2, 2),
            (-1, -2),
            (0, -2),
            (0, 2),
            (1, -1),
            (1, 0),
            (1, 1),
            (1, 2),
        ],

        // MWSS (c/2): bounding box 5 rows × 6 cols (cols 0-5), centred at (2, 3)
        //  ..O...
        //  O....O
        //  .....O
        //  O....O
        //  .OOOOO
        Pattern::Mwss => vec![
            (-2, -1),
            (-1, -3),
            (-1, 2),
            (0, 2),
            (1, -3),
            (1, 2),
            (2, -2),
            (2, -1),
            (2, 0),
            (2, 1),
            (2, 2),
        ],

        // HWSS (c/2): bounding box 5 rows × 7 cols (cols 0-6), centred at (2, 3)
        //  .OO....
        //  O....O.
        //  ......O
        //  O.....O
        //  .OOOOOO
        Pattern::Hwss => vec![
            (-2, -2),
            (-2, -1),
            (-1, -3),
            (-1, 2),
            (0, 3),
            (1, -3),
            (1, 3),
            (2, -2),
            (2, -1),
            (2, 0),
            (2, 1),
            (2, 2),
            (2, 3),
        ],

        // ── Methuselahs ───────────────────────────────────────────────────────
        // R-pentomino: 5 cells, bounding box 3 rows × 3 cols
        //  .OO
        //  OO.
        //  .O.
        Pattern::RPentomino => vec![(-1, 0), (-1, 1), (0, -1), (0, 0), (1, 0)],

        // Acorn: 7 cells, bounding box 3 rows × 7 cols (cols 0-6), centred at (1, 3)
        //  .O.....
        //  ...O...
        //  OO..OOO
        Pattern::Acorn => vec![(-1, -2), (0, 0), (1, -3), (1, -2), (1, 1), (1, 2), (1, 3)],

        // Diehard: 7 cells, bounding box 3 rows × 8 cols (cols 0-7), centred at (1, 4)
        //  ......O.
        //  OO......
        //  .O...OOO
        Pattern::Diehard => vec![(-1, 2), (0, -4), (0, -3), (1, -3), (1, 1), (1, 2), (1, 3)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── cell-count tests ──────────────────────────────────────────────────────

    #[test]
    fn test_block_cell_count() {
        assert_eq!(pattern_cells(Pattern::Block).len(), 4);
    }

    #[test]
    fn test_beehive_cell_count() {
        assert_eq!(pattern_cells(Pattern::Beehive).len(), 6);
    }

    #[test]
    fn test_loaf_cell_count() {
        assert_eq!(pattern_cells(Pattern::Loaf).len(), 7);
    }

    #[test]
    fn test_boat_cell_count() {
        assert_eq!(pattern_cells(Pattern::Boat).len(), 5);
    }

    #[test]
    fn test_glider_cell_count() {
        assert_eq!(pattern_cells(Pattern::Glider).len(), 5);
    }

    #[test]
    fn test_blinker_cell_count() {
        assert_eq!(pattern_cells(Pattern::Blinker).len(), 3);
    }

    #[test]
    fn test_toad_cell_count() {
        assert_eq!(pattern_cells(Pattern::Toad).len(), 6);
    }

    #[test]
    fn test_beacon_cell_count() {
        assert_eq!(pattern_cells(Pattern::Beacon).len(), 8);
    }

    #[test]
    fn test_pulsar_cell_count() {
        assert_eq!(pattern_cells(Pattern::Pulsar).len(), 48);
    }

    #[test]
    fn test_pentadecathlon_cell_count() {
        assert_eq!(pattern_cells(Pattern::Pentadecathlon).len(), 10);
    }

    #[test]
    fn test_lwss_cell_count() {
        assert_eq!(pattern_cells(Pattern::Lwss).len(), 9);
    }

    #[test]
    fn test_mwss_cell_count() {
        assert_eq!(pattern_cells(Pattern::Mwss).len(), 11);
    }

    #[test]
    fn test_hwss_cell_count() {
        assert_eq!(pattern_cells(Pattern::Hwss).len(), 13);
    }

    #[test]
    fn test_rpentomino_cell_count() {
        assert_eq!(pattern_cells(Pattern::RPentomino).len(), 5);
    }

    #[test]
    fn test_acorn_cell_count() {
        assert_eq!(pattern_cells(Pattern::Acorn).len(), 7);
    }

    #[test]
    fn test_diehard_cell_count() {
        assert_eq!(pattern_cells(Pattern::Diehard).len(), 7);
    }

    // ── no-duplicate test ─────────────────────────────────────────────────────

    #[test]
    fn test_no_duplicate_cells() {
        for pattern in [
            Pattern::Block,
            Pattern::Beehive,
            Pattern::Loaf,
            Pattern::Boat,
            Pattern::Blinker,
            Pattern::Toad,
            Pattern::Beacon,
            Pattern::Pulsar,
            Pattern::Pentadecathlon,
            Pattern::Glider,
            Pattern::Lwss,
            Pattern::Mwss,
            Pattern::Hwss,
            Pattern::RPentomino,
            Pattern::Acorn,
            Pattern::Diehard,
        ] {
            let cells = pattern_cells(pattern);
            let mut seen = std::collections::HashSet::new();
            for cell in &cells {
                assert!(
                    seen.insert(cell),
                    "Duplicate cell {cell:?} in pattern {pattern:?}",
                );
            }
        }
    }

    // ── behavioural tests ─────────────────────────────────────────────────────

    /// Block is a still life: must be identical after one step.
    #[test]
    fn test_block_is_still_life() {
        use crate::grid::Grid;
        let mut g = Grid::new(20, 20);
        let cr = 10i32;
        let cc = 10i32;
        for (dr, dc) in pattern_cells(Pattern::Block) {
            g.set((cr + dr) as usize, (cc + dc) as usize, true);
        }
        let before: Vec<_> = (0..g.height)
            .flat_map(|r| (0..g.width).map(move |c| (r, c)))
            .filter(|&(r, c)| g.get(r, c))
            .collect();
        g.step();
        let after: Vec<_> = (0..g.height)
            .flat_map(|r| (0..g.width).map(move |c| (r, c)))
            .filter(|&(r, c)| g.get(r, c))
            .collect();
        assert_eq!(before, after, "Block should be a still life");
    }

    /// Toad is a period-2 oscillator: after 2 steps it returns to the original state.
    #[test]
    fn test_toad_is_period_2() {
        use crate::grid::Grid;
        let mut g = Grid::new(20, 20);
        let cr = 10i32;
        let cc = 10i32;
        for (dr, dc) in pattern_cells(Pattern::Toad) {
            g.set((cr + dr) as usize, (cc + dc) as usize, true);
        }
        let before: Vec<_> = (0..g.height)
            .flat_map(|r| (0..g.width).map(move |c| (r, c)))
            .filter(|&(r, c)| g.get(r, c))
            .collect();
        g.step();
        g.step();
        let after: Vec<_> = (0..g.height)
            .flat_map(|r| (0..g.width).map(move |c| (r, c)))
            .filter(|&(r, c)| g.get(r, c))
            .collect();
        assert_eq!(before, after, "Toad should have period 2");
    }

    /// Beacon is a period-2 oscillator: after 2 steps it returns to the original state.
    #[test]
    fn test_beacon_is_period_2() {
        use crate::grid::Grid;
        let mut g = Grid::new(20, 20);
        let cr = 10i32;
        let cc = 10i32;
        for (dr, dc) in pattern_cells(Pattern::Beacon) {
            g.set((cr + dr) as usize, (cc + dc) as usize, true);
        }
        let before: Vec<_> = (0..g.height)
            .flat_map(|r| (0..g.width).map(move |c| (r, c)))
            .filter(|&(r, c)| g.get(r, c))
            .collect();
        g.step();
        g.step();
        let after: Vec<_> = (0..g.height)
            .flat_map(|r| (0..g.width).map(move |c| (r, c)))
            .filter(|&(r, c)| g.get(r, c))
            .collect();
        assert_eq!(before, after, "Beacon should have period 2");
    }
}
