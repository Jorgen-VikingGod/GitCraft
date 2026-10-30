//! Window furniture: menu bar, tabs, toolbar, operation banner, status bar and the start page.

use gitcraft_core::model::Operation;
use iced::widget::{
    Column, button, column, container, mouse_area, pick_list, row, scrollable, space, text,
    text_input,
};
use iced::{Center, Color, Element, Length, Theme};

use super::icon::icon;
use super::style;
use super::widgets::{
    btn, danger_text, hline, icon_button, muted_text, primary, scrolled, single, spacer,
};
use crate::actions::{Action, MenuRequest};
use crate::app::{App, Message, RepoTab, SearchMode, View};
use crate::command::Command;
use crate::logic::{plural, short_sha};

pub fn menu_bar(app: &App) -> Element<'_, Message> {
    let look = &app.look;
    let entry = |label, request| {
        button(text(label).size(13))
            .padding([3, 8])
            .on_press(Message::OpenMenu(request))
            .style(style::tool(look))
    };
    container(row![
        entry("File", MenuRequest::BarFile),
        entry("View", MenuRequest::BarView),
        entry("Help", MenuRequest::BarHelp)
    ])
    .padding([1, 2])
    .width(Length::Fill)
    .style(style::fill(look.c.toolbar))
    .into()
}

fn tab_view<'a>(app: &'a App, index: usize, tab: &'a RepoTab) -> Element<'a, Message> {
    let look = &app.look;
    let c = &look.c;
    let active = index == app.active;
    let color = if active { c.text } else { c.text_muted };
    let mut line = row![
        icon("repo", 13.0, color),
        single(tab.name.as_str(), 13)
            .font(look.semibold)
            .color(color)
    ]
    .spacing(6)
    .align_y(Center)
    .padding(iced::Padding {
        top: 0.0,
        bottom: 0.0,
        left: 12.0,
        right: 6.0,
    });
    if let Some(branch) = tab.current_branch() {
        line = line.push(single(branch.to_owned(), 11).color(c.text_muted));
    }
    let close = || {
        button(container(icon("x", 12.0, color)).center(Length::Fill))
            .width(20)
            .height(20)
            .padding(0)
            .on_press(Message::Close(index))
            .style(style::ghost(look, color))
    };
    // Inactive tabs only show their close button under the pointer, so keep room for it.
    line = if active {
        line.push(close())
    } else {
        line.push(space().width(20))
    };
    // The active tab has the page's background and an accent line along its top.
    let top: Element<'a, Message> = if active {
        container(space())
            .width(Length::Fill)
            .height(2)
            .style(style::fill(c.accent))
            .into()
    } else {
        space().height(2).into()
    };
    let background = if active { c.bg } else { c.titlebar };
    let body = column![
        top,
        container(line).height(Length::Fill).center_y(Length::Fill)
    ]
    .width(Length::Shrink);
    let surface = container(body)
        .height(Length::Fill)
        .max_width(260)
        .clip(true)
        .style(style::fill(background));
    let shown: Element<'a, Message> = if active {
        surface.into()
    } else {
        let tint = container(row![space::horizontal(), close()].align_y(Center).padding(
            iced::Padding {
                right: 6.0,
                top: 2.0,
                ..iced::Padding::ZERO
            },
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_y(Length::Fill)
        .style(style::fill(c.hover));
        iced::widget::hover(surface, tint)
    };
    row![
        mouse_area(shown)
            .on_press(Message::Activate(index))
            .on_middle_press(Message::Close(index))
            .on_right_press(Message::OpenMenu(MenuRequest::Tab(index))),
        container(space())
            .width(1)
            .height(Length::Fill)
            .style(style::fill(c.border)),
    ]
    .height(Length::Fill)
    .into()
}

pub fn tab_bar(app: &App) -> Element<'_, Message> {
    let look = &app.look;
    let c = &look.c;
    let brand = container(
        row![
            icon("merge", 16.0, c.accent),
            single("GitCraft", 13).font(look.bold).color(c.accent)
        ]
        .spacing(6)
        .align_y(Center),
    )
    .padding([0, 12])
    .height(Length::Fill)
    .center_y(Length::Fill);
    let mut tabs = row![].height(Length::Fill);
    for (index, tab) in app.tabs.iter().enumerate() {
        tabs = tabs.push(tab_view(app, index, tab));
    }
    let line = row![
        brand,
        scrollable(tabs)
            .height(Length::Fill)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(0).scroller_width(0)
            )),
        container(icon_button(look, "plus", 16.0, Some(Message::OpenDialog)))
            .center_y(Length::Fill)
            .padding([0, 4]),
        spacer(),
        container(icon_button(
            look,
            "settings",
            16.0,
            Some(Message::OpenSettings)
        ))
        .center_y(Length::Fill)
        .padding([0, 6]),
    ]
    .height(35);
    column![container(line).style(style::fill(c.titlebar)), hline(look)].into()
}

