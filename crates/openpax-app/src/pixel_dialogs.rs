//! Dialogs of the texture editor: resize and scale, rotate, filters and colour operations,
//! tilemaps and tilesets, export and batch export, animation import, scripts.

use eframe::egui::{self, RichText};
use openpax_core::{Pack, TextureMeta};
use openpax_paint::effects::{Effect, LayerEffect, OutlinePlace};
use openpax_paint::export::{AnimFrame, ExportFormat, ExportPreset, SheetLayout};
use openpax_paint::quantize::Dither;
use openpax_paint::tiles::{GridKind, Tilemap, Tileset};
use openpax_paint::{Document, Palette, Rgba, ScaleMethod, Scope, Transform};

use crate::app::{App, texture_mut};
use crate::dialogs::{footer, form, header, problem};
use crate::icons;
use crate::pixel_actions::scale_method_ui;
use crate::texture_editor::TextureEditor;
use crate::theme::{SPACE_M, SPACE_S};

pub enum PixelDialog {
    Resize {
        width: u32,
        height: u32,
        anchor: (f32, f32),
    },
    Scale {
        width: u32,
        height: u32,
        keep_ratio: bool,
        method: ScaleMethod,
        selection: bool,
        ratio: f32,
    },
    Rotate {
        degrees: f32,
        smooth: bool,
    },
    Filter {
        effect: Effect,
        scope: Scope,
    },
    ReplaceColor {
        from: Rgba,
        to: Rgba,
        tolerance: u8,
        scope: Scope,
    },
    PaletteSwap {
        from: Vec<Rgba>,
        to: Vec<Rgba>,
        scope: Scope,
    },
    Reduce {
        colors: usize,
        use_palette: bool,
        dither: Dither,
        /// A dither pattern by name, instead of dither.
        pattern: Option<String>,
        scope: Scope,
    },
    SaveSelection {
        name: String,
    },
    SaveBrush {
        name: String,
    },
    SavePattern {
        name: String,
    },
    NewTilemap {
        cols: u32,
        rows: u32,
        grid: GridKind,
        tile_w: u32,
        tile_h: u32,
        has_tileset: bool,
    },
    SliceTileset {
        tile_w: u32,
        tile_h: u32,
        offset: (i32, i32),
        spacing: (i32, i32),
        skip_empty: bool,
        dedupe: bool,
        from_file: bool,
    },
    Export {
        name: String,
        preset: ExportPreset,
        tags: Vec<String>,
        presets: Vec<ExportPreset>,
    },
    Batch {
        textures: Vec<(String, bool)>,
        filter: String,
        preset: ExportPreset,
        presets: Vec<ExportPreset>,
    },
    ImportAnimation {
        frames: Vec<AnimFrame>,
        path: String,
    },
    /// An image or animation dropped on the window.
    DropImage {
        frames: Vec<AnimFrame>,
        name: String,
        texture_open: bool,
        path: std::path::PathBuf,
    },
    Scripts {
        scripts: Vec<openpax_script::ScriptFile>,
        selected: Option<usize>,
        console: String,
        output: Vec<String>,
    },
}

