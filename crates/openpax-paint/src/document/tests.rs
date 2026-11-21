use super::*;

const RED: Rgba = [255, 0, 0, 255];
const BLUE: Rgba = [0, 0, 255, 255];

fn settings(color: Rgba) -> ToolSettings {
    ToolSettings {
        color,
        ..Default::default()
    }
}

fn drag(doc: &mut Document, tool: ToolKind, s: ToolSettings, from: (i32, i32), to: (i32, i32)) {
    doc.stroke_begin(tool, &s, from);
    doc.stroke_move(to);
    doc.stroke_end();
}

fn click(doc: &mut Document, tool: ToolKind, s: ToolSettings, at: (i32, i32)) {
    doc.stroke_begin(tool, &s, at);
    doc.stroke_end();
}

/// Renders the flattened document as rows of `#` (opaque) and `.` (transparent).
fn ascii(doc: &Document) -> Vec<String> {
    let flat = doc.flatten();
    (0..flat.height() as i32)
        .map(|y| {
            (0..flat.width() as i32)
                .map(|x| {
                    if flat.get(x, y).unwrap()[3] > 0 {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect()
        })
        .collect()
}

#[test]
fn pencil_click_and_drag() {
    let mut doc = Document::new(4, 4);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    drag(&mut doc, ToolKind::Pencil, settings(RED), (0, 3), (3, 3));
    assert_eq!(ascii(&doc), ["#...", "....", "....", "####"]);
    assert_eq!(doc.flatten().get(0, 0), Some(RED));
}

#[test]
fn pencil_survives_out_of_bounds_drag() {
    let mut doc = Document::new(3, 3);
    drag(&mut doc, ToolKind::Pencil, settings(RED), (-5, 1), (10, 1));
    assert_eq!(ascii(&doc), ["...", "###", "..."]);
}

#[test]
fn brush_size_and_eraser() {
    let mut doc = Document::new(4, 4);
    let big = ToolSettings {
        size: 2,
        ..settings(RED)
    };
    click(&mut doc, ToolKind::Pencil, big, (2, 2));
    assert_eq!(ascii(&doc), ["....", ".##.", ".##.", "...."]);
    click(&mut doc, ToolKind::Eraser, settings(RED), (1, 1));
    assert_eq!(ascii(&doc), ["....", "..#.", ".##.", "...."]);
    assert_eq!(doc.flatten().get(1, 1), Some(TRANSPARENT));
}

#[test]
fn pixel_perfect_freehand_has_no_l_corners() {
    let mut doc = Document::new(4, 4);
    let s = ToolSettings {
        pixel_perfect: true,
        ..settings(RED)
    };
    doc.stroke_begin(ToolKind::Pencil, &s, (0, 0));
    doc.stroke_move((1, 0));
    doc.stroke_move((1, 1));
    doc.stroke_move((2, 1));
    doc.stroke_move((2, 2));
    doc.stroke_end();
    assert_eq!(ascii(&doc), ["#...", ".#..", "..#.", "...."]);
}

#[test]
fn diagonal_line_is_pixel_perfect() {
    let mut doc = Document::new(4, 4);
    drag(&mut doc, ToolKind::Line, settings(RED), (0, 0), (3, 3));
    assert_eq!(ascii(&doc), ["#...", ".#..", "..#.", "...#"]);
}

#[test]
fn constrained_line_snaps_horizontal() {
    let mut doc = Document::new(4, 4);
    let s = ToolSettings {
        constrain: true,
        ..settings(RED)
    };
    drag(&mut doc, ToolKind::Line, s, (0, 0), (3, 1));
    // Snaps to the nearest clean slope (2:1 here would overshoot; 3,0 is closest).
    assert_eq!(doc.flatten().get(0, 0), Some(RED));
}

#[test]
fn shape_preview_does_not_leave_trails() {
    let mut doc = Document::new(4, 4);
    doc.stroke_begin(ToolKind::Line, &settings(RED), (0, 0));
    doc.stroke_move((3, 3));
    doc.stroke_move((3, 0));
    doc.stroke_end();
    assert_eq!(ascii(&doc), ["####", "....", "....", "...."]);
}

#[test]
fn rectangle_outline_and_filled() {
    let mut doc = Document::new(4, 4);
    drag(&mut doc, ToolKind::Rectangle, settings(RED), (3, 3), (0, 0));
    assert_eq!(ascii(&doc), ["####", "#..#", "#..#", "####"]);
    let filled = ToolSettings {
        shape_filled: true,
        ..settings(RED)
    };
    drag(&mut doc, ToolKind::Rectangle, filled, (1, 1), (2, 2));
    assert_eq!(ascii(&doc), ["####", "####", "####", "####"]);
}

#[test]
fn ellipse_outline() {
    let mut doc = Document::new(6, 6);
    drag(&mut doc, ToolKind::Ellipse, settings(RED), (0, 0), (5, 5));
    assert_eq!(
        ascii(&doc),
        [".####.", "#....#", "#....#", "#....#", "#....#", ".####."]
    );
}

#[test]
fn polygon_filled_and_open() {
    let mut doc = Document::new(5, 5);
    let s = ToolSettings {
        shape_filled: true,
        ..settings(RED)
    };
    doc.draw_polygon(&s, &[(0, 0), (4, 0), (0, 4)], true);
    assert_eq!(doc.flatten().get(1, 1), Some(RED));
    assert_eq!(doc.flatten().get(4, 4), Some(TRANSPARENT));
    assert!(doc.undo());
    doc.draw_polygon(&settings(RED), &[(0, 0), (4, 0), (4, 4)], false);
    assert_eq!(ascii(&doc), ["#####", "....#", "....#", "....#", "....#"]);
}

#[test]
fn fill_contiguous_versus_global() {
    let build = || {
        let mut doc = Document::new(5, 3);
        drag(&mut doc, ToolKind::Line, settings(RED), (2, 0), (2, 2));
        doc
    };
    let mut doc = build();
    click(&mut doc, ToolKind::Fill, settings(BLUE), (0, 0));
    assert_eq!(doc.flatten().get(1, 2), Some(BLUE));
    assert_eq!(doc.flatten().get(2, 1), Some(RED));
    assert_eq!(
        doc.flatten().get(4, 0),
        Some(TRANSPARENT),
        "wall stops contiguous fill"
    );

    let mut doc = build();
    let global = ToolSettings {
        fill_contiguous: false,
        ..settings(BLUE)
    };
    click(&mut doc, ToolKind::Fill, global, (0, 0));
    assert_eq!(doc.flatten().get(4, 0), Some(BLUE));
    assert_eq!(doc.flatten().get(2, 1), Some(RED));
}

#[test]
fn fill_tolerance_and_diagonals() {
    let mut doc = Document::new(3, 1);
    click(
        &mut doc,
        ToolKind::Pencil,
        settings([10, 10, 10, 255]),
        (0, 0),
    );
    click(
        &mut doc,
        ToolKind::Pencil,
        settings([14, 10, 10, 255]),
        (1, 0),
    );
    click(
        &mut doc,
        ToolKind::Pencil,
        settings([90, 10, 10, 255]),
        (2, 0),
    );
    let s = ToolSettings {
        tolerance: 5,
        ..settings(BLUE)
    };
    click(&mut doc, ToolKind::Fill, s, (0, 0));
    assert_eq!(doc.flatten().get(1, 0), Some(BLUE));
    assert_eq!(doc.flatten().get(2, 0), Some([90, 10, 10, 255]));

    let mut doc = Document::new(2, 2);
    click(&mut doc, ToolKind::Pencil, settings(RED), (1, 0));
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 1));
    let diag = ToolSettings {
        diagonal: true,
        ..settings(BLUE)
    };
    click(&mut doc, ToolKind::Fill, diag, (0, 0));
    assert_eq!(
        doc.flatten().get(1, 1),
        Some(BLUE),
        "spreads across the diagonal"
    );
}

#[test]
fn mirror_draws_all_four_quadrants() {
    let mut doc = Document::new(4, 4);
    let mirrored = ToolSettings {
        mirror_x: true,
        mirror_y: true,
        ..settings(RED)
    };
    click(&mut doc, ToolKind::Pencil, mirrored, (0, 1));
    assert_eq!(ascii(&doc), ["....", "#..#", "#..#", "...."]);
}

#[test]
fn shading_steps_along_the_ramp() {
    let ramp = vec![[0, 0, 0, 255], [100, 100, 100, 255], [200, 200, 200, 255]];
    let mut doc = Document::new(2, 1);
    click(&mut doc, ToolKind::Pencil, settings(ramp[1]), (0, 0));
    let s = ToolSettings {
        ramp: ramp.clone(),
        ..settings(RED)
    };
    click(&mut doc, ToolKind::Shade, s.clone(), (0, 0));
    assert_eq!(doc.flatten().get(0, 0), Some(ramp[2]));
    let darker = ToolSettings {
        secondary_button: true,
        ..s.clone()
    };
    click(&mut doc, ToolKind::Shade, darker.clone(), (0, 0));
    click(&mut doc, ToolKind::Shade, darker, (0, 0));
    assert_eq!(doc.flatten().get(0, 0), Some(ramp[0]));
    click(&mut doc, ToolKind::Shade, s, (1, 0));
    assert_eq!(
        doc.flatten().get(1, 0),
        Some(TRANSPARENT),
        "colours off the ramp are left alone"
    );
}

#[test]
fn dither_brush_checkers() {
    let mut doc = Document::new(4, 4);
    let s = ToolSettings {
        size: 4,
        dither_level: 8,
        dither_color: BLUE,
        ..settings(RED)
    };
    click(&mut doc, ToolKind::Dither, s, (2, 2));
    let flat = doc.flatten();
    let reds = flat.pixels().filter(|c| *c == RED).count();
    assert_eq!(reds, 8);
}

#[test]
fn dither_brush_and_fill_use_custom_patterns() {
    let lines = crate::quantize::builtin_patterns()
        .into_iter()
        .find(|(n, _)| *n == "Horizontal lines")
        .unwrap()
        .1;
    let mut doc = Document::new(4, 4);
    let s = ToolSettings {
        size: 4,
        dither_color: BLUE,
        dither_pattern: Some(std::sync::Arc::new(lines.clone())),
        ..settings(RED)
    };
    click(&mut doc, ToolKind::Dither, s, (2, 2));
    assert_eq!(doc.flatten().get(1, 0), Some(BLUE));
    assert_eq!(doc.flatten().get(1, 1), Some(RED));

    let mut doc = Document::new(2, 2);
    doc.fill_selected(RED);
    doc.fill_pattern(BLUE, TRANSPARENT, &lines);
    assert_eq!(doc.flatten().get(0, 0), Some(BLUE));
    assert_eq!(
        doc.flatten().get(0, 1),
        Some(RED),
        "a transparent second colour keeps pixels"
    );
    assert!(doc.undo());
    assert_eq!(doc.flatten().get(0, 0), Some(RED));
}

#[test]
fn selection_clips_painting() {
    let mut doc = Document::new(4, 4);
    drag(&mut doc, ToolKind::Select, settings(RED), (1, 1), (2, 2));
    assert_eq!(doc.selection_bounds(), Some(Rect::new(1, 1, 2, 2)));
    let filled = ToolSettings {
        shape_filled: true,
        ..settings(RED)
    };
    drag(&mut doc, ToolKind::Rectangle, filled, (0, 0), (3, 3));
    assert_eq!(ascii(&doc), ["....", ".##.", ".##.", "...."]);

    click(&mut doc, ToolKind::Select, settings(RED), (0, 0));
    assert_eq!(doc.selection(), None, "a click without dragging deselects");
}

#[test]
fn selection_modes_and_undo() {
    let mut doc = Document::new(4, 4);
    drag(&mut doc, ToolKind::Select, settings(RED), (0, 0), (1, 1));
    let add = ToolSettings {
        select_mode: SelectMode::Add,
        ..settings(RED)
    };
    drag(&mut doc, ToolKind::Select, add, (2, 2), (3, 3));
    assert_eq!(doc.selection().unwrap().count(), 8);
    assert!(doc.undo(), "selection changes are undoable");
    assert_eq!(doc.selection().unwrap().count(), 4);
    doc.invert_selection();
    assert_eq!(doc.selection().unwrap().count(), 12);
}

#[test]
fn lasso_and_magic_wand() {
    let mut doc = Document::new(5, 5);
    doc.stroke_begin(ToolKind::Lasso, &settings(RED), (0, 0));
    doc.stroke_move((4, 0));
    doc.stroke_move((0, 4));
    doc.stroke_end();
    let sel = doc.selection().unwrap();
    assert!(sel.get(1, 1) && !sel.get(4, 4));

    let mut doc = Document::new(3, 1);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    click(&mut doc, ToolKind::Pencil, settings(RED), (2, 0));
    let global = ToolSettings {
        fill_contiguous: false,
        ..settings(RED)
    };
    click(&mut doc, ToolKind::MagicWand, global, (0, 0));
    assert_eq!(doc.selection().unwrap().count(), 2);
}

#[test]
fn move_selection_and_undo_restores_it() {
    let mut doc = Document::new(4, 4);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    drag(&mut doc, ToolKind::Select, settings(RED), (0, 0), (1, 1));
    drag(&mut doc, ToolKind::Move, settings(RED), (0, 0), (2, 1));
    assert_eq!(ascii(&doc), ["....", "..#.", "....", "...."]);
    assert_eq!(doc.selection_bounds(), Some(Rect::new(2, 1, 2, 2)));
    assert!(
        doc.floating().is_some(),
        "moved pixels float until put down"
    );

    assert!(doc.undo());
    assert_eq!(ascii(&doc), ["#...", "....", "....", "...."]);
    assert_eq!(doc.selection_bounds(), Some(Rect::new(0, 0, 2, 2)));
}

#[test]
fn nudge_moves_by_one_pixel_and_commits_as_one_step() {
    let mut doc = Document::new(4, 1);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    drag(&mut doc, ToolKind::Select, settings(RED), (0, 0), (0, 0));
    doc.select_all();
    doc.nudge(1, 0);
    doc.nudge(1, 0);
    doc.commit_floating();
    assert_eq!(ascii(&doc), ["..#."]);
    assert!(doc.undo());
    assert_eq!(ascii(&doc), ["#..."]);
}

#[test]
fn moving_a_whole_layer_puts_it_down_at_once() {
    let mut doc = Document::new(3, 1);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    drag(&mut doc, ToolKind::Move, settings(RED), (0, 0), (2, 0));
    assert!(doc.floating().is_none());
    assert_eq!(ascii(&doc), ["..#"]);
    assert_eq!(doc.selection(), None);
}

#[test]
fn transform_selection_flips_in_place() {
    let mut doc = Document::new(4, 1);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    drag(&mut doc, ToolKind::Select, settings(RED), (0, 0), (1, 0));
    doc.transform_selection(Transform::FlipHorizontal);
    doc.commit_floating();
    assert_eq!(ascii(&doc), [".#.."]);
    doc.transform_selection(Transform::Scale {
        width: 4,
        height: 1,
        method: ScaleMethod::Nearest,
    });
    doc.commit_floating();
    assert_eq!(doc.flatten().pixels().filter(|c| c[3] > 0).count(), 2);
}

#[test]
fn layer_and_canvas_rotation() {
    let mut doc = Document::new(3, 2);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    doc.transform_canvas(Transform::Rotate90 { clockwise: true });
    assert_eq!((doc.width(), doc.height()), (2, 3));
    assert_eq!(doc.flatten().get(1, 0), Some(RED));
    doc.undo();
    assert_eq!((doc.width(), doc.height()), (3, 2));
}

#[test]
fn picker_reads_composite_without_editing() {
    let mut doc = Document::new(2, 2);
    click(&mut doc, ToolKind::Pencil, settings(RED), (1, 1));
    doc.add_layer();
    let revision = doc.revision();
    assert_eq!(
        doc.stroke_begin(ToolKind::Picker, &settings(BLUE), (1, 1)),
        Some(RED)
    );
    assert!(!doc.stroke_active());
    assert_eq!(doc.revision(), revision);
    let own = ToolSettings {
        pick_active_layer: true,
        ..settings(BLUE)
    };
    assert_eq!(
        doc.stroke_begin(ToolKind::Picker, &own, (1, 1)),
        Some(TRANSPARENT)
    );
}

#[test]
fn undo_redo_round_trip() {
    let mut doc = Document::new(4, 4);
    let empty = doc.flatten();
    drag(&mut doc, ToolKind::Pencil, settings(RED), (0, 0), (3, 0));
    let one = doc.flatten();
    drag(&mut doc, ToolKind::Pencil, settings(BLUE), (0, 0), (0, 3));
    let two = doc.flatten();

    assert!(doc.undo());
    assert_eq!(doc.flatten(), one);
    assert!(doc.undo());
    assert_eq!(doc.flatten(), empty);
    assert!(!doc.undo());
    assert!(doc.redo() && doc.redo());
    assert_eq!(doc.flatten(), two);
    assert!(!doc.redo());

    doc.undo();
    drag(&mut doc, ToolKind::Pencil, settings(RED), (3, 3), (3, 3));
    assert!(!doc.can_redo(), "a new edit discards the redo stack");
    let (undo, redo) = doc.history();
    assert_eq!(undo, ["Pencil", "Pencil"]);
    assert!(redo.is_empty());
}

#[test]
fn stroke_that_changes_nothing_is_not_recorded() {
    let mut doc = Document::new(2, 2);
    click(&mut doc, ToolKind::Eraser, settings(RED), (0, 0));
    assert!(!doc.can_undo());
}

#[test]
fn layers_composite_and_structural_undo() {
    let mut doc = Document::new(2, 1);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    doc.add_layer();
    assert_eq!((doc.layers().len(), doc.active()), (2, 1));
    drag(&mut doc, ToolKind::Pencil, settings(BLUE), (0, 0), (1, 0));
    assert_eq!(doc.flatten().get(0, 0), Some(BLUE));

    doc.set_layer_visible(1, false);
    assert_eq!(doc.flatten().get(0, 0), Some(RED));
    assert_eq!(doc.flatten().get(1, 0), Some(TRANSPARENT));
    doc.set_layer_visible(1, true);

    doc.move_layer(false);
    assert_eq!(doc.active(), 0);
    assert_eq!(
        doc.flatten().get(0, 0),
        Some(RED),
        "blue layer is now underneath"
    );
    doc.undo();
    assert_eq!(doc.flatten().get(0, 0), Some(BLUE));

    doc.merge_down();
    assert_eq!(doc.layers().len(), 1);
    assert_eq!(doc.flatten().get(1, 0), Some(BLUE));
    doc.undo();
    assert_eq!(doc.layers().len(), 2);

    doc.remove_layer();
    doc.remove_layer();
    assert_eq!(doc.layers().len(), 1, "the last layer cannot be removed");
}

#[test]
fn locked_layers_refuse_paint_and_alpha_lock_keeps_shape() {
    let mut doc = Document::new(2, 1);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    doc.edit_layer(0, "Lock", None, |l| l.locked = true);
    click(&mut doc, ToolKind::Pencil, settings(BLUE), (1, 0));
    assert_eq!(doc.flatten().get(1, 0), Some(TRANSPARENT));
    assert!(doc.edit_blocker().is_some());

    doc.edit_layer(0, "Unlock", None, |l| {
        l.locked = false;
        l.alpha_lock = true;
    });
    drag(&mut doc, ToolKind::Pencil, settings(BLUE), (0, 0), (1, 0));
    assert_eq!(doc.flatten().get(0, 0), Some(BLUE));
    assert_eq!(
        doc.flatten().get(1, 0),
        Some(TRANSPARENT),
        "alpha lock keeps empty pixels empty"
    );
}

#[test]
fn blend_modes_clipping_and_groups() {
    let mut doc = Document::new(2, 1);
    click(
        &mut doc,
        ToolKind::Pencil,
        settings([128, 128, 128, 255]),
        (0, 0),
    );
    doc.add_layer();
    drag(
        &mut doc,
        ToolKind::Pencil,
        settings([128, 128, 128, 255]),
        (0, 0),
        (1, 0),
    );
    doc.set_layer_blend(1, BlendMode::Difference);
    assert_eq!(doc.flatten().get(0, 0), Some([0, 0, 0, 255]));
    doc.set_layer_blend(1, BlendMode::Normal);

    // Clipped to the layer below: only where layer 0 has pixels.
    doc.edit_layer(1, "Clip", None, |l| l.clip = true);
    assert_eq!(doc.flatten().get(1, 0), Some(TRANSPARENT));
    assert_eq!(doc.flatten().get(0, 0), Some([128, 128, 128, 255]));
    doc.edit_layer(1, "Unclip", None, |l| l.clip = false);

    // Hiding a group hides what is in it.
    doc.group_active();
    let group = doc.active();
    assert_eq!(doc.layers()[group].kind, LayerKind::Group);
    assert_eq!(doc.layer_depth(group - 1), 1);
    doc.set_layer_visible(group, false);
    assert_eq!(doc.flatten().get(1, 0), Some(TRANSPARENT));
    assert_eq!(doc.layer_tree(false), vec![group, group - 1, 0]);
    doc.set_layer_visible(group, true);
    doc.ungroup();
    assert!(doc.layers().iter().all(|l| l.kind != LayerKind::Group));
}

#[test]
fn reference_layers_are_not_exported() {
    let mut doc = Document::new(2, 2);
    doc.add_reference_layer("Ref", &Bitmap::filled(2, 2, RED));
    assert_eq!(doc.flatten().get(0, 0), Some(TRANSPARENT));
    let shown = doc.render(
        0,
        RenderOptions {
            reference: true,
            solo: None,
        },
    );
    assert!(shown.get(0, 0).unwrap()[3] > 0);
}

#[test]
fn layer_effects_apply_without_changing_pixels() {
    let mut doc = Document::new(3, 1);
    click(&mut doc, ToolKind::Pencil, settings(RED), (1, 0));
    doc.edit_layer(0, "Outline", None, |l| {
        l.effects.push(LayerEffect {
            enabled: true,
            effect: crate::effects::Effect::Outline {
                color: BLUE,
                place: crate::effects::OutlinePlace::Outside,
                diagonal: false,
            },
        });
    });
    assert_eq!(doc.flatten().get(0, 0), Some(BLUE));
    assert_eq!(doc.layer_pixels(0).get(0, 0), Some(TRANSPARENT));
}

#[test]
fn merge_visible_flatten_and_layer_via_cut() {
    let mut doc = Document::new(2, 1);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    doc.add_layer();
    click(&mut doc, ToolKind::Pencil, settings(BLUE), (1, 0));
    let flat = doc.flatten();
    doc.merge_visible();
    assert_eq!(doc.layers().len(), 1);
    assert_eq!(doc.flatten(), flat);
    doc.undo();
    doc.flatten_layers();
    assert_eq!(doc.layers().len(), 1);
    assert_eq!(doc.flatten(), flat);

    doc.select_mask(
        Mask::from_rect(2, 1, Rect::new(0, 0, 1, 1)),
        SelectMode::Replace,
    );
    doc.layer_from_selection(true);
    assert_eq!(doc.layers().len(), 2);
    assert_eq!(doc.layer_pixels(0).get(0, 0), Some(TRANSPARENT));
    assert_eq!(doc.layer_pixels(1).get(0, 0), Some(RED));
}

#[test]
fn frames_links_and_tags() {
    let mut doc = Document::new(2, 1);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    doc.add_frame(true);
    assert_eq!((doc.frame_count(), doc.current_frame()), (2, 1));
    assert_eq!(
        doc.flatten().get(0, 0),
        Some(RED),
        "duplicated frame keeps pixels"
    );
    click(&mut doc, ToolKind::Pencil, settings(BLUE), (1, 0));
    assert_eq!(
        doc.flatten_frame(0).get(1, 0),
        Some(TRANSPARENT),
        "frames are independent"
    );

    doc.add_linked_frame();
    assert!(doc.is_linked(0, 1) && doc.is_linked(0, 2));
    click(&mut doc, ToolKind::Eraser, settings(RED), (1, 0));
    assert_eq!(
        doc.flatten_frame(1).get(1, 0),
        Some(TRANSPARENT),
        "linked cels share pixels"
    );
    doc.unlink_cel(0, 2);
    assert!(!doc.is_linked(0, 1));

    doc.add_tag(Tag {
        name: "walk".into(),
        from: 1,
        to: 2,
        direction: PlayDirection::Forward,
        color: RED,
    });
    doc.set_current_frame(0);
    doc.add_frame(false);
    assert_eq!(
        (doc.tags()[0].from, doc.tags()[0].to),
        (2, 3),
        "tags shift with inserted frames"
    );
    doc.set_current_frame(2);
    doc.remove_frame();
    assert_eq!((doc.tags()[0].from, doc.tags()[0].to), (2, 2));

    doc.move_frame(0, 2);
    assert_eq!(doc.current_frame(), 2);
    for _ in 0..5 {
        doc.set_frame_duration(0, 300);
        doc.set_frame_duration(0, 250);
    }
    doc.end_coalescing();
    assert_eq!(doc.frames()[0].duration_ms, 250);
    assert!(doc.undo());
    assert_eq!(
        doc.frames()[0].duration_ms,
        100,
        "one duration drag is one undo step"
    );
}

#[test]
fn multi_frame_painting() {
    let mut doc = Document::new(2, 1);
    doc.add_frame(false);
    doc.add_frame(false);
    doc.set_frame_range(Some((0, 2)));
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    for f in 0..3 {
        assert_eq!(doc.flatten_frame(f).get(0, 0), Some(RED));
    }
    assert!(doc.undo());
    for f in 0..3 {
        assert_eq!(doc.flatten_frame(f).get(0, 0), Some(TRANSPARENT));
    }
}

#[test]
fn ping_pong_order() {
    assert_eq!(PlayDirection::PingPong.order(0, 3), vec![0, 1, 2, 3, 2, 1]);
    assert_eq!(PlayDirection::Reverse.order(1, 3), vec![3, 2, 1]);
}

#[test]
fn resize_anchor_scale_crop_and_trim() {
    let mut doc = Document::new(2, 2);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));
    doc.resize_canvas_anchored(4, 4, (0.5, 0.5));
    assert_eq!(doc.flatten().get(1, 1), Some(RED));
    doc.trim();
    assert_eq!((doc.width(), doc.height()), (1, 1));
    doc.scale_image(3, 3, ScaleMethod::Nearest);
    assert_eq!(doc.flatten().pixels().filter(|c| *c == RED).count(), 9);
    drag(&mut doc, ToolKind::Select, settings(RED), (0, 0), (1, 0));
    doc.crop_to_selection();
    assert_eq!((doc.width(), doc.height()), (2, 1));
}

