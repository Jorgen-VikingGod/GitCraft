//! Window furniture: menu bar, tabs, toolbar, operation banner, status bar and the start page.

use egui::text::{CCursor, CCursorRange};
use egui::{
    Align, Align2, Color32, Frame, Id, Key, Layout, Margin, Pos2, Rect, ScrollArea, Sense, Stroke,
    StrokeKind, Ui, vec2,
};
use gitcraft_core::model::Operation;

use super::widgets::{
    bar, bare_edit, btn, choice, danger_text, hline, icon_button, muted, paint_text, primary,
    scroll, text, text_width, tool_text,
};
use super::{Cx, icon};
use crate::actions::{Action, MenuRequest};
use crate::app::{RepoTab, SearchMode, View};
use crate::command::Command;
use crate::logic::{plural, short_sha};

pub fn menu_bar(ui: &mut Ui, cx: &Cx) {
    let look = cx.look();
    bar(ui, 26.0, cx.c().toolbar, 4.0, |ui| {
        for (label, request) in [
            ("File", MenuRequest::BarFile),
            ("View", MenuRequest::BarView),
            ("Help", MenuRequest::BarHelp),
        ] {
            if tool_text(ui, look, label, 13.0).clicked() {
                cx.send(crate::app::Message::OpenMenu(request));
            }
        }
    });
}

fn tab_width(ui: &Ui, cx: &Cx, tab: &RepoTab) -> f32 {
    let look = cx.look();
    let mut width =
        12.0 + 13.0 + 6.0 + text_width(ui, &tab.name, &look.ui(13.0)) + 6.0 + 20.0 + 6.0;
    if let Some(branch) = tab.current_branch() {
        width += 6.0 + text_width(ui, branch, &look.ui(11.0));
    }
    width.min(260.0)
}

fn tab_view(ui: &mut Ui, cx: &Cx, index: usize, tab: &RepoTab) {
    let look = cx.look();
    let c = &look.c;
    let active = index == cx.app.active;
    let width = tab_width(ui, cx, tab);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 35.0), Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 0.0, if active { c.bg } else { c.titlebar });
    if active {
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(rect.width(), 2.0)),
            0.0,
            c.accent,
        );
    } else if hovered {
        painter.rect_filled(rect, 0.0, c.hover);
    }
    let color = if active { c.text } else { c.text_muted };
    let mid = rect.center().y + 1.0;
    icon::paint(
        ui,
        "repo",
        Rect::from_center_size(Pos2::new(rect.left() + 18.5, mid), vec2(13.0, 13.0)),
        color,
    );
    let close = Rect::from_center_size(Pos2::new(rect.right() - 16.0, mid), vec2(20.0, 20.0));
    let text_area = painter.with_clip_rect(Rect::from_min_max(
        rect.min,
        Pos2::new(close.left() - 2.0, rect.max.y),
    ));
    let mut x = rect.left() + 31.0;
    x += paint_text(
        &text_area,
        Pos2::new(x, mid),
        &tab.name,
        look.ui(13.0),
        color,
    ) + 6.0;
    if let Some(branch) = tab.current_branch() {
        paint_text(
            &text_area,
            Pos2::new(x, mid),
            branch,
            look.ui(11.0),
            c.text_muted,
        );
    }
    // Inactive tabs only show their close button under the pointer.
    let mut closed = false;
    if active || hovered {
        let button = ui.interact(close, response.id.with("close"), Sense::click());
        if button.hovered() {
            painter.rect_filled(close, 6.0, c.hover_strong);
        }
        icon::paint(
            ui,
            "x",
            Rect::from_center_size(close.center(), vec2(12.0, 12.0)),
            color,
        );
        closed = button.clicked();
    }
    ui.painter().line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(1.0, c.border),
    );
    if closed || response.middle_clicked() {
        cx.send(crate::app::Message::Close(index));
    } else if response.clicked() {
        cx.send(crate::app::Message::Activate(index));
    } else if response.secondary_clicked() {
        cx.send(crate::app::Message::OpenMenu(MenuRequest::Tab(index)));
    }
}

