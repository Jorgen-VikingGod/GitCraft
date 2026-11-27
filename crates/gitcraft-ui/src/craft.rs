//! GitCraft on the Craft framework (craft-libs): the craft-ui runtime is installed on the window's
//! egui context, and before each frame it is told what it needs from the app: the interface scale,
//! light or dark, the accent, and the commands of the menu bar with their state.
//!
//! The menu bar (craft-menu) is a model of commands. Each entry of [`App::bar_menus`] is a command
//! whose id is its place in the menus, `bar.<menu>.<entry>[.<sub-entry>]`; choosing it fires that
//! command, and [`Runtime::fired`] turns it back into the entry's message. GitCraft's own keys
//! (`logic::SHORTCUTS`, Settings › Keyboard shortcuts) stay GitCraft's: the menu commands have no
//! bindings, and their rows show the entry's key as text.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use craft_host::target::{Backend, ShellInfo};
use craft_menu::{CommandRef, Item, LayoutOptions, Menu, MenuBar, MenuRole, Node, Target};
use craft_ui::contract::Tracked;
use craft_ui::runtime::commands::{
    AppCatalog, CommandDef, CommandInput, CommandState, CommandStates, FireSource, Scope,
};
use craft_ui::runtime::env::{RootEnv, Scale, SystemReader};
use craft_ui::runtime::frame::{FrameInput, FrameOutput};
use craft_ui::runtime::{InstallError, InstallSpec};
use craft_window::policy::Decision;
use craft_window::system::SystemWindows;

use craft_theme::{ColorToken, Pins, Rgba, ThemeDef, ThemeId};
use craft_ui::list::CommandHighlight;

use crate::app::{App, Message};
use crate::i18n::tr;
use crate::menu::{Mark, MenuItem};
use crate::theme::Look;

/// The prefix of the menu bar's command ids.
const BAR: &str = "bar.";

/// Installs the runtime on the window's context, with the decided frame. The window's size and
/// place stay GitCraft's (`window.rs`).
pub fn install(ctx: &egui::Context, decision: Decision) {
    let wake = ctx.clone();
    let spec = InstallSpec::new(
        ShellInfo::default().backend(Backend::from_env()),
        Arc::new(SystemWindows),
    )
    .app_id("gitcraft")
    .frame(decision)
    .system(SystemReader::start(move || wake.request_repaint()));
    // The interface face of the Craft apps, for every script the languages need.
    let fonts = craft_ui::fonts::UiFonts::new(&craft_ui::fonts::I18nScripts);
    match craft_ui::shell::embed::install(ctx, spec, &fonts) {
        Ok(_) | Err(InstallError::AlreadyInstalled) => {}
        Err(error) => eprintln!("The window's interface library couldn't start: {error}"),
    }
}

/// What the runtime is given each frame, kept so that it is rebuilt only when it changes.
#[derive(Default)]
pub struct Runtime {
    /// The menu bar as craft-menu lays it out, which the view draws (`App::bar`).
    bar: Option<MenuBar>,
    states: Tracked<CommandStates>,
    catalog: Option<(u64, AppCatalog)>,
    /// The theme the runtime draws with, for light (`false`) or dark (`true`).
    theme: Option<(bool, Option<Arc<ThemeDef>>)>,
}

