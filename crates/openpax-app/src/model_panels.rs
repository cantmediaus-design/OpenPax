//! Inspector sections shown while a model editor tab is active.

use eframe::egui::{self, Align, Layout, RichText, Sense, vec2};
use openpax_core::{AssetKind, Pack, ResourceLocation};
use openpax_model::{Axis, Direction, Element, Face, ModelDocument, ModelFile, Rotation};
use serde_json::Value;

use crate::app::{App, model_mut};
use crate::commands::Command;
use crate::explorer::{self, Thumbnails};
use crate::theme::{ROW_HEIGHT, SPACE_M, SPACE_S};
use crate::{icons, widgets};

/// Undo-coalescing keys: one slider drag or burst of typing becomes a single undo step.
const KEY_ELEMENT: u64 = 1;
const KEY_MODEL: u64 = 2;
const THUMB: f32 = 18.0;
const LIST_HEIGHT: f32 = 160.0;
/// Element coordinates the game accepts.
const COORDS: std::ops::RangeInclusive<f32> = -16.0..=32.0;

/// The element being renamed in the list: `(index, text so far, focus requested)`.
type Renaming = Option<(usize, String, bool)>;

impl App {
    pub(crate) fn model_sections(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut run = None;
        {
            let Self {
                pack,
                editors,
                thumbs,
                active,
                ..
            } = self;
            let Some(pack) = pack else { return };
            let Some(editor) = model_mut(editors, *active) else {
                return;
            };
            let doc = &mut editor.doc;

            // A fresh press starts a new undo step.
            if ctx.input(|i| i.pointer.any_pressed()) {
                doc.end_coalescing();
            }

            widgets::section(ui, icons::CUBE, "Elements", |ui| {
                run = elements_section(ui, doc);
            });
            widgets::section(ui, icons::RESIZE, "Selected Element", |ui| {
                element_section(ui, doc, pack, thumbs);
            });
            widgets::section(ui, icons::IMAGE, "Model Textures", |ui| {
                model_section(ui, doc, pack, thumbs);
            });
        }
        if let Some(command) = run {
            self.run(command, &ctx);
        }
    }
}

/// The element list. Returns a command when one of its toolbar buttons was clicked.
fn elements_section(ui: &mut egui::Ui, doc: &mut ModelDocument) -> Option<Command> {
    let ctx = ui.ctx().clone();
    let mut run = None;
    let has_selection = doc.selected().is_some();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (command, enabled) in [
            (Command::AddCube, true),
            (Command::DuplicateElement, has_selection),
            (Command::DeleteElement, has_selection),
        ] {
            let tooltip = command.tooltip(&ctx);
            if widgets::icon_button_enabled(ui, enabled, command.icon(), &tooltip).clicked() {
                run = Some(command);
            }
        }
    });

    let labels: Vec<String> = doc
        .model()
        .elements()
        .iter()
        .enumerate()
        .map(|(i, e)| e.label(i))
        .collect();
    if labels.is_empty() {
        ui.weak("This model has no elements of its own.");
        return run;
    }

    let height_id = egui::Id::unique("element_list_height");
    let rename_id = egui::Id::unique("element_rename");
    let mut height = ui.data_mut(|d| *d.get_persisted_mut_or(height_id, LIST_HEIGHT));
    let mut renaming: Renaming = ui.data(|d| d.get_temp(rename_id)).flatten();
    let mut rename_to = None;

    egui::ScrollArea::vertical()
        .id_salt("element_list")
        .max_height(height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (index, label) in labels.into_iter().enumerate() {
                let selected = doc.selected() == Some(index);
                let (rect, response) = widgets::list_row(ui, selected);
                let mut row = widgets::row_ui(ui, rect, 0.0);
                row.label(RichText::new(icons::CUBE).weak());
                match &mut renaming {
                    Some((renamed, text, focused)) if *renamed == index => {
                        let field =
                            row.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                        if !std::mem::replace(focused, true) {
                            field.request_focus();
                        }
                        if field.lost_focus() {
                            // Enter and clicking away keep the name; Escape discards it.
                            if !row.input(|i| i.key_pressed(egui::Key::Escape)) {
                                rename_to = Some((index, text.trim().to_owned()));
                            }
                            renaming = None;
                        }
                    }
                    _ => {
                        let color = widgets::row_text_color(ui, selected);
                        row.add(
                            egui::Label::new(RichText::new(&label).color(color))
                                .selectable(false)
                                .truncate(),
                        );
                    }
                }
                if response.clicked() {
                    doc.select(Some(index));
                }
                if response.double_clicked() {
                    let name = doc.model().elements()[index].name.clone();
                    renaming = Some((index, name.unwrap_or_default(), false));
                }
                response.on_hover_text("Double-click to rename");
            }
        });
    ui.data_mut(|d| d.insert_temp(rename_id, renaming));
    if let Some((index, name)) = rename_to {
        doc.modify(|model| {
            if let Some(element) = model.elements.as_mut().and_then(|e| e.get_mut(index)) {
                element.name = (!name.is_empty()).then_some(name);
            }
        });
    }

    // A grip under the list changes how much room it gets.
    let (grip, response) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::drag());
    let response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let color = if response.hovered() || response.dragged() {
        ui.visuals().widgets.hovered.fg_stroke.color
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke.color
    };
    let centre = grip.center();
    ui.painter().hline(
        centre.x - 14.0..=centre.x + 14.0,
        centre.y,
        egui::Stroke::new(2.0, color),
    );
    if response.dragged() {
        height = (height + response.drag_delta().y).clamp(ROW_HEIGHT * 2.0, 600.0);
        ui.data_mut(|d| d.insert_persisted(height_id, height));
    }
    run
}

