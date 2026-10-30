//! Full-window tools: reflog, blame, file history, conflict resolver, interactive rebase, settings.

use gitcraft_core::model::{BlameLine, Commit, ConflictChunk, ReflogEntry};
use iced::widget::text::Wrapping;
use iced::widget::{
    Column, button, checkbox, column, container, mouse_area, pick_list, row, scrollable, space,
    text, text_editor, text_input,
};
use iced::{Background, Center, Color, Element, Font, Length, Theme, mouse};
use native_theme_iced::{selection_background, styles};

use super::diff;
use super::history::format_time;
use super::popups::modal;
use super::widgets::{btn, hline, muted, placeholder, primary, single, spacer, toggle};
use crate::actions::{Action, MenuRequest};
use crate::app::{
    App, BLAME_ROW_HEIGHT, ConflictState, DiffPane, Message, Overlay, PaneKey, REBASE_ROW_HEIGHT,
    RebaseState,
};
use crate::logic::{Choice, has_conflict_markers, plural, short_sha};
use crate::theme::Look;

const REBASE_ACTIONS: [(&str, &str, &str); 6] = [
    ("pick", "p", "Use the commit"),
    ("reword", "r", "Use the commit, edit its message"),
    ("edit", "e", "Stop to amend the commit"),
    (
        "squash",
        "s",
        "Meld into the commit above, combine messages",
    ),
    (
        "fixup",
        "f",
        "Meld into the commit above, discard this message",
    ),
    ("drop", "d", "Remove the commit"),
];

fn dialog<'a>(
    app: &'a App,
    title: &'static str,
    subtitle: String,
    extra: Option<Element<'a, Message>>,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    let look = &app.look;
    let scale = app.scale();
    let (width, height) = (app.window.width / scale, app.window.height / scale);
    let mut header = row![
        column![
            text(title).size(look.text_size + 3.0),
            muted(subtitle, look.text_size - 1.0)
        ]
        .width(Length::Fill)
    ]
    .spacing(8)
    .align_y(Center);
    if let Some(extra) = extra {
        header = header.push(extra);
    }
    header = header.push(btn(look, "Close", Some(Message::OverlayClose)));
    modal(
        container(column![
            container(header).padding([10, 14]),
            hline(look),
            container(body).height(Length::Fill).width(Length::Fill)
        ])
        .width((width - 60.0).max(300.0))
        .height((height - 50.0).max(200.0))
        .style(styles::dialog(&look.resolved))
        .into(),
    )
}

fn scrolled<'a>(look: &Look, content: Column<'a, Message>) -> Element<'a, Message> {
    scrollable(content.width(Length::Fill))
        .height(Length::Fill)
        .direction(scrollable::Direction::Vertical(styles::scrollbar(
            &look.resolved,
        )))
        .style(styles::scrollable(&look.resolved))
        .into()
}

pub fn view<'a>(app: &'a App, overlay: &'a Overlay) -> Element<'a, Message> {
    match overlay {
        Overlay::Settings => settings(app),
        Overlay::Reflog {
            entries,
            loading,
            error,
        } => reflog(app, entries, *loading, error.as_deref()),
        Overlay::Blame {
            path,
            rev,
            lines,
            loading,
            error,
            offset,
            viewport,
            hovered,
        } => blame(
            app,
            path,
            rev.as_deref(),
            lines,
            *loading,
            error.as_deref(),
            (*offset, *viewport),
            hovered.as_deref(),
        ),
        Overlay::FileHistory {
            path,
            commits,
            selected,
            pane,
            loading,
            error,
        } => file_history(
            app,
            path,
            commits,
            selected.as_deref(),
            pane,
            *loading,
            error.as_deref(),
        ),
        Overlay::Conflict(state) => conflict(app, state),
        Overlay::Rebase(state) => rebase(app, state),
    }
}

