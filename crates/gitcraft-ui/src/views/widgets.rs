//! Small building blocks shared by the views.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Margin, Painter, Pos2, Rect, Response,
    RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, Vec2, vec2,
};
use gitcraft_core::model::{FileChange, RefLabel};

use super::{Cx, icon};
use crate::app::{Message, Splitter};
use crate::theme::{Colors, Look};

const SMALL_BUTTON: f32 = 24.0;
/// The draggable strip between two regions.
pub const GRIP: f32 = 5.0;

// ------------------------------------------------------------------ layout

/// The left `width` of a rectangle, and the rest.
pub fn split_x(rect: Rect, width: f32) -> (Rect, Rect) {
    let width = width.clamp(0.0, rect.width());
    (
        Rect::from_min_size(rect.min, vec2(width, rect.height())),
        Rect::from_min_max(Pos2::new(rect.min.x + width, rect.min.y), rect.max),
    )
}

/// The top `height` of a rectangle, and the rest.
pub fn split_y(rect: Rect, height: f32) -> (Rect, Rect) {
    let height = height.clamp(0.0, rect.height());
    (
        Rect::from_min_size(rect.min, vec2(rect.width(), height)),
        Rect::from_min_max(Pos2::new(rect.min.x, rect.min.y + height), rect.max),
    )
}

/// Lays `add` out inside `rect`, clipped to it.
pub fn region<R>(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.set_min_size(rect.size());
    // The parent's cursor stays where it was: regions are placed by rectangle, and moving
    // the cursor back would draw whatever follows on top of them.
    add(&mut child)
}

pub fn fill(ui: &Ui, rect: Rect, color: Color32) {
    ui.painter().rect_filled(rect, 0.0, color);
}

pub fn hline(ui: &mut Ui, look: &Look) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    fill(ui, rect, look.c.border);
}

/// A drag handle in `rect`; the edge it sits on is drawn as one line.
pub fn splitter(ui: &mut Ui, cx: &Cx, rect: Rect, which: Splitter) {
    let horizontal = which == Splitter::History;
    let response = ui.interact(rect, Id::unique(("splitter", which as u8)), Sense::drag());
    let c = cx.c();
    let mid = rect.center();
    let active = response.hovered() || response.dragged();
    let color = if active { c.accent } else { c.border };
    let (from, to) = if horizontal {
        (
            Pos2::new(rect.left(), mid.y),
            Pos2::new(rect.right(), mid.y),
        )
    } else {
        (
            Pos2::new(mid.x, rect.top()),
            Pos2::new(mid.x, rect.bottom()),
        )
    };
    ui.painter()
        .line_segment([from, to], Stroke::new(1.0, color));
    if active {
        ui.ctx().set_cursor_icon(if horizontal {
            CursorIcon::ResizeVertical
        } else {
            CursorIcon::ResizeHorizontal
        });
    }
    if response.dragged() {
        let delta = response.drag_delta();
        cx.send(Message::Resize(
            which,
            if horizontal { delta.y } else { delta.x },
        ));
    }
    if response.drag_stopped() {
        cx.send(Message::ResizeDone);
    }
}

/// A vertical scroll area that fills the space it is given.
pub fn scroll(id: impl std::hash::Hash + std::fmt::Debug) -> ScrollArea {
    ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
}

// -------------------------------------------------------------------- text

pub fn text(ui: &mut Ui, content: impl Into<String>, size: f32, color: Color32) {
    ui.add(egui::Label::new(RichText::new(content).size(size).color(color)).truncate());
}

pub fn mono(ui: &mut Ui, content: impl Into<String>, size: f32, color: Color32) {
    ui.add(egui::Label::new(RichText::new(content).size(size).color(color).monospace()).truncate());
}

pub fn muted(ui: &mut Ui, look: &Look, content: impl Into<String>, size: f32) {
    text(ui, content, size, look.c.text_muted);
}

/// A small heading in capitals, like those above lists.
pub fn heading(ui: &mut Ui, look: &Look, content: &str) {
    text(ui, content.to_uppercase(), 11.0, look.c.text_muted);
}

/// Text centred in whatever space is left.
pub fn placeholder(ui: &mut Ui, look: &Look, note: &str) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        note,
        look.ui(look.text_size),
        look.c.text_muted,
    );
    ui.allocate_rect(rect, Sense::hover());
}