fn load_presets() -> Vec<ExportPreset> {
    let dir = crate::user_files::dir("presets");
    let mut out: Vec<ExportPreset> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn save_preset(preset: &ExportPreset) -> Result<(), String> {
    let safe: String = preset
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = crate::user_files::dir("presets").join(format!("{safe}.json"));
    std::fs::write(
        path,
        serde_json::to_vec_pretty(preset).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

impl PixelDialog {
    pub fn scale(width: u32, height: u32, selection: bool) -> Self {
        Self::Scale {
            width,
            height,
            keep_ratio: true,
            method: ScaleMethod::Nearest,
            selection,
            ratio: width as f32 / height.max(1) as f32,
        }
    }

    pub fn filter(effect: Effect) -> Self {
        Self::Filter {
            effect,
            scope: Scope::Cel,
        }
    }

    pub fn new_tilemap(doc: &Document) -> Self {
        let (tw, th) = doc
            .extras
            .tileset
            .as_ref()
            .map_or((16, 16), |t| (t.tile_w, t.tile_h));
        Self::NewTilemap {
            cols: (doc.width() / tw).max(1),
            rows: (doc.height() / th).max(1),
            grid: GridKind::Rectangular,
            tile_w: tw,
            tile_h: th,
            has_tileset: doc.extras.tileset.is_some(),
        }
    }

    pub fn export(name: String, doc: &Document) -> Self {
        let preset = ExportPreset {
            file_name: "{name}".into(),
            format: if doc.frame_count() > 1 {
                ExportFormat::Gif
            } else {
                ExportFormat::Png
            },
            ..Default::default()
        };
        Self::Export {
            name,
            preset,
            tags: doc.tags().iter().map(|t| t.name.clone()).collect(),
            presets: load_presets(),
        }
    }

    pub fn batch(textures: Vec<String>) -> Self {
        Self::Batch {
            textures: textures.into_iter().map(|t| (t, false)).collect(),
            filter: String::new(),
            preset: ExportPreset {
                scale: 4,
                ..Default::default()
            },
            presets: load_presets(),
        }
    }

    pub fn scripts() -> Self {
        let scripts = openpax_script::list_scripts(&crate::user_files::scripts_dir());
        Self::Scripts {
            selected: (!scripts.is_empty()).then_some(0),
            scripts,
            console: String::new(),
            output: Vec::new(),
        }
    }

    /// Dialogs that need the wide layout.
    pub fn wide(&self) -> bool {
        matches!(
            self,
            Self::Export { .. }
                | Self::Batch { .. }
                | Self::Scripts { .. }
                | Self::PaletteSwap { .. }
        )
    }
}

pub fn color_button(ui: &mut egui::Ui, color: &mut Rgba) -> bool {
    let [r, g, b, a] = *color;
    let mut c = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
    let changed = ui.color_edit_button_srgba(&mut c).changed();
    if changed {
        *color = c.to_srgba_unmultiplied();
    }
    ui.monospace(crate::widgets::hex(*color));
    changed
}

fn scope_ui(ui: &mut egui::Ui, scope: &mut Scope) {
    ui.label("Apply to");
    egui::ComboBox::from_id_salt("scope")
        .selected_text(scope.name())
        .show_ui(ui, |ui| {
            for s in Scope::ALL {
                ui.selectable_value(scope, s, s.name());
            }
        });
    ui.end_row();
}

/// Editors for an effect's settings.
pub fn effect_ui(ui: &mut egui::Ui, effect: &mut Effect) {
    match effect {
        Effect::Outline {
            color,
            place,
            diagonal,
        } => {
            ui.horizontal(|ui| {
                ui.label("Colour");
                color_button(ui, color);
            });
            ui.horizontal(|ui| {
                ui.radio_value(place, OutlinePlace::Outside, "Outside");
                ui.radio_value(place, OutlinePlace::Inside, "Inside");
                ui.checkbox(diagonal, "Include corners");
            });
        }
        Effect::DropShadow { color, dx, dy } => {
            ui.horizontal(|ui| {
                ui.label("Colour");
                color_button(ui, color);
            });
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(dx).range(-32..=32).prefix("x "));
                ui.add(egui::DragValue::new(dy).range(-32..=32).prefix("y "));
            });
        }
        Effect::HueSaturation {
            hue,
            saturation,
            value,
        } => {
            ui.add(
                egui::Slider::new(hue, -180.0..=180.0)
                    .text("Hue")
                    .suffix("°"),
            );
            ui.add(egui::Slider::new(saturation, -1.0..=1.0).text("Saturation"));
            ui.add(egui::Slider::new(value, -1.0..=1.0).text("Brightness"));
        }
        Effect::BrightnessContrast {
            brightness,
            contrast,
        } => {
            ui.add(egui::Slider::new(brightness, -1.0..=1.0).text("Brightness"));
            ui.add(egui::Slider::new(contrast, -1.0..=1.0).text("Contrast"));
        }
        Effect::ColorOverlay { color, amount } => {
            ui.horizontal(|ui| {
                ui.label("Colour");
                color_button(ui, color);
            });
            ui.add(egui::Slider::new(amount, 0.0..=1.0).text("Amount"));
        }
        Effect::Invert | Effect::Desaturate => {
            ui.weak("No settings.");
        }
    }
}