#[test]
fn resize_flip_and_clear() {
    let mut doc = Document::new(2, 2);
    click(&mut doc, ToolKind::Pencil, settings(RED), (0, 0));

    doc.resize_canvas(3, 4);
    assert_eq!((doc.width(), doc.height()), (3, 4));
    assert_eq!(ascii(&doc), ["#..", "...", "...", "..."]);
    doc.flip_horizontal();
    assert_eq!(ascii(&doc)[0], "..#");
    doc.flip_vertical();
    assert_eq!(ascii(&doc)[3], "..#");

    doc.clear_selected();
    assert_eq!(doc.flatten(), Bitmap::new(3, 4));
    doc.undo();
    doc.undo();
    doc.undo();
    doc.undo();
    assert_eq!((doc.width(), doc.height()), (2, 2));
    assert_eq!(ascii(&doc), ["#.", ".."]);
}

#[test]
fn filters_respect_scope_and_selection() {
    let mut doc = Document::new(2, 1);
    drag(&mut doc, ToolKind::Pencil, settings(RED), (0, 0), (1, 0));
    doc.select_mask(
        Mask::from_rect(2, 1, Rect::new(1, 0, 1, 1)),
        SelectMode::Replace,
    );
    doc.apply_filter("Invert", Scope::Cel, |b| {
        crate::effects::Effect::Invert.apply(b)
    });
    assert_eq!(doc.flatten().get(0, 0), Some(RED));
    assert_eq!(doc.flatten().get(1, 0), Some([0, 255, 255, 255]));
}

