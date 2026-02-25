use egui::{Key, Vec2};

use crate::app::GameOfLifeApp;
use crate::camera::{DEFAULT_CELL_SIZE, ZOOM_STEP};

/// Processes keyboard shortcuts for the main application window.
///
/// Handled keys:
/// - `Space`   — toggle play/pause
/// - `S`       — step one generation (paused only)
/// - `R`       — clear the grid
/// - `G`       — toggle grid lines
/// - `=` / `+` — zoom in
/// - `-`       — zoom out
/// - `0`       — reset zoom to 100 %
///
/// # Arguments
/// * `app` — mutable application state
/// * `ctx` — egui context for reading input
pub(crate) fn handle_keyboard(app: &mut GameOfLifeApp, ctx: &egui::Context) {
    let (toggle, step, clear, grid_lines, zoom_in, zoom_out, zoom_reset) = ctx.input(|i| {
        (
            i.key_pressed(Key::Space),
            i.key_pressed(Key::S) && !app.sim.running,
            i.key_pressed(Key::R),
            i.key_pressed(Key::G),
            i.key_pressed(Key::Equals) || i.key_pressed(Key::Plus),
            i.key_pressed(Key::Minus),
            i.key_pressed(Key::Num0),
        )
    });
    if toggle {
        app.sim.running = !app.sim.running;
    }
    if step {
        let (t, l) = app.sim.step_once();
        app.camera.apply_expansion(t, l);
    }
    if clear {
        app.sim.clear();
    }
    if grid_lines {
        app.show_grid_lines = !app.show_grid_lines;
    }
    let center = app.camera.viewport_rect.size() / 2.0;
    if zoom_in {
        app.camera.apply_zoom(ZOOM_STEP, center);
    }
    if zoom_out {
        app.camera.apply_zoom(1.0 / ZOOM_STEP, center);
    }
    if zoom_reset {
        app.camera
            .apply_zoom(DEFAULT_CELL_SIZE / app.camera.cell_size, center);
    }
}

/// Processes zoom gestures: Ctrl+scroll wheel and touchpad pinch.
///
/// Uses `egui::InputState::zoom_delta()` which abstracts both pinch-to-zoom and
/// Ctrl+scroll. When Ctrl is held the raw scroll delta is consumed so the
/// `ScrollArea` does not also scroll.
///
/// # Arguments
/// * `app` — mutable application state
/// * `ctx` — egui context for reading input
pub(crate) fn handle_zoom(app: &mut GameOfLifeApp, ctx: &egui::Context) {
    let (zoom, ctrl, hover) =
        ctx.input(|i| (i.zoom_delta(), i.modifiers.ctrl, i.pointer.hover_pos()));
    if (zoom - 1.0).abs() < 1e-4 {
        return;
    }
    if ctrl {
        ctx.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
    }
    let vp = app.camera.viewport_rect;
    let anchor = hover
        .map(|p| p - vp.min)
        .filter(|v| v.x >= 0.0 && v.y >= 0.0 && v.x <= vp.width() && v.y <= vp.height())
        .unwrap_or(vp.size() / 2.0);
    app.camera.apply_zoom(zoom, anchor);
}
