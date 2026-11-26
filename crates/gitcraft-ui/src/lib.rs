//! GitCraft's desktop UI, built with egui. All state changes happen in [`App::update`].

pub mod actions;
pub mod app;
mod appearance;
pub mod command;
pub mod environment;
pub mod form;
mod gui;
pub mod highlight;
pub mod i18n;
pub mod instance;
pub mod logic;
pub mod menu;
pub mod settings;
pub mod task;
pub mod theme;
pub mod views;
pub mod window;

pub use app::{App, Message};

pub fn run() -> eframe::Result {
    let repo = std::env::args().nth(1);
    if instance::forward_to_running(repo.as_deref()) {
        return Ok(());
    }
    environment::prepare();
    let dir = gitcraft_core::store::config_dir();
    let saved = settings::Settings::load(&dir);
    i18n::set_language(&saved.general.language);
    gitcraft_core::runner::set_git_program(Some(std::path::PathBuf::from(
        &saved.general.git_program,
    )));
    let frame = window::decide(saved.appearance.system_title_bar);
    let desktop = appearance::Desktop::start();
    let options = eframe::NativeOptions {
        viewport: window::viewport(&frame, settings::old_window_size(&dir)),
        renderer: renderer(std::env::var("GITCRAFT_RENDERER").ok().as_deref()),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "GitCraft",
        options,
        Box::new(move |cc| Ok(Box::new(gui::Shell::new(cc, repo, frame, desktop)))),
    )
}

/// wgpu on Windows, OpenGL elsewhere; `GITCRAFT_RENDERER=glow` asks for OpenGL on Windows too,
/// for a machine whose Direct3D or wgpu misbehaves.
fn renderer(asked: Option<&str>) -> eframe::Renderer {
    #[cfg(windows)]
    if asked != Some("glow") {
        return eframe::Renderer::Wgpu;
    }
    let _ = asked;
    eframe::Renderer::Glow
}
