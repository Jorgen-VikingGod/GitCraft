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
use crate::i18n::{tr, trf};
use crate::logic::{plural, short_date, short_sha};

/// Rows drawn beyond the visible ones, so that a small scroll never shows a gap.
const OVERSCAN: usize = 4;

pub fn view(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let look = cx.look();
    let c = &look.c;
    let area = ui.max_rect();
    let (top, rest) = split_y(
        area,
        cx.app
            .settings
            .layout
            .history_height
            .min(area.height() - 60.0),
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
    // Search results and most filters leave gaps in the history, so no graph is drawn then.
    let gaps = tab
        .history_filter
        .as_ref()
        .is_some_and(|filter| !filter.keeps_graph());
    let graph = if tab.search.is_some() || gaps {
        GraphColumn {
            content: 10.0,
            shown: 10.0,
            offset: 0.0,
            drawn: false,
        }
    } else {
        let content = width_for(tab.graph_lanes);
        // The description keeps room for its header, however wide the graph is set.
        let room = ui.available_rect_before_wrap();
        let columns = &cx.app.settings.columns;
        let subject = crate::logic::column_x(room.right(), columns).subject_end - room.left();
        let shown = content
            .min(tab.graph_width.unwrap_or(f32::INFINITY))
            .min(crate::logic::graph_room(subject));
        GraphColumn {
            content,
            shown,
            offset: crate::logic::graph_offset(tab.graph_scroll, content, shown),
            drawn: true,
        }
    };
    let graph_width = graph.shown;
    let view = &tab.ref_view;
    if !view.is_empty() && tab.search.is_none() && tab.all_branches {
        let short = |name: &String| {
            name.strip_prefix("refs/heads/")
                .or_else(|| name.strip_prefix("refs/remotes/"))
                .unwrap_or(name)
                .to_owned()
        };
        let summary = if view.solo.is_empty() {
            trf(
                "{branches} hidden: {names}",
                &[
                    ("branches", &plural(view.hidden.len(), "branch")),
                    (
                        "names",
                        &view.hidden.iter().map(short).collect::<Vec<_>>().join(", "),
                    ),
                ],
            )
        } else {
            trf(
                "Only {names}",
                &[(
                    "names",
                    &view.solo.iter().map(short).collect::<Vec<_>>().join(", "),
                )],
            )
        };
        super::widgets::bar(ui, 29.0, c.accent_soft, 10.0, |ui| {
            icon::icon(ui, "branch", 13.0, c.text_muted);
            text(ui, summary, 12.0, c.text);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if btn(ui, look, "Show all branches", true) {
                    cx.send(Message::RefVisibility(crate::app::RefVisibility::ShowAll));
                }
            });
        });
        hline(ui, look);
    }
    if let Some(filter) = tab.history_filter.as_ref().filter(|_| tab.search.is_none()) {
        let mut parts = Vec::new();
        if !filter.author.trim().is_empty() {
            parts.push(trf("by “{author}”", &[("author", filter.author.trim())]));
        }
        if !filter.path.trim().is_empty() {
            parts.push(trf("in {path}", &[("path", filter.path.trim())]));
        }
        if !filter.text.is_empty() {
            parts.push(trf("changing “{text}”", &[("text", &filter.text)]));
        }
        let summary = trf(
            "Filtered history: {commits} {filters}",
            &[
                ("commits", &plural(tab.commits.len(), "commit")),
                ("filters", &parts.join(" · ")),
            ],
        );
        super::widgets::bar(ui, 29.0, c.accent_soft, 10.0, |ui| {
            icon::icon(ui, "search", 13.0, c.text_muted);
            text(ui, summary, 12.0, c.text);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if btn(ui, look, "Clear filter", true) {
                    cx.send(Message::SetHistoryFilter(None));
                }
                if btn(ui, look, "Edit…", true) {
                    cx.send(Message::Action(crate::actions::Action::FilterHistory));
                }
            });
        });
        hline(ui, look);
    }
    if let Some(search) = &tab.search {
        let summary = if search.loading {
            tr("Searching…").to_owned()
        } else {
            trf(
                "{commits} matching “{query}” ({mode})",
                &[
                    ("commits", &plural(search.results.len(), "commit")),
                    ("query", &search.query),
                    ("mode", tr(search.mode.key())),
                ],
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
            Pos2::new(rect.left() + graph.description(), rect.center().y),
            tr("Uncommitted changes"),
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
    header(ui, cx, tab, graph);
    hline(ui, look);
    list(ui, cx, tab, graph);
}

/// The graph column: how wide all its lanes are, how much of that shows, and how far it is
/// scrolled sideways. With search results or a filter that leaves gaps no graph is drawn.
#[derive(Debug, Clone, Copy)]
struct GraphColumn {
    content: f32,
    shown: f32,
    offset: f32,
    drawn: bool,
}

/// Room for the coloured block before each commit's description, as wide as the block and the
/// gap after it.
const MARK: f32 = 5.0;
const MARK_ROOM: f32 = MARK + 7.0;

impl GraphColumn {
    /// Where the description starts: after the graph and, with a graph, the coloured block
    /// that links each message to its dot.
    fn description(&self) -> f32 {
        self.shown + if self.drawn { MARK_ROOM } else { 0.0 }
    }

    fn scrolls(&self) -> bool {
        self.drawn && self.content > self.shown + 0.5
    }
}

fn header(ui: &mut Ui, cx: &Cx, tab: &RepoTab, graph: GraphColumn) {
    let graph_width = graph.shown;
    let look = cx.look();
    let c = &look.c;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 25.0), Sense::click());
    fill(ui, rect, c.panel);
    if response.secondary_clicked() {
        cx.send(Message::OpenMenu(crate::actions::MenuRequest::Columns));
    }
    let mid = rect.center().y;
    let columns = &cx.app.settings.columns;
    let x = crate::logic::column_x(rect.right(), columns);
    for (title, at) in [
        ("Commit", x.sha),
        ("Date", x.date),
        ("Author", x.author),
        ("Changes", x.changes),
    ] {
        let Some(at) = at else { continue };
        paint_text(
            ui.painter(),
            Pos2::new(at, mid),
            &tr(title).to_uppercase(),
            look.ui(11.0),
            c.text_muted,
        );
    }
    // The date and author columns widen by dragging their left edge.
    for (column, edge, width) in [
        (crate::app::Column::Date, x.date_edge, columns.date_width),
        (
            crate::app::Column::Author,
            x.author_edge,
            columns.author_width,
        ),
    ] {
        let Some(edge) = edge else { continue };
        let handle = Rect::from_center_size(Pos2::new(edge, mid), vec2(8.0, rect.height()));
        let drag = ui.interact(
            handle,
            ui.scope_id().with(("column-edge", column as u8)),
            Sense::drag(),
        );
        if drag.hovered() || drag.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            ui.painter()
                .vline(edge, rect.y_range(), Stroke::new(1.0, c.border));
        }
        if drag.dragged() {
            let delta = drag.drag_delta().x;
            cx.send(Message::ColumnWidth(column, width - delta, false));
        }
        if drag.drag_stopped() {
            cx.send(Message::ColumnWidth(column, width, true));
        }
    }
    // The graph column narrows or widens by dragging its right edge.
    if graph.drawn {
        let edge = rect.left() + graph_width;
        let handle = Rect::from_center_size(Pos2::new(edge, mid), vec2(8.0, rect.height()));
        let drag = ui.interact(handle, ui.scope_id().with("graph-edge"), Sense::drag());
        if drag.hovered() || drag.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            ui.painter()
                .vline(edge, rect.y_range(), Stroke::new(1.0, c.border));
        }
        if drag.dragged() {
            let width = graph_width + drag.drag_delta().x;
            cx.send(Message::ColumnWidth(
                crate::app::Column::Graph,
                width,
                false,
            ));
        }
        if drag.drag_stopped() {
            cx.send(Message::ColumnWidth(
                crate::app::Column::Graph,
                graph_width,
                true,
            ));
        }
    }
    let description = paint_text(
        ui.painter(),
        Pos2::new(rect.left() + graph.description(), mid),
        &tr("Description").to_uppercase(),
        look.ui(11.0),
        c.text_muted,
    );
    if tab.search.is_none() {
        let slot = Rect::from_min_size(
            Pos2::new(
                rect.left() + graph.description() + description + 12.0,
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

fn list(ui: &mut Ui, cx: &Cx, tab: &RepoTab, graph: GraphColumn) {
    let look = cx.look();
    let commits = tab.visible();
    if commits.is_empty() {
        let note = if tab.loading {
            tr("Loading history…")
        } else if tab.search.is_some() {
            tr("No matching commits")
        } else {
            tr("No commits yet")
        };
        placeholder(ui, look, note);
        return;
    }
    let total = commits.len();
    let head = tab.refs.head.as_deref();
    let more = tab.has_more && tab.search.is_none();
    let lanes = {
        let room = ui.available_rect_before_wrap();
        Rect::from_min_max(
            room.min,
            Pos2::new(room.left() + graph.shown, room.bottom()),
        )
    };
    // A sideways wheel (Shift + wheel, or a trackpad) over the graph scrolls its lanes.
    if graph.scrolls() && ui.rect_contains_pointer(lanes) {
        let sideways = ui.input(|input| input.smooth_scroll_delta.x);
        if sideways != 0.0 {
            let offset =
                crate::logic::graph_offset(graph.offset - sideways, graph.content, graph.shown);
            cx.send(Message::GraphScroll(offset));
        }
    }
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
            let selected = tab.selected.as_deref() == Some(commit.sha.as_str())
                || tab.picked_commits.contains(&commit.sha);
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
                graph,
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
                        tr("Loading…")
                    } else {
                        tr("Load more commits")
                    };
                    if btn(ui, look, label, !tab.loading) {
                        cx.send(Message::LoadMore);
                    }
                });
            });
        }
    });
    super::widgets::scroll_shadows(ui, cx.look(), &output);
    if graph.scrolls() {
        graph_scrollbar(ui, cx, tab, graph, output.inner_rect);
    }
    let offset = output.state.clamped_offset().y;
    let height = output.inner_rect.height();
    if (offset - tab.offset).abs() > 0.5 || (height - tab.viewport).abs() > 0.5 {
        cx.send(Message::Scrolled(offset, height));
    }
}

