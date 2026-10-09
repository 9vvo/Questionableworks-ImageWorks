//! ImageWorks desktop application.

mod actions;
mod app;
mod canvas;
mod cli;
mod icons;
mod layers_model;
mod native_menu;
mod panels;
mod renderer;
mod theme;
mod view;

fn window_icon() -> Option<eframe::egui::IconData> {
    let bytes = include_bytes!("../assets/icon-256.png");
    let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes[..]))
        .read_info()
        .ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut rgba).ok()?;
    rgba.truncate(info.buffer_size());
    (info.color_type == png::ColorType::Rgba).then_some(eframe::egui::IconData {
        rgba,
        width: info.width,
        height: info.height,
    })
}

fn main() -> eframe::Result {
    // Command-line use never opens a window, so it works without a display
    // (and is what CI runs).
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
    }
    // Anything left on the command line is a file to open.
    let open: Vec<std::path::PathBuf> = args.iter().map(std::path::PathBuf::from).collect();

    let mut viewport = eframe::egui::ViewportBuilder::default()
        // This first title also names the macOS application menu, so it
        // is just the app name; document names are set later.
        .with_title("ImageWorks")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([800.0, 500.0])
        .with_drag_and_drop(true);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "ImageWorks",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, open)))),
    )
}