fn tool_button<'a>(
    app: &'a App,
    name: &'static str,
    label: &'a str,
    count: i64,
    message: Option<Message>,
) -> Element<'a, Message> {
    let look = &app.look;
    let c = &look.c;
    let enabled = message.is_some();
    let color = if enabled {
        c.text
    } else {
        Color { a: 0.45, ..c.text }
    };
    let body = column![icon(name, 18.0, color), text(label).size(11).color(color)]
        .spacing(2)
        .align_x(Center);
    let main = button(container(body).center(Length::Fill))
        .width(52)
        .height(44)
        .padding(0)
        .on_press_maybe(message)
        .style(style::tool(look));
    if count <= 0 {
        return main.into();
    }
    let accent = c.accent;
    let pill = container(text(count.to_string()).size(10).color(Color::WHITE))
        .padding([0, 4])
        .style(move |_: &Theme| container::Style {
            background: Some(iced::Background::Color(accent)),
            border: iced::border::rounded(8),
            ..container::Style::default()
        });
    iced::widget::stack![
        main,
        container(pill).align_right(52).padding(iced::Padding {
            top: 2.0,
            right: 4.0,
            bottom: 0.0,
            left: 0.0
        })
    ]
    .into()
}

pub fn toolbar(app: &App) -> Element<'_, Message> {
    let look = &app.look;
    let c = &look.c;
    let Some(tab) = app.current() else {
        return column![
            container(row![
                tool_button(app, "repo", "Open", 0, Some(Message::OpenDialog)),
                tool_button(
                    app,
                    "pull",
                    "Clone",
                    0,
                    Some(Message::Action(Action::Clone))
                ),
            ])
            .padding([4, 12])
            .width(Length::Fill)
            .style(style::fill(c.toolbar)),
            hline(look),
        ]
        .into();
    };
    let idle = app.busy.is_none();
    let when_idle = |message: Message| idle.then_some(message);
    let branch = tab.status.as_ref().map(|status| &status.branch);
    let mut info = row![].spacing(4);
    match branch {
        Some(b) if b.detached => {
            info = info.push(
                single(
                    format!("detached at {}", b.oid.as_deref().map_or("", short_sha)),
                    12,
                )
                .color(c.text_muted),
            );
        }
        Some(b) => {
            info = info.push(
                single(b.head.clone().unwrap_or_else(|| "…".to_owned()), 12).color(c.text_muted),
            );
            if let Some(upstream) = &b.upstream {
                info = info.push(single(format!("→ {upstream}"), 12).color(c.text_faint));
            }
        }
        None => info = info.push(single("…", 12).color(c.text_muted)),
    }
    let search = container(
        row![
            icon("search", 14.0, c.text_muted),
            text_input("Search commits (Ctrl+F)", &tab.search_input)
                .id(app.search_box.clone())
                .on_input(Message::SearchInput)
                .on_submit(Message::SearchSubmit)
                .size(13)
                .padding(0)
                .width(Length::Fill)
                .style(style::bare_input(look)),
            pick_list(
                SearchMode::ALL,
                Some(tab.search_mode),
                Message::SearchModeChanged
            )
            .text_size(12)
            .padding([2, 4])
            .style(style::plain_pick_list(look))
            .menu_style(style::menu(look)),
        ]
        .spacing(6)
        .align_y(Center),
    )
    .padding(iced::Padding {
        top: 0.0,
        bottom: 0.0,
        left: 10.0,
        right: 4.0,
    })
    .width(340)
    .height(30)
    .center_y(30)
    .style(style::search_frame(look));
    column![
        container(
            row![
                row![
                    tool_button(
                        app,
                        "fetch",
                        "Fetch",
                        0,
                        when_idle(Message::Run(Command::Fetch {
                            remote: None,
                            tags: false
                        }))
                    ),
                    tool_button(
                        app,
                        "pull",
                        "Pull",
                        branch.map_or(0, |b| b.behind),
                        when_idle(Message::Action(Action::Pull))
                    ),
                    tool_button(
                        app,
                        "push",
                        "Push",
                        branch.map_or(0, |b| b.ahead),
                        when_idle(Message::Action(Action::Push(None)))
                    ),
                ]
                .spacing(2),
                row![
                    tool_button(
                        app,
                        "stash",
                        "Stash",
                        0,
                        when_idle(Message::Action(Action::Stash))
                    ),
                    tool_button(
                        app,
                        "branch",
                        "Branch",
                        0,
                        when_idle(Message::Action(Action::NewBranch {
                            start: None,
                            label: None
                        }))
                    ),
                    tool_button(
                        app,
                        "more",
                        "More",
                        0,
                        Some(Message::OpenMenu(MenuRequest::Repository))
                    ),
                ]
                .spacing(2),
                container(
                    column![single(tab.name.as_str(), 13).font(look.bold), info].align_x(Center)
                )
                .center_x(Length::Fill)
                .clip(true),
                search,
            ]
            .spacing(14)
            .align_y(Center),
        )
        .padding([0, 12])
        .height(51)
        .center_y(51)
        .width(Length::Fill)
        .style(style::fill(c.toolbar)),
        hline(look),
    ]
    .into()
}

