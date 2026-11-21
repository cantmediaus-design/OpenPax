//! Inspector sections shown while a model editor tab is active.

use eframe::egui;
use openpax_core::{AssetKind, Pack};
use openpax_model::{Axis, Direction, Element, Face, ModelDocument, Rotation};
use serde_json::Value;

use crate::app::{App, model_mut};

/// Undo-coalescing keys: one slider drag or burst of typing becomes a single undo step.
const KEY_ELEMENT: u64 = 1;
const KEY_MODEL: u64 = 2;

impl App {
    pub(crate) fn model_sections(&mut self, ui: &mut egui::Ui) {
        let Some(pack) = &self.pack else { return };
        let Some(editor) = model_mut(&mut self.editors, self.active) else {
            return;
        };

        // A fresh press starts a new undo step.
        if ui.ctx().input(|i| i.pointer.any_pressed()) {
            editor.doc.end_coalescing();
        }

        let header = |title: &str| egui::CollapsingHeader::new(title).default_open(true);
        header("Elements").show(ui, |ui| elements_section(ui, &mut editor.doc));
        header("Selected element").show(ui, |ui| element_section(ui, &mut editor.doc));
        header("Model").show(ui, |ui| {
            model_section(ui, &mut editor.doc, pack);
            ui.separator();
            ui.checkbox(&mut editor.show_grid, "Show grid and block outline");
            if ui.button("Reset view").clicked() {
                editor.reset_view();
            }
        });
    }
}

fn elements_section(ui: &mut egui::Ui, doc: &mut ModelDocument) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Add cube").clicked() {
            let texture = doc
                .model()
                .textures
                .keys()
                .find(|k| k.as_str() != "particle")
                .map_or("#0".to_owned(), |k| format!("#{k}"));
            doc.add_element(Element::cube([4.0, 0.0, 4.0], [12.0, 8.0, 12.0], &texture));
        }
        let has_selection = doc.selected().is_some();
        if ui
            .add_enabled(has_selection, egui::Button::new("Duplicate"))
            .clicked()
        {
            doc.duplicate_selected();
        }
        if ui
            .add_enabled(has_selection, egui::Button::new("Delete"))
            .clicked()
        {
            doc.remove_selected();
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
        ui.weak("This model has no elements.");
    }
    egui::ScrollArea::vertical()
        .id_salt("element_list")
        .max_height(180.0)
        .show(ui, |ui| {
            for (index, label) in labels.into_iter().enumerate() {
                if ui
                    .selectable_label(doc.selected() == Some(index), label)
                    .clicked()
                {
                    doc.select(Some(index));
                }
            }
        });
}

fn vec3(ui: &mut egui::Ui, label: &str, value: &mut [f32; 3]) {
    ui.horizontal(|ui| {
        ui.label(label);
        for component in value {
            ui.add(
                egui::DragValue::new(component)
                    .speed(0.1)
                    .range(-16.0..=32.0),
            );
        }
    });
}

fn element_section(ui: &mut egui::Ui, doc: &mut ModelDocument) {
    let (Some(index), Some(original)) = (doc.selected(), doc.selected_element()) else {
        ui.weak("Click an element in the viewport or the list.");
        return;
    };
    let mut element = original.clone();

    let mut name = element.name.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Name");
        if ui
            .add(egui::TextEdit::singleline(&mut name).hint_text(element.label(index)))
            .changed()
        {
            element.name = (!name.is_empty()).then(|| name.clone());
        }
    });
    vec3(ui, "From", &mut element.from);
    vec3(ui, "To", &mut element.to);

    ui.separator();
    let mut rotated = element.rotation.is_some();
    if ui.checkbox(&mut rotated, "Rotation").changed() {
        element.rotation = rotated.then(Rotation::default);
    }
    if let Some(rotation) = &mut element.rotation {
        ui.horizontal(|ui| {
            ui.label("Axis");
            for axis in Axis::ALL {
                ui.selectable_value(&mut rotation.axis, axis, axis.name());
            }
            ui.label("Angle");
            ui.add(
                egui::DragValue::new(&mut rotation.angle)
                    .speed(0.5)
                    .range(-180.0..=180.0)
                    .suffix("°"),
            );
        });
        vec3(ui, "Pivot", &mut rotation.origin);
        let mut rescale = rotation.rescale == Some(true);
        if ui
            .checkbox(&mut rescale, "Rescale to fill the block")
            .changed()
        {
            rotation.rescale = rescale.then_some(true);
        }
        ui.weak("Older game versions only accept -45, -22.5, 0, 22.5 and 45 degrees.");
    }

    ui.separator();
    ui.label("Faces");
    for direction in Direction::ALL {
        face_row(ui, &mut element, direction);
    }

    if &element != original {
        doc.modify_coalesced(KEY_ELEMENT + ((index as u64) << 8), |model| {
            if let Some(slot) = model.elements.as_mut().and_then(|e| e.get_mut(index)) {
                *slot = element;
            }
        });
    }
}

