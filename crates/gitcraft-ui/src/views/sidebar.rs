//! Navigation, plus branches, remotes, tags, stashes and submodules as collapsible trees.

use iced::widget::{Column, button, column, container, mouse_area, row, space, text, text_input};
use iced::{Center, Color, Element, Length, Theme};

use super::icon::icon;
use super::style;
use super::widgets::{hoverable, icon_button, scrolled, single, spacer};
use crate::actions::{Action, MenuRequest, RefTarget};
use crate::app::{App, Message, RepoTab, View};
use crate::command::Command;
use crate::logic::build_tree;
use crate::theme::Look;

const INDENT: f32 = 14.0;
const BASE_INDENT: f32 = 8.0;

fn nav<'a>(
    look: &Look,
    name: &'static str,
    label: &'static str,
    active: bool,
    count: usize,
    message: Message,
) -> Element<'a, Message> {
    let c = &look.c;
    let color = if active { c.selection_text } else { c.text };
    let mut line = row![
        icon(name, 14.0, color),
        text(label).size(13).font(look.semibold).color(color),
        spacer()
    ]
    .spacing(8)
    .align_y(Center);
    if count > 0 {
        let accent = c.accent;
        line = line.push(
            container(text(count.to_string()).size(11).color(Color::WHITE))
                .padding([1, 6])
                .style(move |_: &Theme| container::Style {
                    background: Some(iced::Background::Color(accent)),
                    border: iced::border::rounded(10),
                    ..container::Style::default()
                }),
        );
    }
    button(line.height(Length::Fill))
        .width(Length::Fill)
        .height(28)
        .padding([0, 10])
        .on_press(message)
        .style(style::nav_item(look, active))
        .into()
}

fn section<'a>(
    app: &App,
    key: &str,
    title: &str,
    count: Option<usize>,
    add: Option<Message>,
) -> Element<'a, Message> {
    let look = &app.look;
    let c = &look.c;
    let name = if app.collapsed.contains(key) {
        "chevron-right"
    } else {
        "chevron-down"
    };
    let mut line = row![
        icon(name, 12.0, c.text_muted),
        single(title.to_uppercase(), 11)
            .font(look.bold)
            .color(c.text_muted),
        spacer()
    ]
    .spacing(4)
    .align_y(Center)
    .height(28)
    .padding(iced::Padding {
        top: 0.0,
        bottom: 0.0,
        left: 6.0,
        right: 8.0,
    });
    if let Some(count) = count {
        line = line.push(single(count.to_string(), 11).color(c.text_faint));
    }
    let header = mouse_area(line).on_press(Message::ToggleCollapsed(key.to_owned()));
    let Some(add) = add else {
        return container(header)
            .padding(iced::Padding {
                top: 6.0,
                ..iced::Padding::ZERO
            })
            .into();
    };
    // The add button only appears while the pointer is over the header.
    let extra = container(
        row![
            space::horizontal(),
            icon_button(look, "plus", 13.0, Some(add))
        ]
        .align_y(Center)
        .padding([0, 4]),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_y(Length::Fill);
    container(iced::widget::hover(header, extra))
        .padding(iced::Padding {
            top: 6.0,
            ..iced::Padding::ZERO
        })
        .into()
}

fn none<'a>(look: &Look, note: &'a str) -> Element<'a, Message> {
    container(
        text(note)
            .size(12)
            .font(look.italic)
            .color(look.c.text_faint),
    )
    .padding(iced::Padding {
        top: 2.0,
        bottom: 4.0,
        left: 26.0,
        right: 0.0,
    })
    .into()
}

struct Item<'a> {
    label: String,
    depth: usize,
    symbol: &'static str,
    selected: bool,
    current: bool,
    badges: Vec<Element<'a, Message>>,
}

