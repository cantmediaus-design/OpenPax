//! Editor tabs: the pixel canvas for textures and a plain text editor for everything else.

use eframe::egui::{
    self, Align, Color32, CursorIcon, Layout, Margin, PointerButton, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, TextureHandle, TextureOptions, UiBuilder, Vec2, pos2, vec2,
};
use openpax_core::{Pack, TextureMeta};
use openpax_paint::{Document, Palette, Rgba, ToolKind, ToolSettings};

use crate::commands::{Active, Command};
use crate::icons;
use crate::model_editor::ModelEditor;
use crate::theme::{self, RAIL_WIDTH, SPACE_M, SPACE_S, TOOLBAR_HEIGHT};
use crate::widgets::{self, Edge};

const ZOOM_LEVELS: [f32; 16] = [
    0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0, 64.0, 96.0, 128.0,
];
const MIN_ZOOM: f32 = ZOOM_LEVELS[0];
const MAX_ZOOM: f32 = ZOOM_LEVELS[ZOOM_LEVELS.len() - 1];
pub const MAX_BRUSH: u32 = 16;
const MAX_RECENT_COLORS: usize = 12;
/// Layer thumbnails of documents up to this many pixels refresh while a stroke is in progress.
const LIVE_THUMB_PIXELS: u32 = 256 * 256;
const LAYERS_JSON: &str = ".layers.json";
const LAYERS_PNG: &str = ".layers.png";

/// Tool selection shared by all texture editors.
pub struct ToolState {
    pub tool: ToolKind,
    /// Brush settings; `settings.color` is the primary colour.
    pub settings: ToolSettings,
    /// The colour the secondary mouse button paints with.
    pub secondary: Rgba,
    pub palette: Palette,
    /// Colours painted with lately, most recent first.
    pub recent: Vec<Rgba>,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            tool: ToolKind::Pencil,
            settings: ToolSettings::default(),
            secondary: [255, 255, 255, 255],
            palette: Palette::default(),
            recent: Vec::new(),
        }
    }
}

impl ToolState {
    pub fn swap_colors(&mut self) {
        std::mem::swap(&mut self.settings.color, &mut self.secondary);
    }

    /// Remembers a colour that was just painted with.
    pub fn note_color(&mut self, color: Rgba) {
        self.recent.retain(|c| *c != color);
        self.recent.insert(0, color);
        self.recent.truncate(MAX_RECENT_COLORS);
    }

    pub fn resize_brush(&mut self, delta: i32) {
        let size = self.settings.size as i32 + delta;
        self.settings.size = size.clamp(1, MAX_BRUSH as i32) as u32;
    }
}

/// Whether the tool's strokes are affected by the brush size.
fn uses_brush(tool: ToolKind) -> bool {
    matches!(
        tool,
        ToolKind::Pencil
            | ToolKind::Eraser
            | ToolKind::Line
            | ToolKind::Rectangle
            | ToolKind::Ellipse
    )
}

/// Whether the tool puts the current colour on the canvas.
fn paints(tool: ToolKind) -> bool {
    matches!(
        tool,
        ToolKind::Pencil
            | ToolKind::Fill
            | ToolKind::Line
            | ToolKind::Rectangle
            | ToolKind::Ellipse
    )
}

pub enum Editor {
    Texture(Box<TextureEditor>),
    Model(Box<ModelEditor>),
    Text(TextEditor),
}

impl Editor {
    pub fn kind(&self) -> Active {
        match self {
            Self::Texture(_) => Active::Texture,
            Self::Model(_) => Active::Model,
            Self::Text(_) => Active::Text,
        }
    }

    pub fn rel_path(&self) -> &str {
        match self {
            Self::Texture(e) => &e.rel_path,
            Self::Model(e) => &e.rel_path,
            Self::Text(e) => &e.rel_path,
        }
    }

    pub fn dirty(&self) -> bool {
        match self {
            Self::Texture(e) => e.dirty(),
            Self::Model(e) => e.dirty(),
            Self::Text(e) => e.dirty(),
        }
    }

    pub fn title(&self) -> String {
        let name = self.rel_path().rsplit('/').next().unwrap_or_default();
        let icon = match self {
            Self::Texture(_) => icons::IMAGE,
            Self::Model(_) => icons::CUBE,
            Self::Text(_) => icons::FILE_CODE,
        };
        if self.dirty() {
            format!("{icon} {name} •")
        } else {
            format!("{icon} {name}")
        }
    }

    pub fn save(&mut self, pack: &Pack) -> Result<(), String> {
        match self {
            Self::Texture(e) => e.save(pack),
            Self::Model(e) => e.save(pack),
            Self::Text(e) => e.save(pack),
        }
    }

