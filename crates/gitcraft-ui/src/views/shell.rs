//! Window furniture: menu bar, tabs, toolbar, operation banner, status bar and the start page.

use gitcraft_core::model::Operation;
use iced::widget::text::Wrapping;
use iced::widget::{
    Column, button, column, container, mouse_area, pick_list, row, scrollable, text, text_input,
};
use iced::{Center, Element, Length};
use native_theme_iced::styles;

use super::widgets::{self, btn, hline, muted, primary, single, spacer};
use crate::actions::{Action, MenuRequest};
use crate::app::{App, Message, RepoTab, SearchMode, View};
use crate::command::Command;
use crate::logic::{plural, short_sha};

pub fn menu_bar(app: &App) -> Element<'_, Message> {
    let look = &app.look;
    let entry = |label, request| {
        button(text(label).size(look.text_size - 1.0))
            .padding([2, 8])
            .on_press(Message::OpenMenu(request))
            .style(styles::button_link(&look.resolved))
    };
    container(row![
        entry("File", MenuRequest::BarFile),
        entry("View", MenuRequest::BarView),
        entry("Help", MenuRequest::BarHelp)
    ])
    .padding([1, 4])
    .width(Length::Fill)
    .into()
}

pub fn tab_bar(app: &App) -> Element<'_, Message> {
    let look = &app.look;
    let mut tabs = row![].spacing(2).padding([3, 6]).align_y(Center);
    for (index, tab) in app.tabs.iter().enumerate() {
        let label = text(tab.name.as_str())
            .size(look.text_size)
            .wrapping(Wrapping::None);
        let select = button(label)
            .padding([3, 9])
            .on_press(Message::Activate(index))
            .style(styles::button_toggle(&look.resolved, index == app.active));
        let close = button(text("×").size(look.text_size))
            .padding([3, 7])
            .on_press(Message::Close(index))
            .style(styles::button(&look.resolved));
        tabs = tabs.push(
            mouse_area(row![select, close].spacing(1))
                .on_middle_press(Message::Close(index))
                .on_right_press(Message::OpenMenu(MenuRequest::Tab(index))),
        );
    }
    tabs = tabs.push(btn(look, "+", Some(Message::OpenDialog)));
    column![
        scrollable(tabs).direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(0).scroller_width(0)
        )),
        hline(look),
    ]
    .into()
}

pub fn toolbar(app: &App) -> Element<'_, Message> {
    let look = &app.look;
    let size = look.text_size;
    let Some(tab) = app.current() else {
        return container(
            row![
                btn(look, "Open…", Some(Message::OpenDialog)),
                btn(look, "Clone…", Some(Message::Action(Action::Clone))),
                spacer(),
                btn(look, "Settings", Some(Message::OpenSettings)),
            ]
            .spacing(6),
        )
        .padding(6)
        .width(Length::Fill)
        .style(styles::toolbar(&look.resolved))
        .into();
    };
    let idle = app.busy.is_none();
    let when_idle = |message: Message| idle.then_some(message);
    let branch = tab.status.as_ref().map(|status| &status.branch);
    let counted = |label: &str, count: i64| {
        if count > 0 {
            format!("{label} {count}")
        } else {
            label.to_owned()
        }
    };
    let pull = counted("Pull", branch.map_or(0, |b| b.behind));
    let push = counted("Push", branch.map_or(0, |b| b.ahead));
    let info = match branch {
        Some(b) if b.detached => format!("detached at {}", b.oid.as_deref().map_or("", short_sha)),
        Some(b) => match &b.upstream {
            Some(upstream) => format!("{} → {upstream}", b.head.as_deref().unwrap_or("…")),
            None => b.head.clone().unwrap_or_else(|| "…".to_owned()),
        },
        None => "…".to_owned(),
    };
    container(
        row![
            btn(
                look,
                "Fetch",
                when_idle(Message::Run(Command::Fetch {
                    remote: None,
                    tags: false
                }))
            ),
            btn(look, pull, when_idle(Message::Action(Action::Pull))),
            btn(look, push, when_idle(Message::Action(Action::Push(None)))),
            btn(look, "Stash", when_idle(Message::Action(Action::Stash))),
            btn(
                look,
                "Branch",
                when_idle(Message::Action(Action::NewBranch {
                    start: None,
                    label: None
                }))
            ),
            btn(
                look,
                "More",
                Some(Message::OpenMenu(MenuRequest::Repository))
            ),
            container(column![
                single(tab.name.as_str(), size),
                muted(info, size - 2.0).wrapping(Wrapping::None)
            ])
            .padding([0, 12])
            .width(Length::Fill)
            .clip(true),
            text_input("Search commits (Ctrl+F)", &tab.search_input)
                .id(app.search_box.clone())
                .on_input(Message::SearchInput)
                .on_submit(Message::SearchSubmit)
                .size(size - 1.0)
                .width(240)
                .style(styles::text_input(&look.resolved)),
            pick_list(
                SearchMode::ALL,
                Some(tab.search_mode),
                Message::SearchModeChanged
            )
            .text_size(size - 1.0)
            .style(styles::pick_list(&look.resolved))
            .menu_style(styles::menu(&look.resolved)),
            btn(look, "Settings", Some(Message::OpenSettings)),
        ]
        .spacing(6)
        .align_y(Center),
    )
    .padding(6)
    .width(Length::Fill)
    .style(styles::toolbar(&look.resolved))
    .into()
}

