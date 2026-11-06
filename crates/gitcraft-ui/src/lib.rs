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

/// GNOME on Wayland doesn't draw title bars for other programs, and what the window toolkit
/// draws instead is a flat grey placeholder. There, GitCraft draws a title bar of its own, in
/// the style of GNOME's. `GITCRAFT_SYSTEM_FRAME=1` keeps the toolkit's frame.
fn wants_client_decorations() -> bool {
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    cfg!(target_os = "linux")
        && var("XDG_SESSION_TYPE") == "wayland"
        && var("XDG_CURRENT_DESKTOP")
            .to_ascii_uppercase()
            .contains("GNOME")
        && var("WINIT_UNIX_BACKEND") != "x11"
        && std::env::var_os("GITCRAFT_SYSTEM_FRAME").is_none()
}

pub fn run() -> eframe::Result {
    let repo = std::env::args().nth(1);
    if instance::forward_to_running(repo.as_deref()) {
        return Ok(());
    }
    environment::prepare();
    let saved = settings::Settings::load(&gitcraft_core::store::config_dir());
    let look = theme::Look::for_setting(&saved.theme);
    let client_decorations = wants_client_decorations();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("gitcraft")
            .with_title("GitCraft")
            // The system draws the title bar and border, unless it can't (see below).
            .with_decorations(!client_decorations)
            .with_transparent(client_decorations)
            .with_inner_size([saved.window_width, saved.window_height])
            .with_min_inner_size([640.0, 400.0]),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "GitCraft",
        options,
        Box::new(move |cc| {
            Ok(Box::new(gui::Shell::new(
                cc,
                repo,
                look,
                client_decorations,
            )))
        }),
    )
}
