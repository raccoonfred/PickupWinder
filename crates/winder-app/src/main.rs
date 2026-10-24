//! Pickup Winder design editor.
//!
//! Edit a coil design on the left, watch the coil wind in 3D (or cut
//! through it) in the middle, read the estimates and export verified G-code
//! on the right.

mod app;
mod chrome;
mod editor;
mod machine;
mod model;
mod view3d;
mod viewcube;
mod views;

fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Pickup Winder")
            .with_inner_size([1360.0, 820.0])
            .with_min_inner_size([900.0, 560.0]),
        // The 3D coil view needs a depth buffer.
        depth_buffer: 24,
        ..Default::default()
    };
    eframe::run_native(
        "Pickup Winder",
        options,
        Box::new(move |cc| Ok(Box::new(app::WinderApp::new(cc, path)))),
    )
}
