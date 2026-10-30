//! A virtualised diff, unified or side by side, with line and hunk staging.

use gitcraft_core::model::{DiffLine, FileDiff};
use iced::widget::text::{Span, Wrapping};
use iced::widget::{column, container, mouse_area, rich_text, row, scrollable, space, text};
use iced::{Background, Center, Color, Element, Font, Length, Theme, never};
use native_theme_iced::{selection_background, styles};

use super::widgets::{btn, danger, hline, muted, placeholder, primary, single, spacer, toggle};
use crate::app::{App, DIFF_ROW_HEIGHT, DiffPane, DiffRow, Message, PaneKey};
use crate::logic::plural;
use crate::theme::Look;

const CODE_SIZE: f32 = 13.0;
const CHAR_WIDTH: f32 = 7.9;
const NUMBER_WIDTH: f32 = 46.0;
const SIGN_WIDTH: f32 = 14.0;
const OVERSCAN: usize = 6;
const HUNK_HEIGHT: f32 = DIFF_ROW_HEIGHT + 8.0;
/// Space for the fixed columns of a unified row, and a margin.
const GUTTER: f32 = NUMBER_WIDTH * 2.0 + SIGN_WIDTH + 40.0;

fn stats(diff: &FileDiff) -> (usize, usize) {
    let lines = diff.hunks.iter().flat_map(|hunk| &hunk.lines);
    lines.fold((0, 0), |(added, removed), line| match line.t {
        '+' => (added + 1, removed),
        '-' => (added, removed + 1),
        _ => (added, removed),
    })
}

fn line_background(look: &Look, tag: char, selected: bool) -> Option<Color> {
    if selected {
        return Some(Color {
            a: 0.45,
            ..selection_background(&look.resolved)
        });
    }
    match tag {
        '+' => Some(Color {
            a: 0.16,
            ..look.palette.added
        }),
        '-' => Some(Color {
            a: 0.16,
            ..look.palette.removed
        }),
        _ => None,
    }
}

/// The text of a line, in colour when highlighting has finished.
fn code<'a>(
    pane: &'a DiffPane,
    hunk: usize,
    index: usize,
    line: &'a DiffLine,
) -> Element<'a, Message> {
    if line.t == '\\' {
        return muted(line.text.as_str(), CODE_SIZE - 1.0)
            .font(Font::MONOSPACE)
            .wrapping(Wrapping::None)
            .into();
    }
    if let Some(runs) = pane
        .highlights
        .as_ref()
        .and_then(|all| all.get(&(hunk, index)))
    {
        let spans: Vec<Span<'a, std::convert::Infallible, Font>> = runs
            .iter()
            .filter_map(|(color, range)| {
                line.text
                    .get(range.clone())
                    .map(|piece| Span::new(piece).color(*color))
            })
            .collect();
        return rich_text(spans)
            .size(CODE_SIZE)
            .font(Font::MONOSPACE)
            .wrapping(Wrapping::None)
            .on_link_click(never)
            .into();
    }
    text(line.text.as_str())
        .size(CODE_SIZE)
        .font(Font::MONOSPACE)
        .wrapping(Wrapping::None)
        .into()
}

fn number<'a>(value: Option<u32>) -> Element<'a, Message> {
    muted(
        value.map_or_else(String::new, |n| n.to_string()),
        CODE_SIZE - 1.0,
    )
    .font(Font::MONOSPACE)
    .width(NUMBER_WIDTH)
    .wrapping(Wrapping::None)
    .into()
}

fn clickable<'a>(
    content: Element<'a, Message>,
    key: PaneKey,
    hunk: usize,
    index: usize,
    tag: char,
    selectable: bool,
) -> Element<'a, Message> {
    if selectable && matches!(tag, '+' | '-') {
        mouse_area(content)
            .on_press(Message::DiffLineClicked(key, hunk, index))
            .into()
    } else {
        content
    }
}

fn styled<'a>(
    look: &Look,
    content: Element<'a, Message>,
    tag: char,
    selected: bool,
) -> Element<'a, Message> {
    let background = line_background(look, tag, selected);
    container(content)
        .width(Length::Fill)
        .height(DIFF_ROW_HEIGHT)
        .clip(true)
        .style(move |_: &Theme| container::Style {
            background: background.map(Background::Color),
            ..container::Style::default()
        })
        .into()
}