#[test]
fn indexed_mode_snaps_to_the_palette() {
    let mut doc = Document::new(1, 1);
    doc.edit_extras("Palette", |e| {
        e.palette = vec![[0, 0, 0, 255], [250, 0, 0, 255]];
        e.indexed = true;
    });
    click(
        &mut doc,
        ToolKind::Pencil,
        settings([200, 10, 10, 255]),
        (0, 0),
    );
    assert_eq!(doc.flatten().get(0, 0), Some([250, 0, 0, 255]));
}

#[test]
fn layer_properties_are_undoable() {
    let mut doc = Document::new(2, 2);
    doc.set_layer_visible(0, false);
    doc.set_layer_name(0, "Base".to_owned());
    assert!(!doc.layers()[0].visible);

    assert!(doc.undo());
    assert_eq!(doc.layers()[0].name, "Layer 1");
    assert!(doc.undo());
    assert!(doc.layers()[0].visible);
    assert!(!doc.can_undo());
    assert!(doc.redo() && doc.redo());
    assert_eq!(doc.layers()[0].name, "Base");
    assert!(!doc.layers()[0].visible);

    let revision = doc.revision();
    doc.set_layer_visible(0, false);
    assert_eq!(doc.revision(), revision);
}

#[test]
fn one_opacity_drag_is_one_undo_step() {
    let mut doc = Document::new(2, 2);
    for step in 1..=5 {
        doc.set_layer_opacity(0, 1.0 - step as f32 * 0.1);
    }
    doc.end_coalescing();
    doc.set_layer_opacity(0, 0.2);

    assert!(doc.undo());
    assert!((doc.layers()[0].opacity - 0.5).abs() < 1e-6);
    assert!(doc.undo());
    assert_eq!(doc.layers()[0].opacity, 1.0);
    assert!(!doc.can_undo());
}

