//! Window furniture: menu bar, tabs, toolbar, operation banner, status bar and the start page.

use egui::text::{CCursor, CCursorRange};
use egui::{
    Align, Align2, Color32, Frame, Id, Key, Layout, Margin, Pos2, Rect, ScrollArea, Sense, Stroke,
    StrokeKind, Ui, vec2,
};
use gitcraft_core::model::Operation;

use super::widgets::{
    accent, bar, bar_with, bare_edit, btn, choice, danger, hline, icon_button, muted, paint_text,
    scroll, text, text_width,
};
use super::{Cx, icon};
use crate::actions::{Action, MenuRequest};
use crate::app::{RepoTab, SearchMode, View};
use crate::command::Command;
use crate::i18n::tr;
use crate::i18n::trf;
use crate::logic::{plural, short_sha};
use crate::menu::{Mark, MenuItem};

/// The title bar, when the app draws it (see `frame.rs`).
pub fn menu_bar(ui: &mut Ui, cx: &Cx) {
    super::frame::title_bar(ui, cx);
}

/// A menu bar entry as craft-menu takes it. The command id is the entry's place in
/// [`crate::app::App::bar_menus`], `bar.<menu>.<entry>[.<sub-entry>]`, so that the message can
/// be found again when it is chosen.
fn bar_node(item: &MenuItem, path: &str) -> Option<craft_menu::Node> {
    use craft_menu::{CommandRef, Item, Node};
    match item {
        MenuItem::Separator => Some(Node::Separator),
        MenuItem::Header(_) => None,
        MenuItem::Entry {
            label,
            message,
            shortcut,
            mark,
            ..
        } => {
            let mut entry = Item::new(CommandRef::new(path), tr(label)).enabled(message.is_some());
            if let Some(shortcut) = shortcut
                && let Some(key) = craft_keymap::Key::from_egui(shortcut.key)
            {
                let mods = craft_keymap::Mods {
                    cmd: shortcut.command,
                    ctrl: false,
                    alt: false,
                    shift: shortcut.shift,
                };
                entry = entry.shortcut(craft_menu::Chord { mods, key });
            }
            entry = match mark {
                Mark::None => entry,
                Mark::Check(on) => entry.check(*on),
                Mark::Radio(on) => entry.radio(*on),
            };
            if matches!(message, Some(crate::app::Message::Quit)) {
                entry = entry.role(craft_menu::ItemRole::Quit);
            }
            Some(entry.into())
        }
        MenuItem::Sub { label, children } => {
            let nodes = children
                .iter()
                .enumerate()
                .filter_map(|(index, child)| bar_node(child, &format!("{path}.{index}")))
                .collect();
            Some(Node::submenu(tr(label), nodes))
        }
    }
}

/// The message of the entry at `path` (see [`bar_node`]).
fn bar_message(menus: &[(&str, Vec<MenuItem>)], path: &str) -> Option<crate::app::Message> {
    let mut steps = path
        .strip_prefix("bar.")?
        .split('.')
        .map(str::parse::<usize>);
    let mut items = &menus.get(steps.next()?.ok()?)?.1;
    loop {
        match items.get(steps.next()?.ok()?)? {
            MenuItem::Entry { message, .. } => return message.clone(),
            MenuItem::Sub { children, .. } => items = children,
            _ => return None,
        }
    }
}

