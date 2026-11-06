//! GitCraft's own look, in light and dark. The OS only decides which of the two applies.

use egui::{Color32, FontFamily, FontId};
use native_theme::SystemTheme;
use native_theme::theme::ColorMode;

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// `alpha` is 0 to 1; the colour is straight (not premultiplied), as CSS writes it.
const fn rgba(r: u8, g: u8, b: u8, alpha: f32) -> Color32 {
    Color32::from_rgba_premultiplied(
        (r as f32 * alpha + 0.5) as u8,
        (g as f32 * alpha + 0.5) as u8,
        (b as f32 * alpha + 0.5) as u8,
        (alpha * 255.0 + 0.5) as u8,
    )
}

#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub bg: Color32,
    pub panel: Color32,
    pub sidebar: Color32,
    pub titlebar: Color32,
    pub toolbar: Color32,
    pub input: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub hover: Color32,
    pub hover_strong: Color32,
    pub selection: Color32,
    pub selection_text: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub warning_bg: Color32,
    pub backdrop: Color32,
    pub shadow: Color32,
    pub diff_add_bg: Color32,
    pub diff_add_text: Color32,
    pub diff_del_bg: Color32,
    pub diff_del_text: Color32,
    pub diff_hunk_bg: Color32,
    pub diff_filler: Color32,
    pub diff_selected: Color32,
    pub conflict_ours: Color32,
    pub conflict_theirs: Color32,
    pub badge_local_bg: Color32,
    pub badge_local_text: Color32,
    pub badge_remote_bg: Color32,
    pub badge_remote_text: Color32,
    pub badge_tag_bg: Color32,
    pub badge_tag_text: Color32,
    pub badge_head_bg: Color32,
    pub badge_head_text: Color32,
    pub lanes: [Color32; 8],
    pub hl_keyword: Color32,
    pub hl_string: Color32,
    pub hl_number: Color32,
    pub hl_comment: Color32,
    pub hl_title: Color32,
    pub hl_type: Color32,
    pub hl_attr: Color32,
    pub hl_meta: Color32,
    pub hl_variable: Color32,
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
    pub c: Colors,
    pub dark: bool,
    pub text_size: f32,
    /// Width of one character of diff text, for working out where lines wrap.
    pub char_width: f32,
}

pub const CODE_SIZE: f32 = 12.5;

impl Look {
    fn build(dark: bool) -> Self {
        Self {
            c: if dark { DARK } else { LIGHT },
            dark,
            text_size: 13.0,
            char_width: CODE_SIZE * 0.6,
        }
    }

    /// "system", "light" or "dark". A failed OS read means light.
    pub fn for_setting(choice: &str) -> Self {
        let os_dark = SystemTheme::from_system()
            .ok()
            .is_some_and(|system| matches!(system.mode, ColorMode::Dark));
        Self::build(match choice {
            "light" => false,
            "dark" => true,
            _ => os_dark,
        })
    }

    /// Doesn't depend on the machine, for tests.
    pub fn preset(dark: bool) -> Self {
        Self::build(dark)
    }

