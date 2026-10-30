//! Modal forms: a confirmation is a form without fields.

use std::collections::HashMap;

use iced::widget::text_editor;

use crate::app::Message;

pub enum FieldKind {
    Text(String),
    Area(text_editor::Content),
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
            label: label.to_owned(),
            kind: FieldKind::Text(value.to_owned()),
            placeholder: String::new(),
            required: false,
        }
    }
    pub fn area(key: &'static str, label: &str, value: &str) -> Self {
        Self {
            kind: FieldKind::Area(text_editor::Content::with_text(value)),
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
        self.placeholder = placeholder.to_owned();
        self
    }

    fn is_filled(&self) -> bool {
        match &self.kind {
            FieldKind::Text(value) => !value.trim().is_empty(),
            FieldKind::Area(content) => !content.text().trim().is_empty(),
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
}

impl Form {
    pub fn new(title: &str, ok: &str, on_ok: impl Fn(&Values) -> Message + 'static) -> Self {
        Self {
            title: title.to_owned(),
            message: None,
            fields: Vec::new(),
            ok: ok.to_owned(),
            danger: false,
            on_ok: Box::new(on_ok),
        }
    }
    /// A question that only needs an answer.
    pub fn confirm(title: &str, message: &str, ok: &str, danger: bool, then: Message) -> Self {
        let mut form = Self::new(title, ok, move |_| then.clone()).message(message);
        form.danger = danger;
        form
    }
    pub fn message(mut self, message: &str) -> Self {
        self.message = Some(message.to_owned());
        self
    }
    pub fn field(mut self, field: Field) -> Self {
        self.fields.push(field);
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
                FieldKind::Area(content) => {
                    values
                        .texts
                        .insert(field.key, content.text().trim_end_matches('\n').to_owned());
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
