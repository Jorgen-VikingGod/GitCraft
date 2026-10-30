//! The only place that talks to native-theme: the OS theme with a bundled fallback,
//! and the colours no OS theme has an opinion on.

use iced::{Color, Theme};
use native_theme_iced::native_theme::SystemTheme;
use native_theme_iced::native_theme::theme::ColorMode;
use native_theme_iced::{AccessibilityPreferences, ResolvedTheme, font_size, to_theme_in};

const FALLBACK_PRESET: &str = "adwaita";

/// Commit graph lanes, change states and badges, as readable on the current background.
#[derive(Debug, Clone)]
pub struct GitPalette {
    pub lanes: [Color; 8],
    pub added: Color,
    pub removed: Color,
    pub modified: Color,
    pub tag: Color,
    pub local: Color,
    pub remote: Color,
}

impl GitPalette {
    fn new(dark: bool) -> Self {
        let rgb = Color::from_rgb8;
        if dark {
            Self {
                lanes: [
                    rgb(0x5b, 0x9c, 0xf6),
                    rgb(0x4c, 0xc3, 0x7a),
                    rgb(0xf0, 0x8a, 0x5d),
                    rgb(0xb3, 0x8b, 0xf5),
                    rgb(0xf0, 0x6f, 0x9b),
                    rgb(0x3e, 0xc6, 0xc9),
                    rgb(0xe3, 0xb7, 0x41),
                    rgb(0x9a, 0xa5, 0xb1),
                ],
                added: rgb(0x4c, 0xc3, 0x7a),
                removed: rgb(0xf0, 0x6f, 0x6f),
                modified: rgb(0xe3, 0xb7, 0x41),
                tag: rgb(0xe3, 0xb7, 0x41),
                local: rgb(0x4c, 0xc3, 0x7a),
                remote: rgb(0x5b, 0x9c, 0xf6),
            }
        } else {
            Self {
                lanes: [
                    rgb(0x1f, 0x6f, 0xeb),
                    rgb(0x1a, 0x8f, 0x4c),
                    rgb(0xd4, 0x5a, 0x1e),
                    rgb(0x7c, 0x4d, 0xdb),
                    rgb(0xd0, 0x2f, 0x6b),
                    rgb(0x0b, 0x8f, 0x96),
                    rgb(0xa8, 0x7b, 0x00),
                    rgb(0x5d, 0x68, 0x73),
                ],
                added: rgb(0x1a, 0x8f, 0x4c),
                removed: rgb(0xc9, 0x2a, 0x2a),
                modified: rgb(0xa8, 0x7b, 0x00),
                tag: rgb(0xa8, 0x7b, 0x00),
                local: rgb(0x1a, 0x8f, 0x4c),
                remote: rgb(0x1f, 0x6f, 0xeb),
            }
        }
    }
}

fn selected_text(resolved: &ResolvedTheme) -> Color {
    let color = resolved.defaults.selection_text_color;
    Color::from_rgba8(color.r, color.g, color.b, f32::from(color.a) / 255.0)
}

#[derive(Debug, Clone)]
pub struct Look {
    pub theme: Theme,
    pub resolved: ResolvedTheme,
    pub palette: GitPalette,
    pub text_size: f32,
    /// Text on a selected row.
    pub selected_text: Color,
    pub dark: bool,
}

impl Look {
    fn build(theme: Theme, resolved: ResolvedTheme, dark: bool, text_size: f32) -> Self {
        let selected_text = selected_text(&resolved);
        Self {
            theme,
            resolved,
            palette: GitPalette::new(dark),
            text_size,
            selected_text,
            dark,
        }
    }

    /// "system", "light" or "dark". The OS theme is used in the chosen colour mode,
    /// and a bundled one if the OS can't be read.
    pub fn for_setting(choice: &str) -> Self {
        let Ok(system) = SystemTheme::from_system() else {
            return Self::preset(choice == "dark");
        };
        let mode = match choice {
            "light" => ColorMode::Light,
            "dark" => ColorMode::Dark,
            _ => system.mode,
        };
        let resolved = if mode.is_dark() {
            system.dark
        } else {
            system.light
        };
        let text_size = font_size(&resolved, &system.accessibility);
        let theme = to_theme_in(&resolved, &system.name, mode);
        Self::build(theme, resolved, mode.is_dark(), text_size)
    }

    /// A fixed theme that doesn't depend on the machine, for tests and as the fallback.
    pub fn preset(dark: bool) -> Self {
        let (theme, resolved) =
            native_theme_iced::from_preset(FALLBACK_PRESET, dark).expect("bundled preset loads");
        let text_size = font_size(&resolved, &AccessibilityPreferences::default());
        Self::build(theme, resolved, dark, text_size)
    }
}
