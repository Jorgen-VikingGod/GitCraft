//! The virtualised commit list with its graph, above the details of the selection.

use egui::{Align, Align2, Layout, Pos2, Rect, Sense, Stroke, Ui, vec2};
use gitcraft_core::model::{Commit, RefLabel};

use super::graph_cell::{GraphCell, width_for};
use super::widgets::{
    badge_paint, badge_width, btn, fill, hline, paint_strong, paint_text, placeholder, region,
    scroll, segmented, split_y, splitter, text,
};
use super::{Cx, details, icon};
use crate::actions::{MenuRequest, RefTarget};
use crate::app::{Message, ROW_HEIGHT, RepoTab, Splitter, View};
use crate::command::Command;
use crate::logic::{plural, short_date, short_sha};

/// Rows drawn beyond the visible ones, so that a small scroll never shows a gap.
const OVERSCAN: usize = 4;
const AUTHOR_WIDTH: f32 = 150.0;
const DATE_WIDTH: f32 = 120.0;
const SHA_WIDTH: f32 = 76.0;

pub fn view(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let look = cx.look();
    let c = &look.c;
    let area = ui.max_rect();
    let (top, rest) = split_y(
        area,
        cx.app.settings.history_height.min(area.height() - 60.0),
    );
    let (grip, bottom) = split_y(rest, super::widgets::GRIP);
    region(ui, top, |ui| {
        fill(ui, top, c.bg);
        top_pane(ui, cx, tab);
    });
    splitter(ui, cx, grip, Splitter::History);
    region(ui, bottom, |ui| {
        if tab.compare.is_some() {
            details::compare(ui, cx, tab);
        } else if tab.selected.is_some() {
            details::commit(ui, cx, tab);
        } else {
            placeholder(ui, look, "Select a commit to see its details");
        }
    });
}

fn top_pane(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let look = cx.look();
    let c = &look.c;
    let graph_width = if tab.search.is_some() {
        10.0
    } else {
        width_for(tab.graph_lanes)
    };
    if let Some(search) = &tab.search {
        let summary = if search.loading {
            "Searching…".to_owned()
        } else {
            format!(
                "{} matching “{}” ({})",
                plural(search.results.len(), "commit"),
                search.query,
                search.mode.key()
            )
        };
        super::widgets::bar(ui, 29.0, c.accent_soft, 10.0, |ui| {
            icon::icon(ui, "search", 13.0, c.text_muted);
            text(ui, summary, 12.0, c.text);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if btn(ui, look, "Clear search", true) {
                    cx.send(Message::SearchClear);
                }
            });
        });
        hline(ui, look);
    } else if tab.change_count() > 0 {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 0.0, c.hover);
        }
        ui.painter().circle_stroke(
            Pos2::new(rect.left() + graph_width / 2.0, rect.center().y),
            4.0,
            Stroke::new(2.0, c.text_muted),
        );
        paint_text(
            ui.painter(),
            Pos2::new(rect.left() + graph_width, rect.center().y),
            "Uncommitted changes",
            look.ui(13.0),
            c.text_muted,
        );
        ui.painter().text(
            Pos2::new(rect.right() - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            plural(tab.change_count(), "file"),
            look.ui(13.0),
            c.text_muted,
        );
        if response.clicked() {
            cx.send(Message::SetView(View::Changes));
        }
        hline(ui, look);
    }
    header(ui, cx, tab, graph_width);
    hline(ui, look);
    list(ui, cx, tab, graph_width);
}

fn header(ui: &mut Ui, cx: &Cx, tab: &RepoTab, graph_width: f32) {
    let look = cx.look();
    let c = &look.c;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 25.0), Sense::hover());
    fill(ui, rect, c.panel);
    let mid = rect.center().y;
    let right = rect.right();
    let sha_x = right - SHA_WIDTH + 4.0;
    let date_x = right - SHA_WIDTH - DATE_WIDTH + 8.0;
    let author_x = right - SHA_WIDTH - DATE_WIDTH - AUTHOR_WIDTH + 8.0;
    for (title, x) in [("COMMIT", sha_x), ("DATE", date_x), ("AUTHOR", author_x)] {
        paint_text(
            ui.painter(),
            Pos2::new(x, mid),
            title,
            look.ui(11.0),
            c.text_muted,
        );
    }
    let description = paint_text(
        ui.painter(),
        Pos2::new(rect.left() + graph_width, mid),
        "DESCRIPTION",
        look.ui(11.0),
        c.text_muted,
    );
    if tab.search.is_none() {
        let slot = Rect::from_min_size(
            Pos2::new(
                rect.left() + graph_width + description + 12.0,
                rect.top() + 1.5,
            ),
            vec2(260.0, 22.0),
        );
        region(ui, slot, |ui| {
            let active = usize::from(!tab.all_branches);
            if let Some(chosen) = segmented(ui, look, ["All branches", "Current branch"], active) {
                cx.send(Message::SetBranchFilter(chosen == 0));
            }
        });
    }
}

