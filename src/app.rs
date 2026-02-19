use egui::{Color32, Key, Painter, Pos2, Rect, Sense, Vec2};

use crate::grid::Grid;
use crate::patterns::Pattern;

/// Width of the game grid in cells.
const GRID_COLS: usize = 100;
/// Height of the game grid in cells.
const GRID_ROWS: usize = 60;
/// Default cell size in logical pixels.
const DEFAULT_CELL_SIZE: f32 = 10.0;
/// Default simulation speed in generations per second.
const DEFAULT_SPEED: f64 = 10.0;
/// Minimum allowed cell size in logical pixels.
const MIN_CELL_SIZE: f32 = 2.0;
/// Maximum allowed cell size in logical pixels.
const MAX_CELL_SIZE: f32 = 64.0;
/// Multiplicative factor for each keyboard/button zoom step.
const ZOOM_STEP: f32 = 1.2;

/// Background colour for the grid canvas.
const COLOR_BG: Color32 = Color32::from_gray(30);
/// Fill colour for live cells.
const COLOR_ALIVE: Color32 = Color32::from_rgb(180, 230, 100);
/// Fill colour for dead cells.
const COLOR_DEAD: Color32 = Color32::from_gray(45);

/// Main application state for the Conway's Game of Life desktop app.
pub struct GameOfLifeApp {
    /// The game grid.
    grid: Grid,
    /// Whether the simulation is currently running.
    running: bool,
    /// Simulation speed in generations per second (1–60).
    speed: f64,
    /// Accumulated time since the last step was performed.
    time_since_last_step: f64,
    /// Display size of each cell in logical pixels.
    cell_size: f32,
    /// While the user is drag-painting, this holds the alive/dead state being applied.
    /// `Some(true)` = painting alive, `Some(false)` = erasing.
    drag_paint_state: Option<bool>,
    /// Total number of generations simulated since the last clear/pattern load.
    generation: u64,
    /// Current scroll position of the grid viewport in logical pixels.
    ///
    /// Adjusted after each `expand_if_needed` call so the visible region stays
    /// centred on the same cells even when the grid grows at the top or left.
    scroll_offset: Vec2,
    /// Last-frame viewport rectangle from the ScrollArea (screen coordinates).
    /// Used to convert mouse hover position into viewport-relative zoom anchor.
    viewport_rect: egui::Rect,
}