/// The pack-relative PNG a texture reference (`#variable` or `namespace:path`) points at.
fn texture_path(model: &ModelFile, reference: &str) -> Option<String> {
    let mut current = reference.to_owned();
    // Variables may refer to other variables; give up on cycles.
    for _ in 0..8 {
        match current.strip_prefix('#') {
            Some(key) => current = model.textures.get(key)?.as_str()?.to_owned(),
            None => {
                let location: ResourceLocation = current.parse().ok()?;
                return Some(format!(
                    "assets/{}/textures/{}.png",
                    location.namespace(),
                    location.path()
                ));
            }
        }
    }
    None
}

/// A small preview of the texture a reference resolves to.
fn texture_thumb(
    ui: &mut egui::Ui,
    model: &ModelFile,
    reference: &str,
    pack: &Pack,
    thumbs: &mut Thumbnails,
) {
    match texture_path(model, reference).filter(|path| pack.exists(path)) {
        Some(path) => thumbs.show(ui, pack, &path, THUMB, icons::IMAGE),
        None => {
            ui.add_sized(
                [THUMB, THUMB],
                egui::Label::new(RichText::new(icons::WARNING).color(ui.visuals().warn_fg_color))
                    .selectable(false),
            )
            .on_hover_text("This texture is not in the pack");
        }
    }
}

fn element_section(
    ui: &mut egui::Ui,
    doc: &mut ModelDocument,
    pack: &Pack,
    thumbs: &mut Thumbnails,
) {
    let (Some(index), Some(original)) = (doc.selected(), doc.selected_element()) else {
        ui.weak("Click an element in the viewport or the list.");
        return;
    };
    let mut element = original.clone();
    let variables: Vec<String> = doc
        .model()
        .textures
        .keys()
        .map(|key| format!("#{key}"))
        .collect();

    let mut name = element.name.clone().unwrap_or_default();
    widgets::field(ui, "Name", |ui| {
        let field = egui::TextEdit::singleline(&mut name)
            .hint_text(element.label(index))
            .desired_width(f32::INFINITY);
        if ui.add(field).changed() {
            element.name = (!name.is_empty()).then(|| name.clone());
        }
    });
    widgets::vec3(ui, "From", &mut element.from, COORDS);
    widgets::vec3(ui, "To", &mut element.to, COORDS);
    let mut size: [f32; 3] = std::array::from_fn(|i| element.to[i] - element.from[i]);
    if widgets::vec3(ui, "Size", &mut size, 0.0..=48.0) {
        // Resizing keeps the "from" corner where it is.
        element.to = std::array::from_fn(|i| {
            (element.from[i] + size[i]).clamp(*COORDS.start(), *COORDS.end())
        });
    }

    ui.add_space(SPACE_M);
    let mut rotated = element.rotation.is_some();
    if ui.checkbox(&mut rotated, "Rotation").changed() {
        element.rotation = rotated.then(Rotation::default);
    }
    if let Some(rotation) = &mut element.rotation {
        widgets::field(ui, "Axis", |ui| {
            for axis in Axis::ALL {
                ui.selectable_value(&mut rotation.axis, axis, axis.name().to_uppercase());
            }
            ui.add(
                egui::DragValue::new(&mut rotation.angle)
                    .speed(0.5)
                    .range(-180.0..=180.0)
                    .suffix("°"),
            )
            .on_hover_text("Older game versions only accept -45, -22.5, 0, 22.5 and 45 degrees");
        });
        widgets::vec3(ui, "Pivot", &mut rotation.origin, COORDS);
        let mut rescale = rotation.rescale == Some(true);
        if ui
            .checkbox(&mut rescale, "Rescale to fill the block")
            .changed()
        {
            rotation.rescale = rescale.then_some(true);
        }
    }

    ui.add_space(SPACE_M);
    widgets::caption(ui, "FACES");
    for direction in Direction::ALL {
        face_row(
            ui,
            doc.model(),
            &mut element,
            direction,
            &variables,
            pack,
            thumbs,
        );
    }

    if &element != original {
        doc.modify_coalesced(KEY_ELEMENT + ((index as u64) << 8), |model| {
            if let Some(slot) = model.elements.as_mut().and_then(|e| e.get_mut(index)) {
                *slot = element;
            }
        });
    }
}

