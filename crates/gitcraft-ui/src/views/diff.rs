//! A virtualised diff, unified or side by side, with wrapped lines and line and hunk staging.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align, Align2, Color32, CursorIcon, FontId, Galley, Layout, Pos2, Rect, Sense, Stroke, Ui,
    Vec2, vec2,
};
use gitcraft_core::model::{DiffLine, FileDiff};

use super::widgets::{
    bar, btn, danger_text, fill, hline, paint_text, placeholder, primary, region, scroll,
    segmented, split_x, text,
};
use super::{Cx, icon};
use crate::app::{DIFF_ROW_HEIGHT, DiffPane, DiffRow, Message, PaneKey};
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

fn row_height(row: &DiffRow) -> f32 {
    match row {
        DiffRow::Hunk(_) => HUNK_HEIGHT,
        DiffRow::Line(..) | DiffRow::Split(..) => DIFF_ROW_HEIGHT,
    }
}

/// Where each row starts, plus the total at the end. Lines don't wrap, so only the rows
/// matter; it's kept until they change, as it covers the whole diff.
fn with_tops<R>(pane: &DiffPane, use_tops: impl FnOnce(&[f32]) -> R) -> R {
    let mut cache = pane.layout.borrow_mut();
    if cache.0 < 0.0 || cache.1 != pane.rows.len() {
        cache.2.clear();
        let mut top = 0.0;
        for row in &pane.rows {
            cache.2.push(top);
            top += row_height(row);
        }
        cache.2.push(top);
        cache.0 = 0.0;
        cache.1 = pane.rows.len();
    }
    use_tops(&cache.2)
}

/// The widest line, in columns: the diff scrolls sideways to show it.
fn widest(pane: &DiffPane) -> usize {
    if let Some(known) = pane.max_cols.get() {
        return known;
    }
    let widest = pane.diff.as_ref().map_or(0, |diff| {
        diff.hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .map(|line| columns(&line.text))
            .max()
            .unwrap_or(0)
    });
    pane.max_cols.set(Some(widest));
    widest
}

/// The first row to draw for a scroll position.
fn first_visible(tops: &[f32], offset: f32) -> usize {
    tops.partition_point(|top| *top <= offset).saturating_sub(1)
}

fn line_background(look: &Look, tag: char, selected: bool) -> Option<Color32> {
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

/// The text of a line laid out in the given width, in colour when highlighting has finished.
fn code(
    ui: &Ui,
    look: &Look,
    pane: &DiffPane,
    hunk: usize,
    index: usize,
    line: &DiffLine,
) -> Arc<Galley> {
    let font = FontId::monospace(CODE_SIZE);
    let format = |color: Color32| TextFormat {
        font_id: font.clone(),
        color,
        line_height: Some(DIFF_ROW_HEIGHT),
        ..TextFormat::default()
    };
    let mut job = LayoutJob {
        wrap: TextWrapping::no_max_width(),
        ..LayoutJob::default()
    };
    if line.t == '\\' {
        job.append(&line.text, 0.0, format(look.c.text_muted));
    } else if let Some(runs) = pane
        .highlights
        .as_ref()
        .and_then(|all| all.get(&(hunk, index)))
    {
        for (color, range) in runs {
            if let Some(piece) = line.text.get(range.clone()) {
                job.append(&piece.replace('\t', "    "), 0.0, format(*color));
            }
        }
    } else {
        job.append(&line.text.replace('\t', "    "), 0.0, format(look.c.text));
    }
    ui.painter().layout_job(job)
}

fn number(painter: &egui::Painter, look: &Look, right: f32, top: f32, value: Option<u32>) {
    if let Some(value) = value {
        painter.text(
            Pos2::new(right, top + DIFF_ROW_HEIGHT / 2.0),
            Align2::RIGHT_CENTER,
            value,
            FontId::monospace(CODE_SIZE),
            look.c.text_faint,
        );
    }
}

struct Lines<'a> {
    cx: &'a Cx<'a>,
    pane: &'a DiffPane,
    key: PaneKey,
    selectable: bool,
}

