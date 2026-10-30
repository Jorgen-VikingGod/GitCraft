//! What's below the history: a commit, a comparison, or a stash, each with a file list and a diff.

use gitcraft_core::model::FileChange;
use iced::widget::text::Wrapping;
use iced::widget::{Column, column, container, mouse_area, row, space, text, text_editor};
use iced::{Background, Center, Color, Element, Length, Theme};

use super::diff;
use super::history::badge;
use super::style;
use super::widgets::{
    btn, danger_text, file_row, heading, hline, muted_text, placeholder, primary, scrolled, single,
    spacer, splitter,
};
use crate::actions::{Action, MenuRequest};
use crate::app::{App, Message, PaneKey, RepoTab, Splitter};
use crate::command::Command;
use crate::logic::{full_date, initials, name_hue, plural, short_sha};
use crate::theme::Look;

fn files<'a>(
    app: &'a App,
    list: &'a [FileChange],
    selected: Option<&str>,
    rev: Option<&'a str>,
    empty: &'a str,
) -> Element<'a, Message> {
    let look = &app.look;
    if list.is_empty() {
        return container(
            text(empty)
                .size(13)
                .font(look.italic)
                .color(look.c.text_muted),
        )
        .padding([10, 14])
        .into();
    }
    let mut rows: Column<'a, Message> = column![].width(Length::Fill);
    for file in list {
        let mut area = mouse_area(file_row(
            look,
            file,
            selected == Some(file.path.as_str()),
            None,
        ))
        .on_press(Message::PickFile(file.path.clone()));
        if let Some(rev) = rev {
            area = area.on_right_press(Message::OpenMenu(MenuRequest::File {
                area: None,
                path: file.path.clone(),
                rev: Some(rev.to_owned()),
            }));
        }
        rows = rows.push(area);
    }
    scrolled(look, rows).height(Length::Fill).into()
}

fn with_diff<'a>(
    app: &'a App,
    tab: &'a RepoTab,
    left: Element<'a, Message>,
    empty: &'a str,
) -> Element<'a, Message> {
    let width = app.window.width / app.scale()
        - app.settings.sidebar_width
        - app.settings.details_width
        - 10.0;
    row![
        container(left)
            .width(app.settings.details_width)
            .height(Length::Fill)
            .clip(true)
            .style(style::fill(app.look.c.panel)),
        splitter(app, Splitter::Details),
        container(diff::view(
            app,
            &tab.commit_pane,
            PaneKey::Commit,
            None,
            empty,
            None,
            width
        ))
        .width(Length::Fill)
        .height(Length::Fill),
    ]
    .into()
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> Color {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let section = hue / 60.0;
    let x = chroma * (1.0 - (section % 2.0 - 1.0).abs());
    let (r, g, b) = match section as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    Color::from_rgb(r + m, g + m, b + m)
}

fn avatar<'a>(look: &Look, name: &str) -> Element<'a, Message> {
    let color = hsl(name_hue(name) as f32, 0.55, 0.45);
    container(
        text(initials(name))
            .size(12)
            .font(look.bold)
            .color(Color::WHITE),
    )
    .center(32)
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(color)),
        border: iced::border::rounded(16),
        ..container::Style::default()
    })
    .into()
}

/// How tall the part above the file list may get: just over half of the pane.
fn meta_height(app: &App) -> f32 {
    let pane = app.window.height / app.scale() - app.settings.history_height - 150.0;
    (pane * 0.55).max(120.0)
}

