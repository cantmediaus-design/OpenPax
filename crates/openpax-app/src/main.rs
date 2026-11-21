#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod dialogs;
mod editor;
mod explorer;
mod model_editor;
mod model_panels;
mod panels;

use eframe::egui;

fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenPax")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    // `openpax <pack folder> [asset path...]` opens that pack (and those assets) on startup.
    let mut args = std::env::args_os().skip(1);
    let initial_pack = args.next().map(std::path::PathBuf::from);
    let initial_assets = args.map(|a| a.to_string_lossy().into_owned()).collect();
    eframe::run_native(
        "OpenPax",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, initial_pack, initial_assets)))),
    )
}
