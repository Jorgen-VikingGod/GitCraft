#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> iced::Result {
    // Must stay first: git starts this executable as its editor during rebases.
    gitcraft_core::editor::run_if_requested();
    gitcraft_ui::run()
}