    /// Whether the editor's own history can undo / redo (text tabs use the text box's).
    pub fn can_undo_redo(&self) -> (bool, bool) {
        match self {
            Self::Texture(e) => (e.doc.can_undo(), e.doc.can_redo()),
            Self::Model(e) => (e.doc.can_undo(), e.doc.can_redo()),
            Self::Text(_) => (false, false),
        }
    }

    pub fn undo(&mut self) {
        match self {
            Self::Texture(e) => drop(e.doc.undo()),
            Self::Model(e) => drop(e.doc.undo()),
            Self::Text(_) => {}
        }
    }

    pub fn redo(&mut self) {
        match self {
            Self::Texture(e) => drop(e.doc.redo()),
            Self::Model(e) => drop(e.doc.redo()),
            Self::Text(_) => {}
        }
    }
}

pub struct TextureEditor {
    pub rel_path: String,
    pub doc: Document,
    saved_revision: u64,
    pub meta: TextureMeta,
    saved_meta: TextureMeta,
    texture: Option<TextureHandle>,
    texture_revision: u64,
    layer_thumbs: Vec<TextureHandle>,
    layer_thumbs_revision: u64,
    pub zoom: f32,
    pan: Vec2,
    pub fit_pending: bool,
    pub show_grid: bool,
    pub tile_preview: bool,
    /// Pixel under the cursor, for the status bar.
    pub hover: Option<(i32, i32)>,
    /// The mouse button the stroke in progress was started with.
    stroke_button: Option<PointerButton>,
    /// Layer being renamed in the layers panel: `(index, text so far, focus requested)`.
    pub renaming: Option<(usize, String, bool)>,
}

impl TextureEditor {
    pub fn open(pack: &Pack, rel_path: &str) -> Result<Self, String> {
        let bytes = pack.read(rel_path).map_err(|e| e.to_string())?;
        let bitmap = openpax_paint::decode_png(&bytes).map_err(|e| e.to_string())?;
        let meta = pack
            .texture_meta(rel_path)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();

        // Layers live in a sidecar; use them only if they still match the PNG on disk.
        let layered = pack
            .read(&Pack::sidecar_path(rel_path, LAYERS_JSON))
            .ok()
            .zip(pack.read(&Pack::sidecar_path(rel_path, LAYERS_PNG)).ok())
            .and_then(|(json, png)| {
                openpax_paint::decode_layered(&String::from_utf8_lossy(&json), &png).ok()
            })
            .filter(|doc| doc.flatten() == bitmap);
        let doc = layered.unwrap_or_else(|| Document::from_bitmap(bitmap));

        Ok(Self {
            rel_path: rel_path.to_owned(),
            saved_revision: doc.revision(),
            doc,
            saved_meta: meta.clone(),
            meta,
            texture: None,
            texture_revision: u64::MAX,
            layer_thumbs: Vec::new(),
            layer_thumbs_revision: u64::MAX,
            zoom: 8.0,
            pan: Vec2::ZERO,
            fit_pending: true,
            show_grid: true,
            tile_preview: false,
            hover: None,
            stroke_button: None,
            renaming: None,
        })
    }

    pub fn dirty(&self) -> bool {
        self.doc.revision() != self.saved_revision || self.meta != self.saved_meta
    }

    pub fn save(&mut self, pack: &Pack) -> Result<(), String> {
        self.doc.stroke_end();
        let png = openpax_paint::encode_png(&self.doc.flatten()).map_err(|e| e.to_string())?;
        pack.write(&self.rel_path, &png)
            .map_err(|e| e.to_string())?;

        let json_path = Pack::sidecar_path(&self.rel_path, LAYERS_JSON);
        let png_path = Pack::sidecar_path(&self.rel_path, LAYERS_PNG);
        if self.doc.layers().len() > 1 {
            let (json, strip) =
                openpax_paint::encode_layered(&self.doc).map_err(|e| e.to_string())?;
            pack.write(&json_path, json.as_bytes())
                .map_err(|e| e.to_string())?;
            pack.write(&png_path, &strip).map_err(|e| e.to_string())?;
        } else {
            pack.remove(&json_path).map_err(|e| e.to_string())?;
            pack.remove(&png_path).map_err(|e| e.to_string())?;
        }

        if self.meta != self.saved_meta {
            pack.write_texture_meta(&self.rel_path, &self.meta)
                .map_err(|e| e.to_string())?;
            self.saved_meta = self.meta.clone();
        }
        self.saved_revision = self.doc.revision();
        Ok(())
    }

