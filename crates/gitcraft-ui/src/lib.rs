//! GitCraft's desktop UI, built with iced. All state changes happen in [`App::update`].

pub mod actions;
pub mod app;
pub mod command;
pub mod environment;
pub mod form;
pub mod highlight;
pub mod instance;
pub mod logic;
pub mod menu;
pub mod settings;
pub mod theme;
mod views;

pub use app::{App, Message};

pub fn run() -> iced::Result {
    let repo = std::env::args().nth(1);
    if instance::forward_to_running(repo.as_deref()) {
        return Ok(());
    }
    environment::prepare();
    let saved = settings::Settings::load(&gitcraft_core::store::config_dir());
    let look = theme::Look::for_setting(&saved.theme);
    let font = look.font;
    iced::application(
        move || App::boot(repo.clone(), look.clone()),
        App::update,
        App::view,
    )
    .title(App::title)
    .theme(App::theme)
    .style(App::style)
    .subscription(App::subscription)
    .scale_factor(App::scale)
    .default_font(font)
    .exit_on_close_request(false)
    .window_size(iced::Size::new(saved.window_width, saved.window_height))
    .run()
}
