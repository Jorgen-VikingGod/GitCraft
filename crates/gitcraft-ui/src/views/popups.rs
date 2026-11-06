//! Things drawn above the page: forms, context menus and toasts.

use egui::{
    Align, Align2, Area, Context, Id, Layout, Margin, Order, Popup, PopupCloseBehavior, Rect,
    RichText, Sense, Ui, vec2,
};

use super::Cx;
use super::widgets::{
    Kind, area_edit, btn, button, check, choice, floating, icon_button, line_edit, text,
};
use crate::app::{Message, Toast, ToastKind};
use crate::form::{FieldKind, Form};
use crate::menu::{Menu, MenuItem};

const MENU_MIN_WIDTH: f32 = 210.0;

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

/// One level of a menu, drawn with egui's own menu buttons: they highlight under the
/// pointer, open submenus on hover and follow the keyboard.
fn entries(ui: &mut Ui, cx: &Cx, items: &[MenuItem]) {
    let c = cx.c();
    ui.set_min_width(MENU_MIN_WIDTH);
    for item in items {
        match item {
            MenuItem::Separator => {
                ui.separator();
            }
            MenuItem::Header(label) => {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(label.to_uppercase())
                        .size(11.0)
                        .color(c.text_muted),
                );
            }
            MenuItem::Entry {
                label,
                message,
                danger,
            } => {
                let mut text = RichText::new(label.as_str()).size(13.0);
                if *danger {
                    text = text.color(c.danger);
                }
                let response = ui.add_enabled(message.is_some(), egui::Button::new(text));
                if response.clicked()
                    && let Some(message) = message
                {
                    cx.send(Message::FromMenu(Box::new(message.clone())));
                    ui.close();
                }
            }
            MenuItem::Sub { label, children } => {
                ui.menu_button(RichText::new(label.as_str()).size(13.0), |ui| {
                    entries(ui, cx, children);
                });
            }
        }
    }
}

pub fn menu(ui: &mut Ui, cx: &Cx, menu: &Menu) {
    // egui anchors a popup to a widget; this one is a dot where the menu was asked for.
    let anchor = ui.interact(
        Rect::from_min_size(menu.at, vec2(1.0, 1.0)),
        Id::unique("context_menu_anchor"),
        Sense::hover(),
    );
    let mut open = true;
    Popup::menu(&anchor)
        .at_position(menu.at)
        .frame(floating(cx.look(), 8).inner_margin(Margin::same(4)))
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .open_bool(&mut open)
        .show(|ui| entries(ui, cx, &menu.items));
    if !open {
        cx.send(Message::CloseMenu);
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