/// The menu bar (craft-menu): drawn like a desktop app's menus, with the shortcuts in their own
/// column. A press on a title opens its menu, moving onto another title opens that one, and the
/// arrow keys, Alt and F10 work as in other programs.
fn menus(ui: &mut Ui, cx: &Cx) {
    use craft_menu::{LayoutOptions, Menu, MenuBar, MenuBarUi, MenuRole, Target};
    let menus = cx.app.bar_menus();
    let roles = [
        Some(MenuRole::File),
        None,
        Some(MenuRole::View),
        Some(MenuRole::Help),
    ];
    let bar = MenuBar {
        menus: menus
            .iter()
            .enumerate()
            .map(|(index, (title, items))| {
                let nodes = items
                    .iter()
                    .enumerate()
                    .filter_map(|(entry, item)| bar_node(item, &format!("bar.{index}.{entry}")))
                    .collect();
                let menu = Menu::new(tr(title), nodes);
                match roles.get(index).copied().flatten() {
                    Some(role) => menu.role(role),
                    None => menu,
                }
            })
            .collect(),
        documents: Vec::new(),
    };
    // The menus stay in the window on every system, laid out as on Windows and Linux.
    let target = if cfg!(windows) {
        Target::Windows
    } else {
        Target::Linux
    };
    let label = |english: &'static str| tr(english).to_owned();
    let options = LayoutOptions {
        label: &label,
        ..LayoutOptions::new("GitCraft")
    };
    let bar = craft_menu::desktop_layout(&bar, target, &options).bar;
    let gate = craft_menu::Gate {
        modal: cx.app.form.is_some(),
        recording_shortcut: false,
        text_editing: ui.ctx().egui_wants_keyboard_input(),
    };
    // egui's built-in fonts have no ✓ or ●, craft-menu's default marks.
    let style = craft_menu::BarStyle {
        check_mark: "•".into(),
        radio_mark: "•".into(),
        ..craft_menu::BarStyle::from_style(ui.style())
    };
    let chosen = MenuBarUi::new(&bar)
        .style(style)
        .gate(gate)
        .label(&label)
        .shrink_to_fit(f32::INFINITY)
        .show(ui);
    for command in chosen {
        if let Some(message) = bar_message(&menus, &command.id) {
            cx.send(crate::app::Message::FromMenu(Box::new(message)));
        }
    }
}

fn tab_width(ui: &Ui, cx: &Cx, tab: &RepoTab) -> f32 {
    let look = cx.look();
    let mut width = 10.0 + text_width(ui, &tab.name, &look.ui(13.0)) + 6.0 + 18.0 + 4.0;
    if let Some(branch) = tab.current_branch() {
        width += 6.0 + text_width(ui, branch, &look.ui(11.0));
    }
    if let Some(note) = tab.worktree_note() {
        width += 6.0 + text_width(ui, &note, &look.ui(11.0));
    }
    width.min(260.0)
}

/// One open repository, as a part of the tab group: the one in front filled with the accent, as
/// the mode switches of the other Craft apps are.
/// Draws one tab; gives its place and response. The id comes from the repository, not the
/// tab's position, so that a drag stays with its tab while the tabs change places under it.
fn tab_view(ui: &mut Ui, cx: &Cx, index: usize, tab: &RepoTab) -> (Rect, egui::Response) {
    let look = cx.look();
    let c = &look.c;
    let active = index == cx.app.active;
    let width = tab_width(ui, cx, tab);
    let (rect, _) = ui.allocate_exact_size(vec2(width, TAB_HEIGHT), Sense::hover());
    let response = ui.interact(
        rect,
        Id::unique(("repo_tab", &tab.path)),
        Sense::click_and_drag(),
    );
    let hovered = ui.rect_contains_pointer(rect);
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    if active {
        let fill = if look.dark {
            c.accent.gamma_multiply(0.85)
        } else {
            c.accent
        };
        painter.rect_filled(rect, 4.0, fill);
    } else if hovered {
        painter.rect_filled(rect, 4.0, c.hover_strong);
    }
    let (color, dim) = if active {
        (Color32::WHITE, Color32::from_white_alpha(200))
    } else {
        (c.text, c.text_muted)
    };
    let mid = rect.center().y;
    let close = Rect::from_center_size(Pos2::new(rect.right() - 13.0, mid), vec2(18.0, 18.0));
    let text_area = painter.with_clip_rect(Rect::from_min_max(
        rect.min,
        Pos2::new(close.left() - 2.0, rect.max.y),
    ));
    let mut x = rect.left() + 10.0;
    x += paint_text(
        &text_area,
        Pos2::new(x, mid),
        &tab.name,
        look.ui(13.0),
        color,
    ) + 6.0;
    if let Some(branch) = tab.current_branch() {
        x += paint_text(&text_area, Pos2::new(x, mid), branch, look.ui(11.0), dim) + 6.0;
    }
    if let Some(note) = tab.worktree_note() {
        let tint = if active { dim } else { c.accent };
        paint_text(&text_area, Pos2::new(x, mid), &note, look.ui(11.0), tint);
    }
    // Tabs not in front only show their close button under the pointer.
    let mut closed = false;
    if active || hovered {
        let button = ui.interact(close, response.id.with("close"), Sense::click());
        if button.hovered() {
            let shade = if active {
                Color32::from_white_alpha(40)
            } else {
                c.hover
            };
            painter.rect_filled(close, 4.0, shade);
        }
        icon::paint(
            ui,
            "x",
            Rect::from_center_size(close.center(), vec2(11.0, 11.0)),
            color,
        );
        closed = button.on_hover_text(tr("Close")).clicked();
    }
    if closed || response.middle_clicked() {
        cx.send(crate::app::Message::Close(index));
    } else if response.clicked() {
        cx.send(crate::app::Message::Activate(index));
    } else if response.secondary_clicked() {
        cx.send(crate::app::Message::OpenMenu(MenuRequest::Tab(index)));
    }
    (rect, response)
}

