use egui::{Painter, Pos2, Rect, Sense, Vec2};

use crate::app::GameOfLifeApp;
use crate::library::{decoded_library, Category};
use crate::ui::grid_view::{COLOR_ALIVE, COLOR_BG};

/// Width of the live-cell preview canvas inside each browser row.
const PREVIEW_SIZE: f32 = 40.0;

/// Draws the left-side pattern browser panel.
///
/// Displays a category `ComboBox`, a name search field, and a scrollable list
/// of all matching entries from the built-in library.  Each entry shows a
/// miniature preview and a clickable button that loads the pattern.
///
/// # Arguments
/// * `app` — mutable application state (browser filter state + simulation)
/// * `ctx` — egui context used to show the panel
pub(crate) fn draw_pattern_browser(app: &mut GameOfLifeApp, ctx: &egui::Context) {
    egui::SidePanel::left("pattern_browser")
        .resizable(false)
        .default_width(220.0)
        .show(ctx, |ui| {
            ui.heading("Patterns");
            ui.separator();

            // ── Category filter ──────────────────────────────────────────────
            let cat_text = category_label(app.browser_category);
            ui.label("Category:");
            egui::ComboBox::from_id_salt("browser_cat")
                .selected_text(cat_text)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut app.browser_category, None, "All");
                    ui.selectable_value(
                        &mut app.browser_category,
                        Some(Category::StillLife),
                        "Still Life",
                    );
                    ui.selectable_value(
                        &mut app.browser_category,
                        Some(Category::Oscillator),
                        "Oscillator",
                    );
                    ui.selectable_value(
                        &mut app.browser_category,
                        Some(Category::Spaceship),
                        "Spaceship",
                    );
                    ui.selectable_value(
                        &mut app.browser_category,
                        Some(Category::Methuselah),
                        "Methuselah",
                    );
                });

            // ── Name search ──────────────────────────────────────────────────
            ui.add_space(4.0);
            ui.label("Search:");
            ui.text_edit_singleline(&mut app.browser_search);

            ui.separator();

            // Snapshot filter fields before entering the scroll closure to
            // satisfy the borrow checker (avoids simultaneous &mut + & on app).
            let search_lower = app.browser_search.to_ascii_lowercase();
            let filter_cat = app.browser_category;

            // ── Scrollable pattern list ──────────────────────────────────────
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for (entry, cells) in decoded_library() {
                        // Category filter
                        if let Some(cat) = filter_cat {
                            if entry.category != cat {
                                continue;
                            }
                        }
                        // Name search filter
                        if !search_lower.is_empty()
                            && !entry.name.to_ascii_lowercase().contains(&search_lower)
                        {
                            continue;
                        }

                        // Row: [preview 40×40] [name button]
                        ui.horizontal(|ui| {
                            let (preview_rect, _) =
                                ui.allocate_exact_size(Vec2::splat(PREVIEW_SIZE), Sense::hover());
                            draw_preview(&ui.painter_at(preview_rect), preview_rect, cells);

                            if ui.button(entry.name).clicked() {
                                app.sim.load_cells(cells);
                            }
                        });
                    }
                });
        });
}

/// Returns the human-readable label for a browser category selection.
///
/// # Arguments
/// * `cat` — `None` means "All"; `Some(c)` returns the category name
fn category_label(cat: Option<Category>) -> &'static str {
    match cat {
        None => "All",
        Some(Category::StillLife) => "Still Life",
        Some(Category::Oscillator) => "Oscillator",
        Some(Category::Spaceship) => "Spaceship",
        Some(Category::Methuselah) => "Methuselah",
    }
}

/// Renders a miniature Conway's Game of Life pattern into `rect`.
///
/// Fills the background with [`COLOR_BG`], then scales the cell bounding box
/// to fit inside `rect` (with 2 px padding) and draws each live cell as a
/// filled rectangle using [`COLOR_ALIVE`].  Does nothing except fill the
/// background when `cells` is empty.
///
/// # Arguments
/// * `painter` — egui painter clipped to `rect`
/// * `rect`    — screen-space rectangle to draw into
/// * `cells`   — centred `(row, col)` live-cell coordinates
pub(crate) fn draw_preview(painter: &Painter, rect: Rect, cells: &[(i32, i32)]) {
    painter.rect_filled(rect, 2.0, COLOR_BG);

    if cells.is_empty() {
        return;
    }

    let row_min = cells.iter().map(|&(r, _)| r).min().unwrap();
    let row_max = cells.iter().map(|&(r, _)| r).max().unwrap();
    let col_min = cells.iter().map(|&(_, c)| c).min().unwrap();
    let col_max = cells.iter().map(|&(_, c)| c).max().unwrap();

    let bbox_rows = (row_max - row_min + 1) as f32;
    let bbox_cols = (col_max - col_min + 1) as f32;

    let pad = 2.0_f32;
    let avail_w = (rect.width() - 2.0 * pad).max(1.0);
    let avail_h = (rect.height() - 2.0 * pad).max(1.0);

    let cell_size = (avail_w / bbox_cols).min(avail_h / bbox_rows).max(1.0);
    let fill = (cell_size - 1.0).max(0.5);

    // Centre the scaled pattern within the available area.
    let origin_x = rect.min.x + pad + (avail_w - bbox_cols * cell_size) / 2.0;
    let origin_y = rect.min.y + pad + (avail_h - bbox_rows * cell_size) / 2.0;

    for &(r, c) in cells {
        let x = origin_x + (c - col_min) as f32 * cell_size;
        let y = origin_y + (r - row_min) as f32 * cell_size;
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(x, y), Vec2::splat(fill)),
            0.0,
            COLOR_ALIVE,
        );
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// draw_preview must not panic when given an empty cell list.
    ///
    /// Creates a no-op painter (via a dummy egui context in headless mode)
    /// and verifies the function returns cleanly.
    #[test]
    fn test_draw_preview_empty_cells() {
        // Construct a minimal egui context with no rendering backend.
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());

        // Build a painter by allocating a layer and a painter from it.
        let layer_id = egui::LayerId::new(egui::Order::Background, egui::Id::new("test"));
        let painter = egui::Painter::new(ctx.clone(), layer_id, Rect::EVERYTHING);

        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(40.0));

        // Must not panic on empty cell list.
        draw_preview(&painter, rect, &[]);

        // Also test with a single cell.
        draw_preview(&painter, rect, &[(0, 0)]);

        let _ = ctx.end_pass();
    }

    /// category_label returns the expected string for every variant.
    #[test]
    fn test_category_label() {
        assert_eq!(category_label(None), "All");
        assert_eq!(category_label(Some(Category::StillLife)), "Still Life");
        assert_eq!(category_label(Some(Category::Oscillator)), "Oscillator");
        assert_eq!(category_label(Some(Category::Spaceship)), "Spaceship");
        assert_eq!(category_label(Some(Category::Methuselah)), "Methuselah");
    }
}