/// The graph column's sideways scroll bar, for a graph with more lanes than the column shows.
/// It is egui's own bar, as on every other scroll area, over the graph column: it shows as the
/// pointer comes over the lanes and is dragged or clicked as the others are. The lanes themselves
/// are drawn by the rows; this area only carries the bar, and its offset is the graph's.
fn graph_scrollbar(ui: &mut Ui, cx: &Cx, tab: &RepoTab, graph: GraphColumn, list: Rect) {
    let column = Rect::from_min_max(
        list.min,
        Pos2::new(list.left() + graph.shown, list.bottom()),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(column));
    let shown = egui::ScrollArea::horizontal()
        .id_salt(("graph-lanes", &tab.path))
        .auto_shrink([false, false])
        // The rows below take the pointer; the wheel is handled with the list (Shift + wheel).
        .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR)
        .horizontal_scroll_offset(graph.offset)
        .show(&mut child, |ui| {
            ui.allocate_space(vec2(graph.content, 1.0));
        });
    // Shadows at the sides where more lanes are out of view, as on the other scroll areas.
    super::widgets::scroll_shadows(&child, cx.look(), &shown);
    let offset = shown.state.clamped_offset().x;
    if (offset - graph.offset).abs() > 0.5 {
        let offset = crate::logic::graph_offset(offset, graph.content, graph.shown);
        cx.send(Message::GraphScroll(offset));
    }
}

