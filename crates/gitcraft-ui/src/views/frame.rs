//! The window's own title bar and edges, when the app draws them (craft-window decides; see
//! `window.rs`): the title in the middle and the window buttons where the desktop puts them, in
//! its look (Adwaita, Breeze, Windows, generic), or room for the traffic lights on macOS.

use craft_window::policy::{CaptionFamily, Frame};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Id, Pos2, Rect, ResizeDirection, Sense, Stroke,
    StrokeKind, Ui, vec2,
};

use super::Cx;
use crate::app::{Message, WindowAction};
use craft_window::Button;

const HEIGHT: f32 = 36.0;
pub const RADIUS: u8 = 10;
/// The resize handles along the window's edges. Scroll bars sit further in (theme.rs).
const EDGE: f32 = 4.0;

/// The width a window button takes in the bar, gap included.
fn slot(family: CaptionFamily) -> f32 {
    match family {
        CaptionFamily::Windows => 46.0,
        CaptionFamily::Breeze => 30.0,
        CaptionFamily::Adwaita | CaptionFamily::Generic => 34.0,
    }
}

fn window_button(ui: &mut Ui, cx: &Cx, bar: Rect, x: f32, family: CaptionFamily, kind: Button) {
    let c = cx.c();
    let is_max = cx.app.maximized;
    let w = slot(family);
    let centre = Pos2::new(x + w / 2.0, bar.center().y);
    let rect = match family {
        CaptionFamily::Windows => {
            Rect::from_min_max(Pos2::new(x, bar.top()), Pos2::new(x + w, bar.bottom()))
        }
        _ => Rect::from_center_size(centre, vec2(26.0, 26.0)),
    };
    let response = ui.interact(
        rect,
        Id::NULL.with(("window_button", kind as u8)),
        Sense::click(),
    );
    let hovered = response.hovered();
    let close = kind == Button::Close;
    let red = Color32::from_rgb(196, 43, 28);
    let painter = ui.painter();
    let mut ink = c.text;
    match family {
        CaptionFamily::Windows => {
            if hovered {
                painter.rect_filled(
                    rect,
                    CornerRadius::ZERO,
                    if close { red } else { c.hover_strong },
                );
                if close {
                    ink = Color32::WHITE;
                }
            }
        }
        CaptionFamily::Breeze => {
            // Breeze: bare symbols; a filled circle on hover, red for close.
            if hovered {
                painter.circle_filled(centre, 10.0, if close { c.danger } else { c.hover_strong });
                if close {
                    ink = Color32::WHITE;
                }
            }
        }
        CaptionFamily::Adwaita | CaptionFamily::Generic => {
            let fill = match (close, hovered) {
                (true, true) => c.danger,
                (_, true) => c.hover_strong,
                _ => c.panel_2,
            };
            painter.circle_filled(centre, 12.0, fill);
            if close && hovered {
                ink = Color32::WHITE;
            }
        }
    }
    let windows = family == CaptionFamily::Windows;
    let stroke = Stroke::new(if windows { 1.0 } else { 1.5 }, ink);
    let s = if windows { 5.0 } else { 4.5 };
    match kind {
        Button::Minimize => {
            let y = if windows { 0.0 } else { 3.0 };
            painter.line_segment([centre + vec2(-s, y), centre + vec2(s, y)], stroke);
        }
        Button::Maximize if is_max => {
            // Two overlapping squares: restore.
            painter.rect_stroke(
                Rect::from_center_size(centre + vec2(-1.0, 1.0), vec2(8.0, 8.0)),
                1.0,
                stroke,
                StrokeKind::Middle,
            );
            painter.line_segment([centre + vec2(0.0, -4.0), centre + vec2(5.0, -4.0)], stroke);
            painter.line_segment([centre + vec2(5.0, -4.0), centre + vec2(5.0, 2.0)], stroke);
        }
        Button::Maximize => {
            painter.rect_stroke(
                Rect::from_center_size(centre, vec2(2.0 * s, 2.0 * s)),
                1.0,
                stroke,
                StrokeKind::Middle,
            );
        }
        Button::Close => {
            painter.line_segment([centre + vec2(-s, -s), centre + vec2(s, s)], stroke);
            painter.line_segment([centre + vec2(-s, s), centre + vec2(s, -s)], stroke);
        }
    }
    let tip = match kind {
        Button::Minimize => "Minimize",
        Button::Maximize if is_max => "Restore",
        Button::Maximize => "Maximize",
        Button::Close => "Close",
    };
    if response.on_hover_text(crate::i18n::tr(tip)).clicked() {
        cx.send(match kind {
            Button::Minimize => Message::Window(WindowAction::Minimize),
            Button::Maximize => Message::Window(WindowAction::ToggleMaximize),
            Button::Close => Message::Quit,
        });
    }
}

