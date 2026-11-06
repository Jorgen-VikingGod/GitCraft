//! Navigation, plus branches, remotes, tags, stashes and submodules as collapsible trees.

use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Ui, vec2};

use super::widgets::{
    fill, hover_fill, icon_in, line_edit, paint_strong, paint_text, scroll, split_y, text_width,
};
use super::{Cx, icon};
use crate::actions::{Action, MenuRequest, RefTarget};
use crate::app::{Message, RepoTab, View};
use crate::command::Command;
use crate::logic::build_tree;

const INDENT: f32 = 14.0;
const BASE_INDENT: f32 = 8.0;

fn nav(
    ui: &mut Ui,
    cx: &Cx,
    name: &str,
    label: &str,
    active: bool,
    count: usize,
    message: Message,
) {
    let look = cx.look();
    let c = &look.c;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    let body = rect.shrink2(vec2(6.0, 0.0));
    if active {
        ui.painter().rect_filled(body, 6.0, c.selection);
    } else if response.hovered() {
        ui.painter().rect_filled(body, 6.0, c.hover);
    }
    let color = if active { c.selection_text } else { c.text };
    icon::paint(
        ui,
        name,
        Rect::from_center_size(
            Pos2::new(body.left() + 17.0, rect.center().y),
            vec2(14.0, 14.0),
        ),
        color,
    );
    paint_text(
        ui.painter(),
        Pos2::new(body.left() + 32.0, rect.center().y),
        label,
        look.ui(13.0),
        color,
    );
    if count > 0 {
        let font = look.ui(11.0);
        let width = (text_width(ui, &count.to_string(), &font) + 12.0).max(18.0);
        let pill = Rect::from_center_size(
            Pos2::new(body.right() - 10.0 - width / 2.0, rect.center().y),
            vec2(width, 16.0),
        );
        ui.painter().rect_filled(pill, 8.0, c.accent);
        ui.painter().text(
            pill.center(),
            Align2::CENTER_CENTER,
            count,
            font,
            Color32::WHITE,
        );
    }
    if response.clicked() {
        cx.send(message);
    }
}

fn section(
    ui: &mut Ui,
    cx: &Cx,
    key: &str,
    title: &str,
    count: Option<usize>,
    add: Option<Message>,
) {
    let look = cx.look();
    let c = &look.c;
    ui.add_space(6.0);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    let name = if cx.app.collapsed.contains(key) {
        "chevron-right"
    } else {
        "chevron-down"
    };
    icon::paint(
        ui,
        name,
        Rect::from_center_size(
            Pos2::new(rect.left() + 12.0, rect.center().y),
            vec2(12.0, 12.0),
        ),
        c.text_muted,
    );
    paint_text(
        ui.painter(),
        Pos2::new(rect.left() + 22.0, rect.center().y),
        &title.to_uppercase(),
        look.ui(11.0),
        c.text_muted,
    );
    if let Some(count) = count {
        ui.painter().text(
            Pos2::new(rect.right() - 8.0, rect.center().y),
            Align2::RIGHT_CENTER,
            count,
            look.ui(11.0),
            c.text_faint,
        );
    }
    let mut pressed_add = false;
    if let (Some(_), true) = (&add, ui.rect_contains_pointer(rect)) {
        let button = Rect::from_center_size(
            Pos2::new(rect.right() - 17.0, rect.center().y),
            vec2(26.0, 26.0),
        );
        let plus = ui.interact(button, response.id.with("add"), Sense::click());
        icon_in(ui, look, &plus, button, "plus", 13.0, true);
        pressed_add = plus.clicked();
    }
    if pressed_add {
        cx.send(add.expect("checked above"));
    } else if response.clicked() {
        cx.send(Message::ToggleCollapsed(key.to_owned()));
    }
}

fn none(ui: &mut Ui, cx: &Cx, note: &str) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.add_space(26.0);
        ui.label(
            egui::RichText::new(note)
                .size(12.0)
                .italics()
                .color(cx.c().text_faint),
        );
    });
    ui.add_space(4.0);
}

