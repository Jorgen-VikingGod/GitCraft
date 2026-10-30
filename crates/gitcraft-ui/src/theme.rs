//! GitCraft's own look, in light and dark. The OS only decides which of the two
//! applies and which font is used; that is the only part read through native-theme.

use iced::font::{Style as FontStyle, Weight};
use iced::theme::Palette;
use iced::{Color, Font, Theme};
use native_theme_iced::native_theme::SystemTheme;
use native_theme_iced::native_theme::theme::ColorMode;
use native_theme_iced::{ResolvedTheme, font_family};

const fn hex(rgb: u32) -> Color {
    Color {
        r: ((rgb >> 16) & 0xff) as f32 / 255.0,
        g: ((rgb >> 8) & 0xff) as f32 / 255.0,
        b: (rgb & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub bg: Color,
    pub panel: Color,
    pub sidebar: Color,
    pub titlebar: Color,
    pub toolbar: Color,
    pub input: Color,
    pub border: Color,
    pub text: Color,
    pub text_muted: Color,
    pub text_faint: Color,
    pub hover: Color,
    pub hover_strong: Color,
    pub selection: Color,
    pub selection_text: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub warning_bg: Color,
    pub backdrop: Color,
    pub shadow: Color,
    pub diff_add_bg: Color,
    pub diff_add_text: Color,
    pub diff_del_bg: Color,
    pub diff_del_text: Color,
    pub diff_hunk_bg: Color,
    pub diff_filler: Color,
    pub diff_selected: Color,
    pub conflict_ours: Color,
    pub conflict_theirs: Color,
    pub badge_local_bg: Color,
    pub badge_local_text: Color,
    pub badge_remote_bg: Color,
    pub badge_remote_text: Color,
    pub badge_tag_bg: Color,
    pub badge_tag_text: Color,
    pub badge_head_bg: Color,
    pub badge_head_text: Color,
    pub lanes: [Color; 8],
    pub hl_keyword: Color,
    pub hl_string: Color,
    pub hl_number: Color,
    pub hl_comment: Color,
    pub hl_title: Color,
    pub hl_type: Color,
    pub hl_attr: Color,
    pub hl_meta: Color,
    pub hl_variable: Color,
}

pub const LIGHT: Colors = Colors {
    bg: hex(0xffffff),
    panel: hex(0xfafbfc),
    sidebar: hex(0xf3f5f8),
    titlebar: hex(0xe9ecf1),
    toolbar: hex(0xf6f7f9),
    input: hex(0xffffff),
    border: hex(0xd9dee6),
    text: hex(0x1d232c),
    text_muted: hex(0x5e6875),
    text_faint: hex(0x98a1ad),
    hover: rgba(30, 50, 90, 0.06),
    hover_strong: rgba(30, 50, 90, 0.12),
    selection: hex(0xdbe6fd),
    selection_text: hex(0x102a63),
    accent: hex(0x3d6ce7),
    accent_soft: hex(0xe8eefc),
    success: hex(0x1f9d55),
    warning: hex(0xc27c0e),
    danger: hex(0xd93a3a),
    warning_bg: hex(0xfff4dc),
    backdrop: rgba(20, 26, 36, 0.35),
    shadow: rgba(20, 30, 50, 0.22),
    diff_add_bg: hex(0xe9f7ee),
    diff_add_text: hex(0x1a7f43),
    diff_del_bg: hex(0xfdecec),
    diff_del_text: hex(0xc0392b),
    diff_hunk_bg: hex(0xf0f3f8),
    diff_filler: hex(0xf4f5f7),
    diff_selected: hex(0xc9dafc),
    conflict_ours: hex(0xeef6ff),
    conflict_theirs: hex(0xf6efff),
    badge_local_bg: hex(0xdfe9ff),
    badge_local_text: hex(0x2550c0),
    badge_remote_bg: hex(0xe3f4ea),
    badge_remote_text: hex(0x1c7a45),
    badge_tag_bg: hex(0xfff0d4),
    badge_tag_text: hex(0x94600a),
    badge_head_bg: hex(0xffe1e1),
    badge_head_text: hex(0xb02727),
    lanes: [
        hex(0x3d6ce7),
        hex(0xe5484d),
        hex(0x2f9e5c),
        hex(0xc17a00),
        hex(0x8e4ec6),
        hex(0x0e8ea7),
        hex(0xd6409f),
        hex(0x6f7d1a),
    ],
    hl_keyword: hex(0xa626a4),
    hl_string: hex(0x50a14f),
    hl_number: hex(0x986801),
    hl_comment: hex(0x8a929c),
    hl_title: hex(0x4078f2),
    hl_type: hex(0xc18401),
    hl_attr: hex(0x986801),
    hl_meta: hex(0x4078f2),
    hl_variable: hex(0xe45649),
};

pub const DARK: Colors = Colors {
    bg: hex(0x1b1e24),
    panel: hex(0x21252c),
    sidebar: hex(0x1e2127),
    titlebar: hex(0x17191e),
    toolbar: hex(0x23272e),
    input: hex(0x2a2f37),
    border: hex(0x353b45),
    text: hex(0xe3e7ed),
    text_muted: hex(0x9aa4b2),
    text_faint: hex(0x646d79),
    hover: rgba(255, 255, 255, 0.05),
    hover_strong: rgba(255, 255, 255, 0.1),
    selection: hex(0x2c4b8f),
    selection_text: hex(0xf2f6ff),
    accent: hex(0x5b8af5),
    accent_soft: hex(0x24324d),
    success: hex(0x3fb871),
    warning: hex(0xe0a43a),
    danger: hex(0xf06262),
    warning_bg: hex(0x3a2f17),
    backdrop: rgba(0, 0, 0, 0.5),
    shadow: rgba(0, 0, 0, 0.55),
    diff_add_bg: hex(0x1d3326),
    diff_add_text: hex(0x5ccf8a),
    diff_del_bg: hex(0x3a2023),
    diff_del_text: hex(0xf07d7d),
    diff_hunk_bg: hex(0x252a33),
    diff_filler: hex(0x1f2228),
    diff_selected: hex(0x2e4a80),
    conflict_ours: hex(0x1e2a3a),
    conflict_theirs: hex(0x2a2238),
    badge_local_bg: hex(0x253a66),
    badge_local_text: hex(0xa9c3ff),
    badge_remote_bg: hex(0x1f3b2b),
    badge_remote_text: hex(0x8fdcaf),
    badge_tag_bg: hex(0x43341a),
    badge_tag_text: hex(0xf2c879),
    badge_head_bg: hex(0x4a2224),
    badge_head_text: hex(0xffb1b1),
    lanes: [
        hex(0x6b93f7),
        hex(0xf2676b),
        hex(0x4cc27c),
        hex(0xe5a33a),
        hex(0xb083e6),
        hex(0x36b9d1),
        hex(0xec6fbd),
        hex(0xa8b84a),
    ],
    hl_keyword: hex(0xc678dd),
    hl_string: hex(0x98c379),
    hl_number: hex(0xd19a66),
    hl_comment: hex(0x7f8896),
    hl_title: hex(0x61afef),
    hl_type: hex(0xe5c07b),
    hl_attr: hex(0xd19a66),
    hl_meta: hex(0x61afef),
    hl_variable: hex(0xe06c75),
};

#[derive(Debug, Clone)]
pub struct Look {
    pub theme: Theme,
    pub c: Colors,
    pub dark: bool,
    pub text_size: f32,
    pub font: Font,
    pub semibold: Font,
    pub bold: Font,
    pub italic: Font,
    pub mono: Font,
    pub mono_bold: Font,
    /// Width of one character of diff text, for working out where lines wrap.
    pub char_width: f32,
}

fn leaked(name: &str) -> &'static str {
    Box::leak(name.to_owned().into_boxed_str())
}

impl Look {
    fn build(dark: bool, font: Font, mono: Font) -> Self {
        let c = if dark { DARK } else { LIGHT };
        let theme = Theme::custom(
            "GitCraft",
            Palette {
                background: c.bg,
                text: c.text,
                primary: c.accent,
                success: c.success,
                warning: c.warning,
                danger: c.danger,
            },
        );
        Self {
            theme,
            c,
            dark,
            text_size: 13.0,
            font,
            // Many families have no semibold face, and asking for one can switch typeface.
            semibold: Font {
                weight: Weight::Bold,
                ..font
            },
            bold: Font {
                weight: Weight::Bold,
                ..font
            },
            italic: Font {
                style: FontStyle::Italic,
                ..font
            },
            mono,
            mono_bold: Font {
                weight: Weight::Bold,
                ..mono
            },
            char_width: measure_char_width(mono),
        }
    }

    /// "system", "light" or "dark". A failed OS read means light, with default fonts.
    pub fn for_setting(choice: &str) -> Self {
        let system = SystemTheme::from_system().ok();
        let os_dark = system
            .as_ref()
            .is_some_and(|system| matches!(system.mode, ColorMode::Dark));
        let dark = match choice {
            "light" => false,
            "dark" => true,
            _ => os_dark,
        };
        let font = system.as_ref().map_or(Font::DEFAULT, |system| {
            let resolved: &ResolvedTheme = if system.mode.is_dark() {
                &system.dark
            } else {
                &system.light
            };
            named_font(font_family(resolved))
        });
        // The OS often reports just "Monospace", which is no family: looking it up gives a
        // proportional font, and diffs would stop lining up.
        let mono = Font::MONOSPACE;
        Self::build(dark, font, mono)
    }

    /// Doesn't depend on the machine, for tests.
    pub fn preset(dark: bool) -> Self {
        Self::build(dark, Font::DEFAULT, Font::MONOSPACE)
    }
}

/// Generic names such as "Sans" aren't families either, so they get the default font.
fn named_font(name: &str) -> Font {
    match name.to_ascii_lowercase().as_str() {
        "" | "sans" | "sans serif" | "sans-serif" | "system-ui" | "serif" | "monospace" => {
            Font::DEFAULT
        }
        _ => Font::with_name(leaked(name)),
    }
}

fn measure_char_width(font: Font) -> f32 {
    use iced::advanced::text::{self, Paragraph as _};
    type Paragraph = <iced::Renderer as text::Renderer>::Paragraph;
    const SAMPLE: &str = "0000000000000000000000000000000000000000";
    let paragraph = Paragraph::with_text(text::Text {
        content: SAMPLE,
        bounds: iced::Size::INFINITE,
        size: crate::theme::CODE_SIZE.into(),
        line_height: text::LineHeight::default(),
        font,
        align_x: text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Basic,
        wrapping: text::Wrapping::None,
    });
    let width = paragraph.min_width() / SAMPLE.len() as f32;
    if width > 1.0 { width } else { CODE_SIZE * 0.6 }
}

pub const CODE_SIZE: f32 = 12.5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_font_names_are_not_looked_up_as_families() {
        assert_eq!(named_font("Sans"), Font::DEFAULT);
        assert_eq!(named_font("monospace"), Font::DEFAULT);
        assert_eq!(named_font("Cantarell"), Font::with_name("Cantarell"));
    }

    #[test]
    fn code_is_always_monospaced() {
        assert_eq!(Look::for_setting("dark").mono, Font::MONOSPACE);
    }
}
