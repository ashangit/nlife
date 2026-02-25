use std::collections::HashMap;

use egui::{Color32, ColorImage, Pos2, Rect, Sense, TextureOptions, Vec2};

use crate::app::{BrowserEntry, GameOfLifeApp};
use crate::library::{Category, decoded_library};
use crate::rle::write_cells;

/// Width of the live-cell preview canvas inside each browser row.
const PREVIEW_SIZE: f32 = 40.0;

/// Draws the left-side pattern browser panel.
///
/// Displays a "Save Pattern…" button, a category `ComboBox`, a name search
/// field, and a scrollable list of all matching entries from the built-in
/// library and from `app.user_patterns`.  Each entry shows a miniature
/// preview and a clickable button that loads the pattern.
///
/// The "Save Pattern…" button opens an inline popup where the user can type a
/// name; confirming writes a `.cells` file to `~/.config/newlife/patterns/`
/// and reloads the user-pattern list.
///
/// # Arguments
/// * `app` — mutable application state (browser filter state + simulation)
/// * `ctx` — egui context used to show the panel
pub(crate) fn draw_pattern_browser(app: &mut GameOfLifeApp, ctx: &egui::Context) {
    egui::SidePanel::left("pattern_browser")
        .resizable(false)
        .default_width(220.0)
        .show(ctx, |ui| {
            // ── Header: title + save button ───────────────────────────────────
            ui.horizontal(|ui| {
                ui.heading("Patterns");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("💾 Save…").clicked() {
                        app.save_popup_open = true;
                        app.save_name.clear();
                    }
                });
            });

            // ── Inline save popup ─────────────────────────────────────────────
            if app.save_popup_open {
                ui.separator();
                ui.label("Save current pattern as:");
                ui.text_edit_singleline(&mut app.save_name);
                ui.horizontal(|ui| {
                    let name_ok = !app.save_name.trim().is_empty();
                    if ui.add_enabled(name_ok, egui::Button::new("OK")).clicked() {
                        save_user_pattern(app);
                    }
                    if ui.button("Cancel").clicked() {
                        app.save_popup_open = false;
                    }
                });
            }

            ui.separator();

            // ── Category filter ───────────────────────────────────────────────
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
                    ui.selectable_value(&mut app.browser_category, Some(Category::Gun), "Gun");
                    ui.selectable_value(
                        &mut app.browser_category,
                        Some(Category::Custom),
                        "Custom",
                    );
                });

            // ── Name search ───────────────────────────────────────────────────
            ui.add_space(4.0);
            ui.label("Search:");
            ui.text_edit_singleline(&mut app.browser_search);

            ui.separator();

            // Rebuild filtered index when filter changes (O(1) most frames).
            let search_lower = app.browser_search.to_ascii_lowercase();
            if app.browser_category != app.browser_entries_cat
                || search_lower != app.browser_entries_search
            {
                app.rebuild_browser_entries();
            }

            // Split into disjoint field borrows so the scroll closure can mutate
            // `preview_textures` while reading `browser_entries` and `user_patterns`.
            let mut to_load: Option<Vec<(i32, i32)>> = None;
            {
                let entries = &app.browser_entries;
                let user_patterns = &app.user_patterns;
                let textures = &mut app.preview_textures;

                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show_rows(ui, PREVIEW_SIZE, entries.len(), |ui, row_range| {
                        for row_idx in row_range {
                            let (name, cells, tex_key): (&str, &[(i32, i32)], String) =
                                match &entries[row_idx] {
                                    BrowserEntry::Library(i) => {
                                        let (e, c) = &decoded_library()[*i];
                                        (e.name, c.as_slice(), e.name.to_owned())
                                    }
                                    BrowserEntry::User(i) => {
                                        let (n, c) = &user_patterns[*i];
                                        (n.as_str(), c.as_slice(), format!("user:{n}"))
                                    }
                                };
                            let tex_id =
                                get_or_create_texture(textures, ui.ctx(), &tex_key, cells).id();
                            ui.horizontal(|ui| {
                                let (rect, _) = ui
                                    .allocate_exact_size(Vec2::splat(PREVIEW_SIZE), Sense::hover());
                                ui.painter_at(rect).image(
                                    tex_id,
                                    rect,
                                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                    Color32::WHITE,
                                );
                                if ui.button(name).clicked() {
                                    to_load = Some(cells.to_vec());
                                }
                            });
                        }
                    });
            }
            if let Some(cells) = to_load {
                app.sim.load_cells(&cells);
            }
        });
}