fn folder<'a>(
    app: &App,
    key: &str,
    label: &str,
    depth: usize,
    symbol: &'static str,
    extra: Option<Message>,
) -> Element<'a, Message> {
    let look = &app.look;
    let c = &look.c;
    let chevron = if app.collapsed.contains(key) {
        "chevron-right"
    } else {
        "chevron-down"
    };
    let line = row![
        icon(chevron, 12.0, c.text_muted),
        icon(symbol, 13.0, c.text_muted),
        single(label.to_owned(), 13).color(c.text_muted)
    ]
    .spacing(6)
    .align_y(Center)
    .height(24)
    .padding(iced::Padding {
        top: 0.0,
        bottom: 0.0,
        left: BASE_INDENT + depth as f32 * INDENT,
        right: 10.0,
    });
    let mut area = mouse_area(hoverable(look, container(line).width(Length::Fill), 5.0))
        .on_press(Message::ToggleCollapsed(key.to_owned()));
    if let Some(extra) = extra {
        area = area.on_right_press(extra);
    }
    container(area).padding([0, 4]).width(Length::Fill).into()
}

fn leaf<'a>(look: &Look, item: Item<'a>) -> Element<'a, Message> {
    let c = &look.c;
    let text_color = if item.selected {
        c.selection_text
    } else {
        c.text
    };
    let symbol_color = if item.selected {
        c.selection_text
    } else if item.current {
        c.accent
    } else {
        c.text_muted
    };
    let label = single(item.label, 13)
        .color(text_color)
        .font(if item.current { look.bold } else { look.font });
    let mut line = row![
        icon(item.symbol, 13.0, symbol_color),
        container(label).width(Length::Fill).clip(true)
    ]
    .spacing(6)
    .align_y(Center)
    .height(24)
    .padding(iced::Padding {
        top: 0.0,
        bottom: 0.0,
        left: BASE_INDENT + 18.0 + item.depth as f32 * INDENT,
        right: 10.0,
    });
    for badge in item.badges {
        line = line.push(badge);
    }
    let fill = item.selected.then_some(c.selection);
    let body = container(line)
        .width(Length::Fill)
        .style(style::pill(fill, 5.0));
    container(hoverable(look, body, 5.0))
        .padding([0, 4])
        .width(Length::Fill)
        .into()
}

