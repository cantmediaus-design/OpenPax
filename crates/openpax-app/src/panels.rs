//! The right-hand inspector: colours, palette, layers and properties of the active tab.

use std::time::Duration;

use eframe::egui::{self, Color32, Rect, RichText, Sense, Stroke, pos2, vec2};
use openpax_core::{Animation, version_for_format};
use openpax_paint::{Palette, Rgba};

use crate::app::{App, texture_mut};
use crate::commands::{Active, Command};
use crate::dialogs::format_picker;
use crate::theme::{self, SPACE_M, SPACE_S};
use crate::{icons, widgets};

const SWATCH: f32 = 20.0;
const PREVIEW: f32 = 96.0;
const LAYER_THUMB: f32 = 18.0;
const TICK: Duration = Duration::from_millis(50);

impl App {
    pub(crate) fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                // Keep controls clear of the scroll bar.
                ui.set_max_width(ui.available_width() - SPACE_M);
                ui.add_space(SPACE_S);
                match self.active_kind() {
                    Active::Texture => {
                        widgets::section(ui, icons::PICKER, "Colour", |ui| self.color_section(ui));
                        widgets::section(ui, icons::PALETTE, "Palette", |ui| {
                            self.palette_section(ui);
                        });
                        widgets::section(ui, icons::LAYERS, "Layers", |ui| self.layers_section(ui));
                        widgets::section(ui, icons::IMAGE, "Texture", |ui| {
                            self.texture_section(ui);
                        });
                    }
                    Active::Model => self.model_sections(ui),
                    Active::Text | Active::None => {}
                }
                widgets::section(ui, icons::PACKAGE, "Pack", |ui| self.pack_section(ui));
            });
    }

    fn color_section(&mut self, ui: &mut egui::Ui) {
        let tools = &mut self.tools;
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            color_well(ui, "Primary", &mut tools.settings.color);
            if widgets::icon_button(ui, icons::SWAP, &Command::SwapColors.tooltip(&ctx)).clicked() {
                tools.swap_colors();
            }
            color_well(ui, "Secondary", &mut tools.secondary);
        });
        ui.weak("Left button paints with the primary colour, right button with the secondary. Hold Alt to pick a colour from the canvas.");

        if !tools.recent.is_empty() {
            ui.add_space(SPACE_S);
            widgets::caption(ui, "RECENT");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                for color in tools.recent.clone() {
                    let selected = color == tools.settings.color;
                    let response = widgets::swatch(ui, color, SWATCH, selected, &self.checker)
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

    fn palette_section(&mut self, ui: &mut egui::Ui) {
        let tools = &mut self.tools;
        let mut rects: Vec<Rect> = Vec::with_capacity(tools.palette.colors.len());
        let mut dragged = None;
        let mut dropped = None;
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
            for (index, color) in tools.palette.colors.iter().copied().enumerate() {
                let selected = color == tools.settings.color;
                let response = widgets::swatch(ui, color, SWATCH, selected, &self.checker);
                rects.push(response.rect);
                if response.clicked() {
                    tools.settings.color = color;
                }
                if response.dragged() {
                    dragged = Some(index);
                }
                if response.drag_stopped() {
                    dropped = Some(index);
                }
                response.context_menu(|ui| {
                    if ui.button("Set as Primary").clicked() {
                        tools.settings.color = color;
                    }
                    if ui.button("Set as Secondary").clicked() {
                        tools.secondary = color;
                    }
                    ui.separator();
                    if ui
                        .button(widgets::icon_label(icons::TRASH, "Remove from Palette"))
                        .clicked()
                    {
                        remove = Some(index);
                    }
                });
                response.on_hover_text(format!(
                    "{}\nRight-click for more, drag to reorder",
                    widgets::hex(color)
                ));
            }
        });

        // Dragging a swatch onto another moves it there.
        let pointer = ui.ctx().pointer_latest_pos();
        let target = pointer.and_then(|pos| rects.iter().position(|r| r.contains(pos)));
        if let (Some(from), Some(to)) = (dragged, target)
            && from != to
        {
            let accent = theme::of(ui).accent;
            ui.painter().rect_stroke(
                rects[to].expand(1.5),
                theme::RADIUS,
                Stroke::new(2.0, accent),
                egui::StrokeKind::Outside,
            );
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if let (Some(from), Some(to)) = (dropped, target)
            && from != to
        {
            let color = tools.palette.colors.remove(from);
            tools.palette.colors.insert(to, color);
        }
        if let Some(index) = remove {
            tools.palette.colors.remove(index);
        }

        ui.add_space(SPACE_S);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if widgets::icon_button(ui, icons::PLUS, "Add the primary colour").clicked() {
                tools.palette.add(tools.settings.color);
            }
            let editor = texture_mut(&mut self.editors, self.active);
            if widgets::icon_button(ui, icons::IMAGE, "Build the palette from this texture")
                .clicked()
                && let Some(editor) = editor
            {
                tools.palette = Palette::from_bitmap(&editor.doc.flatten(), 64);
            }
            if widgets::icon_button(ui, icons::REFRESH, "Reset to the default palette").clicked() {
                tools.palette = Palette::default();
            }
        });
    }

    fn layers_section(&mut self, ui: &mut egui::Ui) {
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        // A fresh press starts a new undo step for the opacity slider.
        if ui.ctx().input(|i| i.pointer.any_pressed()) {
            editor.doc.end_coalescing();
        }
        let thumbs: Vec<egui::TextureId> = editor
            .layer_textures(ui.ctx())
            .iter()
            .map(egui::TextureHandle::id)
            .collect();
        let aspect = editor.doc.height() as f32 / editor.doc.width().max(1) as f32;
        let doc = &mut editor.doc;
        let count = doc.layers().len();

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if widgets::icon_button(ui, icons::PLUS, "Add a layer above").clicked() {
                doc.add_layer();
            }
            if widgets::icon_button(ui, icons::COPY, "Duplicate the layer").clicked() {
                doc.duplicate_layer();
            }
            let active = doc.active();
            if widgets::icon_button_enabled(ui, active + 1 < count, icons::ARROW_UP, "Move up")
                .clicked()
            {
                doc.move_layer(true);
            }
            if widgets::icon_button_enabled(ui, active > 0, icons::ARROW_DOWN, "Move down")
                .clicked()
            {
                doc.move_layer(false);
            }
            if widgets::icon_button_enabled(
                ui,
                active > 0,
                icons::MERGE_DOWN,
                "Merge into the layer below",
            )
            .clicked()
            {
                doc.merge_down();
            }
            if widgets::icon_button_enabled(ui, count > 1, icons::TRASH, "Delete the layer")
                .clicked()
            {
                doc.remove_layer();
            }
        });
        ui.add_space(SPACE_S);

        // Top layer first, like every other paint program.
        let mut rects: Vec<(usize, Rect)> = Vec::with_capacity(count);
        let mut dragged = None;
        let mut dropped = None;
        let checker = self.checker.id();
        let checker_tint = theme::of(ui).checker;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for index in (0..doc.layers().len()).rev() {
                let layer = &doc.layers()[index];
                let (name, visible, opacity) = (layer.name.clone(), layer.visible, layer.opacity);
                let selected = index == doc.active();
                let (rect, response) = widgets::list_row(ui, selected);
                rects.push((index, rect));
                let mut row = widgets::row_ui(ui, rect, 0.0);

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

                let eye = if visible { icons::EYE } else { icons::EYE_OFF };
                let eye_text = if visible {
                    RichText::new(eye)
                } else {
                    RichText::new(eye).weak()
                };
                if row
                    .add(egui::Button::new(eye_text).frame(false))
                    .on_hover_text(if visible { "Hide layer" } else { "Show layer" })
                    .clicked()
                {
                    doc.set_layer_visible(index, !visible);
                }

                // Thumbnail on a checkerboard so transparent layers read as such.
                let size = if aspect >= 1.0 {
                    vec2(LAYER_THUMB / aspect.min(4.0), LAYER_THUMB)
                } else {
                    vec2(LAYER_THUMB, LAYER_THUMB * aspect.max(0.25))
                };
                let (thumb, _) = row.allocate_exact_size(size, Sense::hover());
                let unit = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                let cells = Rect::from_min_max(pos2(0.0, 0.0), pos2(2.0, 2.0 * aspect));
                row.painter().image(checker, thumb, cells, checker_tint);
                if let Some(texture) = thumbs.get(index) {
                    row.painter().image(*texture, thumb, unit, Color32::WHITE);
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
                            // Enter and clicking away keep the name; Escape discards it.
                            if !row.input(|i| i.key_pressed(egui::Key::Escape))
                                && !text.trim().is_empty()
                            {
                                doc.set_layer_name(index, text.trim().to_owned());
                            }
                            editor.renaming = None;
                        }
                    }
                    _ => {
                        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if opacity < 0.995 {
                                ui.add_space(SPACE_S);
                                ui.label(
                                    RichText::new(format!("{:.0}%", opacity * 100.0))
                                        .small()
                                        .weak(),
                                );
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(RichText::new(&name).color(color))
                                            .selectable(false)
                                            .truncate(),
                                    );
                                },
                            );
                        });
                    }
                }

                if response.clicked() {
                    doc.set_active(index);
                }
                if response.double_clicked() {
                    editor.renaming = Some((index, name, false));
                }
                response.on_hover_text("Double-click to rename");
            }
        });

        // Dragging a handle shows where the layer will land and moves it on release.
        let pointer = ui.ctx().pointer_latest_pos();
        let target = pointer.and_then(|pos| {
            rects
                .iter()
                .min_by(|a, b| {
                    let da = (a.1.center().y - pos.y).abs();
                    let db = (b.1.center().y - pos.y).abs();
                    da.total_cmp(&db)
                })
                .map(|(index, rect)| (*index, *rect))
        });
        if let (Some(from), Some((to, rect))) = (dragged, target)
            && from != to
        {
            // Rows are listed top-first, so a higher index sits above.
            let y = if to > from { rect.top() } else { rect.bottom() };
            let accent = theme::of(ui).accent;
            ui.painter()
                .hline(rect.x_range(), y, Stroke::new(2.0, accent));
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if let (Some(from), Some((to, _))) = (dropped, target)
            && from != to
        {
            doc.set_active(from);
            doc.move_layer_to(to);
        }

        ui.add_space(SPACE_M);
        let active = doc.active();
        let mut percent = doc.layers()[active].opacity * 100.0;
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
        if count > 1 {
            ui.weak("Layers are kept in the pack's .openpax folder; the PNG stays flattened.");
        }
    }

    fn texture_section(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut run = None;
        let Some(editor) = texture_mut(&mut self.editors, self.active) else {
            return;
        };
        let (width, height) = (editor.doc.width(), editor.doc.height());
        ui.add(egui::Label::new(RichText::new(&editor.rel_path).monospace().weak()).truncate())
            .on_hover_text(&editor.rel_path);
        ui.horizontal(|ui| {
            ui.label(format!("{width} × {height} px"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for command in [
                    Command::ResizeCanvas,
                    Command::FlipVertical,
                    Command::FlipHorizontal,
                ] {
                    if widgets::icon_button(ui, command.icon(), &command.tooltip(&ctx)).clicked() {
                        run = Some(command);
                    }
                }
            });
        });

        ui.add_space(SPACE_S);
        let mut animated = editor.meta.animation.is_some();
        if ui
            .checkbox(&mut animated, "Animated")
            .on_hover_text("Stores the animation in a .mcmeta file next to the texture")
            .changed()
        {
            editor.meta.animation = animated.then(Animation::default);
        }
        if let Some(mut animation) = editor.meta.animation.clone() {
            let mut frametime = animation.frametime.unwrap_or(1);
            widgets::field(ui, "Frame time", |ui| {
                if ui
                    .add(
                        egui::DragValue::new(&mut frametime)
                            .range(1..=12000)
                            .suffix(" ticks"),
                    )
                    .on_hover_text("20 ticks are one second")
                    .changed()
                {
                    animation.frametime = Some(frametime);
                }
            });
            let mut interpolate = animation.interpolate.unwrap_or(false);
            if ui
                .checkbox(&mut interpolate, "Interpolate between frames")
                .changed()
            {
                animation.interpolate = interpolate.then_some(true);
            }

            let (fw, fh) = animation.frame_size(width, height);
            let count = animation.frame_count(width, height);
            ui.label(format!("{count} frames of {fw} × {fh} px"));
            ui.checkbox(&mut editor.onion_skin, "Onion skin")
                .on_hover_text("Show each frame's previous frame faintly on the canvas");

            // Playback order: which frame shows, for how long, in what order.
            ui.add_space(SPACE_S);
            let texture_id = editor.texture(ui.ctx()).id();
            let columns = (width / fw).max(1);
            let frame_uv = |i: u32| {
                let (col, row) = (i % columns, i / columns);
                Rect::from_min_max(
                    pos2(
                        (col * fw) as f32 / width as f32,
                        (row * fh) as f32 / height as f32,
                    ),
                    pos2(
                        ((col + 1) * fw) as f32 / width as f32,
                        ((row + 1) * fh) as f32 / height as f32,
                    ),
                )
            };
            let mut sequence = animation.sequence(width, height);
            let before = sequence.clone();
            let mut selected = editor.timeline_selected.filter(|s| *s < sequence.len());
            ui.horizontal(|ui| {
                widgets::caption(ui, "PLAYBACK ORDER");
                if animation.frames.is_some() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("Plain order")
                            .on_hover_text("Play every frame once, top to bottom")
                            .clicked()
                        {
                            let time = animation.frametime.unwrap_or(1).max(1);
                            sequence = (0..count).map(|i| (i, time)).collect();
                            selected = None;
                        }
                    });
                }
            });
            for (position, (index, time)) in sequence.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_S;
                    if ui
                        .selectable_label(
                            selected == Some(position),
                            format!("{:>2}", position + 1),
                        )
                        .on_hover_text("Select this step to move, copy or remove it")
                        .clicked()
                    {
                        selected = (selected != Some(position)).then_some(position);
                    }
                    let thumb = vec2(22.0, 22.0 * fh as f32 / fw as f32);
                    ui.add(egui::Image::from_texture((texture_id, thumb)).uv(frame_uv(*index)));
                    ui.add(
                        egui::DragValue::new(index)
                            .range(0..=count - 1)
                            .prefix("frame "),
                    )
                    .on_hover_text("Which frame of the strip shows (0 is the top)");
                    ui.add(egui::DragValue::new(time).range(1..=12000).suffix(" t"))
                        .on_hover_text("How long it shows, in ticks (20 per second)");
                });
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let has = selected.is_some();
                let at = selected.unwrap_or(0);
                if widgets::icon_button_enabled(ui, has && at > 0, icons::ARROW_UP, "Play earlier")
                    .clicked()
                {
                    sequence.swap(at, at - 1);
                    selected = Some(at - 1);
                }
                if widgets::icon_button_enabled(
                    ui,
                    has && at + 1 < sequence.len(),
                    icons::ARROW_DOWN,
                    "Play later",
                )
                .clicked()
                {
                    sequence.swap(at, at + 1);
                    selected = Some(at + 1);
                }
                if widgets::icon_button_enabled(ui, has, icons::COPY, "Repeat this step").clicked()
                {
                    let step = sequence[at];
                    sequence.insert(at + 1, step);
                    selected = Some(at + 1);
                }
                if widgets::icon_button_enabled(
                    ui,
                    has && sequence.len() > 1,
                    icons::TRASH,
                    "Remove this step (the frame stays in the image)",
                )
                .clicked()
                {
                    sequence.remove(at);
                    selected = None;
                }
            });
            if sequence != before {
                animation.set_sequence(&sequence, width, height);
            }
            editor.timeline_selected = selected;
            if width == fw {
                ui.horizontal(|ui| {
                    if ui
                        .button(widgets::icon_label(icons::PLUS, "Add frame"))
                        .clicked()
                    {
                        editor.doc.resize_canvas(width, height + fh);
                        editor.fit_pending = true;
                    }
                    if ui
                        .add_enabled(
                            height > fh,
                            egui::Button::new(widgets::icon_label(icons::MINUS, "Remove last")),
                        )
                        .clicked()
                    {
                        editor.doc.resize_canvas(width, height - fh);
                        editor.fit_pending = true;
                    }
                });
            }
            if editor.meta.animation.as_ref() != Some(&animation) {
                editor.meta.animation = Some(animation.clone());
            }

            // Live preview at game speed (20 ticks per second).
            let tick = (ui.ctx().input(|i| i.time) * 20.0) as u64;
            let frame = animation.frame_at(width, height, tick).min(count - 1);
            let columns = (width / fw).max(1);
            let (col, row) = (frame % columns, frame / columns);
            let uv = Rect::from_min_max(
                pos2(
                    (col * fw) as f32 / width as f32,
                    (row * fh) as f32 / height as f32,
                ),
                pos2(
                    ((col + 1) * fw) as f32 / width as f32,
                    ((row + 1) * fh) as f32 / height as f32,
                ),
            );
            let size = vec2(PREVIEW, PREVIEW * fh as f32 / fw as f32);
            let texture_id = editor.texture(ui.ctx()).id();
            ui.add_space(SPACE_S);
            ui.add(egui::Image::from_texture((texture_id, size)).uv(uv));
            widgets::caption(ui, &format!("Frame {} of {count}", frame + 1));
            ui.ctx().request_repaint_after(TICK);
        }

        if let Some(command) = run {
            self.run(command, &ctx);
        }
    }

    fn pack_section(&mut self, ui: &mut egui::Ui) {
        let dirty = self.pack_dirty();
        let Some(pack) = &mut self.pack else { return };
        let root = pack.root().display().to_string();
        ui.label(RichText::new(pack.name()).strong());
        ui.add(egui::Label::new(RichText::new(&root).monospace().weak()).truncate())
            .on_hover_text(&root);
        ui.add_space(SPACE_S);

        match &mut pack.meta.pack.description {
            serde_json::Value::String(text) => {
                widgets::caption(ui, "DESCRIPTION");
                ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
            }
            _ => {
                ui.weak(
                    "The description is a text component; edit pack.mcmeta as text to change it.",
                );
            }
        }
        match pack.meta.format() {
            Some(mut format) => {
                widgets::caption(ui, "GAME VERSION");
                ui.horizontal(|ui| {
                    let before = format;
                    format_picker(ui, &mut format);
                    if format != before {
                        pack.meta.set_format(format);
                    }
                });
                if version_for_format(format).is_none() {
                    ui.weak("OpenPax does not know which game version this format belongs to.");
                }
            }
            None => {
                ui.weak("No pack format declared.");
            }
        }

        if dirty {
            ui.add_space(SPACE_M);
            ui.horizontal(|ui| {
                let save = widgets::primary_button(ui, true, "Save pack.mcmeta").clicked();
                let revert = widgets::secondary_button(ui, "Revert").clicked();
                if save {
                    self.save_pack_meta();
                } else if revert {
                    self.revert_pack_meta();
                }
            });
        }

        ui.add_space(SPACE_M);
        if let Some(action) = self.pack_icon_ui(ui) {
            self.run_icon_action(action);
        }
    }
}

/// A labelled colour button with the hex value underneath.
fn color_well(ui: &mut egui::Ui, label: &str, color: &mut Rgba) {
    ui.vertical(|ui| {
        widgets::caption(ui, &label.to_uppercase());
        let [r, g, b, a] = *color;
        let mut value = Color32::from_rgba_unmultiplied(r, g, b, a);
        ui.spacing_mut().interact_size = vec2(56.0, 28.0);
        if ui.color_edit_button_srgba(&mut value).changed() {
            *color = value.to_srgba_unmultiplied();
        }
        ui.label(RichText::new(widgets::hex(*color)).monospace().small());
    });
}
