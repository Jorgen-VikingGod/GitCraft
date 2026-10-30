//! Syntax highlighting of diffs, run away from the UI thread.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use gitcraft_core::model::FileDiff;
use iced::Color;
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SyntaxColor, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::SyntaxSet;

/// Diffs with more lines than this are shown without colours.
const LINE_LIMIT: usize = 6000;
/// Longer lines are left uncoloured: they are usually minified or generated.
const LENGTH_LIMIT: usize = 2000;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static LIGHT_THEME: LazyLock<Theme> = LazyLock::new(|| syntax_theme(&crate::theme::LIGHT));
static DARK_THEME: LazyLock<Theme> = LazyLock::new(|| syntax_theme(&crate::theme::DARK));

/// Coloured runs per (hunk, line): colour and byte range within the line's text.
pub type Highlights = HashMap<(usize, usize), Vec<(Color, std::ops::Range<usize>)>>;

fn syntax_color(color: Color) -> SyntaxColor {
    let [r, g, b, a] = color.into_rgba8();
    SyntaxColor { r, g, b, a }
}

/// Maps language scopes onto the app's syntax colours.
fn syntax_theme(c: &crate::theme::Colors) -> Theme {
    let rules: [(&str, Color); 9] = [
        ("comment", c.hl_comment),
        ("string, constant.other.symbol", c.hl_string),
        (
            "constant.numeric, constant.language, constant.character",
            c.hl_number,
        ),
        ("keyword, storage, support.function.builtin", c.hl_keyword),
        (
            "entity.name.function, support.function, markup.heading",
            c.hl_title,
        ),
        (
            "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, support.type, support.class",
            c.hl_type,
        ),
        (
            "entity.other.attribute-name, variable.other.member, support.type.property-name",
            c.hl_attr,
        ),
        (
            "meta.preprocessor, meta.annotation, meta.attribute",
            c.hl_meta,
        ),
        (
            "variable.other.constant, variable.parameter, entity.name.tag",
            c.hl_variable,
        ),
    ];
    Theme {
        name: None,
        author: None,
        settings: ThemeSettings {
            foreground: Some(syntax_color(c.text)),
            ..ThemeSettings::default()
        },
        scopes: rules
            .iter()
            .map(|(selector, color)| ThemeItem {
                scope: selector
                    .parse::<ScopeSelectors>()
                    .expect("the selectors above are valid"),
                style: StyleModifier {
                    foreground: Some(syntax_color(*color)),
                    background: None,
                    font_style: None,
                },
            })
            .collect(),
    }
}

fn theme(dark: bool) -> &'static Theme {
    if dark { &DARK_THEME } else { &LIGHT_THEME }
}

/// Highlights hunk by hunk, so a construct opened inside a hunk carries on through it.
/// Old and new lines get their own state, as they are two different versions of the file.
pub fn highlight(diff: &FileDiff, dark: bool) -> Option<Arc<Highlights>> {
    let total: usize = diff.hunks.iter().map(|hunk| hunk.lines.len()).sum();
    if total == 0 || total > LINE_LIMIT {
        return None;
    }
    let path = diff.new_path.as_deref().or(diff.old_path.as_deref())?;
    let extension = path.rsplit('.').next().unwrap_or("");
    let name = path.rsplit('/').next().unwrap_or(path);
    let syntax = SYNTAXES
        .find_syntax_by_extension(extension)
        .or_else(|| SYNTAXES.find_syntax_by_extension(name))?;
    if syntax.name == "Plain Text" {
        return None;
    }
    let theme = theme(dark);
    let mut result = Highlights::new();
    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
        let mut old = HighlightLines::new(syntax, theme);
        let mut new = HighlightLines::new(syntax, theme);
        for (line_index, line) in hunk.lines.iter().enumerate() {
            if line.text.len() > LENGTH_LIMIT || line.t == '\\' {
                continue;
            }
            let with_newline = format!("{}\n", line.text);
            let regions = match line.t {
                '+' => new.highlight_line(&with_newline, &SYNTAXES),
                '-' => old.highlight_line(&with_newline, &SYNTAXES),
                _ => {
                    let _ = old.highlight_line(&with_newline, &SYNTAXES);
                    new.highlight_line(&with_newline, &SYNTAXES)
                }
            };
            let Ok(regions) = regions else { continue };
            let mut at = 0;
            let mut runs = Vec::with_capacity(regions.len());
            for (style, piece) in regions {
                let end = (at + piece.len()).min(line.text.len());
                if end > at {
                    let c = style.foreground;
                    runs.push((Color::from_rgb8(c.r, c.g, c.b), at..end));
                }
                at += piece.len();
            }
            result.insert((hunk_index, line_index), runs);
        }
    }
    Some(Arc::new(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcraft_core::parsers::parse_diff;

    #[test]
    fn colours_known_languages_and_covers_each_line_exactly() {
        let diff = b"diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,1 @@\n-fn old() {}\n+fn gr\xc3\xbc\xc3\x9fe() {}\n";
        let files = parse_diff(diff);
        let colours = highlight(&files[0], true).expect("Rust is known");
        let line = &files[0].hunks[0].lines[1];
        let runs = &colours[&(0, 1)];
        assert!(runs.len() > 1, "more than one colour");
        let mut at = 0;
        for (_, range) in runs {
            assert_eq!(range.start, at);
            assert!(line.text.is_char_boundary(range.end));
            at = range.end;
        }
        assert_eq!(at, line.text.len());
    }

    #[test]
    fn skips_unknown_languages_and_huge_diffs() {
        let files = parse_diff(
            b"diff --git a/a.zzzz b/a.zzzz\n--- a/a.zzzz\n+++ b/a.zzzz\n@@ -1 +1 @@\n-a\n+b\n",
        );
        assert!(highlight(&files[0], true).is_none());
        let mut text =
            b"diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,7000 @@\n".to_vec();
        text.extend(std::iter::repeat_n(b"+x\n".as_slice(), LINE_LIMIT + 1).flatten());
        assert!(highlight(&parse_diff(&text)[0], true).is_none());
    }
}
