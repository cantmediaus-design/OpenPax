//! "Optimize Model" and "Optimize All Models": geometry passes from `openpax_model::optimize`
//! plus texture shrinking, planned without touching the disk and then applied with backups.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use openpax_core::{
    AssetKind, Error, Pack, Progress, ResourceLocation, SIDECAR_DIR, TextureMeta, vanilla,
};
use openpax_model::optimize::clean_uv;
use openpax_model::{ModelContext, ModelFile, ModelReport, Opacity, OptimizeOptions, optimize};
use openpax_paint::Bitmap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Textures are never shrunk below this many pixels per side: smaller sprites lower the
/// mipmap levels of the whole block atlas.
pub const MIN_TEXTURE_SIDE: u32 = 16;
/// Where the originals of optimized files are copied before they are overwritten.
pub const BACKUP_DIR: &str = ".openpax/backups";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OptimizeSettings {
    pub model: OptimizeOptions,
    /// Store textures that are blown-up pixel art at their real resolution.
    pub downscale_textures: bool,
    /// Pack the used parts of a model's own texture into a smaller image.
    pub repack_textures: bool,
}

impl Default for OptimizeSettings {
    fn default() -> Self {
        Self {
            model: OptimizeOptions::default(),
            downscale_textures: true,
            repack_textures: true,
        }
    }
}

/// `assets/<ns>/textures/<path>.png` for a texture location.
pub fn texture_rel_path(location: &str) -> Option<String> {
    let location: ResourceLocation = location.parse().ok()?;
    Some(format!(
        "assets/{}/textures/{}.png",
        location.namespace(),
        location.path()
    ))
}

/// `ns:path` for `assets/<ns>/models/<path>.json`.
pub fn model_location(rel_path: &str) -> Option<String> {
    let rest = rel_path.strip_prefix("assets/")?;
    let (namespace, rest) = rest.split_once('/')?;
    let path = rest.strip_prefix("models/")?.strip_suffix(".json")?;
    Some(format!("{namespace}:{path}"))
}

/// A location written with its namespace (`block/stone` becomes `minecraft:block/stone`).
fn normalize(location: &str) -> Option<String> {
    location
        .parse::<ResourceLocation>()
        .ok()
        .map(|l| l.to_string())
}

/// Every model in a pack and how they refer to each other and to textures.
#[derive(Clone, Debug, Default)]
pub struct PackGraph {
    /// Parsed models by pack-relative path. Files that fail to parse are left out.
    pub models: BTreeMap<String, ModelFile>,
    /// Locations of models some other model names as its `parent`.
    parents: HashSet<String>,
    /// Texture location to the models whose `textures` mention it.
    texture_users: HashMap<String, BTreeSet<String>>,
}

impl PackGraph {
    pub fn scan(pack: &Pack, progress: &mut Progress) -> Result<Self, Error> {
        let entries: Vec<&str> = pack
            .index
            .entries
            .iter()
            .filter(|e| e.kind == AssetKind::Model)
            .map(|e| e.rel_path.as_str())
            .collect();
        let mut models = BTreeMap::new();
        for (i, rel_path) in entries.iter().enumerate() {
            if i % 32 == 0 {
                progress.step("Reading models", i as u64, entries.len() as u64)?;
            }
            let Ok(bytes) = pack.read(rel_path) else {
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            if let Ok(model) = ModelFile::from_json(&text) {
                models.insert((*rel_path).to_owned(), model);
            }
        }
        Ok(Self::from_models(models))
    }

    pub fn from_models(models: BTreeMap<String, ModelFile>) -> Self {
        let mut parents = HashSet::new();
        let mut texture_users: HashMap<String, BTreeSet<String>> = HashMap::new();
        for (rel_path, model) in &models {
            if let Some(parent) = model.parent.as_deref().and_then(normalize) {
                parents.insert(parent);
            }
            for value in model.textures.values() {
                if let Some(location) = value.as_str().filter(|v| !v.starts_with('#'))
                    && let Some(location) = normalize(location)
                {
                    texture_users
                        .entry(location)
                        .or_default()
                        .insert(rel_path.clone());
                }
            }
        }
        Self {
            models,
            parents,
            texture_users,
        }
    }

    pub fn is_parent(&self, rel_path: &str) -> bool {
        model_location(rel_path).is_some_and(|l| self.parents.contains(&l))
    }

    /// Replaces one model, e.g. with the unsaved version open in an editor.
    pub fn set_model(&mut self, rel_path: &str, model: ModelFile) {
        self.models.insert(rel_path.to_owned(), model);
        *self = Self::from_models(std::mem::take(&mut self.models));
    }
}

/// A texture as read from disk.
#[derive(Clone, Debug)]
struct Texture {
    bitmap: Bitmap,
    /// Animation frame size; the whole image when not animated.
    frame: (u32, u32),
    animated: bool,
    /// The animation sets its frame size explicitly, so it would need rewriting on resize.
    explicit_frame_size: bool,
    /// Lives in the pack being optimized (not only in the vanilla fallback).
    in_pack: bool,
}

/// Reads textures from the pack, falling back to downloaded vanilla resources, and caches them.
struct Textures<'a> {
    pack: &'a Pack,
    /// Root of a downloaded default pack, read file by file (opening it as a `Pack` would
    /// index thousands of files).
    vanilla: Option<PathBuf>,
    cache: RefCell<HashMap<String, Option<Texture>>>,
}