fn face_row(
    ui: &mut egui::Ui,
    model: &ModelFile,
    element: &mut Element,
    direction: Direction,
    variables: &[String],
    pack: &Pack,
    thumbs: &mut Thumbnails,
) {
    const NAME_WIDTH: f32 = 72.0;
    let mut present = element.faces.contains_key(&direction);
    ui.horizontal(|ui| {
        let height = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            vec2(NAME_WIDTH, height),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(NAME_WIDTH);
                if ui.checkbox(&mut present, direction.name()).changed() {
                    if present {
                        let texture = element
                            .faces
                            .values()
                            .next()
                            .map_or("#0".to_owned(), |f| f.texture.clone());
                        element.faces.insert(
                            direction,
                            Face {
                                texture,
                                ..Default::default()
                            },
                        );
                    } else {
                        element.faces.remove(&direction);
                    }
                }
            },
        );
        let Some(face) = element.faces.get_mut(&direction) else {
            return;
        };
        texture_thumb(ui, model, &face.texture, pack, thumbs);
        egui::ComboBox::from_id_salt(("face_texture", direction))
            .width(76.0)
            .selected_text(RichText::new(&face.texture).monospace())
            .show_ui(ui, |ui| {
                for variable in variables {
                    let text = RichText::new(variable).monospace();
                    ui.selectable_value(&mut face.texture, variable.clone(), text);
                }
                if variables.is_empty() {
                    ui.weak("Add a texture variable below first.");
                }
            })
            .response
            .on_hover_text("Texture variable for this face");

        let before = face.rotation.unwrap_or(0).rem_euclid(360) / 90;
        let mut turns = before;
        egui::ComboBox::from_id_salt(("face_rotation", direction))
            .width(56.0)
            .selected_text(format!("{}°", turns * 90))
            .show_ui(ui, |ui| {
                for option in 0..4 {
                    ui.selectable_value(&mut turns, option, format!("{}°", option * 90));
                }
            })
            .response
            .on_hover_text("Texture rotation");
        if turns != before {
            face.rotation = (turns != 0).then_some(turns * 90);
        }
    });

    let default_uv = element.default_uv(direction);
    if let Some(face) = element.faces.get_mut(&direction) {
        ui.horizontal(|ui| {
            ui.add_space(NAME_WIDTH + ui.spacing().item_spacing.x);
            ui.spacing_mut().item_spacing.x = SPACE_S;
            let mut custom = face.uv.is_some();
            if ui
                .checkbox(&mut custom, "UV")
                .on_hover_text("Off: derived from the element's size")
                .changed()
            {
                face.uv = custom.then_some(default_uv);
            }
            match &mut face.uv {
                Some(uv) => {
                    for component in uv {
                        ui.add(
                            egui::DragValue::new(component)
                                .speed(0.1)
                                .range(0.0..=16.0)
                                .max_decimals(2),
                        );
                    }
                }
                None => {
                    let [a, b, c, d] = default_uv;
                    ui.weak(format!("auto  {a} {b} {c} {d}"));
                }
            }
        });
    }
}

