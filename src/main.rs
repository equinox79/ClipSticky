#![windows_subsystem = "windows"]

mod app;
mod clipboard;
mod core;
mod fonts;
mod menu;
mod platform;

use eframe::egui;

use crate::core::settings::{DEFAULT_SIZE, MIN_SIZE, Settings};

fn main() -> eframe::Result {
    let settings = Settings::load();
    let size = settings.size.unwrap_or(DEFAULT_SIZE);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("ClipSticky")
        .with_inner_size([size.0, size.1])
        .with_min_inner_size([MIN_SIZE.0, MIN_SIZE.1])
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_taskbar(false)
        .with_active(false)
        .with_resizable(true);
    if let Some((x, y)) = settings.pos {
        viewport = viewport.with_position([x, y]);
    }

    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    platform::configure(&mut options);

    eframe::run_native(
        "ClipSticky",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, settings)))),
    )
}
