//! Local changes: file lists, commit box and the diff with line staging.

use gitcraft_core::model::FileChange;
use iced::widget::{
    Column, button, checkbox, column, container, mouse_area, row, scrollable, text, text_editor,
    text_input,
};
use iced::{Center, Element, Length};
use native_theme_iced::styles;

use super::diff;
use super::widgets::{
    btn, danger, file_row, hline, muted, placeholder, primary, single, spacer, splitter,
};
use crate::actions::{Action, MenuRequest};
use crate::app::{App, Area, Message, PaneKey, RepoTab, Splitter};
use crate::command::Command;
use crate::logic::plural;

const SUBJECT_LIMIT: usize = 72;

fn list<'a>(
    app: &'a App,
    tab: &'a RepoTab,
    area: Area,
    files: &[FileChange],
    empty: &'a str,
) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    if files.is_empty() {
        return container(muted(empty, size)).padding([6, 10]).into();
    }
    let picked = if tab.picked.0 == area {
        tab.picked.1.as_slice()
    } else {
        &[]
    };
    let (action_label, action): (&str, fn(String) -> Message) = match area {
        Area::Unstaged => ("+", |path| {
            Message::Run(Command::Stage { paths: vec![path] })
        }),
        Area::Staged => ("−", |path| {
            Message::Run(Command::Unstage { paths: vec![path] })
        }),
        Area::Conflicts => ("…", Message::OpenConflict),
    };
    let mut rows: Column<'a, Message> = column![];
    for file in files {
        let path = file.path.clone();
        let selected = picked.contains(&path);
        let quick = button(text(action_label).size(size))
            .padding([0, 7])
            .on_press(action(path.clone()))
            .style(styles::button(&look.resolved));
        rows = rows.push(
            mouse_area(
                row![
                    container(file_row(look, file, selected)).width(Length::Fill),
                    quick
                ]
                .align_y(Center)
                .padding([0, 4]),
            )
            .on_press(Message::FilePicked(area, path.clone()))
            .on_double_click(Message::FileActivated(area, path.clone()))
            .on_right_press(Message::OpenMenu(MenuRequest::File {
                area: Some(area),
                path,
                rev: None,
            })),
        );
    }
    scrollable(rows.width(Length::Fill))
        .height(Length::Fill)
        .direction(scrollable::Direction::Vertical(styles::scrollbar(
            &look.resolved,
        )))
        .style(styles::scrollable(&look.resolved))
        .into()
}

fn commit_box<'a>(app: &'a App, tab: &'a RepoTab) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    let staged = tab.status.as_ref().map_or(0, |status| status.staged.len());
    let has_head = tab
        .status
        .as_ref()
        .is_some_and(|status| status.branch.oid.is_some());
    let label = if tab.committing {
        "Committing…".to_owned()
    } else if tab.amend {
        "Amend commit".to_owned()
    } else if staged > 0 {
        format!("Commit {}", plural(staged, "file"))
    } else {
        "Commit".to_owned()
    };
    let length = tab.summary.chars().count();
    let counter = single(length.to_string(), size - 1.0)
        .color_maybe((length > SUBJECT_LIMIT).then_some(look.palette.modified));
    column![
        row![
            text_input("Commit summary", &tab.summary)
                .on_input(Message::SummaryInput)
                .on_submit(Message::CommitNow)
                .size(size)
                .style(styles::text_input(&look.resolved)),
            counter,
        ]
        .spacing(6)
        .align_y(Center),
        text_editor(&tab.body)
            .placeholder("Description")
            .on_action(Message::BodyAction)
            .height(80)
            .size(size)
            .style(styles::text_editor(&look.resolved)),
        row![
            checkbox(tab.amend)
                .label("Amend last commit")
                .text_size(size)
                .on_toggle_maybe(has_head.then_some(Message::AmendToggled))
                .style(styles::checkbox(&look.resolved)),
            spacer(),
            primary(look, label, tab.can_commit().then_some(Message::CommitNow)),
        ]
        .align_y(Center),
    ]
    .spacing(6)
    .padding(8)
    .into()
}