/// Shown while a merge, rebase, cherry-pick or revert is stopped half-way.
pub fn operation_banner<'a>(app: &'a App, tab: &'a RepoTab) -> Option<Element<'a, Message>> {
    let look = &app.look;
    let size = look.text_size - 1.0;
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
    let mut line = row![
        column![
            text(title).size(size).color(look.palette.modified),
            muted(hint, size - 1.0)
        ]
        .width(Length::Fill)
    ]
    .spacing(6)
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
    line = line.push(widgets::danger(
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
        container(line)
            .padding([6, 10])
            .width(Length::Fill)
            .style(styles::container_card(&look.resolved))
            .into(),
    )
}

pub fn status_bar(app: &App) -> Element<'_, Message> {
    let size = app.look.text_size - 1.0;
    let mut line = row![].spacing(16).align_y(Center);
    if let Some((_, label)) = &app.busy {
        line = line.push(
            text(format!("{label}…"))
                .size(size)
                .color(app.look.palette.modified),
        );
        if !app.progress_line.is_empty() {
            line = line.push(muted(app.progress_line.as_str(), size).wrapping(Wrapping::None));
        }
        if app.cancel.is_some() {
            line = line.push(btn(&app.look, "Cancel", Some(Message::Cancel)));
        }
    }
    line = line.push(spacer());
    if let Some(tab) = app.current() {
        let more = if tab.has_more { "+" } else { "" };
        line = line
            .push(single(plural(tab.change_count(), "change"), size))
            .push(single(format!("{}{more} commits", tab.commits.len()), size))
            .push(muted(tab.path.as_str(), size).wrapping(Wrapping::None));
    }
    container(line)
        .padding([3, 8])
        .width(Length::Fill)
        .clip(true)
        .style(styles::toolbar(&app.look.resolved))
        .into()
}

pub fn welcome(app: &App) -> Element<'_, Message> {
    let look = &app.look;
    let size = look.text_size;
    let mut recent: Column<'_, Message> = column![].spacing(4);
    for repo in &app.recent {
        let open = button(column![
            text(repo.name.as_str()).size(size),
            muted(repo.path.as_str(), size - 2.0)
        ])
        .width(Length::Fill)
        .on_press_maybe(
            repo.exists
                .unwrap_or(true)
                .then(|| Message::OpenPath(repo.path.clone())),
        )
        .style(styles::button(&look.resolved));
        recent = recent.push(
            row![
                open,
                btn(look, "Forget", Some(Message::Forget(repo.path.clone())))
            ]
            .spacing(4)
            .align_y(Center),
        );
    }
    let mut page = column![
        text("GitCraft").size(size * 2.0),
        row![
            primary(look, "Open a repository…", Some(Message::OpenDialog)),
            btn(look, "Clone…", Some(Message::Action(Action::Clone))),
            btn(look, "New…", Some(Message::Action(Action::Init))),
        ]
        .spacing(8),
    ]
    .spacing(14)
    .max_width(620);
    if !app.recent.is_empty() {
        page = page.push(text("Recent").size(size));
        page = page.push(
            scrollable(recent)
                .height(Length::Fill)
                .style(styles::scrollable(&look.resolved)),
        );
    }
    container(page).padding(40).center_x(Length::Fill).into()
}