const TAB_HEIGHT: f32 = 26.0;
/// Space around the tabs inside their group, and between two tabs.
const TAB_GAP: f32 = 2.0;

/// The open repositories on a shared ground exactly as tall as the tabs, scrolling sideways when
/// they don't fit. A tab dragged past the middle of its neighbour swaps places with it.
/// What the tab strip remembers between frames: how far it is scrolled, how far it can be, a
/// jump an arrow asked for, and the tab that was in front (a new one is scrolled into view).
#[derive(Clone, Default)]
struct Strip {
    offset: f32,
    max: f32,
    jump: Option<f32>,
    /// By path: tabs opening one after another can change which index is in front.
    active: Option<String>,
}

/// Short, firm shadows inside the strip's rounded ground where tabs go on past an edge. Drawn as
/// layers that narrow away from the edge, each with the ground's round corners on that side, so
/// the shade follows the corners instead of stopping short of them.
fn strip_shadows(ui: &Ui, look: &crate::theme::Look, ground: Rect, left: bool, right: bool) {
    const DEPTH: f32 = 12.0;
    const LAYERS: usize = 12;
    let strength = if look.dark { 120.0 } else { 60.0 };
    let color = egui::Color32::from_black_alpha((strength / LAYERS as f32) as u8);
    let round = |west: bool| {
        let (a, b) = if west { (5, 0) } else { (0, 5) };
        egui::CornerRadius {
            nw: a,
            sw: a,
            ne: b,
            se: b,
        }
    };
    for layer in 1..=LAYERS {
        let width = DEPTH * layer as f32 / LAYERS as f32;
        if left {
            let rect = Rect::from_min_max(
                ground.min,
                Pos2::new(ground.left() + width, ground.bottom()),
            );
            ui.painter().rect_filled(rect, round(true), color);
        }
        if right {
            let rect =
                Rect::from_min_max(Pos2::new(ground.right() - width, ground.top()), ground.max);
            ui.painter().rect_filled(rect, round(false), color);
        }
    }
}

/// An arrow beside the tab strip, on the same grey as the strip so that it is seen; lighter
/// under the pointer, faint when the strip can't move that way.
fn strip_arrow(ui: &mut Ui, look: &crate::theme::Look, name: &str, enabled: bool) -> bool {
    let c = &look.c;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(24.0, TAB_HEIGHT + 2.0 * TAB_GAP), sense);
    // Off, it is only a faint arrow on the bar.
    let fill = if !enabled {
        None
    } else if response.is_pointer_button_down_on() {
        Some(crate::theme::mix(c.hover_strong, c.text, 0.08))
    } else if response.hovered() {
        Some(c.hover_strong)
    } else {
        Some(c.panel_2)
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, 5.0, fill);
    }
    let color = if !enabled {
        c.text_faint.gamma_multiply(0.6)
    } else if response.hovered() {
        c.text
    } else {
        c.text_muted
    };
    icon::paint(
        ui,
        name,
        Rect::from_center_size(rect.center(), vec2(14.0, 14.0)),
        color,
    );
    enabled && response.clicked()
}

