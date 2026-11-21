//! `openpax-cli`: the pixel tools of OpenPax without the window.
//!
//! Every command reads PNG, GIF, APNG, WebP, BMP, TGA or JPEG files (and OpenPax layer
//! sidecars via `--layers`), and exits with status 1 on failure (or, for `validate`, when
//! problems are found).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use openpax_paint::analysis::{self, CheckOptions};
use openpax_paint::export::{self, AnimFrame, ExportPreset, SheetLayout, SheetOptions, Sprite};
use openpax_paint::quantize::{self, Dither};
use openpax_paint::{Bitmap, Document, Palette, PaletteFormat, ScaleMethod};

#[derive(Parser)]
#[command(
    name = "openpax-cli",
    version,
    about = "Convert, pack, scale, quantize, validate and script pixel art without the OpenPax window"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum Method {
    Nearest,
    Smooth,
    Rotsprite,
}

impl From<Method> for ScaleMethod {
    fn from(m: Method) -> Self {
        match m {
            Method::Nearest => ScaleMethod::Nearest,
            Method::Smooth => ScaleMethod::Smooth,
            Method::Rotsprite => ScaleMethod::RotSprite,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum DitherArg {
    None,
    Bayer2,
    Bayer4,
    Bayer8,
    FloydSteinberg,
    Atkinson,
}

impl From<DitherArg> for Dither {
    fn from(d: DitherArg) -> Self {
        match d {
            DitherArg::None => Dither::None,
            DitherArg::Bayer2 => Dither::Bayer2,
            DitherArg::Bayer4 => Dither::Bayer4,
            DitherArg::Bayer8 => Dither::Bayer8,
            DitherArg::FloydSteinberg => Dither::FloydSteinberg,
            DitherArg::Atkinson => Dither::Atkinson,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Print size, frame count and colour count of images.
    Info { inputs: Vec<PathBuf> },
    /// Convert between formats; the output extension picks the format (.png, .gif,
    /// .apng, .webp). Animated inputs keep their frames.
    Convert {
        input: PathBuf,
        output: PathBuf,
        /// Whole-number upscale factor.
        #[arg(long, default_value_t = 1)]
        scale: u32,
        /// Write one PNG per frame instead (`{frame}` in the output name is the number).
        #[arg(long)]
        sequence: bool,
    },
    /// Put frames or images onto one sprite sheet, with a JSON file next to it.
    Sheet {
        inputs: Vec<PathBuf>,
        #[arg(short, long)]
        output: PathBuf,
        /// horizontal, vertical, packed, or grid:N
        #[arg(long, default_value = "horizontal")]
        layout: String,
        #[arg(long)]
        trim: bool,
        #[arg(long, default_value_t = 0)]
        spacing: u32,
        #[arg(long, default_value_t = 0)]
        border: u32,
        #[arg(long, default_value_t = 0)]
        extrude: u32,
        #[arg(long, default_value_t = 1)]
        scale: u32,
        #[arg(long)]
        power_of_two: bool,
        #[arg(long)]
        dedupe: bool,
        /// Skip the JSON metadata.
        #[arg(long)]
        no_json: bool,
    },
    /// Resize an image (all frames).
    Scale {
        input: PathBuf,
        output: PathBuf,
        /// Whole-number factor (ignored when --width/--height are given).
        #[arg(long, default_value_t = 2)]
        factor: u32,
        #[arg(long)]
        width: Option<u32>,
        #[arg(long)]
        height: Option<u32>,
        #[arg(long, value_enum, default_value_t = Method::Nearest)]
        method: Method,
    },
    /// Reduce colours, to a count or to a palette file.
    Quantize {
        input: PathBuf,
        output: PathBuf,
        #[arg(long, default_value_t = 16)]
        colors: usize,
        /// Map to this palette (.gpl, .pal, .ase, .hex, .png) instead.
        #[arg(long)]
        palette: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = DitherArg::None)]
        dither: DitherArg,
    },
    /// Convert a palette between formats, or extract one from an image.
    Palette { input: PathBuf, output: PathBuf },
    /// Check images for tile seams, colours outside a palette, too many colours and stray
    /// pixels. Exits with status 1 when anything is found.
    Validate {
        inputs: Vec<PathBuf>,
        #[arg(long)]
        no_seams: bool,
        #[arg(long, default_value_t = 48)]
        seam_tolerance: u8,
        #[arg(long)]
        palette: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        max_colors: usize,
        #[arg(long)]
        no_strays: bool,
        /// Print the problems as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Run a Rhai script on each image (`doc` is the image) and save the result.
    Script {
        script: PathBuf,
        inputs: Vec<PathBuf>,
        /// Output folder (default: overwrite the inputs).
        #[arg(short, long)]
        out_dir: Option<PathBuf>,
    },
    /// Export images with a saved export preset (JSON, as written by the editor).
    Batch {
        preset: PathBuf,
        inputs: Vec<PathBuf>,
        #[arg(short, long, default_value = ".")]
        out_dir: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

type Result<T> = std::result::Result<T, String>;

fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn load_frames(path: &Path) -> Result<Vec<AnimFrame>> {
    export::decode_animation(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
}

fn load_doc(path: &Path) -> Result<Document> {
    export::document_from_frames(load_frames(path)?)
        .ok_or_else(|| format!("{}: no frames", path.display()))
}

fn load_palette(path: &Path) -> Result<Palette> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let format = PaletteFormat::from_extension(ext)
        .ok_or_else(|| format!("unknown palette format .{ext}"))?;
    Palette::decode(&read(path)?, format)
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".into())
}

fn save_frames(frames: &[AnimFrame], output: &Path) -> Result<()> {
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let first = frames.first().ok_or("no frames")?;
    let bytes = match ext.as_str() {
        "gif" => export::encode_gif(frames, None),
        "apng" => export::encode_apng(frames, None),
        "png" if frames.len() > 1 => export::encode_apng(frames, None),
        "png" => openpax_paint::encode_png(&first.image),
        "webp" => export::encode_webp(&first.image),
        other => {
            return Err(format!(
                "cannot write .{other} files (use png, apng, gif or webp)"
            ));
        }
    }
    .map_err(|e| e.to_string())?;
    write(output, &bytes)
}

fn run(command: Cmd) -> Result<bool> {
    match command {
        Cmd::Info { inputs } => {
            for input in inputs {
                let frames = load_frames(&input)?;
                let first = &frames[0].image;
                let colors = frames
                    .iter()
                    .map(|f| quantize::count_colors(&f.image))
                    .max()
                    .unwrap_or(0);
                let total: u32 = frames.iter().map(|f| f.duration_ms).sum();
                println!(
                    "{}: {}×{}, {} frame(s), {total} ms, {colors} colour(s), seam score {:.2}",
                    input.display(),
                    first.width(),
                    first.height(),
                    frames.len(),
                    analysis::seam_score(first)
                );
            }
            Ok(true)
        }
        Cmd::Convert {
            input,
            output,
            scale,
            sequence,
        } => {
            let frames = export::scale_frames(&load_frames(&input)?, scale.max(1));
            if sequence {
                let template = output.to_string_lossy().into_owned();
                let template = if template.contains("{frame") {
                    template
                } else {
                    let (base, ext) = template.rsplit_once('.').unwrap_or((&template, "png"));
                    format!("{base}_{{frame:3}}.{ext}")
                };
                for (i, f) in frames.iter().enumerate() {
                    let path = export::frame_file_name(&template, &stem(&input), i, "");
                    write(
                        Path::new(&path),
                        &openpax_paint::encode_png(&f.image).map_err(|e| e.to_string())?,
                    )?;
                }
                return Ok(true);
            }
            save_frames(&frames, &output)?;
            Ok(true)
        }
        Cmd::Sheet {
            inputs,
            output,
            layout,
            trim,
            spacing,
            border,
            extrude,
            scale,
            power_of_two,
            dedupe,
            no_json,
        } => {
            let layout = match layout.as_str() {
                "horizontal" | "h" => SheetLayout::Horizontal,
                "vertical" | "v" => SheetLayout::Vertical,
                "packed" | "atlas" => SheetLayout::Packed,
                other => match other.strip_prefix("grid:").and_then(|n| n.parse().ok()) {
                    Some(columns) => SheetLayout::Grid { columns },
                    None => return Err(format!("unknown layout {other}")),
                },
            };
            let mut sprites = Vec::new();
            for input in &inputs {
                let frames = load_frames(input)?;
                let many = frames.len() > 1;
                for (i, f) in frames.into_iter().enumerate() {
                    sprites.push(Sprite {
                        name: if many {
                            format!("{} {i}", stem(input))
                        } else {
                            stem(input)
                        },
                        image: f.image,
                        duration_ms: f.duration_ms,
                    });
                }
            }
            if sprites.is_empty() {
                return Err("no input images".into());
            }
            let options = SheetOptions {
                layout,
                trim,
                spacing,
                border,
                extrude,
                scale,
                power_of_two,
                dedupe,
            };
            let (sheet, placed) = export::build_sheet(&sprites, &options);
            write(
                &output,
                &openpax_paint::encode_png(&sheet).map_err(|e| e.to_string())?,
            )?;
            if !no_json {
                let name = output
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let json = export::sheet_json(&placed, &sheet, &name, None, scale);
                write(
                    &output.with_extension("json"),
                    &serde_json::to_vec_pretty(&json).map_err(|e| e.to_string())?,
                )?;
            }
            Ok(true)
        }
        Cmd::Scale {
            input,
            output,
            factor,
            width,
            height,
            method,
        } => {
            let frames = load_frames(&input)?;
            let (w0, h0) = (frames[0].image.width(), frames[0].image.height());
            let (w, h) = match (width, height) {
                (Some(w), Some(h)) => (w, h),
                (Some(w), None) => (
                    w,
                    (u64::from(h0) * u64::from(w) / u64::from(w0.max(1))) as u32,
                ),
                (None, Some(h)) => (
                    (u64::from(w0) * u64::from(h) / u64::from(h0.max(1))) as u32,
                    h,
                ),
                (None, None) => (w0 * factor.max(1), h0 * factor.max(1)),
            };
            let scaled: Vec<AnimFrame> = frames
                .iter()
                .map(|f| AnimFrame {
                    image: openpax_paint::resample::scale(
                        &f.image,
                        w.max(1),
                        h.max(1),
                        method.into(),
                    ),
                    duration_ms: f.duration_ms,
                })
                .collect();
            save_frames(&scaled, &output)?;
            Ok(true)
        }
        Cmd::Quantize {
            input,
            output,
            colors,
            palette,
            dither,
        } => {
            let frames = load_frames(&input)?;
            let target: Vec<openpax_paint::Rgba> = match &palette {
                Some(p) => load_palette(p)?.colors,
                None => {
                    // One palette for every frame, so animations do not flicker.
                    let mut all = Bitmap::new(
                        frames[0].image.width(),
                        frames[0].image.height() * frames.len() as u32,
                    );
                    for (i, f) in frames.iter().enumerate() {
                        all.blit(&f.image, 0, (i as u32 * frames[0].image.height()) as i32);
                    }
                    quantize::median_cut(&all, colors.max(1))
                }
            };
            let out: Vec<AnimFrame> = frames
                .iter()
                .map(|f| AnimFrame {
                    image: quantize::remap(&f.image, &target, dither.into()),
                    duration_ms: f.duration_ms,
                })
                .collect();
            println!("{} colour(s)", target.len());
            save_frames(&out, &output)?;
            Ok(true)
        }
        Cmd::Palette { input, output } => {
            let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("");
            let palette = match PaletteFormat::from_extension(ext) {
                Some(PaletteFormat::Png) | None => {
                    let image =
                        openpax_paint::decode_image(&read(&input)?).map_err(|e| e.to_string())?;
                    Palette::from_bitmap(&image, 256)
                }
                Some(_) => load_palette(&input)?,
            };
            let out_ext = output.extension().and_then(|e| e.to_str()).unwrap_or("");
            let format = PaletteFormat::from_extension(out_ext)
                .ok_or_else(|| format!("unknown palette format .{out_ext}"))?;
            println!("{} colour(s)", palette.colors.len());
            write(&output, &palette.encode(format))?;
            Ok(true)
        }
        Cmd::Validate {
            inputs,
            no_seams,
            seam_tolerance,
            palette,
            max_colors,
            no_strays,
            json,
        } => {
            let options = CheckOptions {
                seams: !no_seams,
                seam_tolerance,
                palette: match &palette {
                    Some(p) => load_palette(p)?.colors,
                    None => Vec::new(),
                },
                max_colors,
                strays: !no_strays,
            };
            let mut clean = true;
            let mut report = Vec::new();
            for input in &inputs {
                for (i, f) in load_frames(input)?.iter().enumerate() {
                    let issues = analysis::check(&f.image, &options);
                    clean &= issues.is_empty();
                    for issue in issues {
                        if json {
                            report.push(serde_json::json!({
                                "file": input.display().to_string(), "frame": i,
                                "kind": issue.kind.name(), "x": issue.x, "y": issue.y, "message": issue.message,
                            }));
                        } else {
                            println!(
                                "{}:{i}: ({}, {}) {}: {}",
                                input.display(),
                                issue.x,
                                issue.y,
                                issue.kind.name(),
                                issue.message
                            );
                        }
                    }
                }
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
                );
            } else if clean {
                println!("no problems found");
            }
            Ok(clean)
        }
        Cmd::Script {
            script,
            inputs,
            out_dir,
        } => {
            let source = String::from_utf8(read(&script)?).map_err(|e| e.to_string())?;
            let base = script.parent().map(Path::to_owned).unwrap_or_default();
            let mut ok = true;
            for input in &inputs {
                let mut doc = load_doc(input)?;
                let out = openpax_script::run(&mut doc, &source, "Script", &base);
                for line in &out.printed {
                    println!("{}: {line}", input.display());
                }
                if let Some(e) = out.error {
                    eprintln!("{}: script failed: {e}", input.display());
                    ok = false;
                    continue;
                }
                let target = match &out_dir {
                    Some(dir) => dir.join(input.file_name().unwrap_or_default()),
                    None => input.clone(),
                };
                let frames = export::document_frames(&doc, None);
                save_frames(&frames, &target)?;
            }
            Ok(ok)
        }
        Cmd::Batch {
            preset,
            inputs,
            out_dir,
        } => {
            let preset: ExportPreset = serde_json::from_slice(&read(&preset)?)
                .map_err(|e| format!("{}: {e}", preset.display()))?;
            for input in &inputs {
                let doc = load_doc(input)?;
                for (name, bytes) in
                    export::export(&doc, &stem(input), &preset).map_err(|e| e.to_string())?
                {
                    write(&out_dir.join(name), &bytes)?;
                }
            }
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(dir: &Path, name: &str) -> PathBuf {
        let mut b = Bitmap::new(4, 4);
        b.set(1, 1, [255, 0, 0, 255]);
        let path = dir.join(name);
        std::fs::write(&path, openpax_paint::encode_png(&b).unwrap()).unwrap();
        path
    }

    #[test]
    fn convert_scale_sheet_and_validate() {
        let dir = tempfile::tempdir().unwrap();
        let a = image(dir.path(), "a.png");
        let gif = dir.path().join("a.gif");
        assert!(
            run(Cmd::Convert {
                input: a.clone(),
                output: gif.clone(),
                scale: 2,
                sequence: false
            })
            .unwrap()
        );
        let frames = load_frames(&gif).unwrap();
        assert_eq!(frames[0].image.width(), 8);

        let sheet = dir.path().join("out/sheet.png");
        assert!(
            run(Cmd::Sheet {
                inputs: vec![a.clone(), a.clone()],
                output: sheet.clone(),
                layout: "grid:1".into(),
                trim: true,
                spacing: 1,
                border: 0,
                extrude: 0,
                scale: 1,
                power_of_two: false,
                dedupe: false,
                no_json: false,
            })
            .unwrap()
        );
        assert!(sheet.with_extension("json").exists());

        // The empty edges tile seamlessly.
        assert!(
            run(Cmd::Validate {
                inputs: vec![a.clone()],
                no_seams: false,
                seam_tolerance: 10,
                palette: None,
                max_colors: 0,
                no_strays: true,
                json: false,
            })
            .unwrap()
        );
        // The lone red pixel is a stray, so validation fails.
        assert!(
            !run(Cmd::Validate {
                inputs: vec![a],
                no_seams: true,
                seam_tolerance: 0,
                palette: None,
                max_colors: 0,
                no_strays: false,
                json: true,
            })
            .unwrap()
        );
    }

    #[test]
    fn script_and_palette() {
        let dir = tempfile::tempdir().unwrap();
        let a = image(dir.path(), "a.png");
        let script = dir.path().join("s.rhai");
        std::fs::write(&script, "doc.fill(rgb(0, 255, 0));").unwrap();
        let out = dir.path().join("out");
        assert!(
            run(Cmd::Script {
                script,
                inputs: vec![a.clone()],
                out_dir: Some(out.clone())
            })
            .unwrap()
        );
        let img = openpax_paint::decode_png(&std::fs::read(out.join("a.png")).unwrap()).unwrap();
        assert_eq!(img.get(3, 3), Some([0, 255, 0, 255]));

        let gpl = dir.path().join("p.gpl");
        assert!(
            run(Cmd::Palette {
                input: a,
                output: gpl.clone()
            })
            .unwrap()
        );
        assert_eq!(load_palette(&gpl).unwrap().colors, vec![[255, 0, 0, 255]]);
    }
}
