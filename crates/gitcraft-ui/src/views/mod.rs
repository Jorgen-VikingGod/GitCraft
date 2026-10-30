//! Everything drawn on screen. Views only read state and produce messages.

mod changes;
mod details;
mod diff;
mod graph_cell;
mod history;
mod overlays;
mod popups;
mod shell;
mod sidebar;
mod widgets;

use iced::widget::{column, container, row, stack};
use iced::{Element, Length};

use crate::app::{App, Message, RepoTab, Splitter, View};

pub fn root(app: &App) -> Element<'_, Message> {
    let mut page = column![
        shell::menu_bar(app),
        shell::tab_bar(app),
        shell::toolbar(app)
    ];
    if let Some(tab) = app.current()
        && let Some(banner) = shell::operation_banner(app, tab)
    {
        page = page.push(banner);
    }
    let body: Element<'_, Message> = match app.current() {
        Some(tab) => workspace(app, tab),
        None => shell::welcome(app),
    };
    page = page.push(container(body).width(Length::Fill).height(Length::Fill));
    page = page.push(shell::status_bar(app));

    let mut layers: Vec<Element<'_, Message>> = vec![page.into()];
    if let Some(overlay) = &app.overlay {
        layers.push(overlays::view(app, overlay));
    }
    if let Some(form) = &app.form {
        layers.push(popups::form(app, form));
    }
    if let Some(menu) = &app.menu {
        layers.push(popups::menu(app, menu));
    }
    if !app.toasts.is_empty() {
        layers.push(popups::toasts(app));
    }
    stack(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn workspace<'a>(app: &'a App, tab: &'a RepoTab) -> Element<'a, Message> {
    let main: Element<'a, Message> = match tab.view {
        View::History => history::view(app, tab),
        View::Changes => changes::view(app, tab),
        View::Stash => details::stash(app, tab),
    };
    row![
        container(sidebar::view(app, tab))
            .width(app.settings.sidebar_width)
            .height(Length::Fill),
        widgets::splitter(app, Splitter::Sidebar),
        container(main).width(Length::Fill).height(Length::Fill),
    ]
    .into()
}