fn tab_group(ui: &mut Ui, cx: &Cx) {
    let look = cx.look();
    let c = &look.c;
    let tabs = &cx.app.tabs;
    let content: f32 = tabs.iter().map(|tab| tab_width(ui, cx, tab)).sum::<f32>()
        + TAB_GAP * (tabs.len().saturating_sub(1)) as f32;
    let room = (ui.available_width() - 80.0).max(60.0);
    // When the tabs don't fit, arrows on both sides scroll the strip.
    let overflow = content + 2.0 * TAB_GAP > room;
    let room = if overflow { room - 2.0 * 28.0 } else { room };
    let memory = egui::Id::unique("tab_strip");
    let mut strip_state: Strip = ui.data(|data| data.get_temp(memory).unwrap_or_default());
    let step = (room * 0.6).max(120.0);
    let jump = |ui: &Ui, state: &mut Strip, to: f32| {
        state.jump = Some(to.clamp(0.0, state.max));
        ui.ctx().request_repaint();
    };
    if overflow && strip_arrow(ui, look, "chevron-left", strip_state.offset > 0.5) {
        let to = strip_state.offset - step;
        jump(ui, &mut strip_state, to);
    }
    let size = vec2(
        (content + 2.0 * TAB_GAP).min(room),
        TAB_HEIGHT + 2.0 * TAB_GAP,
    );
    let (ground, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(ground, 5.0, c.panel_2);
    let inner = ground.shrink(TAB_GAP);
    let mut strip = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Min)),
    );
    let mut placed: Vec<(Rect, egui::Response)> = Vec::new();
    let mut area = ScrollArea::horizontal();
    if let Some(to) = strip_state.jump.take() {
        area = area.horizontal_scroll_offset(to);
    }
    let front = tabs.get(cx.app.active).map(|tab| tab.path.clone());
    let reveal = strip_state.active != front;
    let scrolled = area
        .id_salt("tabs")
        // A drag on the tabs moves a tab; the strip scrolls with the wheel.
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .max_width(inner.width())
        .max_height(TAB_HEIGHT)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(&mut strip, |ui| {
            ui.spacing_mut().item_spacing = vec2(TAB_GAP, 0.0);
            // Exactly as tall as a tab and aligned to the top, so the ground's border is the
            // same all round (`horizontal` would centre the tabs in a row of its own height).
            let row = Layout::left_to_right(Align::Min);
            ui.allocate_ui_with_layout(vec2(content, TAB_HEIGHT), row, |ui| {
                for (index, tab) in tabs.iter().enumerate() {
                    let (rect, response) = tab_view(ui, cx, index, tab);
                    if reveal && index == cx.app.active {
                        ui.scroll_to_rect_animation(
                            rect,
                            None,
                            egui::style::ScrollAnimation::none(),
                        );
                    }
                    placed.push((rect, response));
                }
            });
        });
    strip_state.offset = scrolled.state.clamped_offset().x;
    strip_state.max = (scrolled.content_size.x - scrolled.inner_rect.width()).max(0.0);
    // Shaded where tabs go on past the edge.
    strip_shadows(
        ui,
        look,
        ground,
        strip_state.offset > 0.5,
        strip_state.offset < strip_state.max - 0.5,
    );
    // Until the tab in front is wholly in view (the strip only knows its width after a frame),
    // it is asked for again next time; then the strip is the user's to scroll.
    let shown = placed
        .get(cx.app.active)
        .is_some_and(|(rect, _)| inner.expand(0.5).contains_rect(*rect));
    if !reveal || shown {
        strip_state.active = front;
    } else {
        ui.ctx().request_repaint();
    }
    if overflow
        && strip_arrow(
            ui,
            look,
            "chevron-right",
            strip_state.offset < strip_state.max - 0.5,
        )
    {
        let to = strip_state.offset + step;
        jump(ui, &mut strip_state, to);
    }
    ui.data_mut(|data| data.insert_temp(memory, strip_state));
    let Some(dragged) = placed.iter().position(|(_, response)| response.dragged()) else {
        return;
    };
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    let Some(x) = ui.ctx().pointer_interact_pos().map(|pos| pos.x) else {
        return;
    };
    let past = |other: usize| placed.get(other).map(|(rect, _)| rect.center().x);
    if past(dragged + 1).is_some_and(|middle| x > middle) {
        cx.send(crate::app::Message::MoveTab(dragged, 1));
    } else if dragged > 0 && past(dragged - 1).is_some_and(|middle| x < middle) {
        cx.send(crate::app::Message::MoveTab(dragged, -1));
    }
}

