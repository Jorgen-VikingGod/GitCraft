//! The desktop's dark or light setting, watched through the XDG Settings portal (craft-platform),
//! so a change in the system settings shows at once. Where there is no portal (macOS, Windows)
//! the window library tells instead.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use craft_platform::appearance::{Theme, Watcher, sync_native_window_theme};

pub struct Desktop {
    watcher: Option<Watcher>,
    /// The window, once there is one, to wake when the setting changes.
    wake: Arc<OnceLock<egui::Context>>,
}

impl Desktop {
    /// Starts watching, and waits a moment for the first answer so the first frame already has
    /// the desktop's look.
    pub fn start() -> Self {
        let wake = Arc::new(OnceLock::<egui::Context>::new());
        let target = Arc::clone(&wake);
        let watcher = Watcher::start(move || {
            if let Some(ctx) = target.get() {
                ctx.request_repaint();
            }
        });
        if let Some(watcher) = &watcher {
            watcher.wait_first(Duration::from_millis(250));
        }
        Self { watcher, wake }
    }

    /// Whether the desktop is set to dark. Once per frame: also keeps the window's own title
    /// bar in step on desktops where the window library doesn't do that.
    pub fn dark(&self, ctx: &egui::Context) -> bool {
        let _ = self.wake.set(ctx.clone());
        let observed = self.watcher.as_ref().and_then(Watcher::current);
        sync_native_window_theme(ctx, observed);
        match observed {
            Some(theme) => theme == Theme::Dark,
            None => ctx.system_theme() == Some(egui::Theme::Dark),
        }
    }
}