fn reflog<'a>(
    app: &'a App,
    entries: &'a [ReflogEntry],
    loading: bool,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    let body: Element<'a, Message> = if loading {
        placeholder(look, "Loading…")
    } else if let Some(error) = error {
        placeholder(look, error)
    } else if entries.is_empty() {
        placeholder(look, "The reflog is empty")
    } else {
        let mut rows: Column<'a, Message> = column![];
        for entry in entries {
            let line = row![
                single(entry.selector.as_str(), size)
                    .font(Font::MONOSPACE)
                    .width(90),
                single(short_sha(&entry.sha), size)
                    .font(Font::MONOSPACE)
                    .width(66),
                container(muted(entry.action.as_str(), size).wrapping(Wrapping::None))
                    .width(Length::FillPortion(2))
                    .clip(true),
                container(single(entry.subject.as_str(), size))
                    .width(Length::FillPortion(3))
                    .clip(true),
                single(format_time(entry.time), size).width(112),
                btn(look, "Show", Some(Message::ShowCommit(entry.sha.clone()))),
                btn(
                    look,
                    "Branch…",
                    Some(Message::Action(Action::NewBranch {
                        start: Some(entry.sha.clone()),
                        label: None
                    }))
                ),
            ]
            .spacing(8)
            .align_y(Center)
            .padding([2, 12]);
            rows = rows.push(
                mouse_area(line)
                    .on_double_click(Message::ShowCommit(entry.sha.clone()))
                    .on_right_press(Message::OpenMenu(MenuRequest::Commit {
                        sha: entry.sha.clone(),
                        focus: None,
                    })),
            );
        }
        scrolled(look, rows)
    };
    dialog(
        app,
        "Reflog",
        "Every position HEAD has been at. Right-click an entry for all commit actions.".to_owned(),
        None,
        body,
    )
}

#[allow(clippy::too_many_arguments)]
fn blame<'a>(
    app: &'a App,
    path: &'a str,
    rev: Option<&'a str>,
    lines: &'a [BlameLine],
    loading: bool,
    error: Option<&'a str>,
    scroll: (f32, f32),
    hovered: Option<&'a str>,
) -> Element<'a, Message> {
    let look = &app.look;
    let size = 12.0;
    let subtitle = match rev {
        Some(rev) => format!("{path} @ {}", short_sha(rev)),
        None => path.to_owned(),
    };
    let extra = btn(
        look,
        "File history",
        Some(Message::OpenFileHistory(path.to_owned())),
    );
    let body: Element<'a, Message> = if loading {
        placeholder(look, "Loading…")
    } else if let Some(error) = error {
        placeholder(look, error)
    } else {
        let (offset, viewport) = scroll;
        let (min, max) = lines.iter().fold((i64::MAX, i64::MIN), |(lo, hi), line| {
            (lo.min(line.time), hi.max(line.time))
        });
        let span = (max - min).max(1) as f32;
        let count =
            (viewport.max(app.window.height / app.scale()) / BLAME_ROW_HEIGHT) as usize + 13;
        let first = ((offset / BLAME_ROW_HEIGHT) as usize)
            .saturating_sub(6)
            .min(lines.len().saturating_sub(count));
        let end = (first + count).min(lines.len());
        let mut rows: Vec<Element<'a, Message>> =
            vec![space().height(first as f32 * BLAME_ROW_HEIGHT).into()];
        for (index, line) in lines[first..end].iter().enumerate() {
            let at = first + index;
            let starts = at == 0 || lines[at - 1].sha != line.sha;
            let age = (line.time - min) as f32 / span;
            let bar = Color {
                a: 0.15 + age * 0.85,
                ..look.palette.remote
            };
            let gutter: Element<'a, Message> = if starts {
                row![
                    single(short_sha(&line.sha), size)
                        .font(Font::MONOSPACE)
                        .width(60),
                    container(single(line.author.as_str(), size))
                        .width(110)
                        .clip(true),
                    single(format_time(line.time), size).width(106),
                ]
                .spacing(4)
                .into()
            } else {
                space().width(284).into()
            };
            let background = (hovered == Some(line.sha.as_str())).then(|| Color {
                a: 0.2,
                ..selection_background(&look.resolved)
            });
            let content = row![
                container(space())
                    .width(3)
                    .height(BLAME_ROW_HEIGHT)
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(bar)),
                        ..container::Style::default()
                    }),
                container(gutter).width(290).clip(true),
                muted(line.line.to_string(), size)
                    .font(Font::MONOSPACE)
                    .width(44),
                single(line.text.as_str(), 13.0).font(Font::MONOSPACE),
            ]
            .spacing(4)
            .align_y(Center);
            let mut area = mouse_area(
                container(content)
                    .width(Length::Fill)
                    .height(BLAME_ROW_HEIGHT)
                    .clip(true)
                    .style(move |_: &Theme| container::Style {
                        background: background.map(Background::Color),
                        ..container::Style::default()
                    }),
            )
            .on_enter(Message::BlameHover(line.sha.clone()));
            // All zeros means the line isn't committed yet, so there's no commit to show.
            if line.sha.bytes().any(|b| b != b'0') {
                area = area.on_press(Message::ShowCommit(line.sha.clone()));
            }
            rows.push(area.into());
        }
        rows.push(
            space()
                .height((lines.len() - end) as f32 * BLAME_ROW_HEIGHT)
                .into(),
        );
        scrollable(column(rows).width(Length::Fill))
            .height(Length::Fill)
            .direction(scrollable::Direction::Vertical(styles::scrollbar(
                &look.resolved,
            )))
            .style(styles::scrollable(&look.resolved))
            .on_scroll(|v| Message::BlameScrolled(v.absolute_offset().y, v.bounds().height))
            .into()
    };
    dialog(app, "Blame", subtitle, Some(extra), body)
}

