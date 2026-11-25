//! Context menus and the menu bar: plain data, so that what a menu offers can be tested without
//! drawing it.

use egui::Pos2;

use crate::app::Message;
use crate::i18n::tr;

/// Labels are given in English and stored in the interface language (see `i18n.rs`); names of
/// branches, files and the like come back unchanged.
fn shown(label: impl Into<String>) -> String {
    let label = label.into();
    tr(&label).to_owned()
}

/// A tick or a dot in front of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mark {
    #[default]
    None,
    /// An option that is on or off.
    Check(bool),
    /// One of a group of choices; `true` is the chosen one.
    Radio(bool),
}

// Menus are short-lived and small, so entries simply hold their message.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum MenuItem {
    Header(String),
    Separator,
    /// `None` is a disabled entry.
    Entry {
        label: String,
        message: Option<Message>,
        danger: bool,
        /// The key that does the same, shown beside the label (the app handles the key itself).
        shortcut: Option<craft_keymap::Chord>,
        mark: Mark,
    },
    Sub {
        label: String,
        children: Vec<MenuItem>,
    },
}

impl MenuItem {
    pub fn entry(label: impl Into<String>, message: Message) -> Self {
        Self::Entry {
            label: shown(label),
            message: Some(message),
            danger: false,
            shortcut: None,
            mark: Mark::None,
        }
    }
    pub fn entry_if(label: impl Into<String>, enabled: bool, message: Message) -> Self {
        Self::Entry {
            label: shown(label),
            message: enabled.then_some(message),
            danger: false,
            shortcut: None,
            mark: Mark::None,
        }
    }
    pub fn danger(label: impl Into<String>, message: Message) -> Self {
        Self::Entry {
            label: shown(label),
            message: Some(message),
            danger: true,
            shortcut: None,
            mark: Mark::None,
        }
    }
    pub fn danger_if(label: impl Into<String>, enabled: bool, message: Message) -> Self {
        Self::Entry {
            label: shown(label),
            message: enabled.then_some(message),
            danger: true,
            shortcut: None,
            mark: Mark::None,
        }
    }
    pub fn sub(label: impl Into<String>, children: Vec<MenuItem>) -> Self {
        Self::Sub {
            label: shown(label),
            children,
        }
    }
    /// Shows `key` beside the entry, when it has one (a command whose keys were cleared
    /// has none).
    pub fn key(mut self, key: Option<craft_keymap::Chord>) -> Self {
        if let Self::Entry { shortcut, .. } = &mut self {
            *shortcut = key;
        }
        self
    }

    /// Puts a tick or a dot in front of the entry.
    pub fn marked(mut self, with: Mark) -> Self {
        if let Self::Entry { mark, .. } = &mut self {
            *mark = with;
        }
        self
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Header(label) | Self::Entry { label, .. } | Self::Sub { label, .. } => {
                Some(label)
            }
            Self::Separator => None,
        }
    }
}

pub struct Menu {
    pub at: Pos2,
    pub items: Vec<MenuItem>,
    pub open_sub: Option<usize>,
}

impl Menu {
    pub fn new(at: Pos2, items: Vec<MenuItem>) -> Self {
        Self {
            at,
            items,
            open_sub: None,
        }
    }
}