impl Lines<'_> {
    fn line(&self, hunk: usize, index: usize) -> &DiffLine {
        &self.pane.diff.as_ref().expect("rows imply a diff").hunks[hunk].lines[index]
    }

    fn click(&self, ui: &mut Ui, rect: Rect, hunk: usize, index: usize, tag: char) {
        if !(self.selectable && matches!(tag, '+' | '-')) {
            return;
        }
        let response = ui.interact(
            rect,
            ui.scope_id().with(("line", hunk, index)),
            Sense::click(),
        );
        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if response.clicked() {
            self.cx
                .send(Message::DiffLineClicked(self.key, hunk, index));
        }
    }

    fn unified(&self, ui: &mut Ui, rect: Rect, hunk: usize, index: usize) {
        let look = self.cx.look();
        let line = self.line(hunk, index);
        let background =
            line_background(look, line.t, self.pane.selection.contains(&(hunk, index)));
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        if let Some(background) = background {
            painter.rect_filled(rect, 0.0, background);
        }
        number(
            &painter,
            look,
            rect.left() + NUMBER_WIDTH - 8.0,
            rect.top(),
            line.old,
        );
        number(
            &painter,
            look,
            rect.left() + NUMBER_WIDTH * 2.0 - 8.0,
            rect.top(),
            line.new,
        );
        let sign = if line.t == '\\' { ' ' } else { line.t };
        painter.text(
            Pos2::new(
                rect.left() + NUMBER_WIDTH * 2.0,
                rect.top() + DIFF_ROW_HEIGHT / 2.0,
            ),
            Align2::LEFT_CENTER,
            sign,
            FontId::monospace(CODE_SIZE),
            look.c.text_muted,
        );
        let left = rect.left() + NUMBER_WIDTH * 2.0 + SIGN_WIDTH;
        let galley = code(ui, look, self.pane, hunk, index, line);
        painter.galley(Pos2::new(left, rect.top()), galley, look.c.text);
        self.click(ui, rect, hunk, index, line.t);
    }

    fn half(&self, ui: &mut Ui, rect: Rect, hunk: usize, index: Option<usize>, left: bool) {
        let look = self.cx.look();
        let c = &look.c;
        let Some(index) = index else {
            fill(ui, rect, c.diff_filler);
            return;
        };
        let line = self.line(hunk, index);
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        if let Some(background) =
            line_background(look, line.t, self.pane.selection.contains(&(hunk, index)))
        {
            painter.rect_filled(rect, 0.0, background);
        }
        number(
            &painter,
            look,
            rect.left() + NUMBER_WIDTH - 8.0,
            rect.top(),
            if left { line.old } else { line.new },
        );
        let bar = match line.t {
            '+' => Some(c.diff_add_text),
            '-' => Some(c.diff_del_text),
            _ => None,
        };
        if let Some(bar) = bar {
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(rect.left() + NUMBER_WIDTH, rect.top()),
                    vec2(SPLIT_BAR, rect.height()),
                ),
                0.0,
                bar,
            );
        }
        let text_left = rect.left() + NUMBER_WIDTH + SPLIT_BAR + 6.0;
        let galley = code(ui, look, self.pane, hunk, index, line);
        painter.galley(Pos2::new(text_left, rect.top()), galley, c.text);
        self.click(ui, rect, hunk, index, line.t);
    }

    #[allow(clippy::too_many_arguments)]
    fn hunk_header(
        &self,
        ui: &mut Ui,
        rect: Rect,
        visible_right: f32,
        index: usize,
        stage_mode: Option<&'static str>,
        show_title: bool,
        show_buttons: bool,
    ) {
        let look = self.cx.look();
        let c = &look.c;
        let hunk = &self.pane.diff.as_ref().expect("rows imply a diff").hunks[index];
        fill(ui, rect, c.diff_hunk_bg);
        ui.painter()
            .hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, c.border));
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, c.border),
        );
        let title = format!(
            "@@ -{},{} +{},{} @@",
            hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
        );
        let mid = rect.center().y;
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        if show_title {
            let mut x = rect.left() + 10.0;
            x += paint_text(
                &painter,
                Pos2::new(x, mid),
                &title,
                look.mono(12.0),
                c.text_muted,
            ) + 6.0;
            paint_text(
                &painter,
                Pos2::new(x, mid),
                &hunk.context,
                look.mono(12.0),
                c.text,
            );
        }

        let count = self.pane.selected_in(index).len();
        let name = |verb: &str| {
            if count > 0 {
                format!("{verb} {}", plural(count, "line"))
            } else {
                format!("{verb} hunk")
            }
        };
        let Some(mode) = stage_mode.filter(|_| show_buttons) else {
            return;
        };
        let slot = Rect::from_min_max(
            Pos2::new((visible_right - 420.0).max(rect.left()), rect.top() + 4.0),
            Pos2::new(visible_right - 10.0, rect.bottom() - 4.0),
        );
        region(ui, slot, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if mode == "stage" {
                    if primary(ui, look, &name("Stage"), true) {
                        self.cx.send(Message::HunkAction(index, "stage"));
                    }
                    if danger_text(ui, look, &name("Discard"), true) {
                        self.cx.send(Message::HunkAction(index, "discard"));
                    }
                } else if btn(ui, look, &name("Unstage"), true) {
                    self.cx.send(Message::HunkAction(index, "unstage"));
                }
            });
        });
    }
}