fn file_history<'a>(
    app: &'a App,
    path: &'a str,
    commits: &'a [Commit],
    selected: Option<&'a str>,
    pane: &'a DiffPane,
    loading: bool,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    let mut items: Column<'a, Message> = column![];
    if loading {
        items = items.push(container(muted("Loading…", size)).padding(10));
    } else if let Some(error) = error {
        items = items.push(container(text(error).size(size)).padding(10));
    }
    for commit in commits {
        let chosen = selected == Some(commit.sha.as_str());
        let status = commit.file.as_ref().map_or("", |file| file.status.as_str());
        let mut body = column![
            single(commit.subject.as_str(), size),
            muted(
                format!(
                    "{status}  {} · {} · {}",
                    commit.author,
                    format_time(commit.time),
                    short_sha(&commit.sha)
                ),
                size - 1.0
            )
            .wrapping(Wrapping::None),
        ]
        .spacing(1);
        if let Some(orig) = commit
            .file
            .as_ref()
            .and_then(|file| file.orig_path.as_ref())
        {
            body = body.push(muted(format!("renamed from {orig}"), size - 1.0));
        }
        let background = chosen.then(|| Color {
            a: 0.3,
            ..selection_background(&look.resolved)
        });
        items = items.push(
            mouse_area(
                container(body)
                    .padding([5, 10])
                    .width(Length::Fill)
                    .clip(true)
                    .style(move |_: &Theme| container::Style {
                        background: background.map(Background::Color),
                        ..container::Style::default()
                    }),
            )
            .on_press(Message::FileHistorySelect(commit.sha.clone())),
        );
    }
    let current = selected.and_then(|sha| commits.iter().find(|commit| commit.sha == sha));
    let actions: Option<Element<'a, Message>> = current.map(|commit| {
        let target = commit
            .file
            .as_ref()
            .map_or(path, |file| file.path.as_str())
            .to_owned();
        let can_blame = commit.file.as_ref().is_none_or(|file| file.status != "D");
        row![
            btn(
                look,
                "Show in history",
                Some(Message::ShowCommit(commit.sha.clone()))
            ),
            btn(
                look,
                "Blame at this commit",
                can_blame.then(|| Message::OpenBlame(target, Some(commit.sha.clone())))
            ),
        ]
        .spacing(6)
        .into()
    });
    let body = row![
        container(scrolled(look, items))
            .width(360)
            .height(Length::Fill),
        container(diff::view(
            app,
            pane,
            PaneKey::Overlay,
            None,
            "Select a commit",
            actions,
            app.window.width / app.scale() - 420.0
        ))
        .width(Length::Fill)
        .height(Length::Fill),
    ];
    dialog(app, "File history", path.to_owned(), None, body.into())
}

