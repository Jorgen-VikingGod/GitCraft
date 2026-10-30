//! A virtualised diff, unified or side by side, with wrapped lines and line and hunk staging.

use gitcraft_core::model::{DiffLine, FileDiff};
use iced::widget::text::{LineHeight, Span, Wrapping};
use iced::widget::{column, container, mouse_area, rich_text, row, scrollable, space, text};
use iced::{Center, Color, Element, Length, Right, never};

use super::icon::icon;
use super::style;
use super::widgets::{
    btn, danger_text, hline, muted_text, placeholder, primary, segmented, single, spacer,
};
use crate::app::{App, DIFF_ROW_HEIGHT, DiffPane, DiffRow, Message, PaneKey};
use crate::logic::plural;
use crate::theme::{CODE_SIZE, Look};

const NUMBER_WIDTH: f32 = 46.0;
const SIGN_WIDTH: f32 = 14.0;
const SPLIT_BAR: f32 = 3.0;
const TEXT_MARGIN: f32 = 12.0;
const OVERSCAN: usize = 6;
const HUNK_HEIGHT: f32 = 33.0;
const TAB_WIDTH: usize = 4;

fn stats(diff: &FileDiff) -> (usize, usize) {
    let lines = diff.hunks.iter().flat_map(|hunk| &hunk.lines);
    lines.fold((0, 0), |(added, removed), line| match line.t {
        '+' => (added + 1, removed),
        '-' => (added, removed + 1),
        _ => (added, removed),
    })
}

/// Columns a line takes up: tabs are wider, and so are East Asian characters.
fn columns(line: &str) -> usize {
    line.chars()
        .map(|c| match c {
            '\t' => TAB_WIDTH,
            '\u{2e80}'..='\u{9fff}'
            | '\u{ac00}'..='\u{d7a3}'
            | '\u{f900}'..='\u{faff}'
            | '\u{ff00}'..='\u{ffef}' => 2,
            _ => 1,
        })
        .sum()
}

fn wrapped_lines(line: &DiffLine, per_line: usize) -> usize {
    columns(&line.text).div_ceil(per_line.max(1)).max(1)
}

fn unified_columns(width: f32, char_width: f32) -> usize {
    ((width - NUMBER_WIDTH * 2.0 - SIGN_WIDTH - TEXT_MARGIN) / char_width).max(8.0) as usize
}

fn split_columns(width: f32, char_width: f32) -> usize {
    ((width / 2.0 - NUMBER_WIDTH - SPLIT_BAR - TEXT_MARGIN) / char_width).max(8.0) as usize
}

fn row_height(diff: &FileDiff, row: &DiffRow, unified: usize, split: usize) -> f32 {
    let lines = |hunk: usize, index: usize, per_line: usize| {
        wrapped_lines(&diff.hunks[hunk].lines[index], per_line)
    };
    match *row {
        DiffRow::Hunk(_) => HUNK_HEIGHT,
        DiffRow::Line(hunk, index) => lines(hunk, index, unified) as f32 * DIFF_ROW_HEIGHT,
        DiffRow::Split(hunk, left, right) => {
            let side = |index: Option<usize>| index.map_or(1, |index| lines(hunk, index, split));
            side(left).max(side(right)) as f32 * DIFF_ROW_HEIGHT
        }
    }
}

/// Where each row starts, plus the total at the end. Lines wrap, so this depends on the
/// width; it's kept until the width or the rows change, as it covers the whole diff.
fn with_tops<R>(
    pane: &DiffPane,
    width: f32,
    char_width: f32,
    use_tops: impl FnOnce(&[f32]) -> R,
) -> R {
    let mut cache = pane.layout.borrow_mut();
    if cache.0 != width || cache.1 != pane.rows.len() {
        cache.2.clear();
        let mut top = 0.0;
        if let Some(diff) = &pane.diff {
            let (unified, split) = (
                unified_columns(width, char_width),
                split_columns(width, char_width),
            );
            for row in &pane.rows {
                cache.2.push(top);
                top += row_height(diff, row, unified, split);
            }
        }
        cache.2.push(top);
        cache.0 = width;
        cache.1 = pane.rows.len();
    }
    use_tops(&cache.2)
}