/// A row of the sidebar tree: the rectangle, with a hover or selection fill drawn.
fn tree_row(ui: &mut Ui, cx: &Cx, selected: bool) -> (Rect, Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let body = rect.shrink2(vec2(4.0, 0.0));
    if selected {
        ui.painter().rect_filled(body, 5.0, cx.c().selection);
    } else if response.hovered() {
        hover_fill(ui, cx.look(), body, 5.0);
    }
    (body, response)
}

fn folder(
    ui: &mut Ui,
    cx: &Cx,
    key: &str,
    label: &str,
    depth: usize,
    symbol: &str,
    menu: Option<Message>,
) {
    let look = cx.look();
    let c = &look.c;
    let (body, response) = tree_row(ui, cx, false);
    let chevron = if cx.app.collapsed.contains(key) {
        "chevron-right"
    } else {
        "chevron-down"
    };
    let x = body.left() + BASE_INDENT - 4.0 + depth as f32 * INDENT;
    let mid = body.center().y;
    icon::paint(
        ui,
        chevron,
        Rect::from_center_size(Pos2::new(x + 10.0, mid), vec2(12.0, 12.0)),
        c.text_muted,
    );
    icon::paint(
        ui,
        symbol,
        Rect::from_center_size(Pos2::new(x + 29.0, mid), vec2(13.0, 13.0)),
        c.text_muted,
    );
    let clip = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
    paint_text(
        &clip,
        Pos2::new(x + 42.0, mid),
        label,
        look.ui(13.0),
        c.text_muted,
    );
    if response.clicked() {
        cx.send(Message::ToggleCollapsed(key.to_owned()));
    } else if response.secondary_clicked()
        && let Some(menu) = menu
    {
        cx.send(menu);
    }
}

struct Item {
    label: String,
    depth: usize,
    symbol: &'static str,
    selected: bool,
    current: bool,
    /// Small notes at the right end: text and colour.
    badges: Vec<(String, Color32)>,
}

fn leaf(ui: &mut Ui, cx: &Cx, item: &Item) -> Response {
    let look = cx.look();
    let c = &look.c;
    let (body, response) = tree_row(ui, cx, item.selected);
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
    let x = body.left() + BASE_INDENT - 4.0 + 18.0 + item.depth as f32 * INDENT;
    let mid = body.center().y;
    icon::paint(
        ui,
        item.symbol,
        Rect::from_center_size(Pos2::new(x + 7.0, mid), vec2(13.0, 13.0)),
        symbol_color,
    );
    let mut right = body.right() - 10.0;
    for (note, color) in &item.badges {
        let width = text_width(ui, note, &look.mono(11.0));
        ui.painter().text(
            Pos2::new(right, mid),
            Align2::RIGHT_CENTER,
            note,
            look.mono(11.0),
            *color,
        );
        right -= width + 6.0;
    }
    let clip = ui.painter().with_clip_rect(
        Rect::from_min_max(body.min, Pos2::new(right, body.max.y)).intersect(ui.clip_rect()),
    );
    if item.current {
        paint_strong(
            &clip,
            Pos2::new(x + 20.0, mid),
            &item.label,
            look.ui(13.0),
            text_color,
        );
    } else {
        paint_text(
            &clip,
            Pos2::new(x + 20.0, mid),
            &item.label,
            look.ui(13.0),
            text_color,
        );
    }
    response
}

