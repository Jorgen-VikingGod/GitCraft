//! The virtualised commit list with its graph, above the details of the selection.

use chrono::{DateTime, Local};
use gitcraft_core::model::{Commit, RefLabel};
use iced::widget::text::Wrapping;
use iced::widget::{canvas, column, container, mouse_area, row, scrollable, space, text};
use iced::{Background, Center, Color, Element, Length, Theme};
use native_theme_iced::{selection_background, styles};

use super::details;
use super::graph_cell::{GraphCell, width_for};
use super::widgets::{btn, hline, muted, placeholder, single, spacer, splitter, toggle};
use crate::actions::{MenuRequest, RefTarget};
use crate::app::{App, Message, ROW_HEIGHT, RepoTab, Splitter, View};
use crate::command::Command;
use crate::logic::{plural, short_sha};
use crate::theme::Look;

pub fn format_time(seconds: i64) -> String {
    DateTime::from_timestamp(seconds, 0)
        .map(|time| {
            time.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// Rows drawn beyond the visible ones, so that a small scroll never shows a gap.
const OVERSCAN: usize = 4;
const AUTHOR_WIDTH: f32 = 150.0;
const DATE_WIDTH: f32 = 112.0;
const SHA_WIDTH: f32 = 60.0;

pub fn view<'a>(app: &'a App, tab: &'a RepoTab) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    let mut top = column![];
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
        top = top.push(
            container(
                row![
                    text(summary).size(size),
                    spacer(),
                    btn(look, "Clear search", Some(Message::SearchClear))
                ]
                .align_y(Center),
            )
            .padding([3, 10]),
        );
    } else if tab.change_count() > 0 {
        top = top.push(
            mouse_area(
                container(
                    row![
                        single("●", size).color(look.palette.modified),
                        text("Uncommitted changes").size(size),
                        muted(plural(tab.change_count(), "file"), size),
                    ]
                    .spacing(8)
                    .align_y(Center),
                )
                .padding([4, 10])
                .width(Length::Fill),
            )
            .on_press(Message::SetView(View::Changes)),
        );
    }
    let mut header = row![single("Description", size)]
        .spacing(6)
        .align_y(Center)
        .padding([3, 8]);
    if tab.search.is_none() {
        header = header
            .push(toggle(
                look,
                "All branches",
                tab.all_branches,
                Message::SetBranchFilter(true),
            ))
            .push(toggle(
                look,
                "Current branch",
                !tab.all_branches,
                Message::SetBranchFilter(false),
            ));
    }
    header = header
        .push(spacer())
        .push(muted("Author", size).width(AUTHOR_WIDTH))
        .push(muted("Date", size).width(DATE_WIDTH))
        .push(muted("Commit", size).width(SHA_WIDTH));
    top = top.push(header).push(hline(look)).push(list(app, tab));

    let bottom: Element<'a, Message> = if tab.compare.is_some() {
        details::compare(app, tab)
    } else if tab.selected.is_some() {
        details::commit(app, tab)
    } else {
        placeholder(look, "Select a commit to see its details")
    };
    column![
        container(top)
            .height(app.settings.history_height)
            .width(Length::Fill),
        splitter(app, Splitter::History),
        container(bottom).height(Length::Fill).width(Length::Fill),
    ]
    .into()
}

fn list<'a>(app: &'a App, tab: &'a RepoTab) -> Element<'a, Message> {
    let look = &app.look;
    let commits = tab.visible();
    if commits.is_empty() {
        let note = if tab.loading {
            "Loading history…"
        } else if tab.search.is_some() {
            "No matching commits"
        } else {
            "No commits yet"
        };
        return placeholder(look, note);
    }
    let total = commits.len();
    // The window's height is a safe upper bound: the list's own is only known after it scrolled.
    let count = (app.window.height / app.scale() / ROW_HEIGHT) as usize + 2 * OVERSCAN + 1;
    let first = ((tab.offset / ROW_HEIGHT) as usize)
        .saturating_sub(OVERSCAN)
        .min(total.saturating_sub(count));
    let visible = &commits[first..(first + count).min(total)];
    let lanes = visible
        .iter()
        .filter_map(|commit| commit.graph.as_ref().map(|g| g.width))
        .max()
        .unwrap_or(1);
    let head = tab.refs.head.as_deref();

    let mut items: Vec<Element<'a, Message>> = Vec::with_capacity(visible.len() + 3);
    items.push(space().height(first as f32 * ROW_HEIGHT).into());
    for commit in visible {
        let selected = tab.selected.as_deref() == Some(commit.sha.as_str());
        let compared = tab
            .compare
            .as_ref()
            .is_some_and(|c| c.base == commit.sha || c.target == commit.sha);
        items.push(commit_row(
            look,
            commit,
            selected || compared,
            Some(commit.sha.as_str()) == head,
            lanes,
        ));
    }
    let below = total.saturating_sub(first + visible.len());
    items.push(space().height(below as f32 * ROW_HEIGHT).into());
    if tab.has_more && tab.search.is_none() {
        let label = if tab.loading {
            "Loading…"
        } else {
            "Load more commits"
        };
        items.push(
            container(btn(
                look,
                label,
                (!tab.loading).then_some(Message::LoadMore),
            ))
            .center_x(Length::Fill)
            .padding(6)
            .into(),
        );
    }
    scrollable(column(items).width(Length::Fill))
        .id(tab.list.clone())
        .width(Length::Fill)
        .height(Length::Fill)
        .direction(scrollable::Direction::Vertical(styles::scrollbar(
            &look.resolved,
        )))
        .style(styles::scrollable(&look.resolved))
        .on_scroll(|viewport| {
            Message::Scrolled(viewport.absolute_offset().y, viewport.bounds().height)
        })
        .into()
}