fn list(ui: &mut Ui, cx: &Cx, tab: &RepoTab, graph_width: f32) {
    let look = cx.look();
    let commits = tab.visible();
    if commits.is_empty() {
        let note = if tab.loading {
            "Loading history…"
        } else if tab.search.is_some() {
            "No matching commits"
        } else {
            "No commits yet"
        };
        placeholder(ui, look, note);
        return;
    }
    let total = commits.len();
    let head = tab.refs.head.as_deref();
    let more = tab.has_more && tab.search.is_none();
    let mut area = scroll(("history", &tab.path));
    if let Some(offset) = tab.scroll_to.take() {
        // A position past the end would bounce back, showing nothing on the way.
        let longest = total as f32 * ROW_HEIGHT + if more { 44.0 } else { 0.0 };
        area = area.vertical_scroll_offset(offset.clamp(0.0, (longest - tab.viewport).max(0.0)));
    }
    let output = area.show_viewport(ui, |ui, viewport| {
        let origin = ui.max_rect().min;
        let width = ui.max_rect().width();
        let footer = if more { 44.0 } else { 0.0 };
        ui.set_height(total as f32 * ROW_HEIGHT + footer);
        let first = ((viewport.min.y / ROW_HEIGHT) as usize)
            .saturating_sub(OVERSCAN)
            .min(total);
        let end = (((viewport.max.y / ROW_HEIGHT).ceil() as usize) + OVERSCAN).min(total);
        for (index, commit) in commits.iter().enumerate().take(end).skip(first) {
            let rect = Rect::from_min_size(
                origin + vec2(0.0, index as f32 * ROW_HEIGHT),
                vec2(width, ROW_HEIGHT),
            );
            let selected = tab.selected.as_deref() == Some(commit.sha.as_str());
            let compared = tab
                .compare
                .as_ref()
                .is_some_and(|c| c.base == commit.sha || c.target == commit.sha);
            commit_row(
                ui,
                cx,
                rect,
                commit,
                selected && !compared,
                compared,
                Some(commit.sha.as_str()) == head,
                graph_width,
            );
        }
        if more {
            let slot = Rect::from_min_size(
                origin + vec2(0.0, total as f32 * ROW_HEIGHT),
                vec2(width, footer),
            );
            region(ui, slot, |ui| {
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    ui.add_space(10.0);
                    let label = if tab.loading {
                        "Loading…"
                    } else {
                        "Load more commits"
                    };
                    if btn(ui, look, label, !tab.loading) {
                        cx.send(Message::LoadMore);
                    }
                });
            });
        }
    });
    let offset = output.state.clamped_offset().y;
    let height = output.inner_rect.height();
    if (offset - tab.offset).abs() > 0.5 || (height - tab.viewport).abs() > 0.5 {
        cx.send(Message::Scrolled(offset, height));
    }
}

fn row_badge(ui: &mut Ui, cx: &Cx, label: &RefLabel, commit: &Commit, rect: Rect) {
    let target = match label.kind.as_str() {
        "local" => Some(RefTarget::Local(label.name.clone())),
        "remote" => Some(RefTarget::Remote(label.name.clone())),
        "tag" => Some(RefTarget::Tag(label.name.clone())),
        _ => None,
    };
    badge_paint(ui, cx.look(), label, rect);
    let response = ui.interact(
        rect,
        ui.scope_id()
            .with(("badge", &commit.sha, &label.name, &label.kind)),
        Sense::click(),
    );
    let checkout = |kind: &'static str| {
        Message::Run(Command::Checkout {
            reference: label.name.clone(),
            kind,
        })
    };
    if response.double_clicked() && label.kind == "local" && !label.current {
        cx.send(checkout("local"));
    } else if response.double_clicked() && label.kind == "remote" {
        cx.send(checkout("remote"));
    } else if response.clicked() {
        cx.send(Message::Select(commit.sha.clone()));
    } else if response.secondary_clicked() {
        cx.send(Message::OpenMenu(MenuRequest::Commit {
            sha: commit.sha.clone(),
            focus: target,
        }));
    }
}