    pub fn ui(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Proportional)
    }

    pub fn mono(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Monospace)
    }

    /// Measures the code font once there is a context to measure with.
    pub fn measure(&mut self, ctx: &egui::Context) {
        let width = ctx.fonts_mut(|fonts| fonts.glyph_width(&self.mono(CODE_SIZE), '0'));
        if width > 1.0 {
            self.char_width = width;
        }
    }

    /// Styles egui's own widgets (buttons, text fields, menus, scroll bars) from the palette,
    /// so that they look the same in every part of the window.
    pub fn apply(&self, ctx: &egui::Context) {
        use egui::{CornerRadius, Stroke, TextStyle, Vec2};
        let c = &self.c;
        let mut visuals = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.override_text_color = Some(c.text);
        visuals.panel_fill = c.bg;
        visuals.window_fill = c.panel;
        visuals.extreme_bg_color = c.input;
        visuals.faint_bg_color = c.panel;
        visuals.window_stroke = Stroke::new(1.0, c.border);
        visuals.window_corner_radius = CornerRadius::same(10);
        visuals.menu_corner_radius = CornerRadius::same(8);
        visuals.popup_shadow = egui::epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: c.shadow,
        };
        visuals.hyperlink_color = c.accent;
        visuals.selection.bg_fill = c.selection;
        visuals.selection.stroke = Stroke::new(1.0, c.accent);
        visuals.text_cursor.stroke = Stroke::new(1.5, c.accent);
        let hover_fill = mix(c.input, c.text, 0.09);
        let round = CornerRadius::same(5);
        let w = &mut visuals.widgets;
        w.noninteractive.bg_stroke = Stroke::new(1.0, c.border);
        w.noninteractive.fg_stroke = Stroke::new(1.0, c.text);
        w.noninteractive.corner_radius = round;
        w.inactive.bg_fill = c.input;
        w.inactive.weak_bg_fill = c.input;
        w.inactive.bg_stroke = Stroke::new(1.0, c.border);
        w.inactive.fg_stroke = Stroke::new(1.0, c.text);
        w.inactive.corner_radius = round;
        w.hovered.bg_fill = hover_fill;
        w.hovered.weak_bg_fill = hover_fill;
        w.hovered.bg_stroke = Stroke::new(1.0, mix(c.border, c.text, 0.35));
        w.hovered.fg_stroke = Stroke::new(1.0, c.text);
        w.hovered.corner_radius = round;
        w.active.bg_fill = mix(c.input, c.accent, 0.3);
        w.active.weak_bg_fill = mix(c.input, c.accent, 0.3);
        w.active.bg_stroke = Stroke::new(1.0, c.accent);
        w.active.fg_stroke = Stroke::new(1.0, c.text);
        w.active.corner_radius = round;
        w.open.bg_fill = hover_fill;
        w.open.weak_bg_fill = hover_fill;
        w.open.bg_stroke = Stroke::new(1.0, c.border);
        w.open.corner_radius = round;

        let (size, code) = (self.text_size, CODE_SIZE);
        ctx.all_styles_mut(|style| {
            style.visuals = visuals.clone();
            style.text_styles = [
                (
                    TextStyle::Heading,
                    FontId::new(16.0, FontFamily::Proportional),
                ),
                (TextStyle::Body, FontId::new(size, FontFamily::Proportional)),
                (
                    TextStyle::Button,
                    FontId::new(size, FontFamily::Proportional),
                ),
                (
                    TextStyle::Small,
                    FontId::new(11.0, FontFamily::Proportional),
                ),
                (
                    TextStyle::Monospace,
                    FontId::new(code, FontFamily::Monospace),
                ),
            ]
            .into();
            let s = &mut style.spacing;
            s.item_spacing = Vec2::new(6.0, 5.0);
            s.button_padding = Vec2::new(8.0, 4.0);
            s.interact_size = Vec2::new(40.0, 22.0);
            s.combo_width = 190.0;
            s.menu_margin = 4.into();
            s.window_margin = 12.into();
            s.scroll.bar_width = 8.0;
            s.scroll.floating = true;
            s.scroll.floating_width = 5.0;
            s.scroll.floating_allocated_width = 8.0;
            style.interaction.selectable_labels = false;
        });
    }
}

/// `a` moved `t` of the way towards `b`, opaque.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let lerp = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_always_monospaced() {
        assert_eq!(
            Look::preset(true).mono(12.0).family,
            egui::FontFamily::Monospace
        );
    }

    #[test]
    fn colours_keep_their_alpha() {
        assert_eq!(rgba(0, 0, 0, 0.5).a(), 128);
        assert_eq!(hex(0x102a63), Color32::from_rgb(0x10, 0x2a, 0x63));
    }
}
