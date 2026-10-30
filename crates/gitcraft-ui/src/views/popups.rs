//! Things drawn above the page: forms, context menus and toasts.

use iced::widget::text::Wrapping;
use iced::widget::{
    button, checkbox, column, container, mouse_area, opaque, pick_list, pin, row, scrollable,
    stack, text, text_editor, text_input,
};
use iced::{Background, Color, Element, Length, Right, Theme};
use native_theme_iced::styles;

use super::widgets::{self, spacer};
use crate::app::{App, Message, Toast, ToastKind};
use crate::form::{FieldKind, Form};
use crate::menu::{Menu, MenuItem};

const MENU_WIDTH: f32 = 320.0;
const MENU_ROW: f32 = 26.0;
const MENU_SEPARATOR: f32 = 9.0;

/// A dimmed backdrop for a dialog. Clicks outside do nothing, so work isn't lost by accident.
pub fn modal<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    opaque(
        container(opaque(content))
            .center(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.35,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            }),
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
    let mut body = column![text(form.title.as_str()).size(size + 3.0)].spacing(10);
    if let Some(message) = &form.message {
        body = body.push(text(message.as_str()).size(size));
    }
    for (index, field) in form.fields.iter().enumerate() {
        let required = if field.required { " *" } else { "" };
        let label = || widgets::muted(format!("{}{required}", field.label), size - 1.0);
        let control: Element<'a, Message> = match &field.kind {
            FieldKind::Text(value) => column![
                label(),
                text_input(&field.placeholder, value)
                    .on_input(move |text| Message::FormText(index, text))
                    .on_submit(Message::FormSubmit)
                    .size(size)
                    .style(styles::text_input(&look.resolved)),
            ]
            .spacing(3)
            .into(),
            FieldKind::Area(content) => column![
                label(),
                text_editor(content)
                    .on_action(move |action| Message::FormArea(index, action))
                    .height(120)
                    .size(size)
                    .style(styles::text_editor(&look.resolved)),
            ]
            .spacing(3)
            .into(),
            FieldKind::Check(value) => checkbox(*value)
                .label(field.label.as_str())
                .text_size(size)
                .on_toggle(move |checked| Message::FormCheck(index, checked))
                .style(styles::checkbox(&look.resolved))
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
                    .style(styles::pick_list(&look.resolved))
                    .menu_style(styles::menu(&look.resolved)),
                ]
                .spacing(3)
                .into()
            }
        };
        body = body.push(control);
    }
    let submit = form.can_submit().then_some(Message::FormSubmit);
    let ok = if form.danger {
        widgets::danger(look, form.ok.as_str(), submit)
    } else {
        widgets::primary(look, form.ok.as_str(), submit)
    };
    body = body.push(
        row![
            spacer(),
            widgets::btn(look, "Cancel", Some(Message::FormCancel)),
            ok
        ]
        .spacing(8),
    );
    modal(
        container(
            scrollable(container(body).padding(18)).style(styles::scrollable(&look.resolved)),
        )
        .max_width(560)
        .max_height((app.window.height / app.scale() - 60.0).max(200.0))
        .style(styles::dialog(&look.resolved))
        .into(),
    )
}

fn height_of(items: &[MenuItem]) -> f32 {
    items
        .iter()
        .map(|item| {
            if matches!(item, MenuItem::Separator) {
                MENU_SEPARATOR
            } else {
                MENU_ROW
            }
        })
        .sum::<f32>()
        + 8.0
}

fn flat_button(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(Background::Color(palette.primary.weak.color)),
            text_color: palette.primary.weak.text,
            ..button::Style::default()
        },
        button::Status::Disabled => button::Style {
            text_color: Color {
                a: 0.4,
                ..palette.background.base.text
            },
            ..button::Style::default()
        },
        button::Status::Active => button::Style {
            text_color: palette.background.base.text,
            ..button::Style::default()
        },
    }
}

