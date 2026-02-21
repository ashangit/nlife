use crate::app::GameOfLifeApp;
use crate::camera::{DEFAULT_CELL_SIZE, ZOOM_STEP};
use crate::patterns::Pattern;

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
            let pct = (app.camera.cell_size / DEFAULT_CELL_SIZE * 100.0).round() as u32;
            ui.label(format!("Zoom: {pct}%"));
            let center = app.camera.viewport_rect.size() / 2.0;
            if ui.button("＋").clicked() {
                app.camera.apply_zoom(ZOOM_STEP, center);
            }
            if ui.button("−").clicked() {
                app.camera.apply_zoom(1.0 / ZOOM_STEP, center);
            }
            if ui.button("1:1").clicked() {
                app.camera
                    .apply_zoom(DEFAULT_CELL_SIZE / app.camera.cell_size, center);
            }

            ui.separator();
            ui.label("Steps/frame:");
            ui.add(
                egui::DragValue::new(&mut app.sim.steps_per_frame)
                    .range(1..=MAX_STEPS_PER_FRAME)
                    .speed(0.5),
            );
        });

        // Still Lifes
        ui.horizontal(|ui| {
            ui.label("Still Lifes:");
            if ui.button("Block").clicked() {
                app.sim.load_pattern(Pattern::Block);
            }
            if ui.button("Beehive").clicked() {
                app.sim.load_pattern(Pattern::Beehive);
            }
            if ui.button("Loaf").clicked() {
                app.sim.load_pattern(Pattern::Loaf);
            }
            if ui.button("Boat").clicked() {
                app.sim.load_pattern(Pattern::Boat);
            }
        });

        // Oscillators
        ui.horizontal(|ui| {
            ui.label("Oscillators:");
            if ui.button("Blinker (p2)").clicked() {
                app.sim.load_pattern(Pattern::Blinker);
            }
            if ui.button("Toad (p2)").clicked() {
                app.sim.load_pattern(Pattern::Toad);
            }
            if ui.button("Beacon (p2)").clicked() {
                app.sim.load_pattern(Pattern::Beacon);
            }
            if ui.button("Pulsar (p3)").clicked() {
                app.sim.load_pattern(Pattern::Pulsar);
            }
            if ui.button("Pentadecathlon (p15)").clicked() {
                app.sim.load_pattern(Pattern::Pentadecathlon);
            }
        });

        // Spaceships
        ui.horizontal(|ui| {
            ui.label("Spaceships:");
            if ui.button("Glider").clicked() {
                app.sim.load_pattern(Pattern::Glider);
            }
            if ui.button("LWSS").clicked() {
                app.sim.load_pattern(Pattern::Lwss);
            }
            if ui.button("MWSS").clicked() {
                app.sim.load_pattern(Pattern::Mwss);
            }
            if ui.button("HWSS").clicked() {
                app.sim.load_pattern(Pattern::Hwss);
            }
        });

        // Methuselahs
        ui.horizontal(|ui| {
            ui.label("Methuselahs:");
            if ui.button("R-Pentomino").clicked() {
                app.sim.load_pattern(Pattern::RPentomino);
            }
            if ui.button("Acorn").clicked() {
                app.sim.load_pattern(Pattern::Acorn);
            }
            if ui.button("Diehard").clicked() {
                app.sim.load_pattern(Pattern::Diehard);
            }
        });
    });
}
