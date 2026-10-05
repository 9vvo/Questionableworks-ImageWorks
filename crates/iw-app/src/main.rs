//! ImageWorks desktop shell.
//!
//! At M0 this binary is the UI toolkit spike: it exists to prove (or
//! disprove) that the chosen toolkit can do dockable panels, a native menu
//! bar, native file dialogs, HiDPI and pen pressure on macOS and Windows.
//! It exposes no editing features, because none are done yet.

mod native_menu;
mod spike;

fn main() -> eframe::Result {
    // `--version` is the CI launch check for packaged builds: it must work
    // without a display.
    if std::env::args().any(|a| a == "--version") {
        println!(
            "ImageWorks {} (engine {})",
            env!("CARGO_PKG_VERSION"),
            iw_engine::VERSION
        );
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("ImageWorks (M0 toolkit spike)")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "ImageWorks",
        options,
        Box::new(|cc| Ok(Box::new(spike::SpikeApp::new(cc)))),
    )
}