pub fn view(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let look = cx.look();
    let c = &look.c;
    let area = ui.max_rect();
    fill(ui, area, c.sidebar);
    let filter = tab.filter.to_lowercase();
    let matches = |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter);
    let in_history = tab.view == View::History;
    let is_selected = |sha: &str| in_history && tab.selected.as_deref() == Some(sha);
    let collapsed = |key: &str| cx.app.collapsed.contains(key);

    let (head, list_area) = split_y(area, 8.0 + 29.0 + 29.0 + 4.0 + 30.0 + 6.0);
    super::widgets::region(ui, head, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
        ui.add_space(8.0);
        nav(
            ui,
            cx,
            "edit",
            "Local Changes",
            tab.view == View::Changes,
            tab.change_count(),
            Message::SetView(View::Changes),
        );
        nav(
            ui,
            cx,
            "history",
            "All Commits",
            in_history,
            0,
            Message::SetView(View::History),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let (_, changed) = line_edit(
                ui,
                "branch_filter",
                &tab.filter,
                "Filter branches & tags",
                ui.available_width() - 8.0,
                look.ui(12.0),
            );
            if let Some(text) = changed {
                cx.send(Message::FilterInput(text));
            }
        });
    });

    super::widgets::region(ui, list_area, |ui| {
        scroll(("sidebar", &tab.path)).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            branches(ui, cx, tab, &matches, &is_selected, &collapsed);
            remotes(ui, cx, tab, &matches, &is_selected, &collapsed);
            tags_and_more(ui, cx, tab, &matches, &is_selected, &collapsed);
            ui.add_space(16.0);
        });
    });
}

fn checkout(name: &str, kind: &'static str) -> Message {
    Message::Run(Command::Checkout {
        reference: name.to_owned(),
        kind,
    })
}

fn branches(
    ui: &mut Ui,
    cx: &Cx,
    tab: &RepoTab,
    matches: &dyn Fn(&str) -> bool,
    is_selected: &dyn Fn(&str) -> bool,
    collapsed: &dyn Fn(&str) -> bool,
) {
    let c = cx.c();
    let new_branch = Message::Action(Action::NewBranch {
        start: None,
        label: None,
    });
    section(ui, cx, "s:branches", "Branches", None, Some(new_branch));
    if collapsed("s:branches") {
        return;
    }
    let branches: Vec<_> = tab
        .refs
        .local
        .iter()
        .filter(|branch| matches(&branch.name))
        .collect();
    if branches.is_empty() {
        none(ui, cx, "No branches");
    }
    let names: Vec<&str> = branches.iter().map(|branch| branch.name.as_str()).collect();
    for entry in build_tree("b", &names, collapsed) {
        let Some(index) = entry.item else {
            folder(
                ui,
                cx,
                &entry.key,
                &entry.label,
                entry.depth,
                "folder",
                None,
            );
            continue;
        };
        let branch = branches[index];
        let mut badges = Vec::new();
        if branch.upstream_gone {
            badges.push(("gone".to_owned(), c.warning));
        }
        if branch.ahead > 0 {
            badges.push((format!("↑{}", branch.ahead), c.text_muted));
        }
        if branch.behind > 0 {
            badges.push((format!("↓{}", branch.behind), c.text_muted));
        }
        let item = Item {
            label: entry.label,
            depth: entry.depth,
            symbol: if branch.current { "check" } else { "branch" },
            selected: is_selected(&branch.sha),
            current: branch.current,
            badges,
        };
        let response = leaf(ui, cx, &item);
        if response.double_clicked() {
            cx.send(checkout(&branch.name, "local"));
        } else if response.clicked() {
            cx.send(Message::ShowCommit(branch.sha.clone()));
        } else if response.secondary_clicked() {
            cx.send(Message::OpenMenu(MenuRequest::Commit {
                sha: branch.sha.clone(),
                focus: Some(RefTarget::Local(branch.name.clone())),
            }));
        }
    }
}

