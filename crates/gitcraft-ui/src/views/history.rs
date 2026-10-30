//! The virtualised commit list with its graph, above the details of the selection.

use gitcraft_core::model::{Commit, RefLabel};
use iced::widget::text::Wrapping;
use iced::widget::{canvas, column, container, mouse_area, row, scrollable, space, text};
use iced::{Background, Center, Color, Element, Length, Theme};

use super::details;
use super::graph_cell::{GraphCell, width_for};
use super::icon::icon;
use super::style;
use super::widgets::{
    btn, heading, hline, hoverable, placeholder, segmented, single, spacer, splitter,
};
use crate::actions::{MenuRequest, RefTarget};
use crate::app::{App, Message, ROW_HEIGHT, RepoTab, Splitter, View};
use crate::command::Command;
use crate::logic::{plural, short_date, short_sha};
use crate::theme::Look;

/// Rows drawn beyond the visible ones, so that a small scroll never shows a gap.
const OVERSCAN: usize = 4;
const AUTHOR_WIDTH: f32 = 150.0;
const DATE_WIDTH: f32 = 120.0;
const SHA_WIDTH: f32 = 76.0;

pub fn view<'a>(app: &'a App, tab: &'a RepoTab) -> Element<'a, Message> {
    let look = &app.look;
    let c = &look.c;
    let graph_width = if tab.search.is_some() {
        10.0
    } else {
        width_for(tab.graph_lanes)
    };
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
        top = top.push(column![
            container(
                row![
                    icon("search", 13.0, c.text_muted),
                    text(summary).size(12),
                    spacer(),
                    btn(look, "Clear search", Some(Message::SearchClear))
                ]
                .spacing(6)
                .align_y(Center),
            )
            .padding([4, 10])
            .width(Length::Fill)
            .style(style::fill(c.accent_soft)),
            hline(look),
        ]);
    } else if tab.change_count() > 0 {
        let dot = container(space()).width(9).height(9).style({
            let color = c.text_muted;
            move |_: &Theme| container::Style {
                border: iced::Border {
                    color,
                    width: 2.0,
                    radius: 5.0.into(),
                },
                ..container::Style::default()
            }
        });
        top = top.push(column![
            mouse_area(hoverable(
                look,
                container(
                    row![
                        container(dot).width(graph_width).center_x(graph_width),
                        text("Uncommitted changes")
                            .size(13)
                            .font(look.italic)
                            .color(c.text_muted)
                            .width(Length::Fill),
                        single(plural(tab.change_count(), "file"), 13)
                            .font(look.italic)
                            .color(c.text_muted),
                    ]
                    .align_y(Center)
                    .height(ROW_HEIGHT)
                    .padding(iced::Padding {
                        top: 0.0,
                        bottom: 0.0,
                        left: 0.0,
                        right: 12.0
                    }),
                )
                .width(Length::Fill),
                0.0
            ))
            .on_press(Message::SetView(View::Changes)),
            hline(look),
        ]);
    }
    let mut header = row![
        container(heading(look, "Description")).padding(iced::Padding {
            left: graph_width,
            ..iced::Padding::ZERO
        })
    ]
    .spacing(12)
    .align_y(Center);
    if tab.search.is_none() {
        header = header.push(segmented(
            look,
            (
                "All branches",
                tab.all_branches,
                Message::SetBranchFilter(true),
            ),
            (
                "Current branch",
                !tab.all_branches,
                Message::SetBranchFilter(false),
            ),
        ));
    }
    header = header
        .push(spacer())
        .push(
            container(heading(look, "Author"))
                .width(AUTHOR_WIDTH)
                .padding([0, 8]),
        )
        .push(
            container(heading(look, "Date"))
                .width(DATE_WIDTH)
                .padding([0, 8]),
        )
        .push(
            container(heading(look, "Commit"))
                .width(SHA_WIDTH)
                .padding(iced::Padding {
                    top: 0.0,
                    bottom: 0.0,
                    left: 4.0,
                    right: 10.0,
                }),
        );
    top = top
        .push(
            container(header)
                .height(25)
                .center_y(25)
                .width(Length::Fill)
                .style(style::fill(c.panel)),
        )
        .push(hline(look))
        .push(list(app, tab, graph_width));

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
            .width(Length::Fill)
            .style(style::fill(c.bg)),
        splitter(app, Splitter::History),
        container(bottom).height(Length::Fill).width(Length::Fill),
    ]
    .into()
}

fn list<'a>(app: &'a App, tab: &'a RepoTab, graph_width: f32) -> Element<'a, Message> {
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
            selected && !compared,
            compared,
            Some(commit.sha.as_str()) == head,
            graph_width,
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
            .padding(8)
            .into(),
        );
    }
    scrollable(column(items).width(Length::Fill))
        .id(tab.list.clone())
        .width(Length::Fill)
        .height(Length::Fill)
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable(look))
        .on_scroll(|viewport| {
            Message::Scrolled(viewport.absolute_offset().y, viewport.bounds().height)
        })
        .into()
}

