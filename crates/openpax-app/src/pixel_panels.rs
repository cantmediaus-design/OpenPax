//! Inspector sections of the texture editor.

use eframe::egui::{
    self, Color32, Mesh, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, pos2, vec2,
};
use openpax_core::Animation;
use openpax_paint::color::{hsl_to_rgb, hsv_to_rgb, parse_hex, rgb_to_hsl, rgb_to_hsv, to_hex};
use openpax_paint::effects::Effect;
use openpax_paint::tiles::{AutotileMode, Collision, Terrain};
use openpax_paint::{BlendMode, LayerKind, Palette, Rgba, SelectMode};

use crate::app::{App, texture_mut};
use crate::commands::Command;
use crate::pixel_cmds::PixelCmd;
use crate::pixel_dialogs::{PixelDialog, color_button, effect_ui};
use crate::texture_editor::{Background, TICK_MS, Tiling, ViewPrefs, rgba32};
use crate::theme::{self, SPACE_M, SPACE_S};
use crate::{icons, widgets};

const SWATCH: f32 = 20.0;
const LAYER_THUMB: f32 = 18.0;
const TAG_COLORS: [Rgba; 7] = [
    [230, 90, 90, 255],
    [240, 160, 60, 255],
    [230, 210, 70, 255],
    [110, 200, 110, 255],
    [90, 170, 230, 255],
    [180, 110, 230, 255],
    [150, 150, 150, 255],
];

impl App {
    pub(crate) fn texture_sections(&mut self, ui: &mut egui::Ui) {
        widgets::section(ui, icons::PICKER, "Colour", |ui| self.color_section(ui));
        widgets::section(ui, icons::PALETTE, "Palette", |ui| self.palette_section(ui));
        widgets::section(ui, icons::LAYERS, "Layers", |ui| self.layers_section(ui));
        widgets::section(ui, icons::SLIDERS, "Layer Effects", |ui| {
            self.effects_section(ui)
        });
        widgets::section(ui, icons::ANIMATION, "Animation", |ui| {
            self.animation_section(ui)
        });
        widgets::section(ui, icons::BRUSH, "Brushes", |ui| self.brushes_section(ui));
        widgets::section(ui, icons::DITHER, "Dither Patterns", |ui| {
            self.patterns_section(ui);
        });
        widgets::section(ui, icons::STAMP, "Tiles", |ui| self.tiles_section(ui));
        widgets::section(ui, icons::CROSSHAIR, "Origin and Guides", |ui| {
            self.origin_section(ui)
        });
        widgets::section(ui, icons::WARNING_CIRCLE, "Checks", |ui| {
            self.checks_section(ui)
        });
        widgets::section(ui, icons::HISTORY, "History", |ui| self.history_section(ui));
        widgets::section(ui, icons::IMAGE, "Texture", |ui| {
            self.texture_info_section(ui)
        });
    }

    // ---- colour ----

    fn color_section(&mut self, ui: &mut egui::Ui) {
        let id = ui.scope_id().with("editing_secondary");
        let mut secondary: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
        let tools = &mut self.tools;
        ui.horizontal(|ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
            let back = Rect::from_min_size(rect.min + vec2(14.0, 14.0), vec2(26.0, 26.0));
            let front = Rect::from_min_size(rect.min, vec2(26.0, 26.0));
            widgets::paint_swatch(ui, back, tools.secondary, secondary, &self.checker);
            widgets::paint_swatch(ui, front, tools.settings.color, !secondary, &self.checker);
            if response.clicked() {
                // Click the front square for primary, the back one for secondary.
                let pos = response.interact_pointer_pos().unwrap_or(rect.min);
                secondary = !front.contains(pos);
            }
            response.on_hover_text("Click a square to edit that colour");
            ui.vertical(|ui| {
                ui.label(if secondary {
                    "Editing secondary"
                } else {
                    "Editing primary"
                });
                if widgets::icon_button(ui, icons::SWAP, &Command::SwapColors.tooltip(ui.ctx()))
                    .clicked()
                {
                    tools.swap_colors();
                }
            });
        });
        ui.data_mut(|d| d.insert_temp(id, secondary));
        let color = if secondary {
            &mut tools.secondary
        } else {
            &mut tools.settings.color
        };
        color_picker(ui, color);