pub fn text_width(ui: &Ui, content: &str, font: &FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(content.to_owned(), font.clone(), Color32::PLACEHOLDER)
        .size()
        .x
}

/// Draws text with its left edge centred vertically on `at`; returns the width.
pub fn paint_text(painter: &Painter, at: Pos2, content: &str, font: FontId, color: Color32) -> f32 {
    painter
        .text(at, Align2::LEFT_CENTER, content, font, color)
        .width()
}

// ----------------------------------------------------------------- buttons

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Normal,
    Primary,
    Danger,
    /// No frame until hovered.
    Ghost,
    /// No frame, in the danger colour.
    GhostDanger,
}

fn dim(color: Color32, enabled: bool) -> Color32 {
    if enabled {
        color
    } else {
        color.gamma_multiply(0.5)
    }
}

fn brighter(color: Color32) -> Color32 {
    crate::theme::mix(color, Color32::WHITE, 0.14)
}

/// The standard small button, drawn by egui so that it reacts like every other widget.
/// True when it was clicked.
pub fn button(ui: &mut Ui, look: &Look, label: &str, kind: Kind, enabled: bool) -> bool {
    enabled && button_response(ui, look, label, kind, enabled).clicked()
}

/// A [`button`] that says what it does when the pointer rests on it, also while it is off.
pub fn button_tip(
    ui: &mut Ui,
    look: &Look,
    label: &str,
    kind: Kind,
    enabled: bool,
    tip: &str,
) -> bool {
    let response = button_response(ui, look, label, kind, enabled);
    let response = if enabled {
        response.on_hover_text(tip)
    } else {
        response.on_disabled_hover_text(tip)
    };
    enabled && response.clicked()
}

fn button_response(ui: &mut Ui, look: &Look, label: &str, kind: Kind, enabled: bool) -> Response {
    let c = &look.c;
    let text = RichText::new(label).size(12.0);
    ui.scope(|ui| {
        let solid = match kind {
            Kind::Primary => Some(c.accent),
            Kind::Danger => Some(c.danger),
            _ => None,
        };
        let widgets = &mut ui.style_mut().visuals.widgets;
        if let Some(fill) = solid {
            for state in [
                &mut widgets.inactive,
                &mut widgets.hovered,
                &mut widgets.active,
            ] {
                state.fg_stroke.color = Color32::WHITE;
                state.bg_stroke = Stroke::NONE;
            }
            widgets.inactive.weak_bg_fill = fill;
            widgets.hovered.weak_bg_fill = brighter(fill);
            widgets.active.weak_bg_fill = crate::theme::mix(fill, Color32::BLACK, 0.15);
        }
        let mut button = egui::Button::new(text).min_size(vec2(0.0, SMALL_BUTTON));
        match kind {
            Kind::Ghost => button = button.frame_when_inactive(false),
            Kind::GhostDanger => {
                button = egui::Button::new(text_colored(label, c.danger))
                    .min_size(vec2(0.0, SMALL_BUTTON))
                    .frame_when_inactive(false);
            }
            _ => {}
        }
        ui.add_enabled(enabled, button)
    })
    .inner
}

fn text_colored(label: &str, color: Color32) -> RichText {
    RichText::new(label).size(12.0).color(color)
}

pub fn btn(ui: &mut Ui, look: &Look, label: &str, enabled: bool) -> bool {
    button(ui, look, label, Kind::Normal, enabled)
}

pub fn primary(ui: &mut Ui, look: &Look, label: &str, enabled: bool) -> bool {
    button(ui, look, label, Kind::Primary, enabled)
}

pub fn danger_text(ui: &mut Ui, look: &Look, label: &str, enabled: bool) -> bool {
    button(ui, look, label, Kind::GhostDanger, enabled)
}

pub fn ghost(ui: &mut Ui, look: &Look, label: &str, enabled: bool) -> bool {
    button(ui, look, label, Kind::Ghost, enabled)
}

/// A button that stays lit while its choice is on.
pub fn toggle(ui: &mut Ui, look: &Look, label: &str, active: bool) -> bool {
    let kind = if active { Kind::Primary } else { Kind::Normal };
    button(ui, look, label, kind, true)
}

