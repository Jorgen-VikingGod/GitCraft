//! Context menus: plain data, so that what a menu offers can be tested without drawing it.

use iced::Point;

use crate::app::Message;

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
    },
    Sub {
        label: String,
        children: Vec<MenuItem>,
    },
}

impl MenuItem {
    pub fn entry(label: impl Into<String>, message: Message) -> Self {
        Self::Entry {
            label: label.into(),
            message: Some(message),
            danger: false,
        }
    }
    pub fn entry_if(label: impl Into<String>, enabled: bool, message: Message) -> Self {
        Self::Entry {
            label: label.into(),
            message: enabled.then_some(message),
            danger: false,
        }
    }
    pub fn danger(label: impl Into<String>, message: Message) -> Self {
        Self::Entry {
            label: label.into(),
            message: Some(message),
            danger: true,
        }
    }
    pub fn danger_if(label: impl Into<String>, enabled: bool, message: Message) -> Self {
        Self::Entry {
            label: label.into(),
            message: enabled.then_some(message),
            danger: true,
        }
    }
    pub fn sub(label: impl Into<String>, children: Vec<MenuItem>) -> Self {
        Self::Sub {
            label: label.into(),
            children,
        }
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
    pub at: Point,
    pub items: Vec<MenuItem>,
    pub open_sub: Option<usize>,
}

impl Menu {
    pub fn new(at: Point, items: Vec<MenuItem>) -> Self {
        Self {
            at,
            items,
            open_sub: None,
        }
    }
}