impl GameOfLifeApp {
    /// Creates a new `GameOfLifeApp` with an empty grid and default settings.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            grid: Grid::new(GRID_COLS, GRID_ROWS),
            running: false,
            speed: DEFAULT_SPEED,
            time_since_last_step: 0.0,
            cell_size: DEFAULT_CELL_SIZE,
            drag_paint_state: None,
            generation: 0,
            scroll_offset: Vec2::ZERO,
            viewport_rect: egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(800.0, 600.0),
            ),
        }
    }

    /// Loads a preset pattern, resets the generation counter, and stops the simulation.
    fn load_pattern(&mut self, pattern: Pattern) {
        self.grid.set_pattern(pattern);
        self.generation = 0;
        self.running = false;
        self.time_since_last_step = 0.0;
    }

    /// Clears the grid, resets the generation counter, and stops the simulation.
    fn clear(&mut self) {
        self.grid.clear();
        self.generation = 0;
        self.running = false;
        self.time_since_last_step = 0.0;
    }

    /// Adjusts the scroll offset to compensate for grid growth at the top or left edge.
    ///
    /// Called after `expand_if_needed` so the viewport stays centred on the same region
    /// even when new dead rows/columns are prepended.
    ///
    /// # Arguments
    /// * `add_top`  — number of dead rows added above the existing content
    /// * `add_left` — number of dead columns added to the left of the existing content
    fn apply_expansion(&mut self, add_top: usize, add_left: usize) {
        self.scroll_offset.y += add_top as f32 * self.cell_size;
        self.scroll_offset.x += add_left as f32 * self.cell_size;
    }

    /// Scales `cell_size` by `factor` (clamped to [`MIN_CELL_SIZE`, `MAX_CELL_SIZE`]),
    /// adjusting `scroll_offset` so the point at `anchor` (viewport coordinates) stays fixed.
    ///
    /// # Arguments
    /// * `factor` — multiplicative zoom change (>1 = zoom in, <1 = zoom out)
    /// * `anchor` — position in viewport coordinates to zoom towards
    fn apply_zoom(&mut self, factor: f32, anchor: Vec2) {
        let old = self.cell_size;
        let new = (old * factor).clamp(MIN_CELL_SIZE, MAX_CELL_SIZE);
        let actual = new / old;
        self.scroll_offset = anchor * (actual - 1.0) + self.scroll_offset * actual;
        self.cell_size = new;
    }

    /// Processes zoom gestures: Ctrl+scroll wheel and touchpad pinch.
    ///
    /// Uses `egui::InputState::zoom_delta()` which abstracts both pinch-to-zoom and
    /// Ctrl+scroll. When Ctrl is held the raw scroll delta is consumed so the
    /// `ScrollArea` does not also scroll.
    fn handle_zoom(&mut self, ctx: &egui::Context) {
        let (zoom, ctrl, hover) =
            ctx.input(|i| (i.zoom_delta(), i.modifiers.ctrl, i.pointer.hover_pos()));
        if (zoom - 1.0).abs() < 1e-4 {
            return;
        }
        if ctrl {
            ctx.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
        }
        let vp = self.viewport_rect;
        let anchor = hover
            .map(|p| p - vp.min)
            .filter(|v| v.x >= 0.0 && v.y >= 0.0 && v.x <= vp.width() && v.y <= vp.height())
            .unwrap_or(vp.size() / 2.0);
        self.apply_zoom(zoom, anchor);
    }

    /// Advances the simulation by one generation and increments the counter.
    ///
    /// After stepping, the grid is expanded if any live cell has reached an edge,
    /// and the scroll offset is adjusted to keep the viewport in place.
    fn step_once(&mut self) {
        self.grid.step();
        self.generation += 1;
        let (t, l) = self.grid.expand_if_needed();
        self.apply_expansion(t, l);
    }

    /// Draws the top control panel with play/pause/step/clear buttons, speed slider,
    /// generation counter, and preset pattern buttons.
    fn draw_top_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("controls").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("▶ Play").clicked() {
                    self.running = true;
                }
                if ui.button("⏸ Pause").clicked() {
                    self.running = false;
                }
                if ui.button("⏭ Step").clicked() && !self.running {
                    self.step_once();
                }
                if ui.button("🗑 Clear").clicked() {
                    self.clear();
                }

                ui.separator();
                ui.label("Speed:");
                ui.add(
                    egui::Slider::new(&mut self.speed, 1.0..=60.0)
                        .suffix(" gen/s")
                        .step_by(1.0),
                );

                ui.separator();
                ui.label(format!("Gen: {}", self.generation));

                ui.separator();
                let pct = (self.cell_size / DEFAULT_CELL_SIZE * 100.0).round() as u32;
                ui.label(format!("Zoom: {pct}%"));
                let center = self.viewport_rect.size() / 2.0;
                if ui.button("＋").clicked() {
                    self.apply_zoom(ZOOM_STEP, center);
                }
                if ui.button("−").clicked() {
                    self.apply_zoom(1.0 / ZOOM_STEP, center);
                }
                if ui.button("1:1").clicked() {
                    self.apply_zoom(DEFAULT_CELL_SIZE / self.cell_size, center);
                }
            });

            // Still Lifes
            ui.horizontal(|ui| {
                ui.label("Still Lifes:");
                if ui.button("Block").clicked() {
                    self.load_pattern(Pattern::Block);
                }
                if ui.button("Beehive").clicked() {
                    self.load_pattern(Pattern::Beehive);
                }
                if ui.button("Loaf").clicked() {
                    self.load_pattern(Pattern::Loaf);
                }
                if ui.button("Boat").clicked() {
                    self.load_pattern(Pattern::Boat);
                }
            });

            // Oscillators
            ui.horizontal(|ui| {
                ui.label("Oscillators:");
                if ui.button("Blinker (p2)").clicked() {
                    self.load_pattern(Pattern::Blinker);
                }
                if ui.button("Toad (p2)").clicked() {
                    self.load_pattern(Pattern::Toad);
                }
                if ui.button("Beacon (p2)").clicked() {
                    self.load_pattern(Pattern::Beacon);
                }
                if ui.button("Pulsar (p3)").clicked() {
                    self.load_pattern(Pattern::Pulsar);
                }
                if ui.button("Pentadecathlon (p15)").clicked() {
                    self.load_pattern(Pattern::Pentadecathlon);
                }
            });

            // Spaceships
            ui.horizontal(|ui| {
                ui.label("Spaceships:");
                if ui.button("Glider").clicked() {
                    self.load_pattern(Pattern::Glider);
                }
                if ui.button("LWSS").clicked() {
                    self.load_pattern(Pattern::Lwss);
                }
                if ui.button("MWSS").clicked() {
                    self.load_pattern(Pattern::Mwss);
                }
                if ui.button("HWSS").clicked() {
                    self.load_pattern(Pattern::Hwss);
                }
            });

            // Methuselahs
            ui.horizontal(|ui| {
                ui.label("Methuselahs:");
                if ui.button("R-Pentomino").clicked() {
                    self.load_pattern(Pattern::RPentomino);
                }
                if ui.button("Acorn").clicked() {
                    self.load_pattern(Pattern::Acorn);
                }
                if ui.button("Diehard").clicked() {
                    self.load_pattern(Pattern::Diehard);
                }
            });
        });
    }

    /// Draws the central grid canvas, handles mouse drag painting, and returns the
    /// `egui::Response` for the allocated canvas region.
    fn draw_grid(&mut self, ui: &mut egui::Ui) {
        let desired = Vec2::new(
            (self.grid.width as f32) * self.cell_size,
            (self.grid.height as f32) * self.cell_size,
        );

        let (response, painter) = ui.allocate_painter(desired, Sense::click_and_drag());

        let origin = response.rect.min;

        // Handle mouse interaction
        self.handle_mouse(&response, origin, &painter);

        // Paint background
        painter.rect_filled(response.rect, 0.0, COLOR_BG);

        // Paint cells
        self.paint_cells(&painter, origin);
    }

    /// Converts a canvas position to `(row, col)` grid coordinates, returning `None`
    /// if the position is outside the grid bounds.
    fn pos_to_cell(&self, pos: Pos2, origin: Pos2) -> Option<(usize, usize)> {
        let rel = pos - origin;
        if rel.x < 0.0 || rel.y < 0.0 {
            return None;
        }
        let col = (rel.x / self.cell_size) as usize;
        let row = (rel.y / self.cell_size) as usize;
        if col < self.grid.width && row < self.grid.height {
            Some((row, col))
        } else {
            None
        }
    }

    /// Handles mouse click and drag events for painting/erasing cells on the grid.
    ///
    /// On drag start the state to paint (alive/dead) is determined by toggling the
    /// clicked cell. Subsequent drag events apply that same state to all traversed cells.
    fn handle_mouse(&mut self, response: &egui::Response, origin: Pos2, _painter: &Painter) {
        // Handle single click (press+release without drag)
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if let Some((row, col)) = self.pos_to_cell(pos, origin) {
                    self.grid.toggle(row, col);
                }
            }
        }

        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                if let Some((row, col)) = self.pos_to_cell(pos, origin) {
                    // The new state is the opposite of the current cell state
                    let old_state = self.grid.get(row, col);
                    self.drag_paint_state = Some(!old_state);
                    self.grid.toggle(row, col);
                }
            }
        }

        if response.dragged() {
            if let (Some(pos), Some(paint_alive)) =
                (response.interact_pointer_pos(), self.drag_paint_state)
            {
                if let Some((row, col)) = self.pos_to_cell(pos, origin) {
                    self.grid.set(row, col, paint_alive);
                }
            }
        }

        if response.drag_stopped() {
            self.drag_paint_state = None;
        }
    }

    /// Renders every cell of the grid to the painter using the alive/dead colours.
    fn paint_cells(&self, painter: &Painter, origin: Pos2) {
        let s = self.cell_size;
        let gap = 1.0_f32;
        let fill_size = s - gap;

        for row in 0..self.grid.height {
            for col in 0..self.grid.width {
                let x = origin.x + col as f32 * s;
                let y = origin.y + row as f32 * s;
                let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(fill_size));
                let color = if self.grid.get(row, col) {
                    COLOR_ALIVE
                } else {
                    COLOR_DEAD
                };
                painter.rect_filled(rect, 0.0, color);
            }
        }
    }

    /// Processes keyboard shortcuts:
    /// - `Space`   — toggle play/pause
    /// - `S`       — step one generation (paused only)
    /// - `R`       — clear the grid
    /// - `=` / `+` — zoom in
    /// - `-`       — zoom out
    /// - `0`       — reset zoom to 100 %
    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        let (toggle, step, clear, zoom_in, zoom_out, zoom_reset) = ctx.input(|i| {
            (
                i.key_pressed(Key::Space),
                i.key_pressed(Key::S) && !self.running,
                i.key_pressed(Key::R),
                i.key_pressed(Key::Equals) || i.key_pressed(Key::Plus),
                i.key_pressed(Key::Minus),
                i.key_pressed(Key::Num0),
            )
        });
        if toggle {
            self.running = !self.running;
        }
        if step {
            self.step_once();
        }
        if clear {
            self.clear();
        }
        let center = self.viewport_rect.size() / 2.0;
        if zoom_in {
            self.apply_zoom(ZOOM_STEP, center);
        }
        if zoom_out {
            self.apply_zoom(1.0 / ZOOM_STEP, center);
        }
        if zoom_reset {
            self.apply_zoom(DEFAULT_CELL_SIZE / self.cell_size, center);
        }
    }

    /// Advances the simulation by as many steps as `dt` seconds warrant at the current speed,
    /// capping `dt` at 0.1 s to avoid a large first-frame spike.
    fn advance_simulation(&mut self, ctx: &egui::Context) {
        if !self.running {
            return;
        }
        let dt = (ctx.input(|i| i.unstable_dt) as f64).min(0.1);
        self.time_since_last_step += dt;
        let interval = 1.0 / self.speed;
        while self.time_since_last_step >= interval {
            self.grid.step();
            self.generation += 1;
            self.time_since_last_step -= interval;
            let (t, l) = self.grid.expand_if_needed();
            self.apply_expansion(t, l);
        }
        ctx.request_repaint();
    }
}

impl eframe::App for GameOfLifeApp {
    /// Called every frame to update the simulation and render the UI.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_keyboard(ctx);
        self.handle_zoom(ctx);
        self.advance_simulation(ctx);
        self.draw_top_panel(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(COLOR_BG))
            .show(ctx, |ui| {
                let output = egui::ScrollArea::both()
                    .scroll_offset(self.scroll_offset)
                    .show(ui, |ui| {
                        self.draw_grid(ui);
                    });
                self.scroll_offset = output.state.offset;
                self.viewport_rect = output.inner_rect;
            });
    }
}