fn conflict<'a>(app: &'a App, state: &'a ConflictState) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    let body: Element<'a, Message> = if let Some(error) = &state.error {
        placeholder(look, error.as_str())
    } else if let Some(data) = &state.data {
        let resolve = |side: Option<&str>| {
            Message::Action(Action::ConflictResolve {
                path: state.path.clone(),
                side: side.map(str::to_owned),
            })
        };
        let count = state.conflict_count();
        let mut summary = plural(count, "conflict");
        if count > 0 {
            summary.push_str(&format!(" · {} unresolved", state.unresolved()));
        }
        let mut bar = row![text(summary).size(size), spacer()]
            .spacing(6)
            .align_y(Center)
            .padding([6, 12]);
        if count > 0 {
            bar = bar
                .push(btn(
                    look,
                    "All ours",
                    Some(Message::ConflictChooseAll(Choice::Ours)),
                ))
                .push(btn(
                    look,
                    "All theirs",
                    Some(Message::ConflictChooseAll(Choice::Theirs)),
                ));
        }
        bar = bar
            .push(btn(look, "Use our file", Some(resolve(Some("ours")))))
            .push(btn(look, "Use their file", Some(resolve(Some("theirs")))));

        let mut chunks: Column<'a, Message> = column![].spacing(10).padding(12);
        if count == 0 {
            let note = if data.working.is_none() {
                "The file was deleted on one side. Keep one side, or mark it as resolved to keep the current state."
            } else {
                "No conflict markers found (binary file, or the markers were already removed). Edit the result below if needed, then mark the file as resolved."
            };
            chunks = chunks.push(muted(note, size)).push(btn(
                look,
                "Mark as resolved",
                Some(resolve(None)),
            ));
        }
        let code = |lines: &[String]| {
            let joined = lines.join("\n");
            text(if joined.is_empty() {
                "(empty)".to_owned()
            } else {
                joined
            })
            .size(12.5)
            .font(Font::MONOSPACE)
        };
        let mut index = 0;
        for (at, chunk) in data.chunks.iter().enumerate() {
            let ConflictChunk::Conflict { ours, theirs, .. } = chunk else {
                continue;
            };
            let choice = state.choices.get(index).copied().flatten();
            let which = index;
            index += 1;
            let choose = |label: &'static str, option: Choice| {
                toggle(
                    look,
                    label,
                    choice == Some(option),
                    Message::ConflictChoose(which, option),
                )
            };
            let mut block = column![
                row![
                    text(format!("Conflict {}", which + 1)).size(size),
                    spacer(),
                    choose("Ours", Choice::Ours),
                    choose("Theirs", Choice::Theirs),
                    choose("Ours + theirs", Choice::OursTheirs),
                    choose("Theirs + ours", Choice::TheirsOurs),
                ]
                .spacing(4)
                .align_y(Center),
            ]
            .spacing(6);
            if let Some(ConflictChunk::Common { lines }) =
                at.checked_sub(1).and_then(|before| data.chunks.get(before))
            {
                let tail: Vec<String> = lines.iter().rev().take(3).rev().cloned().collect();
                if !tail.is_empty() {
                    block = block.push(muted(tail.join("\n"), 12.5).font(Font::MONOSPACE));
                }
            }
            let taken = |ours_side: bool| match choice {
                Some(Choice::Ours) => ours_side,
                Some(Choice::Theirs) => !ours_side,
                Some(_) => true,
                None => false,
            };
            let side = |label: &'a str, lines: &[String], tint: Color, picked: bool| {
                let background = Color {
                    a: if picked { 0.28 } else { 0.1 },
                    ..tint
                };
                container(column![muted(label, size - 1.0), code(lines)].spacing(3))
                    .padding(6)
                    .width(Length::FillPortion(1))
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(background)),
                        ..container::Style::default()
                    })
            };
            block = block.push(
                row![
                    side(&data.ours_label, ours, look.palette.local, taken(true)),
                    side(
                        &data.theirs_label,
                        theirs,
                        look.palette.remote,
                        taken(false)
                    )
                ]
                .spacing(6),
            );
            chunks = chunks.push(
                container(block)
                    .padding(8)
                    .width(Length::Fill)
                    .style(styles::container_card(&look.resolved)),
            );
        }
        let content = state.result.text();
        let mut head = row![text("Result").size(size)].spacing(6);
        if state.edited {
            head = head.push(muted("· edited by hand", size));
        }
        if has_conflict_markers(&content) {
            head =
                head.push(single("· contains conflict markers", size).color(look.palette.modified));
        }
        column![
            bar,
            hline(look),
            container(scrolled(look, chunks)).height(Length::FillPortion(3)),
            hline(look),
            container(
                column![
                    head,
                    text_editor(&state.result)
                        .on_action(Message::ConflictEdit)
                        .height(Length::Fill)
                        .font(Font::MONOSPACE)
                        .size(12.5)
                        .style(styles::text_editor(&look.resolved)),
                ]
                .spacing(4)
            )
            .padding([6, 12])
            .height(Length::FillPortion(2)),
            hline(look),
            container(
                row![
                    muted("Saving writes the result to the file and stages it.", size),
                    spacer(),
                    btn(look, "Cancel", Some(Message::OverlayClose)),
                    primary(
                        look,
                        "Save & mark resolved",
                        data.working.is_some().then_some(Message::ConflictSave)
                    ),
                ]
                .spacing(6)
                .align_y(Center)
            )
            .padding([8, 12]),
        ]
        .into()
    } else {
        placeholder(look, "Loading…")
    };
    dialog(app, "Resolve conflict", state.path.clone(), None, body)
}