impl<'a> Textures<'a> {
    fn new(pack: &'a Pack) -> Self {
        let cache = vanilla::cache_root();
        let vanilla = vanilla::cached_versions(&cache)
            .last()
            .map(|name| cache.join(name));
        Self {
            pack,
            vanilla,
            cache: RefCell::default(),
        }
    }

    /// The PNG bytes and parsed `.mcmeta` of a texture, and whether it came from the pack.
    fn read(&self, rel_path: &str) -> Option<(Vec<u8>, Option<TextureMeta>, bool)> {
        if self.pack.exists(rel_path) {
            let meta = self.pack.texture_meta(rel_path).ok().flatten();
            return Some((self.pack.read(rel_path).ok()?, meta, true));
        }
        let root = self.vanilla.as_ref()?;
        let bytes = std::fs::read(root.join(rel_path)).ok()?;
        let meta = std::fs::read_to_string(root.join(format!("{rel_path}.mcmeta")))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        Some((bytes, meta, false))
    }

    fn get(&self, location: &str) -> Option<Texture> {
        let key = normalize(location)?;
        if let Some(hit) = self.cache.borrow().get(&key) {
            return hit.clone();
        }
        let loaded = self.load(&key);
        self.cache.borrow_mut().insert(key, loaded.clone());
        loaded
    }

    fn load(&self, location: &str) -> Option<Texture> {
        let rel_path = texture_rel_path(location)?;
        let (bytes, meta, in_pack) = self.read(&rel_path)?;
        let bitmap = openpax_paint::decode_png(&bytes).ok()?;
        let animation = meta.and_then(|m| m.animation);
        let (width, height) = (bitmap.width(), bitmap.height());
        Some(Texture {
            frame: animation
                .as_ref()
                .map_or((width, height), |a| a.frame_size(width, height)),
            animated: animation.is_some(),
            explicit_frame_size: animation
                .as_ref()
                .is_some_and(|a| a.width.is_some() || a.height.is_some()),
            in_pack,
            bitmap,
        })
    }

    fn opacity(&self, location: &str, uv: [f32; 4]) -> Opacity {
        match self.get(location) {
            Some(texture) => region_opacity(&texture, uv),
            None => Opacity::Unknown,
        }
    }
}

/// Opacity of the texels under a UV rectangle (0..16 units), across every animation frame.
fn region_opacity(texture: &Texture, uv: [f32; 4]) -> Opacity {
    let (fw, fh) = texture.frame;
    let to_px = |a: f32, b: f32, size: u32| {
        let scale = size as f32 / 16.0;
        let lo = (a.min(b) * scale + 1e-3).floor().max(0.0) as u32;
        let hi = ((a.max(b) * scale - 1e-3).ceil() as u32).min(size);
        (lo.min(size.saturating_sub(1)), hi.max(lo + 1).min(size))
    };
    let (x0, x1) = to_px(uv[0], uv[2], fw);
    let (y0, y1) = to_px(uv[1], uv[3], fh);
    let bitmap = &texture.bitmap;
    let (columns, rows) = (bitmap.width() / fw.max(1), bitmap.height() / fh.max(1));
    let (mut opaque, mut clear) = (true, true);
    for row in 0..rows.max(1) {
        for column in 0..columns.max(1) {
            for y in y0..y1 {
                for x in x0..x1 {
                    let px = (column * fw + x) as i32;
                    let py = (row * fh + y) as i32;
                    let alpha = bitmap.get(px, py).map_or(0, |p| p[3]);
                    opaque &= alpha == 255;
                    clear &= alpha == 0;
                }
            }
            if !opaque && !clear {
                return Opacity::Mixed;
            }
        }
    }
    match (opaque, clear) {
        (true, _) => Opacity::Opaque,
        (_, true) => Opacity::Empty,
        _ => Opacity::Mixed,
    }
}

