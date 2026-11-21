//! Small building blocks shared by panels, toolbars and dialogs.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Layout, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, TextureHandle, vec2,
};
use openpax_paint::Rgba;

use crate::theme::{self, ICON_BUTTON, ICON_SIZE, RADIUS, SPACE_M, SPACE_S};

pub fn color32(color: Rgba) -> Color32 {
    let [r, g, b, a] = color;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn hex(color: Rgba) -> String {
    let [r, g, b, a] = color;
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// An icon glyph as rich text at the standard icon size.
pub fn icon(glyph: &str) -> RichText {
    RichText::new(glyph).size(ICON_SIZE)
}

/// "icon  label" for buttons and menu items.
pub fn icon_label(glyph: &str, label: &str) -> String {
    format!("{glyph}  {label}")
}

/// A square, frameless-until-hovered button showing one icon.
pub fn icon_button(ui: &mut egui::Ui, glyph: &str, tooltip: &str) -> Response {
    ui.add(
        egui::Button::new(icon(glyph))
            .frame_when_inactive(false)
            .min_size(vec2(ICON_BUTTON, ICON_BUTTON)),
    )
    .on_hover_text(tooltip)
}

/// Like [`icon_button`], greyed out when `enabled` is false.
pub fn icon_button_enabled(
    ui: &mut egui::Ui,
    enabled: bool,
    glyph: &str,
    tooltip: &str,
) -> Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(icon(glyph))
            .frame_when_inactive(false)
            .min_size(vec2(ICON_BUTTON, ICON_BUTTON)),
    )
    .on_hover_text(tooltip)
    .on_disabled_hover_text(tooltip)
}

/// An icon button that stays highlighted while `selected`.
pub fn icon_toggle(ui: &mut egui::Ui, selected: bool, glyph: &str, tooltip: &str) -> Response {
    let tokens = theme::of(ui);
    let text = if selected {
        icon(glyph).color(tokens.accent)
    } else {
        icon(glyph)
    };
    ui.add(
        egui::Button::new(text)
            .selected(selected)
            .frame_when_inactive(selected)
            .min_size(vec2(ICON_BUTTON, ICON_BUTTON)),
    )
    .on_hover_text(tooltip)
}

/// A collapsible inspector section with an icon in its header.
pub fn section(
    ui: &mut egui::Ui,
    glyph: &str,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let header = RichText::new(icon_label(glyph, title)).strong();
    egui::CollapsingHeader::new(header)
        .id_salt(title)
        .default_open(true)
        .show(ui, |ui| {
            ui.add_space(SPACE_S);
            add_contents(ui);
            ui.add_space(SPACE_M);
        });
    ui.separator();
}

/// A dim caption above a group of controls.
pub fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().weak());
}

/// Width of the label column of [`field`].
const FIELD_LABEL_WIDTH: f32 = 60.0;

/// One "label: control" line with the label in a fixed-width column.
pub fn field<R>(ui: &mut egui::Ui, label: &str, add_control: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            vec2(FIELD_LABEL_WIDTH, ui.spacing().interact_size.y),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(FIELD_LABEL_WIDTH);
                ui.label(label);
            },
        );
        add_control(ui)
    })
    .inner
}

/// Three drag values labelled X, Y and Z in the axis colours. Returns true when changed.
pub fn vec3(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut [f32; 3],
    range: RangeInclusive<f32>,
) -> bool {
    let axis = theme::of(ui).axis;
    let mut changed = false;
    field(ui, label, |ui| {
        ui.spacing_mut().item_spacing.x = SPACE_S;
        let width = ((ui.available_width() - 2.0 * SPACE_S) / 3.0).clamp(36.0, 72.0);
        for (i, component) in value.iter_mut().enumerate() {
            let drag = egui::DragValue::new(component)
                .speed(0.1)
                .range(range.clone())
                .max_decimals(3)
                .prefix(["X ", "Y ", "Z "][i]);
            let response = ui.add_sized([width, ui.spacing().interact_size.y], drag);
            // A coloured underline ties each field to its axis in the viewport.
            let line = response.rect.shrink2(vec2(3.0, 0.0));
            ui.painter().hline(
                line.x_range(),
                line.bottom() - 1.0,
                Stroke::new(1.5, axis[i].gamma_multiply(0.8)),
            );
            changed |= response.changed();
        }
    });
    changed
}

/// A colour swatch over a transparency checkerboard. `checker` is the shared 2×2 texture.
pub fn swatch(
    ui: &mut egui::Ui,
    color: Rgba,
    size: f32,
    selected: bool,
    checker: &TextureHandle,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    paint_swatch(ui, rect, color, selected, checker);
    if response.hovered() {
        ui.painter().rect_stroke(
            rect,
            RADIUS,
            Stroke::new(1.0, ui.visuals().strong_text_color()),
            StrokeKind::Inside,
        );
    }
    response
}