/// The rounded labels for branches and tags.
pub fn badge<'a>(look: &Look, label: &RefLabel) -> Element<'a, Message> {
    let c = &look.c;
    let (background, color, name) = match label.kind.as_str() {
        "tag" => (c.badge_tag_bg, c.badge_tag_text, "tag"),
        "remote" => (c.badge_remote_bg, c.badge_remote_text, "cloud"),
        "head" => (c.badge_head_bg, c.badge_head_text, "commit"),
        _ if label.current => (c.badge_local_bg, c.badge_local_text, "check"),
        _ => (c.badge_local_bg, c.badge_local_text, "branch"),
    };
    let font = if label.current {
        look.bold
    } else {
        look.semibold
    };
    let border = if label.current {
        color
    } else {
        Color::TRANSPARENT
    };
    container(
        row![
            icon(name, 11.0, color),
            text(label.name.clone())
                .size(11)
                .font(font)
                .color(color)
                .wrapping(Wrapping::None)
        ]
        .spacing(3)
        .align_y(Center),
    )
    .padding([0, 6])
    .height(18)
    .center_y(18)
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(background)),
        border: iced::Border {
            color: border,
            width: 1.0,
            radius: 9.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

fn row_badge<'a>(look: &Look, label: &'a RefLabel, commit: &'a Commit) -> Element<'a, Message> {
    let target = match label.kind.as_str() {
        "local" => Some(RefTarget::Local(label.name.clone())),
        "remote" => Some(RefTarget::Remote(label.name.clone())),
        "tag" => Some(RefTarget::Tag(label.name.clone())),
        _ => None,
    };
    let mut area = mouse_area(badge(look, label))
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
    compared: bool,
    is_head: bool,
    graph_width: f32,
) -> Element<'a, Message> {
    let c = &look.c;
    let (main, secondary) = if selected {
        (c.selection_text, c.selection_text)
    } else {
        (c.text, c.text_muted)
    };
    let mut line = row![].height(ROW_HEIGHT).align_y(Center);
    if let Some(graph) = &commit.graph {
        let background = if selected { c.selection } else { c.bg };
        line = line.push(
            canvas(GraphCell {
                row: graph,
                lanes: c.lanes,
                background,
                merge: commit.parents.len() > 1,
                head: is_head,
            })
            .width(graph_width)
            .height(ROW_HEIGHT),
        );
    } else {
        line = line.push(space().width(graph_width));
    }
    let mut subject_row = row![].spacing(6).align_y(Center);
    let mut badges = row![].spacing(4).align_y(Center);
    let mut has_badges = false;
    for label in commit.refs.iter().flatten() {
        badges = badges.push(row_badge(look, label, commit));
        has_badges = true;
    }
    if has_badges {
        subject_row = subject_row.push(badges);
    }
    let subject = text(commit.subject.as_str())
        .size(13)
        .color(main)
        .wrapping(Wrapping::None)
        .font(if is_head { look.semibold } else { look.font });
    subject_row = subject_row.push(subject);
    line = line
        .push(container(subject_row).width(Length::Fill).clip(true))
        .push(
            container(
                text(commit.author.as_str())
                    .size(13)
                    .color(secondary)
                    .wrapping(Wrapping::None),
            )
            .width(AUTHOR_WIDTH)
            .padding([0, 8])
            .clip(true),
        )
        .push(
            container(
                text(short_date(commit.time))
                    .size(13)
                    .color(secondary)
                    .wrapping(Wrapping::None),
            )
            .width(DATE_WIDTH)
            .padding([0, 8])
            .clip(true),
        )
        .push(
            container(
                text(short_sha(&commit.sha))
                    .size(12)
                    .font(look.mono)
                    .color(secondary)
                    .wrapping(Wrapping::None),
            )
            .width(SHA_WIDTH)
            .padding(iced::Padding {
                top: 0.0,
                bottom: 0.0,
                left: 4.0,
                right: 10.0,
            }),
        );
    let background = if selected {
        Some(c.selection)
    } else if compared {
        Some(c.accent_soft)
    } else {
        None
    };
    let body = container(line)
        .width(Length::Fill)
        .style(style::maybe_fill(background));
    let shown: Element<'a, Message> = if selected {
        body.into()
    } else {
        hoverable(look, body, 0.0)
    };
    mouse_area(shown)
        .on_press(Message::Select(commit.sha.clone()))
        .on_double_click(Message::ActivateCommit(commit.sha.clone()))
        .on_right_press(Message::OpenMenu(MenuRequest::Commit {
            sha: commit.sha.clone(),
            focus: None,
        }))
        .into()
}