/// The largest factor the image can be shrunk by without losing a pixel: every
/// `factor × factor` block is a single colour. Never shrinks a frame below
/// [`MIN_TEXTURE_SIDE`]. Returns 1 when it cannot shrink.
pub fn lossless_factor(bitmap: &Bitmap, frame: (u32, u32)) -> u32 {
    let (fw, fh) = frame;
    let (w, h) = (bitmap.width(), bitmap.height());
    let limit = fw.min(fh) / MIN_TEXTURE_SIDE;
    (2..=limit)
        .rev()
        .filter(|k| fw % k == 0 && fh % k == 0 && w % k == 0 && h % k == 0)
        .find(|&k| {
            (0..h).all(|y| {
                (0..w).all(|x| {
                    bitmap.get(x as i32, y as i32)
                        == bitmap.get((x - x % k) as i32, (y - y % k) as i32)
                })
            })
        })
        .unwrap_or(1)
}

/// Keeps the top-left pixel of every `factor × factor` block.
pub fn downscale(bitmap: &Bitmap, factor: u32) -> Bitmap {
    let (w, h) = (bitmap.width() / factor, bitmap.height() / factor);
    let mut out = Bitmap::new(w, h);
    for y in 0..h {
        for x in 0..w {
            if let Some(p) = bitmap.get((x * factor) as i32, (y * factor) as i32) {
                out.set(x as i32, y as i32, p);
            }
        }
    }
    out
}

/// A pixel rectangle `[x0, y0, x1, y1)` in a texture.
type PxRect = [u32; 4];

/// Places rectangles of the given sizes on shelves in a `side × side` square.
/// Returns each one's top-left corner, or `None` if they don't fit.
pub fn shelf_pack(sizes: &[(u32, u32)], side: u32) -> Option<Vec<(u32, u32)>> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(sizes[i].1), std::cmp::Reverse(sizes[i].0)));
    let mut out = vec![(0, 0); sizes.len()];
    let (mut x, mut y, mut shelf) = (0, 0, 0);
    for i in order {
        let (w, h) = sizes[i];
        if w > side || h > side {
            return None;
        }
        if x + w > side {
            (x, y, shelf) = (0, y + shelf, 0);
        }
        if y + h > side {
            return None;
        }
        out[i] = (x, y);
        x += w;
        shelf = shelf.max(h);
    }
    Some(out)
}

/// Joins rectangles that overlap into bounding boxes until none overlap.
/// Returns the islands and, for each input rectangle, the island holding it.
fn islands(rects: &[PxRect]) -> (Vec<PxRect>, Vec<usize>) {
    let mut boxes: Vec<PxRect> = Vec::new();
    for &r in rects {
        let mut merged = r;
        loop {
            let overlapping = boxes.iter().position(|b| {
                b[0] < merged[2] && merged[0] < b[2] && b[1] < merged[3] && merged[1] < b[3]
            });
            let Some(i) = overlapping else { break };
            let b = boxes.swap_remove(i);
            merged = [
                merged[0].min(b[0]),
                merged[1].min(b[1]),
                merged[2].max(b[2]),
                merged[3].max(b[3]),
            ];
        }
        boxes.push(merged);
    }
    let owner = rects
        .iter()
        .map(|r| {
            boxes
                .iter()
                .position(|b| b[0] <= r[0] && b[1] <= r[1] && r[2] <= b[2] && r[3] <= b[3])
                .expect("every rectangle lies in an island")
        })
        .collect();
    (boxes, owner)
}

/// One model the plan rewrites.
#[derive(Clone, Debug)]
pub struct ModelChange {
    pub rel_path: String,
    pub model: ModelFile,
    pub report: ModelReport,
    /// Face UVs were moved to match a repacked texture.
    pub uvs_remapped: bool,
}