/// Shown while a merge, rebase, cherry-pick or revert is stopped half-way.
pub fn operation_banner<'a>(app: &'a App, tab: &'a RepoTab) -> Option<Element<'a, Message>> {
    let look = &app.look;
    let c = &look.c;
    let status = tab.status.as_ref()?;
    let operation = status.operation.as_ref()?;
    let conflicts = status.conflicts.len();
    let mut title = match operation {
        Operation::Merge { .. } => "Merge in progress".to_owned(),
        Operation::Rebase { .. } => "Rebase in progress".to_owned(),
        Operation::CherryPick { .. } => "Cherry-pick in progress".to_owned(),
        Operation::Revert { .. } => "Revert in progress".to_owned(),
        Operation::Am => "Applying patches (git am)".to_owned(),
    };
    if let Operation::Rebase {
        step: Some(step),
        total: Some(total),
        ..
    } = operation
    {
        title.push_str(&format!(" · step {step} of {total}"));
    }
    if let Operation::Rebase {
        branch: Some(branch),
        ..
    } = operation
    {
        title.push_str(&format!(" · {branch}"));
    }
    let hint = if conflicts > 0 {
        format!(
            "{} conflicted. Resolve them in Local Changes, then continue.",
            plural(conflicts, "file")
        )
    } else if let Operation::Rebase {
        stopped_sha: Some(sha),
        ..
    } = operation
    {
        format!(
            "Stopped at {}. Amend or make changes, then continue.",
            short_sha(sha)
        )
    } else {
        "All conflicts resolved. Continue to finish.".to_owned()
    };
    let symbol = if conflicts > 0 {
        icon("alert", 16.0, c.danger)
    } else {
        icon("merge", 16.0, c.warning)
    };
    let mut line = row![
        symbol,
        column![
            text(title).size(13).font(look.semibold),
            muted_text(look, hint, 12)
        ]
        .width(Length::Fill)
    ]
    .spacing(10)
    .align_y(Center);
    if conflicts > 0 {
        line = line.push(btn(
            look,
            "Show conflicts",
            Some(Message::SetView(View::Changes)),
        ));
    }
    if !matches!(operation, Operation::Merge { .. }) {
        line = line.push(btn(look, "Skip", Some(Message::Run(Command::Skip))));
    }
    line = line.push(danger_text(
        look,
        "Abort",
        Some(Message::Action(Action::Abort)),
    ));
    line = line.push(primary(
        look,
        "Continue",
        (conflicts == 0).then_some(Message::Run(Command::Continue)),
    ));
    Some(
        column![
            container(line)
                .padding([6, 12])
                .width(Length::Fill)
                .style(style::fill(c.warning_bg)),
            hline(look)
        ]
        .into(),
    )
}