fn model_section(ui: &mut egui::Ui, doc: &mut ModelDocument, pack: &Pack, thumbs: &mut Thumbnails) {
    let mut parent = doc.model().parent.clone().unwrap_or_default();
    let mut textures = doc.model().textures.clone();

    widgets::field(ui, "Parent", |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut parent)
                .hint_text("none")
                .desired_width(f32::INFINITY),
        )
        .on_hover_text("The model this one inherits from, e.g. block/cube_all");
    });

    ui.add_space(SPACE_S);
    widgets::caption(ui, "TEXTURE VARIABLES");
    let mut remove = None;
    for (key, value) in &mut textures {
        let Value::String(location) = value else {
            continue;
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SPACE_S;
            texture_thumb(ui, doc.model(), &format!("#{key}"), pack, thumbs);
            ui.allocate_ui_with_layout(
                vec2(52.0, ui.spacing().interact_size.y),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(format!("#{key}")).monospace()).truncate(),
                    );
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if widgets::icon_button(ui, icons::TRASH, "Remove this variable").clicked() {
                    remove = Some(key.clone());
                }
                texture_picker(ui, key, location, pack, thumbs);
                ui.add_space(SPACE_S);
                ui.add(
                    egui::TextEdit::singleline(location)
                        .hint_text("namespace:path or #variable")
                        .desired_width(f32::INFINITY),
                );
            });
        });
    }
    if let Some(key) = remove {
        textures.shift_remove(&key);
    }
    if ui
        .button(widgets::icon_label(icons::PLUS, "Add texture variable"))
        .clicked()
    {
        textures.insert(
            doc.model().unused_texture_name(),
            Value::String(String::new()),
        );
    }

    let parent = (!parent.is_empty()).then_some(parent);
    if parent != doc.model().parent || textures != doc.model().textures {
        doc.modify_coalesced(KEY_MODEL, |model| {
            model.parent = parent;
            model.textures = textures;
        });
    }
}

/// A button that opens a searchable list of the pack's textures and writes the chosen one
/// into `location`.
fn texture_picker(
    ui: &mut egui::Ui,
    key: &str,
    location: &mut String,
    pack: &Pack,
    thumbs: &mut Thumbnails,
) {
    let button = widgets::icon_button(ui, icons::SEARCH, "Pick a texture from this pack");
    let search_id = egui::Id::unique(("texture_search", key));
    egui::Popup::menu(&button)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(300.0);
            let mut query: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
            let field = ui.add(
                egui::TextEdit::singleline(&mut query)
                    .hint_text(widgets::icon_label(icons::SEARCH, "Search textures…"))
                    .desired_width(f32::INFINITY),
            );
            if ui.memory(|m| m.focused().is_none()) {
                field.request_focus();
            }
            ui.data_mut(|d| d.insert_temp(search_id, query.clone()));

            let needle = query.trim().to_lowercase();
            let candidates: Vec<_> = pack
                .index
                .entries
                .iter()
                .filter(|e| e.kind == AssetKind::Texture && e.category == "textures")
                .filter(|e| needle.is_empty() || e.rel_path.to_lowercase().contains(&needle))
                .collect();
            if candidates.is_empty() {
                ui.weak("No matching textures in this pack.");
                return;
            }
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().max_height(280.0).show_rows(
                ui,
                ROW_HEIGHT,
                candidates.len(),
                |ui, range| {
                    for entry in &candidates[range] {
                        let candidate = explorer::resource_location(entry);
                        let selected = *location == candidate;
                        let (rect, response) = widgets::list_row(ui, selected);
                        let mut row = widgets::row_ui(ui, rect, 0.0);
                        thumbs.show(&mut row, pack, &entry.rel_path, THUMB, icons::IMAGE);
                        let color = widgets::row_text_color(ui, selected);
                        row.add(
                            egui::Label::new(RichText::new(&candidate).color(color))
                                .selectable(false)
                                .truncate(),
                        );
                        if response.clicked() {
                            *location = candidate;
                            ui.close();
                        }
                    }
                },
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_references_resolve_through_variables() {
        let mut model = ModelFile::default();
        model
            .textures
            .insert("all".into(), "minecraft:block/stone".into());
        model.textures.insert("side".into(), "#all".into());
        model.textures.insert("loop_a".into(), "#loop_b".into());
        model.textures.insert("loop_b".into(), "#loop_a".into());

        let stone = Some("assets/minecraft/textures/block/stone.png".to_owned());
        assert_eq!(texture_path(&model, "#side"), stone);
        assert_eq!(texture_path(&model, "minecraft:block/stone"), stone);
        assert_eq!(texture_path(&model, "#missing"), None);
        assert_eq!(texture_path(&model, "#loop_a"), None, "cycles end");
    }
}
