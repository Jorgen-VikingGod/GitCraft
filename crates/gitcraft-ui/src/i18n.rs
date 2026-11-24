//! Interface languages, through craft-i18n. Text in the code is English; [`tr`] looks it up in the
//! catalogs when it is drawn and falls back to the English when a line has no translation yet. The
//! app's state (and its tests) stay in English.
//!
//! The catalogs come from `src/i18n/translations.tsv` (English, German, French, Spanish per
//! line); `build.rs` turns it into one craft-i18n catalog per language.

use std::sync::{LazyLock, RwLock};

pub use craft_i18n::Lang;

include!(concat!(env!("OUT_DIR"), "/craft_i18n.rs"));

/// The languages the interface is ready for, besides English.
const OFFERED: [&str; 3] = ["de", "fr", "es"];

/// The setting that means "the system's language".
pub const AUTO: &str = "auto";

pub static I18N: LazyLock<craft_i18n::I18n> = LazyLock::new(|| {
    let (i18n, problems) = craft_i18n::I18n::new(CRAFT_I18N, &OFFERED);
    for problem in problems {
        eprintln!("translation catalog: {problem}");
    }
    i18n
});

static CURRENT: RwLock<Lang> = RwLock::new(Lang::EN);

/// Show the interface in the language of this setting: a code such as `de`, or [`AUTO`] (also
/// anything unknown) for the system's language.
pub fn set_language(setting: &str) {
    let lang = I18N
        .from_code(setting)
        .unwrap_or_else(|| I18N.system_lang());
    *CURRENT.write().unwrap_or_else(|error| error.into_inner()) = lang;
}

pub fn lang() -> Lang {
    *CURRENT.read().unwrap_or_else(|error| error.into_inner())
}

/// The languages to offer, English first: (code, the language's own name).
pub fn languages() -> Vec<(&'static str, &'static str)> {
    I18N.languages()
        .into_iter()
        .map(|lang| (I18N.code(lang), I18N.name(lang)))
        .collect()
}

/// The text in the chosen language, or as given when there's no translation.
pub fn tr(text: &str) -> &str {
    I18N.t(lang(), text)
}

/// `template` in the chosen language, with each `{name}` replaced by its value.
pub fn trf(template: &str, args: &[(&str, &str)]) -> String {
    fill(tr(template), args)
}