/// One texture the plan rewrites.
#[derive(Clone, Debug)]
pub struct TextureChange {
    pub rel_path: String,
    pub before: (u32, u32),
    pub after: (u32, u32),
    pub png: Vec<u8>,
    /// What was done, e.g. "shrunk 4× losslessly" or "repacked used areas".
    pub how: String,
}

/// Everything an optimization would change, computed without writing anything.
#[derive(Clone, Debug, Default)]
pub struct OptimizePlan {
    pub models: Vec<ModelChange>,
    pub textures: Vec<TextureChange>,
    /// Sums over every model looked at, changed or not.
    pub totals: ModelReport,
    pub models_scanned: usize,
    /// Things that were skipped and why, for the summary.
    pub notes: Vec<String>,
}

impl OptimizePlan {
    pub fn is_empty(&self) -> bool {
        self.models.is_empty() && self.textures.is_empty()
    }

    pub fn texels(&self) -> (u64, u64) {
        self.textures.iter().fold((0, 0), |(b, a), t| {
            (
                b + u64::from(t.before.0) * u64::from(t.before.1),
                a + u64::from(t.after.0) * u64::from(t.after.1),
            )
        })
    }

    /// Pack-relative paths the plan overwrites.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.models
            .iter()
            .map(|m| m.rel_path.as_str())
            .chain(self.textures.iter().map(|t| t.rel_path.as_str()))
    }

    /// Copies the originals into a fresh backup folder, then writes every change.
    /// Returns the backup folder (pack-relative).
    pub fn apply(&self, pack: &Pack) -> Result<String, Error> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let mut folder = format!("{BACKUP_DIR}/optimize-{stamp}");
        let mut n = 1;
        while pack.exists(&folder) {
            folder = format!("{BACKUP_DIR}/optimize-{stamp}-{n}");
            n += 1;
        }
        for rel_path in self.paths() {
            if pack.exists(rel_path) {
                pack.write(&format!("{folder}/{rel_path}"), &pack.read(rel_path)?)?;
            }
        }
        for change in &self.textures {
            pack.write(&change.rel_path, &change.png)?;
            // Layer sidecars describe the old pixels; the texture editor would ignore them.
            for suffix in [".layers.json", ".layers.png"] {
                let sidecar = format!("{SIDECAR_DIR}/{}{suffix}", change.rel_path);
                if pack.exists(&sidecar) {
                    pack.remove(&sidecar)?;
                }
            }
        }
        for change in &self.models {
            pack.write(&change.rel_path, change.model.to_json().as_bytes())?;
        }
        Ok(folder)
    }
}