fn panel<'a>(app: &'a App, items: &'a [MenuItem], top_level: bool) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    let mut list = column![].width(MENU_WIDTH).padding(4);
    for (index, item) in items.iter().enumerate() {
        match item {
            MenuItem::Separator => {
                list = list.push(
                    container(widgets::hline(look))
                        .height(MENU_SEPARATOR)
                        .center_y(MENU_SEPARATOR),
                );
            }
            MenuItem::Header(label) => {
                list = list.push(
                    container(widgets::muted(label.as_str(), size - 1.0))
                        .height(MENU_ROW)
                        .padding([4, 8]),
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
                let color = danger.then_some(look.palette.removed);
                let entry = button(
                    text(label.as_str())
                        .size(size)
                        .color_maybe(color)
                        .wrapping(Wrapping::None),
                )
                .width(Length::Fill)
                .height(MENU_ROW)
                .padding([3, 8])
                .on_press_maybe(wrapped)
                .style(flat_button);
                let mut area = mouse_area(entry);
                if top_level {
                    area = area.on_enter(Message::MenuHover(None));
                }
                list = list.push(area);
            }
            MenuItem::Sub { label, .. } => {
                let open = app.menu.as_ref().and_then(|menu| menu.open_sub) == Some(index);
                let line = row![
                    text(label.as_str()).size(size).wrapping(Wrapping::None),
                    spacer(),
                    text("›").size(size)
                ];
                let entry = button(line)
                    .width(Length::Fill)
                    .height(MENU_ROW)
                    .padding([3, 8])
                    .on_press(Message::MenuHover(Some(index)))
                    .style(move |theme: &Theme, status| {
                        flat_button(
                            theme,
                            if open {
                                button::Status::Hovered
                            } else {
                                status
                            },
                        )
                    });
                list = list.push(mouse_area(entry).on_enter(Message::MenuHover(Some(index))));
            }
        }
    }
    container(list).style(styles::dialog(&look.resolved)).into()
}

pub fn menu<'a>(app: &'a App, menu: &'a Menu) -> Element<'a, Message> {
    let scale = app.scale();
    let (width, height) = (app.window.width / scale, app.window.height / scale);
    let main_height = height_of(&menu.items);
    // Stay on screen: flip left at the right edge, slide up at the bottom.
    let x = if menu.at.x + MENU_WIDTH > width {
        (menu.at.x - MENU_WIDTH).max(0.0)
    } else {
        menu.at.x
    };
    let y = menu.at.y.min(height - main_height).max(0.0);
    let backdrop = mouse_area(container(text("")).width(Length::Fill).height(Length::Fill))
        .on_press(Message::CloseMenu)
        .on_right_press(Message::CloseMenu);
    let mut layers: Vec<Element<'a, Message>> = vec![
        backdrop.into(),
        pin(panel(app, &menu.items, true)).x(x).y(y).into(),
    ];
    if let Some(index) = menu.open_sub
        && let Some(MenuItem::Sub { children, .. }) = menu.items.get(index)
    {
        let above = height_of(&menu.items[..index]) - 4.0;
        let child_height = height_of(children);
        let child_x = if x + 2.0 * MENU_WIDTH > width {
            (x - MENU_WIDTH).max(0.0)
        } else {
            x + MENU_WIDTH - 2.0
        };
        let child_y = (y + above).min(height - child_height).max(0.0);
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
    let size = look.text_size - 1.0;
    let color = match toast.kind {
        ToastKind::Error => look.palette.removed,
        ToastKind::Warning => look.palette.modified,
        ToastKind::Success => look.palette.added,
        ToastKind::Info => look.theme.palette().text,
    };
    let mut body = column![text(toast.message.as_str()).size(size).color(color)]
        .spacing(3)
        .width(Length::Fill);
    if let Some(detail) = toast.detail.as_ref().filter(|detail| !detail.is_empty()) {
        let short: String = detail.chars().take(600).collect();
        body = body.push(text(short).size(size - 1.0));
    }
    container(
        row![
            body,
            button(text("×").size(size))
                .on_press(Message::DismissToast(toast.id))
                .padding([0, 6])
                .style(flat_button)
        ]
        .spacing(6),
    )
    .padding(10)
    .width(380)
    .style(styles::dialog(&look.resolved))
    .into()
}

pub fn toasts(app: &App) -> Element<'_, Message> {
    let mut list = column![].spacing(6).align_x(Right);
    for toast in &app.toasts {
        list = list.push(toast_view(app, toast));
    }
    // Only the toasts themselves take clicks: this layer is otherwise transparent to the page.
    container(list)
        .align_right(Length::Fill)
        .align_bottom(Length::Fill)
        .padding(iced::Padding {
            top: 0.0,
            right: 12.0,
            bottom: 34.0,
            left: 0.0,
        })
        .into()
}
