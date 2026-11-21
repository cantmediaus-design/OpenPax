//! The design system: spacing, sizes, colours and fonts shared by every screen.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Theme, ThemePreference, Visuals, vec2,
};
use serde::{Deserialize, Serialize};

// Spacing follows a 4 px grid.
pub const SPACE_S: f32 = 4.0;
pub const SPACE_M: f32 = 8.0;
pub const SPACE_L: f32 = 12.0;
pub const SPACE_XL: f32 = 16.0;
pub const SPACE_XXL: f32 = 24.0;

pub const RADIUS: u8 = 6;
pub const RADIUS_L: u8 = 10;

/// How far a control grows while hovered, so it lifts under the pointer.
const HOVER_EXPANSION: f32 = 1.0;
/// How far a control shrinks while pressed, so a click feels like a squish.
const PRESS_EXPANSION: f32 = -0.5;

/// Height of list rows (explorer, layers, elements).
pub const ROW_HEIGHT: f32 = 24.0;
/// Side of a square icon button.
pub const ICON_BUTTON: f32 = 26.0;
/// Glyph size inside icon buttons.
pub const ICON_SIZE: f32 = 16.0;
/// Height of the toolbars inside editor tabs.
pub const TOOLBAR_HEIGHT: f32 = 34.0;
/// Width of the vertical tool rail in the pixel editor.
pub const RAIL_WIDTH: f32 = 36.0;
pub const DIALOG_WIDTH: f32 = 400.0;

const FONT_ICONS: &str = "phosphor";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
    #[default]
    System,
    Dark,
    Light,
}

impl ThemeChoice {
    pub const ALL: [Self; 3] = [Self::System, Self::Dark, Self::Light];

    pub fn name(self) -> &'static str {
        match self {
            Self::System => "Match System",
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

/// Colours that egui's own `Visuals` has no slot for.
#[derive(Clone, Copy)]
pub struct Tokens {
    pub accent: Color32,
    /// Text and icons drawn on top of `accent`.
    pub on_accent: Color32,
    pub success: Color32,
    pub info: Color32,
    /// Background of toolbars inside editor tabs.
    pub toolbar: Color32,
    pub border: Color32,
    /// Tint of the transparency checkerboard (the texture itself is white and light grey).
    pub checker: Color32,
    pub pixel_grid: Color32,
    pub frame_line: Color32,
    pub canvas_border: Color32,
    pub axis: [Color32; 3],
    /// 3D viewport colours, sRGB-encoded floats.
    pub viewport_clear: [f32; 3],
    pub viewport_grid: [f32; 4],
    pub viewport_outline: [f32; 4],
    pub viewport_selection: [f32; 4],
}

const ACCENT: Color32 = Color32::from_rgb(255, 140, 13);
const AXIS: [Color32; 3] = [
    Color32::from_rgb(232, 86, 86),
    Color32::from_rgb(96, 190, 86),
    Color32::from_rgb(80, 140, 240),
];

pub fn tokens(dark: bool) -> Tokens {
    if dark {
        Tokens {
            accent: ACCENT,
            on_accent: Color32::from_rgb(28, 18, 4),
            success: Color32::from_rgb(110, 200, 120),
            info: Color32::from_rgb(110, 170, 240),
            toolbar: Color32::from_rgb(34, 35, 40),
            border: Color32::from_rgb(54, 56, 64),
            checker: Color32::from_gray(74),
            pixel_grid: Color32::from_black_alpha(56),
            frame_line: Color32::from_rgb(255, 196, 0),
            canvas_border: Color32::from_gray(120),
            axis: AXIS,
            viewport_clear: [0.086, 0.09, 0.106],
            viewport_grid: [1.0, 1.0, 1.0, 0.10],
            viewport_outline: [1.0, 1.0, 1.0, 0.30],
            viewport_selection: [1.0, 0.55, 0.05, 1.0],
        }
    } else {
        Tokens {
            accent: Color32::from_rgb(214, 106, 0),
            on_accent: Color32::WHITE,
            success: Color32::from_rgb(34, 140, 60),
            info: Color32::from_rgb(30, 100, 200),
            toolbar: Color32::from_rgb(236, 237, 241),
            border: Color32::from_rgb(204, 206, 214),
            checker: Color32::WHITE,
            pixel_grid: Color32::from_black_alpha(40),
            frame_line: Color32::from_rgb(214, 106, 0),
            canvas_border: Color32::from_gray(110),
            axis: AXIS,
            viewport_clear: [0.86, 0.87, 0.89],
            viewport_grid: [0.0, 0.0, 0.0, 0.12],
            viewport_outline: [0.0, 0.0, 0.0, 0.35],
            viewport_selection: [0.9, 0.42, 0.0, 1.0],
        }
    }
}

/// Tokens for the theme the given `Ui` is drawn in.
pub fn of(ui: &egui::Ui) -> Tokens {
    tokens(ui.visuals().dark_mode)
}

/// Installs fonts and both themes. Call once at startup.
pub fn install(ctx: &egui::Context, choice: ThemeChoice) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        FONT_ICONS.to_owned(),
        FontData::from_static(include_bytes!("../assets/Phosphor.ttf")).into(),
    );
    // After the text font, so ordinary characters never resolve to the icon font.
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let keys = fonts.families.entry(family).or_default();
        keys.insert(keys.len().min(1), FONT_ICONS.to_owned());
    }
    ctx.set_fonts(fonts);

    ctx.set_visuals_of(Theme::Dark, dark_visuals());
    ctx.set_visuals_of(Theme::Light, light_visuals());
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.5)),
            (TextStyle::Body, FontId::proportional(14.0)),
            (TextStyle::Button, FontId::proportional(14.0)),
            (TextStyle::Heading, FontId::proportional(20.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        let spacing = &mut style.spacing;
        spacing.item_spacing = vec2(SPACE_M, 6.0);
        spacing.button_padding = vec2(SPACE_M, SPACE_S);
        spacing.interact_size = vec2(40.0, 22.0);
        spacing.indent = SPACE_XL;
        spacing.menu_margin = Margin::same(6);
        spacing.window_margin = Margin::same(SPACE_XL as i8);
        spacing.icon_width = 16.0;
        spacing.icon_spacing = 6.0;
        spacing.slider_width = 120.0;
        spacing.scroll.bar_width = 8.0;
    });
    set_choice(ctx, choice);
}

