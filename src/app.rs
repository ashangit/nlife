use std::collections::{HashMap, VecDeque};

use crate::camera::Camera;
use crate::library::{Category, decoded_library};
use crate::rle::load_user_patterns;
use crate::simulation::Simulation;
use crate::ui::COLOR_BG;

/// Directory under `$HOME` where user-saved `.cells` pattern files are stored.
const USER_PATTERNS_SUBDIR: &str = ".config/newlife/patterns";

/// Keyboard shortcut table shown in the F1 help overlay.
///
/// Each entry is a `(key, description)` pair rendered in a two-column grid.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Space", "Play / Pause"),
    ("S", "Step one generation (paused)"),
    ("R", "Clear grid"),
    ("G", "Toggle grid lines"),
    ("= / +", "Zoom in"),
    ("-", "Zoom out"),
    ("0", "Reset zoom to 100 %"),
    ("F1", "Show / hide this cheat-sheet"),
    ("Ctrl+scroll", "Zoom in / out"),
    ("Middle-drag", "Pan viewport"),
    ("Shift+drag", "Rectangular selection"),
    ("Ctrl+C", "Copy selection"),
    ("Ctrl+V", "Paste clipboard (click to place)"),
    ("Delete", "Delete selection"),
    ("Escape", "Clear selection / cancel paste"),
];

/// An entry in the filtered, unified browser list.
///
/// `Library(i)` indexes into `decoded_library()`; `User(i)` indexes into
/// `GameOfLifeApp::user_patterns`.
#[derive(Debug, Clone)]
pub(crate) enum BrowserEntry {
    Library(usize),
    User(usize),
}

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
    /// Density percentage used by the "🎲 Random" fill button (1–100).
    pub(crate) random_density: u8,
    /// Rolling history of live-cell counts, kept at most 128 entries.
    ///
    /// Pushed once per `advance_simulation` call (not once per step).
    pub(crate) pop_history: VecDeque<u64>,
    /// Whether the F1 keyboard cheat-sheet overlay is currently visible.
    pub(crate) show_help: bool,
    /// Whether grid lines are currently shown on the canvas.
    pub(crate) show_grid_lines: bool,
    /// Screen-space position of the pointer at the last middle-button pan event.
    ///
    /// `Some(pos)` while a middle-button drag is in progress; `None` otherwise.
    /// Used to compute the per-frame delta for `camera.pan_by()`.
    pub(crate) mid_pan_last_pos: Option<egui::Pos2>,
    /// Whether the "Save Pattern…" inline name popup is currently open.
    pub(crate) save_popup_open: bool,
    /// Text field value for the in-progress save-pattern name.
    pub(crate) save_name: String,
    /// Filtered, unified list of browser entries (built-in + user).
    /// Rebuilt when `browser_category`, `browser_search`, or `user_patterns` change.
    pub(crate) browser_entries: Vec<BrowserEntry>,
    /// Value of `browser_category` when `browser_entries` was last built.
    pub(crate) browser_entries_cat: Option<Category>,
    /// Lowercased value of `browser_search` when `browser_entries` was last built.
    pub(crate) browser_entries_search: String,
    /// Cached `TextureHandle`s for pattern previews, keyed by pattern name.
    /// User-pattern keys are prefixed with `"user:"` to allow targeted invalidation.
    pub(crate) preview_textures: HashMap<String, egui::TextureHandle>,
    /// Error message from the last failed drag-and-drop file load, shown as a
    /// dismissible window at the bottom of the screen.
    pub(crate) drop_error: Option<String>,
    /// Currently active rectangular selection as inclusive grid coordinates
    /// `[rmin, cmin, rmax, cmax]`, or `None` when no selection is active.
    pub(crate) selection: Option<[usize; 4]>,
    /// Grid coordinate where the user began dragging a rectangular selection.
    pub(crate) selection_drag_start: Option<(usize, usize)>,
    /// Clipboard: live cells copied from the last `copy_selection` call,
    /// stored as `(row_offset, col_offset)` relative to the selection top-left.
    pub(crate) clipboard: Vec<(i64, i64)>,
    /// Anchor (top-left grid cell) for the pending paste preview, or `None`
    /// when no paste is in progress.
    pub(crate) paste_anchor: Option<(usize, usize)>,
}