    /// The flattened document as a GPU texture, refreshed when the document changes.
    pub fn texture(&mut self, ctx: &egui::Context) -> &TextureHandle {
        let revision = self.doc.revision();
        if self.texture.is_none() || self.texture_revision != revision {
            let flat = self.doc.flatten();
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [flat.width() as usize, flat.height() as usize],
                flat.data(),
            );
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::NEAREST),
                None => {
                    let name = format!("doc:{}", self.rel_path);
                    self.texture = Some(ctx.load_texture(name, image, TextureOptions::NEAREST));
                }
            }
            self.texture_revision = revision;
        }
        self.texture.as_ref().expect("texture was just created")
    }

    /// One texture per layer (bottom first) for the layers panel.
    pub fn layer_textures(&mut self, ctx: &egui::Context) -> &[TextureHandle] {
        let revision = self.doc.revision();
        let stale = self.layer_thumbs_revision != revision
            || self.layer_thumbs.len() != self.doc.layers().len();
        // Big documents wait for the stroke to finish instead of re-uploading every frame.
        let live = self.doc.width() * self.doc.height() <= LIVE_THUMB_PIXELS;
        if stale && (live || !self.doc.stroke_active() || self.layer_thumbs.is_empty()) {
            self.layer_thumbs = self
                .doc
                .layers()
                .iter()
                .enumerate()
                .map(|(index, layer)| {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [
                            layer.pixels.width() as usize,
                            layer.pixels.height() as usize,
                        ],
                        layer.pixels.data(),
                    );
                    let name = format!("layer{index}:{}", self.rel_path);
                    ctx.load_texture(name, image, TextureOptions::NEAREST)
                })
                .collect();
            self.layer_thumbs_revision = revision;
        }
        &self.layer_thumbs
    }

    /// Steps to the next zoom level above (`direction > 0`) or below the current one,
    /// keeping the centre of the view in place.
    pub fn zoom_step(&mut self, direction: i32) {
        let current = self.zoom;
        let next = if direction > 0 {
            ZOOM_LEVELS.iter().copied().find(|z| *z > current * 1.01)
        } else {
            ZOOM_LEVELS
                .iter()
                .rev()
                .copied()
                .find(|z| *z < current * 0.99)
        };
        if let Some(next) = next {
            self.pan *= next / current;
            self.zoom = next;
        }
    }

    fn size(&self) -> Vec2 {
        vec2(self.doc.width() as f32, self.doc.height() as f32)
    }

    /// Screen position of the image's top-left corner.
    fn origin(&self, view: Rect) -> Pos2 {
        view.center() + self.pan - self.size() * self.zoom / 2.0
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, tools: &mut ToolState, checker: &TextureHandle) {
        let full = ui.available_rect_before_wrap();
        let (bar, rest) = full.split_top_bottom_at_y(full.top() + TOOLBAR_HEIGHT);
        let (rail, view) = rest.split_left_right_at_x(rest.left() + RAIL_WIDTH);
        self.canvas(ui, view, tools, checker);
        self.options_bar(ui, bar, tools);
        tool_rail(ui, rail, tools, checker);
    }

    /// The strip above the canvas: settings of the current tool on the left, view toggles
    /// on the right.
    fn options_bar(&mut self, ui: &mut egui::Ui, rect: Rect, tools: &mut ToolState) {
        widgets::toolbar_background(ui, rect, Edge::Bottom);
        let accent = theme::of(ui).accent;
        let mut bar = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(SPACE_M, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let ui = &mut bar;
        let tool = tools.tool;
        ui.label(widgets::icon(icons::tool(tool)).color(accent));
        ui.label(RichText::new(tool.name()).strong());
        ui.separator();

        let settings = &mut tools.settings;
        if uses_brush(tool) {
            ui.label("Size");
            ui.add(
                egui::Slider::new(&mut settings.size, 1..=MAX_BRUSH)
                    .suffix(" px")
                    .trailing_fill(true),
            )
            .on_hover_text("Brush size  ([ and ])");
        }
        if matches!(tool, ToolKind::Rectangle | ToolKind::Ellipse) {
            ui.toggle_value(&mut settings.shape_filled, "Filled")
                .on_hover_text("Fill the shape instead of drawing its outline");
        }
        if tool == ToolKind::Fill {
            ui.toggle_value(&mut settings.fill_contiguous, "Contiguous")
                .on_hover_text(
                    "On: fill the connected area only.\nOff: replace the colour everywhere.",
                );
        }
        if uses_brush(tool) {
            ui.separator();
            ui.toggle_value(&mut settings.mirror_x, "Mirror X")
                .on_hover_text("Mirror strokes left to right");
            ui.toggle_value(&mut settings.mirror_y, "Mirror Y")
                .on_hover_text("Mirror strokes top to bottom");
        }
        let hint = match tool {
            ToolKind::Picker => "Click picks the primary colour, right-click the secondary.",
            ToolKind::Select => "Drag to select an area. Click to deselect.",
            ToolKind::Move => "Drag to move the selection, or the whole layer.",
            _ => "",
        };
        if !hint.is_empty() {
            ui.weak(hint);
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = SPACE_S;
            let ctx = ui.ctx().clone();
            let tiling = Command::ToggleTiling.tooltip(&ctx);
            if widgets::icon_toggle(ui, self.tile_preview, icons::TILING, &tiling).clicked() {
                self.tile_preview = !self.tile_preview;
            }
            let grid = Command::ToggleGrid.tooltip(&ctx);
            if widgets::icon_toggle(ui, self.show_grid, icons::GRID, &grid).clicked() {
                self.show_grid = !self.show_grid;
            }
        });
    }

    fn canvas(
        &mut self,
        ui: &mut egui::Ui,
        view: Rect,
        tools: &mut ToolState,
        checker: &TextureHandle,
    ) {
        let tokens = theme::of(ui);
        let response = ui.allocate_rect(view, Sense::drag());
        let painter = ui.painter_at(view);
        painter.rect_filled(view, 0.0, ui.visuals().extreme_bg_color);

        let size = self.size();
        if self.fit_pending {
            let fit = ((view.width() - 48.0) / size.x).min((view.height() - 48.0) / size.y);
            self.zoom = if fit >= 1.0 { fit.floor() } else { fit }.clamp(MIN_ZOOM, MAX_ZOOM);
            self.pan = Vec2::ZERO;
            self.fit_pending = false;
        }

        let input = ui.ctx().input(|i| CanvasInput {
            scroll: i.smooth_scroll_delta.y,
            pinch: i.zoom_delta(),
            pointer: i.pointer.latest_pos(),
            space: i.key_down(egui::Key::Space),
            alt: i.modifiers.alt,
            primary_pressed: i.pointer.primary_pressed(),
            secondary_pressed: i.pointer.secondary_pressed(),
            primary_down: i.pointer.primary_down(),
            secondary_down: i.pointer.secondary_down(),
            middle_down: i.pointer.middle_down(),
        });
        let pointer = input.pointer;
        let over = response.contains_pointer();

        // Zoom towards the cursor.
        let factor = (input.scroll * 0.01).exp() * input.pinch;
        if over
            && factor != 1.0
            && let Some(pos) = pointer
        {
            let pixel = (pos - self.origin(view)) / self.zoom;
            self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
            let centered = view.center() - size * self.zoom / 2.0;
            self.pan = pos - pixel * self.zoom - centered;
        }

        // Pan with the middle button or space + drag.
        let panning = input.middle_down || input.space;
        if response.dragged_by(PointerButton::Middle)
            || (input.space && response.dragged_by(PointerButton::Primary))
        {
            self.pan += response.drag_delta();
        }

        let origin = self.origin(view);
        let zoom = self.zoom;
        let image_rect = Rect::from_min_size(origin, size * zoom);
        let to_screen = |x: f32, y: f32| origin + vec2(x, y) * zoom;

        // Transparency checkerboard (one cell per pixel), then the image.
        let checker_uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(size.x / 2.0, size.y / 2.0));
        painter.image(checker.id(), image_rect, checker_uv, tokens.checker);
        let full_uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let texture_id = self.texture(ui.ctx()).id();
        if self.tile_preview {
            for (dx, dy) in (-1..=1).flat_map(|y| (-1..=1).map(move |x| (x, y))) {
                if (dx, dy) != (0, 0) {
                    let offset = vec2(dx as f32 * size.x, dy as f32 * size.y) * zoom;
                    let tint = Color32::from_white_alpha(110);
                    painter.image(texture_id, image_rect.translate(offset), full_uv, tint);
                }
            }
        }
        painter.image(texture_id, image_rect, full_uv, Color32::WHITE);

        // Pixel grid.
        if self.show_grid && zoom >= 6.0 && size.x + size.y <= 1024.0 {
            let stroke = Stroke::new(1.0, tokens.pixel_grid);
            for x in 1..self.doc.width() {
                let sx = to_screen(x as f32, 0.0).x;
                painter.vline(sx, image_rect.y_range(), stroke);
            }
            for y in 1..self.doc.height() {
                let sy = to_screen(0.0, y as f32).y;
                painter.hline(image_rect.x_range(), sy, stroke);
            }
        }

        // Animation frame boundaries.
        if let Some(animation) = &self.meta.animation {
            let (fw, fh) = animation.frame_size(self.doc.width(), self.doc.height());
            let stroke = Stroke::new(1.5, tokens.frame_line);
            for x in (fw..self.doc.width()).step_by(fw as usize) {
                painter.vline(to_screen(x as f32, 0.0).x, image_rect.y_range(), stroke);
            }
            for y in (fh..self.doc.height()).step_by(fh as usize) {
                painter.hline(image_rect.x_range(), to_screen(0.0, y as f32).y, stroke);
            }
        }

        painter.rect_stroke(
            image_rect,
            0.0,
            Stroke::new(1.0, tokens.canvas_border),
            StrokeKind::Outside,
        );

        if let Some(sel) = self.doc.selection() {
            let rect = Rect::from_min_max(
                to_screen(sel.x as f32, sel.y as f32),
                to_screen((sel.x + sel.w) as f32, (sel.y + sel.h) as f32),
            );
            outline(&painter, rect, Color32::BLACK, Color32::WHITE);
        }

        // Tools.
        let pixel = pointer.map(|pos| {
            let p = (pos - origin) / zoom;
            (p.x.floor() as i32, p.y.floor() as i32)
        });
        self.hover = pixel.filter(|_| over);

        if over && panning {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        if over && !panning {
            // Holding Alt turns any tool into the colour picker.
            let tool = if input.alt {
                ToolKind::Picker
            } else {
                tools.tool
            };
            ui.ctx().set_cursor_icon(match tool {
                ToolKind::Move => CursorIcon::Move,
                _ => CursorIcon::Crosshair,
            });
            if let Some((x, y)) = pixel {
                let brush = if uses_brush(tool) {
                    tools.settings.size.max(1) as i32
                } else {
                    1
                };
                let (x0, y0) = (x - brush / 2, y - brush / 2);
                let rect = Rect::from_min_max(
                    to_screen(x0 as f32, y0 as f32),
                    to_screen((x0 + brush) as f32, (y0 + brush) as f32),
                );
                outline(&painter, rect, Color32::BLACK, Color32::WHITE);

                let button = if input.primary_pressed {
                    Some(PointerButton::Primary)
                } else if input.secondary_pressed {
                    Some(PointerButton::Secondary)
                } else {
                    None
                };
                if let Some(button) = button {
                    self.begin_stroke(tool, button, (x, y), tools);
                }
            }
        }
        if self.doc.stroke_active() {
            let down = match self.stroke_button {
                Some(PointerButton::Secondary) => input.secondary_down,
                _ => input.primary_down,
            };
            match pixel {
                Some(p) if down => self.doc.stroke_move(p),
                _ if !down => {
                    self.doc.stroke_end();
                    self.stroke_button = None;
                }
                _ => {}
            }
            ui.ctx().request_repaint();
        }
    }

    /// Starts a stroke with the primary colour (left button) or the secondary one (right).
    fn begin_stroke(
        &mut self,
        tool: ToolKind,
        button: PointerButton,
        at: (i32, i32),
        tools: &mut ToolState,
    ) {
        let secondary = button == PointerButton::Secondary;
        let mut settings = tools.settings;
        if secondary {
            settings.color = tools.secondary;
        }
        match self.doc.stroke_begin(tool, settings, at) {
            // The picker returns a colour instead of starting a stroke.
            Some(color) if color[3] != 0 => {
                if secondary {
                    tools.secondary = color;
                } else {
                    tools.settings.color = color;
                }
            }
            Some(_) => {}
            None => {
                self.stroke_button = Some(button);
                if paints(tool) {
                    tools.note_color(settings.color);
                }
            }
        }
    }
}