        if !tools.recent.is_empty() {
            ui.add_space(SPACE_S);
            widgets::caption(ui, "RECENT");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                for color in tools.recent.clone() {
                    let response = widgets::swatch(
                        ui,
                        color,
                        SWATCH,
                        color == tools.settings.color,
                        &self.checker,
                    )
                    .on_hover_text(widgets::hex(color));
                    if response.clicked() {
                        tools.settings.color = color;
                    }
                    if response.secondary_clicked() {
                        tools.secondary = color;
                    }
                }
            });
        }
    }

    // ---- palette ----

    fn palette_section(&mut self, ui: &mut egui::Ui) {
        let tools = &mut self.tools;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut tools.palette.name)
                    .desired_width(140.0)
                    .hint_text("Palette name"),
            );
            ui.weak(format!("{} colours", tools.palette.colors.len()));
        });
        let mut rects: Vec<Rect> = Vec::with_capacity(tools.palette.colors.len());
        let (mut dragged, mut dropped, mut remove, mut insert, mut group_here, mut ramp_from) =
            (None, None, None, None, None, None);
        let mut edit_color: Option<(usize, Rgba)> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
            for (index, color) in tools.palette.colors.clone().into_iter().enumerate() {
                if let Some(label) = tools.palette.group_at(index) {
                    ui.end_row();
                    widgets::caption(ui, &label.to_uppercase());
                    ui.end_row();
                }
                let response = widgets::swatch(ui, color, SWATCH, color == tools.settings.color, &self.checker);
                rects.push(response.rect);
                if response.clicked() {
                    tools.settings.color = color;
                }
                if response.secondary_clicked() && ui.input(|i| i.modifiers.alt) {
                    tools.secondary = color;
                }
                if response.dragged() {
                    dragged = Some(index);
                }
                if response.drag_stopped() {
                    dropped = Some(index);
                }
                response.context_menu(|ui| {
                    let mut c = color;
                    ui.horizontal(|ui| {
                        ui.label("Edit");
                        if color_button(ui, &mut c) {
                            edit_color = Some((index, c));
                        }
                    });
                    if ui.button("Set as Primary").clicked() {
                        tools.settings.color = color;
                        ui.close();
                    }
                    if ui.button("Set as Secondary").clicked() {
                        tools.secondary = color;
                        ui.close();
                    }
                    if ui.button("Insert the primary colour before").clicked() {
                        insert = Some(index);
                        ui.close();
                    }
                    if ui.button("Start a group here").clicked() {
                        group_here = Some(index);
                        ui.close();
                    }
                    if ui.button("Shade ramp from here").on_hover_text("The shade tool steps through the colours from here to the end of this group").clicked() {
                        ramp_from = Some(index);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(widgets::icon_label(icons::TRASH, "Remove from Palette")).clicked() {
                        remove = Some(index);
                        ui.close();
                    }
                });
                response.on_hover_text(format!("{}\nRight-click for more, drag to reorder", widgets::hex(color)));
            }
        });
        // Dragging a swatch onto another moves it there.
        let pointer = ui.ctx().pointer_latest_pos();
        let target = pointer.and_then(|pos| rects.iter().position(|r| r.contains(pos)));
        if let (Some(from), Some(to)) = (dragged, target)
            && from != to
        {
            ui.painter().rect_stroke(
                rects[to].expand(1.5),
                theme::RADIUS,
                Stroke::new(2.0, theme::of(ui).accent),
                StrokeKind::Outside,
            );
        }
        if let (Some(from), Some(to)) = (dropped, target)
            && from != to
        {
            let color = tools.palette.colors.remove(from);
            tools.palette.colors.insert(to, color);
        }
        if let Some((i, c)) = edit_color {
            let old = tools.palette.colors[i];
            tools.palette.colors[i] = c;
            // In indexed mode, recolouring a palette entry recolours the image.
            if let Some(e) = texture_mut(&mut self.editors, self.active)
                && e.doc.extras.indexed
            {
                e.doc
                    .apply_filter("Recolour palette entry", openpax_paint::Scope::All, |b| {
                        let mut b = b.clone();
                        openpax_paint::effects::replace_color(&mut b, old, c, 0);
                        b
                    });
                e.doc.edit_extras("Palette entry", |x| {
                    for p in &mut x.palette {
                        if *p == old {
                            *p = c;
                        }
                    }
                });
            }
        }
        if let Some(i) = insert {
            tools.palette.insert(i, tools.settings.color);
        }
        if let Some(i) = group_here {
            let n = tools.palette.groups.len() + 1;
            tools.palette.groups.retain(|(s, _)| *s != i);
            tools.palette.groups.push((i, format!("Group {n}")));
            tools.palette.groups.sort_by_key(|(s, _)| *s);
        }
        if let Some(i) = ramp_from {
            let end = tools
                .palette
                .groups
                .iter()
                .map(|(s, _)| *s)
                .find(|s| *s > i)
                .unwrap_or(tools.palette.colors.len());
            tools.settings.ramp = tools.palette.colors[i..end].to_vec();
            tools.tool = openpax_paint::ToolKind::Shade;
        }
        if let Some(i) = remove {
            tools.palette.remove(i);
        }
        if !tools.palette.groups.is_empty() {
            ui.collapsing("Group names", |ui| {
                let mut drop_group = None;
                for (gi, (start, label)) in tools.palette.groups.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.weak(format!("#{start}"));
                        ui.text_edit_singleline(label);
                        if widgets::icon_button(
                            ui,
                            icons::TRASH,
                            "Remove the group (keeps its colours)",
                        )
                        .clicked()
                        {
                            drop_group = Some(gi);
                        }
                    });
                }
                if let Some(gi) = drop_group {
                    tools.palette.groups.remove(gi);
                }
            });
        }

        ui.add_space(SPACE_S);
        let editor = texture_mut(&mut self.editors, self.active);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if widgets::icon_button(ui, icons::PLUS, "Add the primary colour").clicked() {
                tools.palette.add(tools.settings.color);
            }
            if widgets::icon_button(ui, icons::IMAGE, "Build the palette from this texture")
                .clicked()
                && let Some(editor) = &editor
            {
                tools.palette = Palette::from_bitmap(&editor.doc.flatten(), 256);
            }
            if widgets::icon_button(ui, icons::SHADE, "Sort by hue").clicked() {
                tools.palette.sort_by_hue();
            }
            if widgets::icon_button(ui, icons::CONTRAST, "Sort by brightness").clicked() {
                tools.palette.sort_by_luma();
            }
            if widgets::icon_button(ui, icons::REFRESH, "Reset to the default palette").clicked() {
                tools.palette = Palette::default();
            }
        });
        let mut run = None;
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Import…").clicked() {
                run = Some(PixelCmd::ImportPalette);
            }
            if ui.small_button("Export…").clicked() {
                run = Some(PixelCmd::ExportPalette);
            }
            if ui
                .small_button("Swap…")
                .on_hover_text("Recolour the texture by palette entries")
                .clicked()
            {
                run = Some(PixelCmd::PaletteSwap);
            }
            if ui
                .small_button("Remap…")
                .on_hover_text("Reduce the texture to these colours")
                .clicked()
            {
                run = Some(PixelCmd::ReduceColors);
            }
        });
        if let Some(editor) = texture_mut(&mut self.editors, self.active) {
            let mut indexed = editor.doc.extras.indexed;
            if ui
                .checkbox(&mut indexed, "Indexed colour mode")
                .on_hover_text("Painting snaps to the document palette, and editing a palette swatch recolours the image")
                .changed()
            {
                let colors = self.tools.palette.colors.clone();
                editor.doc.edit_extras("Indexed colour mode", |e| {
                    e.indexed = indexed;
                    e.palette = colors;
                });
            }
            if editor.doc.extras.indexed
                && ui
                    .small_button("Use this palette for the document")
                    .clicked()
            {
                crate::pixel_dialogs::use_as_document_palette(&mut editor.doc, &self.tools.palette);
            }
        }
        if let Some(c) = run {
            let ctx = ui.ctx().clone();
            self.run(Command::Pixel(c), &ctx);
        }
    }

    // ---- layers ----

    fn layers_section(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut run = None;
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        if ui.ctx().input(|i| i.pointer.any_pressed()) {
            editor.doc.end_coalescing();
        }
        let thumbs: Vec<egui::TextureId> = editor
            .layer_textures(ui.ctx())
            .iter()
            .map(TextureHandle::id)
            .collect();
        let aspect = editor.doc.height() as f32 / editor.doc.width().max(1) as f32;
        let doc = &mut editor.doc;
        let count = doc.layers().len();

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if widgets::icon_button(ui, icons::PLUS, "Add a layer above").clicked() {
                doc.add_layer();
            }
            if widgets::icon_button(ui, icons::FOLDER_PLUS, "Add a group").clicked() {
                doc.add_group();
            }
            if widgets::icon_button(ui, icons::COPY, "Duplicate the layer").clicked() {
                doc.duplicate_layer();
            }
            if widgets::icon_button(ui, icons::ARROW_UP, "Move up").clicked() {
                doc.move_layer(true);
            }
            if widgets::icon_button(ui, icons::ARROW_DOWN, "Move down").clicked() {
                doc.move_layer(false);
            }
            if widgets::icon_button(ui, icons::MERGE_DOWN, "Merge into the layer below").clicked() {
                doc.merge_down();
            }
            if widgets::icon_button(ui, icons::IMAGES, "Add a reference image").clicked() {
                run = Some(PixelCmd::AddReference);
            }
            if widgets::icon_button_enabled(ui, count > 1, icons::TRASH, "Delete the layer")
                .clicked()
            {
                doc.remove_layer();
            }
        });
        ui.add_space(SPACE_S);

        let tree = doc.layer_tree(true);
        let mut rects: Vec<(usize, Rect)> = Vec::with_capacity(tree.len());
        let (mut dragged, mut dropped) = (None, None);
        let checker = self.checker.id();
        let checker_tint = theme::of(ui).checker;
        let mut action: Option<(usize, LayerAction)> = None;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for &index in &tree {
                let layer = doc.layers()[index].clone();
                let selected = index == doc.active();
                let depth = doc.layer_depth(index) as f32;
                let (rect, response) = widgets::list_row(ui, selected);
                rects.push((index, rect));
                if let Some(tag) = layer.color_tag {
                    ui.painter().rect_filled(
                        Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
                        0.0,
                        rgba32(tag),
                    );
                }
                let mut row = widgets::row_ui(ui, rect, depth * 12.0);
                let handle = row
                    .add(
                        egui::Label::new(RichText::new(icons::DRAG_HANDLE).weak())
                            .selectable(false)
                            .sense(Sense::drag()),
                    )
                    .on_hover_cursor(egui::CursorIcon::Grab);
                if handle.dragged() {
                    dragged = Some(index);
                }
                if handle.drag_stopped() {
                    dropped = Some(index);
                }
                if layer.kind == LayerKind::Group {
                    let caret = if layer.collapsed {
                        icons::CARET_RIGHT
                    } else {
                        icons::CARET_DOWN
                    };
                    if row.add(egui::Button::new(caret).frame(false)).clicked() {
                        action = Some((index, LayerAction::ToggleCollapsed));
                    }
                }
                let eye = if layer.visible {
                    RichText::new(icons::EYE)
                } else {
                    RichText::new(icons::EYE_OFF).weak()
                };
                if row
                    .add(egui::Button::new(eye).frame(false))
                    .on_hover_text(if layer.visible {
                        "Hide layer"
                    } else {
                        "Show layer"
                    })
                    .clicked()
                {
                    action = Some((index, LayerAction::Visible(!layer.visible)));
                }
                let lock = if layer.locked {
                    RichText::new(icons::LOCK)
                } else {
                    RichText::new(icons::UNLOCK).weak()
                };
                if row
                    .add(egui::Button::new(lock).frame(false))
                    .on_hover_text(if layer.locked {
                        "Unlock"
                    } else {
                        "Lock against edits"
                    })
                    .clicked()
                {
                    action = Some((index, LayerAction::Lock(!layer.locked)));
                }
                if layer.kind != LayerKind::Group {
                    let size = if aspect >= 1.0 {
                        vec2(LAYER_THUMB / aspect.min(4.0), LAYER_THUMB)
                    } else {
                        vec2(LAYER_THUMB, LAYER_THUMB * aspect.max(0.25))
                    };
                    let (thumb, _) = row.allocate_exact_size(size, Sense::hover());
                    row.painter().image(
                        checker,
                        thumb,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(2.0, 2.0 * aspect)),
                        checker_tint,
                    );
                    if let Some(texture) = thumbs.get(index) {
                        row.painter().image(
                            *texture,
                            thumb,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                }
                let color = widgets::row_text_color(ui, selected);
                match &mut editor.renaming {
                    Some((renaming, text, focused)) if *renaming == index => {
                        let field =
                            row.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                        if !std::mem::replace(focused, true) {
                            field.request_focus();
                        }
                        if field.lost_focus() {
                            if !row.input(|i| i.key_pressed(egui::Key::Escape))
                                && !text.trim().is_empty()
                            {
                                action = Some((index, LayerAction::Rename(text.trim().to_owned())));
                            }
                            editor.renaming = None;
                        }
                    }
                    _ => {
                        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let mut badges = Vec::new();
                            if layer.opacity < 0.995 {
                                badges.push(format!("{:.0}%", layer.opacity * 100.0));
                            }
                            if layer.blend != BlendMode::Normal {
                                badges.push(layer.blend.name().to_owned());
                            }
                            if layer.alpha_lock {
                                badges.push("α".into());
                            }
                            if layer.clip {
                                badges.push("↳".into());
                            }
                            if !layer.effects.is_empty() {
                                badges.push("fx".into());
                            }
                            match layer.kind {
                                LayerKind::Reference => badges.push("ref".into()),
                                LayerKind::Tilemap => badges.push("tiles".into()),
                                _ => {}
                            }
                            if !badges.is_empty() {
                                ui.add_space(SPACE_S);
                                ui.label(RichText::new(badges.join(" ")).small().weak());
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(RichText::new(&layer.name).color(color))
                                            .selectable(false)
                                            .truncate(),
                                    );
                                },
                            );
                        });
                    }
                }
                if response.clicked() {
                    action = Some((index, LayerAction::Select));
                }
                if response.double_clicked() {
                    editor.renaming = Some((index, layer.name.clone(), false));
                }
                let groups: Vec<(u32, String)> = doc
                    .layers()
                    .iter()
                    .filter(|l| l.kind == LayerKind::Group && l.id != layer.id)
                    .map(|l| (l.id, l.name.clone()))
                    .collect();
                response
                    .on_hover_text("Double-click to rename, right-click for more")
                    .context_menu(|ui| {
                        if ui.button("Select its pixels").clicked() {
                            action = Some((index, LayerAction::SelectAlpha));
                            ui.close();
                        }
                        if ui
                            .checkbox(&mut layer.alpha_lock.clone(), "Lock transparency")
                            .clicked()
                        {
                            action = Some((index, LayerAction::AlphaLock(!layer.alpha_lock)));
                            ui.close();
                        }
                        if ui
                            .checkbox(&mut layer.clip.clone(), "Clip to layer below")
                            .clicked()
                        {
                            action = Some((index, LayerAction::Clip(!layer.clip)));
                            ui.close();
                        }
                        ui.menu_button("Colour label", |ui| {
                            ui.horizontal(|ui| {
                                for c in TAG_COLORS {
                                    let (r, resp) =
                                        ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
                                    ui.painter().rect_filled(r, 3.0, rgba32(c));
                                    if resp.clicked() {
                                        action = Some((index, LayerAction::Tag(Some(c))));
                                        ui.close();
                                    }
                                }
                            });
                            if ui.button("None").clicked() {
                                action = Some((index, LayerAction::Tag(None)));
                                ui.close();
                            }
                        });
                        ui.menu_button("Move into group", |ui| {
                            if ui.button("Top level").clicked() {
                                action = Some((index, LayerAction::Parent(None)));
                                ui.close();
                            }
                            for (id, name) in &groups {
                                if ui.button(name).clicked() {
                                    action = Some((index, LayerAction::Parent(Some(*id))));
                                    ui.close();
                                }
                            }
                        });
                        if ui.button("Group this layer").clicked() {
                            action = Some((index, LayerAction::Group));
                            ui.close();
                        }
                        if layer.kind == LayerKind::Group && ui.button("Ungroup").clicked() {
                            action = Some((index, LayerAction::Ungroup));
                            ui.close();
                        }
                    });
            }
        });
        if let Some((index, a)) = action {
            match a {
                LayerAction::Select => doc.set_active(index),
                LayerAction::ToggleCollapsed => {
                    let c = !doc.layers()[index].collapsed;
                    doc.edit_layer(
                        index,
                        if c { "Collapse group" } else { "Expand group" },
                        None,
                        |l| l.collapsed = c,
                    );
                }
                LayerAction::Visible(v) => doc.set_layer_visible(index, v),
                LayerAction::Lock(v) => doc.edit_layer(
                    index,
                    if v { "Lock layer" } else { "Unlock layer" },
                    None,
                    |l| l.locked = v,
                ),
                LayerAction::AlphaLock(v) => {
                    doc.edit_layer(index, "Lock transparency", None, |l| l.alpha_lock = v)
                }
                LayerAction::Clip(v) => doc.edit_layer(index, "Clipping", None, |l| l.clip = v),
                LayerAction::Tag(c) => {
                    doc.edit_layer(index, "Colour label", None, |l| l.color_tag = c)
                }
                LayerAction::Rename(name) => doc.set_layer_name(index, name),
                LayerAction::SelectAlpha => doc.select_layer_alpha(index, SelectMode::Replace),
                LayerAction::Parent(p) => doc.set_layer_parent(index, p),
                LayerAction::Group => {
                    doc.set_active(index);
                    doc.group_active();
                }
                LayerAction::Ungroup => {
                    doc.set_active(index);
                    doc.ungroup();
                }
            }
        }
        // Dragging a handle moves the layer where it is dropped.
        let pointer = ui.ctx().pointer_latest_pos();
        let target = pointer.and_then(|pos| {
            rects
                .iter()
                .min_by(|a, b| {
                    (a.1.center().y - pos.y)
                        .abs()
                        .total_cmp(&(b.1.center().y - pos.y).abs())
                })
                .map(|(i, r)| (*i, *r))
        });
        if let (Some(from), Some((to, rect))) = (dragged, target)
            && from != to
        {
            let y = if to > from { rect.top() } else { rect.bottom() };
            ui.painter()
                .hline(rect.x_range(), y, Stroke::new(2.0, theme::of(ui).accent));
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if let (Some(from), Some((to, _))) = (dropped, target)
            && from != to
        {
            doc.set_active(from);
            if doc.layers()[to].kind == LayerKind::Group && !doc.layers()[to].collapsed {
                let id = doc.layers()[to].id;
                doc.set_layer_parent(from, Some(id));
            } else {
                doc.move_layer_to(to);
            }
        }

        // Properties of the active layer.
        ui.add_space(SPACE_M);
        let active = doc.active();
        let layer = doc.layers()[active].clone();
        let mut percent = layer.opacity * 100.0;
        widgets::field(ui, "Opacity", |ui| {
            if ui
                .add(
                    egui::Slider::new(&mut percent, 0.0..=100.0)
                        .suffix("%")
                        .fixed_decimals(0),
                )
                .changed()
            {
                doc.set_layer_opacity(active, percent / 100.0);
            }
        });
        widgets::field(ui, "Blend", |ui| {
            egui::ComboBox::from_id_salt("blend")
                .selected_text(layer.blend.name())
                .show_ui(ui, |ui| {
                    for mode in BlendMode::ALL {
                        if ui
                            .selectable_label(layer.blend == mode, mode.name())
                            .clicked()
                        {
                            doc.set_layer_blend(active, mode);
                        }
                    }
                });
        });
        ui.horizontal_wrapped(|ui| {
            let mut v = layer.alpha_lock;
            if ui
                .toggle_value(&mut v, "α lock")
                .on_hover_text("Paint only on pixels that are already there")
                .changed()
            {
                doc.edit_layer(active, "Lock transparency", None, |l| l.alpha_lock = v);
            }
            let mut c = layer.clip;
            if ui
                .toggle_value(&mut c, "Clip")
                .on_hover_text("Show only where the layer below has pixels")
                .changed()
            {
                doc.edit_layer(active, "Clipping", None, |l| l.clip = c);
            }
            let mut locked = layer.locked;
            if ui.toggle_value(&mut locked, "Lock").changed() {
                doc.edit_layer(active, "Lock layer", None, |l| l.locked = locked);
            }
        });
        if layer.kind == LayerKind::Reference {
            ui.weak("Reference layer: shown while you work, never saved into the texture.");
        }
        if count > 1 || doc.frame_count() > 1 {
            ui.weak(
                "Layers and frames are kept in the pack's .openpax folder; the PNG stays flat.",
            );
        }
        if let Some(c) = run {
            self.run(Command::Pixel(c), &ctx);
        }
    }

    fn effects_section(&mut self, ui: &mut egui::Ui) {
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        let doc = &mut editor.doc;
        let active = doc.active();
        let mut effects = doc.layers()[active].effects.clone();
        if effects.is_empty() {
            ui.weak("No effects on this layer. Effects change how the layer looks without touching its pixels.");
        }
        let mut changed = false;
        let mut remove = None;
        let mut bake = false;
        for (i, e) in effects.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut e.enabled, e.effect.name()).changed();
                if widgets::icon_button(ui, icons::TRASH, "Remove").clicked() {
                    remove = Some(i);
                }
            });
            ui.indent(("fx", i), |ui| {
                let before = e.effect.clone();
                effect_ui(ui, &mut e.effect);
                changed |= before != e.effect;
            });
        }
        if let Some(i) = remove {
            effects.remove(i);
            changed = true;
        }
        ui.horizontal(|ui| {
            ui.menu_button(widgets::icon_label(icons::PLUS, "Add Effect"), |ui| {
                for preset in Effect::presets() {
                    if ui.button(preset.name()).clicked() {
                        effects.push(openpax_paint::effects::LayerEffect {
                            enabled: true,
                            effect: preset,
                        });
                        changed = true;
                        ui.close();
                    }
                }
            });
            if !effects.is_empty()
                && ui
                    .button("Apply")
                    .on_hover_text("Bake the effects into the pixels")
                    .clicked()
            {
                bake = true;
            }
        });
        if changed {
            doc.edit_layer(active, "Layer effects", Some("effects"), |l| {
                l.effects = effects.clone()
            });
        }
        if bake {
            let fx = doc.layers()[active].effects.clone();
            doc.apply_filter("Apply effects", openpax_paint::Scope::Layer, |b| {
                openpax_paint::effects::apply_all(b, &fx)
            });
            doc.edit_layer(active, "Apply effects", None, |l| l.effects.clear());
        }
    }

    // ---- animation ----

    fn animation_section(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        let frames = editor.doc.frame_count();
        let mut animated = editor.meta.animation.is_some() || frames > 1;
        if ui
            .add_enabled(
                frames <= 1,
                egui::Checkbox::new(&mut animated, "Minecraft animation"),
            )
            .on_hover_text(
                "Stores the animation in a .mcmeta file; the frames are saved as a vertical strip",
            )
            .on_disabled_hover_text("Textures with several frames are always animated")
            .changed()
        {
            editor.meta.animation = animated.then(Animation::default);
        }
        if let Some(anim) = &mut editor.meta.animation {
            let mut interpolate = anim.interpolate.unwrap_or(false);
            if ui
                .checkbox(
                    &mut interpolate,
                    "Interpolate (blend between frames in game)",
                )
                .changed()
            {
                anim.interpolate = interpolate.then_some(true);
            }
        }
        if frames > 1 {
            let ticks =
                editor.doc.frames()[editor.doc.current_frame()].duration_ms as f32 / TICK_MS as f32;
            ui.label(format!(
                "{frames} frames · frame {} shows {:.0} ticks",
                editor.doc.current_frame() + 1,
                ticks.round()
            ));
            let mut all =
                (editor.doc.frames()[0].duration_ms as f32 / TICK_MS as f32).round() as u32;
            widgets::field(ui, "All frames", |ui| {
                if ui
                    .add(
                        egui::DragValue::new(&mut all)
                            .range(1..=1200)
                            .suffix(" ticks"),
                    )
                    .on_hover_text("20 ticks are one second")
                    .changed()
                {
                    editor.doc.set_all_durations(all * TICK_MS);
                }
            });
            // Live preview, with interpolation like the game when it is on.
            let interpolate = editor
                .meta
                .animation
                .as_ref()
                .and_then(|a| a.interpolate)
                .unwrap_or(false);
            let total: u32 = editor.doc.frames().iter().map(|f| f.duration_ms).sum();
            let t = (ctx.input(|i| i.time) * 1000.0) as u64 % u64::from(total.max(1));
            let (mut acc, mut frame, mut into) = (0u64, 0usize, 0.0f32);
            for (i, f) in editor.doc.frames().iter().enumerate() {
                let d = u64::from(f.duration_ms);
                if t < acc + d {
                    frame = i;
                    into = (t - acc) as f32 / d as f32;
                    break;
                }
                acc += d;
            }
            let size = vec2(
                96.0,
                96.0 * editor.doc.height() as f32 / editor.doc.width().max(1) as f32,
            );
            let id = editor.frame_texture(&ctx, frame);
            let next = editor.frame_texture(&ctx, (frame + 1) % frames);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter().image(
                self.checker.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(4.0, 4.0)),
                theme::of(ui).checker,
            );
            ui.painter().image(id, rect, uv, Color32::WHITE);
            if interpolate {
                ui.painter().image(
                    next,
                    rect,
                    uv,
                    Color32::from_white_alpha((into * 255.0) as u8),
                );
            }
            widgets::caption(ui, &format!("Frame {} of {frames}", frame + 1));
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        } else {
            ui.weak("Add frames in the timeline under the canvas (Alt+N), or import a GIF.");
        }
    }

    // ---- brushes ----

    fn brushes_section(&mut self, ui: &mut egui::Ui) {
        let mut delete = None;
        let ctx = ui.ctx().clone();
        ui.horizontal_wrapped(|ui| {
            for b in &self.tools.brushes {
                let selected = self.tools.settings.brush == openpax_paint::BrushShape::Custom
                    && self
                        .tools
                        .settings
                        .custom_brush
                        .as_ref()
                        .is_some_and(|c| std::sync::Arc::ptr_eq(c, &b.image));
                let id =
                    egui::Id::unique(("brush_tex", &b.name, b.image.width(), b.image.height()));
                let texture: TextureHandle =
                    ctx.data_mut(|d| d.get_temp(id)).unwrap_or_else(|| {
                        let t = ctx.load_texture(
                            format!("brush:{}", b.name),
                            crate::texture_editor::color_image(&b.image),
                            egui::TextureOptions::NEAREST,
                        );
                        ctx.data_mut(|d| d.insert_temp(id, t.clone()));
                        t
                    });
                let response = ui
                    .add(
                        egui::Button::image(egui::Image::from_texture((
                            texture.id(),
                            vec2(24.0, 24.0),
                        )))
                        .selected(selected),
                    )
                    .on_hover_text(format!(
                        "{} ({}×{})",
                        b.name,
                        b.image.width(),
                        b.image.height()
                    ));
                if response.clicked() {
                    self.tools.settings.brush = openpax_paint::BrushShape::Custom;
                    self.tools.settings.custom_brush = Some(b.image.clone());
                }
                response.context_menu(|ui| {
                    if ui
                        .button(widgets::icon_label(icons::TRASH, "Delete brush"))
                        .clicked()
                    {
                        delete = Some(b.name.clone());
                        ui.close();
                    }
                });
            }
        });
        if self.tools.brushes.is_empty() {
            ui.weak("Select some pixels and save them as a brush shape.");
        }
        if let Some(name) = delete {
            self.tools.delete_brush(&name);
        }
        ui.horizontal(|ui| {
            let has_texture = texture_mut(&mut self.editors, self.active).is_some();
            if ui
                .add_enabled(has_texture, egui::Button::new("Save selection as brush…"))
                .clicked()
            {
                let n = self.tools.brushes.len() + 1;
                self.open_pixel_dialog(PixelDialog::SaveBrush {
                    name: format!("Brush {n}"),
                });
            }
            if ui.button("Import…").clicked()
                && let Some(paths) = rfd::FileDialog::new()
                    .add_filter("PNG", &["png"])
                    .pick_files()
            {
                for p in paths {
                    if let Ok(image) = std::fs::read(&p)
                        .map_err(|e| e.to_string())
                        .and_then(|b| openpax_paint::decode_png(&b).map_err(|e| e.to_string()))
                    {
                        let name = p
                            .file_stem()
                            .map_or("brush".into(), |s| s.to_string_lossy().into_owned());
                        let _ = self.tools.save_brush(&name, image);
                    }
                }
            }
            if ui.button("Folder").clicked() {
                crate::user_files::reveal(&crate::user_files::dir("brushes"));
            }
        });
    }

    // ---- tiles ----

    fn tiles_section(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut open = None;
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        let doc = &mut editor.doc;
        let Some(set) = doc.extras.tileset.clone() else {
            ui.weak("No tileset yet. Make one from this image, or start a tilemap layer.");
            ui.horizontal(|ui| {
                if ui.button("Slice into tiles…").clicked() {
                    open = Some(PixelDialog::SliceTileset {
                        tile_w: 16,
                        tile_h: 16,
                        offset: (0, 0),
                        spacing: (0, 0),
                        skip_empty: true,
                        dedupe: true,
                        from_file: false,
                    });
                }
                if ui.button("New tilemap layer…").clicked() {
                    open = Some(PixelDialog::new_tilemap(doc));
                }
            });
            if let Some(d) = open {
                self.open_pixel_dialog(d);
            }
            return;
        };
        ui.label(format!(
            "{} tiles of {} × {} px",
            set.tiles.len(),
            set.tile_w,
            set.tile_h
        ));
        let revision = doc.revision();
        let key = egui::Id::unique(("tile_textures", &editor.rel_path));
        let cached: Option<(u64, Vec<TextureHandle>)> = ctx.data_mut(|d| d.get_temp(key));
        let textures = match cached {
            Some((r, t)) if r == revision && t.len() == set.tiles.len() => t,
            _ => {
                let t: Vec<TextureHandle> = set
                    .tiles
                    .iter()
                    .enumerate()
                    .map(|(i, tile)| {
                        ctx.load_texture(
                            format!("tile{i}:{}", editor.rel_path),
                            crate::texture_editor::color_image(&tile.image),
                            egui::TextureOptions::NEAREST,
                        )
                    })
                    .collect();
                ctx.data_mut(|d| d.insert_temp(key, (revision, t.clone())));
                t
            }
        };
        let brush = &mut self.tools.tile;
        let shift = ctx.input(|i| i.modifiers.shift);
        let mut edit_props = None;
        let mut delete = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            for (i, t) in textures.iter().enumerate() {
                let selected = brush.tile.id == i || brush.random.contains(&i);
                let response = ui
                    .add(egui::Button::image(egui::Image::from_texture((t.id(), vec2(26.0, 26.0)))).selected(selected))
                    .on_hover_text(format!("Tile {i}\nClick: stamp this tile · Shift-click: add to the random set · right-click: properties"));
                if response.clicked() {
                    if shift {
                        if let Some(p) = brush.random.iter().position(|r| *r == i) {
                            brush.random.remove(p);
                        } else {
                            brush.random.push(i);
                        }
                    } else {
                        brush.tile.id = i;
                        brush.random.clear();
                        brush.terrain = None;
                    }
                    self.tools.tool = openpax_paint::ToolKind::TileStamp;
                }
                response.context_menu(|ui| {
                    if ui.button("Properties…").clicked() {
                        edit_props = Some(i);
                        ui.close();
                    }
                    if ui.button(widgets::icon_label(icons::TRASH, "Delete tile")).clicked() {
                        delete = Some(i);
                        ui.close();
                    }
                });
            }
        });
        let props_id = egui::Id::unique(("tile_props", &editor.rel_path));
        if let Some(i) = edit_props {
            ctx.data_mut(|d| d.insert_temp(props_id, i));
        }
        let editing: Option<usize> = ctx.data(|d| d.get_temp(props_id));
        if let Some(i) = editing.filter(|i| *i < set.tiles.len()) {
            let mut props = set.tiles[i].props.clone();
            let terrains: Vec<String> = set.terrains.iter().map(|t| t.name.clone()).collect();
            let before = props.clone();
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.strong(format!("Tile {i}"));
                    if ui.small_button("Close").clicked() {
                        ctx.data_mut(|d| d.remove::<usize>(props_id));
                    }
                });
                widgets::field(ui, "Collision", |ui| {
                    ui.selectable_value(&mut props.collision, Collision::None, "None");
                    ui.selectable_value(&mut props.collision, Collision::Solid, "Solid");
                    if ui
                        .selectable_label(matches!(props.collision, Collision::Box { .. }), "Box")
                        .clicked()
                    {
                        props.collision = Collision::Box {
                            x: 0,
                            y: 0,
                            w: set.tile_w as i32,
                            h: set.tile_h as i32 / 2,
                        };
                    }
                });
                if let Collision::Box { x, y, w, h } = &mut props.collision {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(x).prefix("x "));
                        ui.add(egui::DragValue::new(y).prefix("y "));
                        ui.add(egui::DragValue::new(w).prefix("w "));
                        ui.add(egui::DragValue::new(h).prefix("h "));
                    });
                }
                widgets::field(ui, "Material", |ui| {
                    ui.text_edit_singleline(&mut props.material)
                });
                widgets::field(ui, "Weight", |ui| {
                    ui.add(
                        egui::DragValue::new(&mut props.weight)
                            .range(0.0..=100.0)
                            .speed(0.05),
                    )
                })
                .on_hover_text("How often random and auto tiling pick this variant");
                widgets::field(ui, "Terrain", |ui| {
                    egui::ComboBox::from_id_salt("tile_terrain")
                        .selected_text(
                            props
                                .terrain
                                .and_then(|t| terrains.get(t).cloned())
                                .unwrap_or_else(|| "None".into()),
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut props.terrain, None, "None");
                            for (ti, name) in terrains.iter().enumerate() {
                                ui.selectable_value(&mut props.terrain, Some(ti), name);
                            }
                        });
                });
                widgets::caption(ui, "CUSTOM PROPERTIES");
                let mut remove_key = None;
                for (k, v) in props.custom.iter_mut() {
                    ui.horizontal(|ui| {
                        ui.monospace(k);
                        ui.text_edit_singleline(v);
                        if widgets::icon_button(ui, icons::TRASH, "Remove").clicked() {
                            remove_key = Some(k.clone());
                        }
                    });
                }
                if let Some(k) = remove_key {
                    props.custom.remove(&k);
                }
                if ui.small_button("Add property").clicked() {
                    let n = props.custom.len() + 1;
                    props.custom.insert(format!("key{n}"), String::new());
                }
            });
            if props != before {
                doc.edit_extras("Tile properties", |e| {
                    if let Some(s) = &mut e.tileset {
                        s.tiles[i].props = props;
                    }
                });
            }
        }
        if let Some(i) = delete {
            doc.edit_extras("Delete tile", |e| {
                if let Some(s) = &mut e.tileset {
                    s.tiles.remove(i);
                }
            });
        }
        // Terrains for autotiling.
        ui.add_space(SPACE_S);
        widgets::caption(ui, "TERRAINS (AUTOTILING)");
        let mut terrains = set.terrains.clone();
        let mut terrain_changed = false;
        let mut remove_terrain = None;
        for (ti, t) in terrains.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                terrain_changed |= ui.text_edit_singleline(&mut t.name).changed();
                if ui
                    .selectable_label(brush.terrain == Some(ti), "Paint")
                    .clicked()
                {
                    brush.terrain = Some(ti);
                    self.tools.tool = openpax_paint::ToolKind::TileStamp;
                }
                if widgets::icon_button(ui, icons::TRASH, "Remove terrain").clicked() {
                    remove_terrain = Some(ti);
                }
            });
            ui.weak(format!(
                "{} rules · {}",
                t.rules.len(),
                match t.mode {
                    AutotileMode::Edge4 => "4 neighbours (16 tiles)",
                    AutotileMode::Blob8 => "8 neighbours (47 tiles)",
                }
            ));
            ui.horizontal(|ui| {
                if ui.small_button("Rules from selected tiles").on_hover_text(
                    "Shift-click 16 tiles (or 47 for 8 neighbours) in bitmask order: tile n is used where the neighbours spell n (N=1, E=2, S=4, W=8)",
                ).clicked() {
                    t.rules = auto_rules(&brush.random, t.mode);
                    terrain_changed = true;
                }
                if ui.small_button(if t.mode == AutotileMode::Edge4 { "Use 8 neighbours" } else { "Use 4 neighbours" }).clicked() {
                    t.mode = if t.mode == AutotileMode::Edge4 { AutotileMode::Blob8 } else { AutotileMode::Edge4 };
                    terrain_changed = true;
                }
            });
        }
        if let Some(ti) = remove_terrain {
            terrains.remove(ti);
            terrain_changed = true;
        }
        if ui.small_button("Add terrain").clicked() {
            let n = terrains.len() + 1;
            terrains.push(Terrain {
                name: format!("Terrain {n}"),
                mode: AutotileMode::Edge4,
                rules: Default::default(),
            });
            terrain_changed = true;
        }
        if terrain_changed {
            // Tiles in a terrain's rules belong to it.
            doc.edit_extras("Terrains", |e| {
                if let Some(s) = &mut e.tileset {
                    for (ti, t) in terrains.iter().enumerate() {
                        for ids in t.rules.values() {
                            for id in ids {
                                if let Some(tile) = s.tiles.get_mut(*id) {
                                    tile.props.terrain = Some(ti);
                                }
                            }
                        }
                    }
                    s.terrains = terrains;
                }
            });
        }
        ui.add_space(SPACE_S);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Add selection as tile").clicked() {
                let image = doc.copy_selected();
                doc.edit_extras("Add tile", |e| {
                    if let Some(s) = &mut e.tileset {
                        s.add_tile(image);
                    }
                });
            }
            if ui.button("Slice…").clicked() {
                open = Some(PixelDialog::SliceTileset {
                    tile_w: set.tile_w,
                    tile_h: set.tile_h,
                    offset: (0, 0),
                    spacing: (0, 0),
                    skip_empty: true,
                    dedupe: true,
                    from_file: false,
                });
            }
            if ui.button("New tilemap layer…").clicked() {
                open = Some(PixelDialog::new_tilemap(doc));
            }
        });
        let layer = doc.active_layer().clone();
        if let Some(map) = &layer.tilemap {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Export map JSON…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name(format!("{}.json", layer.name))
                        .save_file()
                {
                    let json = map.to_json(&set, &layer.name);
                    let _ =
                        std::fs::write(path, serde_json::to_vec_pretty(&json).unwrap_or_default());
                }
                if ui
                    .button("Export Tiled map…")
                    .on_hover_text("A .tmj map plus the tileset image next to it")
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name(format!("{}.tmj", layer.name))
                        .save_file()
                {
                    let image_name = format!(
                        "{}_tiles.png",
                        path.file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    );
                    let json = map.to_tiled(&set, &image_name, 16);
                    let _ =
                        std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap_or_default());
                    if let Ok(png) = openpax_paint::encode_png(&set.atlas(16)) {
                        let _ = std::fs::write(path.with_file_name(image_name), png);
                    }
                }
            });
        }
        if let Some(d) = open {
            self.open_pixel_dialog(d);
        }
    }

    // ---- origin and guides ----

    fn origin_section(&mut self, ui: &mut egui::Ui) {
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        let doc = &mut editor.doc;
        let mut extras = doc.extras.clone();
        let mut has_pivot = extras.pivot.is_some();
        if ui
            .checkbox(&mut has_pivot, "Pivot (origin for game engines)")
            .changed()
        {
            extras.pivot = has_pivot.then(|| (doc.width() as i32 / 2, doc.height() as i32 - 1));
        }
        if let Some((x, y)) = &mut extras.pivot {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(x).prefix("x "));
                ui.add(egui::DragValue::new(y).prefix("y "));
                if ui.small_button("Centre").clicked() {
                    *x = doc.width() as i32 / 2;
                    *y = doc.height() as i32 / 2;
                }
                if ui.small_button("Bottom").clicked() {
                    *x = doc.width() as i32 / 2;
                    *y = doc.height() as i32 - 1;
                }
            });
        }
        widgets::caption(ui, "ANCHOR POINTS");
        let mut remove = None;
        for (i, a) in extras.anchors.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut a.name).desired_width(80.0));
                ui.add(egui::DragValue::new(&mut a.x).prefix("x "));
                ui.add(egui::DragValue::new(&mut a.y).prefix("y "));
                if widgets::icon_button(ui, icons::TRASH, "Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            extras.anchors.remove(i);
        }
        if ui
            .small_button("Add anchor")
            .on_hover_text("A named point (hand, muzzle...) exported with sprite sheets")
            .clicked()
        {
            let n = extras.anchors.len() + 1;
            let (x, y) = editor.hover.unwrap_or((0, 0));
            extras.anchors.push(openpax_paint::Anchor {
                name: format!("anchor{n}"),
                x,
                y,
            });
        }
        widgets::caption(ui, "GUIDES");
        if extras.guides.is_empty() {
            ui.weak("Turn on the rulers (Ctrl+R) and drag out of them to add guides.");
        }
        let mut remove = None;
        for (i, g) in extras.guides.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(if g.vertical {
                    "Vertical at x"
                } else {
                    "Horizontal at y"
                });
                ui.add(egui::DragValue::new(&mut g.position));
                if widgets::icon_button(ui, icons::TRASH, "Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            extras.guides.remove(i);
        }
        ui.horizontal(|ui| {
            if ui.small_button("Add vertical").clicked() {
                extras.guides.push(openpax_paint::Guide {
                    vertical: true,
                    position: doc.width() as i32 / 2,
                });
            }
            if ui.small_button("Add horizontal").clicked() {
                extras.guides.push(openpax_paint::Guide {
                    vertical: false,
                    position: doc.height() as i32 / 2,
                });
            }
        });
        ui.checkbox(
            &mut self.tools.view.snap_to_guides,
            "Snap selections and shapes to guides",
        );
        if extras != doc.extras {
            doc.edit_extras("Origin and guides", |e| *e = extras);
        }
    }

    // ---- checks ----

    fn checks_section(&mut self, ui: &mut egui::Ui) {
        let view = &mut self.tools.view;
        ui.checkbox(
            &mut view.show_issues,
            "Highlight problems on the canvas (F7)",
        );
        let c = &mut view.checks;
        ui.checkbox(&mut c.seams, "Tile seams (edges that do not wrap)");
        if c.seams {
            ui.add(egui::Slider::new(&mut c.seam_tolerance, 0..=255).text("Seam tolerance"));
        }
        ui.checkbox(&mut c.strays, "Stray pixels");
        ui.horizontal(|ui| {
            ui.label("Colour limit");
            ui.add(egui::DragValue::new(&mut c.max_colors).range(0..=256))
                .on_hover_text("0: no limit");
        });
        let mut use_palette = !c.palette.is_empty();
        if ui
            .checkbox(&mut use_palette, "Only colours from the palette")
            .changed()
        {
            c.palette = if use_palette {
                self.tools.palette.colors.clone()
            } else {
                Vec::new()
            };
        }
        let view = self.tools.view.clone();
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        editor.refresh_issues(&view);
        let flat = editor.doc.flatten();
        let score = openpax_paint::analysis::seam_score(&flat);
        let colors = openpax_paint::quantize::count_colors(&flat);
        ui.label(format!(
            "Seamlessness {:.0}% · {colors} colours",
            score * 100.0
        ));
        if editor.issues.is_empty() {
            ui.label(
                RichText::new(widgets::icon_label(icons::SUCCESS, "No problems found"))
                    .color(theme::of(ui).success),
            );
        } else {
            ui.label(format!("{} problem(s)", editor.issues.len()));
            egui::ScrollArea::vertical()
                .max_height(120.0)
                .id_salt("issues")
                .show(ui, |ui| {
                    for issue in editor.issues.iter().take(200) {
                        if ui
                            .small_button(format!("{}, {}: {}", issue.x, issue.y, issue.message))
                            .clicked()
                        {
                            editor.doc.select_mask(
                                openpax_paint::Mask::from_rect(
                                    editor.doc.width(),
                                    editor.doc.height(),
                                    openpax_paint::Rect::new(issue.x, issue.y, 1, 1),
                                ),
                                SelectMode::Replace,
                            );
                        }
                    }
                });
        }
    }

    // ---- history ----

    fn history_section(&mut self, ui: &mut egui::Ui) {
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        let (undo, redo) = editor.doc.history();
        let (undo, redo): (Vec<String>, Vec<String>) = (
            undo.into_iter().map(str::to_owned).collect(),
            redo.into_iter().map(str::to_owned).collect(),
        );
        let mut target: Option<isize> = None; // steps: negative undoes, positive redoes
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .id_salt("history")
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if ui
                    .selectable_label(undo.is_empty(), RichText::new("Opened").weak())
                    .clicked()
                {
                    target = Some(-(undo.len() as isize));
                }
                for (i, label) in undo.iter().enumerate() {
                    let current = i + 1 == undo.len();
                    if ui.selectable_label(current, label).clicked() {
                        target = Some(i as isize + 1 - undo.len() as isize);
                    }
                }
                for (i, label) in redo.iter().enumerate() {
                    if ui
                        .selectable_label(false, RichText::new(label).weak().italics())
                        .clicked()
                    {
                        target = Some(i as isize + 1);
                    }
                }
            });
        if undo.is_empty() && redo.is_empty() {
            ui.weak("Nothing to undo yet.");
        }
        if let Some(steps) = target {
            if steps < 0 {
                for _ in 0..-steps {
                    editor.doc.undo();
                }
            } else {
                for _ in 0..steps {
                    editor.doc.redo();
                }
            }
        }
    }

    // ---- texture info ----

    fn texture_info_section(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut run = None;
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        ui.add(egui::Label::new(RichText::new(&editor.rel_path).monospace().weak()).truncate())
            .on_hover_text(&editor.rel_path);
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} × {} px",
                editor.doc.width(),
                editor.doc.height()
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for command in [
                    Command::ResizeCanvas,
                    Command::Pixel(PixelCmd::ScaleImage),
                    Command::FlipVertical,
                    Command::FlipHorizontal,
                ] {
                    if widgets::icon_button(ui, command.icon(), &command.tooltip(&ctx)).clicked() {
                        run = Some(command);
                    }
                }
            });
        });
        if ui
            .button(widgets::icon_label(icons::EXPORT, "Export…"))
            .clicked()
        {
            run = Some(Command::Pixel(PixelCmd::ExportImage));
        }
        if let Some(command) = run {
            self.run(command, &ctx);
        }
    }
}