pub fn set_choice(ctx: &egui::Context, choice: ThemeChoice) {
    ctx.set_theme(match choice {
        ThemeChoice::System => ThemePreference::System,
        ThemeChoice::Dark => ThemePreference::Dark,
        ThemeChoice::Light => ThemePreference::Light,
    });
}

/// The neutral surfaces and text colours one theme is built from.
struct Base {
    /// Deepest level: canvases and text fields.
    sunken: Color32,
    /// Panels.
    panel: Color32,
    /// Buttons and other controls at rest.
    control: Color32,
    control_hover: Color32,
    control_active: Color32,
    border: Color32,
    border_strong: Color32,
    text: Color32,
    text_strong: Color32,
    selection_bg: Color32,
    selection_fg: Color32,
    warning: Color32,
    error: Color32,
}

fn dark_visuals() -> Visuals {
    let tokens = tokens(true);
    build(
        Visuals::dark(),
        &tokens,
        &Base {
            sunken: Color32::from_rgb(20, 21, 24),
            panel: Color32::from_rgb(28, 29, 33),
            control: Color32::from_rgb(46, 48, 55),
            control_hover: Color32::from_rgb(58, 60, 69),
            control_active: Color32::from_rgb(70, 73, 84),
            border: tokens.border,
            border_strong: Color32::from_rgb(92, 96, 108),
            text: Color32::from_rgb(200, 203, 211),
            text_strong: Color32::from_rgb(240, 241, 245),
            selection_bg: Color32::from_rgb(94, 56, 14),
            selection_fg: Color32::from_rgb(255, 196, 128),
            warning: Color32::from_rgb(240, 176, 64),
            error: Color32::from_rgb(240, 104, 104),
        },
    )
}

fn light_visuals() -> Visuals {
    let tokens = tokens(false);
    build(
        Visuals::light(),
        &tokens,
        &Base {
            sunken: Color32::from_rgb(255, 255, 255),
            panel: Color32::from_rgb(246, 247, 249),
            control: Color32::from_rgb(228, 230, 235),
            control_hover: Color32::from_rgb(216, 219, 226),
            control_active: Color32::from_rgb(202, 206, 215),
            border: tokens.border,
            border_strong: Color32::from_rgb(150, 154, 166),
            text: Color32::from_rgb(52, 55, 63),
            text_strong: Color32::from_rgb(16, 17, 20),
            selection_bg: Color32::from_rgb(255, 219, 178),
            selection_fg: Color32::from_rgb(120, 58, 0),
            warning: Color32::from_rgb(166, 100, 0),
            error: Color32::from_rgb(196, 44, 44),
        },
    )
}

