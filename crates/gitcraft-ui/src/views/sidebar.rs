//! Navigation, plus branches, remotes, tags, stashes and submodules as collapsible trees.

use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Ui, vec2};

use super::widgets::{
    fill, hover_fill, icon_in, line_edit, paint_strong, paint_text, scroll, split_y, text_width,
};
use super::{Cx, icon};
use crate::actions::{Action, MenuRequest, RefTarget};
use crate::app::{Message, RepoTab, View};
use crate::command::Command;
use crate::i18n::tr;
use crate::logic::{build_tree, short_sha};

const INDENT: f32 = 14.0;
const BASE_INDENT: f32 = 8.0;
const ROW: f32 = 24.0;

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
    let body = rect.shrink2(vec2(6.0, super::widgets::ROW_INSET));
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
        tr(label),
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
        &tr(title).to_uppercase(),
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

/// A section of the sidebar as a card on the sidebar's ground, as the panels of the other Craft
/// apps are.
fn card(ui: &mut Ui, cx: &Cx, add: impl FnOnce(&mut Ui)) {
    let c = cx.c();
    ui.add_space(8.0);
    egui::Frame::new()
        .fill(c.panel)
        .stroke(egui::Stroke::new(1.0, c.card_line))
        .corner_radius(6)
        .inner_margin(egui::Margin {
            left: 0,
            right: 0,
            top: 0,
            bottom: 4,
        })
        .outer_margin(egui::Margin::symmetric(8, 0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn none(ui: &mut Ui, cx: &Cx, note: &str) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.add_space(26.0);
        ui.label(
            egui::RichText::new(tr(note))
                .size(12.0)
                .italics()
                .color(cx.c().text_faint),
        );
    });
    ui.add_space(4.0);
}

/// Takes the room of a row that is out of view and says so; such rows aren't drawn, so that
/// thousands of branches or tags cost no more than those on screen.
fn skipped(ui: &mut Ui) -> bool {
    let rect = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), ROW));
    if ui.is_rect_visible(rect) {
        return false;
    }
    ui.allocate_space(rect.size());
    true
}

/// How a row of the sidebar tree is marked. Only the branch checked out is in the accent colour;
/// whatever is picked or pointed at is grey.
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    None,
    CheckedOut,
    Picked,
    /// Another name for the selected commit: marked, but not as the one picked.
    Related,
}

/// A row of the sidebar tree: the rectangle, with its mark or the hover drawn.
fn tree_row(ui: &mut Ui, cx: &Cx, mark: Mark) -> (Rect, Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    let body = rect.shrink2(vec2(4.0, super::widgets::ROW_INSET));
    let c = cx.c();
    let fill = match mark {
        Mark::CheckedOut => Some(c.selection),
        Mark::Picked => Some(c.hover_strong),
        Mark::Related => Some(crate::theme::mix(c.panel, c.hover_strong, 0.5)),
        Mark::None => None,
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(body, 5.0, fill);
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
    let (body, response) = tree_row(ui, cx, Mark::None);
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
    /// Points at the selected commit too, but isn't the one that was picked.
    related: bool,
    current: bool,
    /// The branch checked out: the one row in the accent colour.
    checked_out: bool,
    /// Small notes at the right end: text and colour.
    badges: Vec<(String, Color32)>,
    /// A branch's full ref name: it gets the graph's hide and solo toggles.
    graph_ref: Option<String>,
}

/// The hide and solo toggles at the right end of a branch's row, right to left from `right`.
/// They show while the row is hovered, and stay in the accent colour while on. Returns where
/// the rest of the row ends.
fn graph_toggles(ui: &mut Ui, cx: &Cx, body: Rect, full: &str, right: f32) -> f32 {
    use crate::app::RefVisibility;
    let look = cx.look();
    let c = &look.c;
    let view = &cx.app.current().map(|tab| &tab.ref_view);
    let hidden = view.is_some_and(|view| view.hidden.iter().any(|name| name == full));
    let solo = view.is_some_and(|view| view.solo.iter().any(|name| name == full));
    let hovered = ui.rect_contains_pointer(body);
    if !hovered && !hidden && !solo {
        return right;
    }
    let toggles = [
        (
            "filter",
            solo,
            if solo {
                "Stop soloing"
            } else {
                "Solo in graph"
            },
            if solo {
                RefVisibility::Unsolo(full.to_owned())
            } else {
                RefVisibility::Solo(full.to_owned())
            },
        ),
        (
            if hidden { "eye-off" } else { "eye" },
            hidden,
            if hidden {
                "Show in graph"
            } else {
                "Hide in graph"
            },
            if hidden {
                RefVisibility::Show(full.to_owned())
            } else {
                RefVisibility::Hide(full.to_owned())
            },
        ),
    ];
    // The branch checked out can't be hidden: git shows its commits anyway.
    let current = cx.app.current().and_then(|tab| tab.current_branch());
    let hideable = crate::logic::can_hide(full, current);
    let mut right = right;
    for (symbol, on, tip, change) in toggles
        .into_iter()
        .filter(|(_, _, _, change)| hideable || !matches!(change, RefVisibility::Hide(_)))
    {
        let rect =
            Rect::from_center_size(Pos2::new(right - 10.0, body.center().y), vec2(20.0, 20.0));
        right -= 22.0;
        // Only the hovered row shows the toggles that are off.
        if !on && !hovered {
            continue;
        }
        let response = ui
            .interact(
                rect,
                egui::Id::unique(("graph-toggle", full, symbol)),
                Sense::click(),
            )
            .on_hover_text(tr(tip));
        if response.hovered() {
            hover_fill(ui, look, rect, 4.0);
        }
        icon::paint(
            ui,
            symbol,
            Rect::from_center_size(rect.center(), vec2(13.0, 13.0)),
            if on { c.accent } else { c.text_muted },
        );
        if response.clicked() {
            cx.send(Message::RefVisibility(change));
        }
    }
    right - 4.0
}

fn leaf(ui: &mut Ui, cx: &Cx, item: &Item) -> Response {
    let look = cx.look();
    let c = &look.c;
    let mark = if item.checked_out {
        Mark::CheckedOut
    } else if item.selected {
        Mark::Picked
    } else if item.related {
        Mark::Related
    } else {
        Mark::None
    };
    let (body, response) = tree_row(ui, cx, mark);
    let text_color = if item.checked_out {
        c.selection_text
    } else {
        c.text
    };
    let symbol_color = if item.checked_out {
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
    if let Some(full) = &item.graph_ref {
        right = graph_toggles(ui, cx, body, full, right + 6.0);
    }
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
    let matches = |name: &str| crate::logic::contains_folded(name, &filter);
    let in_history = tab.view == View::History;
    // For a ref: is it the one that was picked, and does it point at the selected commit too?
    let marks = |key: &str, sha: &str| {
        let picked = in_history && tab.selected_ref.as_deref() == Some(key);
        let same = in_history && tab.selected.as_deref() == Some(sha);
        (picked, same && !picked)
    };
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
        let scrolled = scroll(("sidebar", &tab.path)).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            card(ui, cx, |ui| {
                branches(ui, cx, tab, &matches, &marks, &collapsed)
            });
            card(ui, cx, |ui| {
                remotes(ui, cx, tab, &matches, &marks, &collapsed)
            });
            tags_and_more(ui, cx, tab, &matches, &marks, &collapsed);
            ui.add_space(16.0);
        });
        super::widgets::scroll_shadows(ui, cx.look(), &scrolled);
    });
}