/// The first row to draw for a scroll position.
fn first_visible(tops: &[f32], offset: f32) -> usize {
    tops.partition_point(|top| *top <= offset).saturating_sub(1)
}

fn line_background(look: &Look, tag: char, selected: bool) -> Option<Color> {
    let c = &look.c;
    if selected {
        return Some(c.diff_selected);
    }
    match tag {
        '+' => Some(c.diff_add_bg),
        '-' => Some(c.diff_del_bg),
        _ => None,
    }
}

/// The text of a line, in colour when highlighting has finished.
fn code<'a>(
    look: &Look,
    pane: &'a DiffPane,
    hunk: usize,
    index: usize,
    line: &'a DiffLine,
) -> Element<'a, Message> {
    let height = LineHeight::Absolute(DIFF_ROW_HEIGHT.into());
    if line.t == '\\' {
        return text(line.text.as_str())
            .size(CODE_SIZE)
            .font(look.mono)
            .line_height(height)
            .color(look.c.text_muted)
            .into();
    }
    if let Some(runs) = pane
        .highlights
        .as_ref()
        .and_then(|all| all.get(&(hunk, index)))
    {
        let spans: Vec<Span<'a, std::convert::Infallible, iced::Font>> = runs
            .iter()
            .filter_map(|(color, range)| {
                let piece = line.text.get(range.clone())?;
                Some(Span::new(piece.replace('\t', "    ")).color(*color))
            })
            .collect();
        return rich_text(spans)
            .size(CODE_SIZE)
            .font(look.mono)
            .line_height(height)
            .wrapping(Wrapping::Glyph)
            .on_link_click(never)
            .into();
    }
    text(line.text.replace('\t', "    "))
        .size(CODE_SIZE)
        .font(look.mono)
        .line_height(height)
        .wrapping(Wrapping::Glyph)
        .into()
}

fn number<'a>(look: &Look, value: Option<u32>) -> Element<'a, Message> {
    text(value.map_or_else(String::new, |n| n.to_string()))
        .size(CODE_SIZE)
        .font(look.mono)
        .line_height(LineHeight::Absolute(DIFF_ROW_HEIGHT.into()))
        .color(look.c.text_faint)
        .align_x(Right)
        .width(NUMBER_WIDTH - 8.0)
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
            .interaction(iced::mouse::Interaction::Pointer)
            .on_press(Message::DiffLineClicked(key, hunk, index))
            .into()
    } else {
        content
    }
}

fn unified<'a>(
    app: &'a App,
    pane: &'a DiffPane,
    key: PaneKey,
    hunk: usize,
    index: usize,
    height: f32,
    selectable: bool,
) -> Element<'a, Message> {
    let look = &app.look;
    let line = &pane.diff.as_ref().expect("rows imply a diff").hunks[hunk].lines[index];
    let sign = if line.t == '\\' { ' ' } else { line.t };
    let content = row![
        container(number(look, line.old)).width(NUMBER_WIDTH),
        container(number(look, line.new)).width(NUMBER_WIDTH),
        text(sign.to_string())
            .size(CODE_SIZE)
            .font(look.mono)
            .line_height(LineHeight::Absolute(DIFF_ROW_HEIGHT.into()))
            .color(look.c.text_muted)
            .width(SIGN_WIDTH),
        container(code(look, pane, hunk, index, line))
            .width(Length::Fill)
            .padding(iced::Padding {
                right: TEXT_MARGIN,
                ..iced::Padding::ZERO
            }),
    ];
    let background = line_background(look, line.t, pane.selection.contains(&(hunk, index)));
    let body = container(content)
        .width(Length::Fill)
        .height(height)
        .clip(true)
        .style(style::maybe_fill(background));
    clickable(body.into(), key, hunk, index, line.t, selectable)
}