fn face_row(ui: &mut egui::Ui, element: &mut Element, direction: Direction) {
    let mut present = element.faces.contains_key(&direction);
    ui.horizontal(|ui| {
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
        if let Some(face) = element.faces.get_mut(&direction) {
            ui.add(egui::TextEdit::singleline(&mut face.texture).desired_width(64.0))
                .on_hover_text("Texture variable, e.g. #0");
            let before = face.rotation.unwrap_or(0).rem_euclid(360) / 90;
            let mut turns = before;
            egui::ComboBox::from_id_salt(("face_rotation", direction))
                .width(52.0)
                .selected_text(format!("{}°", turns * 90))
                .show_ui(ui, |ui| {
                    for option in 0..4 {
                        ui.selectable_value(&mut turns, option, format!("{}°", option * 90));
                    }
                });
            if turns != before {
                face.rotation = (turns != 0).then_some(turns * 90);
            }
        }
    });

    let default_uv = element.default_uv(direction);
    if let Some(face) = element.faces.get_mut(&direction) {
        ui.horizontal(|ui| {
            ui.add_space(18.0);
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
                        ui.add(egui::DragValue::new(component).speed(0.1).range(0.0..=16.0));
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

fn model_section(ui: &mut egui::Ui, doc: &mut ModelDocument, pack: &Pack) {
    let mut parent = doc.model().parent.clone().unwrap_or_default();
    let mut textures = doc.model().textures.clone();

    ui.horizontal(|ui| {
        ui.label("Parent");
        ui.add(
            egui::TextEdit::singleline(&mut parent)
                .hint_text("none")
                .desired_width(f32::INFINITY),
        );
    });

    ui.label("Textures");
    let mut remove = None;
    for (key, value) in &mut textures {
        let Value::String(location) = value else {
            continue;
        };
        ui.horizontal(|ui| {
            if ui
                .small_button("✖")
                .on_hover_text("Remove this variable")
                .clicked()
            {
                remove = Some(key.clone());
            }
            ui.monospace(format!("#{key}"));
            egui::ComboBox::from_id_salt(("texture_pick", key.as_str()))
                .width(18.0)
                .selected_text("")
                .show_ui(ui, |ui| {
                    for entry in pack
                        .index
                        .entries
                        .iter()
                        .filter(|e| e.kind == AssetKind::Texture && e.category == "textures")
                    {
                        let path = entry.path.strip_suffix(".png").unwrap_or(&entry.path);
                        let candidate = format!("{}:{path}", entry.namespace);
                        if ui
                            .selectable_label(*location == candidate, &candidate)
                            .clicked()
                        {
                            *location = candidate;
                        }
                    }
                })
                .response
                .on_hover_text("Pick a texture from this pack");
            ui.add(
                egui::TextEdit::singleline(location)
                    .hint_text("namespace:path")
                    .desired_width(f32::INFINITY),
            );
        });
    }
    if let Some(key) = remove {
        textures.shift_remove(&key);
    }
    if ui.button("Add texture variable").clicked() {
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
