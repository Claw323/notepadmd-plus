// No console window for release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use notepadmd_plus::app;

fn main() -> eframe::Result {
    let icon = egui::IconData {
        rgba: include_bytes!("../assets/icon_64.rgba").to_vec(),
        width: 64,
        height: 64,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NotepadMD+")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([420.0, 300.0])
            .with_icon(icon)
            .with_drag_and_drop(true),
        persist_window: true, // remembers size/position across runs
        ..Default::default()
    };
    eframe::run_native(
        "NotepadMD+",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