impl GameOfLifeApp {
    /// Creates a new `GameOfLifeApp` with an empty grid and default settings.
    ///
    /// Loads user-saved patterns from `$HOME/.config/newlife/patterns/` on startup.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let user_patterns = user_patterns_dir()
            .map(|dir| load_user_patterns(&dir))
            .unwrap_or_default();
        let mut app = Self {
            sim: Simulation::new(),
            camera: Camera::new(),
            drag_paint_state: None,
            mid_pan_last_pos: None,
            browser_category: None,
            browser_search: String::new(),
            user_patterns,
            random_density: 30,
            pop_history: VecDeque::new(),
            show_help: false,
            show_grid_lines: false,
            save_popup_open: false,
            save_name: String::new(),
            browser_entries: Vec::new(),
            browser_entries_cat: None,
            browser_entries_search: String::new(),
            preview_textures: HashMap::new(),
            drop_error: None,
            selection: None,
            selection_drag_start: None,
            clipboard: Vec::new(),
            paste_anchor: None,
        };
        app.rebuild_browser_entries();
        app
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
        self.preview_textures.retain(|k, _| !k.starts_with("user:"));
        self.rebuild_browser_entries();
    }

    /// Rebuilds `browser_entries` from the current filter state and saves those
    /// values for change detection on subsequent frames.
    pub(crate) fn rebuild_browser_entries(&mut self) {
        let search_lower = self.browser_search.to_ascii_lowercase();
        let filter_cat = self.browser_category;
        self.browser_entries.clear();

        let show_builtin = filter_cat.map(|c| c != Category::Custom).unwrap_or(true);
        if show_builtin {
            for (i, (entry, _)) in decoded_library().iter().enumerate() {
                if let Some(cat) = filter_cat
                    && entry.category != cat
                {
                    continue;
                }
                if !search_lower.is_empty()
                    && !entry.name.to_ascii_lowercase().contains(&search_lower)
                {
                    continue;
                }
                self.browser_entries.push(BrowserEntry::Library(i));
            }
        }
        let show_custom = filter_cat.map(|c| c == Category::Custom).unwrap_or(true);
        if show_custom {
            for (i, (name, _)) in self.user_patterns.iter().enumerate() {
                if !search_lower.is_empty() && !name.to_ascii_lowercase().contains(&search_lower) {
                    continue;
                }
                self.browser_entries.push(BrowserEntry::User(i));
            }
        }
        self.browser_entries_cat = filter_cat;
        self.browser_entries_search = search_lower;
    }

    /// Resets the camera scroll offset to centre on the loaded pattern and clears pop history.
    ///
    /// Should be called immediately after every `sim.load_cells` invocation so the
    /// viewport is positioned at the centre of the grid where the pattern was placed.
    pub(crate) fn center_camera_on_grid(&mut self) {
        let s = self.camera.cell_size;
        let vp = self.camera.viewport_rect.size();
        self.camera.scroll_offset = egui::Vec2::new(
            (self.sim.width() as f32 * s / 2.0 - vp.x / 2.0).max(0.0),
            (self.sim.height() as f32 * s / 2.0 - vp.y / 2.0).max(0.0),
        );
        self.pop_history.clear();
    }

    /// Parses and loads a pattern from raw text content.
    ///
    /// `extension` is the file extension (e.g. `"rle"`, `"cells"`) used to select
    /// the parser.  `stem` becomes `sim.pattern_name` on success.
    ///
    /// Returns `Ok(())` on success; `Err(message)` (human-readable) on failure.
    pub(crate) fn process_dropped_file(
        &mut self,
        content: &str,
        extension: Option<&str>,
        stem: &str,
    ) -> Result<(), String> {
        let cells = match extension.map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("rle") => crate::rle::parse_rle(content)
                .map(|p| p.cells)
                .map_err(|e| format!("Failed to parse RLE: {e}")),
            Some("cells") => {
                crate::rle::parse_cells(content).map_err(|e| format!("Failed to parse .cells: {e}"))
            }
            _ => Err("Unsupported file type: only .rle and .cells are supported".to_string()),
        }?;
        let centred = crate::rle::center_cells(cells);
        self.sim.load_cells(&centred);
        self.sim.pattern_name = Some(stem.to_owned());
        self.center_camera_on_grid();
        Ok(())
    }

    /// Processes any files dropped onto the window this frame.
    ///
    /// Reads each file's content (from bytes if available, otherwise from disk),
    /// determines the format from the file extension, and calls
    /// [`process_dropped_file`].  On success `drop_error` is cleared; on failure
    /// it is set to a human-readable message.  For multiple simultaneous drops the
    /// last file wins (both for loading and for error state).
    pub(crate) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            // Resolve content: egui may supply bytes directly (common on Wayland)
            // or only a path (common on X11/Windows).
            let content_result: Result<String, String> = if let Some(bytes) = &file.bytes {
                std::str::from_utf8(bytes)
                    .map(str::to_owned)
                    .map_err(|e| format!("File is not valid UTF-8: {e}"))
            } else if let Some(path) = &file.path {
                std::fs::read_to_string(path).map_err(|e| format!("Could not read file: {e}"))
            } else {
                Err("No file content available".to_string())
            };

            match content_result {
                Err(e) => self.drop_error = Some(e),
                Ok(content) => {
                    let extension = file
                        .path
                        .as_ref()
                        .and_then(|p| p.extension())
                        .and_then(|e| e.to_str());
                    let stem = file
                        .path
                        .as_ref()
                        .and_then(|p| p.file_stem())
                        .and_then(|s| s.to_str())
                        .unwrap_or("dropped")
                        .to_owned();
                    match self.process_dropped_file(&content, extension, &stem) {
                        Ok(()) => self.drop_error = None,
                        Err(e) => self.drop_error = Some(e),
                    }
                }
            }
        }
    }

    /// Advances the simulation by as many steps as `dt` seconds warrant at the current speed,
    /// capping `dt` at 0.1 s to avoid a large first-frame spike.
    fn advance_simulation(&mut self, ctx: &egui::Context) {
        if !self.sim.running {
            // Paused idle path: run GC if needed without blocking a step.
            if self.sim.needs_gc() {
                self.sim.run_gc();
            }
            return;
        }
        let dt = (ctx.input(|i| i.unstable_dt) as f64).min(0.1);
        let (t, l) = self.sim.advance(dt);
        self.camera.apply_expansion(t, l);
        self.shift_selection(t, l);
        // Track live-cell population history (max 128 samples).
        self.pop_history.push_back(self.sim.population());
        if self.pop_history.len() > 128 {
            self.pop_history.pop_front();
        }
        // Running idle path: run GC once per frame after all steps complete.
        if self.sim.needs_gc() {
            self.sim.run_gc();
        }
        ctx.request_repaint();
    }
}

