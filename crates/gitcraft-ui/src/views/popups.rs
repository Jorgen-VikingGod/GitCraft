//! Things drawn above the page: forms, context menus and toasts.

use iced::widget::text::Wrapping;
use iced::widget::{
    button, checkbox, column, container, mouse_area, opaque, pick_list, pin, row, space, stack,
    text, text_editor, text_input,
};
use iced::{Center, Color, Element, Length, Theme};

use super::icon::icon;
use super::style;
use super::widgets::{self, icon_button, scrolled, spacer};
use crate::app::{App, Message, Toast, ToastKind};
use crate::form::{FieldKind, Form};
use crate::menu::{Menu, MenuItem};

const MENU_MIN_WIDTH: f32 = 210.0;
const MENU_MAX_WIDTH: f32 = 380.0;
const MENU_ROW: f32 = 27.0;
const MENU_HEADER: f32 = 23.0;
const MENU_SEPARATOR: f32 = 9.0;
const MENU_PADDING: f32 = 4.0;
const MENU_BORDER: f32 = 1.0;

fn backdrop<'a>(
    look: &crate::theme::Look,
    content: Element<'a, Message>,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let tinted = container(opaque(content))
        .center_x(Length::Fill)
        .height(Length::Fill)
        .style(style::fill(look.c.backdrop));
    let mut area = mouse_area(tinted);
    if let Some(on_press) = on_press {
        area = area.on_press(on_press);
    }
    opaque(area)
}

/// A dimmed backdrop for a large tool, centred. Clicks outside do nothing, so work isn't lost by accident.
pub fn modal<'a>(look: &crate::theme::Look, content: Element<'a, Message>) -> Element<'a, Message> {
    opaque(
        container(opaque(content))
            .center(Length::Fill)
            .style(style::fill(look.c.backdrop)),
    )
}

#[derive(Clone, PartialEq, Eq)]
struct Choice {
    value: String,
    label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

pub fn form<'a>(app: &'a App, form: &'a Form) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size;
    let mut body = column![text(form.title.as_str()).size(16).font(look.semibold)].spacing(12);
    if let Some(message) = &form.message {
        body = body.push(text(message.as_str()).size(size).color(look.c.text_muted));
    }
    for (index, field) in form.fields.iter().enumerate() {
        let label = || {
            text(field.label.as_str())
                .size(12)
                .font(look.semibold)
                .color(look.c.text_muted)
        };
        let control: Element<'a, Message> = match &field.kind {
            FieldKind::Text(value) => column![
                label(),
                text_input(&field.placeholder, value)
                    .on_input(move |text| Message::FormText(index, text))
                    .on_submit(Message::FormSubmit)
                    .size(size)
                    .padding([6, 8])
                    .style(style::text_input(look)),
            ]
            .spacing(4)
            .into(),
            FieldKind::Area(content) => column![
                label(),
                text_editor(content)
                    .on_action(move |action| Message::FormArea(index, action))
                    .height(120)
                    .size(size)
                    .padding([6, 8])
                    .style(style::text_editor(look)),
            ]
            .spacing(4)
            .into(),
            FieldKind::Check(value) => checkbox(*value)
                .label(field.label.as_str())
                .text_size(size)
                .size(14)
                .spacing(8)
                .on_toggle(move |checked| Message::FormCheck(index, checked))
                .style(style::checkbox(look))
                .into(),
            FieldKind::Select { options, value } => {
                let choices: Vec<Choice> = options
                    .iter()
                    .map(|(value, label)| Choice {
                        value: value.clone(),
                        label: label.clone(),
                    })
                    .collect();
                let selected = choices
                    .iter()
                    .find(|choice| choice.value == *value)
                    .cloned();
                column![
                    label(),
                    pick_list(
                        choices,
                        selected,
                        move |choice: Choice| Message::FormSelect(index, choice.value)
                    )
                    .width(Length::Fill)
                    .text_size(size)
                    .padding([6, 8])
                    .style(style::pick_list(look))
                    .menu_style(style::menu(look)),
                ]
                .spacing(4)
                .into()
            }
        };
        body = body.push(control);
    }
    let submit = form.can_submit().then_some(Message::FormSubmit);
    let large = |label: &'a str, message: Option<Message>, kind: u8| {
        let base =
            button(container(text(label).size(size).font(look.semibold)).center_y(Length::Fill))
                .height(30)
                .padding([0, 14])
                .on_press_maybe(message);
        match kind {
            0 => base.style(style::button(look)),
            1 => base.style(style::button_primary(look)),
            _ => base.style(style::button_danger(look)),
        }
    };
    body = body.push(
        row![
            spacer(),
            large("Cancel", Some(Message::FormCancel), 0),
            large(form.ok.as_str(), submit, if form.danger { 2 } else { 1 })
        ]
        .spacing(8),
    );
    let height = app.window.height / app.scale();
    let dialog =
        container(scrolled(look, container(body).padding([18, 20])).height(Length::Shrink))
            .width(480)
            .max_height((height * 0.8).max(200.0))
            .style(style::floating(look, 10.0));
    // Forms sit in the upper part of the window, not in its middle.
    backdrop(
        look,
        column![space().height(height * 0.12), dialog].into(),
        Some(Message::FormCancel),
    )
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
    rows + 2.0 * (MENU_PADDING + MENU_BORDER)
}