pub fn commit<'a>(app: &'a App, tab: &'a RepoTab) -> Element<'a, Message> {
    let look = &app.look;
    let c = &look.c;
    if let Some(error) = &tab.detail_error {
        return container(text(error.as_str()).size(13).color(c.danger))
            .center(Length::Fill)
            .into();
    }
    let Some(detail) = &tab.detail else {
        return placeholder(look, "Loading…");
    };
    let fact = |label: &'a str, value: Element<'a, Message>| {
        row![
            container(text(label).size(12).font(look.semibold).color(c.text_muted)).width(62),
            container(value).width(Length::Fill)
        ]
    };
    let mut parents = row![].spacing(6);
    if detail.parents.is_empty() {
        parents = parents.push(text("(root commit)").size(12).font(look.mono));
    }
    for parent in &detail.parents {
        parents = parents.push(
            mouse_area(
                single(short_sha(parent), 12)
                    .font(look.mono)
                    .color(c.accent),
            )
            .on_press(Message::ShowCommit(parent.clone())),
        );
    }
    let mut meta = column![
        row![
            avatar(look, &detail.author),
            column![
                text(detail.author.as_str()).size(13).font(look.semibold),
                text(format!("<{}>", detail.email))
                    .size(13)
                    .color(c.text_muted),
                text(full_date(detail.time)).size(12).color(c.text_muted),
            ]
            .width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Center),
    ]
    .spacing(8)
    .width(Length::Fill);
    if !detail.refs.is_empty() {
        let mut labels = row![].spacing(4);
        for label in &detail.refs {
            labels = labels.push(badge(look, label));
        }
        meta = meta.push(labels.wrap());
    }
    meta = meta
        .push(
            text_editor(&tab.message)
                .on_action(Message::MessageAction)
                .size(13)
                .padding(0)
                .height(Length::Shrink)
                .style(style::plain_editor(look)),
        )
        .push(fact(
            "SHA",
            mouse_area(
                text(detail.sha.as_str())
                    .size(12)
                    .font(look.mono)
                    .color(c.accent)
                    .wrapping(Wrapping::Glyph),
            )
            .on_press(Message::Copy(detail.sha.clone()))
            .into(),
        ))
        .push(fact(
            if detail.parents.len() > 1 {
                "Parents"
            } else {
                "Parent"
            },
            parents.into(),
        ));
    if detail.committer != detail.author || detail.commit_time != detail.time {
        meta = meta.push(fact(
            "Committer",
            text(format!(
                "{}, {}",
                detail.committer,
                full_date(detail.commit_time)
            ))
            .size(12)
            .font(look.mono)
            .into(),
        ));
    }
    let suffix = if detail.parents.len() > 1 {
        " (compared to first parent)"
    } else {
        ""
    };
    let left = column![
        container(scrolled(look, container(meta).padding([12, 14])).height(Length::Shrink))
            .max_height(meta_height(app)),
        hline(look),
        container(heading(
            look,
            &format!("{} changed", plural(detail.files.len(), "file"))
        ))
        .padding(iced::Padding {
            top: 8.0,
            bottom: 4.0,
            left: 14.0,
            right: 14.0
        }),
        if suffix.is_empty() {
            Element::from(space().height(0))
        } else {
            container(muted_text(look, suffix.trim(), 12))
                .padding([0, 14])
                .into()
        },
        files(
            app,
            &detail.files,
            tab.selected_file.as_deref(),
            Some(&detail.sha),
            "No files"
        ),
    ];
    let empty = if detail.files.is_empty() {
        "This commit has no file changes"
    } else {
        "Select a file to see its changes"
    };
    with_diff(app, tab, left.into(), empty)
}

pub fn compare<'a>(app: &'a App, tab: &'a RepoTab) -> Element<'a, Message> {
    let look = &app.look;
    let c = &look.c;
    let Some(state) = &tab.compare else {
        return placeholder(look, "");
    };
    let subject = |sha: &str| {
        tab.commits
            .iter()
            .find(|commit| commit.sha == sha)
            .map_or_else(String::new, |commit| commit.subject.clone())
    };
    let line = |prefix: &'static str, sha: &str| {
        row![
            single(prefix, 12).color(c.text_muted),
            single(short_sha(sha).to_owned(), 12).font(look.mono),
            container(single(subject(sha), 12).color(c.text_muted)).clip(true),
        ]
        .spacing(6)
    };
    let mut meta = column![
        text("Comparing commits").size(13).font(look.semibold),
        line("", &state.base),
        line("→", &state.target),
    ]
    .spacing(4)
    .padding([12, 14]);
    if let Some(result) = &state.result {
        meta = meta.push(muted_text(
            look,
            format!(
                "{} only in target, {} only in base",
                plural(result.target_only as usize, "commit"),
                result.base_only
            ),
            12,
        ));
    }
    if let Some(error) = &state.error {
        meta = meta.push(text(error.as_str()).size(12).color(c.danger));
    }
    meta = meta.push(
        row![
            btn(look, "Swap", Some(Message::CompareSwap)),
            btn(look, "Close", Some(Message::CompareClose))
        ]
        .spacing(6),
    );
    let list = state
        .result
        .as_ref()
        .map_or(&[][..], |result| &result.files);
    let left = column![
        meta,
        hline(look),
        files(
            app,
            list,
            state.selected.as_deref(),
            Some(&state.target),
            "No differences"
        )
    ];
    with_diff(app, tab, left.into(), "Select a file to see its changes")
}

pub fn stash<'a>(app: &'a App, tab: &'a RepoTab) -> Element<'a, Message> {
    let look = &app.look;
    let found = tab.stash.as_ref().and_then(|open| {
        tab.refs
            .stashes
            .iter()
            .find(|stash| stash.name == open.name)
            .map(|stash| (open, stash))
    });
    let Some((open, stash)) = found else {
        return placeholder(look, "Stash not found");
    };
    let name = stash.name.clone();
    let left = column![
        column![
            text(stash.message.as_str()).size(13).font(look.semibold),
            muted_text(
                look,
                format!("{} · {}", stash.name, full_date(stash.time)),
                12
            ),
            row![
                primary(
                    look,
                    "Apply",
                    Some(Message::Run(Command::StashApply {
                        name: name.clone(),
                        pop: false
                    }))
                ),
                btn(
                    look,
                    "Pop",
                    Some(Message::Run(Command::StashApply {
                        name: name.clone(),
                        pop: true
                    }))
                ),
                spacer(),
                danger_text(
                    look,
                    "Drop…",
                    Some(Message::Action(Action::DropStash(name)))
                ),
            ]
            .spacing(6),
        ]
        .spacing(6)
        .padding([12, 14]),
        hline(look),
        files(app, &open.files, open.selected.as_deref(), None, "No files"),
    ];
    with_diff(app, tab, left.into(), "Select a file to see its changes")
}
