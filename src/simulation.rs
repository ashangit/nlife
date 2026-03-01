use std::time::{SystemTime, UNIX_EPOCH};

use crate::grid::Grid;
use crate::hashlife::HashLife;

/// Initial grid width in cells.
const GRID_COLS: usize = 100;
/// Initial grid height in cells.
const GRID_ROWS: usize = 60;
/// Dead-cell margin added on each side when auto-resizing for a large pattern.
const LOAD_MARGIN: usize = 40;
/// Default simulation speed in generations per second.
const DEFAULT_SPEED: f64 = 10.0;

// ── Engine ────────────────────────────────────────────────────────────────────

/// Selects which simulation backend is active.
///
/// `Swar` uses the bit-packed SWAR (SIMD Within A Register) engine from
/// [`Grid`]; `HashLife` uses the quadtree-memoised engine that can advance
/// many generations at once for periodic or repetitive patterns.
///
/// The `Swar` variant is intentionally not boxed — `Grid`'s data is already
/// entirely heap-allocated (Vecs and hash sets), so boxing the struct itself
/// would only add a gratuitous pointer indirection.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Engine {
    /// Bit-packed frontier-based SWAR engine.
    Swar(Grid),
    /// Quadtree-memoised HashLife engine (heap-allocated to keep the enum small).
    HashLife(Box<HashLife>),
}

// ── Simulation ────────────────────────────────────────────────────────────────

/// Pure simulation state — no egui dependency.
///
/// Holds the active simulation engine plus timing and run-control fields that
/// are independent of how the result is rendered.  All grid access goes through
/// the proxy methods (`width`, `height`, `get`, `set`, …) which dispatch to
/// whichever [`Engine`] is currently selected.
pub(crate) struct Simulation {
    /// Active simulation engine.
    pub(crate) engine: Engine,
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
    /// Log₂ of the HashLife step size (generations per step = 2^hl_step_log2).
    ///
    /// Propagated to the [`HashLife`] engine before each step.  Has no effect
    /// when the SWAR engine is active.
    pub(crate) hl_step_log2: u8,
}

impl Simulation {
    /// Creates a `Simulation` with a fresh SWAR grid (100×60) and default settings.
    pub(crate) fn new() -> Self {
        Self {
            engine: Engine::Swar(Grid::new(GRID_COLS, GRID_ROWS)),
            generation: 0,
            running: false,
            speed: DEFAULT_SPEED,
            time_since_last_step: 0.0,
            steps_per_frame: 1,
            hl_step_log2: 0,
        }
    }

    // ── Engine-proxy accessors ────────────────────────────────────────────────

    /// Returns the grid width in cells.
    pub(crate) fn width(&self) -> usize {
        match &self.engine {
            Engine::Swar(g) => g.width,
            Engine::HashLife(hl) => hl.width(),
        }
    }

    /// Returns the grid height in cells.
    pub(crate) fn height(&self) -> usize {
        match &self.engine {
            Engine::Swar(g) => g.height,
            Engine::HashLife(hl) => hl.height(),
        }
    }

    /// Returns the alive/dead state of the cell at `(row, col)`.
    ///
    /// Returns `false` for out-of-bounds coordinates.
    pub(crate) fn get(&self, row: usize, col: usize) -> bool {
        match &self.engine {
            Engine::Swar(g) => g.get(row, col),
            Engine::HashLife(hl) => hl.get(row, col),
        }
    }

    /// Sets the alive/dead state of the cell at `(row, col)`.
    ///
    /// Does nothing for out-of-bounds coordinates.
    pub(crate) fn set(&mut self, row: usize, col: usize, alive: bool) {
        match &mut self.engine {
            Engine::Swar(g) => g.set(row, col, alive),
            Engine::HashLife(hl) => hl.set(row, col, alive),
        }
    }