pub fn view<'a>(app: &'a App, tab: &'a RepoTab) -> Element<'a, Message> {
    let look = &app.look;
    let c = &look.c;
    let filter = tab.filter.to_lowercase();
    let matches = |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter);
    let in_history = tab.view == View::History;
    let is_selected = |sha: &str| in_history && tab.selected.as_deref() == Some(sha);
    let collapsed = |key: &str| app.collapsed.contains(key);

    let head = column![
        column![
            nav(
                look,
                "edit",
                "Local Changes",
                tab.view == View::Changes,
                tab.change_count(),
                Message::SetView(View::Changes)
            ),
            nav(
                look,
                "history",
                "All Commits",
                in_history,
                0,
                Message::SetView(View::History)
            ),
        ]
        .spacing(1)
        .padding(iced::Padding {
            top: 8.0,
            bottom: 4.0,
            left: 6.0,
            right: 6.0
        }),
        container(
            text_input("Filter branches & tags", &tab.filter)
                .on_input(Message::FilterInput)
                .size(12)
                .padding([5, 8])
                .style(style::text_input(look))
        )
        .padding(iced::Padding {
            top: 4.0,
            bottom: 6.0,
            left: 8.0,
            right: 8.0
        }),
    ];
    let mut list: Column<'a, Message> = column![].width(Length::Fill).padding(iced::Padding {
        bottom: 16.0,
        ..iced::Padding::ZERO
    });

    // Branches
    let new_branch = Message::Action(Action::NewBranch {
        start: None,
        label: None,
    });
    list = list.push(section(
        app,
        "s:branches",
        "Branches",
        None,
        Some(new_branch),
    ));
    if !collapsed("s:branches") {
        let branches: Vec<_> = tab
            .refs
            .local
            .iter()
            .filter(|branch| matches(&branch.name))
            .collect();
        if branches.is_empty() {
            list = list.push(none(look, "No branches"));
        }
        let names: Vec<&str> = branches.iter().map(|branch| branch.name.as_str()).collect();
        for entry in build_tree("b", &names, collapsed) {
            let Some(index) = entry.item else {
                list = list.push(folder(
                    app,
                    &entry.key,
                    &entry.label,
                    entry.depth,
                    "folder",
                    None,
                ));
                continue;
            };
            let branch = branches[index];
            let mut badges: Vec<Element<'a, Message>> = Vec::new();
            if branch.upstream_gone {
                badges.push(single("gone", 11).font(look.mono).color(c.warning).into());
            }
            if branch.ahead > 0 {
                badges.push(
                    single(format!("↑{}", branch.ahead), 11)
                        .font(look.mono)
                        .color(c.text_muted)
                        .into(),
                );
            }
            if branch.behind > 0 {
                badges.push(
                    single(format!("↓{}", branch.behind), 11)
                        .font(look.mono)
                        .color(c.text_muted)
                        .into(),
                );
            }
            let item = Item {
                label: entry.label,
                depth: entry.depth,
                symbol: if branch.current { "check" } else { "branch" },
                selected: is_selected(&branch.sha),
                current: branch.current,
                badges,
            };
            list = list.push(
                mouse_area(leaf(look, item))
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
        "Remotes",
        None,
        Some(Message::Action(Action::AddRemote)),
    ));
    if !collapsed("s:remotes") {
        if tab.refs.remotes.is_empty() {
            list = list.push(none(look, "No remotes"));
        }
        for remote in &tab.refs.remotes {
            let key = format!("r:{}", remote.name);
            list = list.push(folder(
                app,
                &key,
                &remote.name,
                0,
                "cloud",
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
                    list = list.push(folder(
                        app,
                        &entry.key,
                        &entry.label,
                        entry.depth + 1,
                        "folder",
                        None,
                    ));
                    continue;
                };
                let branch = branches[index];
                let item = Item {
                    label: entry.label,
                    depth: entry.depth + 1,
                    symbol: "branch",
                    selected: is_selected(&branch.sha),
                    current: false,
                    badges: Vec::new(),
                };
                list = list.push(
                    mouse_area(leaf(look, item))
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
    list = list.push(section(app, "s:tags", "Tags", Some(tags.len()), None));
    if !collapsed("s:tags") {
        if tags.is_empty() {
            list = list.push(none(look, "No tags"));
        }
        for tag in tags {
            let item = Item {
                label: tag.name.clone(),
                depth: 0,
                symbol: "tag",
                selected: is_selected(&tag.sha),
                current: false,
                badges: Vec::new(),
            };
            list = list.push(
                mouse_area(leaf(look, item))
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
        "Stashes",
        Some(tab.refs.stashes.len()),
        None,
    ));
    if !collapsed("s:stashes") {
        if tab.refs.stashes.is_empty() {
            list = list.push(none(look, "No stashes"));
        }
        for stash in &tab.refs.stashes {
            let selected = tab.view == View::Stash
                && tab
                    .stash
                    .as_ref()
                    .is_some_and(|open| open.name == stash.name);
            let item = Item {
                label: stash.message.clone(),
                depth: 0,
                symbol: "stash",
                selected,
                current: false,
                badges: Vec::new(),
            };
            list = list.push(
                mouse_area(leaf(look, item))
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
            "Submodules",
            Some(tab.refs.submodules.len()),
            None,
        ));
        if !collapsed("s:submodules") {
            for module in &tab.refs.submodules {
                let mut badges: Vec<Element<'a, Message>> = Vec::new();
                if module.state != "ok" {
                    badges.push(
                        single(module.state.as_str(), 11)
                            .font(look.mono)
                            .color(c.text_muted)
                            .into(),
                    );
                }
                let item = Item {
                    label: module.path.clone(),
                    depth: 0,
                    symbol: "box",
                    selected: false,
                    current: false,
                    badges,
                };
                let full = crate::actions::join_path(&tab.path, &module.path);
                list = list.push(
                    mouse_area(leaf(look, item))
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

    container(column![head, scrolled(look, list).height(Length::Fill)])
        .height(Length::Fill)
        .width(Length::Fill)
        .style(style::fill(c.sidebar))
        .into()
}
