//! Navigation, plus branches, remotes, tags, stashes and submodules as collapsible trees.

use iced::widget::{
    Column, button, column, container, mouse_area, row, scrollable, text, text_input,
};
use iced::{Background, Center, Color, Element, Length, Theme};
use native_theme_iced::{selection_background, styles};

use super::widgets::{muted, single, spacer, toggle};
use crate::actions::{Action, MenuRequest, RefTarget};
use crate::app::{App, Message, RepoTab, View};
use crate::command::Command;
use crate::logic::build_tree;
use crate::theme::Look;

const INDENT: f32 = 12.0;

fn bold() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Bold,
        ..iced::Font::default()
    }
}

fn section<'a>(app: &App, key: &str, title: String, add: Option<Message>) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    let chevron = if app.collapsed.contains(key) {
        "▸"
    } else {
        "▾"
    };
    let mut line = row![
        single(chevron, size),
        single(title, size).font(bold()),
        spacer()
    ]
    .spacing(5)
    .align_y(Center);
    if let Some(add) = add {
        line = line.push(
            button(text("+").size(size))
                .padding([0, 6])
                .on_press(add)
                .style(styles::button(&look.resolved)),
        );
    }
    mouse_area(container(line).padding(iced::Padding {
        top: 9.0,
        bottom: 3.0,
        left: 8.0,
        right: 8.0,
    }))
    .on_press(Message::ToggleCollapsed(key.to_owned()))
    .into()
}

fn folder<'a>(
    app: &App,
    key: &str,
    label: &str,
    depth: usize,
    extra: Option<Message>,
) -> Element<'a, Message> {
    let size = app.look.text_size - 1.0;
    let chevron = if app.collapsed.contains(key) {
        "▸"
    } else {
        "▾"
    };
    let line = row![single(chevron, size), single(label.to_owned(), size)]
        .spacing(5)
        .padding(iced::Padding {
            top: 1.0,
            bottom: 1.0,
            left: 10.0 + depth as f32 * INDENT,
            right: 4.0,
        });
    let mut area = mouse_area(line).on_press(Message::ToggleCollapsed(key.to_owned()));
    if let Some(extra) = extra {
        area = area.on_right_press(extra);
    }
    area.into()
}

fn leaf<'a>(
    look: &Look,
    label: String,
    depth: usize,
    selected: bool,
    color: Option<Color>,
    badges: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let size = look.text_size - 1.0;
    let text_color = if selected {
        Some(look.selected_text)
    } else {
        color
    };
    let mut line = row![single(label, size).color_maybe(text_color)]
        .spacing(4)
        .align_y(Center)
        .padding(iced::Padding {
            top: 1.0,
            bottom: 1.0,
            left: 22.0 + depth as f32 * INDENT,
            right: 8.0,
        });
    if !badges.is_empty() {
        line = line.push(spacer());
        for badge in badges {
            line = line.push(badge);
        }
    }
    let background = selected.then(|| selection_background(&look.resolved));
    container(line)
        .width(Length::Fill)
        .clip(true)
        .style(move |_: &Theme| container::Style {
            background: background.map(Background::Color),
            ..container::Style::default()
        })
        .into()
}