    /// Toggles the alive/dead state of the cell at `(row, col)`.
    ///
    /// Does nothing for out-of-bounds coordinates.
    pub(crate) fn toggle(&mut self, row: usize, col: usize) {
        match &mut self.engine {
            Engine::Swar(g) => g.toggle(row, col),
            Engine::HashLife(hl) => hl.toggle(row, col),
        }
    }

    /// Returns the total live-cell count.
    pub(crate) fn population(&self) -> u64 {
        match &self.engine {
            Engine::Swar(g) => g.live_count(),
            Engine::HashLife(hl) => hl.population(),
        }
    }

    /// Returns all live cells as centred `(row_offset, col_offset)` pairs,
    /// compatible with [`load_cells`](Simulation::load_cells).
    pub(crate) fn live_cells_offsets(&self) -> Vec<(i32, i32)> {
        match &self.engine {
            Engine::Swar(g) => g.live_cells_offsets(),
            Engine::HashLife(hl) => hl.live_cells_offsets(),
        }
    }

    /// Returns all live cells as `(row, col)` pairs normalised so the
    /// top-left of the bounding box is `(0, 0)`.  Intended for saving to disk.
    pub(crate) fn live_cells_for_save(&self) -> Vec<(usize, usize)> {
        match &self.engine {
            Engine::Swar(g) => {
                let live: Vec<(usize, usize)> = (0..g.height)
                    .flat_map(|r| (0..g.width).map(move |c| (r, c)))
                    .filter(|&(r, c)| g.get(r, c))
                    .collect();
                if live.is_empty() {
                    return live;
                }
                let row_min = live.iter().map(|&(r, _)| r).min().unwrap();
                let col_min = live.iter().map(|&(_, c)| c).min().unwrap();
                live.iter()
                    .map(|&(r, c)| (r - row_min, c - col_min))
                    .collect()
            }
            Engine::HashLife(hl) => hl.live_cells_for_save(),
        }
    }

    /// Returns all live cells within `[row_min, row_max) × [col_min, col_max)`.
    ///
    /// For SWAR, iterates and collects.  For HashLife, uses tree traversal.
    pub(crate) fn live_cells_in_viewport(
        &self,
        row_min: usize,
        col_min: usize,
        row_max: usize,
        col_max: usize,
    ) -> Vec<(usize, usize)> {
        match &self.engine {
            Engine::Swar(g) => {
                let mut out = Vec::new();
                for row in row_min..row_max {
                    for col in col_min..col_max {
                        if g.get(row, col) {
                            out.push((row, col));
                        }
                    }
                }
                out
            }
            Engine::HashLife(hl) => hl.live_cells_in_viewport(row_min, col_min, row_max, col_max),
        }
    }

    /// Returns `true` if the HashLife engine is currently active.
    pub(crate) fn is_hashlife(&self) -> bool {
        matches!(self.engine, Engine::HashLife(_))
    }

    // ── Lifecycle methods ─────────────────────────────────────────────────────

    /// Loads centred cell offsets as the new grid state, resets the generation
    /// counter, and stops the simulation.
    ///
    /// For SWAR, auto-resizes the grid if the pattern is larger than the
    /// current dimensions.  For HashLife, [`HashLife::set_cells`] auto-sizes
    /// internally.
    ///
    /// # Arguments
    /// * `cells` — centred `(row_offset, col_offset)` pairs as returned by
    ///   `decoded_library()` or `center_cells()`
    pub(crate) fn load_cells(&mut self, cells: &[(i32, i32)]) {
        match &mut self.engine {
            Engine::Swar(grid) => {
                if !cells.is_empty() {
                    let min_dr = cells.iter().map(|&(dr, _)| dr).min().unwrap();
                    let max_dr = cells.iter().map(|&(dr, _)| dr).max().unwrap();
                    let min_dc = cells.iter().map(|&(_, dc)| dc).min().unwrap();
                    let max_dc = cells.iter().map(|&(_, dc)| dc).max().unwrap();
                    let half_h = ((-min_dr).max(0) as usize).max((max_dr + 1).max(0) as usize);
                    let half_w = ((-min_dc).max(0) as usize).max((max_dc + 1).max(0) as usize);
                    let required_h = (2 * (half_h + LOAD_MARGIN)).max(GRID_ROWS);
                    let required_w = (2 * (half_w + LOAD_MARGIN)).max(GRID_COLS);
                    if required_h > grid.height || required_w > grid.width {
                        *grid = Grid::new(required_w, required_h);
                    }
                }
                grid.set_cells(cells);
            }
            Engine::HashLife(hl) => {
                hl.set_cells(cells);
            }
        }
        self.generation = 0;
        self.running = false;
        self.time_since_last_step = 0.0;
    }

