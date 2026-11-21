//! Rhai scripting for OpenPax documents.
//!
//! A script sees the open document as `doc` and can read and change pixels, layers, frames,
//! the selection and the palette, run filters and transforms, and read or write image
//! files. A whole script run is one undo step; if it fails, nothing it did is kept.
//!
//! Colours are integers `0xRRGGBBAA`; build them with `rgb(r, g, b)`, `rgba(r, g, b, a)` or
//! `hex("#ff8800")`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use openpax_paint::effects::{Effect, OutlinePlace};
use openpax_paint::{Bitmap, Document, Rect, Rgba, ScaleMethod, Scope, SelectMode, Transform};
use rhai::{Array, Dynamic, Engine, EvalAltResult};

type Doc = Arc<Mutex<Document>>;
type Fallible<T> = Result<T, Box<EvalAltResult>>;

/// What a script run printed and whether it failed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub printed: Vec<String>,
    pub error: Option<String>,
}

fn to_color(v: i64) -> Rgba {
    let v = v as u32;
    [(v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn from_color(c: Rgba) -> i64 {
    i64::from(u32::from_be_bytes(c))
}

fn err(message: impl Into<String>) -> Box<EvalAltResult> {
    message.into().into()
}

fn with<T>(doc: &mut Doc, f: impl FnOnce(&mut Document) -> T) -> T {
    let mut guard = doc
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut guard)
}

/// Builds the engine with the OpenPax API registered. `base` resolves relative file paths.
fn engine(printed: Arc<Mutex<Vec<String>>>, base: PathBuf) -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(200_000_000);
    engine.set_max_expr_depths(128, 64);
    let out = printed.clone();
    engine.on_print(move |s| out.lock().map(|mut v| v.push(s.to_owned())).unwrap_or(()));
    let out = printed;
    engine.on_debug(move |s, _, _| out.lock().map(|mut v| v.push(s.to_owned())).unwrap_or(()));

    // Colours.
    engine.register_fn("rgb", |r: i64, g: i64, b: i64| {
        from_color([r as u8, g as u8, b as u8, 255])
    });
    engine.register_fn("rgba", |r: i64, g: i64, b: i64, a: i64| {
        from_color([r as u8, g as u8, b as u8, a as u8])
    });
    engine.register_fn("hex", |s: &str| -> Fallible<i64> {
        openpax_paint::color::parse_hex(s)
            .map(from_color)
            .ok_or_else(|| err(format!("not a colour: {s}")))
    });
    engine.register_fn("to_hex", |c: i64| openpax_paint::color::to_hex(to_color(c)));
    engine.register_fn("red", |c: i64| i64::from(to_color(c)[0]));
    engine.register_fn("green", |c: i64| i64::from(to_color(c)[1]));
    engine.register_fn("blue", |c: i64| i64::from(to_color(c)[2]));
    engine.register_fn("alpha", |c: i64| i64::from(to_color(c)[3]));

    engine.register_type_with_name::<Doc>("Doc");
    // Size and structure.
    engine.register_fn("width", |d: &mut Doc| with(d, |d| i64::from(d.width())));
    engine.register_fn("height", |d: &mut Doc| with(d, |d| i64::from(d.height())));
    engine.register_fn("frame_count", |d: &mut Doc| {
        with(d, |d| d.frame_count() as i64)
    });
    engine.register_fn("layer_count", |d: &mut Doc| {
        with(d, |d| d.layers().len() as i64)
    });
    engine.register_fn("frame", |d: &mut Doc| with(d, |d| d.current_frame() as i64));
    engine.register_fn("set_frame", |d: &mut Doc, f: i64| {
        with(d, |d| d.set_current_frame(f.max(0) as usize))
    });
    engine.register_fn("layer", |d: &mut Doc| with(d, |d| d.active() as i64));
    engine.register_fn("set_layer", |d: &mut Doc, l: i64| {
        with(d, |d| d.set_active(l.max(0) as usize))
    });
    engine.register_fn("layer_name", |d: &mut Doc, l: i64| {
        with(d, |d| {
            d.layers()
                .get(l.max(0) as usize)
                .map(|l| l.name.clone())
                .unwrap_or_default()
        })
    });
    engine.register_fn("rename_layer", |d: &mut Doc, l: i64, name: &str| {
        let name = name.to_owned();
        with(d, |d| d.set_layer_name(l.max(0) as usize, name));
    });
    engine.register_fn("set_layer_visible", |d: &mut Doc, l: i64, v: bool| {
        with(d, |d| d.set_layer_visible(l.max(0) as usize, v));
    });
    engine.register_fn("set_layer_opacity", |d: &mut Doc, l: i64, o: f64| {
        with(d, |d| d.set_layer_opacity(l.max(0) as usize, o as f32));
    });
    engine.register_fn("add_layer", |d: &mut Doc| with(d, Document::add_layer));
    engine.register_fn("remove_layer", |d: &mut Doc| {
        with(d, Document::remove_layer)
    });
    engine.register_fn("duplicate_layer", |d: &mut Doc| {
        with(d, Document::duplicate_layer)
    });
    engine.register_fn("merge_down", |d: &mut Doc| with(d, Document::merge_down));
    engine.register_fn("flatten", |d: &mut Doc| with(d, Document::flatten_layers));
    engine.register_fn("add_frame", |d: &mut Doc| with(d, |d| d.add_frame(false)));
    engine.register_fn("duplicate_frame", |d: &mut Doc| {
        with(d, |d| d.add_frame(true))
    });
    engine.register_fn("remove_frame", |d: &mut Doc| {
        with(d, Document::remove_frame)
    });
    engine.register_fn("frame_duration", |d: &mut Doc, f: i64| {
        with(d, |d| {
            d.frames()
                .get(f.max(0) as usize)
                .map_or(0, |f| i64::from(f.duration_ms))
        })
    });
    engine.register_fn("set_frame_duration", |d: &mut Doc, f: i64, ms: i64| {
        with(d, |d| {
            d.set_frame_duration(f.max(0) as usize, ms.clamp(1, 65535) as u32);
            d.end_coalescing();
        });
    });

    // Pixels.
    engine.register_fn("get", |d: &mut Doc, x: i64, y: i64| {
        with(d, |d| d.get_pixel(x as i32, y as i32).map_or(0, from_color))
    });
    engine.register_fn("get_merged", |d: &mut Doc, x: i64, y: i64| {
        with(d, |d| {
            d.flatten().get(x as i32, y as i32).map_or(0, from_color)
        })
    });
    engine.register_fn("set", |d: &mut Doc, x: i64, y: i64, c: i64| {
        with(d, |d| d.set_pixel(x as i32, y as i32, to_color(c)));
    });
    engine.register_fn("fill", |d: &mut Doc, c: i64| {
        with(d, |d| d.fill_selected(to_color(c)))
    });
    engine.register_fn("clear", |d: &mut Doc| with(d, Document::clear_selected));

    // Selection.
    engine.register_fn("select", |d: &mut Doc, x: i64, y: i64, w: i64, h: i64| {
        with(d, |d| {
            let mask = openpax_paint::Mask::from_rect(
                d.width(),
                d.height(),
                Rect::new(x as i32, y as i32, w as i32, h as i32),
            );
            d.select_mask(mask, SelectMode::Replace);
        });
    });
    engine.register_fn("select_all", |d: &mut Doc| with(d, Document::select_all));
    engine.register_fn("deselect", |d: &mut Doc| with(d, Document::deselect));
    engine.register_fn("invert_selection", |d: &mut Doc| {
        with(d, Document::invert_selection)
    });
    engine.register_fn("is_selected", |d: &mut Doc, x: i64, y: i64| {
        with(d, |d| {
            d.selection().is_some_and(|m| m.get(x as i32, y as i32))
        })
    });

    // Transforms and filters.
    let transform = |d: &mut Doc, t: Transform| with(d, |d| d.transform_selection(t));
    engine.register_fn("flip_h", move |d: &mut Doc| {
        transform(d, Transform::FlipHorizontal)
    });
    engine.register_fn("flip_v", move |d: &mut Doc| {
        transform(d, Transform::FlipVertical)
    });
    engine.register_fn("rotate", move |d: &mut Doc, deg: f64| {
        transform(
            d,
            Transform::Rotate {
                degrees: deg as f32,
                smooth: true,
            },
        );
    });
    engine.register_fn("rotate_canvas", |d: &mut Doc, clockwise: bool| {
        with(d, |d| d.transform_canvas(Transform::Rotate90 { clockwise }));
    });
    engine.register_fn("resize", |d: &mut Doc, w: i64, h: i64| {
        with(d, |d| {
            d.resize_canvas_anchored(w.max(1) as u32, h.max(1) as u32, (0.5, 0.5))
        });
    });
    engine.register_fn("scale", |d: &mut Doc, w: i64, h: i64| {
        with(d, |d| {
            d.scale_image(w.max(1) as u32, h.max(1) as u32, ScaleMethod::Nearest)
        });
    });
    engine.register_fn("trim", |d: &mut Doc| with(d, Document::trim));
    let filter = |d: &mut Doc, label: &'static str, e: Effect| {
        with(d, |d| d.apply_filter(label, Scope::Cel, |b| e.apply(b)));
    };
    engine.register_fn("invert", move |d: &mut Doc| {
        filter(d, "Invert", Effect::Invert)
    });
    engine.register_fn("desaturate", move |d: &mut Doc| {
        filter(d, "Desaturate", Effect::Desaturate)
    });
    engine.register_fn("outline", move |d: &mut Doc, c: i64| {
        filter(
            d,
            "Outline",
            Effect::Outline {
                color: to_color(c),
                place: OutlinePlace::Outside,
                diagonal: false,
            },
        );
    });
    engine.register_fn("hue_shift", move |d: &mut Doc, deg: f64| {
        filter(
            d,
            "Hue",
            Effect::HueSaturation {
                hue: deg as f32,
                saturation: 0.0,
                value: 0.0,
            },
        );
    });
    engine.register_fn(
        "replace_color",
        |d: &mut Doc, from: i64, to: i64, tolerance: i64| {
            with(d, |d| {
                d.apply_filter("Replace colour", Scope::Cel, |b| {
                    let mut b = b.clone();
                    openpax_paint::effects::replace_color(
                        &mut b,
                        to_color(from),
                        to_color(to),
                        tolerance.clamp(0, 255) as u8,
                    );
                    b
                });
            });
        },
    );
    engine.register_fn("quantize", |d: &mut Doc, n: i64| {
        with(d, |d| {
            d.apply_filter("Reduce colours", Scope::Cel, |b| {
                openpax_paint::quantize::quantize(
                    b,
                    n.max(1) as usize,
                    openpax_paint::quantize::Dither::None,
                )
                .0
            });
        });
    });
    engine.register_fn("palette", |d: &mut Doc| -> Array {
        with(d, |d| {
            let colors = if d.extras.palette.is_empty() {
                openpax_paint::Palette::from_bitmap(&d.flatten(), 256).colors
            } else {
                d.extras.palette.clone()
            };
            colors
                .into_iter()
                .map(|c| Dynamic::from(from_color(c)))
                .collect()
        })
    });
    engine.register_fn("set_palette", |d: &mut Doc, colors: Array| {
        let colors: Vec<Rgba> = colors
            .into_iter()
            .filter_map(|c| c.as_int().ok())
            .map(to_color)
            .collect();
        with(d, |d| d.edit_extras("Set palette", |e| e.palette = colors));
    });

    // Files.
    let b = base.clone();
    engine.register_fn(
        "export_png",
        move |d: &mut Doc, path: &str, scale: i64| -> Fallible<()> {
            let image = with(d, |d| d.flatten());
            let s = scale.clamp(1, 64) as u32;
            let image =
                openpax_paint::resample::nearest(&image, image.width() * s, image.height() * s);
            let bytes = openpax_paint::encode_png(&image).map_err(|e| err(e.to_string()))?;
            std::fs::write(resolve(&b, path), bytes).map_err(|e| err(e.to_string()))
        },
    );
    let b = base;
    engine.register_fn(
        "import_layer",
        move |d: &mut Doc, path: &str| -> Fallible<()> {
            let bytes = std::fs::read(resolve(&b, path)).map_err(|e| err(e.to_string()))?;
            let image: Bitmap =
                openpax_paint::decode_image(&bytes).map_err(|e| err(e.to_string()))?;
            with(d, |d| d.paste(&image, (0, 0)));
            Ok(())
        },
    );
    engine
}

