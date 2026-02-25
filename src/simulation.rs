use std::time::{SystemTime, UNIX_EPOCH};

use crate::grid::Grid;

/// Initial grid width in cells.
const GRID_COLS: usize = 100;
/// Initial grid height in cells.
const GRID_ROWS: usize = 60;
/// Dead-cell margin added on each side when auto-resizing for a large pattern.
const LOAD_MARGIN: usize = 40;
/// Default simulation speed in generations per second.
const DEFAULT_SPEED: f64 = 10.0;

/// Pure simulation state — no egui dependency.
///
/// Holds the grid, timing, and run-control fields that are independent of
/// how the result is rendered.
pub(crate) struct Simulation {
    /// The game grid.
    pub(crate) grid: Grid,
    /// Total generations since last clear/pattern load.
    pub(crate) generation: u64,
    /// Whether the simulation is currently running.
    pub(crate) running: bool,
    /// Simulation speed in generations per second (1–60).
    pub(crate) speed: f64,
    /// Accumulated time since the last step was performed.
    pub(crate) time_since_last_step: f64,
    /// Number of simulation steps to advance per visual frame.
    pub(crate) steps_per_frame: u32,
}

impl Simulation {
    /// Creates a Simulation with a fresh 100×60 grid and default settings.
    pub(crate) fn new() -> Self {
        Self {
            grid: Grid::new(GRID_COLS, GRID_ROWS),
            generation: 0,
            running: false,
            speed: DEFAULT_SPEED,
            time_since_last_step: 0.0,
            steps_per_frame: 1,
        }
    }

    /// Loads centred cell offsets as the new grid state, resets the generation
    /// counter, and stops the simulation.
    ///
    /// The `cells` slice is passed directly to [`Grid::set_cells`], which
    /// centres the pattern at `(height/2, width/2)`.
    ///
    /// # Arguments
    /// * `cells` — centred `(row_offset, col_offset)` pairs as returned by
    ///   `decoded_library()` or `center_cells()`
    pub(crate) fn load_cells(&mut self, cells: &[(i32, i32)]) {
        if !cells.is_empty() {
            let min_dr = cells.iter().map(|&(dr, _)| dr).min().unwrap();
            let max_dr = cells.iter().map(|&(dr, _)| dr).max().unwrap();
            let min_dc = cells.iter().map(|&(_, dc)| dc).min().unwrap();
            let max_dc = cells.iter().map(|&(_, dc)| dc).max().unwrap();
            // Half-extents: how far from centre the pattern reaches in each direction.
            let half_h = ((-min_dr).max(0) as usize).max((max_dr + 1).max(0) as usize);
            let half_w = ((-min_dc).max(0) as usize).max((max_dc + 1).max(0) as usize);
            let required_h = (2 * (half_h + LOAD_MARGIN)).max(GRID_ROWS);
            let required_w = (2 * (half_w + LOAD_MARGIN)).max(GRID_COLS);
            if required_h > self.grid.height || required_w > self.grid.width {
                self.grid = Grid::new(required_w, required_h);
            }
        }
        self.grid.set_cells(cells);
        self.generation = 0;
        self.running = false;
        self.time_since_last_step = 0.0;
    }

    /// Fills the grid randomly using a time-derived seed and resets the simulation.
    ///
    /// Derives the PRNG seed from `SystemTime::now()` so each call produces a
    /// different pattern.  Resets `generation` and `time_since_last_step`.
    ///
    /// # Arguments
    /// * `density_pct` — percentage of cells to set alive (0–100)
    pub(crate) fn fill_random(&mut self, density_pct: u8) {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(1);
        self.grid.fill_random(density_pct, seed);
        self.generation = 0;
        self.time_since_last_step = 0.0;
    }

    /// Clears the grid, resets the generation counter, and stops the simulation.
    pub(crate) fn clear(&mut self) {
        self.grid.clear();
        self.generation = 0;
        self.running = false;
        self.time_since_last_step = 0.0;
    }

    /// Advances the grid by one step and increments the generation counter.
    ///
    /// Returns `(add_top, add_left)` from `expand_if_needed` for scroll compensation.
    pub(crate) fn step_once(&mut self) -> (usize, usize) {
        self.grid.step();
        self.generation += 1;
        self.grid.expand_if_needed()
    }

    /// Advances the simulation by as many steps as `dt` seconds warrant at the
    /// current speed.
    ///
    /// Each timer tick runs `steps_per_frame` simulation steps. Returns the total
    /// `(add_top, add_left)` expansion accumulated across all steps taken, for
    /// scroll compensation by the caller.
    ///
    /// # Arguments
    /// * `dt` — elapsed time in seconds since the last call
    pub(crate) fn advance(&mut self, dt: f64) -> (usize, usize) {
        self.time_since_last_step += dt;
        let interval = 1.0 / self.speed;
        let mut total_top = 0usize;
        let mut total_left = 0usize;
        while self.time_since_last_step >= interval {
            for _ in 0..self.steps_per_frame {
                let (t, l) = self.step_once();
                total_top += t;
                total_left += l;
            }
            self.time_since_last_step -= interval;
        }
        (total_top, total_left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Loading a pattern whose extents exceed the default 100×60 grid must resize
    /// the grid to fit, with LOAD_MARGIN dead cells on each side.  No cells must
    /// be silently clipped.
    #[test]
    fn test_load_cells_auto_resize() {
        let mut sim = Simulation::new();
        // Pattern spans ±200 rows and ±200 cols from centre.
        let cells: Vec<(i32, i32)> = vec![(-200, -200), (-200, 200), (200, -200), (200, 200)];
        sim.load_cells(&cells);

        // Grid must be at least 2*(200+1+LOAD_MARGIN) = 482 in each dimension.
        let min_dim = 2 * (201 + LOAD_MARGIN);
        assert!(
            sim.grid.height >= min_dim,
            "height {} < required {}",
            sim.grid.height,
            min_dim
        );
        assert!(
            sim.grid.width >= min_dim,
            "width {} < required {}",
            sim.grid.width,
            min_dim
        );

        // All four corner cells must be alive — no clipping.
        let origin_row = (sim.grid.height / 2) as i32;
        let origin_col = (sim.grid.width / 2) as i32;
        for &(dr, dc) in &cells {
            let r = (origin_row + dr) as usize;
            let c = (origin_col + dc) as usize;
            assert!(
                sim.grid.get(r, c),
                "cell ({r},{c}) should be alive but was dead"
            );
        }
    }
}
