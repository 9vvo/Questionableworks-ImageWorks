//! ImageWorks desktop shell.
//!
//! At M0 this binary is the UI toolkit spike: it exists to prove (or
//! disprove) that the chosen toolkit can do dockable panels, a native menu
//! bar, native file dialogs, HiDPI and pen pressure on macOS and Windows.
//! It exposes no editing features, because none are done yet.

mod cli;
mod native_menu;
mod spike;

fn main() -> eframe::Result {
    // Command-line use never opens a window, so it works without a display
    // (and is what CI runs).
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
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
