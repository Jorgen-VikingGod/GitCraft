//! GitCraft's own look, in light and dark, in the style of the other Craft apps: neutral grey
//! panels, cards with a faint edge and GitCraft's blue as the accent. The OS only decides which
//! of the two applies (watched through craft-platform, see `appearance.rs`).

use egui::{Color32, FontFamily, FontId};

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
    /// The edge of a card (a sidebar section, a group in a dialog).
    pub card_line: Color32,
    /// A raised fill on a panel: an idle button, a segmented switch.
    pub panel_2: Color32,
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
    /// Buttons that undo a step without losing anything, such as unstaging: orange.
    pub caution: Color32,
    pub danger: Color32,
    /// The close button of a Windows-style title bar under the pointer.
    pub close_hover: Color32,
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
    bg: hex(0xf6f7f9),
    panel: hex(0xffffff),
    sidebar: hex(0xe8eaef),
    titlebar: hex(0xeef0f4),
    toolbar: hex(0xeef0f4),
    input: hex(0xffffff),
    border: hex(0xcdd2dc),
    card_line: hex(0xd4d9e3),
    panel_2: hex(0xe6e9ef),
    text: hex(0x14171d),
    text_muted: hex(0x3f4653),
    text_faint: hex(0x5d6574),
    hover: rgba(30, 50, 90, 0.06),
    hover_strong: hex(0xd6dbe5),
    selection: hex(0xc2dcfb),
    selection_text: hex(0x0f1d3d),
    accent: hex(0x2e8ff0),
    accent_soft: hex(0xdcebfd),
    success: hex(0x1e8240),
    warning: hex(0x946200),
    caution: hex(0xe07800),
    danger: hex(0xc4302a),
    close_hover: hex(0xc42b1c),
    warning_bg: hex(0xfff4dc),
    backdrop: rgba(20, 26, 36, 0.22),
    shadow: rgba(20, 30, 50, 0.22),
    diff_add_bg: hex(0xe9f7ee),
    diff_add_text: hex(0x1a7f43),
    diff_del_bg: hex(0xfdecec),
    diff_del_text: hex(0xc0392b),
    diff_hunk_bg: hex(0xf0f3f8),
    diff_filler: hex(0xf4f5f7),
    diff_selected: hex(0xb6d4fa),
    conflict_ours: hex(0xeef6ff),
    conflict_theirs: hex(0xf6efff),
    badge_local_bg: hex(0xdbeafe),
    badge_local_text: hex(0x1662b8),
    badge_remote_bg: hex(0xe3f4ea),
    badge_remote_text: hex(0x1c7a45),
    badge_tag_bg: hex(0xfff0d4),
    badge_tag_text: hex(0x94600a),
    badge_head_bg: hex(0xffe1e1),
    badge_head_text: hex(0xb02727),
    lanes: [
        hex(0x2e8ff0),
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
    bg: hex(0x181b21),
    panel: hex(0x1c1f26),
    sidebar: hex(0x111318),
    titlebar: hex(0x14161b),
    toolbar: hex(0x14161b),
    input: hex(0x0e1014),
    border: hex(0x2e333d),
    card_line: hex(0x262a33),
    panel_2: hex(0x20242c),
    text: hex(0xdee2e9),
    text_muted: hex(0x9299a6),
    text_faint: hex(0x626976),
    hover: rgba(255, 255, 255, 0.05),
    hover_strong: hex(0x2e333e),
    selection: hex(0x1f3d5e),
    selection_text: hex(0xdee2e9),
    accent: hex(0x2e8ff0),
    accent_soft: hex(0x1c2d40),
    success: hex(0x5ec478),
    warning: hex(0xe8ba4a),
    caution: hex(0xf0883e),
    danger: hex(0xe8544c),
    close_hover: hex(0xc42b1c),
    warning_bg: hex(0x3a2f17),
    backdrop: rgba(0, 0, 0, 0.5),
    shadow: rgba(0, 0, 0, 0.55),
    diff_add_bg: hex(0x1d3326),
    diff_add_text: hex(0x5ccf8a),
    diff_del_bg: hex(0x3a2023),
    diff_del_text: hex(0xf07d7d),
    diff_hunk_bg: hex(0x20242c),
    diff_filler: hex(0x15171c),
    diff_selected: hex(0x24476e),
    conflict_ours: hex(0x1e2a3a),
    conflict_theirs: hex(0x2a2238),
    badge_local_bg: hex(0x1d3d62),
    badge_local_text: hex(0x9ccbff),
    badge_remote_bg: hex(0x1f3b2b),
    badge_remote_text: hex(0x8fdcaf),
    badge_tag_bg: hex(0x43341a),
    badge_tag_text: hex(0xf2c879),
    badge_head_bg: hex(0x4a2224),
    badge_head_text: hex(0xffb1b1),
    lanes: [
        hex(0x4aa2f5),
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
    /// The setting is "system": the window's own title bar and dialogs follow the OS too.
    pub follow_system: bool,
    pub text_size: f32,
    /// Width of one character of diff text, for working out where lines wrap.
    pub char_width: f32,
}

/// The fills of a control, from [`Look::fills`].
#[derive(Debug, Clone, Copy)]
pub struct Fills {
    pub idle: Color32,
    pub hover: Color32,
    pub press: Color32,
    pub hover_edge: Color32,
    pub press_edge: Color32,
}

pub const CODE_SIZE: f32 = 12.5;
/// The gap between the entries of a dropdown list or a context menu.
pub const DROPDOWN_GAP: f32 = 2.0;

impl Look {
    fn build(dark: bool) -> Self {
        Self {
            c: if dark { DARK } else { LIGHT },
            dark,
            follow_system: false,
            text_size: 13.0,
            char_width: CODE_SIZE * 0.6,
        }
    }

    /// `choice` is "system", "light" or "dark"; `system_dark` is what the OS is set to.
    pub fn for_setting(choice: &str, system_dark: bool) -> Self {
        let mut look = Self::build(match choice {
            "light" => false,
            "dark" => true,
            _ => system_dark,
        });
        look.follow_system = !matches!(choice, "light" | "dark");
        look
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

    /// The fills of buttons, fields and menu rows in the neutral, hovered and pressed states.
    pub fn fills(&self) -> Fills {
        let rgb = Color32::from_rgb;
        if self.dark {
            Fills {
                idle: rgb(48, 54, 66),
                hover: rgb(46, 51, 62),
                press: rgb(56, 62, 75),
                hover_edge: rgb(78, 86, 102),
                press_edge: rgb(110, 120, 140),
            }
        } else {
            Fills {
                idle: rgb(222, 226, 234),
                hover: rgb(212, 217, 228),
                press: rgb(198, 205, 219),
                hover_edge: rgb(150, 160, 180),
                press_edge: rgb(110, 122, 148),
            }
        }
    }

    /// Styles egui's own widgets (buttons, text fields, menus, scroll bars) from the palette,
    /// so that they look the same in every part of the window, and as in the other Craft apps.
    pub fn apply(&self, ctx: &egui::Context) {
        use egui::{CornerRadius, Stroke, TextStyle, Vec2};
        // Replaced, not added: egui keeps the first provider, which would keep the first theme.
        ctx.replace_widget_theme::<egui::widget_style::ButtonStyle>(SelectedOutline);
        ctx.replace_widget_theme::<egui::widget_style::TextEditStyle>(FieldBorder {
            light: !self.dark,
        });
        let c = &self.c;
        let mut visuals = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        let Fills {
            idle,
            hover,
            press,
            hover_edge,
            press_edge,
        } = self.fills();
        let strong = if self.dark {
            Color32::WHITE
        } else {
            Color32::BLACK
        };
        visuals.override_text_color = Some(c.text);
        visuals.panel_fill = c.bg;
        visuals.window_fill = c.panel;
        visuals.extreme_bg_color = c.input;
        visuals.faint_bg_color = c.panel_2;
        visuals.window_stroke = Stroke::new(1.0, c.border);
        visuals.window_corner_radius = CornerRadius::same(8);
        visuals.menu_corner_radius = CornerRadius::same(6);
        visuals.popup_shadow = egui::epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: c.shadow,
        };
        visuals.hyperlink_color = c.accent;
        visuals.selection.bg_fill = c.selection;
        // Also the outline of a focused field. On light panels the accent is darkened a little.
        let ink = if self.dark {
            c.accent
        } else {
            mix(c.accent, Color32::BLACK, 0.2)
        };
        visuals.selection.stroke = Stroke::new(1.0, ink);
        visuals.text_cursor.stroke = Stroke::new(2.0, c.accent);
        let round = CornerRadius::same(4);
        let w = &mut visuals.widgets;
        w.noninteractive.bg_stroke = Stroke::new(1.0, c.border);
        w.noninteractive.fg_stroke = Stroke::new(1.0, c.text);
        w.noninteractive.corner_radius = round;
        w.inactive.bg_fill = idle;
        w.inactive.weak_bg_fill = c.panel_2;
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, c.text);
        w.inactive.corner_radius = round;
        w.hovered.bg_fill = hover;
        w.hovered.weak_bg_fill = hover;
        w.hovered.bg_stroke = Stroke::new(1.0, hover_edge);
        w.hovered.fg_stroke = Stroke::new(1.0, strong);
        w.hovered.corner_radius = round;
        w.active.bg_fill = press;
        w.active.weak_bg_fill = press;
        w.active.bg_stroke = Stroke::new(1.0, press_edge);
        w.active.fg_stroke = Stroke::new(1.0, strong);
        w.active.corner_radius = round;
        w.open.bg_fill = hover;
        w.open.weak_bg_fill = hover;
        w.open.bg_stroke = Stroke::NONE;
        w.open.corner_radius = round;

        let (size, code) = (self.text_size, CODE_SIZE);
        let mut style = (*ctx.global_style()).clone();
        style.visuals = visuals;
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
        s.menu_margin = 6.into();
        s.window_margin = 12.into();
        // Thin until the pointer comes near, then wide enough to grab. Set in from the edge, so
        // that a bar along the window's edge isn't under the window's resize handle.
        s.scroll.bar_width = 9.0;
        s.scroll.floating = true;
        s.scroll.floating_width = 5.0;
        s.scroll.floating_allocated_width = 8.0;
        s.scroll.bar_outer_margin = 5.0;
        style.interaction.selectable_labels = false;
        // Follow the app's own choice, not the system's: otherwise a light system theme swaps in
        // egui's light style for popups, which is unreadable on dark panels.
        let dark = self.dark;
        ctx.options_mut(|options| {
            options.theme_preference = if dark {
                egui::ThemePreference::Dark
            } else {
                egui::ThemePreference::Light
            };
        });
        ctx.set_style_of(egui::Theme::Dark, style.clone());
        ctx.set_style_of(egui::Theme::Light, style);
        // The system's title bar and dialogs follow the same choice. When following the system,
        // leave them alone, or the window would stop reporting the system's changes.
        let system = if self.follow_system {
            egui::SystemTheme::SystemDefault
        } else if dark {
            egui::SystemTheme::Dark
        } else {
            egui::SystemTheme::Light
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(system));
    }
}