/// The top row of the window, when the app draws it. Dragging the empty part moves the window;
/// a double click does what the system does.
pub fn title_bar(ui: &mut Ui, cx: &Cx) {
    let height = match &cx.app.frame {
        Frame::Native => return,
        // Tall enough that the traffic lights sit centred in it.
        Frame::Inset(inset) => (inset.centre_from_top * 2.0).max(28.0),
        Frame::Integrated(_) => HEIGHT,
    };
    if cx.app.fullscreen {
        return;
    }
    let look = cx.look();
    let c = &look.c;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let corner = if cx.app.rounded() { RADIUS } else { 0 };
    ui.painter().rect_filled(
        rect,
        CornerRadius {
            nw: corner,
            ne: corner,
            sw: 0,
            se: 0,
        },
        c.titlebar,
    );
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, c.border),
    );
    // Before the buttons, so they sit on top and take their own clicks.
    craft_window::title_bar::empty_area(ui, rect);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        cx.app.title(),
        look.ui(13.5),
        c.text,
    );
    if let Frame::Integrated(captions) = &cx.app.frame {
        let w = slot(captions.family);
        let pad = if captions.family == CaptionFamily::Windows {
            0.0
        } else {
            7.0
        };
        for (i, kind) in captions.layout.left.iter().enumerate() {
            let x = rect.left() + pad + i as f32 * w;
            window_button(ui, cx, rect, x, captions.family, *kind);
        }
        let n = captions.layout.right.len() as f32;
        for (i, kind) in captions.layout.right.iter().enumerate() {
            let x = rect.right() - pad - (n - i as f32) * w;
            window_button(ui, cx, rect, x, captions.family, *kind);
        }
    }
}

/// Invisible handles along the window's edges for resizing, and a thin outline, when the app
/// draws the frame.
pub fn window_edges(ui: &mut Ui, cx: &Cx) {
    if !matches!(cx.app.frame, Frame::Integrated(_)) || cx.app.maximized || cx.app.fullscreen {
        return;
    }
    let ctx = ui.ctx().clone();
    let full = ctx.content_rect();
    let (w, h) = (full.width(), full.height());
    let at = |x: f32, y: f32, width: f32, height: f32| {
        Rect::from_min_size(full.min + vec2(x, y), vec2(width, height))
    };
    let corner = EDGE * 2.0;
    use CursorIcon as C;
    use ResizeDirection as D;
    // Edges first and corners last, as what is added later is on top.
    let zones = [
        (
            at(corner, 0.0, w - 2.0 * corner, EDGE),
            D::North,
            C::ResizeNorth,
        ),
        (
            at(corner, h - EDGE, w - 2.0 * corner, EDGE),
            D::South,
            C::ResizeSouth,
        ),
        (
            at(0.0, corner, EDGE, h - 2.0 * corner),
            D::West,
            C::ResizeWest,
        ),
        (
            at(w - EDGE, corner, EDGE, h - 2.0 * corner),
            D::East,
            C::ResizeEast,
        ),
        (
            at(0.0, 0.0, corner, corner),
            D::NorthWest,
            C::ResizeNorthWest,
        ),
        (
            at(w - corner, 0.0, corner, corner),
            D::NorthEast,
            C::ResizeNorthEast,
        ),
        (
            at(0.0, h - corner, corner, corner),
            D::SouthWest,
            C::ResizeSouthWest,
        ),
        (
            at(w - corner, h - corner, corner, corner),
            D::SouthEast,
            C::ResizeSouthEast,
        ),
    ];
    let radius = if cx.app.rounded() { RADIUS } else { 0 };
    egui::Area::new(Id::NULL.with("window_edges"))
        .order(egui::Order::Foreground)
        .fixed_pos(full.min)
        .show(&ctx, |ui| {
            for (index, (rect, direction, cursor)) in zones.into_iter().enumerate() {
                let response =
                    ui.interact(rect, Id::NULL.with(("window_edge", index)), Sense::drag());
                if response.hovered() {
                    ctx.set_cursor_icon(cursor);
                }
                if response.drag_started() {
                    cx.send(Message::Window(WindowAction::Resize(direction)));
                }
            }
            ui.painter().rect_stroke(
                full.shrink(0.5),
                CornerRadius::same(radius),
                Stroke::new(1.0, cx.c().border),
                StrokeKind::Inside,
            );
        });
}
