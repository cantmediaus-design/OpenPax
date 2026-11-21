//! The animation timeline under the canvas: transport controls, frames with thumbnails,
//! tags, per-layer cels (with links) and durations.

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind,
    UiBuilder, pos2, vec2,
};
use openpax_paint::{LayerKind, PlayDirection, Tag};

use crate::icons;
use crate::texture_editor::{TICK_MS, TextureEditor, ToolState, rgba32};
use crate::theme::{self, SPACE_S};
use crate::widgets::{self, Edge};

const CELL: f32 = 34.0;
const NAMES: f32 = 120.0;
const HEADER: f32 = 26.0;
const TAGS: f32 = 14.0;
const THUMB: f32 = 34.0;
const ROW: f32 = 16.0;

pub fn show(ui: &mut egui::Ui, rect: Rect, editor: &mut TextureEditor, tools: &mut ToolState) {
    widgets::toolbar_background(ui, rect, Edge::Top);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink(2.0))
            .layout(Layout::top_down(Align::Min)),
    );
    let ui = &mut child;
    // A fresh press starts a new undo step for duration drags.
    if ui.ctx().input(|i| i.pointer.any_pressed()) {
        editor.doc.end_coalescing();
    }
    egui::ScrollArea::horizontal()
        .id_salt("transport")
        .show(ui, |ui| transport(ui, editor, tools));
    egui::ScrollArea::both()
        .id_salt(("timeline", &editor.rel_path))
        .auto_shrink(false)
        .show(ui, |ui| frames(ui, editor));
}

fn transport(ui: &mut egui::Ui, editor: &mut TextureEditor, tools: &mut ToolState) {
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        ui.set_height(HEADER);
        ui.spacing_mut().item_spacing.x = 0.0;
        let count = editor.doc.frame_count();
        let current = editor.doc.current_frame();
        if widgets::icon_button(ui, icons::SKIP_BACK, "First frame (Home)").clicked() {
            editor.doc.set_current_frame(0);
        }
        if widgets::icon_button(ui, icons::CARET_LEFT, "Previous frame (,)").clicked() {
            editor.doc.set_current_frame((current + count - 1) % count);
        }
        let (glyph, tip) = if editor.playing {
            (icons::PAUSE, "Pause (Enter)")
        } else {
            (icons::PLAY, "Play (Enter)")
        };
        if widgets::icon_toggle(ui, editor.playing, glyph, tip).clicked() {
            editor.toggle_play(&ctx, &tools.view);
        }
        if widgets::icon_button(ui, icons::CARET_RIGHT, "Next frame (.)").clicked() {
            editor.doc.set_current_frame((current + 1) % count);
        }
        if widgets::icon_button(ui, icons::SKIP_FORWARD, "Last frame (End)").clicked() {
            editor.doc.set_current_frame(count - 1);
        }
        if widgets::icon_toggle(ui, tools.view.play_loop, icons::REPEAT, "Loop").clicked() {
            tools.view.play_loop ^= true;
        }
        egui::ComboBox::from_id_salt("play_dir")
            .width(90.0)
            .selected_text(tools.view.play_direction.name())
            .show_ui(ui, |ui| {
                for d in PlayDirection::ALL {
                    ui.selectable_value(&mut tools.view.play_direction, d, d.name());
                }
            })
            .response
            .on_hover_text("Playback direction (a tag's own direction wins inside the tag)");
        ui.add_space(SPACE_S);
        ui.separator();
        // Onion skin.
        if widgets::icon_toggle(ui, editor.onion_skin, icons::GHOST, "Onion skin (F3)").clicked() {
            editor.onion_skin ^= true;
        }
        ui.menu_button("▾", |ui| {
            let v = &mut tools.view;
            ui.horizontal(|ui| {
                ui.label("Frames before");
                ui.add(egui::DragValue::new(&mut v.onion_before).range(0..=8));
            });
            ui.horizontal(|ui| {
                ui.label("Frames after");
                ui.add(egui::DragValue::new(&mut v.onion_after).range(0..=8));
            });
            ui.add(egui::Slider::new(&mut v.onion_opacity, 0.05..=1.0).text("Opacity"));
            ui.checkbox(&mut v.onion_tint, "Tint (red before, blue after)");
        });
        ui.add_space(SPACE_S);
        ui.separator();
        // Frame operations.
        if widgets::icon_button(ui, icons::PLUS, "New empty frame (Alt+N)").clicked() {
            editor.doc.add_frame(false);
        }
        if widgets::icon_button(ui, icons::COPY, "Duplicate frame (Alt+D)").clicked() {
            editor.doc.add_frame(true);
        }
        if widgets::icon_button(
            ui,
            icons::LINK,
            "New frame linked to this one (shares pixels)",
        )
        .clicked()
        {
            editor.doc.add_linked_frame();
        }
        if widgets::icon_button_enabled(ui, count > 1, icons::TRASH, "Delete frame (Alt+Delete)")
            .clicked()
        {
            editor.doc.remove_frame();
        }
        let range = editor.doc.frame_range();
        let active = editor.doc.active();
        if widgets::icon_button_enabled(
            ui,
            range.is_some(),
            icons::LINK,
            "Link the active layer's cels across the selected frames",
        )
        .clicked()
            && let Some((a, b)) = range
        {
            editor.doc.link_cels(active, a, b);
        }
        let linked = editor.doc.is_linked(active, current);
        if widgets::icon_button_enabled(ui, linked, icons::UNLINK, "Give this cel its own pixels")
            .clicked()
        {
            editor.doc.unlink_cel(active, current);
        }
        if widgets::icon_button(ui, icons::TAG, "Tag the selected frames (or this one)").clicked() {
            let (from, to) = range.unwrap_or((current, current));
            let n = editor.doc.tags().len() + 1;
            editor.doc.add_tag(Tag {
                name: format!("Tag {n}"),
                from,
                to,
                direction: PlayDirection::Forward,
                color: TAG_COLORS[n % TAG_COLORS.len()],
            });
        }
        ui.add_space(SPACE_S);
        ui.separator();
        // Duration of the current frame.
        let mut ms = editor.doc.frames()[current].duration_ms;
        ui.label(RichText::new(icons::CLOCK).size(14.0));
        if ui
            .add(egui::DragValue::new(&mut ms).range(1..=60000).suffix(" ms"))
            .on_hover_text(format!(
                "How long frame {} shows ({} Minecraft ticks)",
                current + 1,
                (ms as f32 / TICK_MS as f32).round()
            ))
            .changed()
        {
            match range {
                Some((a, b)) if (a..=b).contains(&current) => {
                    for f in a..=b {
                        editor.doc.set_frame_duration(f, ms);
                    }
                }
                _ => editor.doc.set_frame_duration(current, ms),
            }
        }
        if ui
            .small_button("All")
            .on_hover_text("Give every frame this duration")
            .clicked()
        {
            editor.doc.set_all_durations(ms);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let total: u32 = editor.doc.frames().iter().map(|f| f.duration_ms).sum();
            ui.weak(format!("{count} frames · {:.2} s", total as f32 / 1000.0));
        });
    });
}