/// The folder a working tree lives in, which is what tells several of them apart.
fn tree_name(tree: &gitcraft_core::model::Worktree) -> String {
    std::path::Path::new(&tree.path)
        .file_name()
        .map_or_else(|| tree.path.clone(), |n| n.to_string_lossy().into_owned())
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
    marks: &dyn Fn(&str, &str) -> (bool, bool),
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
        if skipped(ui) {
            continue;
        }
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
            selected: marks(&format!("b:{}", branch.name), &branch.sha).0,
            related: marks(&format!("b:{}", branch.name), &branch.sha).1,
            current: branch.current,
            checked_out: branch.current,
            badges,
            graph_ref: Some(format!("refs/heads/{}", branch.name)),
        };
        let response = leaf(ui, cx, &item);
        if response.double_clicked() {
            cx.send(checkout(&branch.name, "local"));
        } else if response.clicked() {
            cx.send(Message::ShowRef(
                format!("b:{}", branch.name),
                branch.sha.clone(),
            ));
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
    marks: &dyn Fn(&str, &str) -> (bool, bool),
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
            if skipped(ui) {
                continue;
            }
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
                selected: marks(&format!("r:{}", branch.name), &branch.sha).0,
                related: marks(&format!("r:{}", branch.name), &branch.sha).1,
                current: false,
                checked_out: false,
                badges: Vec::new(),
                graph_ref: Some(format!("refs/remotes/{}", branch.name)),
            };
            let response = leaf(ui, cx, &item);
            if response.double_clicked() {
                cx.send(checkout(&branch.name, "remote"));
            } else if response.clicked() {
                cx.send(Message::ShowRef(
                    format!("r:{}", branch.name),
                    branch.sha.clone(),
                ));
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
    marks: &dyn Fn(&str, &str) -> (bool, bool),
    collapsed: &dyn Fn(&str) -> bool,
) {
    let c = cx.c();
    let simple = |label: String, symbol: &'static str, (selected, related), badges| Item {
        label,
        depth: 0,
        symbol,
        selected,
        related,
        current: false,
        checked_out: false,
        badges,
        graph_ref: None,
    };

    // Tags
    let tags: Vec<_> = tab
        .refs
        .tags
        .iter()
        .filter(|tag| matches(&tag.name))
        .collect();
    card(ui, cx, |ui| {
        section(ui, cx, "s:tags", "Tags", Some(tags.len()), None);
        if !collapsed("s:tags") {
            if tags.is_empty() {
                none(ui, cx, "No tags");
            }
            for tag in tags {
                if skipped(ui) {
                    continue;
                }
                let item = simple(
                    tag.name.clone(),
                    "tag",
                    marks(&format!("t:{}", tag.name), &tag.sha),
                    Vec::new(),
                );
                let response = leaf(ui, cx, &item);
                if response.clicked() {
                    cx.send(Message::ShowRef(format!("t:{}", tag.name), tag.sha.clone()));
                } else if response.secondary_clicked() {
                    cx.send(Message::OpenMenu(MenuRequest::Commit {
                        sha: tag.sha.clone(),
                        focus: Some(RefTarget::Tag(tag.name.clone())),
                    }));
                }
            }
        }
    });

    // Stashes
    card(ui, cx, |ui| {
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
                if skipped(ui) {
                    continue;
                }
                let selected = tab.view == View::Stash
                    && tab
                        .stash
                        .as_ref()
                        .is_some_and(|open| open.name == stash.name);
                let item = simple(
                    stash.message.clone(),
                    "stash",
                    (selected, false),
                    Vec::new(),
                );
                let response = leaf(ui, cx, &item);
                if response.clicked() {
                    cx.send(Message::OpenStash(stash.name.clone()));
                } else if response.secondary_clicked() {
                    cx.send(Message::OpenMenu(MenuRequest::Stash(stash.name.clone())));
                }
            }
        }
    });

    // Worktrees
    let trees: Vec<_> = tab
        .refs
        .worktrees
        .iter()
        .filter(|tree| !tree.bare)
        .filter(|tree| matches(&tree_name(tree)) || tree.branch.as_deref().is_some_and(matches))
        .collect();
    card(ui, cx, |ui| {
        section(
            ui,
            cx,
            "s:worktrees",
            "Worktrees",
            Some(trees.len()),
            Some(Message::Action(Action::AddWorktree(None))),
        );
        if !collapsed("s:worktrees") {
            for tree in trees {
                let mut badges = Vec::new();
                match (&tree.branch, tree.detached) {
                    (Some(branch), _) => badges.push((branch.clone(), c.text_muted)),
                    (None, true) => {
                        badges.push((format!("@{}", short_sha(&tree.sha)), c.text_muted))
                    }
                    _ => {}
                }
                if tree.locked {
                    badges.push((tr("locked").to_owned(), c.text_muted));
                }
                if tree.prunable {
                    badges.push((tr("missing").to_owned(), c.text_muted));
                }
                let mut item = simple(tree_name(tree), "worktree", (false, false), badges);
                item.current = tree.current;
                let response = leaf(ui, cx, &item).on_hover_text(&tree.path);
                if response.double_clicked() && !tree.current {
                    cx.send(Message::OpenPath(tree.path.clone()));
                } else if response.secondary_clicked() {
                    cx.send(Message::OpenMenu(MenuRequest::Worktree(tree.path.clone())));
                }
            }
        }
    });

    // Git LFS, once the repository uses it or is set up for it.
    if let Some(lfs) = tab
        .refs
        .lfs
        .as_ref()
        .filter(|lfs| lfs.installed || !lfs.patterns.is_empty())
    {
        card(ui, cx, |ui| {
            section(
                ui,
                cx,
                "s:lfs",
                "Git LFS",
                Some(lfs.patterns.len()),
                Some(Message::Action(Action::LfsTrack(String::new()))),
            );
            if collapsed("s:lfs") {
                return;
            }
            if lfs.patterns.is_empty() {
                none(ui, cx, "No files are stored in LFS");
            }
            for pattern in &lfs.patterns {
                let badges = if pattern.lockable {
                    vec![(tr("lockable").to_owned(), c.text_muted)]
                } else {
                    Vec::new()
                };
                let item = simple(pattern.pattern.clone(), "file", (false, false), badges);
                let response = leaf(ui, cx, &item).on_hover_text(&pattern.source);
                if response.secondary_clicked() {
                    cx.send(Message::OpenMenu(MenuRequest::LfsPattern(
                        pattern.pattern.clone(),
                    )));
                }
            }
            if !tab.refs.remotes.is_empty() {
                let item = simple(tr("Locks…").to_owned(), "lock", (false, false), Vec::new());
                if leaf(ui, cx, &item).clicked() {
                    cx.send(Message::OpenLfsLocks);
                }
            }
        });
    }

    // Submodules
    if tab.refs.submodules.is_empty() {
        return;
    }
    card(ui, cx, |ui| {
        section(
            ui,
            cx,
            "s:submodules",
            "Submodules",
            Some(tab.refs.submodules.len()),
            Some(Message::Action(crate::actions::Action::AddSubmodule)),
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
            let item = simple(module.path.clone(), "box", (false, false), badges);
            let response = leaf(ui, cx, &item);
            if response.double_clicked() {
                cx.send(Message::OpenPath(crate::actions::join_path(
                    &tab.path,
                    &module.path,
                )));
            } else if response.secondary_clicked() {
                cx.send(Message::OpenMenu(MenuRequest::Submodule(
                    module.path.clone(),
                )));
            }
        }
    });
}