pub fn tab_bar(ui: &mut Ui, cx: &Cx) {
    use crate::app::Message;
    let look = cx.look();
    let c = &look.c;
    bar(ui, 35.0, c.titlebar, 0.0, |ui| {
        ui.add_space(12.0);
        icon::icon(ui, "merge", 16.0, c.accent);
        text(ui, "GitCraft", 13.0, c.accent);
        ui.add_space(8.0);
        let reserve = 90.0;
        ScrollArea::horizontal()
            .id_salt("tabs")
            .max_width((ui.available_width() - reserve).max(60.0))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                ui.horizontal(|ui| {
                    for (index, tab) in cx.app.tabs.iter().enumerate() {
                        tab_view(ui, cx, index, tab);
                    }
                });
            });
        if icon_button(ui, look, "plus", 16.0, true) {
            cx.send(Message::OpenDialog);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(6.0);
            if icon_button(ui, look, "settings", 16.0, true) {
                cx.send(Message::OpenSettings);
            }
        });
    });
    hline(ui, look);
}

fn tool_button(
    ui: &mut Ui,
    cx: &Cx,
    at: Pos2,
    name: &str,
    label: &str,
    count: i64,
    enabled: bool,
) -> bool {
    let look = cx.look();
    let c = &look.c;
    let rect = Rect::from_min_size(at, vec2(52.0, 44.0));
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, Id::unique(("tool", label)), sense);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 6.0, c.hover);
    }
    let color = if enabled {
        c.text
    } else {
        c.text.gamma_multiply(0.45)
    };
    icon::paint(
        ui,
        name,
        Rect::from_center_size(
            Pos2::new(rect.center().x, rect.top() + 15.0),
            vec2(18.0, 18.0),
        ),
        color,
    );
    ui.painter().text(
        Pos2::new(rect.center().x, rect.top() + 34.0),
        Align2::CENTER_CENTER,
        label,
        look.ui(11.0),
        color,
    );
    if count > 0 {
        let corner = Pos2::new(rect.right() - 4.0, rect.top() + 2.0);
        let width = (text_width(ui, &count.to_string(), &look.ui(10.0)) + 12.0).max(16.0);
        let pill = Rect::from_min_size(Pos2::new(corner.x - width, corner.y), vec2(width, 14.0));
        ui.painter().rect_filled(pill, 7.0, c.accent);
        ui.painter().text(
            pill.center(),
            Align2::CENTER_CENTER,
            count,
            look.ui(10.0),
            Color32::WHITE,
        );
    }
    enabled && response.clicked()
}

fn search_box(ui: &mut Ui, cx: &Cx, tab: &RepoTab, rect: Rect) {
    use crate::app::Message;
    let look = cx.look();
    let c = &look.c;
    ui.painter().rect_filled(rect, 8.0, c.input);
    ui.painter()
        .rect_stroke(rect, 8.0, Stroke::new(1.0, c.border), StrokeKind::Inside);
    icon::paint(
        ui,
        "search",
        Rect::from_center_size(
            Pos2::new(rect.left() + 17.0, rect.center().y),
            vec2(14.0, 14.0),
        ),
        c.text_muted,
    );
    let id = Id::unique("search_box");
    let field = Rect::from_min_max(
        Pos2::new(rect.left() + 30.0, rect.top() + 4.0),
        Pos2::new(rect.right() - 92.0, rect.bottom() - 4.0),
    );
    super::widgets::region(ui, field, |ui| {
        let (response, changed) = bare_edit(
            ui,
            id,
            &tab.search_input,
            "Search commits (Ctrl+F)",
            field.width(),
            look.ui(13.0),
        );
        if cx.app.focus_search.take() {
            ui.memory_mut(|memory| memory.request_focus(id));
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                let end = tab.search_input.chars().count();
                state
                    .cursor
                    .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(end))));
                state.store(ui.ctx(), id);
            }
        }
        if let Some(text) = changed {
            cx.send(Message::SearchInput(text));
        }
        if response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
            cx.send(Message::SearchSubmit);
        }
    });
    let mode = Rect::from_min_max(
        Pos2::new(rect.right() - 90.0, rect.top() + 3.0),
        Pos2::new(rect.right() - 4.0, rect.bottom() - 3.0),
    );
    super::widgets::region(ui, mode, |ui| {
        let options: Vec<(String, String)> = SearchMode::ALL
            .iter()
            .map(|mode| (mode.key().to_owned(), mode.to_string()))
            .collect();
        if let Some(key) = choice(ui, "search_mode", &options, tab.search_mode.key(), 76.0)
            && let Some(mode) = SearchMode::ALL.into_iter().find(|mode| mode.key() == key)
        {
            cx.send(Message::SearchModeChanged(mode));
        }
    });
}