enum LayerAction {
    Select,
    ToggleCollapsed,
    Visible(bool),
    Lock(bool),
    AlphaLock(bool),
    Clip(bool),
    Tag(Option<Rgba>),
    Rename(String),
    SelectAlpha,
    Parent(Option<u32>),
    Group,
    Ungroup,
}

/// Autotile rules from tiles picked in bitmask order.
fn auto_rules(tiles: &[usize], mode: AutotileMode) -> std::collections::BTreeMap<u8, Vec<usize>> {
    let masks: Vec<u8> = match mode {
        AutotileMode::Edge4 => (0..16).collect(),
        AutotileMode::Blob8 => {
            let mut v: Vec<u8> = (0..=255u8).map(openpax_paint::tiles::reduce_blob).collect();
            v.sort_unstable();
            v.dedup();
            v
        }
    };
    masks
        .into_iter()
        .zip(tiles.iter())
        .map(|(m, t)| (m, vec![*t]))
        .collect()
}

/// A colour editor: saturation/value square, hue and alpha bars, RGB / HSV / HSL sliders and
/// a hex field.
pub fn color_picker(ui: &mut egui::Ui, color: &mut Rgba) {
    let (h, s, v) = rgb_to_hsv(*color);
    // Remember the hue while saturation is zero (it cannot be recovered from the colour).
    let hue_id = ui.scope_id().with("picker_hue");
    let mut hue: f32 = ui.data(|d| d.get_temp(hue_id)).unwrap_or(h);
    if s > 0.01 && v > 0.01 {
        hue = h;
    }
    let width = ui.available_width().min(220.0);
    // Saturation / value square.
    let (rect, response) = ui.allocate_exact_size(vec2(width, 110.0), Sense::click_and_drag());
    let hue_rgb = hsv_to_rgb(hue, 1.0, 1.0);
    let mut mesh = Mesh::default();
    let corners = [
        (rect.left_top(), Color32::WHITE),
        (
            rect.right_top(),
            Color32::from_rgb(hue_rgb[0], hue_rgb[1], hue_rgb[2]),
        ),
        (rect.right_bottom(), Color32::BLACK),
        (rect.left_bottom(), Color32::BLACK),
    ];
    for (p, c) in corners {
        mesh.colored_vertex(p, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(mesh);
    let marker = pos2(
        rect.left() + s * rect.width(),
        rect.top() + (1.0 - v) * rect.height(),
    );
    ui.painter()
        .circle_stroke(marker, 4.0, Stroke::new(2.0, Color32::WHITE));
    ui.painter()
        .circle_stroke(marker, 5.0, Stroke::new(1.0, Color32::BLACK));
    let mut new = None;
    if let Some(p) = response
        .interact_pointer_pos()
        .filter(|_| response.is_pointer_button_down_on())
    {
        let ns = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let nv = 1.0 - ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        let rgb = hsv_to_rgb(hue, ns, nv);
        new = Some([rgb[0], rgb[1], rgb[2], color[3]]);
    }
    // Hue bar.
    let (bar, response) = ui.allocate_exact_size(vec2(width, 12.0), Sense::click_and_drag());
    let mut mesh = Mesh::default();
    for i in 0..=6 {
        let x = bar.left() + bar.width() * i as f32 / 6.0;
        let c = hsv_to_rgb(i as f32 * 60.0, 1.0, 1.0);
        let c = Color32::from_rgb(c[0], c[1], c[2]);
        mesh.colored_vertex(pos2(x, bar.top()), c);
        mesh.colored_vertex(pos2(x, bar.bottom()), c);
        if i > 0 {
            let b = (i * 2) as u32;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b, b + 1);
        }
    }
    ui.painter().add(mesh);
    let hx = bar.left() + hue / 360.0 * bar.width();
    ui.painter().rect_stroke(
        Rect::from_center_size(pos2(hx, bar.center().y), vec2(4.0, 14.0)),
        1.0,
        Stroke::new(1.5, Color32::WHITE),
        StrokeKind::Outside,
    );
    if let Some(p) = response
        .interact_pointer_pos()
        .filter(|_| response.is_pointer_button_down_on())
    {
        hue = ((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0) * 359.9;
        let rgb = hsv_to_rgb(hue, s.max(if s == 0.0 && v > 0.0 { 0.0 } else { s }), v);
        new = Some([rgb[0], rgb[1], rgb[2], color[3]]);
    }
    // Alpha bar.
    let (abar, response) = ui.allocate_exact_size(vec2(width, 12.0), Sense::click_and_drag());
    let mut mesh = Mesh::default();
    mesh.colored_vertex(
        abar.left_top(),
        Color32::from_rgba_unmultiplied(color[0], color[1], color[2], 0),
    );
    mesh.colored_vertex(
        abar.right_top(),
        Color32::from_rgb(color[0], color[1], color[2]),
    );
    mesh.colored_vertex(
        abar.right_bottom(),
        Color32::from_rgb(color[0], color[1], color[2]),
    );
    mesh.colored_vertex(
        abar.left_bottom(),
        Color32::from_rgba_unmultiplied(color[0], color[1], color[2], 0),
    );
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().rect_filled(abar, 0.0, Color32::from_gray(150));
    ui.painter().add(mesh);
    let ax = abar.left() + f32::from(color[3]) / 255.0 * abar.width();
    ui.painter().rect_stroke(
        Rect::from_center_size(pos2(ax, abar.center().y), vec2(4.0, 14.0)),
        1.0,
        Stroke::new(1.5, Color32::WHITE),
        StrokeKind::Outside,
    );
    if let Some(p) = response
        .interact_pointer_pos()
        .filter(|_| response.is_pointer_button_down_on())
    {
        let a = (((p.x - abar.left()) / abar.width()).clamp(0.0, 1.0) * 255.0).round() as u8;
        new = Some([color[0], color[1], color[2], a]);
    }
    ui.data_mut(|d| d.insert_temp(hue_id, hue));

    // Numeric editors.
    let mode_id = ui.scope_id().with("picker_mode");
    let mut mode: u8 = ui.data(|d| d.get_temp(mode_id)).unwrap_or(0);
    ui.horizontal(|ui| {
        for (m, label) in [(0u8, "RGB"), (1, "HSV"), (2, "HSL")] {
            ui.selectable_value(&mut mode, m, label);
        }
        let hex_id = ui.scope_id().with("picker_hex");
        let mut text: String = ui
            .data(|d| d.get_temp(hex_id))
            .unwrap_or_else(|| to_hex(*color));
        let field = ui.add(
            egui::TextEdit::singleline(&mut text)
                .desired_width(76.0)
                .font(egui::TextStyle::Monospace),
        );
        if field.changed()
            && let Some(c) = parse_hex(&text)
        {
            new = Some(c);
        }
        if field.has_focus() {
            ui.data_mut(|d| d.insert_temp(hex_id, text));
        } else {
            ui.data_mut(|d| d.remove::<String>(hex_id));
        }
    });
    ui.data_mut(|d| d.insert_temp(mode_id, mode));
    let c = new.unwrap_or(*color);
    match mode {
        0 => {
            let mut v = c;
            let mut changed = false;
            for (i, label) in ["R", "G", "B", "A"].iter().enumerate() {
                changed |= ui
                    .add(egui::Slider::new(&mut v[i], 0..=255).text(*label))
                    .changed();
            }
            if changed {
                new = Some(v);
            }
        }
        1 => {
            let (mut hh, mut ss, mut vv) = rgb_to_hsv(c);
            if ss <= 0.01 {
                hh = hue;
            }
            let mut a = c[3];
            let changed = ui
                .add(egui::Slider::new(&mut hh, 0.0..=359.9).text("H"))
                .changed()
                | ui.add(egui::Slider::new(&mut ss, 0.0..=1.0).text("S"))
                    .changed()
                | ui.add(egui::Slider::new(&mut vv, 0.0..=1.0).text("V"))
                    .changed()
                | ui.add(egui::Slider::new(&mut a, 0..=255).text("A"))
                    .changed();
            if changed {
                let rgb = hsv_to_rgb(hh, ss, vv);
                new = Some([rgb[0], rgb[1], rgb[2], a]);
                ui.data_mut(|d| d.insert_temp(hue_id, hh));
            }
        }
        _ => {
            let (mut hh, mut ss, mut ll) = rgb_to_hsl(c);
            if ss <= 0.01 {
                hh = hue;
            }
            let mut a = c[3];
            let changed = ui
                .add(egui::Slider::new(&mut hh, 0.0..=359.9).text("H"))
                .changed()
                | ui.add(egui::Slider::new(&mut ss, 0.0..=1.0).text("S"))
                    .changed()
                | ui.add(egui::Slider::new(&mut ll, 0.0..=1.0).text("L"))
                    .changed()
                | ui.add(egui::Slider::new(&mut a, 0..=255).text("A"))
                    .changed();
            if changed {
                let rgb = hsl_to_rgb(hh, ss, ll);
                new = Some([rgb[0], rgb[1], rgb[2], a]);
                ui.data_mut(|d| d.insert_temp(hue_id, hh));
            }
        }
    }
    if let Some(c) = new {
        *color = c;
    }
}

/// Canvas display settings, for the Canvas menu.
pub fn view_settings_ui(ui: &mut egui::Ui, view: &mut ViewPrefs) {
    ui.menu_button("Pixel grid", |ui| {
        ui.horizontal(|ui| {
            ui.label("Colour");
            color_button(ui, &mut view.grid_color);
        });
        ui.add(egui::Slider::new(&mut view.grid_min_zoom, 2.0..=32.0).text("Show from zoom"));
    });
    ui.menu_button("Tile grid", |ui| {
        ui.checkbox(&mut view.tile_grid, "Show tile grid");
        ui.horizontal(|ui| {
            ui.label("Size");
            ui.add(egui::DragValue::new(&mut view.tile_grid_size.0).range(1..=512));
            ui.add(egui::DragValue::new(&mut view.tile_grid_size.1).range(1..=512));
        });
        ui.horizontal(|ui| {
            ui.label("Offset");
            ui.add(egui::DragValue::new(&mut view.tile_grid_offset.0));
            ui.add(egui::DragValue::new(&mut view.tile_grid_offset.1));
        });
        ui.horizontal(|ui| {
            ui.label("Colour");
            color_button(ui, &mut view.tile_grid_color);
        });
        for g in openpax_paint::tiles::GridKind::ALL {
            ui.radio_value(&mut view.tile_grid_kind, g, g.name());
        }
    });
    ui.menu_button("Background", |ui| {
        if ui
            .radio(view.background == Background::Checker, "Checkerboard")
            .clicked()
        {
            view.background = Background::Checker;
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut view.checker_size)
                    .range(1..=64)
                    .suffix(" px squares"),
            );
        });
        ui.horizontal(|ui| {
            color_button(ui, &mut view.checker_a);
            color_button(ui, &mut view.checker_b);
        });
        let mut solid = match view.background {
            Background::Solid(c) => c,
            Background::Checker => [64, 64, 64, 255],
        };
        ui.horizontal(|ui| {
            if ui
                .radio(matches!(view.background, Background::Solid(_)), "Solid")
                .clicked()
                || color_button(ui, &mut solid)
            {
                view.background = Background::Solid(solid);
            }
        });
    });
    ui.menu_button("Tiling preview", |ui| {
        for t in Tiling::ALL {
            ui.radio_value(&mut view.tiling, t, t.name());
        }
    });
    ui.menu_button("Colour vision", |ui| {
        for v in openpax_paint::color::VisionSim::ALL {
            ui.radio_value(&mut view.vision, v, v.name());
        }
    });
    ui.menu_button("Mouse buttons", |ui| {
        ui.label("Right button");
        ui.radio_value(
            &mut view.right_button,
            crate::texture_editor::RightButton::SecondaryColor,
            "Paint with the secondary colour",
        );
        ui.radio_value(
            &mut view.right_button,
            crate::texture_editor::RightButton::Eraser,
            "Erase",
        );
        ui.radio_value(
            &mut view.right_button,
            crate::texture_editor::RightButton::Picker,
            "Pick the secondary colour",
        );
        ui.separator();
        ui.label("Middle button");
        ui.radio_value(
            &mut view.middle_button,
            crate::texture_editor::MiddleButton::Pan,
            "Pan",
        );
        ui.radio_value(
            &mut view.middle_button,
            crate::texture_editor::MiddleButton::Picker,
            "Pick colour",
        );
        ui.separator();
        for (label, slot) in [
            ("Back button", &mut view.extra1),
            ("Forward button", &mut view.extra2),
        ] {
            egui::ComboBox::from_label(label)
                .selected_text(slot.map_or("Nothing", |t| t.name()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(slot, None, "Nothing");
                    for t in openpax_paint::ToolKind::ALL {
                        ui.selectable_value(slot, Some(t), t.name());
                    }
                });
        }
    });
    ui.checkbox(&mut view.integer_zoom, "Whole-step zoom only");
}