/// egui's own text field look, with a border on light panels: there the field is as white as
/// the panel, and without one only the hovered field could be seen.
struct FieldBorder {
    light: bool,
}

impl egui::theme::StyleProvider<egui::widget_style::TextEditStyle> for FieldBorder {
    fn style(
        &mut self,
        args: &egui::widget_style::StyleArgs<'_>,
    ) -> egui::widget_style::TextEditStyle {
        use egui::theme::StyleProvider;
        let mut style: egui::widget_style::TextEditStyle =
            StyleProvider::style(&mut egui::theme::DefaultStyle, args);
        let frame = &mut style.atom_layout.frame;
        if self.light && frame.stroke.width == 0.0 {
            *frame = frame.apply_stroke_and_expansion_without_layout_shift(
                egui::Stroke::new(1.0, Color32::from_rgb(176, 184, 198)),
                0.0,
            );
        }
        style
    }
}

/// egui's own button look, with the selected row of a list or drop-down outlined in the accent,
/// like a focused text field.
struct SelectedOutline;

impl egui::theme::StyleProvider<egui::widget_style::ButtonStyle> for SelectedOutline {
    fn style(
        &mut self,
        args: &egui::widget_style::StyleArgs<'_>,
    ) -> egui::widget_style::ButtonStyle {
        use egui::class::HasClasses as _;
        use egui::theme::StyleProvider;
        let mut style: egui::widget_style::ButtonStyle =
            StyleProvider::style(&mut egui::theme::DefaultStyle, args);
        if args.classes.has_class(&egui::Button::CLASS_SELECTED) {
            let frame = &mut style.atom_layout.frame;
            *frame = frame.apply_stroke_and_expansion_without_layout_shift(
                args.style.visuals.selection.stroke,
                0.0,
            );
        }
        style
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
    fn the_setting_or_the_system_decides_dark() {
        assert!(Look::for_setting("system", true).dark);
        assert!(!Look::for_setting("system", false).dark);
        assert!(Look::for_setting("system", false).follow_system);
        assert!(!Look::for_setting("light", true).dark);
        assert!(!Look::for_setting("light", true).follow_system);
        assert!(Look::for_setting("dark", false).dark);
    }

    #[test]
    fn colours_keep_their_alpha() {
        assert_eq!(rgba(0, 0, 0, 0.5).a(), 128);
        assert_eq!(hex(0x102a63), Color32::from_rgb(0x10, 0x2a, 0x63));
    }
}