/// As wide as the longest entry, as a menu sized to its content would be.
fn width_of(items: &[MenuItem]) -> f32 {
    let longest = items
        .iter()
        .map(|item| {
            let extra = if matches!(item, MenuItem::Sub { .. }) {
                26.0
            } else {
                0.0
            };
            item.label()
                .map_or(0.0, |label| label.chars().count() as f32 * 6.7)
                + extra
        })
        .fold(0.0, f32::max);
    (longest + 24.0 + 2.0 * (MENU_PADDING + MENU_BORDER)).clamp(MENU_MIN_WIDTH, MENU_MAX_WIDTH)
}

fn panel<'a>(app: &'a App, items: &'a [MenuItem], top_level: bool) -> Element<'a, Message> {
    let look = &app.look;
    let c = &look.c;
    let mut list = column![].width(Length::Fill);
    for (index, item) in items.iter().enumerate() {
        match item {
            MenuItem::Separator => {
                list = list.push(
                    container(widgets::hline(look))
                        .height(MENU_SEPARATOR)
                        .center_y(MENU_SEPARATOR)
                        .padding([0, 6]),
                );
            }
            MenuItem::Header(label) => {
                list = list.push(
                    container(single_clip(
                        widgets::heading(look, label).font(look.bold).into(),
                    ))
                    .height(MENU_HEADER)
                    .padding(iced::Padding {
                        top: 6.0,
                        bottom: 2.0,
                        left: 12.0,
                        right: 12.0,
                    }),
                );
            }
            MenuItem::Entry {
                label,
                message,
                danger,
            } => {
                let wrapped = message
                    .clone()
                    .map(|message| Message::FromMenu(Box::new(message)));
                let entry = button(
                    container(text(label.as_str()).size(13).wrapping(Wrapping::None))
                        .center_y(Length::Fill)
                        .clip(true),
                )
                .width(Length::Fill)
                .height(MENU_ROW)
                .padding([0, 12])
                .on_press_maybe(wrapped)
                .style(style::menu_entry(look, *danger, false));
                let mut area = mouse_area(entry);
                if top_level {
                    area = area.on_enter(Message::MenuHover(None));
                }
                list = list.push(area);
            }
            MenuItem::Sub { label, .. } => {
                let open = app.menu.as_ref().and_then(|menu| menu.open_sub) == Some(index);
                let arrow = if open { Color::WHITE } else { c.text_muted };
                let line = row![
                    container(text(label.as_str()).size(13).wrapping(Wrapping::None))
                        .width(Length::Fill)
                        .clip(true),
                    icon("chevron-right", 12.0, arrow),
                ]
                .align_y(Center)
                .height(Length::Fill);
                let entry = button(line)
                    .width(Length::Fill)
                    .height(MENU_ROW)
                    .padding([0, 12])
                    .on_press(Message::MenuHover(Some(index)))
                    .style(style::menu_entry(look, false, open));
                list = list.push(mouse_area(entry).on_enter(Message::MenuHover(Some(index))));
            }
        }
    }
    // The padding keeps highlighted rows clear of the rounded corners.
    container(list)
        .width(width_of(items))
        .padding(MENU_PADDING)
        .style(style::floating(look, 8.0))
        .into()
}