/// Extracts the current live cells, writes them as a `.cells` file, and
/// reloads the user-pattern list.
///
/// Does nothing if the patterns directory cannot be created.
///
/// # Arguments
/// * `app` — mutable application state; `save_name` is used as the file stem
fn save_user_pattern(app: &mut GameOfLifeApp) {
    let name = app.save_name.trim().to_owned();
    if name.is_empty() {
        return;
    }

    let Some(dir) = GameOfLifeApp::ensure_user_patterns_dir() else {
        return;
    };

    // Collect live cells relative to their bounding box (top-left = 0,0).
    let grid = &app.sim.grid;
    let mut live: Vec<(i32, i32)> = (0..grid.height)
        .flat_map(|r| (0..grid.width).map(move |c| (r, c)))
        .filter(|&(r, c)| grid.get(r, c))
        .map(|(r, c)| (r as i32, c as i32))
        .collect();

    if live.is_empty() {
        app.save_popup_open = false;
        return;
    }

    // Normalise to top-left = (0, 0).
    let row_min = live.iter().map(|&(r, _)| r).min().unwrap();
    let col_min = live.iter().map(|&(_, c)| c).min().unwrap();
    for cell in &mut live {
        cell.0 -= row_min;
        cell.1 -= col_min;
    }

    let content = write_cells(&live, &name);
    let path = format!("{dir}/{name}.cells");
    if std::fs::write(&path, content).is_ok() {
        app.reload_user_patterns();
    }
    app.save_popup_open = false;
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
        Some(Category::Gun) => "Gun",
        Some(Category::Puffer) => "Puffer",
        Some(Category::Wick) => "Wick",
        Some(Category::Custom) => "Custom",
    }
}

/// Rasterises `cells` into a 40×40 `ColorImage` using the same scaling logic
/// as the old painter approach, suitable for uploading to the GPU once.
///
/// # Arguments
/// * `cells` — centred `(row, col)` live-cell coordinates
fn render_preview_image(cells: &[(i32, i32)]) -> ColorImage {
    const SIZE: usize = 40;
    let bg = Color32::from_gray(30);
    let fg = Color32::from_rgb(180, 230, 100);
    let mut pixels = vec![bg; SIZE * SIZE];

    if cells.is_empty() {
        return ColorImage::new([SIZE, SIZE], pixels);
    }
    let row_min = cells.iter().map(|&(r, _)| r).min().unwrap();
    let row_max = cells.iter().map(|&(r, _)| r).max().unwrap();
    let col_min = cells.iter().map(|&(_, c)| c).min().unwrap();
    let col_max = cells.iter().map(|&(_, c)| c).max().unwrap();
    let bbox_rows = (row_max - row_min + 1) as f32;
    let bbox_cols = (col_max - col_min + 1) as f32;
    let pad = 2.0_f32;
    let avail = (SIZE as f32 - 2.0 * pad).max(1.0);
    let cell_size = (avail / bbox_cols).min(avail / bbox_rows).max(1.0);
    let fill_px = (cell_size - 1.0).max(0.5).ceil() as usize;
    let origin_x = pad + (avail - bbox_cols * cell_size) / 2.0;
    let origin_y = pad + (avail - bbox_rows * cell_size) / 2.0;

    for &(r, c) in cells {
        let x0 = (origin_x + (c - col_min) as f32 * cell_size) as usize;
        let y0 = (origin_y + (r - row_min) as f32 * cell_size) as usize;
        for dy in 0..fill_px {
            for dx in 0..fill_px {
                pixels[(y0 + dy).min(SIZE - 1) * SIZE + (x0 + dx).min(SIZE - 1)] = fg;
            }
        }
    }
    ColorImage::new([SIZE, SIZE], pixels)
}

/// Returns the cached `TextureHandle` for `key`, creating and uploading the
/// preview image from `cells` on first call.
///
/// # Arguments
/// * `textures` — mutable map of cached handles, keyed by pattern name
/// * `ctx`      — egui context used for texture upload
/// * `key`      — cache key (pattern name; user patterns prefixed with `"user:"`)
/// * `cells`    — centred `(row, col)` live-cell coordinates used if creating
fn get_or_create_texture<'a>(
    textures: &'a mut HashMap<String, egui::TextureHandle>,
    ctx: &egui::Context,
    key: &str,
    cells: &[(i32, i32)],
) -> &'a egui::TextureHandle {
    textures.entry(key.to_owned()).or_insert_with(|| {
        ctx.load_texture(key, render_preview_image(cells), TextureOptions::NEAREST)
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// render_preview_image must not panic on empty input and must return the correct size
    /// with all pixels set to the background colour.
    #[test]
    fn test_render_preview_image_empty() {
        let img = render_preview_image(&[]);
        assert_eq!(img.size, [40, 40]);
        let bg = Color32::from_gray(30);
        assert!(img.pixels.iter().all(|&p| p == bg));
    }

    /// render_preview_image must not panic for a single cell and must produce at least one
    /// foreground-coloured pixel.
    #[test]
    fn test_render_preview_image_single_cell() {
        let img = render_preview_image(&[(0, 0)]);
        assert_eq!(img.size, [40, 40]);
        let fg = Color32::from_rgb(180, 230, 100);
        assert!(img.pixels.iter().any(|&p| p == fg));
    }

    /// category_label returns the expected string for every variant including Custom.
    #[test]
    fn test_category_label() {
        assert_eq!(category_label(None), "All");
        assert_eq!(category_label(Some(Category::StillLife)), "Still Life");
        assert_eq!(category_label(Some(Category::Oscillator)), "Oscillator");
        assert_eq!(category_label(Some(Category::Spaceship)), "Spaceship");
        assert_eq!(category_label(Some(Category::Methuselah)), "Methuselah");
        assert_eq!(category_label(Some(Category::Gun)), "Gun");
        assert_eq!(category_label(Some(Category::Custom)), "Custom");
    }
}