/// A square button with only an icon.
pub fn icon_button(ui: &mut Ui, look: &Look, name: &str, size: f32, enabled: bool) -> bool {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(26.0, 26.0), sense);
    icon_in(ui, look, &response, rect, name, size, enabled);
    enabled && response.clicked()
}

/// The inside of an icon button: a hover background and the icon.
pub fn icon_in(
    ui: &Ui,
    look: &Look,
    response: &Response,
    rect: Rect,
    name: &str,
    size: f32,
    enabled: bool,
) {
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 6.0, look.c.hover_strong);
    }
    let color = dim(look.c.text_muted, enabled);
    icon::paint(
        ui,
        name,
        Rect::from_center_size(rect.center(), Vec2::splat(size)),
        color,
    );
}

/// A two-part switch, such as Unified and Split. Gives the part that was clicked.
pub fn segmented(ui: &mut Ui, look: &Look, labels: [&str; 2], active: usize) -> Option<usize> {
    let c = &look.c;
    let font = look.ui(11.5);
    let widths = labels.map(|label| text_width(ui, label, &font) + 18.0);
    let (frame, _) =
        ui.allocate_exact_size(vec2(widths[0] + widths[1] + 2.0, 22.0), Sense::hover());
    ui.painter().rect_filled(frame, 6.0, c.panel);
    ui.painter()
        .rect_stroke(frame, 6.0, Stroke::new(1.0, c.border), StrokeKind::Inside);
    let mut clicked = None;
    let mut left = frame.left() + 1.0;
    for (index, label) in labels.iter().enumerate() {
        let part = Rect::from_min_size(
            Pos2::new(left, frame.top() + 1.0),
            vec2(widths[index], frame.height() - 2.0),
        );
        left += widths[index];
        let response = ui.interact(
            part,
            ui.scope_id().with(("segment", index, *label)),
            Sense::click(),
        );
        let radius = if index == 0 {
            CornerRadius {
                nw: 5,
                sw: 5,
                ne: 0,
                se: 0,
            }
        } else {
            CornerRadius {
                nw: 0,
                sw: 0,
                ne: 5,
                se: 5,
            }
        };
        let on = index == active;
        if on {
            ui.painter().rect_filled(part, radius, c.accent);
        }
        ui.painter().text(
            part.center(),
            Align2::CENTER_CENTER,
            label,
            font.clone(),
            if on { Color32::WHITE } else { c.text_muted },
        );
        if response.clicked() {
            clicked = Some(index);
        }
    }
    clicked
}

// ------------------------------------------------------------------ inputs

/// A one-line field. Gives the response, and the new text when it changed.
pub fn line_edit(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &str,
    hint: &str,
    width: f32,
    font: FontId,
) -> (Response, Option<String>) {
    let mut buffer = value.to_owned();
    let response = ui.add(
        TextEdit::singleline(&mut buffer)
            .id_salt(id)
            .hint_text(hint)
            .font(font)
            .desired_width(width)
            .margin(Margin::symmetric(8, 5)),
    );
    let changed = (buffer != value).then_some(buffer);
    (response, changed)
}

/// A field without its own frame, for use inside another one.
pub fn bare_edit(
    ui: &mut Ui,
    id: Id,
    value: &str,
    hint: &str,
    width: f32,
    font: FontId,
) -> (Response, Option<String>) {
    let mut buffer = value.to_owned();
    let response = ui.add(
        TextEdit::singleline(&mut buffer)
            .id(id)
            .hint_text(hint)
            .font(font)
            .frame(egui::Frame::NONE)
            .desired_width(width),
    );
    let changed = (buffer != value).then_some(buffer);
    (response, changed)
}

/// A multi-line field of a fixed height that scrolls.
pub fn area_edit(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &str,
    hint: &str,
    height: f32,
    font: FontId,
) -> Option<String> {
    let mut buffer = value.to_owned();
    ScrollArea::vertical()
        .id_salt(("area", Id::unique(format!("{id:?}"))))
        .max_height(height)
        .min_scrolled_height(height)
        .show(ui, |ui| {
            ui.add(
                TextEdit::multiline(&mut buffer)
                    .id_salt(id)
                    .hint_text(hint)
                    .font(font)
                    .desired_width(f32::INFINITY)
                    .desired_rows(((height - 12.0) / 16.0).max(2.0) as usize)
                    .margin(Margin::symmetric(8, 6)),
            );
        });
    (buffer != value).then_some(buffer)
}