#[allow(clippy::too_many_arguments)]
fn half<'a>(
    app: &'a App,
    pane: &'a DiffPane,
    key: PaneKey,
    hunk: usize,
    index: Option<usize>,
    left: bool,
    height: f32,
    selectable: bool,
) -> Element<'a, Message> {
    let look = &app.look;
    let c = &look.c;
    let Some(index) = index else {
        return container(space())
            .width(Length::Fill)
            .height(height)
            .style(style::fill(c.diff_filler))
            .into();
    };
    let line = &pane.diff.as_ref().expect("rows imply a diff").hunks[hunk].lines[index];
    let bar = match line.t {
        '+' => Some(c.diff_add_text),
        '-' => Some(c.diff_del_text),
        _ => None,
    };
    let content = row![
        container(number(look, if left { line.old } else { line.new })).width(NUMBER_WIDTH),
        container(space())
            .width(SPLIT_BAR)
            .height(height)
            .style(style::maybe_fill(bar)),
        container(code(look, pane, hunk, index, line))
            .width(Length::Fill)
            .padding(iced::Padding {
                left: 6.0,
                right: TEXT_MARGIN - 6.0,
                ..iced::Padding::ZERO
            }),
    ];
    let background = line_background(look, line.t, pane.selection.contains(&(hunk, index)));
    let body = container(content)
        .width(Length::Fill)
        .height(height)
        .clip(true)
        .style(style::maybe_fill(background));
    clickable(body.into(), key, hunk, index, line.t, selectable)
}