/// Plans optimizing the models at `targets` (pack-relative paths present in `graph`).
///
/// `locked` lists texture paths that must not be rewritten, e.g. ones open with unsaved edits.
/// Textures used by models outside `targets` are only shrunk losslessly, never repacked.
pub fn plan(
    pack: &Pack,
    graph: &PackGraph,
    targets: &[String],
    settings: &OptimizeSettings,
    locked: &HashSet<String>,
    progress: &mut Progress,
) -> Result<OptimizePlan, Error> {
    let textures = Textures::new(pack);
    let opacity = |location: &str, uv: [f32; 4]| textures.opacity(location, uv);
    let mut plan = OptimizePlan::default();

    // Geometry.
    let mut optimized: BTreeMap<String, (ModelFile, ModelReport)> = BTreeMap::new();
    for (i, rel_path) in targets.iter().enumerate() {
        progress.step("Optimizing models", i as u64, targets.len() as u64)?;
        let Some(original) = graph.models.get(rel_path) else {
            continue;
        };
        let mut model = original.clone();
        let context = ModelContext::for_path(rel_path, graph.is_parent(rel_path));
        let report = optimize(&mut model, &settings.model, &context, &opacity);
        plan.totals.add(&report);
        plan.models_scanned += 1;
        optimized.insert(rel_path.clone(), (model, report));
    }

    // Textures the optimized models use, by location.
    let mut used: BTreeSet<String> = BTreeSet::new();
    for (model, _) in optimized.values() {
        for value in model.textures.values() {
            if let Some(location) = value.as_str().filter(|v| !v.starts_with('#'))
                && let Some(location) = normalize(location)
            {
                used.insert(location);
            }
        }
    }

    let mut new_bitmaps: BTreeMap<String, (Texture, Vec<String>)> = BTreeMap::new();
    let mut remapped: HashSet<String> = HashSet::new();
    let total = used.len() as u64;
    for (i, location) in used.iter().enumerate() {
        progress.step("Shrinking textures", i as u64, total)?;
        let Some(rel_path) = texture_rel_path(location) else {
            continue;
        };
        let Some(mut texture) = textures.get(location) else {
            continue;
        };
        if !texture.in_pack || locked.contains(&rel_path) {
            continue;
        }
        let mut how = Vec::new();

        if settings.downscale_textures && !texture.explicit_frame_size {
            let factor = lossless_factor(&texture.bitmap, texture.frame);
            if factor > 1 {
                texture.bitmap = downscale(&texture.bitmap, factor);
                texture.frame = (texture.frame.0 / factor, texture.frame.1 / factor);
                how.push(format!("shrunk {factor}× losslessly"));
            }
        }

        if settings.repack_textures {
            match repack(location, &texture, graph, targets, &mut optimized) {
                Ok(Some((bitmap, models))) => {
                    texture.frame = (bitmap.width(), bitmap.height());
                    texture.bitmap = bitmap;
                    how.push("repacked used areas".to_owned());
                    remapped.extend(models);
                }
                Ok(None) => {}
                Err(note) => {
                    if !note.is_empty() {
                        plan.notes.push(format!("{location}: {note}"));
                    }
                }
            }
        }
        if !how.is_empty() {
            new_bitmaps.insert(rel_path, (texture, how));
        }
    }

    for (rel_path, (texture, how)) in new_bitmaps {
        let Some(original) = textures.get(&location_of(&rel_path)) else {
            continue;
        };
        let png = openpax_paint::encode_png(&texture.bitmap)
            .map_err(|e| Error::Format(format!("{rel_path}: {e}")))?;
        plan.textures.push(TextureChange {
            before: (original.bitmap.width(), original.bitmap.height()),
            after: (texture.bitmap.width(), texture.bitmap.height()),
            rel_path,
            png,
            how: how.join(", "),
        });
    }

    for (rel_path, (model, report)) in optimized {
        if graph.models.get(&rel_path) != Some(&model) {
            plan.models.push(ModelChange {
                uvs_remapped: remapped.contains(&rel_path),
                rel_path,
                model,
                report,
            });
        }
    }
    Ok(plan)
}

/// `ns:path` for `assets/<ns>/textures/<path>.png`.
fn location_of(texture_rel_path: &str) -> String {
    let rest = texture_rel_path
        .strip_prefix("assets/")
        .unwrap_or(texture_rel_path);
    let (namespace, rest) = rest.split_once('/').unwrap_or(("minecraft", rest));
    let path = rest.strip_prefix("textures/").unwrap_or(rest);
    let path = path.strip_suffix(".png").unwrap_or(path);
    format!("{namespace}:{path}")
}