/// A check box. Gives the new value when it was clicked.
pub fn check(ui: &mut Ui, value: bool, label: &str, enabled: bool) -> Option<bool> {
    let mut checked = value;
    let response = ui.add_enabled(enabled, egui::Checkbox::new(&mut checked, label));
    response.changed().then_some(checked)
}

/// A drop-down list of (value, label). Gives the value that was chosen.
pub fn choice(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    options: &[(String, String)],
    value: &str,
    width: f32,
) -> Option<String> {
    let shown = options
        .iter()
        .find(|(candidate, _)| candidate == value)
        .map_or("", |(_, label)| label.as_str());
    let mut chosen = None;
    egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .width(width)
        .show_ui(ui, |ui| {
            for (candidate, label) in options {
                if ui.selectable_label(candidate == value, label).clicked() {
                    chosen = Some(candidate.clone());
                }
            }
        });
    chosen
}

// ------------------------------------------------------------------- rows

/// Lights up under the pointer, as list rows do.
pub fn hover_fill(ui: &Ui, look: &Look, rect: Rect, radius: f32) {
    ui.painter().rect_filled(rect, radius, look.c.hover);
}

pub fn status_color(c: &Colors, status: &str) -> Color32 {
    match status {
        "A" | "?" => c.success,
        "D" | "U" => c.danger,
        "R" | "C" => c.accent,
        _ => c.warning,
    }
}

pub struct FileRow {
    pub row: Response,
    /// The quick action at the right end was clicked.
    pub action: bool,
}

/// A row in a file list: status letter, folder, name. Clicks are up to the caller.
/// `action` is the icon of a button that appears when the row is hovered or selected.
pub fn file_row(
    ui: &mut Ui,
    cx: &Cx,
    file: &FileChange,
    selected: bool,
    action: Option<&str>,
) -> FileRow {
    let look = cx.look();
    let c = &look.c;
    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    let body = rect.shrink2(vec2(4.0, 0.0));
    let hovered = ui.rect_contains_pointer(rect);
    // Moving the selection with the keyboard must bring the row into view.
    let was_selected_id = row.id.with("was_selected");
    let was_selected = ui.data(|data| data.get_temp::<bool>(was_selected_id).unwrap_or(false));
    if selected && !was_selected {
        ui.scroll_to_rect(rect, None);
    }
    ui.data_mut(|data| data.insert_temp(was_selected_id, selected));
    if selected {
        ui.painter().rect_filled(body, 4.0, c.selection);
    } else if hovered {
        hover_fill(ui, look, body, 4.0);
    }
    let (dir, name) = match file.path.rfind('/') {
        Some(at) => file.path.split_at(at + 1),
        None => ("", file.path.as_str()),
    };
    let letter = if file.status == "?" {
        "+"
    } else {
        file.status.as_str()
    };
    let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
    let mid = body.center().y;
    painter.text(
        Pos2::new(body.left() + 16.0, mid),
        Align2::CENTER_CENTER,
        letter,
        look.mono(11.0),
        status_color(c, &file.status),
    );
    let name_color = if selected { c.selection_text } else { c.text };
    let dir_color = if selected {
        c.selection_text.gamma_multiply(0.75)
    } else {
        c.text_muted
    };
    let mut x = body.left() + 32.0;
    let font = look.ui(13.0);
    if let Some(orig) = &file.orig_path {
        x += paint_text(
            &painter,
            Pos2::new(x, mid),
            &format!("{orig} → "),
            font.clone(),
            dir_color,
        );
    }
    if !dir.is_empty() {
        x += paint_text(&painter, Pos2::new(x, mid), dir, font.clone(), dir_color);
    }
    paint_text(&painter, Pos2::new(x, mid), name, font, name_color);
    let mut clicked = false;
    if let (Some(name), true) = (action, hovered || selected) {
        let button = Rect::from_center_size(Pos2::new(body.right() - 18.0, mid), vec2(26.0, 22.0));
        let response = ui.interact(button, row.id.with("action"), Sense::click());
        icon_in(ui, look, &response, button, name, 14.0, true);
        clicked = response.clicked();
    }
    FileRow {
        row,
        action: clicked,
    }
}

// ------------------------------------------------------------------ badges