impl Runtime {
    /// The input for the coming frame, after the app's menus and look were brought up to date.
    pub fn frame_input(&mut self, app: &mut App) -> FrameInput {
        let menus = app.bar_menus();
        let bar = laid_out(&menus);
        if self.bar.as_ref() != Some(&bar) {
            app.bar.edit(|shown| *shown = bar.clone());
            self.bar = Some(bar);
        }
        let mut states = CommandStates::new();
        let mut defs = Vec::new();
        for (index, (_, items)) in menus.iter().enumerate() {
            commands(items, &format!("{BAR}{index}"), &mut states, &mut defs);
        }
        if *self.states != states {
            self.states.edit(|held| *held = states);
        }
        // The labels change with the language, the ids with what the menus hold.
        let mut hasher = DefaultHasher::new();
        for def in &defs {
            (&def.command.id, &def.label).hash(&mut hasher);
        }
        let key = hasher.finish();
        if self.catalog.as_ref().is_none_or(|(held, _)| *held != key) {
            let generation = self
                .catalog
                .as_ref()
                .map_or(1, |(_, held)| held.generation() + 1);
            match AppCatalog::new(defs, generation) {
                Ok(catalog) => self.catalog = Some((key, catalog)),
                Err(error) => eprintln!("The menu's commands couldn't be listed: {error}"),
            }
        }
        let mut commands = CommandInput::new().states(&self.states);
        if let Some((_, catalog)) = &self.catalog {
            commands = commands.catalog(catalog.clone());
        }
        let look = &app.look;
        if self
            .theme
            .as_ref()
            .is_none_or(|(dark, _)| *dark != look.dark)
        {
            self.theme = Some((look.dark, theme(look).map(Arc::new)));
        }
        let mut root = RootEnv::new()
            .scale(Scale::new(app.settings.appearance.zoom))
            .mode(if look.dark {
                craft_theme::ModeChoice::Dark
            } else {
                craft_theme::ModeChoice::Light
            })
            .accent(Some(rgba(look.c.accent)));
        if let Some((_, Some(def))) = &self.theme {
            root = root.library_theme(def.clone());
        }
        // The row under the pointer shows the hover fill, as in GitCraft's context menus.
        root.values.set(CommandHighlight::Hover);
        FrameInput::new(root).commands(commands)
    }

    /// The messages of the menu entries chosen since the last frame.
    pub fn fired(app: &App, output: &FrameOutput) -> Vec<Message> {
        let chosen: Vec<&str> = output
            .fired
            .iter()
            .filter(|fired| fired.source != FireSource::Key)
            .filter_map(|fired| fired.command.id.strip_prefix(BAR))
            .collect();
        if chosen.is_empty() {
            return Vec::new();
        }
        let menus = app.bar_menus();
        chosen
            .into_iter()
            .filter_map(|path| bar_message(&menus, path))
            .map(|message| Message::FromMenu(Box::new(message)))
            .collect()
    }
}

fn rgba(color: egui::Color32) -> Rgba {
    Rgba(color.to_srgba_unmultiplied())
}

/// The runtime's theme (it draws the menu bar's menus and the system dialogs): the Craft apps'
/// default with GitCraft's own colours pinned, so its menus look like GitCraft's context menus.
fn theme(look: &Look) -> Option<ThemeDef> {
    let builtins = craft_theme::builtins();
    let mut def = builtins
        .iter()
        .find(|def| def.id.as_str() == craft_theme::SUITE_DEFAULT)
        .or_else(|| builtins.first())?
        .clone();
    if let Ok(id) = ThemeId::parse("gitcraft") {
        def.id = id;
    }
    def.name = "GitCraft".to_owned();
    let c = &look.c;
    let fills = look.fills();
    let pins: Pins = [
        (ColorToken::SurfacePanel, c.panel),
        (ColorToken::SurfaceShadow, c.shadow),
        (ColorToken::LineBorder, c.border),
        (ColorToken::LineDivider, c.border),
        (ColorToken::TextNormal, c.text),
        (ColorToken::TextMuted, c.text_muted),
        (ColorToken::TextFaint, c.text_faint),
        (ColorToken::StateHover, fills.hover),
        (ColorToken::StatePressed, fills.press),
    ]
    .into_iter()
    .map(|(token, color)| (token, rgba(color)))
    .collect();
    if look.dark {
        def.pins.dark = pins;
    } else {
        def.pins.light = pins;
    }
    Some(def)
}