/// Packs the parts of `texture` that faces use into the smallest square that holds them, and
/// moves those faces' UVs to match. `Ok(None)` when it would not get smaller; `Err` with a
/// reason (possibly empty) when the texture must be left alone.
fn repack(
    location: &str,
    texture: &Texture,
    graph: &PackGraph,
    targets: &[String],
    optimized: &mut BTreeMap<String, (ModelFile, ModelReport)>,
) -> Result<Option<(Bitmap, Vec<String>)>, String> {
    if location.starts_with("minecraft:") {
        return Err(String::new()); // vanilla models may use it
    }
    if texture.animated {
        return Err(String::new());
    }
    let users = graph
        .texture_users
        .get(location)
        .cloned()
        .unwrap_or_default();
    let outside = users.iter().filter(|u| !targets.contains(u)).count();
    if outside > 0 {
        return Err(format!(
            "not repacked, shared with {outside} other model(s) — use Optimize All Models"
        ));
    }
    for user in &users {
        let Some((model, _)) = optimized.get(user) else {
            return Err(String::new());
        };
        if model.elements().is_empty() {
            return Err(String::new()); // e.g. an item drawn from its layers
        }
        if graph.is_parent(user) {
            return Err(format!("not repacked, {user} is a parent of other models"));
        }
    }

    // Every face drawn with this texture, as (model, element, direction) and pixel rectangle.
    let (w, h) = (texture.bitmap.width(), texture.bitmap.height());
    let (sx, sy) = (w as f32 / 16.0, h as f32 / 16.0);
    let mut faces = Vec::new();
    let mut rects: Vec<PxRect> = Vec::new();
    for user in &users {
        let (model, _) = &optimized[user];
        for (e, element) in model.elements().iter().enumerate() {
            for (&direction, face) in &element.faces {
                if model
                    .resolve_texture(&face.texture)
                    .and_then(|t| normalize(&t))
                    .as_deref()
                    != Some(location)
                {
                    continue;
                }
                let uv = face.uv.unwrap_or_else(|| element.default_uv(direction));
                let px = [uv[0] * sx, uv[1] * sy, uv[2] * sx, uv[3] * sy];
                if px.iter().any(|v| (v - v.round()).abs() > 1e-3) {
                    return Err("not repacked, some UVs fall between pixels".to_owned());
                }
                let px = px.map(|v| v.round());
                let rect = [
                    px[0].min(px[2]),
                    px[1].min(px[3]),
                    px[0].max(px[2]),
                    px[1].max(px[3]),
                ];
                if rect[0] < 0.0 || rect[1] < 0.0 || rect[2] > w as f32 || rect[3] > h as f32 {
                    return Err("not repacked, some UVs reach outside the texture".to_owned());
                }
                // A zero-width UV still samples one row or column of texels.
                let rect = [
                    rect[0] as u32,
                    rect[1] as u32,
                    (rect[2] as u32).max(rect[0] as u32 + 1),
                    (rect[3] as u32).max(rect[1] as u32 + 1),
                ];
                faces.push((user.clone(), e, direction, px));
                rects.push(rect);
            }
        }
    }
    if rects.is_empty() {
        return Ok(None);
    }

    let (boxes, owner) = islands(&rects);
    let sizes: Vec<(u32, u32)> = boxes.iter().map(|b| (b[2] - b[0], b[3] - b[1])).collect();
    let area: u32 = sizes.iter().map(|(w, h)| w * h).sum();
    let mut side = MIN_TEXTURE_SIDE
        .max((area as f32).sqrt().ceil() as u32)
        .max(sizes.iter().map(|s| s.0.max(s.1)).max().unwrap_or(1))
        .next_power_of_two();
    let positions = loop {
        if u64::from(side) * u64::from(side) >= u64::from(w) * u64::from(h) {
            return Ok(None);
        }
        if let Some(positions) = shelf_pack(&sizes, side) {
            break positions;
        }
        side *= 2;
    };

    let mut bitmap = Bitmap::new(side, side);
    for (b, &(nx, ny)) in boxes.iter().zip(&positions) {
        for y in b[1]..b[3] {
            for x in b[0]..b[2] {
                if let Some(p) = texture.bitmap.get(x as i32, y as i32) {
                    bitmap.set((nx + x - b[0]) as i32, (ny + y - b[1]) as i32, p);
                }
            }
        }
    }

    let scale = 16.0 / side as f32;
    let mut models = Vec::new();
    for ((user, e, direction, px), island) in faces.into_iter().zip(owner) {
        let b = boxes[island];
        let (nx, ny) = positions[island];
        let dx = nx as f32 - b[0] as f32;
        let dy = ny as f32 - b[1] as f32;
        let uv = [
            (px[0] + dx) * scale,
            (px[1] + dy) * scale,
            (px[2] + dx) * scale,
            (px[3] + dy) * scale,
        ]
        .map(clean_uv);
        let (model, _) = optimized.get_mut(&user).expect("user was optimized");
        let element = &mut model.elements.as_mut().expect("has elements")[e];
        element.faces.get_mut(&direction).expect("face exists").uv = Some(uv);
        if !models.contains(&user) {
            models.push(user);
        }
    }
    for user in &models {
        let (model, _) = optimized.get_mut(user).expect("user was optimized");
        if model.extra.contains_key("texture_size") {
            model
                .extra
                .insert("texture_size".into(), Value::from(vec![side, side]));
        }
    }
    Ok(Some((bitmap, models)))
}

