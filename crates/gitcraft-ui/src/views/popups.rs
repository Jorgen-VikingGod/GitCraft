//! Things drawn above the page: forms, context menus and toasts.

use egui::{
    Align, Align2, Area, Color32, Context, Id, Layout, Margin, Order, Pos2, Rect, Sense, Stroke,
    Ui, vec2,
};

use super::widgets::{
    Kind, area_edit, btn, button, check, choice, floating, icon_button, line_edit, text,
};
use super::{Cx, icon};
use crate::app::{Message, Toast, ToastKind};
use crate::form::{FieldKind, Form};
use crate::menu::{Menu, MenuItem};

const MENU_MIN_WIDTH: f32 = 210.0;
const MENU_MAX_WIDTH: f32 = 380.0;
const MENU_ROW: f32 = 27.0;
const MENU_HEADER: f32 = 23.0;
const MENU_SEPARATOR: f32 = 9.0;
const MENU_PADDING: f32 = 4.0;

pub fn form(ctx: &Context, cx: &Cx, form: &Form) {
    let look = cx.look();
    let c = &look.c;
    let size = look.text_size;
    let window = cx.app.window;
    let id = Id::unique("form");
    let area = egui::Modal::default_area(id).anchor(Align2::CENTER_TOP, vec2(0.0, window.y * 0.12));
    let response = egui::Modal::new(id)
        .area(area)
        .backdrop_color(c.backdrop)
        .frame(floating(look, 10).inner_margin(Margin::symmetric(20, 18)))
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.spacing_mut().item_spacing = vec2(8.0, 12.0);
            egui::ScrollArea::vertical()
                .max_height((window.y * 0.8 - 40.0).max(160.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.set_width(440.0);
                    text(ui, form.title.as_str(), 16.0, c.text);
                    if let Some(message) = &form.message {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(message.as_str())
                                    .size(size)
                                    .color(c.text_muted),
                            )
                            .wrap(),
                        );
                    }
                    for (index, field) in form.fields.iter().enumerate() {
                        form_field(ui, cx, index, field);
                    }
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let kind = if form.danger {
                                Kind::Danger
                            } else {
                                Kind::Primary
                            };
                            if button(ui, look, &form.ok, kind, form.can_submit()) {
                                cx.send(Message::FormSubmit);
                            }
                            if btn(ui, look, "Cancel", true) {
                                cx.send(Message::FormCancel);
                            }
                        });
                    });
                });
        });
    if response.backdrop_response.clicked() {
        cx.send(Message::FormCancel);
    }
}

fn form_field(ui: &mut Ui, cx: &Cx, index: usize, field: &crate::form::Field) {
    let look = cx.look();
    let c = &look.c;
    let size = look.text_size;
    let label = |ui: &mut Ui| text(ui, field.label.as_str(), 12.0, c.text_muted);
    match &field.kind {
        FieldKind::Text(value) => {
            ui.spacing_mut().item_spacing.y = 4.0;
            label(ui);
            let (response, changed) = line_edit(
                ui,
                ("form", index),
                value,
                &field.placeholder,
                ui.available_width(),
                look.ui(size),
            );
            if let Some(text) = changed {
                cx.send(Message::FormText(index, text));
            }
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                cx.send(Message::FormSubmit);
            }
        }
        FieldKind::Area(value) => {
            ui.spacing_mut().item_spacing.y = 4.0;
            label(ui);
            if let Some(text) = area_edit(ui, ("form", index), value, "", 120.0, look.ui(size)) {
                cx.send(Message::FormArea(index, text));
            }
        }
        FieldKind::Check(value) => {
            if let Some(checked) = check(ui, *value, &field.label, true) {
                cx.send(Message::FormCheck(index, checked));
            }
        }
        FieldKind::Select { options, value } => {
            ui.spacing_mut().item_spacing.y = 4.0;
            label(ui);
            if let Some(chosen) = choice(
                ui,
                ("form", index),
                options,
                value,
                ui.available_width() - 20.0,
            ) {
                cx.send(Message::FormSelect(index, chosen));
            }
        }
    }
}

fn height_of(items: &[MenuItem]) -> f32 {
    let rows: f32 = items
        .iter()
        .map(|item| match item {
            MenuItem::Separator => MENU_SEPARATOR,
            MenuItem::Header(_) => MENU_HEADER,
            _ => MENU_ROW,
        })
        .sum();
    rows + 2.0 * (MENU_PADDING + 1.0)
}