struct CanvasInput {
    scroll: f32,
    pinch: f32,
    pointer: Option<Pos2>,
    space: bool,
    alt: bool,
    primary_pressed: bool,
    secondary_pressed: bool,
    primary_down: bool,
    secondary_down: bool,
    middle_down: bool,
}

/// A two-tone outline that stays visible on any background.
fn outline(painter: &egui::Painter, rect: Rect, inner: Color32, outer: Color32) {
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, inner), StrokeKind::Inside);
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, outer), StrokeKind::Outside);
}

/// The vertical strip of tool buttons left of the canvas, with the colour wells below.
fn tool_rail(ui: &mut egui::Ui, rect: Rect, tools: &mut ToolState, checker: &TextureHandle) {
    widgets::toolbar_background(ui, rect, Edge::Right);
    let mut rail = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(0.0, SPACE_S)))
            .layout(Layout::top_down(Align::Center)),
    );
    let ui = &mut rail;
    ui.spacing_mut().item_spacing.y = 2.0;
    let ctx = ui.ctx().clone();
    for tool in ToolKind::ALL {
        let tooltip = Command::Tool(tool).tooltip(&ctx);
        if widgets::icon_toggle(ui, tools.tool == tool, icons::tool(tool), &tooltip).clicked() {
            tools.tool = tool;
        }
    }

    ui.add_space(SPACE_M);
    // Primary colour in front, secondary behind it.
    let (wells, response) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
    let side = vec2(18.0, 18.0);
    let back = Rect::from_min_size(wells.max - side, side);
    let front = Rect::from_min_size(wells.min, side);
    widgets::paint_swatch(ui, back, tools.secondary, false, checker);
    widgets::paint_swatch(ui, front, tools.settings.color, false, checker);
    response.on_hover_text(format!(
        "Primary {}\nSecondary {} (right mouse button)",
        widgets::hex(tools.settings.color),
        widgets::hex(tools.secondary)
    ));
    if widgets::icon_button(ui, icons::SWAP, &Command::SwapColors.tooltip(&ctx)).clicked() {
        tools.swap_colors();
    }
}

