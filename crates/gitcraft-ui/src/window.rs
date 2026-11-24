//! The window itself, decided by craft-window so that GitCraft looks like the other Craft apps on
//! each desktop:
//!
//! - Who draws the title bar and edges. GNOME, KDE, Windows and most other floating desktops: the
//!   app, with window buttons in the desktop's look and order (Adwaita, Breeze, Windows,
//!   generic). macOS: the system's traffic lights over the app's title bar. Tiling compositors
//!   and window managers (Sway, Hyprland, i3, …): the system's frame.
//!   *View › Use system title bar* gives the frame to the system everywhere (after a restart).
//!   `GITCRAFT_TITLEBAR=app` or `=system` (or `CRAFT_TITLEBAR`) wins over that setting; the older
//!   `GITCRAFT_SYSTEM_FRAME=1` still asks for the system's frame.
//! - Where the window opens: the size, position and maximized state of the last run, kept in
//!   `window.json` in the config folder. Wayland doesn't tell windows where they are, so there
//!   only the size and maximized state come back.

use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use craft_window::policy::{Backend, Frame, TitleBarPref};
use craft_window::restore::{Defaults, MAIN, MonitorKey, Placement, Store, place};
use craft_window::{Button, Layout, Prefix};

pub const SIZE: [f32; 2] = [1280.0, 800.0];
pub const MIN: [f32; 2] = [640.0, 400.0];

/// Decide the frame once, before the window is created.
pub fn decide(system_title_bar: bool) -> Frame {
    let legacy = std::env::var_os("GITCRAFT_SYSTEM_FRAME").is_some();
    let pref = if system_title_bar || legacy {
        TitleBarPref::System
    } else {
        TitleBarPref::Integrated
    };
    let mut frame =
        craft_window::policy::decide_for(Prefix::new("GITCRAFT"), Backend::from_env(), pref).frame;
    if let Frame::Integrated(captions) = &mut frame {
        captions.layout = all_buttons(&captions.layout);
    }
    frame
}

/// The desktop's button order, with minimize and maximize added where the desktop leaves them
/// out (GNOME shows only close by default): next to close, on its side, so the order reads
/// minimize, maximize, close from the inside out.
fn all_buttons(layout: &Layout) -> Layout {
    let mut out = layout.clone();
    let on_left = out.left.contains(&Button::Close) && !out.right.contains(&Button::Close);
    for button in [Button::Minimize, Button::Maximize] {
        if out.left.contains(&button) || out.right.contains(&button) {
            continue;
        }
        let side = if on_left {
            &mut out.left
        } else {
            &mut out.right
        };
        let close = side.iter().position(|b| *b == Button::Close);
        match (on_left, close) {
            // Close on the left is outermost, so the others go to its right.
            (true, Some(i)) => side.insert(i + 1, button),
            (false, Some(i)) => side.insert(i, button),
            (_, None) => side.push(button),
        }
    }
    out
}

/// Whether the app paints round window corners on a see-through window: only Linux desktops
/// leave the corners to the app.
pub fn rounded_for(frame: &Frame) -> bool {
    cfg!(not(any(target_os = "windows", target_os = "macos")))
        && matches!(frame, Frame::Integrated(_))
}

/// The window to create: the frame's decorations and the size and place of the last run.
pub fn viewport(frame: &Frame) -> egui::ViewportBuilder {
    let viewport = egui::ViewportBuilder::default()
        .with_app_id("gitcraft")
        .with_title("GitCraft")
        .with_inner_size(SIZE)
        .with_min_inner_size(MIN);
    let viewport =
        craft_window::policy::apply(viewport, frame).with_transparent(rounded_for(frame));
    restored(viewport)
}

// ---------------------------------------------------------------- placement

/// Wait this long after the last change before writing the file.
const SAVE_AFTER: Duration = Duration::from_secs(2);

struct Restore {
    store: Store,
    no_position: bool,
    changed: Option<Instant>,
}

fn restore() -> &'static Mutex<Restore> {
    static RESTORE: OnceLock<Mutex<Restore>> = OnceLock::new();
    RESTORE.get_or_init(|| {
        let file = gitcraft_core::store::config_dir().join(craft_window::restore::FILE_NAME);
        let store = Store::at(&file);
        if let Some(moved) = store.moved_aside() {
            eprintln!(
                "The window file was damaged and was moved to {}.",
                moved.display()
            );
        }
        Mutex::new(Restore {
            store,
            no_position: Backend::from_env() == Backend::Wayland,
            changed: None,
        })
    })
}