/// As wide as the longest entry, as a menu sized to its content would be.
fn width_of(ctx: &Context, cx: &Cx, items: &[MenuItem]) -> f32 {
    let font = cx.look().ui(13.0);
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::unique("measure")));
    let longest = items
        .iter()
        .map(|item| {
            let extra = if matches!(item, MenuItem::Sub { .. }) {
                26.0
            } else {
                0.0
            };
            let width = item.label().map_or(0.0, |label| {
                painter
                    .layout_no_wrap(label.to_owned(), font.clone(), Color32::PLACEHOLDER)
                    .size()
                    .x
            });
            width + extra
        })
        .fold(0.0, f32::max);
    (longest + 24.0 + 2.0 * (MENU_PADDING + 1.0)).clamp(MENU_MIN_WIDTH, MENU_MAX_WIDTH)
}

fn panel(
    ui: &mut Ui,
    cx: &Cx,
    items: &[MenuItem],
    open_sub: Option<usize>,
    top_level: bool,
    width: f32,
) {
    let look = cx.look();
    let c = &look.c;
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let inner = width - 2.0 * (MENU_PADDING + 1.0);
    for (index, item) in items.iter().enumerate() {
        match item {
            MenuItem::Separator => {
                let (rect, _) = ui.allocate_exact_size(vec2(inner, MENU_SEPARATOR), Sense::hover());
                ui.painter().hline(
                    rect.x_range().shrink(6.0),
                    rect.center().y,
                    Stroke::new(1.0, c.border),
                );
            }
            MenuItem::Header(label) => {
                let (rect, _) = ui.allocate_exact_size(vec2(inner, MENU_HEADER), Sense::hover());
                ui.painter().with_clip_rect(rect).text(
                    Pos2::new(rect.left() + 12.0, rect.center().y + 2.0),
                    Align2::LEFT_CENTER,
                    label.to_uppercase(),
                    look.ui(11.0),
                    c.text_muted,
                );
            }
            MenuItem::Entry {
                label,
                message,
                danger,
            } => {
                let enabled = message.is_some();
                let sense = if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                };
                let (rect, response) = ui.allocate_exact_size(vec2(inner, MENU_ROW), sense);
                let lit = enabled && response.hovered();
                let highlight = if *danger { c.danger } else { c.accent };
                if lit {
                    ui.painter().rect_filled(rect, 5.0, highlight);
                }
                let color = if lit {
                    Color32::WHITE
                } else if !enabled {
                    c.text_faint
                } else if *danger {
                    c.danger
                } else {
                    c.text
                };
                ui.painter().with_clip_rect(rect).text(
                    Pos2::new(rect.left() + 12.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    look.ui(13.0),
                    color,
                );
                if top_level && response.hovered() && open_sub.is_some() {
                    cx.send(Message::MenuHover(None));
                }
                if response.clicked()
                    && let Some(message) = message
                {
                    cx.send(Message::FromMenu(Box::new(message.clone())));
                }
            }
            MenuItem::Sub { label, .. } => {
                let open = open_sub == Some(index);
                let (rect, response) =
                    ui.allocate_exact_size(vec2(inner, MENU_ROW), Sense::click());
                let lit = open || response.hovered();
                if lit {
                    ui.painter().rect_filled(rect, 5.0, c.accent);
                }
                ui.painter().with_clip_rect(rect).text(
                    Pos2::new(rect.left() + 12.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    look.ui(13.0),
                    if lit { Color32::WHITE } else { c.text },
                );
                icon::paint(
                    ui,
                    "chevron-right",
                    Rect::from_center_size(
                        Pos2::new(rect.right() - 14.0, rect.center().y),
                        vec2(12.0, 12.0),
                    ),
                    if lit { Color32::WHITE } else { c.text_muted },
                );
                if (response.hovered() || response.clicked()) && open_sub != Some(index) {
                    cx.send(Message::MenuHover(Some(index)));
                }
            }
        }
    }
}

pub fn menu(ctx: &Context, cx: &Cx, menu: &Menu) {
    let look = cx.look();
    let window = cx.app.window;
    // Anything outside the menu closes it, and does nothing else.
    Area::new(Id::unique("menu_backdrop"))
        .order(Order::Foreground)
        .fixed_pos(Pos2::ZERO)
        .show(ctx, |ui| {
            let response = ui.allocate_response(window, Sense::click());
            if response.clicked() || response.secondary_clicked() {
                cx.send(Message::CloseMenu);
            }
        });

    let main_width = width_of(ctx, cx, &menu.items);
    let main_height = height_of(&menu.items);
    // Stay on screen, with a small margin.
    let origin = Pos2::new(
        menu.at.x.min(window.x - main_width - 4.0).max(4.0),
        menu.at.y.min(window.y - main_height - 4.0).max(4.0),
    );
    Area::new(Id::unique("menu_panel"))
        .order(Order::Foreground)
        .fixed_pos(origin)
        .show(ctx, |ui| {
            floating(look, 8)
                .inner_margin(Margin::same(MENU_PADDING as i8))
                .show(ui, |ui| {
                    panel(ui, cx, &menu.items, menu.open_sub, true, main_width);
                });
        });

    if let Some(index) = menu.open_sub
        && let Some(MenuItem::Sub { children, .. }) = menu.items.get(index)
    {
        let child_width = width_of(ctx, cx, children);
        let above = height_of(&menu.items[..index]) - 2.0 * (MENU_PADDING + 1.0);
        // Opens to the left when there's no room on the right.
        let child_x = if origin.x + main_width + child_width > window.x - 4.0 {
            (origin.x - child_width + 2.0).max(4.0)
        } else {
            origin.x + main_width - 2.0
        };
        let child_y = (origin.y + above)
            .min(window.y - height_of(children) - 4.0)
            .max(4.0);
        Area::new(Id::unique("menu_sub"))
            .order(Order::Foreground)
            .fixed_pos(Pos2::new(child_x, child_y))
            .show(ctx, |ui| {
                floating(look, 8)
                    .inner_margin(Margin::same(MENU_PADDING as i8))
                    .show(ui, |ui| {
                        panel(ui, cx, children, None, false, child_width);
                    });
            });
    }
}

fn toast_view(ui: &mut Ui, cx: &Cx, toast: &Toast) {
    let look = cx.look();
    let c = &look.c;
    let bar = match toast.kind {
        ToastKind::Error => c.danger,
        ToastKind::Warning => c.warning,
        ToastKind::Success => c.success,
        ToastKind::Info => c.accent,
    };
    let frame = floating(look, 8).inner_margin(Margin::ZERO);
    let shown = frame.show(ui, |ui| {
        ui.set_width(420.0);
        let start = ui.cursor().min;
        egui::Frame::new()
            .inner_margin(Margin {
                left: 14,
                right: 8,
                top: 10,
                bottom: 10,
            })
            .show(ui, |ui| {
                ui.set_width(398.0);
                ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(350.0);
                        ui.spacing_mut().item_spacing.y = 6.0;
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(toast.message.as_str())
                                    .size(13.0)
                                    .color(c.text),
                            )
                            .wrap(),
                        );
                        if let Some(detail) =
                            toast.detail.as_ref().filter(|detail| !detail.is_empty())
                        {
                            let short: String = detail.chars().take(600).collect();
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(short)
                                        .size(11.0)
                                        .monospace()
                                        .color(c.text_muted),
                                )
                                .wrap(),
                            );
                        }
                    });
                    if icon_button(ui, look, "x", 13.0, true) {
                        cx.send(Message::DismissToast(toast.id));
                    }
                });
            });
        start
    });
    let rect = shown.response.rect;
    let strip = Rect::from_min_size(rect.min, vec2(4.0, rect.height()));
    ui.painter().rect_filled(
        strip,
        egui::CornerRadius {
            nw: 8,
            sw: 8,
            ne: 0,
            se: 0,
        },
        bar,
    );
}

pub fn toasts(ctx: &Context, cx: &Cx) {
    if cx.app.toasts.is_empty() {
        return;
    }
    Area::new(Id::unique("toasts"))
        .order(Order::Tooltip)
        .anchor(Align2::LEFT_BOTTOM, vec2(16.0, -36.0))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for toast in &cx.app.toasts {
                toast_view(ui, cx, toast);
            }
        });
}
