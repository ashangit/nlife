mod app;
mod camera;
mod grid;
mod input;
mod patterns;
mod simulation;
mod ui;

use app::GameOfLifeApp;

/// Entry point: creates the native window and starts the eframe event loop.
fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Conway's Game of Life")
            .with_inner_size([1050.0, 680.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Conway's Game of Life",
        native_options,
        Box::new(|cc| Ok(Box::new(GameOfLifeApp::new(cc)))),
    )
}
