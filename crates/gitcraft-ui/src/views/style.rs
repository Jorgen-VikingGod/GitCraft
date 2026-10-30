//! How widgets look: one place that turns the theme's colours into iced styles.

use iced::border::{self, Radius};
use iced::widget::{self, container, overlay};
use iced::{Background, Border, Color, Shadow, Theme, Vector};

use crate::theme::{Colors, Look};

pub fn fill(color: Color) -> impl Fn(&Theme) -> container::Style + Copy + use<> {
    move |_| container::Style {
        background: Some(Background::Color(color)),
        ..container::Style::default()
    }
}

pub fn maybe_fill(color: Option<Color>) -> impl Fn(&Theme) -> container::Style + Copy + use<> {
    move |_| container::Style {
        background: color.map(Background::Color),
        ..container::Style::default()
    }
}

/// A rounded background, for the selected item in a list.
pub fn pill(
    color: Option<Color>,
    radius: f32,
) -> impl Fn(&Theme) -> container::Style + Copy + use<> {
    move |_| container::Style {
        background: color.map(Background::Color),
        border: border::rounded(radius),
        ..container::Style::default()
    }
}

fn shadow(c: &Colors, blur: f32, y: f32) -> Shadow {
    Shadow {
        color: c.shadow,
        offset: Vector::new(0.0, y),
        blur_radius: blur,
    }
}

/// A floating surface: dialogs, menus and toasts.
pub fn floating(look: &Look, radius: f32) -> impl Fn(&Theme) -> container::Style + Copy + use<> {
    let c = look.c;
    move |_| container::Style {
        background: Some(Background::Color(c.panel)),
        border: Border {
            color: c.border,
            width: 1.0,
            radius: radius.into(),
        },
        shadow: shadow(&c, 28.0, 10.0),
        ..container::Style::default()
    }
}

pub fn dialog(look: &Look) -> impl Fn(&Theme) -> container::Style + Copy + use<> {
    floating(look, 12.0)
}