    /// Fills the grid randomly using a time-derived seed and resets the simulation.
    ///
    /// # Arguments
    /// * `density_pct` — percentage of cells to set alive (0–100)
    pub(crate) fn fill_random(&mut self, density_pct: u8) {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(1);
        match &mut self.engine {
            Engine::Swar(g) => g.fill_random(density_pct, seed),
            Engine::HashLife(hl) => hl.fill_random(density_pct, seed),
        }
        self.generation = 0;
        self.time_since_last_step = 0.0;
    }

    /// Clears the grid, resets the generation counter, and stops the simulation.
    pub(crate) fn clear(&mut self) {
        match &mut self.engine {
            Engine::Swar(g) => g.clear(),
            Engine::HashLife(hl) => hl.clear(),
        }
        self.generation = 0;
        self.running = false;
        self.time_since_last_step = 0.0;
    }

    /// Advances the active engine by one logical step and returns
    /// `(add_top, add_left)` for scroll compensation.
    ///
    /// For SWAR, this is always 1 generation.  For HashLife, the generation
    /// counter is incremented by `2^effective_j` where
    /// `effective_j = hl_step_log2.min(level−2)`, and the returned expansion
    /// is equal on all four sides (symmetric grid growth).
    pub(crate) fn step_once(&mut self) -> (usize, usize) {
        match &mut self.engine {
            Engine::Swar(grid) => {
                grid.step();
                self.generation += 1;
                grid.expand_if_needed()
            }
            Engine::HashLife(hl) => {
                hl.set_step_log2(self.hl_step_log2);
                let (gens, expansion) = hl.step_universe();
                self.generation += gens;
                (expansion, expansion)
            }
        }
    }

    /// Advances the simulation by as many steps as `dt` seconds warrant at the
    /// current speed.
    ///
    /// Returns the total `(add_top, add_left)` expansion for scroll compensation.
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

