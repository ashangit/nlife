use crate::camera::Camera;
use crate::simulation::Simulation;
use crate::ui::COLOR_BG;

/// Main application state for the Conway's Game of Life desktop app.
pub struct GameOfLifeApp {
    /// Simulation state (grid + timing + run control).
    pub(crate) sim: Simulation,
    /// Viewport camera (zoom + scroll state).
    pub(crate) camera: Camera,
    /// While the user is drag-painting, holds the alive/dead state being applied.
    /// `Some(true)` = painting alive, `Some(false)` = erasing.
    pub(crate) drag_paint_state: Option<bool>,
}

impl GameOfLifeApp {
    /// Creates a new `GameOfLifeApp` with an empty grid and default settings.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            sim: Simulation::new(),
            camera: Camera::new(),
            drag_paint_state: None,
        }
    }

    /// Advances the simulation by as many steps as `dt` seconds warrant at the current speed,
    /// capping `dt` at 0.1 s to avoid a large first-frame spike.
    fn advance_simulation(&mut self, ctx: &egui::Context) {
        if !self.sim.running {
            return;
        }
        let dt = (ctx.input(|i| i.unstable_dt) as f64).min(0.1);
        let (t, l) = self.sim.advance(dt);
        self.camera.apply_expansion(t, l);
        ctx.request_repaint();
    }
}

impl eframe::App for GameOfLifeApp {
    /// Called every frame to update the simulation and render the UI.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::input::handle_keyboard(self, ctx);
        crate::input::handle_zoom(self, ctx);
        self.advance_simulation(ctx);
        crate::ui::draw_top_panel(self, ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(COLOR_BG))
            .show(ctx, |ui| {
                let output = egui::ScrollArea::both()
                    .scroll_offset(self.camera.scroll_offset)
                    .show(ui, |ui| {
                        crate::ui::draw_grid(self, ui);
                    });
                self.camera.scroll_offset = output.state.offset;
                self.camera.viewport_rect = output.inner_rect;
            });
    }
}