fn resolve(base: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_owned()
    } else {
        base.join(p)
    }
}

/// Runs a script against a document as one undo step. Relative paths in the script resolve
/// against `base`. If the script fails, its changes are undone.
pub fn run(doc: &mut Document, script: &str, label: &str, base: &Path) -> Output {
    let printed = Arc::new(Mutex::new(Vec::new()));
    let mut error = None;
    let undo_before = doc.history().0.len();
    let changed_before = doc.revision();
    doc.batch(label, |d| {
        let taken = std::mem::replace(d, Document::new(1, 1));
        let handle: Doc = Arc::new(Mutex::new(taken));
        {
            let engine = engine(printed.clone(), base.to_owned());
            let mut scope = rhai::Scope::new();
            scope.push("doc", handle.clone());
            if let Err(e) = engine.run_with_scope(&mut scope, script) {
                error = Some(e.to_string());
            }
        }
        let restored = match Arc::try_unwrap(handle) {
            Ok(m) => m
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            Err(shared) => {
                let mut guard = shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                std::mem::replace(&mut *guard, Document::new(1, 1))
            }
        };
        *d = restored;
    });
    if error.is_some() && doc.history().0.len() > undo_before && doc.revision() != changed_before {
        doc.undo();
    }
    Output {
        printed: printed.lock().map(|v| v.clone()).unwrap_or_default(),
        error,
    }
}

