//! Things drawn above the page: forms, context menus and toasts.

use egui::{Align, Align2, Area, Context, Id, Layout, Margin, Order, Rect, Sense, Ui, vec2};

use super::Cx;
use super::widgets::{
    Kind, area_edit, btn, button, check, choice, floating, icon_button, line_edit, text,
};
use crate::app::{Message, Toast, ToastKind};
use crate::form::{FieldKind, Form};
use crate::i18n::tr;
use crate::menu::{Mark, Menu, MenuItem};
use craft_ui::contract::overlay::Anchor;
use craft_ui::contract::{Bind, Props as _, Show as _, ShowContainer as _};
use craft_ui::display::Divider;
use craft_ui::menu::{Menu as CraftMenu, MenuItem as Row};

pub fn form(ctx: &Context, cx: &Cx, form: &Form) {
    let look = cx.look();
    let c = &look.c;
    let size = look.text_size;
    let window = cx.app.window;
    let id = Id::unique("form");
    super::settle(ctx, "form_frames");
    let area = egui::Modal::default_area(id).anchor(Align2::CENTER_TOP, vec2(0.0, window.y * 0.12));
    let response = egui::Modal::new(id)
        .area(area)
        .backdrop_color(c.backdrop)
        .frame(floating(look, 10).inner_margin(Margin::symmetric(20, 18)))
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.spacing_mut().item_spacing = vec2(8.0, 12.0);
            let body = |ui: &mut Ui| {
                ui.set_width(440.0);
                text(ui, form.title.as_str(), 22.0, c.text);
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
                            Kind::Accent
                        };
                        if button(ui, look, &form.ok, kind, form.can_submit()) {
                            cx.send(Message::FormSubmit);
                        }
                        // Laid out from the right: the first choice sits next to OK.
                        for (index, choice) in form.choices.iter().enumerate() {
                            let kind = if choice.danger {
                                Kind::Danger
                            } else {
                                Kind::Normal
                            };
                            if button(ui, look, &choice.label, kind, true) {
                                cx.send(Message::FormChoice(index));
                            }
                        }
                        if btn(ui, look, "Cancel", true) {
                            cx.send(Message::FormCancel);
                        }
                    });
                });
            };
            // The dialog is as tall as its content, and only scrolls past 80% of the window.
            // Without a scroll area its height is exactly its content, so that is used
            // whenever the content (with some room to spare) fits.
            let limit = (window.y * 0.8 - 36.0).max(80.0);
            if estimated_height(form) > limit {
                egui::ScrollArea::vertical()
                    .max_height(limit)
                    .auto_shrink([false, true])
                    .show(ui, body);
            } else {
                body(ui);
            }
        });
    if response.backdrop_response.clicked() {
        cx.send(Message::FormCancel);
    }
}

/// About how tall a form is, generously, to decide whether it needs a scroll area.
fn estimated_height(form: &Form) -> f32 {
    let message = form.message.as_ref().map_or(0.0, |message| {
        (message.chars().count() / 55 + 1) as f32 * 20.0
    });
    let fields: f32 = form
        .fields
        .iter()
        .map(|field| match field.kind {
            FieldKind::Text(_) | FieldKind::Select { .. } => 66.0,
            FieldKind::Area(_) => 150.0,
            FieldKind::Check(_) => 32.0,
        })
        .sum();
    let rows = form.fields.len() + 3;
    34.0 + message + fields + 40.0 + rows as f32 * 12.0
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
            if let Some(chosen) = choice(ui, ("form", index), options, value, ui.available_width())
            {
                cx.send(Message::FormSelect(index, chosen));
            }
        }
    }
}

/// One level of a context menu as rows of craft-ui's menus, the ones the menu bar shows: the
/// same rows, highlight, marks and submenus. Returns the message of the entry chosen at any depth.
fn rows(ui: &mut Ui, cx: &Cx, items: &[MenuItem]) -> Option<Message> {
    let mut chosen = None;
    for item in items {
        match item {
            MenuItem::Separator => {
                Divider::horizontal().show(ui);
            }
            MenuItem::Header(label) => {
                Row::heading(label.to_uppercase()).show(ui);
            }
            MenuItem::Entry {
                label,
                message,
                danger,
                shortcut,
                mark,
            } => {
                let mut row = Row::new(label.clone()).enabled(message.is_some());
                if *danger {
                    let [r, g, b, a] = cx.c().danger.to_srgba_unmultiplied();
                    row = row.tint(craft_theme::Rgba([r, g, b, a]));
                }
                if let Some(chord) = shortcut {
                    row = row.shortcut(chord.display(craft_keymap::Platform::current()));
                }
                row = match mark {
                    Mark::None | Mark::Radio(false) => row,
                    Mark::Check(on) => row.check(*on),
                    Mark::Radio(true) => row.radio_mark(true),
                };
                if row.show(ui).ran {
                    chosen = message.clone();
                }
            }
            MenuItem::Sub { label, children } => {
                let enabled = children.iter().any(enabled);
                CraftMenu::submenu_row_enabled(ui, label.clone(), enabled, |ui, _| {
                    if let Some(message) = rows(ui, cx, children) {
                        chosen = Some(message);
                    }
                });
            }
        }
    }
    chosen
}

/// Whether `item` is, or holds at any depth, an entry that can run.
fn enabled(item: &MenuItem) -> bool {
    match item {
        MenuItem::Entry { message, .. } => message.is_some(),
        MenuItem::Sub { children, .. } => children.iter().any(enabled),
        MenuItem::Header(_) | MenuItem::Separator => false,
    }
}

/// A context menu, where it was asked for: craft-ui's menu, as the menu bar's menus are.
pub fn menu(ui: &mut Ui, cx: &Cx, menu: &Menu) {
    // The menu hangs from a dot where it was asked for. The place is part of the id, so a menu
    // opened somewhere else starts afresh.
    let id = Id::unique(("context_menu", menu.at.x.to_bits(), menu.at.y.to_bits()));
    let anchor = ui.interact(
        Rect::from_min_size(menu.at, vec2(1.0, 1.0)),
        id,
        Sense::hover(),
    );
    let label = menu
        .items
        .iter()
        .find_map(|item| match item {
            MenuItem::Header(label) => Some(label.clone()),
            _ => None,
        })
        .unwrap_or_else(|| tr("Menu").to_owned());
    let mut open = true;
    let shown = CraftMenu::context(label, &anchor)
        .anchor(Anchor::Point(menu.at))
        .open(Bind::bound(&mut open))
        .show(ui, |ui, _| rows(ui, cx, &menu.items));
    match shown.inner.flatten() {
        Some(message) => cx.send(Message::FromMenu(Box::new(message))),
        None if !open || shown.output.closed.is_some() => cx.send(Message::CloseMenu),
        None => {}
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