const TAG_COLORS: [[u8; 4]; 6] = [
    [230, 90, 90, 255],
    [90, 170, 230, 255],
    [110, 200, 110, 255],
    [230, 190, 70, 255],
    [180, 110, 230, 255],
    [240, 140, 60, 255],
];

fn frames(ui: &mut egui::Ui, editor: &mut TextureEditor) {
    let count = editor.doc.frame_count();
    let tree = editor.doc.layer_tree(true);
    let width = NAMES + CELL * count as f32 + 8.0;
    let height = TAGS + THUMB + ROW * tree.len() as f32 + 4.0;
    let (rect, _) = ui.allocate_exact_size(
        vec2(width.max(ui.available_width()), height),
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let tokens = theme::of(ui);
    let current = editor.doc.current_frame();
    let range = editor.doc.frame_range();
    let col = |f: usize| rect.left() + NAMES + f as f32 * CELL;
    let ctx = ui.ctx().clone();
    let shift = ctx.input(|i| i.modifiers.shift);

    // Tags.
    let mut tag_action: Option<(usize, TagAction)> = None;
    for (i, tag) in editor.doc.tags().iter().enumerate() {
        let r = Rect::from_min_max(
            pos2(col(tag.from) + 1.0, rect.top()),
            pos2(col(tag.to + 1) - 1.0, rect.top() + TAGS - 2.0),
        );
        painter.rect_filled(r, 3.0, rgba32(tag.color));
        painter.text(
            r.left_center() + vec2(3.0, 0.0),
            Align2::LEFT_CENTER,
            &tag.name,
            FontId::proportional(10.0),
            Color32::BLACK,
        );
        let response = ui.interact(r, ui.scope_id().with(("tag", i)), Sense::click());
        response.clone().on_hover_text(format!(
            "{} · frames {}–{} · {}\nRight-click to edit",
            tag.name,
            tag.from + 1,
            tag.to + 1,
            tag.direction.name()
        ));
        if response.clicked() {
            tag_action = Some((i, TagAction::Select));
        }
        let mut tag = tag.clone();
        response.context_menu(|ui| {
            ui.horizontal(|ui| {
                ui.label("Name");
                if ui.text_edit_singleline(&mut tag.name).changed() {
                    tag_action = Some((i, TagAction::Set(tag.clone())));
                }
            });
            for d in PlayDirection::ALL {
                if ui.radio(tag.direction == d, d.name()).clicked() {
                    tag.direction = d;
                    tag_action = Some((i, TagAction::Set(tag.clone())));
                }
            }
            ui.horizontal(|ui| {
                for c in TAG_COLORS {
                    let (r, resp) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
                    ui.painter().rect_filled(r, 2.0, rgba32(c));
                    if resp.clicked() {
                        tag.color = c;
                        tag_action = Some((i, TagAction::Set(tag.clone())));
                    }
                }
            });
            if ui
                .button(widgets::icon_label(icons::TRASH, "Delete tag"))
                .clicked()
            {
                tag_action = Some((i, TagAction::Delete));
                ui.close();
            }
        });
    }
    match tag_action {
        Some((i, TagAction::Select)) => {
            let t = editor.doc.tags()[i].clone();
            editor.doc.set_current_frame(t.from);
            editor
                .doc
                .set_frame_range((t.from < t.to).then_some((t.from, t.to)));
        }
        Some((i, TagAction::Set(t))) => editor.doc.set_tag(i, t),
        Some((i, TagAction::Delete)) => editor.doc.remove_tag(i),
        None => {}
    }

    // Frame thumbnails.
    let thumb_top = rect.top() + TAGS;
    let aspect = editor.doc.height() as f32 / editor.doc.width().max(1) as f32;
    let mut clicked_frame = None;
    let mut dropped_on = None;
    for f in 0..count {
        let cell = Rect::from_min_size(pos2(col(f), thumb_top), vec2(CELL, THUMB));
        let selected_range = range.is_some_and(|(a, b)| (a..=b).contains(&f));
        let fill = if f == current {
            ui.visuals().selection.bg_fill
        } else if selected_range {
            ui.visuals().selection.bg_fill.gamma_multiply(0.4)
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(cell.shrink(1.0), 3.0, fill);
        let inner = cell.shrink(4.0);
        let size = if aspect >= 1.0 {
            vec2(inner.height() / aspect, inner.height() - 10.0)
        } else {
            vec2(inner.width(), inner.width() * aspect)
        };
        let image = Rect::from_center_size(
            inner.center() - vec2(0.0, 5.0),
            size.min(vec2(inner.width(), inner.height() - 10.0)),
        );
        painter.rect_filled(image, 0.0, tokens.checker.gamma_multiply(0.6));
        let id = editor.frame_texture(&ctx, f);
        painter.image(
            id,
            image,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.text(
            pos2(cell.center().x, cell.bottom() - 2.0),
            Align2::CENTER_BOTTOM,
            (f + 1).to_string(),
            FontId::monospace(9.0),
            ui.visuals().text_color(),
        );
        let response = ui.interact(
            cell,
            ui.scope_id().with(("frame", f)),
            Sense::click_and_drag(),
        );
        if response.clicked() {
            clicked_frame = Some(f);
        }
        if response.drag_started() {
            editor.frame_drag = Some(f);
        }
        if editor.frame_drag.is_some() && response.contains_pointer() {
            painter.rect_stroke(
                cell,
                3.0,
                Stroke::new(2.0, tokens.accent),
                StrokeKind::Inside,
            );
            if ctx.input(|i| i.pointer.any_released()) {
                dropped_on = Some(f);
            }
        }
        let ms = editor.doc.frames()[f].duration_ms;
        let response = response.on_hover_text(format!(
            "Frame {} · {ms} ms\nClick to show, Shift-click to select a range, drag to reorder",
            f + 1
        ));
        response.context_menu(|ui| {
            if ui.button("Duplicate").clicked() {
                editor.doc.set_current_frame(f);
                editor.doc.add_frame(true);
                ui.close();
            }
            if ui.button("New linked frame").clicked() {
                editor.doc.set_current_frame(f);
                editor.doc.add_linked_frame();
                ui.close();
            }
            if ui.button("Copy to current frame").clicked() {
                editor.doc.copy_frame_to(f, current);
                ui.close();
            }
            if ui
                .add_enabled(count > 1, egui::Button::new("Delete"))
                .clicked()
            {
                editor.doc.set_frame_range(None);
                editor.doc.set_current_frame(f);
                editor.doc.remove_frame();
                ui.close();
            }
        });
    }
    if let Some(f) = clicked_frame {
        if shift {
            let anchor = range.map_or(current, |(a, _)| a);
            editor
                .doc
                .set_frame_range(Some((anchor.min(f), anchor.max(f))));
        } else {
            editor.doc.set_frame_range(None);
        }
        editor.doc.set_current_frame(f);
    }
    if let (Some(from), Some(to)) = (editor.frame_drag, dropped_on)
        && from != to
    {
        editor.doc.move_frame(from, to);
    }
    if ctx.input(|i| i.pointer.any_released()) {
        editor.frame_drag = None;
    }

    // Layer rows with their cels.
    let rows_top = thumb_top + THUMB;
    let active = editor.doc.active();
    let mut clicked_cel = None;
    let mut cel_action = None;
    for (row, &layer_index) in tree.iter().enumerate() {
        let y = rows_top + row as f32 * ROW;
        let layer = editor.doc.layers()[layer_index].clone();
        let depth = editor.doc.layer_depth(layer_index) as f32;
        let name_rect = Rect::from_min_size(pos2(rect.left(), y), vec2(NAMES, ROW));
        if layer_index == active {
            painter.rect_filled(
                name_rect,
                0.0,
                ui.visuals().selection.bg_fill.gamma_multiply(0.5),
            );
        }
        let mut name = layer.name.clone();
        if layer.kind == LayerKind::Group {
            name = format!("{} {name}", icons::FOLDER);
        }
        painter.text(
            pos2(rect.left() + 4.0 + depth * 8.0, y + ROW / 2.0),
            Align2::LEFT_CENTER,
            name,
            FontId::proportional(11.0),
            if layer.visible {
                ui.visuals().text_color()
            } else {
                ui.visuals().weak_text_color()
            },
        );
        if ui
            .interact(
                name_rect,
                ui.scope_id().with(("lname", layer_index)),
                Sense::click(),
            )
            .clicked()
        {
            editor.doc.set_active(layer_index);
        }
        if layer.kind == LayerKind::Group {
            continue;
        }
        for f in 0..count {
            let r = Rect::from_min_size(pos2(col(f), y), vec2(CELL, ROW)).shrink(1.0);
            let linked = editor.doc.is_linked(layer_index, f);
            let empty = editor.doc.cel(layer_index, f).is_clear();
            let here = f == current && layer_index == active;
            if here {
                painter.rect_stroke(r, 2.0, Stroke::new(1.5, tokens.accent), StrokeKind::Inside);
            }
            let c = r.center();
            let dot = ui.visuals().text_color();
            if empty {
                painter.circle_stroke(c, 3.0, Stroke::new(1.0, dot));
            } else {
                painter.circle_filled(c, 3.5, dot);
            }
            // Linked cels are joined by a line to the previous frame's.
            if linked
                && f > 0
                && editor.doc.cel_id(layer_index, f) == editor.doc.cel_id(layer_index, f - 1)
            {
                painter.hline(c.x - CELL..=c.x - 3.5, c.y, Stroke::new(1.5, tokens.accent));
            }
            let response = ui.interact(
                r,
                ui.scope_id().with(("cel", layer_index, f)),
                Sense::click(),
            );
            if response.clicked() {
                clicked_cel = Some((layer_index, f));
            }
            response.context_menu(|ui| {
                if f > 0 && ui.button("Link to previous frame").clicked() {
                    cel_action = Some((layer_index, f, true));
                    ui.close();
                }
                if linked && ui.button("Unlink").clicked() {
                    cel_action = Some((layer_index, f, false));
                    ui.close();
                }
            });
        }
    }
    if let Some((l, f)) = clicked_cel {
        editor.doc.set_active(l);
        editor.doc.set_current_frame(f);
    }
    match cel_action {
        Some((l, f, true)) => editor.doc.link_cels(l, f - 1, f),
        Some((l, f, false)) => editor.doc.unlink_cel(l, f),
        None => {}
    }
}

enum TagAction {
    Select,
    Set(Tag),
    Delete,
}