impl App {
    pub(crate) fn pixel_dialog(&mut self, ui: &mut egui::Ui, dialog: &mut PixelDialog) -> bool {
        match dialog {
            PixelDialog::Resize {
                width,
                height,
                anchor,
            } => {
                header(ui, icons::RESIZE, "Resize Canvas");
                form(ui, "resize", |ui| {
                    ui.label("Size");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(width).range(1..=4096).suffix(" px"));
                        ui.label("×");
                        ui.add(egui::DragValue::new(height).range(1..=4096).suffix(" px"));
                        for (label, w, h) in [
                            ("16", 16, 16),
                            ("32", 32, 32),
                            ("64", 64, 64),
                            ("128", 128, 128),
                            ("16×32 strip", 16, 32),
                        ] {
                            if ui.small_button(label).clicked() {
                                (*width, *height) = (w, h);
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Anchor");
                    egui::Grid::new("anchor")
                        .spacing([2.0, 2.0])
                        .show(ui, |ui| {
                            for ay in [0.0, 0.5, 1.0] {
                                for ax in [0.0, 0.5, 1.0] {
                                    let selected = *anchor == (ax, ay);
                                    let glyph = if selected { "■" } else { "□" };
                                    if ui.selectable_label(selected, glyph).clicked() {
                                        *anchor = (ax, ay);
                                    }
                                }
                                ui.end_row();
                            }
                        });
                    ui.end_row();
                });
                ui.weak("The image keeps its place at the anchor; growing adds transparent pixels, shrinking crops.");
                let (confirm, cancel) = footer(ui, "Resize", true);
                if confirm && let Some(e) = texture_mut(&mut self.editors, self.active) {
                    e.doc.resize_canvas_anchored(*width, *height, *anchor);
                    e.fit_pending = true;
                }
                confirm || cancel
            }
            PixelDialog::Scale {
                width,
                height,
                keep_ratio,
                method,
                selection,
                ratio,
            } => {
                header(
                    ui,
                    icons::RESIZE,
                    if *selection {
                        "Scale Selection"
                    } else {
                        "Scale Image"
                    },
                );
                form(ui, "scale", |ui| {
                    ui.label("Size");
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::DragValue::new(width).range(1..=4096).suffix(" px"))
                            .changed()
                            && *keep_ratio
                        {
                            *height = ((*width as f32 / *ratio).round() as u32).max(1);
                        }
                        ui.label("×");
                        if ui
                            .add(egui::DragValue::new(height).range(1..=4096).suffix(" px"))
                            .changed()
                            && *keep_ratio
                        {
                            *width = ((*height as f32 * *ratio).round() as u32).max(1);
                        }
                        ui.checkbox(keep_ratio, "Keep proportions");
                    });
                    ui.end_row();
                    ui.label("Quick");
                    ui.horizontal(|ui| {
                        for f in [0.5f32, 2.0, 3.0, 4.0] {
                            if ui.small_button(format!("{f}×")).clicked() {
                                *width = ((*width as f32 * f).round() as u32).max(1);
                                *height = ((*height as f32 * f).round() as u32).max(1);
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Method");
                    scale_method_ui(ui, method);
                    ui.end_row();
                });
                ui.weak("Nearest keeps hard pixels. Smooth edges and RotSprite round off jagged diagonals when enlarging.");
                let (confirm, cancel) = footer(ui, "Scale", true);
                if confirm && let Some(e) = texture_mut(&mut self.editors, self.active) {
                    if *selection {
                        e.doc.transform_selection(Transform::Scale {
                            width: *width,
                            height: *height,
                            method: *method,
                        });
                    } else {
                        e.doc.scale_image(*width, *height, *method);
                        e.fit_pending = true;
                    }
                }
                confirm || cancel
            }
            PixelDialog::Rotate { degrees, smooth } => {
                header(ui, icons::REFRESH, "Rotate");
                ui.add(
                    egui::Slider::new(degrees, -180.0..=180.0)
                        .suffix("°")
                        .text("Angle (clockwise)"),
                );
                ui.checkbox(smooth, "RotSprite (cleaner edges)");
                ui.weak("Rotates the selection, or the layer if nothing is selected.");
                let (confirm, cancel) = footer(ui, "Rotate", true);
                if confirm && let Some(e) = texture_mut(&mut self.editors, self.active) {
                    e.doc.transform_selection(Transform::Rotate {
                        degrees: *degrees,
                        smooth: *smooth,
                    });
                }
                confirm || cancel
            }
            PixelDialog::Filter { effect, scope } => {
                header(ui, icons::SLIDERS, "Adjustments and Effects");
                form(ui, "filter", |ui| {
                    ui.label("Effect");
                    egui::ComboBox::from_id_salt("effect")
                        .selected_text(effect.name())
                        .show_ui(ui, |ui| {
                            for preset in Effect::presets() {
                                let name = preset.name();
                                if ui.selectable_label(effect.name() == name, name).clicked() {
                                    *effect = preset;
                                }
                            }
                        });
                    ui.end_row();
                    scope_ui(ui, scope);
                });
                ui.add_space(SPACE_S);
                effect_ui(ui, effect);
                ui.add_space(SPACE_S);
                let mut as_layer = false;
                if ui
                    .button(crate::widgets::icon_label(
                        icons::LAYERS,
                        "Add as Layer Effect",
                    ))
                    .on_hover_text("Keep it editable: the layer's pixels stay as they are")
                    .clicked()
                {
                    as_layer = true;
                }
                let (confirm, cancel) = footer(ui, "Apply", true);
                if let Some(e) = texture_mut(&mut self.editors, self.active) {
                    if as_layer {
                        let i = e.doc.active();
                        let effect = effect.clone();
                        e.doc.edit_layer(i, "Add layer effect", None, |l| {
                            l.effects.push(LayerEffect {
                                enabled: true,
                                effect,
                            })
                        });
                        return true;
                    }
                    if confirm {
                        let effect = effect.clone();
                        e.doc
                            .apply_filter(effect.name(), *scope, |b| effect.apply(b));
                    }
                }
                confirm || cancel
            }
            PixelDialog::ReplaceColor {
                from,
                to,
                tolerance,
                scope,
            } => {
                header(ui, icons::SLIDERS, "Replace Colour");
                form(ui, "replace", |ui| {
                    ui.label("Find");
                    ui.horizontal(|ui| color_button(ui, from));
                    ui.end_row();
                    ui.label("Replace with");
                    ui.horizontal(|ui| color_button(ui, to));
                    ui.end_row();
                    ui.label("Tolerance");
                    ui.add(egui::Slider::new(tolerance, 0..=255));
                    ui.end_row();
                    scope_ui(ui, scope);
                });
                let (confirm, cancel) = footer(ui, "Replace", true);
                if confirm && let Some(e) = texture_mut(&mut self.editors, self.active) {
                    let (f, t, tol) = (*from, *to, *tolerance);
                    e.doc.apply_filter("Replace colour", *scope, |b| {
                        let mut b = b.clone();
                        openpax_paint::effects::replace_color(&mut b, f, t, tol);
                        b
                    });
                }
                confirm || cancel
            }
            PixelDialog::PaletteSwap { from, to, scope } => {
                header(ui, icons::PALETTE, "Palette Swap");
                ui.label("Each colour on the left becomes the colour next to it.");
                ui.horizontal(|ui| {
                    if ui.button("Target: current palette").clicked() {
                        *to = self
                            .tools
                            .palette
                            .colors
                            .iter()
                            .copied()
                            .take(from.len())
                            .collect();
                        to.resize(from.len(), [0, 0, 0, 255]);
                    }
                    if ui.button("Reset").clicked() {
                        to.clone_from(from);
                    }
                });
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        egui::Grid::new("swap").show(ui, |ui| {
                            for (i, f) in from.iter().enumerate() {
                                let mut f = *f;
                                ui.add_enabled_ui(false, |ui| color_button(ui, &mut f));
                                ui.label("→");
                                if let Some(t) = to.get_mut(i) {
                                    ui.horizontal(|ui| color_button(ui, t));
                                }
                                ui.end_row();
                            }
                        });
                    });
                form(ui, "swap_scope", |ui| scope_ui(ui, scope));
                let (confirm, cancel) = footer(ui, "Swap", true);
                if confirm && let Some(e) = texture_mut(&mut self.editors, self.active) {
                    let (f, t) = (from.clone(), to.clone());
                    e.doc.apply_filter("Palette swap", *scope, |b| {
                        let mut b = b.clone();
                        openpax_paint::effects::palette_swap(&mut b, &f, &t);
                        b
                    });
                }
                confirm || cancel
            }
            PixelDialog::Reduce {
                colors,
                use_palette,
                dither,
                pattern,
                scope,
            } => {
                header(ui, icons::PALETTE, "Reduce Colours");
                form(ui, "reduce", |ui| {
                    ui.label("Target");
                    ui.vertical(|ui| {
                        ui.radio_value(use_palette, false, "A number of colours (best fit)");
                        ui.radio_value(
                            use_palette,
                            true,
                            format!(
                                "The current palette ({} colours)",
                                self.tools.palette.colors.len()
                            ),
                        );
                    });
                    ui.end_row();
                    if !*use_palette {
                        ui.label("Colours");
                        ui.add(egui::DragValue::new(colors).range(1..=256));
                        ui.end_row();
                    }
                    ui.label("Dithering");
                    let selected = match pattern {
                        Some(name) => format!("Pattern: {name}"),
                        None => dither.name().to_owned(),
                    };
                    egui::ComboBox::from_id_salt("dither")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for d in Dither::ALL {
                                if ui
                                    .selectable_label(pattern.is_none() && *dither == d, d.name())
                                    .clicked()
                                {
                                    *dither = d;
                                    *pattern = None;
                                }
                            }
                            ui.separator();
                            for p in &self.tools.patterns {
                                let on = pattern.as_deref() == Some(p.name.as_str());
                                if ui
                                    .selectable_label(on, format!("Pattern: {}", p.name))
                                    .clicked()
                                {
                                    *pattern = Some(p.name.clone());
                                }
                            }
                        });
                    ui.end_row();
                    scope_ui(ui, scope);
                });
                if pattern.is_some() {
                    ui.weak("The pattern's brightness levels are used as the dither thresholds.");
                }
                let (confirm, cancel) = footer(ui, "Reduce", true);
                let tile = pattern
                    .as_ref()
                    .and_then(|n| self.tools.patterns.iter().find(|p| &p.name == n))
                    .map(|p| p.image.clone());
                if confirm && let Some(e) = texture_mut(&mut self.editors, self.active) {
                    let palette: Vec<Rgba> = if *use_palette {
                        self.tools.palette.colors.clone()
                    } else {
                        openpax_paint::quantize::median_cut(&e.doc.flatten(), *colors)
                    };
                    let d = *dither;
                    e.doc
                        .apply_filter("Reduce colours", *scope, |b| match &tile {
                            Some(tile) => openpax_paint::quantize::remap_pattern(b, &palette, tile),
                            None => openpax_paint::quantize::remap(b, &palette, d),
                        });
                }
                confirm || cancel
            }
            PixelDialog::SavePattern { name } => {
                header(ui, icons::DITHER, "Save Dither Pattern");
                form(ui, "save_pattern", |ui| {
                    crate::dialogs::text_row(ui, "Name", name, true)
                });
                ui.weak(
                    "The selected pixels become a pattern tile: opaque pixels get the dither \
                     brush's second colour, and their brightness sets the order when reducing \
                     colours.",
                );
                let (confirm, cancel) = footer(ui, "Save", !name.trim().is_empty());
                if confirm && let Some(e) = texture_mut(&mut self.editors, self.active) {
                    let image = e.doc.copy_selected();
                    match self.tools.save_pattern(name.trim(), image) {
                        Ok(()) => {
                            let saved = self.tools.patterns.last().map(|p| p.name.clone());
                            self.tools.select_pattern(saved.as_deref());
                        }
                        Err(err) => self.error(format!("Could not save the pattern:\n{err}")),
                    }
                }
                confirm || cancel
            }
            PixelDialog::SaveSelection { name } => {
                header(ui, icons::SELECT, "Save Selection");
                form(ui, "save_sel", |ui| {
                    crate::dialogs::text_row(ui, "Name", name, true)
                });
                let (confirm, cancel) = footer(ui, "Save", !name.trim().is_empty());
                if confirm && let Some(e) = texture_mut(&mut self.editors, self.active) {
                    e.doc.save_selection(name.trim());
                }
                confirm || cancel
            }
            PixelDialog::SaveBrush { name } => {
                header(ui, icons::BRUSH, "Save Brush");
                form(ui, "save_brush", |ui| {
                    crate::dialogs::text_row(ui, "Name", name, true)
                });
                ui.weak(
                    "The selected pixels become a brush shape (transparent pixels are left out).",
                );
                let (confirm, cancel) = footer(ui, "Save", !name.trim().is_empty());
                if confirm && let Some(e) = texture_mut(&mut self.editors, self.active) {
                    let image = e.doc.copy_selected();
                    match self.tools.save_brush(name.trim(), image) {
                        Ok(()) => {
                            if let Some(b) = self.tools.brushes.last() {
                                self.tools.settings.brush = openpax_paint::BrushShape::Custom;
                                self.tools.settings.custom_brush = Some(b.image.clone());
                            }
                        }
                        Err(err) => self.error(format!("Could not save the brush:\n{err}")),
                    }
                }
                confirm || cancel
            }
            PixelDialog::NewTilemap {
                cols,
                rows,
                grid,
                tile_w,
                tile_h,
                has_tileset,
            } => {
                header(ui, icons::STAMP, "New Tilemap Layer");
                form(ui, "tilemap", |ui| {
                    if !*has_tileset {
                        ui.label("Tile size");
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(tile_w).range(1..=512));
                            ui.label("×");
                            ui.add(egui::DragValue::new(tile_h).range(1..=512));
                        });
                        ui.end_row();
                    }
                    ui.label("Map size");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(cols).range(1..=512).suffix(" columns"));
                        ui.add(egui::DragValue::new(rows).range(1..=512).suffix(" rows"));
                    });
                    ui.end_row();
                    ui.label("Grid");
                    egui::ComboBox::from_id_salt("grid_kind")
                        .selected_text(grid.name())
                        .show_ui(ui, |ui| {
                            for g in GridKind::ALL {
                                ui.selectable_value(grid, g, g.name());
                            }
                        });
                    ui.end_row();
                });
                if !*has_tileset {
                    ui.weak("An empty tileset is created; add tiles in the Tiles section.");
                }
                let (confirm, cancel) = footer(ui, "Create", true);
                if confirm && let Some(e) = texture_mut(&mut self.editors, self.active) {
                    if e.doc.extras.tileset.is_none() {
                        let (w, h) = (*tile_w, *tile_h);
                        e.doc
                            .edit_extras("New tileset", |x| x.tileset = Some(Tileset::new(w, h)));
                    }
                    e.doc.add_tilemap_layer(Tilemap::new(*grid, *cols, *rows));
                    self.tools.tool = openpax_paint::ToolKind::TileStamp;
                }
                confirm || cancel
            }
            PixelDialog::SliceTileset {
                tile_w,
                tile_h,
                offset,
                spacing,
                skip_empty,
                dedupe,
                from_file,
            } => {
                header(ui, icons::STAMP, "Slice Image into Tiles");
                form(ui, "slice", |ui| {
                    ui.label("Source");
                    ui.vertical(|ui| {
                        ui.radio_value(from_file, false, "The active layer");
                        ui.radio_value(from_file, true, "An image file…");
                    });
                    ui.end_row();
                    ui.label("Tile size");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(tile_w).range(1..=512));
                        ui.label("×");
                        ui.add(egui::DragValue::new(tile_h).range(1..=512));
                    });
                    ui.end_row();
                    ui.label("Offset");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut offset.0).range(0..=512));
                        ui.add(egui::DragValue::new(&mut offset.1).range(0..=512));
                    });
                    ui.end_row();
                    ui.label("Spacing");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut spacing.0).range(0..=64));
                        ui.add(egui::DragValue::new(&mut spacing.1).range(0..=64));
                    });
                    ui.end_row();
                    ui.label("");
                    ui.vertical(|ui| {
                        ui.checkbox(skip_empty, "Skip empty tiles");
                        ui.checkbox(dedupe, "Skip duplicate tiles");
                    });
                    ui.end_row();
                });
                let (confirm, cancel) = footer(ui, "Slice", true);
                if confirm {
                    let image = if *from_file {
                        rfd::FileDialog::new()
                            .add_filter("Images", &["png", "gif", "webp", "bmp"])
                            .pick_file()
                            .and_then(|p| std::fs::read(p).ok())
                            .and_then(|b| openpax_paint::decode_image(&b).ok())
                    } else {
                        texture_mut(&mut self.editors, self.active)
                            .map(|e| e.doc.layer_image(e.doc.active(), e.doc.current_frame()))
                    };
                    if let (Some(image), Some(e)) =
                        (image, texture_mut(&mut self.editors, self.active))
                    {
                        let tiles = Tileset::slice(
                            &image,
                            *tile_w,
                            *tile_h,
                            *offset,
                            *spacing,
                            *skip_empty,
                            *dedupe,
                        );
                        let n = tiles.len();
                        let (w, h) = (*tile_w, *tile_h);
                        e.doc.edit_extras("Slice tiles", |x| {
                            let set = x.tileset.get_or_insert_with(|| Tileset::new(w, h));
                            for t in tiles {
                                set.add_tile(t);
                            }
                        });
                        self.success(format!("Added {n} tiles"));
                    }
                }
                confirm || cancel
            }
            PixelDialog::Export {
                name,
                preset,
                tags,
                presets,
            } => {
                header(ui, icons::EXPORT, "Export");
                preset_ui(ui, preset, presets, tags);
                ui.add_space(SPACE_S);
                ui.label(
                    RichText::new(format!("Files: {}", preview_names(name, preset)))
                        .weak()
                        .monospace(),
                );
                let (confirm, cancel) = footer(ui, "Choose Folder…", true);
                if confirm
                    && let Some(dir) = rfd::FileDialog::new()
                        .set_title("Export into…")
                        .pick_folder()
                    && let Some(e) = texture_mut(&mut self.editors, self.active)
                {
                    e.doc.commit_floating();
                    match openpax_paint::export::export(&e.doc, name, preset) {
                        Ok(files) => {
                            let mut written = 0;
                            for (file, bytes) in files {
                                if let Err(err) = std::fs::write(dir.join(&file), bytes) {
                                    self.error(format!("Could not write {file}:\n{err}"));
                                    return true;
                                }
                                written += 1;
                            }
                            self.success(format!(
                                "Exported {written} file(s) to {}",
                                dir.display()
                            ));
                        }
                        Err(err) => self.error(format!("Export failed:\n{err}")),
                    }
                    return true;
                }
                confirm || cancel
            }
            PixelDialog::Batch {
                textures,
                filter,
                preset,
                presets,
            } => {
                header(ui, icons::EXPORT, "Batch Export Textures");
                ui.horizontal(|ui| {
                    ui.label(icons::SEARCH);
                    ui.add(
                        egui::TextEdit::singleline(filter)
                            .hint_text("Filter by path")
                            .desired_width(220.0),
                    );
                    if ui.small_button("Select shown").clicked() {
                        for (t, on) in textures.iter_mut() {
                            if t.contains(filter.as_str()) {
                                *on = true;
                            }
                        }
                    }
                    if ui.small_button("None").clicked() {
                        for (_, on) in textures.iter_mut() {
                            *on = false;
                        }
                    }
                });
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for (t, on) in textures
                            .iter_mut()
                            .filter(|(t, _)| t.contains(filter.as_str()))
                        {
                            ui.checkbox(on, RichText::new(t.as_str()).monospace());
                        }
                    });
                ui.add_space(SPACE_M);
                preset_ui(ui, preset, presets, &[]);
                let chosen: Vec<String> = textures
                    .iter()
                    .filter(|(_, on)| *on)
                    .map(|(t, _)| t.clone())
                    .collect();
                let (confirm, cancel) = footer(
                    ui,
                    &format!("Export {} Textures…", chosen.len()),
                    !chosen.is_empty(),
                );
                if confirm
                    && let Some(dir) = rfd::FileDialog::new()
                        .set_title("Export into…")
                        .pick_folder()
                    && let Some(pack) = &self.pack
                {
                    let (mut ok, mut failed) = (0, Vec::new());
                    for rel in &chosen {
                        let result = TextureEditor::open(pack, rel).and_then(|e| {
                            let name = rel
                                .rsplit('/')
                                .next()
                                .unwrap_or("texture")
                                .trim_end_matches(".png");
                            openpax_paint::export::export(&e.doc, name, preset)
                                .map_err(|err| err.to_string())
                        });
                        match result.and_then(|files| {
                            files.into_iter().try_for_each(|(file, bytes)| {
                                std::fs::write(dir.join(file), bytes).map_err(|e| e.to_string())
                            })
                        }) {
                            Ok(()) => ok += 1,
                            Err(e) => failed.push(format!("{rel}: {e}")),
                        }
                    }
                    if failed.is_empty() {
                        self.success(format!("Exported {ok} textures to {}", dir.display()));
                    } else {
                        self.error(format!(
                            "Exported {ok}; {} failed:\n{}",
                            failed.len(),
                            failed.join("\n")
                        ));
                    }
                    return true;
                }
                confirm || cancel
            }
            PixelDialog::ImportAnimation { frames, path } => {
                header(ui, icons::DOWNLOAD, "Import Animation");
                let (w, h) = frames
                    .first()
                    .map_or((0, 0), |f| (f.image.width(), f.image.height()));
                ui.label(format!("{} frame(s) of {w} × {h} px", frames.len()));
                form(ui, "import_anim", |ui| {
                    crate::dialogs::text_row(ui, "Texture path", path, true)
                });
                let rel_path = format!("assets/minecraft/textures/{path}.png");
                let exists = self.pack.as_ref().is_some_and(|p| p.exists(&rel_path));
                if exists {
                    problem(ui, "A texture with this name already exists.");
                } else {
                    ui.label(RichText::new(&rel_path).monospace().weak());
                }
                let (confirm, cancel) = footer(ui, "Import", !exists && !path.trim().is_empty());
                if confirm
                    && let Some(doc) =
                        openpax_paint::export::document_from_frames(std::mem::take(frames))
                {
                    let mut editor =
                        TextureEditor::with_doc(&rel_path, doc, TextureMeta::default());
                    let result = self.pack.as_ref().map(|p: &Pack| editor.save(p));
                    match result {
                        Some(Ok(())) => {
                            self.rescan();
                            self.open_asset(&rel_path);
                        }
                        Some(Err(e)) => self.error(format!("Could not import:\n{e}")),
                        None => {}
                    }
                }
                confirm || cancel
            }
            PixelDialog::DropImage {
                frames,
                name,
                texture_open,
                path,
            } => {
                header(ui, icons::DOWNLOAD, "Add Image");
                let (w, h) = frames
                    .first()
                    .map_or((0, 0), |f| (f.image.width(), f.image.height()));
                ui.label(format!("{name}: {w} × {h} px, {} frame(s)", frames.len()));
                ui.add_space(SPACE_S);
                let mut choice = None;
                ui.add_enabled_ui(*texture_open, |ui| {
                    if ui
                        .button(crate::widgets::icon_label(
                            icons::LAYERS,
                            "Add as a new layer",
                        ))
                        .clicked()
                    {
                        choice = Some(DropChoice::Layer);
                    }
                    if ui
                        .button(crate::widgets::icon_label(
                            icons::IMAGES,
                            "Add as a reference image",
                        ))
                        .on_hover_text("Shown while you work, never saved into the texture")
                        .clicked()
                    {
                        choice = Some(DropChoice::Reference);
                    }
                    if frames.len() > 1
                        && ui
                            .button(crate::widgets::icon_label(
                                icons::ANIMATION,
                                "Add its frames after the current frame",
                            ))
                            .clicked()
                    {
                        choice = Some(DropChoice::Frames);
                    }
                });
                if ui
                    .button(crate::widgets::icon_label(
                        icons::IMAGE,
                        "Make it a new texture in the pack",
                    ))
                    .clicked()
                {
                    choice = Some(DropChoice::Texture);
                }
                ui.add_space(SPACE_M);
                let cancel = ui.button("Cancel").clicked();
                let still_png = frames.len() == 1
                    && path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("png"));
                match choice {
                    Some(DropChoice::Texture) if still_png => {
                        self.open_dialog(crate::dialogs::Dialog::import_texture(path.clone()));
                        true
                    }
                    Some(DropChoice::Texture) => {
                        let frames = std::mem::take(frames);
                        self.open_pixel_dialog(PixelDialog::ImportAnimation {
                            frames,
                            path: format!("block/{name}"),
                        });
                        true
                    }
                    Some(choice) => {
                        if let Some(e) = texture_mut(&mut self.editors, self.active) {
                            let first = frames[0].image.clone();
                            match choice {
                                DropChoice::Layer => e.doc.paste(&first, (0, 0)),
                                DropChoice::Reference => e.doc.add_reference_layer(name, &first),
                                _ => {
                                    let (w, h) = (e.doc.width(), e.doc.height());
                                    let added: Vec<(openpax_paint::Bitmap, u32)> = frames
                                        .iter()
                                        .map(|f| (f.image.resized(w, h), f.duration_ms))
                                        .collect();
                                    e.doc.batch("Add frames", |doc| {
                                        for (image, ms) in &added {
                                            doc.add_frame(false);
                                            doc.set_frame_duration(doc.current_frame(), *ms);
                                            doc.paste_floating(image, (0, 0));
                                            doc.commit_floating();
                                        }
                                    });
                                }
                            }
                        }
                        true
                    }
                    None => cancel,
                }
            }
            PixelDialog::Scripts {
                scripts,
                selected,
                console,
                output,
            } => {
                header(ui, icons::SCRIPT, "Scripts");
                ui.horizontal(|ui| {
                    if ui
                        .button(crate::widgets::icon_label(
                            icons::FOLDER_OPEN,
                            "Open Folder",
                        ))
                        .clicked()
                    {
                        crate::user_files::reveal(&crate::user_files::scripts_dir());
                    }
                    if ui
                        .button(crate::widgets::icon_label(icons::REFRESH, "Reload"))
                        .clicked()
                    {
                        *scripts = openpax_script::list_scripts(&crate::user_files::scripts_dir());
                    }
                });
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .id_salt("script_list")
                    .show(ui, |ui| {
                        for (i, s) in scripts.iter().enumerate() {
                            let r = ui.selectable_label(*selected == Some(i), &s.name);
                            if r.clicked() {
                                *selected = Some(i);
                            }
                            if !s.description.is_empty() {
                                r.on_hover_text(&s.description);
                            }
                        }
                        if scripts.is_empty() {
                            ui.weak("No scripts yet. Put .rhai files in the scripts folder.");
                        }
                    });
                ui.add_space(SPACE_S);
                crate::widgets::caption(ui, "OR TYPE A SCRIPT (doc is the open texture)");
                ui.add(
                    egui::TextEdit::multiline(console)
                        .code_editor()
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .hint_text("doc.outline(rgb(0, 0, 0));"),
                );
                let mut run_source = None;
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            selected.is_some(),
                            egui::Button::new(crate::widgets::icon_label(
                                icons::PLAY,
                                "Run Selected",
                            )),
                        )
                        .clicked()
                        && let Some(s) = selected.and_then(|i| scripts.get(i))
                    {
                        run_source = std::fs::read_to_string(&s.path).ok().map(|src| {
                            (
                                s.name.clone(),
                                src,
                                s.path
                                    .parent()
                                    .map(std::path::Path::to_owned)
                                    .unwrap_or_default(),
                            )
                        });
                    }
                    if ui
                        .add_enabled(
                            !console.trim().is_empty(),
                            egui::Button::new(crate::widgets::icon_label(
                                icons::TERMINAL,
                                "Run Typed",
                            )),
                        )
                        .clicked()
                    {
                        run_source = Some((
                            "Script".into(),
                            console.clone(),
                            crate::user_files::scripts_dir(),
                        ));
                    }
                });
                if let Some((label, source, base)) = run_source
                    && let Some(e) = texture_mut(&mut self.editors, self.active)
                {
                    let out = openpax_script::run(&mut e.doc, &source, &label, &base);
                    *output = out.printed;
                    if let Some(err) = out.error {
                        output.push(format!("Error: {err}"));
                    } else {
                        output.push("Done.".into());
                    }
                }
                if !output.is_empty() {
                    egui::ScrollArea::vertical()
                        .max_height(120.0)
                        .id_salt("script_out")
                        .show(ui, |ui| {
                            for line in output.iter() {
                                ui.monospace(line);
                            }
                        });
                }
                ui.add_space(SPACE_M);
                ui.horizontal(|ui| ui.button("Close").clicked()).inner
            }
        }
    }
}

