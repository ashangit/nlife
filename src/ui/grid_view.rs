use egui::{Color32, Painter, Pos2, Rect, Sense, Vec2};

use crate::app::GameOfLifeApp;

/// Gap in logical pixels between adjacent cells (subtracted from cell_size when painting).
const CELL_GAP_PX: f32 = 1.0;
/// Background colour for the grid canvas.
pub(crate) const COLOR_BG: Color32 = Color32::from_gray(30);
/// Fill colour for live cells.
const COLOR_ALIVE: Color32 = Color32::from_rgb(180, 230, 100);
/// Fill colour for dead cells.
const COLOR_DEAD: Color32 = Color32::from_gray(45);

/// Draws the central grid canvas and handles mouse drag-painting.
///
/// Allocates a canvas sized to the full grid, delegates mouse event handling to
/// `handle_mouse`, fills the background, then renders visible cells via `paint_cells`.
///
/// # Arguments
/// * `app` — mutable application state
/// * `ui`  — egui UI context for the central panel
pub(crate) fn draw_grid(app: &mut GameOfLifeApp, ui: &mut egui::Ui) {
    let desired = Vec2::new(
        (app.sim.grid.width as f32) * app.camera.cell_size,
        (app.sim.grid.height as f32) * app.camera.cell_size,
    );

    let (response, painter) = ui.allocate_painter(desired, Sense::click_and_drag());

    let origin = response.rect.min;

    // Handle mouse interaction
    handle_mouse(app, &response, origin);

    // Paint background
    painter.rect_filled(response.rect, 0.0, COLOR_BG);

    // Paint cells (only those inside the visible viewport)
    let viewport = app.camera.viewport_rect;
    paint_cells(app, &painter, origin, viewport);
}

/// Handles mouse click and drag events for painting/erasing cells on the grid.
///
/// On drag start the state to paint (alive/dead) is determined by toggling the
/// clicked cell. Subsequent drag events apply that same state to all traversed cells.
///
/// # Arguments
/// * `app`      — mutable application state
/// * `response` — egui response for the canvas widget
/// * `origin`   — screen-space top-left corner of the grid canvas
fn handle_mouse(app: &mut GameOfLifeApp, response: &egui::Response, origin: Pos2) {
    let (w, h) = (app.sim.grid.width, app.sim.grid.height);

    // Handle single click (press+release without drag)
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if let Some((row, col)) = app.camera.pos_to_cell(pos, origin, w, h) {
                app.sim.grid.toggle(row, col);
            }
        }
    }

    if response.drag_started() {
        if let Some(pos) = response.interact_pointer_pos() {
            if let Some((row, col)) = app.camera.pos_to_cell(pos, origin, w, h) {
                // The new state is the opposite of the current cell state
                let old_state = app.sim.grid.get(row, col);
                app.drag_paint_state = Some(!old_state);
                app.sim.grid.toggle(row, col);
            }
        }
    }

    if response.dragged() {
        if let (Some(pos), Some(paint_alive)) =
            (response.interact_pointer_pos(), app.drag_paint_state)
        {
            if let Some((row, col)) = app.camera.pos_to_cell(pos, origin, w, h) {
                app.sim.grid.set(row, col, paint_alive);
            }
        }
    }

    if response.drag_stopped() {
        app.drag_paint_state = None;
    }
}

/// Renders only the cells that intersect `viewport` to the painter.
///
/// Computes the visible row/column range from the viewport rectangle and the
/// canvas `origin` so that off-screen cells are never submitted to the painter,
/// reducing CPU draw-call cost proportionally to the zoom level.
///
/// # Arguments
/// * `app`      — application state (read-only access to grid and camera)
/// * `painter`  — egui painter for the grid canvas
/// * `origin`   — screen-space top-left corner of the grid canvas
/// * `viewport` — screen-space rectangle of the visible scroll-area window
fn paint_cells(app: &GameOfLifeApp, painter: &Painter, origin: Pos2, viewport: egui::Rect) {
    let s = app.camera.cell_size;
    let fill_size = s - CELL_GAP_PX;

    // Project viewport edges into grid coordinates to find the visible range.
    let col_min = ((viewport.min.x - origin.x) / s).floor().max(0.0) as usize;
    let col_max = (((viewport.max.x - origin.x) / s).ceil() as usize).min(app.sim.grid.width);
    let row_min = ((viewport.min.y - origin.y) / s).floor().max(0.0) as usize;
    let row_max = (((viewport.max.y - origin.y) / s).ceil() as usize).min(app.sim.grid.height);

    for row in row_min..row_max {
        for col in col_min..col_max {
            let x = origin.x + col as f32 * s;
            let y = origin.y + row as f32 * s;
            let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(fill_size));
            let color = if app.sim.grid.get(row, col) {
                COLOR_ALIVE
            } else {
                COLOR_DEAD
            };
            painter.rect_filled(rect, 0.0, color);
        }
    }
}