fn single_clip(content: Element<'_, Message>) -> Element<'_, Message> {
    container(content).clip(true).into()
}

pub fn menu<'a>(app: &'a App, menu: &'a Menu) -> Element<'a, Message> {
    let scale = app.scale();
    let (width, height) = (app.window.width / scale, app.window.height / scale);
    let (main_width, main_height) = (width_of(&menu.items), height_of(&menu.items));
    // Stay on screen, with a small margin.
    let x = menu.at.x.min(width - main_width - 4.0).max(4.0);
    let y = menu.at.y.min(height - main_height - 4.0).max(4.0);
    let outside = mouse_area(container(space()).width(Length::Fill).height(Length::Fill))
        .on_press(Message::CloseMenu)
        .on_right_press(Message::CloseMenu);
    let mut layers: Vec<Element<'a, Message>> = vec![
        outside.into(),
        pin(panel(app, &menu.items, true)).x(x).y(y).into(),
    ];
    if let Some(index) = menu.open_sub
        && let Some(MenuItem::Sub { children, .. }) = menu.items.get(index)
    {
        let child_width = width_of(children);
        let above = height_of(&menu.items[..index]) - 2.0 * (MENU_PADDING + MENU_BORDER);
        // Opens to the left when there's no room on the right.
        let child_x = if x + main_width + child_width > width - 4.0 {
            (x - child_width + 2.0).max(4.0)
        } else {
            x + main_width - 2.0
        };
        let child_y = (y + above).min(height - height_of(children) - 4.0).max(4.0);
        layers.push(
            pin(panel(app, children, false))
                .x(child_x)
                .y(child_y)
                .into(),
        );
    }
    opaque(stack(layers).width(Length::Fill).height(Length::Fill))
}

fn toast_view<'a>(app: &'a App, toast: &'a Toast) -> Element<'a, Message> {
    let look = &app.look;
    let c = &look.c;
    let bar = match toast.kind {
        ToastKind::Error => c.danger,
        ToastKind::Warning => c.warning,
        ToastKind::Success => c.success,
        ToastKind::Info => c.accent,
    };
    let mut body = column![text(toast.message.as_str()).size(13).font(look.semibold)]
        .spacing(6)
        .width(Length::Fill);
    if let Some(detail) = toast.detail.as_ref().filter(|detail| !detail.is_empty()) {
        let short: String = detail.chars().take(600).collect();
        body = body.push(text(short).size(11).font(look.mono).color(c.text_muted));
    }
    let strip = container(space())
        .width(4)
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(iced::Background::Color(bar)),
            border: iced::border::rounded(iced::border::left(8.0)),
            ..container::Style::default()
        });
    let line = row![
        strip,
        container(
            row![
                body,
                icon_button(look, "x", 13.0, Some(Message::DismissToast(toast.id)))
            ]
            .spacing(8)
        )
        .padding(iced::Padding {
            top: 10.0,
            bottom: 10.0,
            left: 10.0,
            right: 8.0
        }),
    ]
    .height(Length::Shrink);
    container(line)
        .width(420)
        .style(style::floating(look, 8.0))
        .into()
}

pub fn toasts(app: &App) -> Element<'_, Message> {
    let mut list = column![].spacing(8);
    for toast in &app.toasts {
        list = list.push(toast_view(app, toast));
    }
    // Only the toasts themselves take clicks: this layer is otherwise transparent to the page.
    container(list)
        .align_left(Length::Fill)
        .align_bottom(Length::Fill)
        .padding(iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: 36.0,
            left: 16.0,
        })
        .into()
}