/// `response` with `text` as its tooltip when the text, ending at `end`, runs past `room`, where
/// it is cut off.
fn cut_off(response: egui::Response, end: f32, room: f32, text: &str) -> egui::Response {
    if end > room + 0.5 {
        response.on_hover_text_at_pointer(text)
    } else {
        response
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
    graph: GraphColumn,
) {
    let graph_width = graph.shown;
    let look = cx.look();
    let c = &look.c;
    let (main, secondary) = if is_head {
        (c.selection_text, c.selection_text)
    } else {
        (c.text, c.text_muted)
    };
    let response = ui.interact(
        rect,
        ui.scope_id().with(("commit", &commit.sha)),
        Sense::click(),
    );
    // Only the commit checked out is in the accent colour; the selected one is grey.
    let marked = if is_head {
        Some(c.selection)
    } else if selected {
        Some(c.hover_strong)
    } else if compared {
        Some(c.accent_soft)
    } else {
        None
    };
    if let Some(marked) = marked {
        fill(ui, rect, marked);
    } else if ui.rect_contains_pointer(rect) {
        fill(ui, rect, c.hover);
    }
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let mid = rect.center().y;
    if let Some(row) = &commit.graph {
        // The lanes stay in their column, shifted by how far it is scrolled.
        let column = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.left() + graph_width, rect.bottom()),
        );
        let lanes = painter.with_clip_rect(column.intersect(painter.clip_rect()));
        GraphCell {
            row,
            lanes: c.lanes,
            background: marked.unwrap_or(c.bg),
            merge: commit.parents.len() > 1,
            head: is_head,
        }
        .paint(&lanes, rect.min - vec2(graph.offset, 0.0));
    }
    let columns = &cx.app.settings.columns;
    let x = crate::logic::column_x(rect.right(), columns);
    // Each column is clipped to its own room, up to where the next one starts.
    let clip = |from: f32, to: f32| {
        painter.with_clip_rect(Rect::from_min_max(
            Pos2::new(from, rect.top()),
            Pos2::new(to, rect.bottom()),
        ))
    };
    if let Some(sha_x) = x.sha {
        paint_text(
            &painter,
            Pos2::new(sha_x, mid),
            short_sha(&commit.sha),
            look.mono(12.0),
            secondary,
        );
    }
    if let (Some(date_x), Some(edge)) = (x.date, x.date_edge) {
        paint_text(
            &clip(edge, edge + columns.date_width),
            Pos2::new(date_x, mid),
            &short_date(commit.time),
            look.ui(13.0),
            secondary,
        );
    }
    if let (Some(author_x), Some(edge)) = (x.author, x.author_edge) {
        paint_text(
            &clip(edge, edge + columns.author_width),
            Pos2::new(author_x, mid),
            &commit.author,
            look.ui(13.0),
            secondary,
        );
    }
    if let Some(changes_x) = x.changes {
        let stats = cx
            .app
            .current()
            .and_then(|tab| tab.stats.get(&commit.sha).copied());
        if let Some(stats) = stats {
            let room = clip(
                changes_x - 8.0,
                changes_x - 8.0 + crate::logic::CHANGES_COLUMN,
            );
            let added = format!("+{}", stats.added);
            let width = paint_text(
                &room,
                Pos2::new(changes_x, mid),
                &added,
                look.mono(12.0),
                c.diff_add_text,
            );
            paint_text(
                &room,
                Pos2::new(changes_x + width + 6.0, mid),
                &format!("−{}", stats.removed),
                look.mono(12.0),
                c.diff_del_text,
            );
        }
    }

    // A block in the commit's lane colour, so the message is easy to match with its dot.
    if graph.drawn
        && let Some(row) = &commit.graph
    {
        let block = Rect::from_min_size(
            Pos2::new(rect.left() + graph_width, rect.top() + 5.0),
            vec2(MARK, rect.height() - 10.0),
        );
        painter.rect_filled(
            block,
            1.5,
            super::graph_cell::lane_color(&c.lanes, row.color),
        );
    }
    let subject_area = Rect::from_min_max(
        Pos2::new(rect.left() + graph.description(), rect.top()),
        Pos2::new(x.subject_end, rect.bottom()),
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
    if let Some(name) = &commit.stash {
        // A stash: its name, then its message, muted, and it opens as a stash.
        x += paint_text(
            &subject_painter,
            Pos2::new(x, mid),
            name,
            look.mono(12.0),
            secondary,
        );
        x += 8.0;
        let width = paint_text(
            &subject_painter,
            Pos2::new(x, mid),
            &commit.subject,
            look.ui(13.0),
            secondary,
        );
        let response = cut_off(response, x + width, subject_area.right(), &commit.subject);
        if response.clicked() || response.double_clicked() {
            cx.send(Message::OpenStash(name.clone()));
        } else if response.secondary_clicked() {
            cx.send(Message::OpenMenu(MenuRequest::Stash(name.clone())));
        }
        return;
    }
    let width = if is_head {
        paint_strong(
            &subject_painter,
            Pos2::new(x, mid),
            &commit.subject,
            look.ui(13.0),
            main,
        )
    } else {
        paint_text(
            &subject_painter,
            Pos2::new(x, mid),
            &commit.subject,
            look.ui(13.0),
            main,
        )
    };
    // A message cut off at the column's end shows in full under the pointer.
    let response = cut_off(response, x + width, subject_area.right(), &commit.subject);

    if response.double_clicked() {
        cx.send(Message::ActivateCommit(commit.sha.clone()));
    } else if response.clicked() {
        cx.send(Message::Select(commit.sha.clone()));
    } else if response.secondary_clicked() {
        let picked = cx
            .app
            .current()
            .map(|tab| tab.picked_commits.clone())
            .unwrap_or_default();
        if picked.len() > 1 && picked.contains(&commit.sha) {
            cx.send(Message::OpenMenu(MenuRequest::Commits(picked)));
        } else {
            cx.send(Message::OpenMenu(MenuRequest::Commit {
                sha: commit.sha.clone(),
                focus: None,
            }));
        }
    }
}