/// `stage_mode`: `None` is read-only, otherwise "stage" or "unstage". `actions` are extra header buttons.
#[allow(clippy::too_many_arguments)]
pub fn view(
    ui: &mut Ui,
    cx: &Cx,
    pane: &DiffPane,
    key: PaneKey,
    stage_mode: Option<&'static str>,
    empty: &str,
    actions: Option<&dyn Fn(&mut Ui)>,
    available_width: f32,
) {
    let look = cx.look();
    let c = &look.c;
    fill(ui, ui.max_rect(), c.bg);
    if pane.loading {
        placeholder(ui, look, "Loading diff…");
        return;
    }
    let Some(diff) = &pane.diff else {
        placeholder(ui, look, empty);
        return;
    };
    let (added, removed) = stats(diff);
    let title = match (&diff.old_path, &diff.new_path) {
        (Some(old), Some(new)) if old != new => format!("{old} → {new}"),
        (_, Some(new)) => new.clone(),
        (Some(old), None) => old.clone(),
        _ => String::new(),
    };
    let split = cx.app.settings.diff_split;
    bar(ui, 35.0, c.panel, 12.0, |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        icon::icon(ui, "file", 14.0, c.text_muted);
        let reserve = if actions.is_some() { 420.0 } else { 220.0 };
        ui.scope(|ui| {
            ui.set_max_width((available_width - reserve).max(80.0));
            text(ui, title, 13.0, c.text);
        });
        ui.spacing_mut().item_spacing.x = 4.0;
        super::widgets::mono(ui, format!("+{added}"), 12.0, c.diff_add_text);
        super::widgets::mono(ui, format!("−{removed}"), 12.0, c.diff_del_text);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(chosen) = segmented(ui, look, ["Unified", "Split"], usize::from(split)) {
                cx.send(Message::SetSplit(chosen == 1));
            }
            if let Some(actions) = actions {
                ui.spacing_mut().item_spacing.x = 6.0;
                actions(ui);
            }
        });
    });
    hline(ui, look);
    if diff.binary {
        placeholder(ui, look, "Binary file — no text diff available");
        return;
    }
    if diff.hunks.is_empty() {
        placeholder(ui, look, "No content changes (mode or rename only)");
        return;
    }

    // Line staging can't work while the diff ignores whitespace: it wouldn't match the file.
    let selectable =
        stage_mode.is_some() && !cx.app.settings.ignore_whitespace && diff.status != "?";
    let shown_mode = stage_mode.filter(|_| selectable);
    let note = if selectable {
        Some(
            "Click lines to select them (Shift+click for a range), then stage or discard just those lines.",
        )
    } else if stage_mode.is_some() {
        Some(if diff.status == "?" {
            "Untracked file: stage the whole file to include it."
        } else {
            "Line staging is disabled while “Ignore whitespace” is on."
        })
    } else {
        None
    };
    let lines = Lines {
        cx,
        pane,
        key,
        selectable,
    };
    let repo = cx
        .app
        .current()
        .map(|tab| tab.path.as_str())
        .unwrap_or_default();
    let longest = with_tops(pane, |tops| tops.last().copied().unwrap_or(0.0));
    // A position past the end would bounce back, showing nothing on the way.
    let requested = pane
        .scroll_to
        .take()
        .map(|offset| offset.clamp(0.0, (longest - pane.viewport.y).max(0.0)));
    let text = widest(pane) as f32 * look.char_width;
    let body = ui.available_rect_before_wrap();
    // Side by side, each side is a pane of its own with its own sideways scroll bar; they
    // scroll up and down together.
    let panes: Vec<(Side, Rect)> = if split {
        let (left, right) = split_x(body, (body.width() / 2.0).floor());
        vec![(Side::Left, left), (Side::Right, right)]
    } else {
        vec![(Side::Whole, body)]
    };
    let mut reported: Vec<(f32, Vec2)> = Vec::new();
    for (side, rect) in panes {
        region(ui, rect, |ui| {
            // The same bars as the history list, but not hidden until the pointer comes near,
            // so that each pane shows its own sideways bar.
            let bars = &mut ui.spacing_mut().scroll;
            bars.dormant_handle_opacity = bars.active_handle_opacity;
            let mut area = scroll(("diff", repo, key as u8, side as u8))
                .scroll([true, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible);
            if let Some(offset) = requested.or(split.then_some(pane.offset)) {
                area = area.vertical_scroll_offset(offset);
            }
            let output = area.show_viewport(ui, |ui, viewport| {
                let origin = ui.max_rect().min;
                // As wide as the longest line needs, and at least as wide as the pane.
                let needed = match side {
                    Side::Whole => NUMBER_WIDTH * 2.0 + SIGN_WIDTH + text + TEXT_MARGIN,
                    _ => NUMBER_WIDTH + SPLIT_BAR + 6.0 + text + TEXT_MARGIN,
                };
                let width = needed.max(viewport.width());
                let seen = origin.x + viewport.min.x;
                ui.set_width(width);
                with_tops(pane, |tops| {
                    let total = tops.last().copied().unwrap_or(0.0);
                    let extra = if diff.truncated { 70.0 } else { 0.0 }
                        + if note.is_some() { 34.0 } else { 0.0 };
                    ui.set_height(total + extra);
                    let rows = pane.rows.len();
                    let first = first_visible(tops, viewport.min.y)
                        .saturating_sub(OVERSCAN)
                        .min(rows);
                    let end =
                        (tops.partition_point(|top| *top < viewport.max.y) + OVERSCAN).min(rows);
                    for index in first..end {
                        let rect = Rect::from_min_max(
                            origin + vec2(0.0, tops[index]),
                            origin + vec2(width, tops[index + 1]),
                        );
                        match pane.rows[index] {
                            DiffRow::Hunk(hunk) => lines.hunk_header(
                                ui,
                                rect,
                                origin.x + viewport.max.x,
                                hunk,
                                shown_mode,
                                side != Side::Right,
                                side != Side::Left,
                            ),
                            DiffRow::Line(hunk, line) => lines.unified(ui, rect, hunk, line),
                            DiffRow::Split(hunk, left, right) => match side {
                                Side::Right => lines.half(ui, rect, hunk, right, false),
                                _ => lines.half(ui, rect, hunk, left, true),
                            },
                        }
                    }
                    if side == Side::Right {
                        return;
                    }
                    let mut y = origin.y + total;
                    if diff.truncated {
                        ui.painter().text(
                            Pos2::new(seen + viewport.width() / 2.0, y + 35.0),
                            Align2::CENTER_CENTER,
                            "Diff truncated — the file is too large to show completely.",
                            look.ui(13.0),
                            c.text_muted,
                        );
                        y += 70.0;
                    }
                    if let Some(note) = note {
                        paint_text(
                            ui.painter(),
                            Pos2::new(seen + 12.0, y + 17.0),
                            note,
                            look.ui(12.0),
                            c.text_muted,
                        );
                    }
                });
            });
            reported.push((
                output.state.clamped_offset().y,
                Vec2::new(output.inner_rect.width(), output.inner_rect.height()),
            ));
        });
    }
    // Whichever side was scrolled leads.
    let (offset, mut size) = reported
        .iter()
        .copied()
        .find(|(offset, _)| (offset - pane.offset).abs() > 0.5)
        .or_else(|| reported.first().copied())
        .unwrap_or((pane.offset, pane.viewport));
    if split {
        size.x = body.width();
    }
    if (offset - pane.offset).abs() > 0.5 || (size - pane.viewport).abs().max_elem() > 0.5 {
        cx.send(Message::DiffScrolled(key, offset, size));
    }
}

/// The part of a diff a scroll area shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Whole,
    Left,
    Right,
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