fn preview_names(name: &str, preset: &ExportPreset) -> String {
    let base = openpax_paint::export::frame_file_name(
        &preset.file_name,
        name,
        0,
        preset.tag.as_deref().unwrap_or(""),
    );
    match preset.format {
        ExportFormat::PngSequence => format!("{}_000.png, …", base.replace("{frame}", "")),
        ExportFormat::SpriteSheet if preset.json => format!("{base}.png, {base}.json"),
        f => format!("{base}.{}", f.extension()),
    }
}

/// The settings shared by the export and batch export dialogs.
fn preset_ui(
    ui: &mut egui::Ui,
    preset: &mut ExportPreset,
    presets: &mut Vec<ExportPreset>,
    tags: &[String],
) {
    ui.horizontal(|ui| {
        ui.label("Preset");
        egui::ComboBox::from_id_salt("preset")
            .selected_text(&preset.name)
            .show_ui(ui, |ui| {
                for p in presets.iter() {
                    if ui
                        .selectable_label(p.name == preset.name, &p.name)
                        .clicked()
                    {
                        *preset = p.clone();
                    }
                }
            });
        ui.add(egui::TextEdit::singleline(&mut preset.name).desired_width(120.0));
        if ui.small_button("Save preset").clicked() && save_preset(preset).is_ok() {
            *presets = load_presets();
        }
    });
    form(ui, "export_settings", |ui| {
        ui.label("Format");
        egui::ComboBox::from_id_salt("format")
            .selected_text(preset.format.name())
            .show_ui(ui, |ui| {
                for f in ExportFormat::ALL {
                    ui.selectable_value(&mut preset.format, f, f.name());
                }
            });
        ui.end_row();
        ui.label("Scale");
        ui.add(
            egui::DragValue::new(&mut preset.scale)
                .range(1..=32)
                .suffix("×"),
        );
        ui.end_row();
        ui.label("File name");
        ui.add(
            egui::TextEdit::singleline(&mut preset.file_name).hint_text("{name}, {tag}, {frame:3}"),
        );
        ui.end_row();
        if !tags.is_empty() {
            ui.label("Frames");
            egui::ComboBox::from_id_salt("tag")
                .selected_text(preset.tag.clone().unwrap_or_else(|| "All frames".into()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut preset.tag, None, "All frames");
                    for t in tags {
                        ui.selectable_value(&mut preset.tag, Some(t.clone()), t);
                    }
                });
            ui.end_row();
        }
        if preset.format == ExportFormat::SpriteSheet {
            let s = &mut preset.sheet;
            ui.label("Layout");
            ui.horizontal(|ui| {
                let mut columns = match s.layout {
                    SheetLayout::Grid { columns } => columns,
                    _ => 4,
                };
                ui.selectable_value(&mut s.layout, SheetLayout::Horizontal, "Row");
                ui.selectable_value(&mut s.layout, SheetLayout::Vertical, "Column");
                if ui
                    .selectable_label(matches!(s.layout, SheetLayout::Grid { .. }), "Grid")
                    .clicked()
                {
                    s.layout = SheetLayout::Grid { columns };
                }
                ui.selectable_value(&mut s.layout, SheetLayout::Packed, "Packed");
                if let SheetLayout::Grid { .. } = s.layout
                    && ui
                        .add(
                            egui::DragValue::new(&mut columns)
                                .range(1..=64)
                                .suffix(" columns"),
                        )
                        .changed()
                {
                    s.layout = SheetLayout::Grid { columns };
                }
            });
            ui.end_row();
            ui.label("Spacing");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut s.spacing)
                        .range(0..=64)
                        .prefix("between "),
                );
                ui.add(
                    egui::DragValue::new(&mut s.border)
                        .range(0..=64)
                        .prefix("border "),
                );
                ui.add(
                    egui::DragValue::new(&mut s.extrude)
                        .range(0..=16)
                        .prefix("extrude "),
                );
            });
            ui.end_row();
            ui.label("");
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.trim, "Trim");
                ui.checkbox(&mut s.dedupe, "Merge duplicates");
                ui.checkbox(&mut s.power_of_two, "Power of two");
                ui.checkbox(&mut preset.json, "JSON data");
            });
            ui.end_row();
        }
    });
}

/// Brings a palette into the active document as its indexed palette.
pub fn use_as_document_palette(doc: &mut Document, palette: &Palette) {
    let colors = palette.colors.clone();
    doc.edit_extras("Document palette", |e| e.palette = colors);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DropChoice {
    Layer,
    Reference,
    Frames,
    Texture,
}
