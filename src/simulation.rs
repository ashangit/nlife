use crate::grid::Grid;
use crate::patterns::Pattern;

/// Initial grid width in cells.
const GRID_COLS: usize = 100;
/// Initial grid height in cells.
const GRID_ROWS: usize = 60;
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

    /// Loads a pattern, resets the generation counter, and stops the simulation.
    ///
    /// # Arguments
    /// * `pattern` — the preset pattern to load into the grid
    #[allow(dead_code)]
    pub(crate) fn load_pattern(&mut self, pattern: Pattern) {
        self.grid.set_pattern(pattern);
        self.generation = 0;
        self.running = false;
        self.time_since_last_step = 0.0;
    }

    /// Loads centred cell offsets as the new grid state, resets the generation
    /// counter, and stops the simulation.
    ///
    /// This is the library-entry counterpart of [`load_pattern`].  The `cells`
    /// slice is passed directly to [`Grid::set_cells`], which centres the
    /// pattern at `(height/2, width/2)`.
    ///
    /// # Arguments
    /// * `cells` — centred `(row_offset, col_offset)` pairs as returned by
    ///   `decoded_library()` or `center_cells()`
    pub(crate) fn load_cells(&mut self, cells: &[(i32, i32)]) {
        self.grid.set_cells(cells);
        self.generation = 0;
        self.running = false;
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