/// Replaces each `{name}` in one pass over `template`: a value is put in as it is, so a file
/// called `{commit}.txt` stays that, and a `{…}` without a value is kept.
fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let value = after.find('}').and_then(|close| {
            let name = &after[..close];
            let (_, value) = args.iter().find(|(key, _)| *key == name)?;
            Some((*value, close))
        });
        match value {
            Some((value, close)) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn values_are_put_in_as_they_are() {
        let args = [("path", "{commit}.txt"), ("commit", "abc1234")];
        assert_eq!(
            fill("{path} restored from {commit}", &args),
            "{commit}.txt restored from abc1234"
        );
        assert_eq!(fill("{a}{a} {missing} {", &[("a", "x")]), "xx {missing} {");
    }

    const TABLE: &str = include_str!("i18n/translations.tsv");

    /// English -> [German, French, Spanish].
    fn table() -> HashMap<&'static str, [&'static str; 3]> {
        TABLE
            .lines()
            .filter_map(|line| {
                let mut c = line.split('\t');
                Some((c.next()?, [c.next()?, c.next()?, c.next()?]))
            })
            .collect()
    }

    /// `{name}` parts that must survive translation untouched.
    fn placeholders(s: &str) -> Vec<&str> {
        let mut v: Vec<&str> = s
            .split('{')
            .skip(1)
            .filter_map(|p| p.split('}').next())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn every_line_is_complete_and_keeps_its_placeholders() {
        let mut seen = std::collections::HashSet::new();
        for (n, line) in TABLE.lines().enumerate() {
            let c: Vec<&str> = line.split('\t').collect();
            assert_eq!(
                c.len(),
                4,
                "line {}: needs English, German, French and Spanish: {line}",
                n + 1
            );
            assert!(
                seen.insert(c[0]),
                "line {}: {} is listed twice",
                n + 1,
                c[0]
            );
            for (i, t) in c.iter().enumerate() {
                assert!(!t.trim().is_empty(), "line {}: column {i} is empty", n + 1);
                assert_eq!(*t, t.trim(), "line {}: stray space around {t:?}", n + 1);
            }
            for t in &c[1..] {
                assert_eq!(
                    placeholders(c[0]),
                    placeholders(t),
                    "line {}: placeholders differ in {t}",
                    n + 1
                );
            }
        }
    }

    #[test]
    fn lookup_translates_and_falls_back() {
        // Looks up by language, without changing the one in use: tests run side by side.
        let t = |code: &str, text: &str| {
            let lang = I18N.from_code(code).unwrap();
            I18N.t(lang, text).to_owned()
        };
        assert_eq!(t("de", "Settings…"), "Einstellungen…");
        assert_eq!(t("fr", "Settings…"), "Paramètres…");
        assert_eq!(t("es", "Settings…"), "Ajustes…");
        assert_eq!(t("en", "Settings…"), "Settings…");
        assert_eq!(
            t("de", "Something nobody translated"),
            "Something nobody translated"
        );
        assert_eq!(
            t("de", "Delete local branch {name}?"),
            "Lokalen Branch {name} löschen?"
        );
        assert_eq!(
            tr("Settings…"),
            "Settings…",
            "English until a language is chosen"
        );
    }

    #[test]
    fn the_catalogs_have_no_problems_and_name_their_languages() {
        assert!(I18N.check_all().is_empty(), "{:?}", I18N.check_all());
        let names: Vec<_> = languages().into_iter().map(|(_, n)| n).collect();
        assert_eq!(names, ["English", "Deutsch", "Español", "Français"]);
    }

    #[test]
    fn every_translatable_string_in_the_ui_is_in_the_table() {
        // Every literal passed to `tr(...)` or `trf(...)` must have a line, so nothing quietly
        // stays English in a translated window.
        let table = table();
        let mut missing = Vec::new();
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut files = vec![std::path::PathBuf::from(dir)];
        while let Some(path) = files.pop() {
            if path.is_dir() {
                files.extend(std::fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") || path.ends_with("i18n.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            for key in literal_keys(&src) {
                if !table.contains_key(key.as_str()) && !EXEMPT.contains(&key.as_str()) {
                    missing.push(format!("{}: {key}", path.display()));
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "no translation for:\n{}",
            missing.join("\n")
        );
    }

    /// Names and strings that stay as they are in every language.
    const EXEMPT: &[&str] = &["GitCraft"];

    /// The string literals passed first to `tr(` or `trf(` in Rust source, also when rustfmt has
    /// put the literal on a line of its own, with `\"` and `\\` read as Rust reads them.
    fn literal_keys(src: &str) -> Vec<String> {
        let mut keys = Vec::new();
        for opener in ["tr(", "trf("] {
            let mut rest = src;
            while let Some(i) = rest.find(opener) {
                // `push_str(` and `ptr(` end in `tr(` too: only a call of `tr` itself counts.
                let named = rest[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
                rest = &rest[i + opener.len()..];
                let Some(literal) = rest.trim_start().strip_prefix('"').filter(|_| !named) else {
                    continue;
                };
                let mut key = String::new();
                let mut chars = literal.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => key.push('\n'),
                            Some(other) => key.push(other),
                            None => break,
                        },
                        c => key.push(c),
                    }
                }
                keys.push(key);
            }
        }
        keys
    }

    #[test]
    fn the_scan_finds_calls_split_over_lines() {
        let src =
            "trf(\n    \"Copied {what}\",\n    &[]);\nx.push_str(\"no\"); tr(\"say \\\"hi\\\"\")";
        assert_eq!(literal_keys(src), ["say \"hi\"", "Copied {what}"]);
    }
}
