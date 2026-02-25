/// Default cell size in logical pixels.
pub(crate) const DEFAULT_CELL_SIZE: f32 = 10.0;
/// Minimum allowed cell size in logical pixels.
const MIN_CELL_SIZE: f32 = 1.0;
/// Maximum allowed cell size in logical pixels.
pub(crate) const MAX_CELL_SIZE: f32 = 64.0;
/// Multiplicative factor for each keyboard/button zoom step.
pub(crate) const ZOOM_STEP: f32 = 1.2;

/// Encapsulates viewport rendering parameters: zoom level, scroll position,
/// and the last-frame viewport rectangle.
pub(crate) struct Camera {
    /// Display size of each cell in logical pixels (current, animated).
    pub(crate) cell_size: f32,
    /// Target cell size for the smooth-zoom animation.
    ///
    /// `tick_zoom()` lerps `cell_size` toward this value each frame.
    /// Set by `set_zoom_target()` (keyboard/button zoom) or updated by
    /// `apply_zoom()` (Ctrl+scroll / pinch) to keep them in sync.
    target_cell_size: f32,
    /// Viewport-space anchor point for the ongoing zoom animation.
    ///
    /// The pixel at this position stays fixed as `cell_size` changes.
    zoom_anchor: egui::Vec2,
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
            target_cell_size: DEFAULT_CELL_SIZE,
            zoom_anchor: egui::Vec2::new(400.0, 300.0),
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
    /// Also updates `target_cell_size` to `cell_size` so that any pending smooth-zoom
    /// animation is cancelled — direct zoom gestures (Ctrl+scroll, pinch) are immediate.
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
        // Keep target in sync so tick_zoom() does not fight direct gestures.
        self.target_cell_size = new;
    }

    /// Returns the current zoom animation target cell size.
    ///
    /// Used by keyboard/button zoom reset to compute the correct factor relative
    /// to the in-flight target rather than the current animated `cell_size`.
    pub(crate) fn target_cell_size(&self) -> f32 {
        self.target_cell_size
    }

    /// Sets the zoom target for a smooth animation without immediately changing `cell_size`.
    ///
    /// `factor` is multiplied onto `target_cell_size` (clamped to the allowed range).
    /// The animation progresses each frame via `tick_zoom()`.
    ///
    /// # Arguments
    /// * `factor` — multiplicative change to apply to the current target (>1 = in, <1 = out)
    /// * `anchor` — viewport-space anchor that should stay fixed during the animation
    pub(crate) fn set_zoom_target(&mut self, factor: f32, anchor: egui::Vec2) {
        self.target_cell_size =
            (self.target_cell_size * factor).clamp(MIN_CELL_SIZE, MAX_CELL_SIZE);
        self.zoom_anchor = anchor;
    }

    /// Advances the smooth-zoom animation by one frame.
    ///
    /// Lerps `cell_size` 25 % toward `target_cell_size`, adjusting `scroll_offset`
    /// to keep `zoom_anchor` fixed.  Snaps to the target when within 0.1 px.
    ///
    /// Returns `true` while the animation is still in progress (caller should
    /// call `ctx.request_repaint()` in that case), `false` once settled.
    pub(crate) fn tick_zoom(&mut self) -> bool {
        let diff = self.target_cell_size - self.cell_size;
        if diff.abs() < 0.1 {
            if self.cell_size != self.target_cell_size {
                // Snap and do one final scroll adjustment.
                let old = self.cell_size;
                self.cell_size = self.target_cell_size;
                let actual = self.cell_size / old;
                self.scroll_offset =
                    self.zoom_anchor * (actual - 1.0) + self.scroll_offset * actual;
            }
            return false;
        }
        let old = self.cell_size;
        // Lerp 25 % of the remaining distance each frame.
        let new = old + diff * 0.25;
        let actual = new / old;
        self.scroll_offset = self.zoom_anchor * (actual - 1.0) + self.scroll_offset * actual;
        self.cell_size = new;
        true
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tick_zoom_converges() {
        // Set target to 2× default; after 50 ticks the cell_size should be within 0.1 px.
        let mut cam = Camera::new();
        cam.set_zoom_target(2.0, egui::Vec2::ZERO);
        for _ in 0..50 {
            cam.tick_zoom();
        }
        assert!(
            (cam.cell_size - cam.target_cell_size).abs() < 0.1,
            "cell_size {:.3} did not converge to target {:.3}",
            cam.cell_size,
            cam.target_cell_size
        );
    }

    #[test]
    fn test_tick_zoom_no_animation() {
        // When cell_size already equals target, tick_zoom should return false immediately.
        let mut cam = Camera::new();
        let animating = cam.tick_zoom();
        assert!(
            !animating,
            "tick_zoom should return false when already at target"
        );
    }

    #[test]
    fn test_set_zoom_target_clamps() {
        // A very large factor should clamp at MAX_CELL_SIZE (64.0).
        let mut cam = Camera::new();
        cam.set_zoom_target(1_000_000.0, egui::Vec2::ZERO);
        assert_eq!(
            cam.target_cell_size, MAX_CELL_SIZE,
            "target_cell_size should be clamped at MAX_CELL_SIZE"
        );
    }

    #[test]
    fn test_set_zoom_target_clamps_min() {
        // A tiny factor should clamp at MIN_CELL_SIZE (1.0).
        let mut cam = Camera::new();
        cam.set_zoom_target(0.000_001, egui::Vec2::ZERO);
        assert_eq!(
            cam.target_cell_size, MIN_CELL_SIZE,
            "target_cell_size should be clamped at MIN_CELL_SIZE"
        );
    }
}
