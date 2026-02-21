/// Default cell size in logical pixels.
pub(crate) const DEFAULT_CELL_SIZE: f32 = 10.0;
/// Minimum allowed cell size in logical pixels.
const MIN_CELL_SIZE: f32 = 2.0;
/// Maximum allowed cell size in logical pixels.
const MAX_CELL_SIZE: f32 = 64.0;
/// Multiplicative factor for each keyboard/button zoom step.
pub(crate) const ZOOM_STEP: f32 = 1.2;

/// Encapsulates viewport rendering parameters: zoom level, scroll position,
/// and the last-frame viewport rectangle.
pub(crate) struct Camera {
    /// Display size of each cell in logical pixels.
    pub(crate) cell_size: f32,
    /// Current scroll position of the grid viewport in logical pixels.
    ///
    /// Adjusted after each `expand_if_needed` call so the visible region stays
    /// centred on the same cells even when the grid grows at the top or left.
    pub(crate) scroll_offset: egui::Vec2,
    /// Last-frame viewport rectangle from the ScrollArea (screen coordinates).
    /// Used to convert mouse hover position into viewport-relative zoom anchor.
    pub(crate) viewport_rect: egui::Rect,
}

impl Camera {
    /// Creates a Camera with default cell size and zero scroll offset.
    pub(crate) fn new() -> Self {
        Self {
            cell_size: DEFAULT_CELL_SIZE,
            scroll_offset: egui::Vec2::ZERO,
            viewport_rect: egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(800.0, 600.0),
            ),
        }
    }

    /// Scales `cell_size` by `factor` (clamped to [`MIN_CELL_SIZE`, `MAX_CELL_SIZE`]),
    /// adjusting `scroll_offset` so the pixel at `anchor` (viewport coordinates) stays fixed.
    ///
    /// # Arguments
    /// * `factor` — multiplicative zoom change (>1 = zoom in, <1 = zoom out)
    /// * `anchor` — position in viewport coordinates to zoom towards
    pub(crate) fn apply_zoom(&mut self, factor: f32, anchor: egui::Vec2) {
        let old = self.cell_size;
        let new = (old * factor).clamp(MIN_CELL_SIZE, MAX_CELL_SIZE);
        let actual = new / old;
        self.scroll_offset = anchor * (actual - 1.0) + self.scroll_offset * actual;
        self.cell_size = new;
    }

    /// Adjusts `scroll_offset` to compensate for grid rows/cols prepended at top/left.
    ///
    /// Called after `expand_if_needed` so the viewport stays centred on the same region
    /// even when new dead rows/columns are prepended.
    ///
    /// # Arguments
    /// * `add_top`  — number of dead rows added above the existing content
    /// * `add_left` — number of dead columns added to the left of the existing content
    pub(crate) fn apply_expansion(&mut self, add_top: usize, add_left: usize) {
        self.scroll_offset.y += add_top as f32 * self.cell_size;
        self.scroll_offset.x += add_left as f32 * self.cell_size;
    }

    /// Converts a canvas position to `(row, col)` grid coordinates.
    ///
    /// Returns `None` if the position is outside the grid bounds.
    ///
    /// # Arguments
    /// * `pos`    — screen-space position to convert
    /// * `origin` — screen-space top-left corner of the grid canvas
    /// * `width`  — grid width in columns
    /// * `height` — grid height in rows
    pub(crate) fn pos_to_cell(
        &self,
        pos: egui::Pos2,
        origin: egui::Pos2,
        width: usize,
        height: usize,
    ) -> Option<(usize, usize)> {
        let rel = pos - origin;
        if rel.x < 0.0 || rel.y < 0.0 {
            return None;
        }
        let col = (rel.x / self.cell_size) as usize;
        let row = (rel.y / self.cell_size) as usize;
        if col < width && row < height {
            Some((row, col))
        } else {
            None
        }
    }
}