fn hunk_header<'a>(
    app: &'a App,
    pane: &'a DiffPane,
    index: usize,
    stage_mode: Option<&'static str>,
) -> Element<'a, Message> {
    let look = &app.look;
    let c = &look.c;
    let hunk = &pane.diff.as_ref().expect("rows imply a diff").hunks[index];
    let title = format!(
        "@@ -{},{} +{},{} @@",
        hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
    );
    let mut line = row![
        container(
            row![
                single(title, 12).font(look.mono).color(c.text_muted),
                single(hunk.context.as_str(), 12)
                    .font(look.mono)
                    .color(c.text)
            ]
            .spacing(6)
        )
        .width(Length::Fill)
        .clip(true)
    ]
    .spacing(6)
    .align_y(Center)
    .padding([0, 10]);
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
                .push(danger_text(
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
    container(column![
        hline(look),
        container(line).height(Length::Fill).center_y(Length::Fill),
        hline(look)
    ])
    .width(Length::Fill)
    .height(HUNK_HEIGHT)
    .style(style::fill(c.diff_hunk_bg))
    .into()
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
    let c = &look.c;
    let page_style = style::fill(c.bg);
    if pane.loading {
        return container(placeholder(look, "Loading diff…"))
            .style(page_style)
            .into();
    }
    let Some(diff) = &pane.diff else {
        return container(placeholder(look, empty)).style(page_style).into();
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
        icon("file", 14.0, c.text_muted),
        container(single(title, 13).font(look.semibold)).clip(true),
        row![
            single(format!("+{added}"), 12)
                .font(look.mono)
                .color(c.diff_add_text),
            single(format!("−{removed}"), 12)
                .font(look.mono)
                .color(c.diff_del_text)
        ]
        .spacing(4),
        spacer(),
    ]
    .spacing(8)
    .align_y(Center);
    if let Some(actions) = actions {
        header = header.push(actions);
    }
    header = header.push(segmented(
        look,
        ("Unified", !split, Message::SetSplit(false)),
        ("Split", split, Message::SetSplit(true)),
    ));

    let mut page = column![
        container(header)
            .padding([6, 12])
            .width(Length::Fill)
            .style(style::fill(c.panel)),
        hline(look)
    ];
    if diff.binary {
        page = page.push(placeholder(look, "Binary file — no text diff available"));
        return container(page).style(page_style).into();
    }
    if diff.hunks.is_empty() {
        page = page.push(placeholder(
            look,
            "No content changes (mode or rename only)",
        ));
        return container(page).style(page_style).into();
    }

    // Line staging can't work while the diff ignores whitespace: it wouldn't match the file.
    let selectable = stage_mode.is_some() && !app.settings.ignore_whitespace && diff.status != "?";
    let shown_mode = stage_mode.filter(|_| selectable);
    // The window's height is a safe upper bound: the pane's own is only known after it scrolled.
    let window_height = app.window.height / app.scale();
    let (first, end, above, below, heights) =
        with_tops(pane, available_width, look.char_width, |tops| {
            let total = tops.last().copied().unwrap_or(0.0);
            let rows = pane.rows.len();
            // A scroll position left over from a longer diff can't leave the pane blank.
            let offset = pane.offset.min((total - window_height).max(0.0));
            let first = first_visible(tops, offset)
                .saturating_sub(OVERSCAN)
                .min(rows);
            let bottom = offset + window_height;
            let end = (tops.partition_point(|top| *top < bottom) + OVERSCAN).min(rows);
            let heights: Vec<f32> = (first..end)
                .map(|index| tops[index + 1] - tops[index])
                .collect();
            (first, end, tops[first], total - tops[end], heights)
        });

    let mut items: Vec<Element<'a, Message>> = Vec::with_capacity(end - first + 4);
    items.push(space().height(above).into());
    for (diff_row, height) in pane.rows[first..end].iter().zip(heights) {
        items.push(match *diff_row {
            DiffRow::Hunk(index) => hunk_header(app, pane, index, shown_mode),
            DiffRow::Line(hunk, index) => unified(app, pane, key, hunk, index, height, selectable),
            DiffRow::Split(hunk, left, right) => row![
                container(half(app, pane, key, hunk, left, true, height, selectable))
                    .width(Length::FillPortion(1)),
                container(half(app, pane, key, hunk, right, false, height, selectable))
                    .width(Length::FillPortion(1)),
            ]
            .into(),
        });
    }
    items.push(space().height(below).into());
    if diff.truncated {
        items.push(
            container(muted_text(
                look,
                "Diff truncated — the file is too large to show completely.",
                13,
            ))
            .padding(24)
            .center_x(Length::Fill)
            .into(),
        );
    }
    if selectable {
        items.push(container(muted_text(look, "Click lines to select them (Shift+click for a range), then stage or discard just those lines.", 12)).padding([8, 12]).into());
    } else if stage_mode.is_some() {
        let note = if diff.status == "?" {
            "Untracked file: stage the whole file to include it."
        } else {
            "Line staging is disabled while “Ignore whitespace” is on."
        };
        items.push(
            container(muted_text(look, note, 12))
                .padding([8, 12])
                .into(),
        );
    }

    page = page.push(
        scrollable(column(items).width(Length::Fill))
            .id(pane.scroll.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable(look))
            .on_scroll(move |viewport| {
                let bounds = viewport.bounds();
                Message::DiffScrolled(
                    key,
                    viewport.absolute_offset().y,
                    iced::Size::new(bounds.width, bounds.height),
                )
            }),
    );
    container(page).style(page_style).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_columns_like_the_screen_does() {
        assert_eq!(columns("abc"), 3);
        assert_eq!(columns("\tx"), 5);
        assert_eq!(columns("日本"), 4);
    }

    #[test]
    fn finds_the_row_at_a_scroll_position() {
        let tops = [0.0, 33.0, 53.0, 93.0, 126.0, 146.0];
        assert_eq!(first_visible(&tops, 0.0), 0);
        assert_eq!(first_visible(&tops, 32.0), 0);
        assert_eq!(first_visible(&tops, 33.0), 1);
        assert_eq!(first_visible(&tops, 60.0), 2);
        assert_eq!(first_visible(&tops, 10_000.0), 5);
    }
}