    /// Switches between SWAR and HashLife engines, transferring the current
    /// live cells.  Preserves `generation`.
    ///
    /// Switching to HashLife: the current pattern is extracted as centred
    /// offsets and loaded into a new [`HashLife`] instance.
    ///
    /// Switching to SWAR: the inverse transfer with auto-resize identical to
    /// [`load_cells`](Simulation::load_cells).
    pub(crate) fn toggle_engine(&mut self) {
        let cells = self.live_cells_offsets();
        let saved_gen = self.generation;

        self.engine = match &self.engine {
            Engine::Swar(_) => {
                let mut hl = Box::new(HashLife::new());
                hl.set_cells(&cells);
                Engine::HashLife(hl)
            }
            Engine::HashLife(_) => {
                let mut grid = Grid::new(GRID_COLS, GRID_ROWS);
                if !cells.is_empty() {
                    let min_dr = cells.iter().map(|&(dr, _)| dr).min().unwrap();
                    let max_dr = cells.iter().map(|&(dr, _)| dr).max().unwrap();
                    let min_dc = cells.iter().map(|&(_, dc)| dc).min().unwrap();
                    let max_dc = cells.iter().map(|&(_, dc)| dc).max().unwrap();
                    let half_h = ((-min_dr).max(0) as usize).max((max_dr + 1).max(0) as usize);
                    let half_w = ((-min_dc).max(0) as usize).max((max_dc + 1).max(0) as usize);
                    let required_h = (2 * (half_h + LOAD_MARGIN)).max(GRID_ROWS);
                    let required_w = (2 * (half_w + LOAD_MARGIN)).max(GRID_COLS);
                    if required_h > grid.height || required_w > grid.width {
                        grid = Grid::new(required_w, required_h);
                    }
                }
                grid.set_cells(&cells);
                Engine::Swar(grid)
            }
        };

        self.generation = saved_gen;
        self.time_since_last_step = 0.0;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Loading a pattern whose extents exceed the default 100×60 SWAR grid must
    /// resize the grid to fit, with LOAD_MARGIN dead cells on each side.
    #[test]
    fn test_load_cells_auto_resize() {
        let mut sim = Simulation::new();
        let cells: Vec<(i32, i32)> = vec![(-200, -200), (-200, 200), (200, -200), (200, 200)];
        sim.load_cells(&cells);

        let min_dim = 2 * (201 + LOAD_MARGIN);
        assert!(
            sim.height() >= min_dim,
            "height {} < required {}",
            sim.height(),
            min_dim
        );
        assert!(
            sim.width() >= min_dim,
            "width {} < required {}",
            sim.width(),
            min_dim
        );

        let origin_row = (sim.height() / 2) as i32;
        let origin_col = (sim.width() / 2) as i32;
        for &(dr, dc) in &cells {
            let r = (origin_row + dr) as usize;
            let c = (origin_col + dc) as usize;
            assert!(sim.get(r, c), "cell ({r},{c}) should be alive but was dead");
        }
    }

    /// toggle_engine preserves the live-cell pattern (same offsets before and
    /// after SWAR→HashLife→SWAR round-trip).
    #[test]
    fn test_toggle_engine_preserves_pattern() {
        let mut sim = Simulation::new();
        // Load a small L-shape.
        sim.load_cells(&[(0, 0), (1, 0), (1, 1)]);
        assert!(!sim.is_hashlife());

        let before: std::collections::HashSet<_> = sim.live_cells_offsets().into_iter().collect();

        sim.toggle_engine();
        assert!(sim.is_hashlife());

        sim.toggle_engine();
        assert!(!sim.is_hashlife());

        let after: std::collections::HashSet<_> = sim.live_cells_offsets().into_iter().collect();

        assert_eq!(
            before, after,
            "pattern must survive SWAR→HL→SWAR round-trip"
        );
    }

    /// toggle_engine preserves the generation counter.
    #[test]
    fn test_toggle_engine_preserves_generation() {
        let mut sim = Simulation::new();
        sim.generation = 42;
        sim.toggle_engine();
        assert_eq!(sim.generation, 42);
        sim.toggle_engine();
        assert_eq!(sim.generation, 42);
    }

    // ── Variable step-size (hl_step_log2) tests ──────────────────────────────
    //
    // These tests reference `Simulation::hl_step_log2` and the propagation of
    // that field to the HashLife engine before each step.  They are written
    // *before* the implementation and will fail to compile until it exists.

    /// When hl_step_log2=0, a single step_once call on a HashLife simulation
    /// must advance generation by exactly 1.
    #[test]
    fn test_simulation_hl_step_log2_propagates() {
        let mut sim = Simulation::new();
        sim.toggle_engine(); // switch to HashLife

        // Load a blinker so there is something live to step.
        sim.load_cells(&[(0, -1), (0, 0), (0, 1)]);
        assert!(sim.is_hashlife());

        sim.hl_step_log2 = 0;
        sim.step_once();

        assert_eq!(
            sim.generation, 1,
            "hl_step_log2=0 must cause step_once to advance generation by exactly 1"
        );
    }

    /// step_once on HashLife increments generation by 1 when hl_step_log2=0 (default).
    #[test]
    fn test_hashlife_step_increments_generation_by_step_size() {
        let mut sim = Simulation::new();
        sim.toggle_engine(); // switch to HashLife
        // default hl_step_log2=0 → 2^0 = 1 generation per step
        sim.step_once();
        assert_eq!(sim.generation, 1);
    }
}