/// Result of checking a text tab's contents as JSON.
#[derive(Clone, Debug, PartialEq)]
enum JsonStatus {
    NotJson,
    Valid,
    /// Message plus 1-based line and column of the problem.
    Invalid(String, usize, usize),
}

pub struct TextEditor {
    pub rel_path: String,
    text: String,
    saved: String,
    status: JsonStatus,
    /// Right-aligned line numbers, rebuilt when the number of lines changes.
    gutter: String,
    gutter_lines: usize,
    /// Line (1-based) and character index to move the cursor to on the next frame.
    jump: Option<(usize, usize)>,
}

impl TextEditor {
    pub fn open(pack: &Pack, rel_path: &str) -> Result<Self, String> {
        let bytes = pack.read(rel_path).map_err(|e| e.to_string())?;
        let text =
            String::from_utf8(bytes).map_err(|_| "file is not valid UTF-8 text".to_owned())?;
        let mut editor = Self {
            rel_path: rel_path.to_owned(),
            saved: text.clone(),
            text,
            status: JsonStatus::NotJson,
            gutter: String::new(),
            gutter_lines: 0,
            jump: None,
        };
        editor.text_changed();
        Ok(editor)
    }

    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }

    pub fn save(&mut self, pack: &Pack) -> Result<(), String> {
        pack.write(&self.rel_path, self.text.as_bytes())
            .map_err(|e| e.to_string())?;
        self.saved.clone_from(&self.text);
        Ok(())
    }

    fn is_json(&self) -> bool {
        self.rel_path.ends_with(".json") || self.rel_path.ends_with(".mcmeta")
    }

    /// Re-validates the text and refreshes the line-number gutter.
    fn text_changed(&mut self) {
        self.status = if self.is_json() {
            match serde_json::from_str::<serde_json::Value>(&self.text) {
                Ok(_) => JsonStatus::Valid,
                Err(e) => {
                    // serde appends " at line L column C"; the toolbar shows that separately.
                    let message = e.to_string();
                    let message = message.split(" at line ").next().unwrap_or(&message);
                    JsonStatus::Invalid(message.to_owned(), e.line(), e.column())
                }
            }
        } else {
            JsonStatus::NotJson
        };

        let lines = self.text.split('\n').count();
        if lines != self.gutter_lines {
            let width = lines.to_string().len();
            self.gutter = (1..=lines)
                .map(|n| format!("{n:>width$}"))
                .collect::<Vec<_>>()
                .join("\n");
            self.gutter_lines = lines;
        }
    }

    /// Rewrites the text as consistently indented JSON. Returns false if it does not parse.
    fn format_json(&mut self) -> bool {
        match serde_json::from_str::<serde_json::Value>(&self.text) {
            Ok(value) => {
                self.text = openpax_model::to_pretty_json(&value);
                self.text_changed();
                true
            }
            Err(_) => false,
        }
    }

    /// Character index of a 1-based line and byte column, clamped to the text.
    fn char_index(&self, line: usize, column: usize) -> usize {
        let line_start: usize = self
            .text
            .split_inclusive('\n')
            .take(line.saturating_sub(1))
            .map(str::len)
            .sum();
        let mut byte = (line_start + column.saturating_sub(1)).min(self.text.len());
        while !self.text.is_char_boundary(byte) {
            byte -= 1;
        }
        self.text[..byte].chars().count()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        let bar = Rect::from_min_size(full.min, vec2(full.width(), TOOLBAR_HEIGHT));
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.allocate_rect(bar, Sense::hover());
        self.toolbar(ui, bar);

        let id = ui.scope_id().with("text");
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
        let mut scroll_to_line = None;
        if let Some((line, index)) = self.jump.take() {
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            let cursor = egui::text::CCursor::new(index);
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(cursor)));
            egui::TextEdit::store_state(ui.ctx(), id, state);
            ui.ctx().memory_mut(|m| m.request_focus(id));
            scroll_to_line = Some(line);
        }

        let margin = Margin::symmetric(8, 6);
        let gutter_fill = theme::of(ui).toolbar;
        let mut changed = false;
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            let available = ui.available_size();
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                egui::Frame::new()
                    .fill(gutter_fill)
                    .inner_margin(margin)
                    .show(ui, |ui| {
                        // A vertical layout, so the numbers start at the top like the text.
                        ui.vertical(|ui| {
                            ui.set_min_height(available.y - 2.0 * f32::from(margin.top));
                            let numbers = RichText::new(&self.gutter).font(font.clone()).weak();
                            ui.add(egui::Label::new(numbers).selectable(false).extend());
                        });
                    });
                let output = egui::TextEdit::multiline(&mut self.text)
                    .id(id)
                    .code_editor()
                    .frame(egui::Frame::NONE.inner_margin(margin))
                    .desired_width(f32::INFINITY)
                    .min_size(vec2(0.0, available.y))
                    .show(ui);
                changed = output.response.changed();
                if let Some(line) = scroll_to_line {
                    let top = output.response.rect.top()
                        + f32::from(margin.top)
                        + line.saturating_sub(1) as f32 * row_height;
                    let target = Rect::from_min_size(
                        pos2(output.response.rect.left(), top),
                        vec2(1.0, row_height),
                    );
                    ui.scroll_to_rect(target, Some(Align::Center));
                }
            });
        });
        if changed {
            self.text_changed();
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, rect: Rect) {
        widgets::toolbar_background(ui, rect, Edge::Bottom);
        let mut bar = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(SPACE_M, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let ui = &mut bar;
        let tokens = theme::of(ui);
        ui.label(widgets::icon(icons::FILE_CODE).color(tokens.accent));
        ui.label(RichText::new(if self.is_json() { "JSON" } else { "Text" }).strong());
        ui.separator();

        match self.status.clone() {
            JsonStatus::NotJson => {
                ui.weak(format!("{} lines", self.gutter_lines));
            }
            JsonStatus::Valid => {
                ui.label(widgets::icon(icons::SUCCESS).color(tokens.success));
                ui.weak("Valid JSON");
            }
            JsonStatus::Invalid(message, line, column) => {
                let color = ui.visuals().error_fg_color;
                ui.label(widgets::icon(icons::ERROR).color(color));
                let text =
                    RichText::new(format!("Line {line}, column {column}: {message}")).color(color);
                if ui
                    .add(egui::Button::new(text).frame_when_inactive(false))
                    .on_hover_text("Jump to the problem")
                    .clicked()
                {
                    self.jump = Some((line, self.char_index(line, column)));
                }
            }
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let can_format = self.status == JsonStatus::Valid;
            let button = egui::Button::new(widgets::icon_label(icons::FORMAT, "Format"));
            if ui
                .add_enabled(can_format, button)
                .on_hover_text("Re-indent the JSON")
                .on_disabled_hover_text("Only valid JSON can be formatted")
                .clicked()
            {
                self.format_json();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpax_core::Animation;
    use openpax_paint::Bitmap;

    const REL: &str = "assets/minecraft/textures/block/test.png";

    fn pack_with_texture() -> (tempfile::TempDir, Pack) {
        let dir = tempfile::tempdir().unwrap();
        let pack = Pack::create(dir.path().join("pack"), "test", 46).unwrap();
        let png = openpax_paint::encode_png(&Bitmap::new(4, 4)).unwrap();
        pack.write(REL, &png).unwrap();
        (dir, pack)
    }

    fn paint(editor: &mut TextureEditor, at: (i32, i32), color: [u8; 4]) {
        let settings = ToolSettings {
            color,
            ..Default::default()
        };
        editor.doc.stroke_begin(ToolKind::Pencil, settings, at);
        editor.doc.stroke_end();
    }

    #[test]
    fn save_and_reopen_keeps_pixels_layers_and_animation() {
        let (_dir, pack) = pack_with_texture();
        let mut editor = TextureEditor::open(&pack, REL).unwrap();
        assert!(!editor.dirty());

        paint(&mut editor, (0, 0), [255, 0, 0, 255]);
        editor.doc.add_layer();
        paint(&mut editor, (3, 3), [0, 0, 255, 255]);
        editor.meta.animation = Some(Animation {
            frametime: Some(5),
            ..Default::default()
        });
        assert!(editor.dirty());
        editor.save(&pack).unwrap();
        assert!(!editor.dirty());

        // The PNG Minecraft reads is the flattened image.
        let on_disk = openpax_paint::decode_png(&pack.read(REL).unwrap()).unwrap();
        assert_eq!(on_disk.get(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(on_disk.get(3, 3), Some([0, 0, 255, 255]));

        let reopened = TextureEditor::open(&pack, REL).unwrap();
        assert_eq!(
            reopened.doc.layers().len(),
            2,
            "layers come back from the sidecar"
        );
        assert_eq!(reopened.doc.flatten(), on_disk);
        assert_eq!(reopened.meta.animation.unwrap().frametime, Some(5));
    }

    #[test]
    fn stale_layer_sidecar_is_ignored() {
        let (_dir, pack) = pack_with_texture();
        let mut editor = TextureEditor::open(&pack, REL).unwrap();
        editor.doc.add_layer();
        paint(&mut editor, (1, 1), [0, 255, 0, 255]);
        editor.save(&pack).unwrap();

        // Another program overwrites the PNG; the saved layers no longer describe it.
        let other = openpax_paint::encode_png(&Bitmap::filled(4, 4, [9, 9, 9, 255])).unwrap();
        pack.write(REL, &other).unwrap();
        let reopened = TextureEditor::open(&pack, REL).unwrap();
        assert_eq!(reopened.doc.layers().len(), 1);
        assert_eq!(reopened.doc.flatten().get(1, 1), Some([9, 9, 9, 255]));
    }

    #[test]
    fn single_layer_save_removes_sidecar() {
        let (_dir, pack) = pack_with_texture();
        let mut editor = TextureEditor::open(&pack, REL).unwrap();
        editor.doc.add_layer();
        editor.save(&pack).unwrap();
        assert!(pack.exists(&Pack::sidecar_path(REL, LAYERS_JSON)));
        editor.doc.merge_down();
        editor.save(&pack).unwrap();
        assert!(!pack.exists(&Pack::sidecar_path(REL, LAYERS_JSON)));
        assert!(!pack.exists(&Pack::sidecar_path(REL, LAYERS_PNG)));
    }

    #[test]
    fn text_editor_round_trip() {
        let (_dir, pack) = pack_with_texture();
        pack.write("assets/minecraft/models/item/x.json", b"{}")
            .unwrap();
        let mut editor = TextEditor::open(&pack, "assets/minecraft/models/item/x.json").unwrap();
        editor.text.push('\n');
        assert!(editor.dirty());
        editor.save(&pack).unwrap();
        assert!(!editor.dirty());
        assert_eq!(
            pack.read("assets/minecraft/models/item/x.json").unwrap(),
            b"{}\n"
        );
    }

    #[test]
    fn json_errors_report_their_position_and_formatting_fixes_layout() {
        let (_dir, pack) = pack_with_texture();
        let rel = "assets/minecraft/models/item/y.json";
        pack.write(rel, b"{\n  \"a\": 1,\n  oops\n}").unwrap();
        let mut editor = TextEditor::open(&pack, rel).unwrap();
        let JsonStatus::Invalid(_, line, column) = editor.status.clone() else {
            panic!("expected a JSON error");
        };
        assert_eq!(line, 3);
        assert_eq!(editor.gutter, "1\n2\n3\n4");
        // The index points into the third line.
        let index = editor.char_index(line, column);
        assert!((12..=17).contains(&index), "{index}");
        assert!(!editor.format_json());

        editor.text = "{\"b\":[1,2],\"a\":{\"c\":true}}".to_owned();
        editor.text_changed();
        assert_eq!(editor.status, JsonStatus::Valid);
        assert!(editor.format_json());
        assert!(editor.text.contains('\n'));
        assert!(editor.text.find("\"b\"") < editor.text.find("\"a\""));
        assert!(editor.dirty());
    }

    #[test]
    fn zoom_steps_walk_the_ladder_and_stop_at_the_ends() {
        let (_dir, pack) = pack_with_texture();
        let mut editor = TextureEditor::open(&pack, REL).unwrap();
        editor.zoom = 7.0;
        editor.zoom_step(1);
        assert_eq!(editor.zoom, 8.0);
        editor.zoom_step(-1);
        assert_eq!(editor.zoom, 6.0);
        for _ in 0..40 {
            editor.zoom_step(1);
        }
        assert_eq!(editor.zoom, MAX_ZOOM);
        for _ in 0..40 {
            editor.zoom_step(-1);
        }
        assert_eq!(editor.zoom, MIN_ZOOM);
    }

    #[test]
    fn tool_state_tracks_recent_colours_and_swaps() {
        let mut tools = ToolState::default();
        let (primary, secondary) = (tools.settings.color, tools.secondary);
        tools.swap_colors();
        assert_eq!(
            (tools.settings.color, tools.secondary),
            (secondary, primary)
        );

        for i in 0..20u8 {
            tools.note_color([i, 0, 0, 255]);
        }
        tools.note_color([15, 0, 0, 255]);
        assert_eq!(tools.recent.len(), MAX_RECENT_COLORS);
        assert_eq!(tools.recent[0], [15, 0, 0, 255]);
        assert_eq!(tools.recent.iter().filter(|c| c[0] == 15).count(), 1);

        tools.resize_brush(-5);
        assert_eq!(tools.settings.size, 1);
        tools.resize_brush(100);
        assert_eq!(tools.settings.size, MAX_BRUSH);
    }
}
