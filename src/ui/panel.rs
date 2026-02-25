use egui::{Color32, Rect, Vec2};

use crate::app::GameOfLifeApp;
use crate::camera::{DEFAULT_CELL_SIZE, ZOOM_STEP};

/// Maximum number of simulation steps to run per visual frame.
const MAX_STEPS_PER_FRAME: u32 = 1024;

/// Draws the top control panel with play/pause/step/clear buttons, speed slider,
/// generation counter, zoom controls, and preset pattern buttons.
///
/// # Arguments
/// * `app` — mutable application state
/// * `ctx` — egui context used to show the panel
pub(crate) fn draw_top_panel(app: &mut GameOfLifeApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("controls").show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button("▶ Play").clicked() {
                app.sim.running = true;
            }
            if ui.button("⏸ Pause").clicked() {
                app.sim.running = false;
            }
            if ui.button("⏭ Step").clicked() && !app.sim.running {
                let (t, l) = app.sim.step_once();
                app.camera.apply_expansion(t, l);
            }
            if ui.button("🗑 Clear").clicked() {
                app.sim.clear();
            }
            if ui.button("🎲 Random").clicked() {
                app.sim.fill_random(app.random_density);
            }
            ui.add(
                egui::DragValue::new(&mut app.random_density)
                    .range(1..=100u8)
                    .suffix(" %"),
            );

            ui.separator();
            ui.label("Speed:");
            ui.add(
                egui::Slider::new(&mut app.sim.speed, 1.0..=60.0)
                    .suffix(" gen/s")
                    .step_by(1.0),
            );

            ui.separator();
            ui.label(format!("Gen: {}", app.sim.generation));

            ui.separator();
            let pop = app.sim.grid.live_count();
            ui.label(format!("Pop: {pop}"));
            // Sparkline: 80×18 px canvas showing rolling population history.
            let (sparkline_rect, _) =
                ui.allocate_exact_size(Vec2::new(80.0, 18.0), egui::Sense::hover());
            let painter = ui.painter_at(sparkline_rect);
            painter.rect_filled(sparkline_rect, 2.0, Color32::from_gray(20));
            if app.pop_history.len() > 1 {
                let max_v = *app.pop_history.iter().max().unwrap_or(&1);
                let max_v = max_v.max(1) as f32;
                let n = app.pop_history.len();
                let bar_w = sparkline_rect.width() / n as f32;
                for (i, &v) in app.pop_history.iter().enumerate() {
                    let h = (v as f32 / max_v) * sparkline_rect.height();
                    let x = sparkline_rect.min.x + i as f32 * bar_w;
                    let bar = Rect::from_min_size(
                        egui::Pos2::new(x, sparkline_rect.max.y - h),
                        Vec2::new(bar_w.max(1.0), h.max(1.0)),
                    );
                    painter.rect_filled(bar, 0.0, Color32::from_rgb(100, 200, 80));
                }
            }

            ui.separator();
            let pct = (app.camera.cell_size / DEFAULT_CELL_SIZE * 100.0).round() as u32;
            ui.label(format!("Zoom: {pct}%"));
            let center = app.camera.viewport_rect.size() / 2.0;
            if ui.button("＋").clicked() {
                app.camera.set_zoom_target(ZOOM_STEP, center);
            }
            if ui.button("−").clicked() {
                app.camera.set_zoom_target(1.0 / ZOOM_STEP, center);
            }
            if ui.button("1:1").clicked() {
                app.camera
                    .set_zoom_target(DEFAULT_CELL_SIZE / app.camera.target_cell_size(), center);
            }

            ui.separator();
            // Save current grid as a .cells file chosen by the user.
            if ui.button("💾 Save").clicked() {
                let offsets = app.sim.grid.live_cells_offsets();
                if !offsets.is_empty()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Plaintext cells", &["cells"])
                        .set_file_name("pattern.cells")
                        .save_file()
                {
                    let content = crate::rle::write_cells(&offsets, "pattern");
                    let _ = std::fs::write(&path, content);
                }
            }
            // Load a .cells or .rle file chosen by the user.
            if ui.button("📂 Load").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Pattern files", &["cells", "rle"])
                    .pick_file()
                && let Ok(content) = std::fs::read_to_string(&path)
            {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let cells = if ext == "rle" {
                    crate::rle::parse_rle(&content).ok().map(|p| p.cells)
                } else {
                    crate::rle::parse_cells(&content).ok()
                };
                if let Some(cells) = cells {
                    app.sim.load_cells(&crate::rle::center_cells(cells));
                }
            }

            ui.separator();
            ui.label("Steps/frame:");
            ui.add(
                egui::DragValue::new(&mut app.sim.steps_per_frame)
                    .range(1..=MAX_STEPS_PER_FRAME)
                    .speed(0.5),
            );
        });
    });
}
