//! GitCraft's desktop UI, built with egui. All state changes happen in [`App::update`].

pub mod actions;
pub mod app;
pub mod command;
pub mod environment;
pub mod form;
mod gui;
pub mod highlight;
pub mod instance;
pub mod logic;
pub mod menu;
pub mod settings;
pub mod task;
pub mod theme;
pub mod views;

pub use app::{App, Message};

pub fn run() -> eframe::Result {
    let repo = std::env::args().nth(1);
    if instance::forward_to_running(repo.as_deref()) {
        return Ok(());
    }
    environment::prepare();
    let saved = settings::Settings::load(&gitcraft_core::store::config_dir());
    let look = theme::Look::for_setting(&saved.theme);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("gitcraft")
            .with_title("GitCraft")
            // The system draws the title bar and border, whatever the platform offers.
            .with_decorations(true)
            .with_inner_size([saved.window_width, saved.window_height])
            .with_min_inner_size([640.0, 400.0]),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "GitCraft",
        options,
        Box::new(move |cc| Ok(Box::new(gui::Shell::new(cc, repo, look)))),
    )
}