fn badge_colors(c: &Colors, label: &RefLabel) -> (Color32, Color32, &'static str) {
    match label.kind.as_str() {
        "tag" => (c.badge_tag_bg, c.badge_tag_text, "tag"),
        "remote" => (c.badge_remote_bg, c.badge_remote_text, "cloud"),
        "head" => (c.badge_head_bg, c.badge_head_text, "commit"),
        _ if label.current => (c.badge_local_bg, c.badge_local_text, "check"),
        _ => (c.badge_local_bg, c.badge_local_text, "branch"),
    }
}

pub fn badge_width(ui: &Ui, look: &Look, label: &RefLabel) -> f32 {
    text_width(ui, &label.name, &look.ui(11.0)) + 11.0 + 3.0 + 12.0
}

/// The rounded labels for branches and tags, drawn into `rect`.
pub fn badge_paint(ui: &Ui, look: &Look, label: &RefLabel, rect: Rect) {
    let (background, color, name) = badge_colors(&look.c, label);
    let painter = ui.painter();
    painter.rect_filled(rect, 9.0, background);
    if label.current {
        painter.rect_stroke(rect, 9.0, Stroke::new(1.0, color), StrokeKind::Inside);
    }
    icon::paint(
        ui,
        name,
        Rect::from_center_size(
            Pos2::new(rect.left() + 11.0, rect.center().y),
            Vec2::splat(11.0),
        ),
        color,
    );
    painter.text(
        Pos2::new(rect.left() + 20.0, rect.center().y),
        Align2::LEFT_CENTER,
        &label.name,
        look.ui(11.0),
        color,
    );
}

/// A badge as a widget of its own.
pub fn badge(ui: &mut Ui, look: &Look, label: &RefLabel) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(badge_width(ui, look, label), 18.0), Sense::click());
    badge_paint(ui, look, label, rect);
    response
}

// -------------------------------------------------------------------- bars

/// A full-width strip of a fixed height, with a left-to-right row of centred items inside.
pub fn bar(ui: &mut Ui, height: f32, color: Color32, pad: f32, add: impl FnOnce(&mut Ui)) {
    bar_with(ui, height, color, CornerRadius::ZERO, pad, add);
}

/// A [`bar`] with rounded corners, for the edges of a window drawn by the app itself.
pub fn bar_with(
    ui: &mut Ui,
    height: f32,
    color: Color32,
    radius: CornerRadius,
    pad: f32,
    add: impl FnOnce(&mut Ui),
) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    ui.painter().rect_filled(rect, radius, color);
    let inner = rect.shrink2(vec2(pad, 0.0));
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.set_clip_rect(inner.intersect(ui.clip_rect()));
    row.spacing_mut().item_spacing = vec2(6.0, 0.0);
    add(&mut row);
}

/// A borderless label button that lights up under the pointer.
pub fn tool_text(ui: &mut Ui, label: &str, size: f32) -> Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(size))
            .frame_when_inactive(false)
            .min_size(vec2(0.0, 22.0)),
    )
}

// ------------------------------------------------------------------ frames

/// A floating surface: dialogs, menus and toasts.
pub fn floating(look: &Look, radius: u8) -> egui::Frame {
    let c = &look.c;
    egui::Frame::new()
        .fill(c.panel)
        .stroke(Stroke::new(1.0, c.border))
        .corner_radius(radius)
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: c.shadow,
        })
}

/// A bordered panel for grouping, such as one conflict.
pub fn card(look: &Look) -> egui::Frame {
    egui::Frame::new()
        .fill(look.c.panel)
        .stroke(Stroke::new(1.0, look.c.border))
        .corner_radius(8)
}

/// Text drawn twice, a hair apart, to look heavier than the one weight there is.
pub fn paint_strong(painter: &Painter, at: Pos2, content: &str, font: FontId, color: Color32) {
    painter.text(at, Align2::LEFT_CENTER, content, font.clone(), color);
    painter.text(
        at + vec2(0.6, 0.0),
        Align2::LEFT_CENTER,
        content,
        font,
        color,
    );
}

/// Lays `add` out in the middle of `rect`, as wide as it needs to be.
pub fn centered<R>(ui: &mut Ui, rect: Rect, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let inner =
        Rect::from_center_size(rect.center(), vec2(rect.width(), height.min(rect.height())));
    region(ui, inner, |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Center), add)
            .inner
    })
}