fn action_button<'a>(
    look: &Look,
    index: usize,
    action: &'static str,
    current: &'static str,
) -> Element<'a, Message> {
    let label = REBASE_ACTIONS
        .iter()
        .find(|entry| entry.0 == action)
        .map_or(action, |entry| entry.1);
    button(
        text(label.to_uppercase())
            .size(look.text_size - 2.0)
            .font(Font::MONOSPACE),
    )
    .padding([1, 6])
    .on_press(Message::RebaseSetAction(index, action))
    .style(styles::button_toggle(&look.resolved, action == current))
    .into()
}

fn rebase<'a>(app: &'a App, state: &'a RebaseState) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    let branch = app
        .current()
        .and_then(|tab| tab.current_branch())
        .unwrap_or("HEAD");
    let subtitle = format!(
        "Rewrite {branch} on top of {}. Commits are applied from top to bottom.",
        state.base_label
    );
    let list: Element<'a, Message> = if state.loading {
        placeholder(look, "Loading commits…")
    } else if let Some(error) = &state.error {
        placeholder(look, error.as_str())
    } else if state.steps.is_empty() {
        placeholder(look, "No commits between the base and HEAD.")
    } else {
        let mut rows: Column<'a, Message> = column![];
        for (index, step) in state.steps.iter().enumerate() {
            let mut actions = row![].spacing(2);
            for (action, _, _) in REBASE_ACTIONS {
                actions = actions.push(action_button(look, index, action, step.action));
            }
            let handle = mouse_area(container(single("⠿", size + 2.0)).width(20).center_x(20))
                .on_press(Message::RebaseDragStart(index))
                .interaction(mouse::Interaction::Grab);
            let subject: Element<'a, Message> = if step.action == "reword" {
                text_input("Commit message", &step.message)
                    .on_input(move |value| Message::RebaseMessage(index, value))
                    .size(size)
                    .style(styles::text_input(&look.resolved))
                    .into()
            } else if step.action == "drop" {
                container(muted(step.commit.subject.as_str(), size).wrapping(Wrapping::None))
                    .clip(true)
                    .into()
            } else {
                container(single(step.commit.subject.as_str(), size))
                    .clip(true)
                    .into()
            };
            let line = row![
                handle,
                actions,
                single(short_sha(&step.commit.sha), size)
                    .font(Font::MONOSPACE)
                    .width(66),
                container(subject).width(Length::Fill),
                container(single(step.commit.author.as_str(), size))
                    .width(140)
                    .clip(true),
                btn(
                    look,
                    "↑",
                    (index > 0).then_some(Message::RebaseMove(index, -1))
                ),
                btn(
                    look,
                    "↓",
                    (index + 1 < state.steps.len()).then_some(Message::RebaseMove(index, 1))
                ),
            ]
            .spacing(8)
            .align_y(Center)
            .padding([0, 12])
            .height(REBASE_ROW_HEIGHT);
            let background = (state.focused == index).then(|| Color {
                a: 0.2,
                ..selection_background(&look.resolved)
            });
            rows = rows.push(
                mouse_area(container(line).width(Length::Fill).style(move |_: &Theme| {
                    container::Style {
                        background: background.map(Background::Color),
                        ..container::Style::default()
                    }
                }))
                .on_press(Message::RebaseFocus(index)),
            );
        }
        scrolled(look, rows)
    };
    let mut legend = column![].spacing(1).padding([4, 12]);
    for (action, key, hint) in REBASE_ACTIONS {
        legend = legend.push(muted(format!("{key}  {action}: {hint}"), size - 2.0));
    }
    let problem = state.problem().filter(|_| !state.loading);
    let footer = row![
        checkbox(state.autostash)
            .label("Stash local changes automatically")
            .text_size(size)
            .on_toggle(Message::RebaseAutostash)
            .style(styles::checkbox(&look.resolved)),
        text(problem.unwrap_or(""))
            .size(size)
            .color(look.palette.removed),
        spacer(),
        btn(
            look,
            "Reset",
            (!state.unchanged()).then_some(Message::RebaseReset)
        ),
        btn(look, "Cancel", Some(Message::OverlayClose)),
        primary(
            look,
            "Start rebase",
            (problem.is_none() && !state.loading).then_some(Message::RebaseStart)
        ),
    ]
    .spacing(8)
    .align_y(Center)
    .padding([8, 12]);
    let body = column![
        container(list).height(Length::Fill),
        hline(look),
        legend,
        hline(look),
        footer
    ];
    dialog(app, "Interactive rebase", subtitle, None, body.into())
}