/// Checks a script for syntax errors without running it.
pub fn check(script: &str) -> Result<(), String> {
    let engine = engine(Arc::default(), PathBuf::new());
    engine
        .compile(script)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// A script file found in a scripts folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptFile {
    /// From a `// name: ...` first line, or the file name.
    pub name: String,
    /// From a `// description: ...` line, if any.
    pub description: String,
    pub path: PathBuf,
}

/// The `.rhai` scripts in a folder, sorted by name.
pub fn list_scripts(dir: &Path) -> Vec<ScriptFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<ScriptFile> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("rhai"))
        })
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let header = |key: &str| {
                text.lines()
                    .take_while(|l| l.trim_start().starts_with("//"))
                    .find_map(|l| {
                        l.trim_start()
                            .trim_start_matches('/')
                            .trim()
                            .strip_prefix(key)
                            .map(|v| v.trim().to_owned())
                    })
            };
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().replace(['_', '-'], " "))
                .unwrap_or_default();
            ScriptFile {
                name: header("name:").unwrap_or(stem),
                description: header("description:").unwrap_or_default(),
                path,
            }
        })
        .collect();
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

/// Example scripts written into a new scripts folder.
pub const EXAMPLES: &[(&str, &str)] = &[
    (
        "outline_black.rhai",
        "// name: Black outline\n// description: Draws a 1-pixel black outline around the active layer.\ndoc.outline(rgb(0, 0, 0));\n",
    ),
    (
        "checkerboard.rhai",
        "// name: Fill with checkerboard\n// description: Fills the active layer with a two-colour checkerboard.\nlet a = rgb(40, 40, 40);\nlet b = rgb(70, 70, 70);\nfor y in 0..doc.height() {\n    for x in 0..doc.width() {\n        doc.set(x, y, if (x + y) % 2 == 0 { a } else { b });\n    }\n}\n",
    ),
    (
        "report.rhai",
        "// name: Report colours\n// description: Prints how many colours the image uses.\nprint(`${doc.width()} x ${doc.height()}, ${doc.frame_count()} frame(s), ${doc.palette().len()} colours`);\n",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_edit_in_one_undo_step() {
        let mut doc = Document::new(3, 2);
        let out = run(
            &mut doc,
            "for x in 0..doc.width() { doc.set(x, 0, rgb(255, 0, 0)); } print(doc.get(1, 0) == hex(\"#ff0000\"));",
            "Test",
            Path::new("."),
        );
        assert_eq!(out.error, None);
        assert_eq!(out.printed, ["true"]);
        assert_eq!(doc.flatten().get(2, 0), Some([255, 0, 0, 255]));
        assert_eq!(doc.history().0, ["Test"]);
        assert!(doc.undo());
        assert_eq!(doc.flatten().get(2, 0), Some([0, 0, 0, 0]));
    }

    #[test]
    fn failing_scripts_change_nothing() {
        let mut doc = Document::new(2, 2);
        let out = run(
            &mut doc,
            "doc.set(0, 0, rgb(1, 2, 3)); throw \"boom\";",
            "Test",
            Path::new("."),
        );
        assert!(out.error.unwrap().contains("boom"));
        assert_eq!(doc.flatten().get(0, 0), Some([0, 0, 0, 0]));
    }

    #[test]
    fn structure_and_filters() {
        let mut doc = Document::new(4, 4);
        let out = run(
            &mut doc,
            "doc.add_layer(); doc.add_frame(); doc.select(1, 1, 2, 2); doc.fill(rgb(9, 9, 9)); doc.deselect(); doc.outline(rgb(0,0,0)); print(doc.layer_count()); print(doc.frame_count());",
            "Test",
            Path::new("."),
        );
        assert_eq!(out.error, None);
        assert_eq!(out.printed, ["2", "2"]);
        assert_eq!(doc.flatten().get(0, 1), Some([0, 0, 0, 255]));
    }

    #[test]
    fn examples_compile() {
        for (_, source) in EXAMPLES {
            check(source).unwrap();
        }
        assert!(check("let x = ;").is_err());
    }
}
