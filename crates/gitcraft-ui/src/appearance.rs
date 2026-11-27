//! The desktop's dark or light setting, watched through the XDG Settings portal (craft-platform),
//! so a change in the system settings shows at once. Where there is no portal (macOS, Windows)
//! the window library tells instead.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use craft_platform::appearance::{Theme, Watcher};

pub struct Desktop {
    watcher: Option<Watcher>,
    /// The window, once there is one, to wake when the setting changes.
    wake: Arc<OnceLock<egui::Context>>,
    /// The theme last given to the window's own frame: 0 none yet, 1 light, 2 dark.
    told: AtomicU8,
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
        Self {
            watcher,
            wake,
            told: AtomicU8::new(0),
        }
    }

    /// Whether the desktop is set to dark. Once per frame: also keeps the window's own title
    /// bar in step on desktops where the window library doesn't do that.
    pub fn dark(&self, ctx: &egui::Context) -> bool {
        let _ = self.wake.set(ctx.clone());
        let observed = self.watcher.as_ref().and_then(Watcher::current);
        self.tell_window(ctx, observed);
        match observed {
            Some(theme) => theme == Theme::Dark,
            None => ctx.system_theme() == Some(egui::Theme::Dark),
        }
    }
}

impl Desktop {
    /// Where the window library can't see the desktop's setting (the portal knows it, GTK or the
    /// compositor may not), the window's own frame is told, so a system title bar matches.
    fn tell_window(&self, ctx: &egui::Context, observed: Option<Theme>) {
        let Some(theme) = observed else { return };
        let (code, system) = match theme {
            Theme::Dark => (2, egui::SystemTheme::Dark),
            _ => (1, egui::SystemTheme::Light),
        };
        if self.told.swap(code, Ordering::Relaxed) != code {
            ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(system));
        }
    }
}
