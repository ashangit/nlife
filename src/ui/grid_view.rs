use egui::{Color32, Painter, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};

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
/// Colour for the rectangular selection overlay border.
const COLOR_SELECTION: Color32 = Color32::from_rgba_premultiplied(100, 180, 255, 180);
/// Colour for ghost (paste-preview) cells.
const COLOR_GHOST: Color32 = Color32::from_rgba_premultiplied(180, 230, 100, 120);
/// Filled dash length in logical pixels for the selection border.
const DASH_FILLED_PX: f32 = 6.0;
/// Gap length in logical pixels for the selection border.
const DASH_GAP_PX: f32 = 4.0;

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

    // Draw selection overlay and ghost paste-preview cells.
    paint_selection_overlay(app, &painter, origin);
    paint_ghost_cells(app, &painter, origin);

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

    // Read whether Shift is currently held.
    let shift_held = response.ctx.input(|i| i.modifiers.shift);

    // Update paste anchor to follow the hovered cell when paste mode is active.
    if app.paste_anchor.is_some()
        && let Some(pos) = response.hover_pos()
        && let Some((row, col)) = app.camera.pos_to_cell(pos, origin, w, h)
    {
        app.paste_anchor = Some((row, col));
    }

    // Left-click (no drag) while in paste mode: commit paste at hover cell.
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some((row, col)) = app.camera.pos_to_cell(pos, origin, w, h)
    {
        if app.paste_anchor.is_some() {
            app.commit_paste(row, col);
            app.paste_anchor = None;
        } else if !shift_held {
            app.sim.toggle(row, col);
            app.sim.pattern_name = None;
        }
    }

    // Primary drag start.
    if response.drag_started_by(PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
        && let Some((row, col)) = app.camera.pos_to_cell(pos, origin, w, h)
    {
        if shift_held {
            // Begin rectangular selection; suppress normal paint.
            app.selection_drag_start = Some((row, col));
            app.selection = Some([row, col, row, col]);
            app.drag_paint_state = None;
            app.paste_anchor = None;
        } else {
            // Clear any existing selection and begin paint.
            app.selection = None;
            app.selection_drag_start = None;
            let old_state = app.sim.get(row, col);
            app.drag_paint_state = Some(!old_state);
            app.sim.toggle(row, col);
            app.sim.pattern_name = None;
        }
    }

    // Primary drag in progress.
    if response.dragged_by(PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
        && let Some((row, col)) = app.camera.pos_to_cell(pos, origin, w, h)
    {
        if let Some((sr, sc)) = app.selection_drag_start {
            // Extend selection rectangle.
            app.selection = Some([sr.min(row), sc.min(col), sr.max(row), sc.max(col)]);
        } else if let Some(paint_alive) = app.drag_paint_state {
            app.sim.set(row, col, paint_alive);
            app.sim.pattern_name = None;
        }
    }

    // Primary drag stopped.
    if response.drag_stopped_by(PointerButton::Primary) {
        app.drag_paint_state = None;
        app.selection_drag_start = None;
    }

    // Middle-button pan: record start position.
    if response.drag_started_by(PointerButton::Middle) {
        app.mid_pan_last_pos = response.interact_pointer_pos();
    }

    // Middle-button pan: apply delta each drag event.
    if response.dragged_by(PointerButton::Middle)
        && let (Some(last), Some(current)) = (app.mid_pan_last_pos, response.interact_pointer_pos())
    {
        let delta = current - last;
        app.camera.pan_by(delta);
        app.mid_pan_last_pos = Some(current);
    }

    // Middle-button pan: clear state on release.
    if response.drag_stopped_by(PointerButton::Middle) {
        app.mid_pan_last_pos = None;
    }
}

/// Renders only the cells that intersect `viewport` to the painter.
///
/// Both engines use the same sparse strategy: fill the visible area with
/// `COLOR_DEAD` in a single rect, then paint only the live cells `COLOR_ALIVE`.
/// This reduces the egui draw-call count from O(viewport area) to
/// O(1 + live cells in viewport), which is a significant win at low zoom or
/// low density.
///
/// # Arguments
/// * `app`      — application state (read-only access to grid and camera)
/// * `painter`  — egui painter for the grid canvas
/// * `origin`   — screen-space top-left corner of the grid canvas
/// * `viewport` — screen-space rectangle of the visible scroll-area window
fn paint_cells(app: &GameOfLifeApp, painter: &Painter, origin: Pos2, viewport: egui::Rect) {
    let s = app.camera.cell_size;
    // Drop the gap when s ≤ CELL_GAP_PX so fill_size stays positive at minimum zoom.
    let fill_size = if s > CELL_GAP_PX { s - CELL_GAP_PX } else { s };

    // Project viewport edges into grid coordinates to find the visible range.
    let col_min = ((viewport.min.x - origin.x) / s).floor().max(0.0) as usize;
    let col_max = (((viewport.max.x - origin.x) / s).ceil() as usize).min(app.sim.width());
    let row_min = ((viewport.min.y - origin.y) / s).floor().max(0.0) as usize;
    let row_max = (((viewport.max.y - origin.y) / s).ceil() as usize).min(app.sim.height());

    // Fill the visible area dead in one call, then paint only live cells.
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

/// Draws a dashed rectangular border around `app.selection`, if any.
///
/// Each side is subdivided into alternating filled (`DASH_FILLED_PX`) and
/// gap (`DASH_GAP_PX`) segments drawn with [`Painter::line_segment`].
///
/// # Arguments
/// * `app`     — application state (read-only access to selection and camera)
/// * `painter` — egui painter for the grid canvas
/// * `origin`  — screen-space top-left corner of the grid canvas
fn paint_selection_overlay(app: &GameOfLifeApp, painter: &Painter, origin: Pos2) {
    let [rmin, cmin, rmax, cmax] = match app.selection {
        Some(s) => s,
        None => return,
    };
    let s = app.camera.cell_size;
    let x0 = origin.x + cmin as f32 * s;
    let y0 = origin.y + rmin as f32 * s;
    let x1 = origin.x + (cmax + 1) as f32 * s;
    let y1 = origin.y + (rmax + 1) as f32 * s;

    let stroke = Stroke::new(1.5, COLOR_SELECTION);
    paint_dashed_segment(painter, Pos2::new(x0, y0), Pos2::new(x1, y0), stroke);
    paint_dashed_segment(painter, Pos2::new(x1, y0), Pos2::new(x1, y1), stroke);
    paint_dashed_segment(painter, Pos2::new(x1, y1), Pos2::new(x0, y1), stroke);
    paint_dashed_segment(painter, Pos2::new(x0, y1), Pos2::new(x0, y0), stroke);
}

/// Draws a dashed line from `a` to `b` using alternating filled/gap segments.
fn paint_dashed_segment(painter: &Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    let total = (b - a).length();
    if total < 1e-3 {
        return;
    }
    let dir = (b - a) / total;
    let period = DASH_FILLED_PX + DASH_GAP_PX;
    let mut dist = 0.0f32;
    while dist < total {
        let dash_end = (dist + DASH_FILLED_PX).min(total);
        painter.line_segment([a + dir * dist, a + dir * dash_end], stroke);
        dist += period;
    }
}

/// Draws ghost (semi-transparent) cells showing where the clipboard would be
/// pasted at `app.paste_anchor`.
///
/// # Arguments
/// * `app`     — application state (read-only access to clipboard, paste_anchor, camera)
/// * `painter` — egui painter for the grid canvas
/// * `origin`  — screen-space top-left corner of the grid canvas
fn paint_ghost_cells(app: &GameOfLifeApp, painter: &Painter, origin: Pos2) {
    let (anchor_row, anchor_col) = match app.paste_anchor {
        Some(a) => a,
        None => return,
    };
    if app.clipboard.is_empty() {
        return;
    }
    let s = app.camera.cell_size;
    let fill_size = if s > CELL_GAP_PX { s - CELL_GAP_PX } else { s };
    let (w, h) = (app.sim.width(), app.sim.height());
    for &(dr, dc) in &app.clipboard {
        let r = anchor_row as i64 + dr;
        let c = anchor_col as i64 + dc;
        if r < 0 || c < 0 || r as usize >= h || c as usize >= w {
            continue;
        }
        let x = origin.x + c as f32 * s;
        let y = origin.y + r as f32 * s;
        let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(fill_size));
        painter.rect_filled(rect, 0.0, COLOR_GHOST);
    }
}