pub fn card(look: &Look) -> impl Fn(&Theme) -> container::Style + Copy + use<> {
    let c = look.c;
    move |_| container::Style {
        background: Some(Background::Color(c.panel)),
        border: Border {
            color: c.border,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

#[derive(Clone, Copy)]
struct Paint {
    background: Option<Color>,
    text: Color,
    border: Option<Color>,
    radius: Radius,
}

fn painted(paint: Paint) -> widget::button::Style {
    widget::button::Style {
        background: paint.background.map(Background::Color),
        text_color: paint.text,
        border: Border {
            color: paint.border.unwrap_or(Color::TRANSPARENT),
            width: if paint.border.is_some() { 1.0 } else { 0.0 },
            radius: paint.radius,
        },
        ..widget::button::Style::default()
    }
}

fn dimmed(mut style: widget::button::Style, amount: f32) -> widget::button::Style {
    style.text_color.a *= amount;
    if let Some(Background::Color(color)) = &mut style.background {
        color.a *= amount;
    }
    style.border.color.a *= amount;
    style
}

fn brighter(color: Color) -> Color {
    Color {
        r: (color.r * 1.08).min(1.0),
        g: (color.g * 1.08).min(1.0),
        b: (color.b * 1.08).min(1.0),
        a: color.a,
    }
}

/// The standard bordered button.
pub fn button(
    look: &Look,
) -> impl Fn(&Theme, widget::button::Status) -> widget::button::Style + Copy + use<> {
    let c = look.c;
    move |_, status| {
        let base = Paint {
            background: Some(c.panel),
            text: c.text,
            border: Some(c.border),
            radius: 6.0.into(),
        };
        match status {
            widget::button::Status::Hovered | widget::button::Status::Pressed => painted(Paint {
                background: Some(c.hover_strong),
                ..base
            }),
            widget::button::Status::Disabled => dimmed(painted(base), 0.5),
            widget::button::Status::Active => painted(base),
        }
    }
}

fn solid(
    color: Color,
) -> impl Fn(&Theme, widget::button::Status) -> widget::button::Style + Copy + use<> {
    move |_, status| {
        let base = Paint {
            background: Some(color),
            text: Color::WHITE,
            border: Some(color),
            radius: 6.0.into(),
        };
        match status {
            widget::button::Status::Hovered | widget::button::Status::Pressed => painted(Paint {
                background: Some(brighter(color)),
                border: Some(brighter(color)),
                ..base
            }),
            widget::button::Status::Disabled => dimmed(painted(base), 0.5),
            widget::button::Status::Active => painted(base),
        }
    }
}

pub fn button_primary(
    look: &Look,
) -> impl Fn(&Theme, widget::button::Status) -> widget::button::Style + Copy + use<> {
    solid(look.c.accent)
}

pub fn button_danger(
    look: &Look,
) -> impl Fn(&Theme, widget::button::Status) -> widget::button::Style + Copy + use<> {
    solid(look.c.danger)
}

/// No frame until hovered, in the given text colour.
pub fn ghost(
    look: &Look,
    text: Color,
) -> impl Fn(&Theme, widget::button::Status) -> widget::button::Style + Copy + use<> {
    let c = look.c;
    move |_, status| {
        let base = Paint {
            background: None,
            text,
            border: None,
            radius: 6.0.into(),
        };
        match status {
            widget::button::Status::Hovered | widget::button::Status::Pressed => painted(Paint {
                background: Some(c.hover_strong),
                ..base
            }),
            widget::button::Status::Disabled => dimmed(painted(base), 0.45),
            widget::button::Status::Active => painted(base),
        }
    }
}

/// Toolbar and icon buttons: light hover, no frame.
pub fn tool(
    look: &Look,
) -> impl Fn(&Theme, widget::button::Status) -> widget::button::Style + Copy + use<> {
    let c = look.c;
    move |_, status| {
        let base = Paint {
            background: None,
            text: c.text,
            border: None,
            radius: 6.0.into(),
        };
        match status {
            widget::button::Status::Hovered | widget::button::Status::Pressed => painted(Paint {
                background: Some(c.hover),
                ..base
            }),
            widget::button::Status::Disabled => dimmed(painted(base), 0.45),
            widget::button::Status::Active => painted(base),
        }
    }
}

/// The sidebar's Local Changes and All Commits.
pub fn nav_item(
    look: &Look,
    active: bool,
) -> impl Fn(&Theme, widget::button::Status) -> widget::button::Style + Copy + use<> {
    let c = look.c;
    move |_, status| {
        let base = Paint {
            background: None,
            text: c.text,
            border: None,
            radius: 6.0.into(),
        };
        match (active, status) {
            (true, _) => painted(Paint {
                background: Some(c.selection),
                text: c.selection_text,
                ..base
            }),
            (_, widget::button::Status::Hovered | widget::button::Status::Pressed) => {
                painted(Paint {
                    background: Some(c.hover),
                    ..base
                })
            }
            _ => painted(base),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    First,
    Last,
}

/// One part of a segmented control. Only the outer corners are rounded, so that
/// a filled segment stays inside the control's frame.
pub fn segment(
    look: &Look,
    active: bool,
    edge: Edge,
) -> impl Fn(&Theme, widget::button::Status) -> widget::button::Style + Copy + use<> {
    let c = look.c;
    let radius = if edge == Edge::First {
        border::left(5.0)
    } else {
        border::right(5.0)
    };
    move |_, _| {
        if active {
            painted(Paint {
                background: Some(c.accent),
                text: Color::WHITE,
                border: None,
                radius,
            })
        } else {
            painted(Paint {
                background: None,
                text: c.text_muted,
                border: None,
                radius,
            })
        }
    }
}

pub fn segmented_frame(look: &Look) -> impl Fn(&Theme) -> container::Style + Copy + use<> {
    let c = look.c;
    move |_| container::Style {
        background: Some(Background::Color(c.panel)),
        border: Border {
            color: c.border,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..container::Style::default()
    }
}

/// The search box, which holds a borderless field and a chooser.
pub fn search_frame(look: &Look) -> impl Fn(&Theme) -> container::Style + Copy + use<> {
    let c = look.c;
    move |_| container::Style {
        background: Some(Background::Color(c.input)),
        border: Border {
            color: c.border,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

/// A button that stays lit while its choice is on.
pub fn toggle(
    look: &Look,
    active: bool,
) -> impl Fn(&Theme, widget::button::Status) -> widget::button::Style + Copy + use<> {
    let (on, off) = (button_primary(look), button(look));
    move |theme, status| {
        if active {
            on(theme, status)
        } else {
            off(theme, status)
        }
    }
}

/// A row in a context menu.
pub fn menu_entry(
    look: &Look,
    danger: bool,
    open: bool,
) -> impl Fn(&Theme, widget::button::Status) -> widget::button::Style + Copy + use<> {
    let c = look.c;
    move |_, status| {
        let text = if danger { c.danger } else { c.text };
        let base = Paint {
            background: None,
            text,
            border: None,
            radius: 5.0.into(),
        };
        let highlight = if danger { c.danger } else { c.accent };
        match status {
            widget::button::Status::Disabled => painted(Paint {
                text: c.text_faint,
                ..base
            }),
            widget::button::Status::Hovered | widget::button::Status::Pressed => painted(Paint {
                background: Some(highlight),
                text: Color::WHITE,
                ..base
            }),
            widget::button::Status::Active if open => painted(Paint {
                background: Some(highlight),
                text: Color::WHITE,
                ..base
            }),
            widget::button::Status::Active => painted(base),
        }
    }
}

pub fn text_input(
    look: &Look,
) -> impl Fn(&Theme, widget::text_input::Status) -> widget::text_input::Style + Copy + use<> {
    let c = look.c;
    move |_, status| {
        let color = match status {
            widget::text_input::Status::Focused { .. } => c.accent,
            _ => c.border,
        };
        widget::text_input::Style {
            background: Background::Color(c.input),
            border: Border {
                color,
                width: 1.0,
                radius: 6.0.into(),
            },
            icon: c.text_muted,
            placeholder: c.text_faint,
            value: c.text,
            selection: c.diff_selected,
        }
    }
}

/// A field with no frame of its own, inside something that draws one.
pub fn bare_input(
    look: &Look,
) -> impl Fn(&Theme, widget::text_input::Status) -> widget::text_input::Style + Copy + use<> {
    let c = look.c;
    move |_, _| widget::text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: c.text_muted,
        placeholder: c.text_faint,
        value: c.text,
        selection: c.diff_selected,
    }
}

pub fn text_editor(
    look: &Look,
) -> impl Fn(&Theme, widget::text_editor::Status) -> widget::text_editor::Style + Copy + use<> {
    let c = look.c;
    move |_, status| {
        let color = match status {
            widget::text_editor::Status::Focused { .. } => c.accent,
            _ => c.border,
        };
        widget::text_editor::Style {
            background: Background::Color(c.input),
            border: Border {
                color,
                width: 1.0,
                radius: 6.0.into(),
            },
            placeholder: c.text_faint,
            value: c.text,
            selection: c.diff_selected,
        }
    }
}

/// Read-only text that can be selected, and looks like plain text.
pub fn plain_editor(
    look: &Look,
) -> impl Fn(&Theme, widget::text_editor::Status) -> widget::text_editor::Style + Copy + use<> {
    let c = look.c;
    move |_, _| widget::text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        placeholder: c.text_faint,
        value: c.text,
        selection: c.diff_selected,
    }
}

pub fn checkbox(
    look: &Look,
) -> impl Fn(&Theme, widget::checkbox::Status) -> widget::checkbox::Style + Copy + use<> {
    let c = look.c;
    move |_, status| {
        let checked = match status {
            widget::checkbox::Status::Active { is_checked }
            | widget::checkbox::Status::Hovered { is_checked }
            | widget::checkbox::Status::Disabled { is_checked } => is_checked,
        };
        let faded = matches!(status, widget::checkbox::Status::Disabled { .. });
        let accent = Color {
            a: if faded { 0.5 } else { 1.0 },
            ..c.accent
        };
        widget::checkbox::Style {
            background: Background::Color(if checked { accent } else { c.input }),
            icon_color: Color::WHITE,
            border: Border {
                color: if checked { accent } else { c.text_faint },
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: Some(c.text),
        }
    }
}

pub fn pick_list(
    look: &Look,
) -> impl Fn(&Theme, widget::pick_list::Status) -> widget::pick_list::Style + Copy + use<> {
    let c = look.c;
    move |_, status| {
        let color = if matches!(status, widget::pick_list::Status::Opened { .. }) {
            c.accent
        } else {
            c.border
        };
        widget::pick_list::Style {
            text_color: c.text,
            placeholder_color: c.text_faint,
            handle_color: c.text_muted,
            background: Background::Color(c.input),
            border: Border {
                color,
                width: 1.0,
                radius: 6.0.into(),
            },
        }
    }
}

/// The search mode chooser, which sits inside the search box.
pub fn plain_pick_list(
    look: &Look,
) -> impl Fn(&Theme, widget::pick_list::Status) -> widget::pick_list::Style + Copy + use<> {
    let c = look.c;
    move |_, _| widget::pick_list::Style {
        text_color: c.text_muted,
        placeholder_color: c.text_faint,
        handle_color: c.text_muted,
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
    }
}

pub fn menu(look: &Look) -> impl Fn(&Theme) -> overlay::menu::Style + Copy + use<> {
    let c = look.c;
    move |_| overlay::menu::Style {
        background: Background::Color(c.panel),
        border: Border {
            color: c.border,
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: c.text,
        selected_text_color: Color::WHITE,
        selected_background: Background::Color(c.accent),
        shadow: shadow(&c, 28.0, 10.0),
    }
}

pub fn scrollable(
    look: &Look,
) -> impl Fn(&Theme, widget::scrollable::Status) -> widget::scrollable::Style + Copy + use<> {
    let c = look.c;
    move |_, status| {
        let active = !matches!(status, widget::scrollable::Status::Active { .. });
        let thumb = if active { c.text_muted } else { c.text_faint };
        let rail = widget::scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: widget::scrollable::Scroller {
                background: Background::Color(thumb),
                border: border::rounded(4),
            },
        };
        widget::scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: widget::scrollable::AutoScroll {
                background: Background::Color(c.panel),
                border: border::rounded(u32::MAX).color(c.border).width(1),
                shadow: Shadow::default(),
                icon: c.text,
            },
        }
    }
}

pub fn scrollbar() -> widget::scrollable::Scrollbar {
    widget::scrollable::Scrollbar::new()
        .width(8)
        .scroller_width(6)
        .margin(1)
}
