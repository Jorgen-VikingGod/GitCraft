//! What's below the history: a commit, a comparison, or a stash, each with a file list and a diff.

use gitcraft_core::model::FileChange;
use iced::widget::{Column, column, container, mouse_area, row, scrollable, text, text_editor};
use iced::{Element, Length};
use native_theme_iced::styles;

use super::diff;
use super::history::format_time;
use super::widgets::{
    btn, danger, file_row, hline, muted, placeholder, primary, single, spacer, splitter,
};
use crate::actions::{Action, MenuRequest};
use crate::app::{App, Message, PaneKey, RepoTab, Splitter};
use crate::command::Command;
use crate::logic::{plural, short_sha};

fn files<'a>(
    app: &'a App,
    list: &'a [FileChange],
    selected: Option<&str>,
    rev: Option<&'a str>,
    empty: &'a str,
) -> Element<'a, Message> {
    let look = &app.look;
    if list.is_empty() {
        return placeholder(look, empty);
    }
    let mut rows: Column<'a, Message> = column![];
    for file in list {
        let mut area = mouse_area(file_row(look, file, selected == Some(file.path.as_str())))
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
    scrollable(rows.width(Length::Fill))
        .height(Length::Fill)
        .direction(scrollable::Direction::Vertical(styles::scrollbar(
            &look.resolved,
        )))
        .style(styles::scrollable(&look.resolved))
        .into()
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
            .clip(true),
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

pub fn commit<'a>(app: &'a App, tab: &'a RepoTab) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    if let Some(error) = &tab.detail_error {
        return placeholder(look, error.as_str());
    }
    let Some(detail) = &tab.detail else {
        return placeholder(look, "Loading…");
    };
    let fact = |label: &'a str, value: Element<'a, Message>| {
        row![muted(label, size).width(70), value].spacing(6)
    };
    let mut parents = row![].spacing(6);
    if detail.parents.is_empty() {
        parents = parents.push(muted("(root commit)", size));
    }
    for parent in &detail.parents {
        parents = parents.push(
            mouse_area(single(short_sha(parent), size).color(look.palette.remote))
                .on_press(Message::ShowCommit(parent.clone())),
        );
    }
    let mut meta = column![
        single(format!("{} <{}>", detail.author, detail.email), size),
        muted(format_time(detail.time), size),
        text_editor(&tab.message)
            .on_action(Message::MessageAction)
            .size(size)
            .height(90)
            .style(styles::text_editor(&look.resolved)),
        fact(
            "SHA",
            mouse_area(single(detail.sha.as_str(), size).font(iced::Font::MONOSPACE))
                .on_press(Message::Copy(detail.sha.clone()))
                .into()
        ),
        fact(
            if detail.parents.len() > 1 {
                "Parents"
            } else {
                "Parent"
            },
            parents.into()
        ),
    ]
    .spacing(4)
    .padding(8);
    if detail.committer != detail.author || detail.commit_time != detail.time {
        meta = meta.push(fact(
            "Committer",
            single(
                format!("{}, {}", detail.committer, format_time(detail.commit_time)),
                size,
            )
            .into(),
        ));
    }
    let suffix = if detail.parents.len() > 1 {
        " (compared to first parent)"
    } else {
        ""
    };
    let left = column![
        meta,
        hline(look),
        container(muted(
            format!("{} changed{suffix}", plural(detail.files.len(), "file")),
            size
        ))
        .padding([4, 8]),
        files(
            app,
            &detail.files,
            tab.selected_file.as_deref(),
            Some(&detail.sha),
            "This commit has no file changes"
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
    let size = look.text_size - 1.0;
    let Some(state) = &tab.compare else {
        return placeholder(look, "");
    };
    let subject = |sha: &str| {
        tab.commits
            .iter()
            .find(|commit| commit.sha == sha)
            .map_or_else(String::new, |commit| commit.subject.clone())
    };
    let mut meta = column![
        text("Comparing commits").size(size),
        single(
            format!("{} {}", short_sha(&state.base), subject(&state.base)),
            size
        ),
        single(
            format!("→ {} {}", short_sha(&state.target), subject(&state.target)),
            size
        ),
    ]
    .spacing(3)
    .padding(8);
    if let Some(result) = &state.result {
        meta = meta.push(muted(
            format!(
                "{} only in target, {} only in base",
                plural(result.target_only as usize, "commit"),
                result.base_only
            ),
            size,
        ));
    }
    if let Some(error) = &state.error {
        meta = meta.push(text(error.as_str()).size(size).color(look.palette.removed));
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
    let size = look.text_size - 1.0;
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
            text(stash.message.as_str()).size(size),
            muted(
                format!("{} · {}", stash.name, format_time(stash.time)),
                size
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
                danger(
                    look,
                    "Drop…",
                    Some(Message::Action(Action::DropStash(name)))
                ),
            ]
            .spacing(6),
        ]
        .spacing(4)
        .padding(8),
        hline(look),
        files(app, &open.files, open.selected.as_deref(), None, "No files"),
    ];
    with_diff(app, tab, left.into(), "Select a file to see its changes")
}