fn build(mut v: Visuals, tokens: &Tokens, base: &Base) -> Visuals {
    let radius = CornerRadius::same(RADIUS);
    v.panel_fill = base.panel;
    v.window_fill = base.panel;
    v.window_stroke = Stroke::new(1.0, base.border);
    v.window_corner_radius = CornerRadius::same(RADIUS_L + 2);
    v.menu_corner_radius = CornerRadius::same(RADIUS_L);
    v.extreme_bg_color = base.sunken;
    v.text_edit_bg_color = Some(base.sunken);
    v.faint_bg_color = base.control.gamma_multiply(0.5);
    v.code_bg_color = base.control;
    v.hyperlink_color = tokens.accent;
    v.warn_fg_color = base.warning;
    v.error_fg_color = base.error;
    v.selection.bg_fill = base.selection_bg;
    v.selection.stroke = Stroke::new(1.0, base.selection_fg);
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;
    v.slider_trailing_fill = true;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = base.panel;
    w.noninteractive.weak_bg_fill = base.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, base.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, base.text);
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = base.control;
    w.inactive.weak_bg_fill = base.control;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, base.text);
    w.inactive.corner_radius = radius;

    w.hovered.bg_fill = base.control_hover;
    w.hovered.weak_bg_fill = base.control_hover;
    w.hovered.bg_stroke = Stroke::new(1.0, base.border_strong);
    w.hovered.fg_stroke = Stroke::new(1.0, base.text_strong);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = HOVER_EXPANSION;

    w.active.bg_fill = base.control_active;
    w.active.weak_bg_fill = base.control_active;
    w.active.bg_stroke = Stroke::new(1.5, tokens.accent);
    w.active.fg_stroke = Stroke::new(1.0, base.text_strong);
    w.active.corner_radius = radius;
    w.active.expansion = PRESS_EXPANSION;

    w.open.bg_fill = base.control_hover;
    w.open.weak_bg_fill = base.control_hover;
    w.open.bg_stroke = Stroke::new(1.0, base.border_strong);
    w.open.fg_stroke = Stroke::new(1.0, base.text_strong);
    w.open.corner_radius = radius;
    v
}

/// Tab strip styling that matches the theme (egui_dock's defaults ignore most of it).
pub fn dock_style(ui: &egui::Ui) -> egui_dock::Style {
    let visuals = ui.visuals();
    let tokens = of(ui);
    let mut style = egui_dock::Style::from_egui(ui.style().as_ref());
    style.main_surface_border_stroke = Stroke::NONE;
    style.dock_area_padding = None;
    style.separator.width = 1.0;
    style.separator.color_idle = tokens.border;
    style.separator.color_hovered = tokens.accent;
    style.separator.color_dragged = tokens.accent;

    style.tab_bar.bg_fill = visuals.panel_fill;
    style.tab_bar.height = 30.0;
    style.tab_bar.hline_color = tokens.border;
    style.tab_bar.corner_radius = CornerRadius::ZERO;

    let body = visuals.extreme_bg_color;
    style.tab.tab_body.bg_fill = body;
    style.tab.tab_body.inner_margin = Margin::ZERO;
    style.tab.tab_body.stroke = Stroke::NONE;
    style.tab.tab_body.corner_radius = CornerRadius::ZERO;
    style.tab.hline_below_active_tab_name = false;
    style.tab.spacing = 0.0;

    let top = CornerRadius {
        nw: RADIUS,
        ne: RADIUS,
        sw: 0,
        se: 0,
    };
    let strong = visuals.strong_text_color();
    let weak = visuals.text_color();
    for (tab, fill, text, outline) in [
        (&mut style.tab.active, tokens.toolbar, strong, tokens.border),
        (
            &mut style.tab.focused,
            tokens.toolbar,
            strong,
            tokens.accent,
        ),
        (
            &mut style.tab.inactive,
            visuals.panel_fill,
            weak,
            Color32::TRANSPARENT,
        ),
        (
            &mut style.tab.hovered,
            visuals.widgets.hovered.bg_fill,
            strong,
            Color32::TRANSPARENT,
        ),
    ] {
        tab.bg_fill = fill;
        tab.text_color = text;
        tab.outline_color = outline;
        tab.corner_radius = top;
    }
    style.tab.active_with_kb_focus = style.tab.active.clone();
    style.tab.focused_with_kb_focus = style.tab.focused.clone();
    style.tab.inactive_with_kb_focus = style.tab.inactive.clone();

    style.buttons.close_tab_bg_fill = visuals.widgets.hovered.bg_fill;
    style.buttons.close_tab_color = weak;
    style.buttons.close_tab_active_color = strong;
    style.overlay.selection_color = tokens.accent.gamma_multiply(0.35);
    style
}

/// A frame for toolbars and cards that float over a canvas or viewport.
pub fn floating_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(ui.visuals().panel_fill.gamma_multiply(0.94))
        .stroke(Stroke::new(1.0, of(ui).border))
        .corner_radius(CornerRadius::same(RADIUS_L))
        .inner_margin(Margin::same(SPACE_S as i8))
}