fn unified<'a>(
    app: &'a App,
    pane: &'a DiffPane,
    key: PaneKey,
    hunk: usize,
    index: usize,
    selectable: bool,
) -> Element<'a, Message> {
    let line = &pane.diff.as_ref().expect("rows imply a diff").hunks[hunk].lines[index];
    let sign = if line.t == '\\' { ' ' } else { line.t };
    let content = row![
        number(line.old),
        number(line.new),
        single(sign.to_string(), CODE_SIZE)
            .font(Font::MONOSPACE)
            .width(SIGN_WIDTH),
        code(pane, hunk, index, line),
    ]
    .align_y(Center)
    .padding([0, 4]);
    let selected = pane.selection.contains(&(hunk, index));
    clickable(
        styled(&app.look, content.into(), line.t, selected),
        key,
        hunk,
        index,
        line.t,
        selectable,
    )
}

fn half<'a>(
    app: &'a App,
    pane: &'a DiffPane,
    key: PaneKey,
    hunk: usize,
    index: Option<usize>,
    left: bool,
    selectable: bool,
) -> Element<'a, Message> {
    let Some(index) = index else {
        return container(space())
            .width(Length::Fill)
            .height(DIFF_ROW_HEIGHT)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.06,
                    ..theme.palette().text
                })),
                ..container::Style::default()
            })
            .into();
    };
    let line = &pane.diff.as_ref().expect("rows imply a diff").hunks[hunk].lines[index];
    let content = row![
        number(if left { line.old } else { line.new }),
        code(pane, hunk, index, line)
    ]
    .align_y(Center)
    .padding([0, 4]);
    let selected = pane.selection.contains(&(hunk, index));
    clickable(
        styled(&app.look, content.into(), line.t, selected),
        key,
        hunk,
        index,
        line.t,
        selectable,
    )
}

fn hunk_header<'a>(
    app: &'a App,
    pane: &'a DiffPane,
    index: usize,
    stage_mode: Option<&'static str>,
) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 2.0;
    let hunk = &pane.diff.as_ref().expect("rows imply a diff").hunks[index];
    let title = format!(
        "@@ -{},{} +{},{} @@ {}",
        hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines, hunk.context
    );
    // Buttons come first, so that long lines scrolling sideways can't push them out of reach.
    let mut line = row![].spacing(6).align_y(Center).padding([0, 6]);
    let count = pane.selected_in(index).len();
    let name = |verb: &str| {
        if count > 0 {
            format!("{verb} {}", plural(count, "line"))
        } else {
            format!("{verb} hunk")
        }
    };
    match stage_mode {
        Some("stage") => {
            line = line
                .push(danger(
                    look,
                    name("Discard"),
                    Some(Message::HunkAction(index, "discard")),
                ))
                .push(primary(
                    look,
                    name("Stage"),
                    Some(Message::HunkAction(index, "stage")),
                ));
        }
        Some(_) => {
            line = line.push(btn(
                look,
                name("Unstage"),
                Some(Message::HunkAction(index, "unstage")),
            ))
        }
        None => {}
    }
    line = line.push(
        muted(title, size)
            .font(Font::MONOSPACE)
            .wrapping(Wrapping::None),
    );
    container(line)
        .width(Length::Fill)
        .height(HUNK_HEIGHT)
        .center_y(HUNK_HEIGHT)
        .clip(true)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(Color {
                a: 0.07,
                ..theme.palette().text
            })),
            ..container::Style::default()
        })
        .into()
}

fn height_of(rows: &[DiffRow]) -> f32 {
    rows.iter()
        .map(|row| {
            if matches!(row, DiffRow::Hunk(_)) {
                HUNK_HEIGHT
            } else {
                DIFF_ROW_HEIGHT
            }
        })
        .sum()
}

/// The first row to draw for a scroll position, allowing for the taller hunk headers.
fn first_visible(rows: &[DiffRow], offset: f32) -> usize {
    let mut top = 0.0;
    for (index, row) in rows.iter().enumerate() {
        top += if matches!(row, DiffRow::Hunk(_)) {
            HUNK_HEIGHT
        } else {
            DIFF_ROW_HEIGHT
        };
        if top > offset {
            return index;
        }
    }
    rows.len()
}

