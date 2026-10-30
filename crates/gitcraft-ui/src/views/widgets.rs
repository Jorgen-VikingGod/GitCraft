//! Small building blocks shared by the views.

use gitcraft_core::model::FileChange;
use iced::widget::text::Wrapping;
use iced::widget::{button, container, mouse_area, row, rule, space, text};
use iced::{Background, Center, Color, Element, Length, Theme, mouse};
use native_theme_iced::{selection_background, styles};

use crate::app::{App, Message, Splitter};
use crate::theme::{GitPalette, Look};

pub fn btn<'a>(
    look: &Look,
    label: impl text::IntoFragment<'a>,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(
        text(label)
            .size(look.text_size - 1.0)
            .wrapping(Wrapping::None),
    )
    .on_press_maybe(message)
    .padding([3, 9])
    .style(styles::button(&look.resolved))
    .into()
}

pub fn primary<'a>(
    look: &Look,
    label: impl text::IntoFragment<'a>,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(
        text(label)
            .size(look.text_size - 1.0)
            .wrapping(Wrapping::None),
    )
    .on_press_maybe(message)
    .padding([3, 9])
    .style(styles::button_primary(&look.resolved))
    .into()
}

pub fn danger<'a>(
    look: &Look,
    label: impl text::IntoFragment<'a>,
    message: Option<Message>,
) -> Element<'a, Message> {
    button(
        text(label)
            .size(look.text_size - 1.0)
            .wrapping(Wrapping::None),
    )
    .on_press_maybe(message)
    .padding([3, 9])
    .style(styles::button_danger(&look.resolved))
    .into()
}

pub fn toggle<'a>(
    look: &Look,
    label: impl text::IntoFragment<'a>,
    active: bool,
    message: Message,
) -> Element<'a, Message> {
    button(
        text(label)
            .size(look.text_size - 1.0)
            .wrapping(Wrapping::None),
    )
    .on_press(message)
    .padding([3, 9])
    .style(styles::button_toggle(&look.resolved, active))
    .into()
}

pub fn single<'a>(content: impl text::IntoFragment<'a>, size: f32) -> iced::widget::Text<'a> {
    text(content).size(size).wrapping(Wrapping::None)
}

/// A secondary line of text.
pub fn muted<'a>(content: impl text::IntoFragment<'a>, size: f32) -> iced::widget::Text<'a> {
    text(content).size(size).style(|theme: &Theme| text::Style {
        color: Some(Color {
            a: 0.65,
            ..theme.palette().text
        }),
    })
}

pub fn placeholder<'a>(look: &Look, note: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    container(muted(note, look.text_size))
        .center(Length::Fill)
        .into()
}

pub fn hline<'a>(look: &Look) -> Element<'a, Message> {
    rule::horizontal(1)
        .style(styles::rule(&look.resolved))
        .into()
}

pub fn splitter(app: &App, which: Splitter) -> Element<'_, Message> {
    let look = &app.look;
    let horizontal = which == Splitter::History;
    let line: Element<'_, Message> = if horizontal {
        rule::horizontal(1)
            .style(styles::rule(&look.resolved))
            .into()
    } else {
        rule::vertical(1).style(styles::rule(&look.resolved)).into()
    };
    let zone = if horizontal {
        container(line).height(5).width(Length::Fill).center_y(5)
    } else {
        container(line).width(5).height(Length::Fill).center_x(5)
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

pub fn status_color(palette: &GitPalette, status: &str, fallback: Color) -> Color {
    match status {
        "A" | "?" => palette.added,
        "D" | "U" => palette.removed,
        "M" | "R" | "C" | "T" => palette.modified,
        _ => fallback,
    }
}

/// A row in a file list: status letter, folder, name. Clicks are up to the caller.
pub fn file_row<'a>(look: &Look, file: &FileChange, selected: bool) -> Element<'a, Message> {
    let size = look.text_size - 1.0;
    let fallback = look.theme.palette().text;
    let color = status_color(&look.palette, &file.status, fallback);
    let (dir, name) = match file.path.rfind('/') {
        Some(at) => file.path.split_at(at + 1),
        None => ("", file.path.as_str()),
    };
    let letter = if file.status == "?" {
        "+".to_owned()
    } else {
        file.status.clone()
    };
    let on_row = selected.then_some(look.selected_text);
    let mut line = row![
        single(letter, size)
            .color(on_row.unwrap_or(color))
            .width(16)
    ]
    .spacing(4)
    .align_y(Center)
    .padding([0, 8])
    .height(24);
    if let Some(orig) = &file.orig_path {
        line = line.push(muted_or(on_row, format!("{orig} → "), size));
    }
    if !dir.is_empty() {
        line = line.push(muted_or(on_row, dir.to_owned(), size));
    }
    line = line.push(single(name.to_owned(), size).color_maybe(on_row));
    let background = selected.then(|| selection_background(&look.resolved));
    container(line)
        .width(Length::Fill)
        .clip(true)
        .style(move |_: &Theme| container::Style {
            background: background.map(Background::Color),
            ..container::Style::default()
        })
        .into()
}

fn muted_or<'a>(on_row: Option<Color>, content: String, size: f32) -> iced::widget::Text<'a> {
    match on_row {
        Some(color) => single(content, size).color(color),
        None => muted(content, size).wrapping(Wrapping::None),
    }
}

pub fn spacer<'a>() -> Element<'a, Message> {
    space::horizontal().into()
}