fn lock() -> MutexGuard<'static, Restore> {
    restore().lock().unwrap_or_else(|error| error.into_inner())
}

/// The window as it was last time. The monitors aren't known before the window exists, so the
/// saved geometry is applied as it is (raised to the smallest size) and the system keeps the
/// window on screen.
fn restored(viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    let restore = lock();
    let Some(saved) = restore.store.get(MAIN) else {
        return viewport;
    };
    let defaults = Defaults::new(SIZE, MIN).with_no_position(restore.no_position);
    let placed = place(Some(&saved), &[], None, &defaults);
    let mut viewport = viewport
        .with_inner_size(placed.normal.size)
        .with_maximized(placed.maximized);
    if let Some([x, y]) = placed.position_px {
        viewport = viewport.with_position([x as f32 / placed.scale, y as f32 / placed.scale]);
    }
    viewport
}

/// Once per frame: note the window's size and place, and write them a moment after they stop
/// changing.
pub fn track(ctx: &egui::Context) {
    let info = ctx.input(|input| input.viewport().clone());
    let zoom = ctx.zoom_factor();
    let mut restore = lock();
    if info.minimized == Some(true) {
        return save_when_due(&mut restore);
    }
    let scale = info.native_pixels_per_point.unwrap_or(1.0);
    let maximized = info.maximized.unwrap_or(false);
    let fullscreen = info.fullscreen.unwrap_or(false);
    let mut placement = restore
        .store
        .get(MAIN)
        .unwrap_or_else(|| Placement::new(SIZE, scale));
    let before = placement.clone();
    if !maximized && !fullscreen {
        if let Some(inner) = info.inner_rect {
            placement.normal.size = [inner.width() * zoom, inner.height() * zoom];
        }
        placement.position_px = if restore.no_position {
            None
        } else {
            info.outer_rect.map(|outer| {
                [
                    (outer.min.x * scale).round() as i32,
                    (outer.min.y * scale).round() as i32,
                ]
            })
        };
        placement.normal.pos = placement
            .position_px
            .map_or([0.0, 0.0], |[x, y]| [x as f32 / scale, y as f32 / scale]);
        placement.scale = scale;
    }
    placement.zoom = zoom;
    placement.maximized = maximized;
    placement.fullscreen = fullscreen;
    if let Some(size) = info.monitor_size {
        placement.monitor = Some(MonitorKey::new(
            "",
            [
                (size.x * scale).round() as u32,
                (size.y * scale).round() as u32,
            ],
        ));
    }
    let sane = placement
        .normal
        .size
        .iter()
        .all(|v| v.is_finite() && *v > 0.0);
    if placement != before && sane {
        restore.store.set(MAIN, &placement);
        restore.changed = Some(Instant::now());
    }
    save_when_due(&mut restore);
    if restore.changed.is_some() {
        ctx.request_repaint_after(SAVE_AFTER);
    }
}

fn save_when_due(restore: &mut Restore) {
    if restore
        .changed
        .is_some_and(|changed| changed.elapsed() >= SAVE_AFTER)
    {
        save_now(restore);
    }
}

fn save_now(restore: &mut Restore) {
    restore.changed = None;
    if let Err(error) = restore.store.save() {
        eprintln!("Couldn't save the window's size and place: {error}");
    }
}

/// When the program ends: write what hasn't been written yet.
pub fn save() {
    let mut restore = lock();
    if restore.store.is_dirty() {
        save_now(&mut restore);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Button::{Close, Maximize, Minimize};

    fn layout(left: &[Button], right: &[Button]) -> Layout {
        Layout {
            left: left.to_vec(),
            right: right.to_vec(),
        }
    }

    #[test]
    fn missing_buttons_are_added_next_to_close() {
        // GNOME's default `appmenu:close`.
        assert_eq!(
            all_buttons(&layout(&[], &[Close])),
            layout(&[], &[Minimize, Maximize, Close])
        );
        // `close:` on the left.
        assert_eq!(
            all_buttons(&layout(&[Close], &[])),
            layout(&[Close, Maximize, Minimize], &[])
        );
        // A full layout stays as the desktop has it.
        assert_eq!(
            all_buttons(&layout(&[], &[Minimize, Maximize, Close])),
            layout(&[], &[Minimize, Maximize, Close])
        );
        assert_eq!(
            all_buttons(&layout(&[], &[Minimize, Close])),
            layout(&[], &[Minimize, Maximize, Close])
        );
    }
}