// ── Selection / clipboard ─────────────────────────────────────────────────────

impl GameOfLifeApp {
    /// Copies all live cells within `self.selection` into `self.clipboard` as
    /// `(row_offset, col_offset)` pairs relative to the selection's top-left
    /// corner `(rmin, cmin)`.  Does nothing when `self.selection` is `None`.
    pub(crate) fn copy_selection(&mut self) {
        let [rmin, cmin, rmax, cmax] = match self.selection {
            Some(s) => s,
            None => return,
        };
        let live = self
            .sim
            .live_cells_in_viewport(rmin, cmin, rmax + 1, cmax + 1);
        self.clipboard = live
            .into_iter()
            .map(|(r, c)| (r as i64 - rmin as i64, c as i64 - cmin as i64))
            .collect();
    }

    /// Erases every live cell that falls within `self.selection`, then clears
    /// `self.selection`.  Does nothing when `self.selection` is `None`.
    pub(crate) fn delete_selection(&mut self) {
        let [rmin, cmin, rmax, cmax] = match self.selection {
            Some(s) => s,
            None => return,
        };
        let live = self
            .sim
            .live_cells_in_viewport(rmin, cmin, rmax + 1, cmax + 1);
        for (r, c) in live {
            self.sim.set(r, c, false);
        }
        self.selection = None;
    }

    /// Places the clipboard cells into the grid at absolute position
    /// `(anchor_row, anchor_col)`.  Clipboard offsets that would land outside
    /// the grid bounds are silently skipped.  Does nothing when
    /// `self.clipboard` is empty.
    pub(crate) fn commit_paste(&mut self, anchor_row: usize, anchor_col: usize) {
        let width = self.sim.width();
        let height = self.sim.height();
        for &(dr, dc) in &self.clipboard {
            let r = anchor_row as i64 + dr;
            let c = anchor_col as i64 + dc;
            if r < 0 || c < 0 || r as usize >= height || c as usize >= width {
                continue;
            }
            self.sim.set(r as usize, c as usize, true);
        }
    }

