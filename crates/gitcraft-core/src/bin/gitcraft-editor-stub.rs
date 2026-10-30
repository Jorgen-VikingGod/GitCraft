//! Stands in for the app binary as git's editor while the core's own tests run.

fn main() {
    gitcraft_core::editor::run_if_requested();
}