/// The menus as craft-menu takes them, laid out as on Windows and Linux: the menus stay in the
/// window on every system.
fn laid_out(menus: &[(&'static str, Vec<MenuItem>)]) -> MenuBar {
    // One for each of File, Edit, Repository, View and Help, in that order: About goes last in
    // the menu with the Help role.
    let roles = [
        Some(MenuRole::File),
        None,
        None,
        Some(MenuRole::View),
        Some(MenuRole::Help),
    ];
    let bar = MenuBar::new(
        menus
            .iter()
            .enumerate()
            .map(|(index, (title, items))| {
                let nodes = items
                    .iter()
                    .enumerate()
                    .filter_map(|(entry, item)| node(item, &format!("{BAR}{index}.{entry}")))
                    .collect();
                let menu = Menu::new(tr(title), nodes).english(*title);
                match roles.get(index).copied().flatten() {
                    Some(role) => menu.role(role),
                    None => menu,
                }
            })
            .collect(),
    );
    let target = if cfg!(windows) {
        Target::Windows
    } else {
        Target::Linux
    };
    craft_menu::desktop_layout(&bar, target, &LayoutOptions::new("GitCraft")).bar
}

/// One entry of the menus as a craft-menu node.
fn node(item: &MenuItem, path: &str) -> Option<Node> {
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
            let mut entry = Item::new(CommandRef::new(path), tr(label))
                .english(label.as_str())
                .enabled(message.is_some());
            if let Some(chord) = shortcut {
                entry = entry.shortcut(*chord);
            }
            entry = match mark {
                // Only the chosen one of a group shows its dot; the others show no empty ring.
                Mark::None | Mark::Radio(false) => entry,
                Mark::Check(on) => entry.check(*on),
                Mark::Radio(true) => entry.radio(true),
            };
            if matches!(message, Some(Message::Quit)) {
                entry = entry.role(craft_menu::ItemRole::Quit);
            }
            if matches!(message, Some(Message::OpenAbout)) {
                entry = entry.role(craft_menu::ItemRole::About);
            }
            Some(entry.into())
        }
        MenuItem::Sub { label, children } => {
            let nodes = children
                .iter()
                .enumerate()
                .filter_map(|(index, child)| node(child, &format!("{path}.{index}")))
                .collect();
            Some(Node::submenu(tr(label), nodes).english(label.as_str()))
        }
    }
}

/// The command and state of every entry under `path`.
fn commands(
    items: &[MenuItem],
    path: &str,
    states: &mut CommandStates,
    defs: &mut Vec<CommandDef>,
) {
    for (index, item) in items.iter().enumerate() {
        let path = format!("{path}.{index}");
        match item {
            MenuItem::Entry {
                label,
                message,
                mark,
                ..
            } => {
                let mut def = CommandDef::new(CommandRef::new(&path), tr(label), Scope::app());
                let mut state = CommandState::new().enabled(message.is_some());
                match mark {
                    Mark::None => {}
                    Mark::Check(on) | Mark::Radio(on) => {
                        def = def.toggle();
                        state = state.checked(*on);
                    }
                }
                states.set(CommandRef::new(&path), state);
                defs.push(def);
            }
            MenuItem::Sub { children, .. } => commands(children, &path, states, defs),
            MenuItem::Separator | MenuItem::Header(_) => {}
        }
    }
}

/// The message of the entry at `path`, `<menu>.<entry>[.<sub-entry>]`.
fn bar_message(menus: &[(&str, Vec<MenuItem>)], path: &str) -> Option<Message> {
    let mut steps = path.split('.').map(str::parse::<usize>);
    let mut items = &menus.get(steps.next()?.ok()?)?.1;
    loop {
        match items.get(steps.next()?.ok()?)? {
            MenuItem::Entry { message, .. } => return message.clone(),
            MenuItem::Sub { children, .. } => items = children,
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_menu_path_finds_its_entry_again() {
        let menus = vec![(
            "File",
            vec![
                MenuItem::entry("Open", Message::OpenDialog),
                MenuItem::Separator,
                MenuItem::sub("More", vec![MenuItem::entry("Quit", Message::Quit)]),
            ],
        )];
        assert!(matches!(
            bar_message(&menus, "0.0"),
            Some(Message::OpenDialog)
        ));
        assert!(matches!(bar_message(&menus, "0.2.0"), Some(Message::Quit)));
        assert!(bar_message(&menus, "0.1").is_none());
        assert!(bar_message(&menus, "1.0").is_none());
    }
}