    /// Shifts the selection, drag-start, and paste-anchor coordinates after a
    /// grid auto-expansion.
    ///
    /// Called from `advance_simulation` with the `(add_top, add_left)` values
    /// returned by `sim.advance` so that the selection rectangle stays aligned
    /// with the same logical cells after the grid grows.
    pub(crate) fn shift_selection(&mut self, add_top: usize, add_left: usize) {
        if let Some(ref mut sel) = self.selection {
            sel[0] += add_top;
            sel[2] += add_top;
            sel[1] += add_left;
            sel[3] += add_left;
        }
        if let Some(ref mut start) = self.selection_drag_start {
            start.0 += add_top;
            start.1 += add_left;
        }
        if let Some(ref mut anchor) = self.paste_anchor {
            anchor.0 += add_top;
            anchor.1 += add_left;
        }
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

#[cfg(test)]
impl GameOfLifeApp {
    /// Constructs a minimal `GameOfLifeApp` with default field values for unit tests.
    ///
    /// Unlike `new()`, this does not require an `eframe::CreationContext` and does
    /// not load user patterns from disk.
    pub(crate) fn new_for_test() -> Self {
        let mut app = Self {
            sim: Simulation::new(),
            camera: Camera::new(),
            drag_paint_state: None,
            mid_pan_last_pos: None,
            browser_category: None,
            browser_search: String::new(),
            user_patterns: Vec::new(),
            random_density: 30,
            pop_history: VecDeque::new(),
            show_help: false,
            show_grid_lines: false,
            save_popup_open: false,
            save_name: String::new(),
            browser_entries: Vec::new(),
            browser_entries_cat: None,
            browser_entries_search: String::new(),
            preview_textures: HashMap::new(),
            drop_error: None,
            selection: None,
            selection_drag_start: None,
            clipboard: Vec::new(),
            paste_anchor: None,
        };
        app.rebuild_browser_entries();
        app
    }
}

impl eframe::App for GameOfLifeApp {
    /// Called every frame to update the simulation and render the UI.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_dropped_files(ctx);
        crate::input::handle_keyboard(self, ctx);
        crate::input::handle_zoom(self, ctx);
        // Advance smooth-zoom animation; request repaint while still animating.
        if self.camera.tick_zoom() {
            ctx.request_repaint();
        }
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

        // Keyboard cheat-sheet overlay (F1).
        if self.show_help {
            let mut open = true;
            egui::Window::new("Keyboard Shortcuts")
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .open(&mut open)
                .show(ctx, |ui| {
                    egui::Grid::new("shortcuts_grid")
                        .striped(true)
                        .num_columns(2)
                        .show(ui, |ui| {
                            for &(key, desc) in SHORTCUTS {
                                ui.strong(key);
                                ui.label(desc);
                                ui.end_row();
                            }
                        });
                    ui.separator();
                    if ui.button("Close (F1)").clicked() {
                        self.show_help = false;
                    }
                });
            if !open {
                self.show_help = false;
            }
        }

        // Drag-and-drop error toast (dismissible window anchored to bottom-centre).
        if self.drop_error.is_some() {
            egui::Window::new("Drop Error")
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -20.0))
                .show(ctx, |ui| {
                    if let Some(msg) = &self.drop_error {
                        ui.label(msg);
                    }
                    if ui.button("OK").clicked() {
                        self.drop_error = None;
                    }
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_show_grid_lines_default() {
        let app = GameOfLifeApp::new_for_test();
        assert!(
            !app.show_grid_lines,
            "show_grid_lines should default to false"
        );
    }

    #[test]
    fn test_show_help_default() {
        let app = GameOfLifeApp::new_for_test();
        assert!(!app.show_help, "show_help should default to false");
    }

    #[test]
    fn test_drop_error_default_none() {
        let app = GameOfLifeApp::new_for_test();
        assert!(
            app.drop_error.is_none(),
            "drop_error should default to None"
        );
    }

    /// Dropping a valid glider RLE loads the pattern and leaves no error.
    #[test]
    fn test_drop_rle_valid() {
        let mut app = GameOfLifeApp::new_for_test();
        let glider_rle = "x = 3, y = 3, rule = B3/S23\nbo$2bo$3o!\n";
        let result = app.process_dropped_file(glider_rle, Some("rle"), "glider");
        assert!(result.is_ok(), "valid RLE should load without error");
        assert!(
            app.sim.population() > 0,
            "population should be non-zero after load"
        );
        assert!(
            app.drop_error.is_none(),
            "drop_error should be None after success"
        );
        assert_eq!(
            app.sim.pattern_name.as_deref(),
            Some("glider"),
            "pattern_name should be set from stem"
        );
    }

    /// Dropping a valid .cells file loads the pattern and leaves no error.
    #[test]
    fn test_drop_cells_valid() {
        let mut app = GameOfLifeApp::new_for_test();
        // Simple blinker in .cells format
        let blinker_cells = "!Name: blinker\nOOO\n";
        let result = app.process_dropped_file(blinker_cells, Some("cells"), "blinker");
        assert!(result.is_ok(), "valid .cells should load without error");
        assert!(
            app.sim.population() > 0,
            "population should be non-zero after load"
        );
        assert_eq!(app.sim.pattern_name.as_deref(), Some("blinker"));
    }

    /// Dropping a file with an unsupported extension yields an error.
    #[test]
    fn test_drop_unsupported_ext() {
        let mut app = GameOfLifeApp::new_for_test();
        let result = app.process_dropped_file("some content", Some("txt"), "notes");
        assert!(result.is_err(), "unsupported extension should return Err");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("Unsupported file type"),
            "error message should mention 'Unsupported file type', got: {msg}"
        );
    }

    /// Dropping a file with no extension yields an error.
    #[test]
    fn test_drop_no_extension() {
        let mut app = GameOfLifeApp::new_for_test();
        let result = app.process_dropped_file("some content", None, "noext");
        assert!(result.is_err(), "no extension should return Err");
    }

    /// Dropping an .rle file with garbage content yields a parse error.
    #[test]
    fn test_drop_invalid_rle_content() {
        let mut app = GameOfLifeApp::new_for_test();
        let result = app.process_dropped_file("not valid rle !!!", Some("rle"), "bad");
        assert!(result.is_err(), "invalid RLE content should return Err");
    }

    /// Dropping a .cells file with garbage content yields a parse error.
    #[test]
    fn test_drop_invalid_cells_content() {
        let mut app = GameOfLifeApp::new_for_test();
        // Content with an invalid character (not O, ., !, #, or newline)
        let result = app.process_dropped_file("ZZZZ\n", Some("cells"), "bad");
        assert!(result.is_err(), "invalid .cells content should return Err");
    }

    /// process_dropped_file with .rle extension is case-insensitive.
    #[test]
    fn test_drop_rle_extension_case_insensitive() {
        let mut app = GameOfLifeApp::new_for_test();
        let glider_rle = "x = 3, y = 3\nbo$2bo$3o!\n";
        let result = app.process_dropped_file(glider_rle, Some("RLE"), "glider");
        assert!(
            result.is_ok(),
            ".RLE uppercase extension should be accepted"
        );
    }

    // ── Selection / copy / paste tests ────────────────────────────────────────

    /// Helper: place a set of (row, col) live cells into `app.sim`.
    fn place_cells(app: &mut GameOfLifeApp, cells: &[(usize, usize)]) {
        for &(r, c) in cells {
            app.sim.set(r, c, true);
        }
    }

    /// copy_selection copies exactly the live cells inside the selection as
    /// relative (dr, dc) offsets from (rmin, cmin).
    #[test]
    fn test_copy_selection_basic() {
        let mut app = GameOfLifeApp::new_for_test();
        // Place three live cells: two inside selection, one outside.
        place_cells(&mut app, &[(2, 3), (2, 5), (10, 10)]);
        // Selection covers rows 2..=4, cols 3..=6 — captures (2,3) and (2,5).
        app.selection = Some([2, 3, 4, 6]);
        app.copy_selection();
        let mut got = app.clipboard.clone();
        got.sort_unstable();
        // Expected relative offsets: (2-2, 3-3)=(0,0) and (2-2, 5-3)=(0,2).
        let mut expected: Vec<(i64, i64)> = vec![(0, 0), (0, 2)];
        expected.sort_unstable();
        assert_eq!(
            got, expected,
            "clipboard should contain only cells inside selection as relative offsets"
        );
    }

    /// copy_selection with no live cells inside the selection yields an empty clipboard.
    #[test]
    fn test_copy_selection_empty_region() {
        let mut app = GameOfLifeApp::new_for_test();
        // Live cells are entirely outside the selection.
        place_cells(&mut app, &[(20, 20), (30, 30)]);
        app.selection = Some([0, 0, 5, 5]);
        app.copy_selection();
        assert!(
            app.clipboard.is_empty(),
            "clipboard must be empty when selection contains no live cells"
        );
    }

    /// delete_selection erases cells inside the selection and leaves cells
    /// outside untouched; selection is cleared afterwards.
    #[test]
    fn test_delete_selection_erases_inside_and_preserves_outside() {
        let mut app = GameOfLifeApp::new_for_test();
        // Cells inside selection at (3,3) and (4,4); outside at (10,10).
        place_cells(&mut app, &[(3, 3), (4, 4), (10, 10)]);
        app.selection = Some([3, 3, 5, 5]);
        app.delete_selection();
        assert!(
            !app.sim.get(3, 3),
            "cell (3,3) inside selection must be dead after delete"
        );
        assert!(
            !app.sim.get(4, 4),
            "cell (4,4) inside selection must be dead after delete"
        );
        assert!(
            app.sim.get(10, 10),
            "cell (10,10) outside selection must remain alive"
        );
        assert!(
            app.selection.is_none(),
            "selection must be cleared after delete_selection"
        );
    }

    /// commit_paste places clipboard cells at the given anchor.
    #[test]
    fn test_commit_paste_basic() {
        let mut app = GameOfLifeApp::new_for_test();
        // Manually load clipboard: relative offsets (0,0) and (1,2).
        app.clipboard = vec![(0, 0), (1, 2)];
        // Paste at anchor (5, 7).
        app.commit_paste(5, 7);
        assert!(
            app.sim.get(5, 7),
            "cell (5,7) = anchor + (0,0) must be alive after paste"
        );
        assert!(
            app.sim.get(6, 9),
            "cell (6,9) = anchor + (1,2) must be alive after paste"
        );
    }

    /// copy → delete → paste at the same origin reproduces the original pattern.
    #[test]
    fn test_copy_paste_round_trip() {
        let mut app = GameOfLifeApp::new_for_test();
        // Place an L-shaped pattern inside a 5×5 selection at (2,2).
        let original: Vec<(usize, usize)> = vec![(2, 2), (3, 2), (4, 2), (4, 3)];
        place_cells(&mut app, &original);
        app.selection = Some([2, 2, 6, 6]);
        // Step 1: copy.
        app.copy_selection();
        // Step 2: delete.
        app.selection = Some([2, 2, 6, 6]);
        app.delete_selection();
        // Verify all original cells are gone.
        for &(r, c) in &original {
            assert!(
                !app.sim.get(r, c),
                "cell ({r},{c}) must be dead after delete"
            );
        }
        // Step 3: paste back at original top-left (2,2).
        app.commit_paste(2, 2);
        // Verify original pattern is reproduced.
        for &(r, c) in &original {
            assert!(
                app.sim.get(r, c),
                "cell ({r},{c}) must be alive after round-trip paste"
            );
        }
    }

    /// commit_paste near the grid edge silently skips out-of-bounds offsets.
    #[test]
    fn test_commit_paste_out_of_bounds_skips_without_panic() {
        let mut app = GameOfLifeApp::new_for_test();
        // Grid is 100×60 by default (GRID_COLS × GRID_ROWS).
        // Anchor at bottom-right corner; offsets (0,0), (5,10), (100,200) — last two OOB.
        app.clipboard = vec![(0, 0), (5, 10), (100, 200)];
        let height = app.sim.height();
        let width = app.sim.width();
        // Anchor near the bottom-right so that large offsets go OOB.
        let anchor_row = height.saturating_sub(3);
        let anchor_col = width.saturating_sub(3);
        // Must not panic.
        app.commit_paste(anchor_row, anchor_col);
        // The (0,0) offset should be alive (anchor itself is in bounds).
        assert!(
            app.sim.get(anchor_row, anchor_col),
            "in-bounds cell at anchor must be alive after paste"
        );
    }
}