#[test]
fn move_layer_to_reorders_in_one_step() {
    let mut doc = Document::new(1, 1);
    doc.add_layer();
    doc.add_layer();
    let names =
        |doc: &Document| -> Vec<String> { doc.layers().iter().map(|l| l.name.clone()).collect() };
    assert_eq!(names(&doc), ["Layer 1", "Layer 2", "Layer 3"]);

    doc.move_layer_to(0);
    assert_eq!(names(&doc), ["Layer 3", "Layer 1", "Layer 2"]);
    assert_eq!(doc.active(), 0);
    doc.move_layer_to(99);
    assert_eq!(names(&doc), ["Layer 1", "Layer 2", "Layer 3"]);
    assert_eq!(doc.active(), 2);

    assert!(doc.undo());
    assert_eq!(names(&doc), ["Layer 3", "Layer 1", "Layer 2"]);
}

#[test]
fn copy_and_paste_onto_a_new_layer() {
    let mut doc = Document::new(4, 4);
    drag(&mut doc, ToolKind::Pencil, settings(RED), (0, 0), (1, 0));
    drag(&mut doc, ToolKind::Select, settings(RED), (0, 0), (1, 1));
    let copied = doc.copy_selected();
    assert_eq!((copied.width(), copied.height()), (2, 2));
    assert_eq!(copied.get(1, 0), Some(RED));

    doc.paste(&copied, (3, 2));
    assert_eq!(doc.layers().len(), 2);
    assert_eq!(doc.active(), 1);
    assert_eq!(doc.layers()[1].name, "Pasted");
    assert_eq!(
        doc.selection_bounds(),
        Some(Rect::new(3, 2, 1, 2)),
        "clipped to the canvas"
    );
    assert_eq!(ascii(&doc), ["##..", "....", "...#", "...."]);

    assert!(doc.undo());
    assert_eq!(doc.layers().len(), 1);
    assert_eq!(doc.selection_bounds(), Some(Rect::new(0, 0, 2, 2)));
}