pub fn status_bar(app: &App) -> Element<'_, Message> {
    let look = &app.look;
    let c = &look.c;
    let mut line = row![].spacing(10).align_y(Center);
    if let Some((_, label)) = &app.busy {
        line = line.push(single(format!("{label}…"), 12).font(look.semibold));
        if !app.progress_line.is_empty() {
            line = line.push(
                container(
                    single(app.progress_line.as_str(), 12)
                        .font(look.mono)
                        .color(c.text_muted),
                )
                .clip(true),
            );
        }
        if app.cancel.is_some() {
            line = line.push(
                button(text("Cancel").size(11))
                    .padding([0, 6])
                    .on_press(Message::Cancel)
                    .style(style::ghost(look, c.text)),
            );
        }
    } else {
        let path = app
            .current()
            .map_or("No repository open", |tab| tab.path.as_str());
        line = line.push(container(single(path, 12).color(c.text_muted)).clip(true));
    }
    line = line.push(spacer());
    if let Some(tab) = app.current() {
        let more = if tab.has_more { "+" } else { "" };
        line = line
            .push(single(format!("{}{more} commits", tab.commits.len()), 12).color(c.text_muted));
    }
    let success = c.success;
    line = line.push(
        container(space())
            .width(8)
            .height(8)
            .style(move |_: &Theme| container::Style {
                background: Some(iced::Background::Color(success)),
                border: iced::border::rounded(4),
                ..container::Style::default()
            }),
    );
    column![
        hline(look),
        container(line)
            .padding([0, 12])
            .height(23)
            .center_y(23)
            .width(Length::Fill)
            .clip(true)
            .style(style::fill(c.titlebar))
    ]
    .into()
}

pub fn welcome(app: &App) -> Element<'_, Message> {
    let look = &app.look;
    let c = &look.c;
    let mut recent: Column<'_, Message> = column![].spacing(2);
    for repo in &app.recent {
        let exists = repo.exists.unwrap_or(true);
        let label = row![
            icon("repo", 16.0, c.text_muted),
            column![
                single(repo.name.as_str(), 13).font(look.semibold),
                single(repo.path.as_str(), 12).color(c.text_muted)
            ],
        ]
        .spacing(10)
        .align_y(Center)
        .padding([5, 10]);
        let open = button(label)
            .width(Length::Fill)
            .padding(0)
            .on_press_maybe(exists.then(|| Message::OpenPath(repo.path.clone())))
            .style(style::tool(look));
        recent = recent.push(
            row![
                open,
                icon_button(look, "x", 13.0, Some(Message::Forget(repo.path.clone())))
            ]
            .spacing(4)
            .align_y(Center),
        );
    }
    let large = |label: &'static str, message: Message, primary: bool| {
        let base =
            button(container(text(label).size(13).font(look.semibold)).center_y(Length::Fill))
                .height(30)
                .padding([0, 14])
                .on_press(message);
        if primary {
            base.style(style::button_primary(look))
        } else {
            base.style(style::button(look))
        }
    };
    let mut page = column![
        row![
            icon("merge", 26.0, c.accent),
            text("GitCraft").size(22).font(look.bold).color(c.accent)
        ]
        .spacing(10)
        .align_y(Center),
        row![
            large("Open a repository…", Message::OpenDialog, true),
            large("Clone…", Message::Action(Action::Clone), false),
            large("New…", Message::Action(Action::Init), false),
        ]
        .spacing(8),
    ]
    .spacing(16)
    .max_width(620);
    if !app.recent.is_empty() {
        page = page.push(text("RECENT").size(11).font(look.bold).color(c.text_muted));
        page = page.push(scrolled(look, recent).height(Length::Fill));
    }
    container(page).padding(40).center_x(Length::Fill).into()
}
