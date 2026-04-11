/// Default cell size in logical pixels.
pub(crate) const DEFAULT_CELL_SIZE: f32 = 10.0;
/// Minimum allowed cell size in logical pixels.
const MIN_CELL_SIZE: f32 = 1.0;
/// Maximum allowed cell size in logical pixels.
pub(crate) const MAX_CELL_SIZE: f32 = 64.0;
/// Multiplicative factor for each keyboard/button zoom step.
pub(crate) const ZOOM_STEP: f32 = 1.2;
/// Fraction of the viewport kept as padding on each side during zoom-to-fit.
const FIT_PADDING: f32 = 0.1;

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

    /// Centers the viewport on the live-cell bounding box and adjusts zoom to fit.
    ///
    /// The bounding box is given as `[row_min, col_min, row_max, col_max]` in
    /// grid coordinates (inclusive on all four sides).  The resulting cell size
    /// is clamped to [`MIN_CELL_SIZE`, `MAX_CELL_SIZE`] and any ongoing
    /// smooth-zoom animation is cancelled.
    ///
    /// # Arguments
    /// * `bbox` — `[row_min, col_min, row_max, col_max]` of the live cells
    pub(crate) fn zoom_to_fit(&mut self, bbox: [usize; 4]) {
        let [row_min, col_min, row_max, col_max] = bbox;
        let bbox_w = (col_max - col_min + 1) as f32;
        let bbox_h = (row_max - row_min + 1) as f32;
        let vp = self.viewport_rect.size();

        let usable_w = vp.x * (1.0 - 2.0 * FIT_PADDING);
        let usable_h = vp.y * (1.0 - 2.0 * FIT_PADDING);

        let fit_cell_size = (usable_w / bbox_w).min(usable_h / bbox_h);
        let new_cell_size = fit_cell_size.clamp(MIN_CELL_SIZE, MAX_CELL_SIZE);

        let bbox_center_x = (col_min as f32 + col_max as f32) / 2.0 * new_cell_size;
        let bbox_center_y = (row_min as f32 + row_max as f32) / 2.0 * new_cell_size;

        self.scroll_offset =
            egui::Vec2::new(bbox_center_x - vp.x / 2.0, bbox_center_y - vp.y / 2.0);
        self.cell_size = new_cell_size;
        self.target_cell_size = new_cell_size;
    }

    /// Pans the viewport by a pointer-drag delta in logical pixels.
    ///
    /// Pass the raw pointer displacement directly: when the pointer moves right,
    /// the viewport moves right (content scrolls left), matching natural pan
    /// behaviour.  Internally `scroll_offset` is decremented by `delta` because
    /// `scroll_offset` tracks how far the content has been scrolled.
    ///
    /// # Arguments
    /// * `delta` — pointer movement in logical pixels (positive x = pointer moved right)
    pub(crate) fn pan_by(&mut self, delta: egui::Vec2) {
        self.scroll_offset -= delta;
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
    fn test_pan_by_positive_x() {
        // Pointer moves right (+x): content scrolls left, so scroll_offset.x decreases.
        let mut cam = Camera::new();
        cam.pan_by(egui::Vec2::new(50.0, 0.0));
        assert_eq!(
            cam.scroll_offset,
            egui::Vec2::new(-50.0, 0.0),
            "pan_by positive x (pointer right) should decrement scroll_offset.x"
        );
    }

    #[test]
    fn test_pan_by_negative_y() {
        // Pointer moves up (-y): content scrolls down, so scroll_offset.y increases.
        let mut cam = Camera::new();
        cam.pan_by(egui::Vec2::new(0.0, -30.0));
        assert_eq!(
            cam.scroll_offset,
            egui::Vec2::new(0.0, 30.0),
            "pan_by negative y (pointer up) should increment scroll_offset.y"
        );
    }

    #[test]
    fn test_pan_by_diagonal() {
        let mut cam = Camera::new();
        cam.pan_by(egui::Vec2::new(10.0, 20.0));
        cam.pan_by(egui::Vec2::new(-5.0, 5.0));
        assert_eq!(
            cam.scroll_offset,
            egui::Vec2::new(-5.0, -25.0),
            "successive pan_by calls should accumulate (negated)"
        );
    }

    #[test]
    fn test_pan_by_zero() {
        let mut cam = Camera::new();
        cam.scroll_offset = egui::Vec2::new(100.0, 200.0);
        cam.pan_by(egui::Vec2::ZERO);
        assert_eq!(
            cam.scroll_offset,
            egui::Vec2::new(100.0, 200.0),
            "pan_by zero should leave scroll_offset unchanged"
        );
    }

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

    // ── zoom_to_fit tests ─────────────────────────────────────────────────────
    //
    // These tests reference `Camera::zoom_to_fit` which does not exist yet.
    // They will fail to compile until the method is implemented.

    /// zoom_to_fit centers a 10×10 bbox in an 800×600 viewport at the correct
    /// cell size and scroll offset.
    ///
    /// bbox = [10, 10, 19, 19]: bbox_w=10, bbox_h=10.
    /// usable_w = 800*0.8 = 640, usable_h = 600*0.8 = 480.
    /// fit = min(640/10, 480/10) = min(64, 48) = 48. Clamped → 48.
    /// center_col = 14.5, center_row = 14.5.
    /// canvas_x = 14.5*48 = 696, canvas_y = 14.5*48 = 696.
    /// scroll.x = 696 - 400 = 296, scroll.y = 696 - 300 = 396.
    #[test]
    fn test_zoom_to_fit_centers_bbox() {
        let mut cam = Camera::new(); // viewport 800×600 by default
        cam.zoom_to_fit([10, 10, 19, 19]);
        assert!(
            (cam.cell_size - 48.0).abs() < 0.01,
            "cell_size expected 48.0, got {}",
            cam.cell_size
        );
        assert!(
            (cam.scroll_offset.x - 296.0).abs() < 0.01,
            "scroll_offset.x expected 296.0, got {}",
            cam.scroll_offset.x
        );
        assert!(
            (cam.scroll_offset.y - 396.0).abs() < 0.01,
            "scroll_offset.y expected 396.0, got {}",
            cam.scroll_offset.y
        );
    }

    /// When the bbox is much taller than wide, the width constraint (usable_w /
    /// bbox_w) governs the resulting cell size.
    ///
    /// viewport=100×200, bbox 5 wide × 40 tall.
    /// usable_w=80, usable_h=160. fit_w=80/5=16, fit_h=160/40=4 → cell_size=4.
    #[test]
    fn test_zoom_to_fit_wide_bbox_uses_height_constraint() {
        let mut cam = Camera::new();
        cam.viewport_rect =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(100.0, 200.0));
        // bbox: col 0..4 (5 wide), row 0..39 (40 tall)
        cam.zoom_to_fit([0, 0, 39, 4]);
        assert!(
            (cam.cell_size - 4.0).abs() < 0.01,
            "cell_size expected 4.0 (height constraint), got {}",
            cam.cell_size
        );
    }

    /// When the bbox is much wider than tall, the height constraint governs.
    ///
    /// viewport=200×100, bbox 40 wide × 5 tall.
    /// usable_w=160, usable_h=80. fit_w=160/40=4, fit_h=80/5=16 → cell_size=4.
    #[test]
    fn test_zoom_to_fit_tall_bbox_uses_width_constraint() {
        let mut cam = Camera::new();
        cam.viewport_rect =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(200.0, 100.0));
        // bbox: row 0..4 (5 tall), col 0..39 (40 wide)
        cam.zoom_to_fit([0, 0, 4, 39]);
        assert!(
            (cam.cell_size - 4.0).abs() < 0.01,
            "cell_size expected 4.0 (width constraint), got {}",
            cam.cell_size
        );
    }

    /// For a 1×1 bbox the unclamped cell size would be very large; it must be
    /// clamped at MAX_CELL_SIZE.
    ///
    /// viewport=800×600, bbox=[5,5,5,5].
    /// unclamped = min(640/1, 480/1) = 480 → clamped to 64.
    #[test]
    fn test_zoom_to_fit_clamps_at_max() {
        let mut cam = Camera::new();
        cam.zoom_to_fit([5, 5, 5, 5]);
        assert_eq!(
            cam.cell_size, MAX_CELL_SIZE,
            "cell_size should be clamped at MAX_CELL_SIZE=64 for a 1×1 bbox"
        );
    }

    /// For a huge bbox the unclamped cell size would be tiny; it must be clamped
    /// at MIN_CELL_SIZE.
    ///
    /// viewport=100×100, bbox 10000×10000.
    /// unclamped = 80/10000 = 0.008 → clamped to 1.0.
    #[test]
    fn test_zoom_to_fit_clamps_at_min() {
        let mut cam = Camera::new();
        cam.viewport_rect =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(100.0, 100.0));
        cam.zoom_to_fit([0, 0, 9999, 9999]);
        assert_eq!(
            cam.cell_size, MIN_CELL_SIZE,
            "cell_size should be clamped at MIN_CELL_SIZE=1 for a 10000×10000 bbox"
        );
    }

    /// After zoom_to_fit both cell_size and target_cell_size must agree
    /// (no ongoing smooth-zoom animation).
    #[test]
    fn test_zoom_to_fit_cancels_animation() {
        let mut cam = Camera::new();
        // Start an animation toward 2× zoom.
        cam.set_zoom_target(2.0, egui::Vec2::ZERO);
        // Now request zoom-to-fit; this should snap and cancel any in-flight anim.
        cam.zoom_to_fit([10, 10, 19, 19]);
        assert_eq!(
            cam.cell_size,
            cam.target_cell_size(),
            "zoom_to_fit must cancel smooth-zoom animation (cell_size == target_cell_size)"
        );
    }

    /// zoom_to_fit on a single-cell bbox must not panic.
    #[test]
    fn test_zoom_to_fit_single_cell_no_panic() {
        let mut cam = Camera::new();
        cam.zoom_to_fit([5, 5, 5, 5]); // must not panic
    }
}
