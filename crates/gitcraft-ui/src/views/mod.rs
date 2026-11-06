//! Everything drawn on screen. Views only read state and produce messages.
//!
//! egui draws immediately, so each frame calls [`root`], which walks the state and returns
//! the messages that the user's clicks and typing caused. The shell hands them to `App::update`.

mod changes;
mod details;
mod diff;
mod frame;
mod graph_cell;
mod history;
mod icon;
mod overlays;
mod popups;
mod shell;
mod sidebar;
mod widgets;

use std::cell::RefCell;

use egui::{Frame, Panel, Ui, Vec2};

use crate::app::{App, Message, RepoTab, Splitter, View};
use crate::theme::{Colors, Look};

/// What a view needs: the state to read and a place for the messages it produces.
pub struct Cx<'a> {
    pub app: &'a App,
    out: RefCell<Vec<Message>>,
}

impl<'a> Cx<'a> {
    pub fn new(app: &'a App) -> Self {
        Self {
            app,
            out: RefCell::new(Vec::new()),
        }
    }

    pub fn send(&self, message: Message) {
        self.out.borrow_mut().push(message);
    }

    pub fn look(&self) -> &'a Look {
        &self.app.look
    }

    pub fn c(&self) -> &'a Colors {
        &self.app.look.c
    }

    pub fn finish(self) -> Vec<Message> {
        self.out.into_inner()
    }
}

/// Draws a whole frame and returns the messages it caused.
pub fn root(ui: &mut Ui, app: &App) -> Vec<Message> {
    let cx = Cx::new(app);
    let c = cx.c();
    let flat = |color| Frame::NONE.fill(color);
    // With a window of our own making, the corners are rounded and what is outside is clear.
    let (top_fill, bottom_fill) = if app.csd {
        (egui::Color32::TRANSPARENT, egui::Color32::TRANSPARENT)
    } else {
        (c.bg, c.titlebar)
    };

    Panel::top("top")
        .frame(flat(top_fill))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            shell::menu_bar(ui, &cx);
            shell::tab_bar(ui, &cx);
            shell::toolbar(ui, &cx);
            if let Some(tab) = app.current() {
                shell::operation_banner(ui, &cx, tab);
            }
        });
    Panel::bottom("status")
        .frame(flat(bottom_fill))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            shell::status_bar(ui, &cx);
        });
    egui::CentralPanel::no_frame().show(ui, |ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        match app.current() {
            Some(tab) => workspace(ui, &cx, tab),
            None => shell::welcome(ui, &cx),
        }
    });

    let ctx = ui.ctx().clone();
    frame::window_edges(ui, &cx);
    if let Some(overlay) = &app.overlay {
        overlays::show(&ctx, &cx, overlay);
    }
    if let Some(form) = &app.form {
        popups::form(&ctx, &cx, form);
    }
    if let Some(menu) = &app.menu {
        popups::menu(ui, &cx, menu);
    }
    popups::toasts(&ctx, &cx);
    cx.finish()
}

fn workspace(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let app = cx.app;
    let rect = ui.available_rect_before_wrap();
    let side = app
        .settings
        .sidebar_width
        .min(rect.width() - 100.0)
        .max(100.0);
    let (left, rest) = widgets::split_x(rect, side);
    widgets::region(ui, left, |ui| sidebar::view(ui, cx, tab));
    let (grip, main) = widgets::split_x(rest, widgets::GRIP);
    widgets::splitter(ui, cx, grip, Splitter::Sidebar);
    widgets::region(ui, main, |ui| match tab.view {
        View::History => history::view(ui, cx, tab),
        View::Changes => changes::view(ui, cx, tab),
        View::Stash => details::stash(ui, cx, tab),
    });
}