/// The app's row: its name, the menus and the open repositories, as in the other Craft apps.
pub fn tab_bar(ui: &mut Ui, cx: &Cx) {
    use crate::app::Message;
    let look = cx.look();
    let c = &look.c;
    bar(ui, 40.0, c.toolbar, 0.0, |ui| {
        ui.add_space(12.0);
        icon::icon(ui, "merge", 17.0, c.accent);
        ui.add_space(2.0);
        text(ui, "GitCraft", 14.0, c.text);
        ui.add_space(10.0);
        menus(ui, cx);
        ui.add_space(10.0);
        if !cx.app.tabs.is_empty() {
            tab_group(ui, cx);
        }
        let plus = ui.scope(|ui| icon_button(ui, look, "plus", 15.0, true));
        if plus.inner {
            cx.send(Message::OpenDialog);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(8.0);
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
        tr(label),
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
    // The search kind's drop-down is as wide as its longest name in the interface language.
    let mode_width = SearchMode::ALL
        .iter()
        .map(|mode| text_width(ui, &mode.to_string(), &look.ui(13.0)))
        .fold(0.0, f32::max)
        + 34.0;
    let mode_width = mode_width.max(86.0);
    let field = Rect::from_min_max(
        Pos2::new(rect.left() + 30.0, rect.top() + 6.5),
        Pos2::new(rect.right() - mode_width - 6.0, rect.bottom() - 1.5),
    );
    super::widgets::region(ui, field, |ui| {
        // Centred, so that the hint text sits in the middle of the box, not at its top.
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
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
                    state.cursor.set_char_range(Some(CCursorRange::two(
                        CCursor::new(0),
                        CCursor::new(end),
                    )));
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
    });
    let mode = Rect::from_min_max(
        Pos2::new(rect.right() - mode_width - 4.0, rect.top() + 3.0),
        Pos2::new(rect.right() - 4.0, rect.bottom() - 3.0),
    );
    super::widgets::region(ui, mode, |ui| {
        // Against the right end, whichever kind is shown: a shorter name leaves room on the left,
        // where the field goes on.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let options: Vec<(String, String)> = SearchMode::ALL
                .iter()
                .map(|mode| (mode.key().to_owned(), mode.to_string()))
                .collect();
            if let Some(key) = choice(
                ui,
                "search_mode",
                &options,
                tab.search_mode.key(),
                mode_width - 14.0,
            ) && let Some(mode) = SearchMode::ALL.into_iter().find(|mode| mode.key() == key)
            {
                cx.send(Message::SearchModeChanged(mode));
            }
        });
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
        Some(b) if b.detached => trf(
            "detached at {commit}",
            &[("commit", b.oid.as_deref().map_or("", short_sha))],
        ),
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
        Operation::Merge { .. } => tr("Merge in progress").to_owned(),
        Operation::Rebase { .. } => tr("Rebase in progress").to_owned(),
        Operation::CherryPick { .. } => tr("Cherry-pick in progress").to_owned(),
        Operation::Revert { .. } => tr("Revert in progress").to_owned(),
        Operation::Am => tr("Applying patches (git am)").to_owned(),
    };
    if let Operation::Rebase {
        step: Some(step),
        total: Some(total),
        ..
    } = operation
    {
        title.push_str(" · ");
        title.push_str(&trf(
            "step {step} of {total}",
            &[("step", &step.to_string()), ("total", &total.to_string())],
        ));
    }
    if let Operation::Rebase {
        branch: Some(branch),
        ..
    } = operation
    {
        title.push_str(&format!(" · {branch}"));
    }
    let hint = if conflicts > 0 {
        trf(
            "{files} conflicted. Resolve them in Local Changes, then continue.",
            &[("files", &plural(conflicts, "file"))],
        )
    } else if let Operation::Rebase {
        stopped_sha: Some(sha),
        ..
    } = operation
    {
        trf(
            "Stopped at {commit}. Amend or make changes, then continue.",
            &[("commit", short_sha(sha))],
        )
    } else {
        tr("All conflicts resolved. Continue to finish.").to_owned()
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
                    if accent(ui, look, "Continue", conflicts == 0) {
                        cx.send(Message::Run(Command::Continue));
                    }
                    if danger(ui, look, "Abort", true) {
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
    let corner = if app.rounded() {
        super::frame::RADIUS
    } else {
        0
    };
    let radius = egui::CornerRadius {
        sw: corner,
        se: corner,
        ..egui::CornerRadius::ZERO
    };
    bar_with(ui, 23.0, c.titlebar, radius, 12.0, |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let right = app.current().map(|tab| {
            let more = if tab.has_more { "+" } else { "" };
            trf(
                "{count} commits",
                &[("count", &format!("{}{more}", tab.commits.len()))],
            )
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
                    .map_or(tr("No repository open"), |tab| tab.path.as_str());
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
            if super::widgets::accent(ui, look, "Open a repository…", true) {
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
        text(ui, tr("Recent").to_uppercase(), 11.0, c.text_muted);
        let scrolled = scroll("recent").show(ui, |ui| {
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
        super::widgets::scroll_shadows(ui, cx.look(), &scrolled);
    });
}