#[allow(clippy::too_many_arguments)]
fn commit_row(
    ui: &mut Ui,
    cx: &Cx,
    rect: Rect,
    commit: &Commit,
    selected: bool,
    compared: bool,
    is_head: bool,
    graph_width: f32,
) {
    let look = cx.look();
    let c = &look.c;
    let (main, secondary) = if selected {
        (c.selection_text, c.selection_text)
    } else {
        (c.text, c.text_muted)
    };
    let response = ui.interact(
        rect,
        ui.scope_id().with(("commit", &commit.sha)),
        Sense::click(),
    );
    if selected {
        fill(ui, rect, c.selection);
    } else if compared {
        fill(ui, rect, c.accent_soft);
    } else if ui.rect_contains_pointer(rect) {
        fill(ui, rect, c.hover);
    }
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let mid = rect.center().y;
    if let Some(graph) = &commit.graph {
        GraphCell {
            row: graph,
            lanes: c.lanes,
            background: if selected { c.selection } else { c.bg },
            merge: commit.parents.len() > 1,
            head: is_head,
        }
        .paint(&painter, rect.min);
    }
    let sha_x = rect.right() - SHA_WIDTH + 4.0;
    let date_x = rect.right() - SHA_WIDTH - DATE_WIDTH + 8.0;
    let author_x = rect.right() - SHA_WIDTH - DATE_WIDTH - AUTHOR_WIDTH + 8.0;
    paint_text(
        &painter,
        Pos2::new(sha_x, mid),
        short_sha(&commit.sha),
        look.mono(12.0),
        secondary,
    );
    let date = painter.with_clip_rect(Rect::from_min_max(
        Pos2::new(date_x - 8.0, rect.top()),
        Pos2::new(sha_x, rect.bottom()),
    ));
    paint_text(
        &date,
        Pos2::new(date_x, mid),
        &short_date(commit.time),
        look.ui(13.0),
        secondary,
    );
    let author = painter.with_clip_rect(Rect::from_min_max(
        Pos2::new(author_x - 8.0, rect.top()),
        Pos2::new(date_x - 8.0, rect.bottom()),
    ));
    paint_text(
        &author,
        Pos2::new(author_x, mid),
        &commit.author,
        look.ui(13.0),
        secondary,
    );

    let subject_area = Rect::from_min_max(
        Pos2::new(rect.left() + graph_width, rect.top()),
        Pos2::new(author_x - 8.0, rect.bottom()),
    );
    let subject_painter = painter.with_clip_rect(subject_area.intersect(painter.clip_rect()));
    let mut x = subject_area.left();
    for label in commit.refs.iter().flatten() {
        let width = badge_width(ui, look, label);
        if x + width > subject_area.right() {
            break;
        }
        let badge = Rect::from_min_size(Pos2::new(x, mid - 9.0), vec2(width, 18.0));
        row_badge(ui, cx, label, commit, badge);
        x += width + 4.0;
    }
    if commit.refs.as_ref().is_some_and(|refs| !refs.is_empty()) {
        x += 2.0;
    }
    if is_head {
        paint_strong(
            &subject_painter,
            Pos2::new(x, mid),
            &commit.subject,
            look.ui(13.0),
            main,
        );
    } else {
        paint_text(
            &subject_painter,
            Pos2::new(x, mid),
            &commit.subject,
            look.ui(13.0),
            main,
        );
    }

    if response.double_clicked() {
        cx.send(Message::ActivateCommit(commit.sha.clone()));
    } else if response.clicked() {
        cx.send(Message::Select(commit.sha.clone()));
    } else if response.secondary_clicked() {
        cx.send(Message::OpenMenu(MenuRequest::Commit {
            sha: commit.sha.clone(),
            focus: None,
        }));
    }
}
