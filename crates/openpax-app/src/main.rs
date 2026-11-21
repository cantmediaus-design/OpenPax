#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod clipboard;
mod command_palette;
mod commands;
mod confetti;
mod dialogs;
mod editor;
mod explorer;
mod file_actions;
mod icons;
mod jobs;
mod model_editor;
mod model_panels;
mod pack_icon;
mod panels;
mod recovery;
mod remix_ui;
mod settings;
mod shortcuts_ui;
mod theme;
mod toasts;
mod tour;
mod widgets;

use eframe::egui;

fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenPax")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true),
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
