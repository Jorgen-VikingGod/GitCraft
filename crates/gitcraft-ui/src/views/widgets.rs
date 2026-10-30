//! Small building blocks shared by the views.

use gitcraft_core::model::FileChange;
use iced::widget::text::Wrapping;
use iced::widget::{button, container, hover, mouse_area, row, rule, space, text};
use iced::{Center, Color, Element, Length, Pixels, mouse};

use super::icon::icon;
use super::style::{self, Edge};
use crate::app::{App, Message, Splitter};
use crate::theme::{Colors, Look};

const SMALL_BUTTON: f32 = 24.0;

fn labelled<'a>(look: &Look, label: impl text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(label)
        .size(12)
        .font(look.semibold)
        .wrapping(Wrapping::None)
}

/// The standard small button.
pub fn btn<'a>(
    look: &Look,
    label: impl text::IntoFragment<'a>,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(container(labelled(look, label)).center_y(Length::Fill))
        .on_press_maybe(message)
        .height(SMALL_BUTTON)
        .padding([0, 10])
        .style(style::button(look))
        .into()
}

pub fn primary<'a>(
    look: &Look,
    label: impl text::IntoFragment<'a>,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(container(labelled(look, label)).center_y(Length::Fill))
        .on_press_maybe(message)
        .height(SMALL_BUTTON)
        .padding([0, 10])
        .style(style::button_primary(look))
        .into()
}

/// No frame, in the danger colour: destructive, but not the main action.
pub fn danger_text<'a>(
    look: &Look,
    label: impl text::IntoFragment<'a>,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(container(labelled(look, label)).center_y(Length::Fill))
        .on_press_maybe(message)
        .height(SMALL_BUTTON)
        .padding([0, 10])
        .style(style::ghost(look, look.c.danger))
        .into()
}

pub fn ghost<'a>(
    look: &Look,
    label: impl text::IntoFragment<'a>,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(container(labelled(look, label)).center_y(Length::Fill))
        .on_press_maybe(message)
        .height(SMALL_BUTTON)
        .padding([0, 10])
        .style(style::ghost(look, look.c.text))
        .into()
}

/// A square button with only an icon.
pub fn icon_button<'a>(
    look: &Look,
    name: &str,
    size: f32,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(container(icon(name, size, look.c.text_muted)).center(Length::Fill))
        .on_press_maybe(message)
        .width(26)
        .height(26)
        .padding(0)
        .style(style::ghost(look, look.c.text_muted))
        .into()
}

/// A two-part switch, such as Unified and Split.
pub fn segmented<'a>(
    look: &Look,
    left: (&'a str, bool, Message),
    right: (&'a str, bool, Message),
) -> Element<'a, Message> {
    let part = |(label, active, message): (&'a str, bool, Message), edge| {
        button(
            text(label)
                .size(11.5)
                .font(look.semibold)
                .wrapping(Wrapping::None),
        )
        .padding([2, 9])
        .on_press(message)
        .style(style::segment(look, active, edge))
    };
    container(row![part(left, Edge::First), part(right, Edge::Last)])
        .padding(1)
        .style(style::segmented_frame(look))
        .into()
}

/// A toggle shown as a button; used where one choice stands alone.
pub fn toggle<'a>(
    look: &Look,
    label: impl text::IntoFragment<'a>,
    active: bool,
    message: Message,
) -> Element<'a, Message> {
    button(container(labelled(look, label)).center_y(Length::Fill))
        .on_press(message)
        .height(SMALL_BUTTON)
        .padding([0, 10])
        .style(style::toggle(look, active))
        .into()
}

pub fn single<'a>(
    content: impl text::IntoFragment<'a>,
    size: impl Into<Pixels>,
) -> iced::widget::Text<'a> {
    text(content).size(size).wrapping(Wrapping::None)
}

/// A secondary line of text. The colour is passed in so that it follows the theme.
pub fn muted_text<'a>(
    look: &Look,
    content: impl text::IntoFragment<'a>,
    size: impl Into<Pixels>,
) -> iced::widget::Text<'a> {
    text(content).size(size).color(look.c.text_muted)
}

/// A small heading in capitals, like those above lists.
pub fn heading<'a>(look: &Look, content: &str) -> iced::widget::Text<'a> {
    text(content.to_uppercase())
        .size(11)
        .font(look.semibold)
        .color(look.c.text_muted)
        .wrapping(Wrapping::None)
}