pub fn toolbar(ui: &mut Ui, cx: &Cx) {
    use crate::app::Message;
    let look = cx.look();
    let c = &look.c;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 51.0), Sense::hover());
    super::widgets::fill(ui, rect, c.toolbar);
    let top = rect.top() + 3.5;
    let Some(tab) = cx.app.current() else {
        if tool_button(
            ui,
            cx,
            Pos2::new(rect.left() + 12.0, top),
            "repo",
            "Open",
            0,
            true,
        ) {
            cx.send(Message::OpenDialog);
        }
        if tool_button(
            ui,
            cx,
            Pos2::new(rect.left() + 66.0, top),
            "pull",
            "Clone",
            0,
            true,
        ) {
            cx.send(Message::Action(Action::Clone));
        }
        hline(ui, look);
        return;
    };
    let idle = cx.app.busy.is_none();
    let branch = tab.status.as_ref().map(|status| &status.branch);
    let left = rect.left() + 12.0;
    let buttons: [(&str, &str, i64, Option<Message>); 6] = [
        (
            "fetch",
            "Fetch",
            0,
            Some(Message::Run(Command::Fetch {
                remote: None,
                tags: false,
            })),
        ),
        (
            "pull",
            "Pull",
            branch.map_or(0, |b| b.behind),
            Some(Message::Action(Action::Pull)),
        ),
        (
            "push",
            "Push",
            branch.map_or(0, |b| b.ahead),
            Some(Message::Action(Action::Push(None))),
        ),
        ("stash", "Stash", 0, Some(Message::Action(Action::Stash))),
        (
            "branch",
            "Branch",
            0,
            Some(Message::Action(Action::NewBranch {
                start: None,
                label: None,
            })),
        ),
        ("more", "More", 0, None),
    ];
    for (index, (name, label, count, message)) in buttons.into_iter().enumerate() {
        let group_gap = if index >= 3 { 14.0 } else { 0.0 };
        let at = Pos2::new(left + index as f32 * 54.0 + group_gap, top);
        let (message, enabled) = match message {
            Some(message) => (message, idle),
            None => (Message::OpenMenu(MenuRequest::Repository), true),
        };
        if tool_button(ui, cx, at, name, label, count, enabled) {
            cx.send(message);
        }
    }
    let search = Rect::from_min_size(
        Pos2::new(rect.right() - 12.0 - 340.0, rect.center().y - 15.0),
        vec2(340.0, 30.0),
    );
    search_box(ui, cx, tab, search);

    // The repository's name and branch, centred between the buttons and the search box.
    let centre = Rect::from_min_max(
        Pos2::new(left + 6.0 * 54.0 + 14.0 + 14.0, rect.top()),
        Pos2::new(search.left() - 14.0, rect.bottom()),
    );
    let clip = ui
        .painter()
        .with_clip_rect(centre.intersect(ui.clip_rect()));
    clip.text(
        Pos2::new(centre.center().x, rect.center().y - 8.0),
        Align2::CENTER_CENTER,
        &tab.name,
        look.ui(13.0),
        c.text,
    );
    let info = match branch {
        Some(b) if b.detached => format!("detached at {}", b.oid.as_deref().map_or("", short_sha)),
        Some(b) => {
            let name = b.head.clone().unwrap_or_else(|| "…".to_owned());
            match &b.upstream {
                Some(upstream) => format!("{name} → {upstream}"),
                None => name,
            }
        }
        None => "…".to_owned(),
    };
    clip.text(
        Pos2::new(centre.center().x, rect.center().y + 8.0),
        Align2::CENTER_CENTER,
        info,
        look.ui(12.0),
        c.text_muted,
    );
    hline(ui, look);
}

/// Shown while a merge, rebase, cherry-pick or revert is stopped half-way.
pub fn operation_banner(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    use crate::app::Message;
    let look = cx.look();
    let c = &look.c;
    let Some(status) = tab.status.as_ref() else {
        return;
    };
    let Some(operation) = status.operation.as_ref() else {
        return;
    };
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
    Frame::new()
        .fill(c.warning_bg)
        .inner_margin(Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
            ui.horizontal(|ui| {
                if conflicts > 0 {
                    icon::icon(ui, "alert", 16.0, c.danger);
                } else {
                    icon::icon(ui, "merge", 16.0, c.warning);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    text(ui, title, 13.0, c.text);
                    muted(ui, look, hint, 12.0);
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary(ui, look, "Continue", conflicts == 0) {
                        cx.send(Message::Run(Command::Continue));
                    }
                    if danger_text(ui, look, "Abort", true) {
                        cx.send(Message::Action(Action::Abort));
                    }
                    if !matches!(operation, Operation::Merge { .. }) && btn(ui, look, "Skip", true)
                    {
                        cx.send(Message::Run(Command::Skip));
                    }
                    if conflicts > 0 && btn(ui, look, "Show conflicts", true) {
                        cx.send(Message::SetView(View::Changes));
                    }
                });
            });
        });
    hline(ui, look);
}

