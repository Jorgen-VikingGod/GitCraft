//! The window's own title bar and edges, for desktops that don't draw them (GNOME on Wayland).
//! The look follows GNOME's: a header bar with the title in the middle and round buttons.

use egui::{
    Align2, CornerRadius, CursorIcon, Id, Pos2, Rect, ResizeDirection, Sense, Stroke, StrokeKind,
    Ui, vec2,
};

use super::Cx;
use super::widgets::{paint_strong, region, tool_text};
use crate::actions::MenuRequest;
use crate::app::{Message, WindowAction};
use crate::theme::mix;

const HEIGHT: f32 = 38.0;
const RADIUS: u8 = 10;
const EDGE: f32 = 6.0;

#[derive(Clone, Copy)]
enum Button {
    Minimize,
    Maximize,
    Close,
}

fn window_button(ui: &mut Ui, cx: &Cx, centre: Pos2, kind: Button) {
    let c = cx.c();
    let rect = Rect::from_center_size(centre, vec2(26.0, 26.0));
    let response = ui.interact(
        rect,
        Id::unique(("window_button", kind as u8)),
        Sense::click(),
    );
    let shade = if response.hovered() { 0.24 } else { 0.12 };
    ui.painter()
        .circle_filled(centre, 13.0, mix(c.toolbar, c.text, shade));
    let stroke = Stroke::new(1.5, c.text);
    let painter = ui.painter();
    match kind {
        Button::Minimize => {
            painter.line_segment([centre + vec2(-5.0, 3.0), centre + vec2(5.0, 3.0)], stroke);
        }
        Button::Maximize if cx.app.maximized => {
            // Two overlapping squares: restore.
            painter.rect_stroke(
                Rect::from_center_size(centre + vec2(-1.0, 1.0), vec2(8.0, 8.0)),
                1.0,
                stroke,
                StrokeKind::Middle,
            );
            painter.line_segment(
                [centre + vec2(-0.0, -4.0), centre + vec2(5.0, -4.0)],
                stroke,
            );
            painter.line_segment([centre + vec2(5.0, -4.0), centre + vec2(5.0, 2.0)], stroke);
        }
        Button::Maximize => {
            painter.rect_stroke(
                Rect::from_center_size(centre, vec2(10.0, 10.0)),
                1.5,
                stroke,
                StrokeKind::Middle,
            );
        }
        Button::Close => {
            painter.line_segment([centre + vec2(-4.5, -4.5), centre + vec2(4.5, 4.5)], stroke);
            painter.line_segment([centre + vec2(-4.5, 4.5), centre + vec2(4.5, -4.5)], stroke);
        }
    }
    if response.clicked() {
        cx.send(match kind {
            Button::Minimize => Message::Window(WindowAction::Minimize),
            Button::Maximize => Message::Window(WindowAction::ToggleMaximize),
            Button::Close => Message::Quit,
        });
    }
}

/// The top row of the window: menus on the left, the title in the middle, window buttons on
/// the right. Dragging the empty part moves the window; a double click maximises it.
pub fn title_bar(ui: &mut Ui, cx: &Cx) {
    let look = cx.look();
    let c = &look.c;
    let (rect, background) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::click_and_drag());
    let corner = if cx.app.maximized { 0 } else { RADIUS };
    ui.painter().rect_filled(
        rect,
        CornerRadius {
            nw: corner,
            ne: corner,
            sw: 0,
            se: 0,
        },
        c.toolbar,
    );
    paint_strong_centered(ui, cx, rect);
    let menus = Rect::from_min_size(rect.min + vec2(6.0, 0.0), vec2(220.0, HEIGHT));
    region(ui, menus, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (label, request) in [
                ("File", MenuRequest::BarFile),
                ("View", MenuRequest::BarView),
                ("Help", MenuRequest::BarHelp),
            ] {
                if tool_text(ui, label, 13.0).clicked() {
                    cx.send(Message::OpenMenu(request));
                }
            }
        });
    });
    let y = rect.center().y;
    for (index, kind) in [Button::Minimize, Button::Maximize, Button::Close]
        .into_iter()
        .enumerate()
    {
        let x = rect.right() - 20.0 - (2 - index) as f32 * 34.0;
        window_button(ui, cx, Pos2::new(x, y), kind);
    }
    if background.drag_started() {
        cx.send(Message::Window(WindowAction::Drag));
    }
    if background.double_clicked() {
        cx.send(Message::Window(WindowAction::ToggleMaximize));
    }
}

fn paint_strong_centered(ui: &Ui, cx: &Cx, rect: Rect) {
    let look = cx.look();
    let title = cx.app.title();
    let font = look.ui(13.5);
    let width = super::widgets::text_width(ui, &title, &font);
    let _ = Align2::CENTER_CENTER;
    paint_strong(
        ui.painter(),
        Pos2::new(rect.center().x - width / 2.0, rect.center().y),
        &title,
        font,
        look.c.text,
    );
}

/// Invisible handles along the window's edges for resizing, and a thin outline.
pub fn window_edges(ui: &mut Ui, cx: &Cx) {
    if !cx.app.csd || cx.app.maximized {
        return;
    }
    let full = ui.ctx().content_rect();
    let (w, h) = (full.width(), full.height());
    let at = |x: f32, y: f32, width: f32, height: f32| {
        Rect::from_min_size(full.min + vec2(x, y), vec2(width, height))
    };
    let corner = EDGE * 2.0;
    // Edges first and corners last, as what is added later is on top.
    let zones = [
        (
            at(corner, 0.0, w - 2.0 * corner, EDGE),
            ResizeDirection::North,
            CursorIcon::ResizeNorth,
        ),
        (
            at(corner, h - EDGE, w - 2.0 * corner, EDGE),
            ResizeDirection::South,
            CursorIcon::ResizeSouth,
        ),
        (
            at(0.0, corner, EDGE, h - 2.0 * corner),
            ResizeDirection::West,
            CursorIcon::ResizeWest,
        ),
        (
            at(w - EDGE, corner, EDGE, h - 2.0 * corner),
            ResizeDirection::East,
            CursorIcon::ResizeEast,
        ),
        (
            at(0.0, 0.0, corner, corner),
            ResizeDirection::NorthWest,
            CursorIcon::ResizeNorthWest,
        ),
        (
            at(w - corner, 0.0, corner, corner),
            ResizeDirection::NorthEast,
            CursorIcon::ResizeNorthEast,
        ),
        (
            at(0.0, h - corner, corner, corner),
            ResizeDirection::SouthWest,
            CursorIcon::ResizeSouthWest,
        ),
        (
            at(w - corner, h - corner, corner, corner),
            ResizeDirection::SouthEast,
            CursorIcon::ResizeSouthEast,
        ),
    ];
    for (index, (rect, direction, cursor)) in zones.into_iter().enumerate() {
        let response = ui.interact(rect, Id::unique(("window_edge", index)), Sense::drag());
        if response.hovered() {
            ui.ctx().set_cursor_icon(cursor);
        }
        if response.drag_started() {
            cx.send(Message::Window(WindowAction::Resize(direction)));
        }
    }
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        Id::unique("window_outline"),
    ));
    painter.rect_stroke(
        full.shrink(0.5),
        CornerRadius::same(RADIUS),
        Stroke::new(1.0, cx.c().border),
        StrokeKind::Inside,
    );
}
