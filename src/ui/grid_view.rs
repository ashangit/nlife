use egui::{Color32, Painter, Pos2, Rect, Sense, Stroke, Vec2};

use crate::app::GameOfLifeApp;

/// Gap in logical pixels between adjacent cells (subtracted from cell_size when painting).
const CELL_GAP_PX: f32 = 1.0;
/// Background colour for the grid canvas.
pub(crate) const COLOR_BG: Color32 = Color32::from_gray(30);
/// Fill colour for live cells.
pub(crate) const COLOR_ALIVE: Color32 = Color32::from_rgb(180, 230, 100);
/// Fill colour for dead cells.
const COLOR_DEAD: Color32 = Color32::from_gray(45);
/// Minimum cell size (in logical pixels) at which grid lines are drawn.
const GRID_LINE_MIN_CELL_SIZE: f32 = 4.0;
/// Colour for grid lines.
const COLOR_GRID_LINE: Color32 = Color32::from_gray(60);

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
        (app.sim.width() as f32) * app.camera.cell_size,
        (app.sim.height() as f32) * app.camera.cell_size,
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

    // Optionally draw grid lines when zoomed in enough.
    if app.show_grid_lines && app.camera.cell_size >= GRID_LINE_MIN_CELL_SIZE {
        paint_grid_lines(app, &painter, origin, viewport);
    }

    // Show cell coordinate tooltip on hover.
    if let Some(hover_pos) = response.hover_pos()
        && let Some((row, col)) =
            app.camera
                .pos_to_cell(hover_pos, origin, app.sim.width(), app.sim.height())
    {
        response.on_hover_text_at_pointer(format!("({row}, {col})"));
    }
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
    let (w, h) = (app.sim.width(), app.sim.height());

    // Handle single click (press+release without drag)
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some((row, col)) = app.camera.pos_to_cell(pos, origin, w, h)
    {
        app.sim.toggle(row, col);
    }

    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some((row, col)) = app.camera.pos_to_cell(pos, origin, w, h)
    {
        // The new state is the opposite of the current cell state
        let old_state = app.sim.get(row, col);
        app.drag_paint_state = Some(!old_state);
        app.sim.toggle(row, col);
    }

    if response.dragged()
        && let (Some(pos), Some(paint_alive)) =
            (response.interact_pointer_pos(), app.drag_paint_state)
        && let Some((row, col)) = app.camera.pos_to_cell(pos, origin, w, h)
    {
        app.sim.set(row, col, paint_alive);
    }

    if response.drag_stopped() {
        app.drag_paint_state = None;
    }
}

/// Renders only the cells that intersect `viewport` to the painter.
///
/// **SWAR mode**: iterates every `(row, col)` in the visible range and paints
/// `COLOR_ALIVE` or `COLOR_DEAD` for each cell — O(viewport area).
///
/// **HashLife mode**: fills the visible grid area with `COLOR_DEAD` then calls
/// `sim.live_cells_in_viewport` and paints only those cells `COLOR_ALIVE` —
/// O(live cells in viewport + tree depth).
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
    let col_max = (((viewport.max.x - origin.x) / s).ceil() as usize).min(app.sim.width());
    let row_min = ((viewport.min.y - origin.y) / s).floor().max(0.0) as usize;
    let row_max = (((viewport.max.y - origin.y) / s).ceil() as usize).min(app.sim.height());

    if app.sim.is_hashlife() {
        // HashLife: fill visible grid area with dead colour, then draw sparse live cells.
        let x_start = origin.x + col_min as f32 * s;
        let y_start = origin.y + row_min as f32 * s;
        let x_end = origin.x + col_max as f32 * s;
        let y_end = origin.y + row_max as f32 * s;
        let dead_area = Rect::from_min_max(Pos2::new(x_start, y_start), Pos2::new(x_end, y_end));
        painter.rect_filled(dead_area, 0.0, COLOR_DEAD);

        for (row, col) in app
            .sim
            .live_cells_in_viewport(row_min, col_min, row_max, col_max)
        {
            let x = origin.x + col as f32 * s;
            let y = origin.y + row as f32 * s;
            let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(fill_size));
            painter.rect_filled(rect, 0.0, COLOR_ALIVE);
        }
    } else {
        // SWAR: iterate every visible cell and paint alive or dead.
        for row in row_min..row_max {
            for col in col_min..col_max {
                let x = origin.x + col as f32 * s;
                let y = origin.y + row as f32 * s;
                let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(fill_size));
                let color = if app.sim.get(row, col) {
                    COLOR_ALIVE
                } else {
                    COLOR_DEAD
                };
                painter.rect_filled(rect, 0.0, color);
            }
        }
    }
}

/// Draws hairline grid lines over the visible portion of the canvas.
///
/// Uses the same viewport-cull math as `paint_cells` to avoid emitting
/// off-screen line segments.  Lines are drawn at 0.5 px width so they
/// remain crisp at all zoom levels without eating into cell bodies.
///
/// # Arguments
/// * `app`      — application state (read-only access to grid and camera)
/// * `painter`  — egui painter for the grid canvas
/// * `origin`   — screen-space top-left corner of the grid canvas
/// * `viewport` — screen-space rectangle of the visible scroll-area window
fn paint_grid_lines(app: &GameOfLifeApp, painter: &Painter, origin: Pos2, viewport: Rect) {
    let s = app.camera.cell_size;
    let stroke = Stroke::new(0.5, COLOR_GRID_LINE);

    // Visible column range.
    let col_min = ((viewport.min.x - origin.x) / s).floor().max(0.0) as usize;
    let col_max = (((viewport.max.x - origin.x) / s).ceil() as usize + 1).min(app.sim.width() + 1);
    // Visible row range.
    let row_min = ((viewport.min.y - origin.y) / s).floor().max(0.0) as usize;
    let row_max = (((viewport.max.y - origin.y) / s).ceil() as usize + 1).min(app.sim.height() + 1);

    let x_start = origin.x + col_min as f32 * s;
    let x_end = origin.x + (col_max - 1) as f32 * s;
    let y_start = origin.y + row_min as f32 * s;
    let y_end = origin.y + (row_max - 1) as f32 * s;

    // Horizontal lines — one per row boundary.
    for row in row_min..row_max {
        let y = origin.y + row as f32 * s;
        painter.line_segment([Pos2::new(x_start, y), Pos2::new(x_end, y)], stroke);
    }
    // Vertical lines — one per column boundary.
    for col in col_min..col_max {
        let x = origin.x + col as f32 * s;
        painter.line_segment([Pos2::new(x, y_start), Pos2::new(x, y_end)], stroke);
    }
}
