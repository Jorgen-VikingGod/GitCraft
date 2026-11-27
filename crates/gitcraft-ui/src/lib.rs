//! GitCraft's desktop UI, built with egui. All state changes happen in [`App::update`].

pub mod actions;
pub mod app;
mod appearance;
pub mod command;
mod control;
mod craft;
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
    let decision = window::decide(saved.appearance.system_title_bar);
    let frame = decision.frame.clone();
    let desktop = appearance::Desktop::start();
    let options = eframe::NativeOptions {
        viewport: window::viewport(&frame, settings::old_window_size(&dir)),
        renderer: renderer(std::env::var("GITCRAFT_RENDERER").ok().as_deref()),
        #[cfg(windows)]
        wgpu_options: direct3d(),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "GitCraft",
        options,
        Box::new(move |cc| {
            craft::install(&cc.egui_ctx, decision);
            Ok(Box::new(gui::Shell::new(cc, repo, frame, desktop)))
        }),
    )
}

/// Installs the craft-ui runtime on a context that has no window, as the tests draw the UI: the
/// menus and other craft-ui parts work there as in the window.
#[doc(hidden)]
pub fn install_runtime(ctx: &egui::Context) {
    craft::install(ctx, window::decide(true));
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

/// wgpu would take Vulkan first, and Vulkan on Windows (Intel's drivers above all) can make the
/// whole window flicker while it redraws quickly; Direct3D 12 is what Windows composes itself.
/// `WGPU_BACKEND` still chooses another.
#[cfg(windows)]
fn direct3d() -> eframe::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuSetup, WgpuSetupCreateNew};
    use eframe::wgpu::Backends;
    let mut setup = WgpuSetupCreateNew::without_display_handle();
    if Backends::from_env().is_none() {
        setup.instance_descriptor.backends = Backends::DX12 | Backends::GL;
    }
    eframe::WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(setup),
        ..eframe::NativeOptions::default().wgpu_options
    }
}