pub fn paint_swatch(
    ui: &egui::Ui,
    rect: Rect,
    color: Rgba,
    selected: bool,
    checker: &TextureHandle,
) {
    let tokens = theme::of(ui);
    let painter = ui.painter();
    let radius = CornerRadius::same(RADIUS);
    if color[3] < 255 {
        let cells = rect.size() / 6.0;
        let uv = Rect::from_min_size(egui::Pos2::ZERO, cells / 2.0);
        painter.add(
            egui::epaint::RectShape::filled(rect, radius, tokens.checker)
                .with_texture(checker.id(), uv),
        );
    }
    painter.rect_filled(rect, radius, color32(color));
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, tokens.border),
        StrokeKind::Inside,
    );
    if selected {
        painter.rect_stroke(
            rect.expand(1.0),
            radius,
            Stroke::new(2.0, tokens.accent),
            StrokeKind::Outside,
        );
    }
}

/// Allocates a full-width list row and paints its hover or selection background.
/// Add the row's contents with [`row_ui`].
pub fn list_row(ui: &mut egui::Ui, selected: bool) -> (Rect, Response) {
    let size = vec2(ui.available_width(), theme::ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if selected {
        Some(ui.visuals().selection.bg_fill)
    } else if response.contains_pointer() {
        Some(
            ui.visuals()
                .widgets
                .hovered
                .weak_bg_fill
                .gamma_multiply(0.6),
        )
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter()
            .rect_filled(rect.shrink2(vec2(2.0, 0.5)), RADIUS, fill);
    }
    (rect, response)
}

/// A left-to-right child `Ui` covering a row from [`list_row`], indented by `indent`.
pub fn row_ui(ui: &mut egui::Ui, rect: Rect, indent: f32) -> egui::Ui {
    let mut inner = rect.shrink2(vec2(SPACE_S, 0.0));
    inner.min.x += indent;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 6.0;
    child
}

/// Text colour for a list row.
pub fn row_text_color(ui: &egui::Ui, selected: bool) -> Color32 {
    if selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    }
}

/// A centred icon, title and hint for panels with nothing to show.
pub fn empty_state(ui: &mut egui::Ui, glyph: &str, title: &str, hint: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.35).max(SPACE_M));
        ui.label(RichText::new(glyph).size(40.0).weak());
        ui.add_space(SPACE_S);
        ui.label(RichText::new(title).strong());
        ui.label(RichText::new(hint).weak());
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Right,
}

/// Fills `rect` with the toolbar colour and draws a hairline on the given edge.
pub fn toolbar_background(ui: &egui::Ui, rect: Rect, line: Edge) {
    let tokens = theme::of(ui);
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, tokens.toolbar);
    let stroke = Stroke::new(1.0, tokens.border);
    match line {
        Edge::Top => painter.hline(rect.x_range(), rect.top() + 0.5, stroke),
        Edge::Bottom => painter.hline(rect.x_range(), rect.bottom() - 0.5, stroke),
        Edge::Right => painter.vline(rect.right() - 0.5, rect.y_range(), stroke),
    };
}

/// The primary (accent-coloured) button of a dialog or card.
pub fn primary_button(ui: &mut egui::Ui, enabled: bool, text: &str) -> Response {
    let tokens = theme::of(ui);
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(tokens.on_accent).strong())
            .fill(tokens.accent)
            .min_size(vec2(88.0, 28.0)),
    )
}

pub fn secondary_button(ui: &mut egui::Ui, text: &str) -> Response {
    ui.add(egui::Button::new(text).min_size(vec2(72.0, 28.0)))
}

/// Shortens a filesystem path for display by replacing the middle with an ellipsis.
pub fn shorten_path(path: &str, max_chars: usize) -> String {
    let count = path.chars().count();
    if count <= max_chars || max_chars < 8 {
        return path.to_owned();
    }
    let tail_len = max_chars * 2 / 3;
    let head_len = max_chars - tail_len - 1;
    let head: String = path.chars().take(head_len).collect();
    let tail: String = path.chars().skip(count - tail_len).collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_drops_opaque_alpha() {
        assert_eq!(hex([255, 0, 16, 255]), "#ff0010");
        assert_eq!(hex([255, 0, 16, 128]), "#ff001080");
    }

    #[test]
    fn long_paths_are_shortened_in_the_middle() {
        assert_eq!(shorten_path("C:/packs/a", 40), "C:/packs/a");
        let long = "C:/Users/someone/Documents/minecraft/resourcepacks/my_pack";
        let short = shorten_path(long, 30);
        assert_eq!(short.chars().count(), 30);
        assert!(short.starts_with("C:/Users/") && short.ends_with("my_pack"));
        assert!(short.contains('…'));
    }
}
