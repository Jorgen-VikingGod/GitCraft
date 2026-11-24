//! Modal forms: a confirmation is a form without fields.
//!
//! Titles, labels, buttons and hints are given in English and stored in the interface language
//! (see `i18n.rs`); values are never translated.

use std::collections::HashMap;

use crate::app::Message;
use crate::i18n::tr;

pub enum FieldKind {
    Text(String),
    Area(String),
    Check(bool),
    Select {
        options: Vec<(String, String)>,
        value: String,
    },
}

pub struct Field {
    pub key: &'static str,
    pub label: String,
    pub kind: FieldKind,
    pub placeholder: String,
    pub required: bool,
}

impl Field {
    pub fn text(key: &'static str, label: &str, value: &str) -> Self {
        Self {
            key,
            label: tr(label).to_owned(),
            kind: FieldKind::Text(value.to_owned()),
            placeholder: String::new(),
            required: false,
        }
    }
    pub fn area(key: &'static str, label: &str, value: &str) -> Self {
        Self {
            kind: FieldKind::Area(value.to_owned()),
            ..Self::text(key, label, "")
        }
    }
    pub fn check(key: &'static str, label: &str, value: bool) -> Self {
        Self {
            kind: FieldKind::Check(value),
            ..Self::text(key, label, "")
        }
    }
    pub fn select(
        key: &'static str,
        label: &str,
        value: &str,
        options: Vec<(String, String)>,
    ) -> Self {
        let options = options
            .into_iter()
            .map(|(value, label)| {
                let label = tr(&label).to_owned();
                (value, label)
            })
            .collect();
        Self {
            kind: FieldKind::Select {
                options,
                value: value.to_owned(),
            },
            ..Self::text(key, label, "")
        }
    }
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
    pub fn hint(mut self, placeholder: &str) -> Self {
        self.placeholder = tr(placeholder).to_owned();
        self
    }

    fn is_filled(&self) -> bool {
        match &self.kind {
            FieldKind::Text(value) => !value.trim().is_empty(),
            FieldKind::Area(value) => !value.trim().is_empty(),
            FieldKind::Check(_) | FieldKind::Select { .. } => true,
        }
    }
}

/// The values a form was submitted with.
#[derive(Debug, Default, Clone)]
pub struct Values {
    texts: HashMap<&'static str, String>,
    checks: HashMap<&'static str, bool>,
}

impl Values {
    pub fn text(&self, key: &str) -> String {
        self.texts.get(key).cloned().unwrap_or_default()
    }
    /// The text, or nothing when it's empty.
    pub fn opt(&self, key: &str) -> Option<String> {
        self.texts
            .get(key)
            .filter(|value| !value.is_empty())
            .cloned()
    }
    pub fn check(&self, key: &str) -> bool {
        self.checks.get(key).copied().unwrap_or(false)
    }
}

pub struct Form {
    pub title: String,
    pub message: Option<String>,
    pub fields: Vec<Field>,
    pub ok: String,
    pub danger: bool,
    pub on_ok: Box<dyn Fn(&Values) -> Message>,
    /// Other answers than OK, shown between Cancel and OK.
    pub choices: Vec<Choice>,
}

/// An answer of its own in a form, such as "Commit first" beside OK.
pub struct Choice {
    pub label: String,
    pub danger: bool,
    pub message: Message,
}

impl Form {
    pub fn new(title: &str, ok: &str, on_ok: impl Fn(&Values) -> Message + 'static) -> Self {
        Self {
            title: tr(title).to_owned(),
            message: None,
            fields: Vec::new(),
            ok: tr(ok).to_owned(),
            danger: false,
            on_ok: Box::new(on_ok),
            choices: Vec::new(),
        }
    }
    /// A question that only needs an answer.
    pub fn confirm(title: &str, message: &str, ok: &str, danger: bool, then: Message) -> Self {
        let mut form = Self::new(title, ok, move |_| then.clone()).message(message);
        form.danger = danger;
        form
    }
    pub fn message(mut self, message: &str) -> Self {
        self.message = Some(tr(message).to_owned());
        self
    }
    pub fn field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }
    /// Adds an answer of its own; `danger` shows it in red.
    pub fn choice(mut self, label: &str, danger: bool, message: Message) -> Self {
        self.choices.push(Choice {
            label: tr(label).to_owned(),
            danger,
            message,
        });
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub fn can_submit(&self) -> bool {
        self.fields
            .iter()
            .all(|field| !field.required || field.is_filled())
    }
    pub fn values(&self) -> Values {
        let mut values = Values::default();
        for field in &self.fields {
            match &field.kind {
                FieldKind::Text(value) => {
                    values.texts.insert(field.key, value.clone());
                }
                FieldKind::Area(value) => {
                    values
                        .texts
                        .insert(field.key, value.trim_end_matches('\n').to_owned());
                }
                FieldKind::Check(value) => {
                    values.checks.insert(field.key, *value);
                }
                FieldKind::Select { value, .. } => {
                    values.texts.insert(field.key, value.clone());
                }
            }
        }
        values
    }
}