fn remotes(
    ui: &mut Ui,
    cx: &Cx,
    tab: &RepoTab,
    matches: &dyn Fn(&str) -> bool,
    is_selected: &dyn Fn(&str) -> bool,
    collapsed: &dyn Fn(&str) -> bool,
) {
    section(
        ui,
        cx,
        "s:remotes",
        "Remotes",
        None,
        Some(Message::Action(Action::AddRemote)),
    );
    if collapsed("s:remotes") {
        return;
    }
    if tab.refs.remotes.is_empty() {
        none(ui, cx, "No remotes");
    }
    for remote in &tab.refs.remotes {
        let key = format!("r:{}", remote.name);
        folder(
            ui,
            cx,
            &key,
            &remote.name,
            0,
            "cloud",
            Some(Message::OpenMenu(MenuRequest::Remote(remote.name.clone()))),
        );
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
                folder(
                    ui,
                    cx,
                    &entry.key,
                    &entry.label,
                    entry.depth + 1,
                    "folder",
                    None,
                );
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
            let response = leaf(ui, cx, &item);
            if response.double_clicked() {
                cx.send(checkout(&branch.name, "remote"));
            } else if response.clicked() {
                cx.send(Message::ShowCommit(branch.sha.clone()));
            } else if response.secondary_clicked() {
                cx.send(Message::OpenMenu(MenuRequest::Commit {
                    sha: branch.sha.clone(),
                    focus: Some(RefTarget::Remote(branch.name.clone())),
                }));
            }
        }
    }
}

fn tags_and_more(
    ui: &mut Ui,
    cx: &Cx,
    tab: &RepoTab,
    matches: &dyn Fn(&str) -> bool,
    is_selected: &dyn Fn(&str) -> bool,
    collapsed: &dyn Fn(&str) -> bool,
) {
    let c = cx.c();
    let simple = |label: String, symbol: &'static str, selected: bool, badges| Item {
        label,
        depth: 0,
        symbol,
        selected,
        current: false,
        badges,
    };

    // Tags
    let tags: Vec<_> = tab
        .refs
        .tags
        .iter()
        .filter(|tag| matches(&tag.name))
        .collect();
    section(ui, cx, "s:tags", "Tags", Some(tags.len()), None);
    if !collapsed("s:tags") {
        if tags.is_empty() {
            none(ui, cx, "No tags");
        }
        for tag in tags {
            let item = simple(tag.name.clone(), "tag", is_selected(&tag.sha), Vec::new());
            let response = leaf(ui, cx, &item);
            if response.clicked() {
                cx.send(Message::ShowCommit(tag.sha.clone()));
            } else if response.secondary_clicked() {
                cx.send(Message::OpenMenu(MenuRequest::Commit {
                    sha: tag.sha.clone(),
                    focus: Some(RefTarget::Tag(tag.name.clone())),
                }));
            }
        }
    }

    // Stashes
    section(
        ui,
        cx,
        "s:stashes",
        "Stashes",
        Some(tab.refs.stashes.len()),
        None,
    );
    if !collapsed("s:stashes") {
        if tab.refs.stashes.is_empty() {
            none(ui, cx, "No stashes");
        }
        for stash in &tab.refs.stashes {
            let selected = tab.view == View::Stash
                && tab
                    .stash
                    .as_ref()
                    .is_some_and(|open| open.name == stash.name);
            let item = simple(stash.message.clone(), "stash", selected, Vec::new());
            let response = leaf(ui, cx, &item);
            if response.clicked() {
                cx.send(Message::OpenStash(stash.name.clone()));
            } else if response.secondary_clicked() {
                cx.send(Message::OpenMenu(MenuRequest::Stash(stash.name.clone())));
            }
        }
    }

    // Submodules
    if tab.refs.submodules.is_empty() {
        return;
    }
    section(
        ui,
        cx,
        "s:submodules",
        "Submodules",
        Some(tab.refs.submodules.len()),
        None,
    );
    if collapsed("s:submodules") {
        return;
    }
    for module in &tab.refs.submodules {
        let badges = if module.state != "ok" {
            vec![(module.state.clone(), c.text_muted)]
        } else {
            Vec::new()
        };
        let item = simple(module.path.clone(), "box", false, badges);
        let response = leaf(ui, cx, &item);
        if response.double_clicked() {
            cx.send(Message::OpenPath(crate::actions::join_path(
                &tab.path,
                &module.path,
            )));
        } else if response.secondary_clicked() {
            cx.send(Message::OpenMenu(MenuRequest::File {
                area: None,
                path: module.path.clone(),
                rev: None,
            }));
        }
    }
}
