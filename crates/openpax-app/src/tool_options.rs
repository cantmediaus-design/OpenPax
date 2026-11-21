//! The strip of tool settings above the canvas and the tool rail beside it.

use eframe::egui::{self, Align, Layout, Rect, RichText, Sense, TextureHandle, UiBuilder, vec2};
use openpax_paint::{BrushShape, SelectMode, ToolKind};

use crate::commands::Command;
use crate::icons;
use crate::pixel_cmds::PixelCmd;
use crate::texture_editor::{MAX_BRUSH, TextureEditor, Tiling, ToolState};
use crate::theme::{self, SPACE_M, SPACE_S};
use crate::widgets::{self, Edge};

/// Tools shown in the rail, grouped; the rest are reached through their group's menu.
const RAIL: [&[ToolKind]; 9] = [
    &[ToolKind::Pencil, ToolKind::Shade, ToolKind::Dither],
    &[ToolKind::Eraser],
    &[ToolKind::Fill],
    &[ToolKind::Picker],
    &[ToolKind::Line, ToolKind::Polyline],
    &[ToolKind::Rectangle, ToolKind::Ellipse, ToolKind::Polygon],
    &[ToolKind::Select, ToolKind::Lasso, ToolKind::MagicWand],
    &[ToolKind::Move],
    &[ToolKind::TileStamp],
];

pub fn options_bar(
    ui: &mut egui::Ui,
    rect: Rect,
    editor: &mut TextureEditor,
    tools: &mut ToolState,
) {
    widgets::toolbar_background(ui, rect, Edge::Bottom);
    let accent = theme::of(ui).accent;
    let mut bar = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(SPACE_M, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut bar;
    ui.spacing_mut().item_spacing.x = SPACE_S;
    let tool = tools.tool;
    ui.label(widgets::icon(icons::tool(tool)).color(accent));
    ui.label(RichText::new(tool.name()).strong());
    ui.separator();

    egui::ScrollArea::horizontal()
        .max_width(ui.available_width() - 140.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| tool_settings(ui, editor, tools));
        });

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = SPACE_S;
        let ctx = ui.ctx().clone();
        let tiling = Command::ToggleTiling.tooltip(&ctx);
        if widgets::icon_toggle(ui, tools.view.tiling != Tiling::Off, icons::TILING, &tiling)
            .clicked()
        {
            tools.view.tiling = tools.view.tiling.next();
        }
        let grid = Command::ToggleGrid.tooltip(&ctx);
        if widgets::icon_toggle(ui, editor.show_grid, icons::GRID, &grid).clicked() {
            editor.show_grid = !editor.show_grid;
        }
        let rulers = Command::Pixel(PixelCmd::ToggleRulers).tooltip(&ctx);
        if widgets::icon_toggle(ui, tools.view.rulers, icons::RULER, &rulers).clicked() {
            tools.view.rulers = !tools.view.rulers;
        }
        let onion = Command::Pixel(PixelCmd::ToggleOnionSkin).tooltip(&ctx);
        if widgets::icon_toggle(ui, editor.onion_skin, icons::GHOST, &onion).clicked() {
            editor.onion_skin = !editor.onion_skin;
        }
    });
}