pub fn status_bar(ui: &mut Ui, cx: &Cx) {
    use crate::app::Message;
    let app = cx.app;
    let look = cx.look();
    let c = &look.c;
    hline(ui, look);
    bar(ui, 23.0, c.titlebar, 12.0, |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let right = app.current().map(|tab| {
            let more = if tab.has_more { "+" } else { "" };
            format!("{}{more} commits", tab.commits.len())
        });
        let reserve = right
            .as_ref()
            .map_or(30.0, |label| text_width(ui, label, &look.ui(12.0)) + 40.0);
        let room = (ui.available_width() - reserve).max(40.0);
        ui.scope(|ui| {
            ui.set_max_width(room);
            if let Some((_, label)) = &app.busy {
                text(ui, format!("{label}…"), 12.0, c.text);
                if !app.progress_line.is_empty() {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&app.progress_line)
                                .size(12.0)
                                .monospace()
                                .color(c.text_muted),
                        )
                        .truncate(),
                    );
                }
                if app.cancel.is_some()
                    && super::widgets::button(ui, look, "Cancel", super::widgets::Kind::Ghost, true)
                {
                    cx.send(Message::Cancel);
                }
            } else {
                let path = app
                    .current()
                    .map_or("No repository open", |tab| tab.path.as_str());
                text(ui, path, 12.0, c.text_muted);
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, c.success);
            if let Some(label) = right {
                text(ui, label, 12.0, c.text_muted);
            }
        });
    });
}

pub fn welcome(ui: &mut Ui, cx: &Cx) {
    use crate::app::Message;
    let look = cx.look();
    let c = &look.c;
    let full = ui.available_rect_before_wrap();
    super::widgets::fill(ui, full, c.bg);
    let width = full.width().min(620.0);
    let page = Rect::from_min_size(
        Pos2::new(full.center().x - width / 2.0, full.top() + 40.0),
        vec2(width, full.height() - 40.0),
    );
    super::widgets::region(ui, page, |ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        ui.horizontal(|ui| {
            icon::icon(ui, "merge", 26.0, c.accent);
            text(ui, "GitCraft", 22.0, c.accent);
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if super::widgets::primary(ui, look, "Open a repository…", true) {
                cx.send(Message::OpenDialog);
            }
            if btn(ui, look, "Clone…", true) {
                cx.send(Message::Action(Action::Clone));
            }
            if btn(ui, look, "New…", true) {
                cx.send(Message::Action(Action::Init));
            }
        });
        if cx.app.recent.is_empty() {
            return;
        }
        ui.add_space(8.0);
        text(ui, "RECENT", 11.0, c.text_muted);
        scroll("recent").show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for repo in &cx.app.recent {
                let exists = repo.exists.unwrap_or(true);
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                if exists && response.hovered() {
                    ui.painter().rect_filled(rect, 6.0, c.hover);
                }
                let color = if exists { c.text } else { c.text_faint };
                icon::paint(
                    ui,
                    "repo",
                    egui::Rect::from_center_size(
                        Pos2::new(rect.left() + 18.0, rect.center().y),
                        vec2(16.0, 16.0),
                    ),
                    c.text_muted,
                );
                let painter = ui.painter().with_clip_rect(rect.shrink2(vec2(0.0, 0.0)));
                paint_text(
                    &painter,
                    Pos2::new(rect.left() + 36.0, rect.top() + 12.0),
                    &repo.name,
                    look.ui(13.0),
                    color,
                );
                paint_text(
                    &painter,
                    Pos2::new(rect.left() + 36.0, rect.top() + 28.0),
                    &repo.path,
                    look.ui(12.0),
                    c.text_muted,
                );
                let forget = Rect::from_center_size(
                    Pos2::new(rect.right() - 18.0, rect.center().y),
                    vec2(26.0, 26.0),
                );
                let forget_response =
                    ui.interact(forget, response.id.with("forget"), Sense::click());
                super::widgets::icon_in(ui, look, &forget_response, forget, "x", 13.0, true);
                if forget_response.clicked() {
                    cx.send(Message::Forget(repo.path.clone()));
                } else if exists && response.clicked() {
                    cx.send(Message::OpenPath(repo.path.clone()));
                }
            }
        });
    });
}