pub fn placeholder<'a>(look: &Look, note: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    container(muted_text(look, note, look.text_size))
        .center(Length::Fill)
        .into()
}

pub fn hline<'a>(look: &Look) -> Element<'a, Message> {
    let color = look.c.border;
    rule::horizontal(1)
        .style(move |_| rule::Style {
            color,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

fn vline<'a>(look: &Look) -> Element<'a, Message> {
    let color = look.c.border;
    rule::vertical(1)
        .style(move |_| rule::Style {
            color,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

pub fn splitter(app: &App, which: Splitter) -> Element<'_, Message> {
    let look = &app.look;
    let horizontal = which == Splitter::History;
    let zone = if horizontal {
        container(hline(look))
            .height(5)
            .width(Length::Fill)
            .center_y(5)
    } else {
        container(vline(look))
            .width(5)
            .height(Length::Fill)
            .center_x(5)
    };
    mouse_area(zone)
        .on_press(Message::SplitterPressed(which))
        .interaction(if horizontal {
            mouse::Interaction::ResizingVertically
        } else {
            mouse::Interaction::ResizingHorizontally
        })
        .into()
}

/// Lights up under the pointer, as list rows do.
pub fn hoverable<'a>(
    look: &Look,
    content: impl Into<Element<'a, Message>>,
    radius: f32,
) -> Element<'a, Message> {
    let tint = container(space())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::pill(Some(look.c.hover), radius));
    hover(content, tint)
}

pub fn status_color(c: &Colors, status: &str) -> Color {
    match status {
        "A" | "?" => c.success,
        "D" | "U" => c.danger,
        "R" | "C" => c.accent,
        _ => c.warning,
    }
}

/// A row in a file list: status letter, folder, name. Clicks are up to the caller.
/// `action` is a button that appears when the row is hovered or selected.
pub fn file_row<'a>(
    look: &Look,
    file: &FileChange,
    selected: bool,
    action: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let c = &look.c;
    let (dir, name) = match file.path.rfind('/') {
        Some(at) => file.path.split_at(at + 1),
        None => ("", file.path.as_str()),
    };
    let letter = if file.status == "?" {
        "+".to_owned()
    } else {
        file.status.clone()
    };
    let text_color = selected.then_some(c.selection_text);
    let dir_color = if selected {
        Color {
            a: 0.75,
            ..c.selection_text
        }
    } else {
        c.text_muted
    };
    let mut path = row![].align_y(Center);
    if let Some(orig) = &file.orig_path {
        path = path.push(single(format!("{orig} → "), 13).color(dir_color));
    }
    if !dir.is_empty() {
        path = path.push(single(dir.to_owned(), 13).color(dir_color));
    }
    path = path.push(single(name.to_owned(), 13).color_maybe(text_color));
    let line = row![
        container(
            single(letter, 11)
                .font(look.mono_bold)
                .color(status_color(c, &file.status))
        )
        .width(16)
        .center_x(16),
        container(path).width(Length::Fill).clip(true),
    ]
    .spacing(8)
    .align_y(Center)
    .height(26)
    .padding([0, 8]);
    let selected_fill = selected.then_some(c.selection);
    let body = container(line)
        .width(Length::Fill)
        .style(style::pill(selected_fill, 4.0));
    let tint: Element<'a, Message> = match action {
        Some(action) => container(
            row![space::horizontal(), action]
                .align_y(Center)
                .padding([0, 6]),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_y(Length::Fill)
        .style(style::pill(Some(c.hover), 4.0))
        .into(),
        None => container(space())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::pill(Some(c.hover), 4.0))
            .into(),
    };
    container(hover(body, tint))
        .padding([0, 4])
        .width(Length::Fill)
        .into()
}

pub fn spacer<'a>() -> Element<'a, Message> {
    space::horizontal().into()
}

/// A column that scrolls vertically, with the app's scrollbar.
pub fn scrolled<'a>(
    look: &Look,
    content: impl Into<Element<'a, Message>>,
) -> iced::widget::Scrollable<'a, Message> {
    iced::widget::scrollable(content)
        .direction(iced::widget::scrollable::Direction::Vertical(
            style::scrollbar(),
        ))
        .style(style::scrollable(look))
}