fn badge<'a>(
    look: &Look,
    label: &'a RefLabel,
    commit: &'a Commit,
    selected: bool,
) -> Element<'a, Message> {
    let color = match label.kind.as_str() {
        _ if selected => look.selected_text,
        "tag" => look.palette.tag,
        "remote" => look.palette.remote,
        _ => look.palette.local,
    };
    let weight = if label.current {
        iced::font::Weight::Bold
    } else {
        iced::font::Weight::Normal
    };
    let chip = container(
        text(label.name.as_str())
            .size(look.text_size - 2.0)
            .color(color)
            .font(iced::Font {
                weight,
                ..iced::Font::default()
            })
            .wrapping(Wrapping::None),
    )
    .padding([0, 5])
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(Color { a: 0.14, ..color })),
        border: iced::border::rounded(3),
        ..container::Style::default()
    });
    let target = match label.kind.as_str() {
        "local" => Some(RefTarget::Local(label.name.clone())),
        "remote" => Some(RefTarget::Remote(label.name.clone())),
        "tag" => Some(RefTarget::Tag(label.name.clone())),
        _ => None,
    };
    let mut area = mouse_area(chip)
        .on_press(Message::Select(commit.sha.clone()))
        .on_right_press(Message::OpenMenu(MenuRequest::Commit {
            sha: commit.sha.clone(),
            focus: target,
        }));
    if label.kind == "local" && !label.current {
        area = area.on_double_click(Message::Run(Command::Checkout {
            reference: label.name.clone(),
            kind: "local",
        }));
    } else if label.kind == "remote" {
        area = area.on_double_click(Message::Run(Command::Checkout {
            reference: label.name.clone(),
            kind: "remote",
        }));
    }
    area.into()
}

fn commit_row<'a>(
    look: &Look,
    commit: &'a Commit,
    selected: bool,
    is_head: bool,
    lanes: u32,
) -> Element<'a, Message> {
    let size = look.text_size - 1.0;
    let mut line = row![]
        .spacing(6)
        .height(ROW_HEIGHT)
        .align_y(Center)
        .padding([0, 8]);
    if let Some(graph) = &commit.graph {
        line = line.push(
            canvas(GraphCell {
                row: graph,
                lanes: look.palette.lanes,
            })
            .width(width_for(lanes))
            .height(ROW_HEIGHT),
        );
    }
    for label in commit.refs.iter().flatten() {
        line = line.push(badge(look, label, commit, selected));
    }
    let on_row = selected.then_some(look.selected_text);
    let single_line = |content: &'a str| {
        text(content)
            .size(size)
            .color_maybe(on_row)
            .wrapping(Wrapping::None)
    };
    let subject = single_line(&commit.subject).font(iced::Font {
        weight: if is_head {
            iced::font::Weight::Bold
        } else {
            iced::font::Weight::Normal
        },
        ..iced::Font::default()
    });
    line = line
        .push(container(subject).width(Length::Fill).clip(true))
        .push(
            container(single_line(&commit.author))
                .width(AUTHOR_WIDTH)
                .clip(true),
        )
        .push(
            container(
                text(format_time(commit.time))
                    .size(size)
                    .color_maybe(on_row)
                    .wrapping(Wrapping::None),
            )
            .width(DATE_WIDTH)
            .clip(true),
        )
        .push(
            text(short_sha(&commit.sha))
                .size(size)
                .color_maybe(on_row)
                .font(iced::Font::MONOSPACE)
                .width(SHA_WIDTH),
        );
    let background = selected.then(|| selection_background(&look.resolved));
    mouse_area(
        container(line)
            .width(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: background.map(Background::Color),
                ..container::Style::default()
            }),
    )
    .on_press(Message::Select(commit.sha.clone()))
    .on_double_click(Message::ActivateCommit(commit.sha.clone()))
    .on_right_press(Message::OpenMenu(MenuRequest::Commit {
        sha: commit.sha.clone(),
        focus: None,
    }))
    .into()
}