pub fn view<'a>(app: &'a App, tab: &'a RepoTab) -> Element<'a, Message> {
    let look = &app.look;
    let size = look.text_size - 1.0;
    let filter = tab.filter.to_lowercase();
    let matches = |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter);
    let shown_in_history = tab.view == View::History;
    let is_selected = |sha: &str| shown_in_history && tab.selected.as_deref() == Some(sha);
    let collapsed = |key: &str| app.collapsed.contains(key);

    let count = tab.change_count();
    let changes = if count > 0 {
        format!("Local Changes ({count})")
    } else {
        "Local Changes".to_owned()
    };
    let mut list: Column<'a, Message> = column![
        row![
            toggle(
                look,
                changes,
                tab.view == View::Changes,
                Message::SetView(View::Changes)
            ),
            toggle(
                look,
                "All Commits",
                tab.view == View::History,
                Message::SetView(View::History)
            ),
        ]
        .spacing(4)
        .padding(6),
        container(
            text_input("Filter branches & tags", &tab.filter)
                .on_input(Message::FilterInput)
                .size(size)
                .style(styles::text_input(&look.resolved))
        )
        .padding([0, 6]),
    ];

    // Branches
    let new_branch = Message::Action(Action::NewBranch {
        start: None,
        label: None,
    });
    list = list.push(section(
        app,
        "s:branches",
        "Branches".to_owned(),
        Some(new_branch),
    ));
    if !collapsed("s:branches") {
        let branches: Vec<_> = tab
            .refs
            .local
            .iter()
            .filter(|branch| matches(&branch.name))
            .collect();
        let names: Vec<&str> = branches.iter().map(|branch| branch.name.as_str()).collect();
        for entry in build_tree("b", &names, collapsed) {
            let Some(index) = entry.item else {
                list = list.push(folder(app, &entry.key, &entry.label, entry.depth, None));
                continue;
            };
            let branch = branches[index];
            let mut badges: Vec<Element<'a, Message>> = Vec::new();
            if branch.upstream_gone {
                badges.push(muted("gone", size - 1.0).into());
            }
            if branch.ahead > 0 {
                badges.push(
                    single(format!("↑{}", branch.ahead), size - 1.0)
                        .color(look.palette.added)
                        .into(),
                );
            }
            if branch.behind > 0 {
                badges.push(
                    single(format!("↓{}", branch.behind), size - 1.0)
                        .color(look.palette.modified)
                        .into(),
                );
            }
            let color = branch.current.then_some(look.palette.local);
            let label = if branch.current {
                format!("✓ {}", entry.label)
            } else {
                entry.label
            };
            list = list.push(
                mouse_area(leaf(
                    look,
                    label,
                    entry.depth,
                    is_selected(&branch.sha),
                    color,
                    badges,
                ))
                .on_press(Message::ShowCommit(branch.sha.clone()))
                .on_double_click(Message::Run(Command::Checkout {
                    reference: branch.name.clone(),
                    kind: "local",
                }))
                .on_right_press(Message::OpenMenu(MenuRequest::Commit {
                    sha: branch.sha.clone(),
                    focus: Some(RefTarget::Local(branch.name.clone())),
                })),
            );
        }
    }

    // Remotes
    list = list.push(section(
        app,
        "s:remotes",
        "Remotes".to_owned(),
        Some(Message::Action(Action::AddRemote)),
    ));
    if !collapsed("s:remotes") {
        for remote in &tab.refs.remotes {
            let key = format!("r:{}", remote.name);
            list = list.push(folder(
                app,
                &key,
                &remote.name,
                0,
                Some(Message::OpenMenu(MenuRequest::Remote(remote.name.clone()))),
            ));
            if collapsed(&key) {
                continue;
            }
            let branches: Vec<_> = tab
                .refs
                .remote
                .iter()
                .filter(|branch| branch.remote == remote.name && matches(&branch.name))
                .collect();
            let names: Vec<&str> = branches
                .iter()
                .map(|branch| branch.branch.as_str())
                .collect();
            for entry in build_tree(&key, &names, collapsed) {
                let Some(index) = entry.item else {
                    list = list.push(folder(app, &entry.key, &entry.label, entry.depth + 1, None));
                    continue;
                };
                let branch = branches[index];
                list = list.push(
                    mouse_area(leaf(
                        look,
                        entry.label,
                        entry.depth + 1,
                        is_selected(&branch.sha),
                        None,
                        Vec::new(),
                    ))
                    .on_press(Message::ShowCommit(branch.sha.clone()))
                    .on_double_click(Message::Run(Command::Checkout {
                        reference: branch.name.clone(),
                        kind: "remote",
                    }))
                    .on_right_press(Message::OpenMenu(MenuRequest::Commit {
                        sha: branch.sha.clone(),
                        focus: Some(RefTarget::Remote(branch.name.clone())),
                    })),
                );
            }
        }
    }

    // Tags
    let tags: Vec<_> = tab
        .refs
        .tags
        .iter()
        .filter(|tag| matches(&tag.name))
        .collect();
    list = list.push(section(
        app,
        "s:tags",
        format!("Tags ({})", tags.len()),
        None,
    ));
    if !collapsed("s:tags") {
        for tag in tags {
            list = list.push(
                mouse_area(leaf(
                    look,
                    tag.name.clone(),
                    0,
                    is_selected(&tag.sha),
                    None,
                    Vec::new(),
                ))
                .on_press(Message::ShowCommit(tag.sha.clone()))
                .on_right_press(Message::OpenMenu(MenuRequest::Commit {
                    sha: tag.sha.clone(),
                    focus: Some(RefTarget::Tag(tag.name.clone())),
                })),
            );
        }
    }

    // Stashes
    list = list.push(section(
        app,
        "s:stashes",
        format!("Stashes ({})", tab.refs.stashes.len()),
        None,
    ));
    if !collapsed("s:stashes") {
        for stash in &tab.refs.stashes {
            let selected = tab.view == View::Stash
                && tab
                    .stash
                    .as_ref()
                    .is_some_and(|open| open.name == stash.name);
            list = list.push(
                mouse_area(leaf(
                    look,
                    stash.message.clone(),
                    0,
                    selected,
                    None,
                    Vec::new(),
                ))
                .on_press(Message::OpenStash(stash.name.clone()))
                .on_right_press(Message::OpenMenu(MenuRequest::Stash(stash.name.clone()))),
            );
        }
    }

    // Submodules
    if !tab.refs.submodules.is_empty() {
        list = list.push(section(
            app,
            "s:submodules",
            format!("Submodules ({})", tab.refs.submodules.len()),
            None,
        ));
        if !collapsed("s:submodules") {
            for module in &tab.refs.submodules {
                let mut badges: Vec<Element<'a, Message>> = Vec::new();
                if module.state != "ok" {
                    badges.push(muted(module.state.as_str(), size - 1.0).into());
                }
                let full = crate::actions::join_path(&tab.path, &module.path);
                list = list.push(
                    mouse_area(leaf(look, module.path.clone(), 0, false, None, badges))
                        .on_double_click(Message::OpenPath(full))
                        .on_right_press(Message::OpenMenu(MenuRequest::File {
                            area: None,
                            path: module.path.clone(),
                            rev: None,
                        })),
                );
            }
        }
    }

    scrollable(list.width(Length::Fill))
        .height(Length::Fill)
        .direction(scrollable::Direction::Vertical(styles::scrollbar(
            &look.resolved,
        )))
        .style(styles::scrollable(&look.resolved))
        .into()
}