pub fn view<'a>(app: &'a App, tab: &'a RepoTab) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    let conflicts = tab.files_in(Area::Conflicts);
    let unstaged = tab.files_in(Area::Unstaged);
    let staged = tab.files_in(Area::Staged);
    let title = |label: String| single(label, size);
    let mut lists: Column<'a, Message> = column![];
    if !conflicts.is_empty() {
        lists = lists
            .push(
                container(
                    title(format!("Conflicts ({})", conflicts.len())).color(look.palette.removed),
                )
                .padding([4, 8]),
            )
            .push(
                container(list(app, tab, Area::Conflicts, &conflicts, ""))
                    .height(Length::FillPortion(1)),
            )
            .push(hline(look));
    }
    let mut unstaged_header = row![title(format!("Unstaged ({})", unstaged.len())), spacer()]
        .spacing(6)
        .align_y(Center)
        .padding([4, 8]);
    if !unstaged.is_empty() {
        let every: Vec<String> = unstaged.iter().map(|file| file.path.clone()).collect();
        unstaged_header = unstaged_header
            .push(btn(
                look,
                "Discard all",
                Some(Message::Action(Action::Discard(every))),
            ))
            .push(btn(
                look,
                "Stage all",
                Some(Message::Run(Command::StageAll)),
            ));
    }
    let mut staged_header = row![title(format!("Staged ({})", staged.len())), spacer()]
        .align_y(Center)
        .padding([4, 8]);
    if !staged.is_empty() {
        staged_header = staged_header.push(btn(
            look,
            "Unstage all",
            Some(Message::Run(Command::UnstageAll)),
        ));
    }
    lists = lists
        .push(unstaged_header)
        .push(
            container(list(
                app,
                tab,
                Area::Unstaged,
                &unstaged,
                "No unstaged changes",
            ))
            .height(Length::FillPortion(2)),
        )
        .push(hline(look))
        .push(staged_header)
        .push(
            container(list(app, tab, Area::Staged, &staged, "Nothing staged yet"))
                .height(Length::FillPortion(2)),
        )
        .push(hline(look))
        .push(commit_box(app, tab));

    let right: Element<'a, Message> = match &tab.focus {
        Some((Area::Conflicts, path)) => {
            let resolve = |side: Option<&str>| {
                Some(Message::Action(Action::ConflictResolve {
                    path: path.clone(),
                    side: side.map(str::to_owned),
                }))
            };
            container(
                column![
                    text(format!("{path} has conflicts")).size(size + 3.0),
                    muted(
                        "Resolve the conflicting sections, then the file is marked as resolved.",
                        size
                    ),
                    row![
                        primary(
                            look,
                            "Open merge tool",
                            Some(Message::OpenConflict(path.clone()))
                        ),
                        btn(look, "Use ours", resolve(Some("ours"))),
                        btn(look, "Use theirs", resolve(Some("theirs"))),
                        btn(look, "Mark as resolved", resolve(None)),
                    ]
                    .spacing(6),
                ]
                .spacing(10)
                .align_x(Center),
            )
            .center(Length::Fill)
            .into()
        }
        Some((area, path)) => {
            let actions: Element<'a, Message> = if *area == Area::Unstaged {
                row![
                    danger(
                        look,
                        "Discard file",
                        Some(Message::Action(Action::Discard(vec![path.clone()])))
                    ),
                    btn(
                        look,
                        "Stage file",
                        Some(Message::Run(Command::Stage {
                            paths: vec![path.clone()]
                        }))
                    ),
                ]
                .spacing(6)
                .into()
            } else {
                btn(
                    look,
                    "Unstage file",
                    Some(Message::Run(Command::Unstage {
                        paths: vec![path.clone()],
                    })),
                )
            };
            let mode = if *area == Area::Unstaged {
                "stage"
            } else {
                "unstage"
            };
            let width = app.window.width / app.scale()
                - app.settings.sidebar_width
                - app.settings.changes_width
                - 10.0;
            diff::view(
                app,
                &tab.changes_pane,
                PaneKey::Changes,
                Some(mode),
                "Select a file to see its changes",
                Some(actions),
                width,
            )
        }
        None => {
            let note = if tab.change_count() > 0 {
                "Select a file to see its changes"
            } else {
                "Working tree is clean"
            };
            placeholder(look, note)
        }
    };
    row![
        container(lists)
            .width(app.settings.changes_width)
            .height(Length::Fill),
        splitter(app, Splitter::Changes),
        container(right).width(Length::Fill).height(Length::Fill),
    ]
    .into()
}