impl App {
    /// Dither patterns: pick one for the dither brush, fill the selection with it, save the
    /// selection as a new one.
    fn patterns_section(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut chosen: Option<Option<String>> = None;
        let mut delete = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
            let ordered = self.tools.settings.dither_pattern.is_none();
            if ui
                .selectable_label(ordered, "Ordered")
                .on_hover_text("The Bayer matrix, with an adjustable density")
                .clicked()
            {
                chosen = Some(None);
            }
            for p in &self.tools.patterns {
                // A preview of the tile repeated over 4 × 4 tiles.
                let id =
                    egui::Id::unique(("pattern_tex", &p.name, p.image.width(), p.image.height()));
                let texture: TextureHandle =
                    ctx.data_mut(|d| d.get_temp(id)).unwrap_or_else(|| {
                        let mut preview = openpax_paint::Bitmap::new(16, 16);
                        preview.map_pixels(|x, y, _| {
                            if openpax_paint::quantize::pattern_on(&p.image, x, y) {
                                [30, 30, 30, 255]
                            } else {
                                [225, 225, 225, 255]
                            }
                        });
                        let t = ctx.load_texture(
                            format!("pattern:{}", p.name),
                            crate::texture_editor::color_image(&preview),
                            egui::TextureOptions::NEAREST,
                        );
                        ctx.data_mut(|d| d.insert_temp(id, t.clone()));
                        t
                    });
                let selected = self.tools.settings.dither_pattern_name.as_deref()
                    == Some(p.name.as_str())
                    && !ordered;
                let response = ui
                    .add(
                        egui::Button::image(egui::Image::from_texture((
                            texture.id(),
                            vec2(24.0, 24.0),
                        )))
                        .selected(selected),
                    )
                    .on_hover_text(format!(
                        "{} ({}×{})",
                        p.name,
                        p.image.width(),
                        p.image.height()
                    ));
                if response.clicked() {
                    chosen = Some(Some(p.name.clone()));
                }
                if !p.builtin {
                    response.context_menu(|ui| {
                        if ui
                            .button(widgets::icon_label(icons::TRASH, "Delete pattern"))
                            .clicked()
                        {
                            delete = Some(p.name.clone());
                            ui.close();
                        }
                    });
                }
            }
        });
        if let Some(name) = chosen {
            self.tools.select_pattern(name.as_deref());
            self.tools.tool = openpax_paint::ToolKind::Dither;
        }
        if let Some(name) = delete {
            self.tools.delete_pattern(&name);
        }
        let pattern = self.tools.settings.dither_pattern.clone();
        let (primary, secondary) = (self.tools.settings.color, self.tools.secondary);
        let has_texture = texture_mut(&mut self.editors, self.active).is_some();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    has_texture && pattern.is_some(),
                    egui::Button::new("Fill selection"),
                )
                .on_hover_text("Primary colour where the pattern is on, secondary elsewhere")
                .on_disabled_hover_text("Pick a pattern first")
                .clicked()
                && let (Some(e), Some(p)) = (texture_mut(&mut self.editors, self.active), &pattern)
            {
                e.doc.fill_pattern(primary, secondary, p);
            }
            if ui
                .add_enabled(
                    has_texture && pattern.is_some(),
                    egui::Button::new("Overlay"),
                )
                .on_hover_text("Primary colour where the pattern is on; other pixels stay")
                .clicked()
                && let (Some(e), Some(p)) = (texture_mut(&mut self.editors, self.active), &pattern)
            {
                e.doc.fill_pattern(primary, [0, 0, 0, 0], p);
            }
            if ui
                .add_enabled(has_texture, egui::Button::new("Save selection as pattern…"))
                .clicked()
            {
                let n = self.tools.patterns.iter().filter(|p| !p.builtin).count() + 1;
                self.open_pixel_dialog(PixelDialog::SavePattern {
                    name: format!("Pattern {n}"),
                });
            }
            if ui.button("Folder").clicked() {
                crate::user_files::reveal(&crate::user_files::dir("patterns"));
            }
        });
        ui.weak("Patterns tile across the image. Draw one small tile, select it and save it.");
    }
}