/// `stage_mode`: `None` is read-only, otherwise "stage" or "unstage". `actions` are extra header buttons.
pub fn view<'a>(
    app: &'a App,
    pane: &'a DiffPane,
    key: PaneKey,
    stage_mode: Option<&'static str>,
    empty: &'a str,
    actions: Option<Element<'a, Message>>,
    available_width: f32,
) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    if pane.loading {
        return placeholder(look, "Loading diff…");
    }
    let Some(diff) = &pane.diff else {
        return placeholder(look, empty);
    };
    let (added, removed) = stats(diff);
    let title = match (&diff.old_path, &diff.new_path) {
        (Some(old), Some(new)) if old != new => format!("{old} → {new}"),
        (_, Some(new)) => new.clone(),
        (Some(old), None) => old.clone(),
        _ => String::new(),
    };
    let split = app.settings.diff_split;
    let mut header = row![
        container(single(title, size)).clip(true),
        single(format!("+{added}"), size).color(look.palette.added),
        single(format!("−{removed}"), size).color(look.palette.removed),
        spacer(),
    ]
    .spacing(8)
    .align_y(Center)
    .padding([3, 8]);
    if let Some(actions) = actions {
        header = header.push(actions);
    }
    header = header
        .push(toggle(look, "Unified", !split, Message::SetSplit(false)))
        .push(toggle(look, "Split", split, Message::SetSplit(true)));

    let mut page = column![header, hline(look)];
    if diff.binary {
        return page
            .push(placeholder(look, "Binary file — no text diff available"))
            .into();
    }
    if diff.hunks.is_empty() {
        return page
            .push(placeholder(
                look,
                "No content changes (mode or rename only)",
            ))
            .into();
    }

    // Line staging can't work while the diff ignores whitespace: it wouldn't match the file.
    let selectable = stage_mode.is_some() && !app.settings.ignore_whitespace && diff.status != "?";
    let shown_mode = stage_mode.filter(|_| selectable);
    // The window's height is a safe upper bound: the pane's own is only known after it scrolled.
    let count = (app.window.height / app.scale() / DIFF_ROW_HEIGHT) as usize + 2 * OVERSCAN + 1;
    let first = first_visible(&pane.rows, pane.offset)
        .saturating_sub(OVERSCAN)
        .min(pane.rows.len().saturating_sub(count));
    let end = (first + count).min(pane.rows.len());

    let mut items: Vec<Element<'a, Message>> = Vec::with_capacity(end - first + 4);
    items.push(space().height(height_of(&pane.rows[..first])).into());
    for diff_row in &pane.rows[first..end] {
        items.push(match *diff_row {
            DiffRow::Hunk(index) => hunk_header(app, pane, index, shown_mode),
            DiffRow::Line(hunk, index) => unified(app, pane, key, hunk, index, selectable),
            DiffRow::Split(hunk, left, right) => row![
                container(half(app, pane, key, hunk, left, true, selectable))
                    .width(Length::FillPortion(1))
                    .clip(true),
                container(half(app, pane, key, hunk, right, false, selectable))
                    .width(Length::FillPortion(1))
                    .clip(true),
            ]
            .into(),
        });
    }
    items.push(space().height(height_of(&pane.rows[end..])).into());
    if diff.truncated {
        items.push(
            container(muted(
                "Diff truncated — the file is too large to show completely.",
                size,
            ))
            .padding(8)
            .into(),
        );
    }
    if selectable {
        items.push(container(muted("Click lines to select them (Shift+click for a range), then stage or discard just those lines.", size)).padding(8).into());
    } else if stage_mode.is_some() {
        let note = if diff.status == "?" {
            "Untracked file: stage the whole file to include it."
        } else {
            "Line staging is disabled while “Ignore whitespace” is on."
        };
        items.push(container(muted(note, size)).padding(8).into());
    }

    let list = column(items);
    let body = if split {
        scrollable(list.width(Length::Fill)).direction(scrollable::Direction::Vertical(
            styles::scrollbar(&look.resolved),
        ))
    } else {
        // Long lines scroll sideways; the rows are as wide as the longest one.
        let width = (pane.longest as f32 * CHAR_WIDTH + GUTTER).max(available_width);
        scrollable(list.width(width)).direction(scrollable::Direction::Both {
            vertical: styles::scrollbar(&look.resolved),
            horizontal: styles::scrollbar(&look.resolved),
        })
    };
    page = page.push(
        body.id(pane.scroll.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(styles::scrollable(&look.resolved))
            .on_scroll(move |viewport| {
                let bounds = viewport.bounds();
                Message::DiffScrolled(
                    key,
                    viewport.absolute_offset().y,
                    iced::Size::new(bounds.width, bounds.height),
                )
            }),
    );
    page.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_first_row_past_taller_headers() {
        let rows = [
            DiffRow::Hunk(0),
            DiffRow::Line(0, 0),
            DiffRow::Line(0, 1),
            DiffRow::Hunk(1),
            DiffRow::Line(1, 0),
        ];
        assert_eq!(first_visible(&rows, 0.0), 0);
        assert_eq!(first_visible(&rows, HUNK_HEIGHT - 1.0), 0);
        assert_eq!(first_visible(&rows, HUNK_HEIGHT), 1);
        assert_eq!(first_visible(&rows, HUNK_HEIGHT + DIFF_ROW_HEIGHT * 2.0), 3);
        assert_eq!(first_visible(&rows, 10_000.0), 5);
    }
}