fn tool_settings(ui: &mut egui::Ui, editor: &mut TextureEditor, tools: &mut ToolState) {
    let tool = tools.tool;
    let s = &mut tools.settings;
    if tool.uses_brush() {
        ui.label("Size");
        ui.add(
            egui::DragValue::new(&mut s.size)
                .range(1..=MAX_BRUSH)
                .suffix(" px")
                .speed(0.2),
        )
        .on_hover_text("Brush size  ([ and ])");
        egui::ComboBox::from_id_salt("brush_shape")
            .width(84.0)
            .selected_text(match s.brush {
                BrushShape::Square => "Square",
                BrushShape::Circle => "Round",
                BrushShape::Custom => "Custom",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut s.brush, BrushShape::Square, "Square");
                ui.selectable_value(&mut s.brush, BrushShape::Circle, "Round");
                if !tools.brushes.is_empty() {
                    ui.separator();
                }
                for b in &tools.brushes {
                    let selected = s.brush == BrushShape::Custom
                        && s.custom_brush
                            .as_ref()
                            .is_some_and(|c| std::sync::Arc::ptr_eq(c, &b.image));
                    if ui.selectable_label(selected, &b.name).clicked() {
                        s.brush = BrushShape::Custom;
                        s.custom_brush = Some(b.image.clone());
                    }
                }
            })
            .response
            .on_hover_text("Brush shape. Save the selection as a brush in the Brushes section.");
        if s.brush == BrushShape::Custom {
            ui.toggle_value(&mut s.brush_tinted, "Tint").on_hover_text(
                "Paint the brush shape in the current colour instead of its own colours",
            );
        }
        if matches!(
            tool,
            ToolKind::Pencil | ToolKind::Eraser | ToolKind::Shade | ToolKind::Dither
        ) {
            ui.toggle_value(&mut s.pixel_perfect, "Pixel-perfect")
                .on_hover_text("Remove doubled corner pixels from 1-pixel freehand lines");
        }
    }
    match tool {
        ToolKind::Rectangle | ToolKind::Ellipse | ToolKind::Polygon => {
            ui.toggle_value(&mut s.shape_filled, "Filled")
                .on_hover_text("Fill the shape instead of drawing its outline");
            ui.weak("Shift: square/circle · Ctrl: from centre");
        }
        ToolKind::Line => {
            ui.weak("Shift snaps to clean angles");
        }
        ToolKind::Polyline => {
            ui.weak("Click points · double-click, Enter or right-click to finish · Esc cancels");
        }
        ToolKind::Fill | ToolKind::MagicWand => {
            ui.toggle_value(&mut s.fill_contiguous, "Contiguous")
                .on_hover_text("On: the connected area only.\nOff: every matching pixel.");
            ui.label("Tolerance");
            ui.add(egui::DragValue::new(&mut s.tolerance).range(0..=255));
            ui.toggle_value(&mut s.diagonal, "Diagonal")
                .on_hover_text("Also spread across diagonal neighbours");
            ui.toggle_value(&mut s.sample_merged, "All layers")
                .on_hover_text("Look at every visible layer instead of only the active one");
        }
        ToolKind::Picker => {
            ui.toggle_value(&mut s.pick_active_layer, "Active layer only")
                .on_hover_text("Pick from the active layer instead of what you see");
            ui.weak("Click: primary · right-click: secondary · Alt with any tool");
        }
        ToolKind::Shade => {
            ui.weak("Left lightens, right darkens, along");
            let source = if s.ramp.is_empty() {
                "the palette"
            } else {
                "the ramp"
            };
            ui.label(source);
            if ui
                .small_button("Use palette")
                .on_hover_text("Shade along the palette's colours, in order")
                .clicked()
            {
                s.ramp.clear();
            }
        }
        ToolKind::Dither => {
            ui.label("Density");
            ui.add(egui::Slider::new(&mut s.dither_level, 0..=16));
            let [r, g, b, a] = s.dither_color;
            let mut c = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
            if ui
                .color_edit_button_srgba(&mut c)
                .on_hover_text("Second dither colour")
                .changed()
            {
                s.dither_color = c.to_srgba_unmultiplied();
            }
            if ui
                .small_button("Secondary")
                .on_hover_text("Use the secondary colour")
                .clicked()
            {
                s.dither_color = tools.secondary;
            }
        }
        ToolKind::Select | ToolKind::Lasso => {
            select_modes(ui, &mut s.select_mode);
            ui.weak("Shift adds, Alt subtracts");
        }
        ToolKind::Move => {
            ui.weak("Drag or use the arrow keys (Shift: 10 px). Enter applies.");
            if editor.doc.floating().is_some() {
                let ctx = ui.ctx().clone();
                let _ = ctx;
                if ui
                    .button(widgets::icon_label(icons::CHECK, "Apply"))
                    .clicked()
                {
                    editor.doc.commit_floating();
                }
            }
        }
        ToolKind::TileStamp => tile_options(ui, editor, tools),
        _ => {}
    }
    if tools.tool == ToolKind::MagicWand {
        select_modes(ui, &mut tools.settings.select_mode);
    }
    let s = &mut tools.settings;
    if tool.uses_brush() || tool == ToolKind::Fill {
        ui.separator();
        ui.menu_button(widgets::icon_label(icons::FLIP_H, "Symmetry"), |ui| {
            ui.checkbox(&mut s.mirror_x, "Mirror left / right");
            ui.checkbox(&mut s.mirror_y, "Mirror top / bottom");
            ui.checkbox(&mut s.symmetry.diagonal, "Mirror across the ╲ diagonal");
            ui.checkbox(
                &mut s.symmetry.anti_diagonal,
                "Mirror across the ╱ diagonal",
            );
            ui.horizontal(|ui| {
                ui.label("Radial copies");
                ui.add(egui::DragValue::new(&mut s.symmetry.radial).range(0..=12));
            });
            let mut custom = s.symmetry.center.is_some();
            if ui.checkbox(&mut custom, "Custom centre").changed() {
                s.symmetry.center = custom.then(|| {
                    (
                        editor.doc.width() as f32 / 2.0,
                        editor.doc.height() as f32 / 2.0,
                    )
                });
            }
            if let Some((cx, cy)) = &mut s.symmetry.center {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(cx).speed(0.5).prefix("x "));
                    ui.add(egui::DragValue::new(cy).speed(0.5).prefix("y "));
                });
            }
        });
        ui.toggle_value(&mut s.wrap, "Wrap").on_hover_text(
            "Strokes that leave one edge come back at the opposite edge (seamless tiles)",
        );
        ui.menu_button("Pen", |ui| {
            ui.checkbox(&mut s.pressure_size, "Pressure changes size");
            ui.checkbox(&mut s.pressure_opacity, "Pressure changes opacity");
            ui.horizontal(|ui| {
                ui.label("Response");
                ui.add(egui::Slider::new(&mut tools.view.pressure_curve, 0.3..=3.0));
            });
        });
    }
}