#[test]
fn paste_in_place_floats_on_the_active_layer() {
    let mut doc = Document::new(3, 1);
    doc.paste_floating(&Bitmap::filled(1, 1, RED), (1, 0));
    assert_eq!(doc.layers().len(), 1);
    assert_eq!(ascii(&doc), [".#."]);
    doc.nudge(1, 0);
    doc.commit_floating();
    assert_eq!(ascii(&doc), ["..#"]);
    assert!(doc.undo());
    assert_eq!(ascii(&doc), ["..."]);
}

#[test]
fn copy_with_a_lasso_keeps_the_shape() {
    let mut doc = Document::new(2, 2);
    doc.fill_selected(RED);
    let mut mask = Mask::new(2, 2);
    mask.set(0, 0, true);
    mask.set(1, 1, true);
    doc.select_mask(mask, SelectMode::Replace);
    let copied = doc.copy_selected();
    assert_eq!(copied.get(1, 0), Some(TRANSPARENT));
    assert_eq!(copied.get(1, 1), Some(RED));
}

#[test]
fn saved_selections_round_trip() {
    let mut doc = Document::new(3, 3);
    drag(&mut doc, ToolKind::Select, settings(RED), (0, 0), (1, 2));
    doc.save_selection("left");
    doc.deselect();
    doc.load_selection("left", SelectMode::Replace);
    assert_eq!(doc.selection_bounds(), Some(Rect::new(0, 0, 2, 3)));
}

#[test]
fn copy_without_selection_takes_the_whole_layer() {
    let doc = Document::new(3, 2);
    let copied = doc.copy_selected();
    assert_eq!((copied.width(), copied.height()), (3, 2));
}

#[test]
fn undo_history_is_capped() {
    let mut doc = Document::new(1, 1);
    for i in 0..MAX_UNDO_STEPS + 50 {
        let color = [(i % 2) as u8 * 255, 0, 0, 255];
        click(&mut doc, ToolKind::Pencil, settings(color), (0, 0));
    }
    let mut steps = 0;
    while doc.undo() {
        steps += 1;
    }
    assert_eq!(steps, MAX_UNDO_STEPS);
}

#[test]
fn cancelled_stroke_leaves_no_trace() {
    let mut doc = Document::new(4, 4);
    doc.stroke_begin(ToolKind::Pencil, &settings(RED), (0, 0));
    doc.stroke_move((3, 3));
    assert_eq!(doc.flatten().get(2, 2), Some(RED));
    doc.stroke_cancel();
    assert!(!doc.stroke_active());
    assert_eq!(doc.flatten(), Bitmap::new(4, 4));
    assert!(!doc.can_undo());
}