/// "12 → 3 cubes, 60 → 18 faces" for messages.
pub fn describe(totals: &ModelReport) -> String {
    format!(
        "{} → {} cubes, {} → {} faces",
        totals.elements_before, totals.elements_after, totals.faces_before, totals.faces_after
    )
}

/// "2 textures, 8192 → 512 pixels" for messages.
pub fn describe_textures(plan: &OptimizePlan) -> String {
    let (before, after) = plan.texels();
    format!(
        "{} texture(s), {before} → {after} pixels",
        plan.textures.len()
    )
}

/// A file name for messages.
pub fn short_name(rel_path: &str) -> &str {
    rel_path.rsplit('/').next().unwrap_or(rel_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openpax_model::{Direction, Element};

    fn pack() -> (tempfile::TempDir, Pack) {
        let dir = tempfile::tempdir().unwrap();
        let pack = Pack::create(dir.path().join("pack"), "test", 46).unwrap();
        (dir, pack)
    }

    fn write_png(pack: &Pack, rel: &str, bitmap: &Bitmap) {
        pack.write(rel, &openpax_paint::encode_png(bitmap).unwrap())
            .unwrap();
    }

    fn write_model(pack: &Pack, rel: &str, model: &ModelFile) {
        pack.write(rel, model.to_json().as_bytes()).unwrap();
    }

    fn rescanned(mut pack: Pack) -> Pack {
        pack.rescan().unwrap();
        pack
    }

    /// A 64×64 texture, opaque in its top-left 16×16 corner only, used by a 4-pixel cube.
    fn sparse_setup(pack: &Pack, model_names: &[&str]) {
        let mut sheet = Bitmap::new(64, 64);
        for y in 0..16 {
            for x in 0..16 {
                sheet.set(x, y, [x as u8 * 10, y as u8 * 10, 7, 255]);
            }
        }
        write_png(pack, "assets/t/textures/block/sheet.png", &sheet);
        for name in model_names {
            let mut model = ModelFile::default();
            model.textures.insert("0".into(), "t:block/sheet".into());
            let mut cube = Element::cube([0.0; 3], [16.0; 3], "#0");
            for face in cube.faces.values_mut() {
                face.uv = Some([0.0, 0.0, 4.0, 4.0]);
            }
            model.elements = Some(vec![cube]);
            model
                .extra
                .insert("texture_size".into(), Value::from(vec![64, 64]));
            write_model(pack, &format!("assets/t/models/block/{name}.json"), &model);
        }
    }

    fn run(pack: &Pack, targets: &[&str]) -> OptimizePlan {
        let graph = PackGraph::scan(pack, &mut Progress::none()).unwrap();
        let targets: Vec<String> = targets.iter().map(|t| (*t).to_owned()).collect();
        plan(
            pack,
            &graph,
            &targets,
            &OptimizeSettings::default(),
            &HashSet::new(),
            &mut Progress::none(),
        )
        .unwrap()
    }

    #[test]
    fn exclusive_sparse_texture_is_repacked_and_uvs_follow() {
        let (_dir, pack) = pack();
        sparse_setup(&pack, &["a"]);
        let pack = rescanned(pack);
        let plan = run(&pack, &["assets/t/models/block/a.json"]);

        assert_eq!(plan.textures.len(), 1);
        let texture = &plan.textures[0];
        assert_eq!((texture.before, texture.after), ((64, 64), (16, 16)));
        let bitmap = openpax_paint::decode_png(&texture.png).unwrap();
        assert_eq!(bitmap.get(15, 15), Some([150, 150, 7, 255]));

        let model = &plan.models[0];
        assert!(model.uvs_remapped);
        let face = &model.model.elements()[0].faces[&Direction::North];
        assert_eq!(face.uv, Some([0.0, 0.0, 16.0, 16.0]));
        assert_eq!(model.model.extra["texture_size"], Value::from(vec![16, 16]));

        let folder = plan.apply(&pack).unwrap();
        assert!(pack.exists(&format!("{folder}/assets/t/textures/block/sheet.png")));
        assert!(pack.exists(&format!("{folder}/assets/t/models/block/a.json")));
        let written =
            openpax_paint::decode_png(&pack.read("assets/t/textures/block/sheet.png").unwrap())
                .unwrap();
        assert_eq!(written.width(), 16);
    }

    #[test]
    fn shared_texture_is_only_repacked_when_every_user_is_optimized() {
        let (_dir, pack) = pack();
        sparse_setup(&pack, &["a", "b"]);
        let pack = rescanned(pack);

        let single = run(&pack, &["assets/t/models/block/a.json"]);
        assert!(single.textures.is_empty());
        assert!(single.notes.iter().any(|n| n.contains("shared with 1")));

        let both = run(
            &pack,
            &[
                "assets/t/models/block/a.json",
                "assets/t/models/block/b.json",
            ],
        );
        assert_eq!(both.textures.len(), 1);
        assert!(both.models.iter().all(|m| m.uvs_remapped));
    }

    #[test]
    fn upscaled_texture_shrinks_without_touching_uvs() {
        let (_dir, pack) = pack();
        // 64×64 made of 2×2 blocks: shrinks to 32×32.
        let mut art = Bitmap::new(64, 64);
        for y in 0..64 {
            for x in 0..64 {
                art.set(x, y, [(x / 2) as u8, (y / 2) as u8, 0, 255]);
            }
        }
        write_png(&pack, "assets/minecraft/textures/block/stone.png", &art);
        let mut model = ModelFile::default();
        model.textures.insert("all".into(), "block/stone".into());
        model.elements = Some(vec![Element::cube([0.0; 3], [16.0; 3], "#all")]);
        write_model(&pack, "assets/minecraft/models/block/stone.json", &model);
        let pack = rescanned(pack);

        let plan = run(&pack, &["assets/minecraft/models/block/stone.json"]);
        assert_eq!(plan.textures.len(), 1);
        assert_eq!(plan.textures[0].after, (32, 32));
        assert!(plan.models.iter().all(|m| !m.uvs_remapped));
    }

    #[test]
    fn lossless_factor_respects_the_floor() {
        let flat = Bitmap::filled(32, 32, [1, 2, 3, 255]);
        assert_eq!(lossless_factor(&flat, (32, 32)), 2);
        assert_eq!(
            lossless_factor(&Bitmap::filled(16, 16, [0; 4]), (16, 16)),
            1
        );
        let mut noisy = flat.clone();
        noisy.set(1, 0, [9, 9, 9, 255]);
        assert_eq!(lossless_factor(&noisy, (32, 32)), 1);
    }

    #[test]
    fn packer_never_overlaps() {
        let sizes = [(8, 4), (4, 4), (16, 2), (3, 7), (5, 5)];
        let at = shelf_pack(&sizes, 16).unwrap();
        for i in 0..sizes.len() {
            let a = [at[i].0, at[i].1, at[i].0 + sizes[i].0, at[i].1 + sizes[i].1];
            assert!(a[2] <= 16 && a[3] <= 16);
            for j in i + 1..sizes.len() {
                let b = [at[j].0, at[j].1, at[j].0 + sizes[j].0, at[j].1 + sizes[j].1];
                assert!(a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1]);
            }
        }
        assert!(shelf_pack(&[(17, 1)], 16).is_none());
    }

    #[test]
    fn overlapping_rects_become_one_island() {
        let (boxes, owner) = islands(&[[0, 0, 4, 4], [2, 2, 6, 6], [10, 10, 12, 12], [5, 0, 7, 3]]);
        assert_eq!(boxes.len(), 2);
        assert_eq!(owner[0], owner[1]);
        assert_eq!(owner[0], owner[3]);
        assert_ne!(owner[0], owner[2]);
    }

    #[test]
    fn region_opacity_checks_every_frame() {
        let mut strip = Bitmap::filled(16, 32, [1, 1, 1, 255]);
        let texture = |bitmap: &Bitmap, animated| Texture {
            bitmap: bitmap.clone(),
            frame: if animated { (16, 16) } else { (16, 32) },
            animated,
            explicit_frame_size: false,
            in_pack: true,
        };
        assert_eq!(
            region_opacity(&texture(&strip, true), [0.0, 0.0, 16.0, 16.0]),
            Opacity::Opaque
        );
        strip.set(3, 20, [0, 0, 0, 0]);
        assert_eq!(
            region_opacity(&texture(&strip, true), [0.0, 0.0, 16.0, 16.0]),
            Opacity::Mixed
        );
        assert_eq!(
            region_opacity(&texture(&Bitmap::new(16, 16), false), [16.0, 0.0, 0.0, 4.0]),
            Opacity::Empty
        );
    }
}