#[derive(Clone, PartialEq, Eq)]
struct ThemeChoice(&'static str, &'static str);

impl std::fmt::Display for ThemeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1)
    }
}

const THEMES: [ThemeChoice; 3] = [
    ThemeChoice("system", "Follow system"),
    ThemeChoice("light", "Light"),
    ThemeChoice("dark", "Dark"),
];

fn settings(app: &App) -> Element<'_, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    let values = &app.settings;
    let heading = |title: &'static str| muted(title, size);
    let line = |label: &'static str, control: Element<'static, Message>| {
        row![text(label).size(size).width(Length::Fill), control].align_y(Center)
    };
    let chosen = THEMES
        .iter()
        .find(|choice| choice.0 == values.theme)
        .cloned();
    let number = |value: String, message: fn(String) -> Message| -> Element<'static, Message> {
        text_input("", &value)
            .on_input(message)
            .size(size)
            .width(90)
            .style(styles::text_input(&look.resolved))
            .into()
    };
    let check = |label: &'static str, value: bool, message: fn(bool) -> Message| {
        checkbox(value)
            .label(label)
            .text_size(size)
            .on_toggle(message)
            .style(styles::checkbox(&look.resolved))
    };
    let shortcut = |keys: &'static str, what: &'static str| {
        row![single(keys, size).width(170), muted(what, size)]
    };
    let body = column![
        heading("Appearance"),
        line(
            "Theme",
            pick_list(THEMES, chosen, |choice: ThemeChoice| Message::SetTheme(
                choice.0.to_owned()
            ))
            .text_size(size)
            .style(styles::pick_list(&look.resolved))
            .menu_style(styles::menu(&look.resolved))
            .into()
        ),
        heading("Diffs"),
        line(
            "Layout",
            row![
                toggle(
                    look,
                    "Unified",
                    !values.diff_split,
                    Message::SetSplit(false)
                ),
                toggle(
                    look,
                    "Side by side",
                    values.diff_split,
                    Message::SetSplit(true)
                )
            ]
            .spacing(4)
            .into()
        ),
        line(
            "Context lines",
            number(values.diff_context.to_string(), Message::SetContext)
        ),
        check(
            "Ignore whitespace changes",
            values.ignore_whitespace,
            Message::SetIgnoreWhitespace
        ),
        check(
            "Syntax highlighting",
            values.syntax_highlight,
            Message::SetSyntax
        ),
        heading("Repository"),
        line(
            "Commits loaded at a time",
            number(values.log_limit.to_string(), Message::SetLogLimit)
        ),
        check(
            "Fetch when opening a repository",
            values.fetch_on_open,
            Message::SetFetchOnOpen
        ),
        check(
            "Keep a commit-graph file for faster history",
            values.commit_graph,
            Message::SetCommitGraph
        ),
        heading("Keyboard shortcuts"),
        shortcut("Ctrl/⌘ + 1 / 2", "All commits / Local changes"),
        shortcut("Ctrl/⌘ + F", "Search commits"),
        shortcut("Ctrl/⌘ + T or O", "Open repository"),
        shortcut("Ctrl/⌘ + W", "Close tab"),
        shortcut("Ctrl/⌘ + Enter", "Commit"),
        shortcut("Ctrl/⌘ + + / − / 0", "Zoom"),
        shortcut("F5 / Ctrl/⌘ + R", "Refresh repository"),
        shortcut("↑ ↓ PgUp PgDn", "Move through commits"),
    ]
    .spacing(8)
    .padding(16)
    .max_width(560);
    dialog(
        app,
        "Settings",
        String::new(),
        None,
        scrollable(container(body).center_x(Length::Fill))
            .direction(scrollable::Direction::Vertical(styles::scrollbar(
                &look.resolved,
            )))
            .style(styles::scrollable(&look.resolved))
            .into(),
    )
}