fn select_modes(ui: &mut egui::Ui, mode: &mut SelectMode) {
    for (m, label, tip) in [
        (SelectMode::Replace, "New", "Replace the selection"),
        (SelectMode::Add, "Add", "Add to the selection (Shift)"),
        (
            SelectMode::Subtract,
            "Subtract",
            "Remove from the selection (Alt)",
        ),
        (
            SelectMode::Intersect,
            "Intersect",
            "Keep only the overlap (Shift+Alt)",
        ),
    ] {
        ui.selectable_value(mode, m, label).on_hover_text(tip);
    }
}

fn tile_options(ui: &mut egui::Ui, editor: &mut TextureEditor, tools: &mut ToolState) {
    let Some(set) = &editor.doc.extras.tileset else {
        ui.weak("Create a tileset in the Tiles section first.");
        return;
    };
    if editor.doc.active_layer().tilemap.is_none() {
        ui.weak("Select a tilemap layer (Layer › New Tilemap Layer).");
        return;
    }
    let brush = &mut tools.tile;
    ui.label(format!("Tile {}", brush.tile.id));
    ui.toggle_value(&mut brush.tile.flip_h, "Flip H")
        .on_hover_text("Mirror the tile left to right");
    ui.toggle_value(&mut brush.tile.flip_v, "Flip V")
        .on_hover_text("Mirror the tile top to bottom");
    if ui
        .button(format!("Turn {}°", u32::from(brush.tile.rotation) * 90))
        .on_hover_text("Rotate the tile a quarter turn")
        .clicked()
    {
        brush.tile.rotation = (brush.tile.rotation + 1) % 4;
    }
    egui::ComboBox::from_id_salt("terrain")
        .selected_text(match brush.terrain.and_then(|t| set.terrains.get(t)) {
            Some(t) => t.name.clone(),
            None => "No autotiling".to_owned(),
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut brush.terrain, None, "No autotiling");
            for (i, t) in set.terrains.iter().enumerate() {
                ui.selectable_value(&mut brush.terrain, Some(i), &t.name);
            }
        });
    if !brush.random.is_empty() {
        ui.label(format!("Random from {} tiles", brush.random.len()));
        if ui.small_button("Clear").clicked() {
            brush.random.clear();
        }
    }
    ui.weak("Right-click erases");
}

/// The vertical strip of tool buttons left of the canvas, with the colour wells below.
pub fn tool_rail(ui: &mut egui::Ui, rect: Rect, tools: &mut ToolState, checker: &TextureHandle) {
    widgets::toolbar_background(ui, rect, Edge::Right);
    let mut rail = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(0.0, SPACE_S)))
            .layout(Layout::top_down(Align::Center)),
    );
    let ui = &mut rail;
    ui.spacing_mut().item_spacing.y = 2.0;
    let ctx = ui.ctx().clone();
    for group in RAIL {
        // The group shows its selected tool (or its first); right-click lists the others.
        let shown = group
            .iter()
            .copied()
            .find(|t| *t == tools.tool)
            .unwrap_or(group[0]);
        let mut tooltip = Command::Tool(shown).tooltip(&ctx);
        if group.len() > 1 {
            tooltip.push_str("\nRight-click for related tools");
        }
        let response = widgets::icon_toggle(
            ui,
            group.contains(&tools.tool),
            icons::tool(shown),
            &tooltip,
        );
        if response.clicked() {
            tools.tool = shown;
        }
        if group.len() > 1 {
            response.context_menu(|ui| {
                for t in group.iter() {
                    let label =
                        widgets::icon_label(icons::tool(*t), &Command::Tool(*t).tooltip(&ctx));
                    if ui.selectable_label(tools.tool == *t, label).clicked() {
                        tools.tool = *t;
                        ui.close();
                    }
                }
            });
        }
    }

    ui.add_space(SPACE_M);
    // Primary colour in front, secondary behind it.
    let (wells, response) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    let side = vec2(18.0, 18.0);
    let back = Rect::from_min_size(wells.max - side, side);
    let front = Rect::from_min_size(wells.min, side);
    widgets::paint_swatch(ui, back, tools.secondary, false, checker);
    widgets::paint_swatch(ui, front, tools.settings.color, false, checker);
    if response.clicked() {
        tools.swap_colors();
    }
    response.on_hover_text(format!(
        "Primary {}\nSecondary {} (right mouse button)\nClick to swap",
        widgets::hex(tools.settings.color),
        widgets::hex(tools.secondary)
    ));
    if widgets::icon_button(ui, icons::SWAP, &Command::SwapColors.tooltip(&ctx)).clicked() {
        tools.swap_colors();
    }
}
