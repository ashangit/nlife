use crate::camera::Camera;
use crate::library::Category;
use crate::rle::load_user_patterns;
use crate::simulation::Simulation;
use crate::ui::COLOR_BG;

/// Directory under `$HOME` where user-saved `.cells` pattern files are stored.
const USER_PATTERNS_SUBDIR: &str = ".config/newlife/patterns";

/// Main application state for the Conway's Game of Life desktop app.
pub struct GameOfLifeApp {
    /// Simulation state (grid + timing + run control).
    pub(crate) sim: Simulation,
    /// Viewport camera (zoom + scroll state).
    pub(crate) camera: Camera,
    /// While the user is drag-painting, holds the alive/dead state being applied.
    /// `Some(true)` = painting alive, `Some(false)` = erasing.
    pub(crate) drag_paint_state: Option<bool>,
    /// Active category filter in the pattern browser (`None` = All).
    pub(crate) browser_category: Option<Category>,
    /// Current text in the pattern browser name-search field.
    pub(crate) browser_search: String,
    /// User-saved patterns loaded from `~/.config/newlife/patterns/`.
    /// Each entry is `(name, centred_cells)`.
    pub(crate) user_patterns: Vec<(String, Vec<(i32, i32)>)>,
    /// Whether the "Save Pattern…" inline name popup is currently open.
    pub(crate) save_popup_open: bool,
    /// Text field value for the in-progress save-pattern name.
    pub(crate) save_name: String,
}

impl GameOfLifeApp {
    /// Creates a new `GameOfLifeApp` with an empty grid and default settings.
    ///
    /// Loads user-saved patterns from `$HOME/.config/newlife/patterns/` on startup.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let user_patterns = user_patterns_dir()
            .map(|dir| load_user_patterns(&dir))
            .unwrap_or_default();
        Self {
            sim: Simulation::new(),
            camera: Camera::new(),
            drag_paint_state: None,
            browser_category: None,
            browser_search: String::new(),
            user_patterns,
            save_popup_open: false,
            save_name: String::new(),
        }
    }

    /// Returns the absolute path to the user patterns directory and ensures it exists.
    ///
    /// Returns `None` if `$HOME` is not set or the directory cannot be created.
    pub(crate) fn ensure_user_patterns_dir() -> Option<String> {
        let dir = user_patterns_dir()?;
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }

    /// Rescans the user patterns directory and refreshes `user_patterns`.
    pub(crate) fn reload_user_patterns(&mut self) {
        self.user_patterns = user_patterns_dir()
            .map(|dir| load_user_patterns(&dir))
            .unwrap_or_default();
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

// ── Private helpers ───────────────────────────────────────────────────────────

/// Returns the path `$HOME/.config/newlife/patterns` as a `String`, or `None`
/// if the `HOME` environment variable is not set.
fn user_patterns_dir() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .map(|home| format!("{home}/{USER_PATTERNS_SUBDIR}"))
}

impl eframe::App for GameOfLifeApp {
    /// Called every frame to update the simulation and render the UI.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::input::handle_keyboard(self, ctx);
        crate::input::handle_zoom(self, ctx);
        self.advance_simulation(ctx);
        crate::ui::draw_top_panel(self, ctx);
        crate::ui::draw_pattern_browser(self, ctx);

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
