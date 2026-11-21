//! Checks a pack for problems Minecraft would only mention in its log, or show as a purple and
//! black missing texture: files that are not valid JSON, references to models, textures and
//! sounds that do not exist, names the game refuses to load, and broken animations.
//!
//! Validation reads the files on disk and never changes them.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::pack_meta::FIRST_RANGE_FORMAT;
use crate::{AssetEntry, AssetKind, Pack, Progress, ResourceLocation, Result, TextureMeta};

/// The namespace whose files the game itself provides.
const VANILLA_NAMESPACE: &str = ResourceLocation::DEFAULT_NAMESPACE;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const NAME_RULE: &str = "Minecraft only loads names made of a-z, 0-9, _ - . and /";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Something the game will not load, or will show as missing.
    Error,
    /// Something that loads but is probably not what was meant.
    Warning,
}

impl Severity {
    pub fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

/// One problem found in a pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// A short, stable name for the kind of problem (`missing-texture`, `invalid-json`...).
    pub code: &'static str,
    /// The file the problem is in, from the pack root, `/`-separated.
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// A folder holding the game's default resources (laid out as a pack). With it, references
    /// into the `minecraft` namespace are checked too; without it they are assumed to exist.
    pub vanilla: Option<PathBuf>,
}

/// What [`validate`] found.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// In file order, `pack.mcmeta` first.
    pub issues: Vec<Issue>,
    pub files_checked: usize,
    /// Whether references into the `minecraft` namespace were checked (see [`Options::vanilla`]).
    pub vanilla_checked: bool,
}

impl Report {
    pub fn count(&self, severity: Severity) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == severity)
            .count()
    }

    pub fn errors(&self) -> usize {
        self.count(Severity::Error)
    }

    pub fn warnings(&self) -> usize {
        self.count(Severity::Warning)
    }

    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Checks every file of `pack`. Fails only when cancelled through `progress`.
pub fn validate(pack: &Pack, options: &Options, progress: &mut Progress) -> Result<Report> {
    let entries = &pack.index.entries;
    let mut checker = Checker {
        pack,
        vanilla: options.vanilla.as_deref().filter(|dir| dir.is_dir()),
        files: entries.iter().map(|e| e.rel_path.as_str()).collect(),
        namespaces: entries.iter().map(|e| e.namespace.as_str()).collect(),
        generated: HashSet::new(),
        parents: HashMap::new(),
        issues: Vec::new(),
    };
    checker.read_atlases();
    checker.check_meta();
    let total = entries.len() as u64;
    for (i, entry) in entries.iter().enumerate() {
        if i % 64 == 0 {
            progress.step("Checking files", i as u64, total)?;
        }
        checker.check_entry(entry);
    }
    checker.check_parent_cycles();
    Ok(Report {
        vanilla_checked: checker.vanilla.is_some(),
        issues: checker.issues,
        files_checked: entries.len() + 1,
    })
}

struct Checker<'a> {
    pack: &'a Pack,
    vanilla: Option<&'a Path>,
    /// Every indexed file, by pack-relative path.
    files: HashSet<&'a str>,
    namespaces: HashSet<&'a str>,
    /// Textures the game makes while loading (recoloured armour trims and the like), which
    /// can be referenced although they have no file.
    generated: HashSet<String>,
    /// Model file to the file of its parent, for finding cycles.
    parents: HashMap<String, String>,
    issues: Vec<Issue>,
}

impl Checker<'_> {
    fn push(&mut self, severity: Severity, path: &str, code: &'static str, message: String) {
        self.issues.push(Issue {
            severity,
            code,
            path: path.to_owned(),
            message,
        });
    }

    fn error(&mut self, path: &str, code: &'static str, message: String) {
        self.push(Severity::Error, path, code, message);
    }

    fn warning(&mut self, path: &str, code: &'static str, message: String) {
        self.push(Severity::Warning, path, code, message);
    }

    fn exists(&self, rel_path: &str) -> bool {
        self.files.contains(rel_path)
            || self.generated.contains(rel_path)
            || self.vanilla.is_some_and(|dir| dir.join(rel_path).is_file())
    }

    /// Whether a file missing from `namespace` is really missing. The game supplies the
    /// `minecraft` namespace, and other packs or mods may supply namespaces this pack has no
    /// files in, so those references are only judged when there is something to check against.
    fn can_judge(&self, namespace: &str) -> bool {
        if namespace == VANILLA_NAMESPACE {
            self.vanilla.is_some()
        } else {
            self.namespaces.contains(namespace)
        }
    }

    /// Checks a reference such as `minecraft:block/stone` to a file in
    /// `assets/<namespace>/<folder>/<path><ext>`. Returns that file's path if the reference
    /// is well formed.
    fn reference(
        &mut self,
        from: &str,
        (what, code): (&str, &'static str),
        (folder, ext): (&str, &str),
        raw: &str,
    ) -> Option<String> {
        let Ok(location) = raw.parse::<ResourceLocation>() else {
            self.error(
                from,
                "invalid-reference",
                format!("{what} `{raw}` is not a valid name. {NAME_RULE}."),
            );
            return None;
        };
        let rel_path = format!(
            "assets/{}/{folder}/{}{ext}",
            location.namespace(),
            location.path()
        );
        if !self.exists(&rel_path) && self.can_judge(location.namespace()) {
            self.error(
                from,
                code,
                format!("{what} `{location}` does not exist (looked for {rel_path})"),
            );
        }
        Some(rel_path)
    }

    fn model_reference(&mut self, from: &str, raw: &str) {
        self.reference(from, ("model", "missing-model"), ("models", ".json"), raw);
    }

    /// Notes the textures that atlas files (the pack's and the game's) generate.
    fn read_atlases(&mut self) {
        let mut atlases: Vec<PathBuf> = self
            .pack
            .index
            .entries
            .iter()
            .filter(|e| e.kind == AssetKind::Atlas)
            .map(|e| self.pack.abs(&e.rel_path))
            .collect();
        let vanilla_atlases = self
            .vanilla
            .map(|dir| dir.join(format!("assets/{VANILLA_NAMESPACE}/atlases")))
            .and_then(|dir| std::fs::read_dir(dir).ok());
        atlases.extend(
            vanilla_atlases
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path()),
        );
        for atlas in atlases {
            // A broken atlas in the pack is reported when its turn comes.
            let json: Option<Value> = std::fs::read(&atlas)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
            if let Some(json) = json {
                generated_textures(&json, &mut self.generated);
            }
        }
        // The purple and black placeholder is built into the game.
        self.generated
            .insert(format!("assets/{VANILLA_NAMESPACE}/textures/missingno.png"));
    }

    // ---- pack.mcmeta ----

    fn check_meta(&mut self) {
        const PATH: &str = "pack.mcmeta";
        let section = &self.pack.meta.pack;
        let major = |key: &str| match section.extra.get(key) {
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::Array(a)) => a.first().and_then(Value::as_u64),
            _ => None,
        };
        let (min, max) = (major("min_format"), major("max_format"));
        let legacy = section.pack_format;
        let no_description = section.description.is_null();

        if legacy.is_none() && min.is_none() && max.is_none() {
            self.error(
                PATH,
                "no-format",
                "no pack format is declared, so Minecraft will not load the pack".to_owned(),
            );
        }
        if let (Some(min), Some(max)) = (min, max)
            && min > max
        {
            self.error(
                PATH,
                "format-range",
                format!("min_format ({min}) is higher than max_format ({max})"),
            );
        }
        if let Some(min) = min
            && legacy.is_none()
            && min < u64::from(FIRST_RANGE_FORMAT)
        {
            self.warning(
                PATH,
                "format-legacy",
                format!(
                    "min_format {min} covers game versions that only read pack_format and \
                     supported_formats, which are missing"
                ),
            );
        }
        if no_description {
            self.warning(
                PATH,
                "no-description",
                "the pack has no description".to_owned(),
            );
        }
    }

    // ---- files ----

    fn check_entry(&mut self, entry: &AssetEntry) {
        let path = entry.rel_path.as_str();
        if !matches!(entry.kind, AssetKind::Other | AssetKind::Text) {
            self.check_name(entry);
        }
        if entry.kind == AssetKind::Texture {
            self.check_texture(entry);
            return;
        }
        if !path.ends_with(".json") {
            return;
        }
        let Some(json) = self.read_json(path) else {
            return;
        };
        match entry.kind {
            AssetKind::Model => self.check_model(path, &json),
            AssetKind::Blockstate => self.check_blockstate(path, &json),
            AssetKind::ItemDefinition => self.check_item(path, &json),
            // `deprecated.json` lists removed and renamed keys; it holds no translations.
            AssetKind::Lang if entry.path != "deprecated.json" => self.check_lang(path, &json),
            AssetKind::Font => self.check_font(path, &json),
            AssetKind::Json if entry.category.is_empty() && entry.path == "sounds.json" => {
                self.check_sounds(path, &json);
            }
            _ => {}
        }
    }

    fn check_name(&mut self, entry: &AssetEntry) {
        let path = entry.rel_path.as_str();
        if !ResourceLocation::is_valid_namespace(&entry.namespace) {
            self.error(
                path,
                "invalid-name",
                format!(
                    "the namespace `{}` is not a valid name. {NAME_RULE}.",
                    entry.namespace
                ),
            );
        }
        let below = if entry.category.is_empty() {
            entry.path.clone()
        } else {
            format!("{}/{}", entry.category, entry.path)
        };
        if !ResourceLocation::is_valid_path(&below) {
            self.error(
                path,
                "invalid-name",
                format!("`{below}` is not a valid file name. {NAME_RULE}."),
            );
        }
    }

    /// Reads and parses a JSON file, reporting it if that fails.
    fn read_json(&mut self, path: &str) -> Option<Value> {
        let bytes = match self.pack.read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.error(path, "unreadable", e.to_string());
                return None;
            }
        };
        match serde_json::from_slice(&bytes) {
            Ok(json) => Some(json),
            Err(e) => {
                self.error(path, "invalid-json", format!("not valid JSON: {e}"));
                None
            }
        }
    }

    // ---- textures ----

    fn check_texture(&mut self, entry: &AssetEntry) {
        let path = entry.rel_path.as_str();
        let size = match png_size(&self.pack.abs(path)) {
            Ok(Some(size)) => size,
            Ok(None) => {
                self.error(path, "invalid-png", "not a PNG image".to_owned());
                return;
            }
            Err(e) => {
                self.error(path, "unreadable", e.to_string());
                return;
            }
        };
        if !entry.has_mcmeta {
            return;
        }
        let meta_path = format!("{path}.mcmeta");
        let meta: TextureMeta = match self.pack.texture_meta(path) {
            Ok(Some(meta)) => meta,
            Ok(None) => return,
            Err(e) => {
                let message = match &e {
                    crate::Error::Json { source, .. } => format!("not valid JSON: {source}"),
                    other => other.to_string(),
                };
                self.error(&meta_path, "invalid-json", message);
                return;
            }
        };
        let Some(animation) = &meta.animation else {
            return;
        };
        let (width, height) = size;
        let (long, short) = (width.max(height), width.min(height).max(1));
        if animation.width.is_none() && animation.height.is_none() && long % short != 0 {
            self.warning(
                &meta_path,
                "animation-size",
                format!(
                    "the texture is {width}×{height}, which does not divide into whole \
                     {short}×{short} frames"
                ),
            );
        }
        let count = animation.frame_count(width, height);
        let out_of_range = animation.frames.iter().flatten().find_map(|frame| {
            let index = match frame {
                crate::Frame::Index(index) | crate::Frame::Timed { index, .. } => *index,
            };
            (index >= count).then_some(index)
        });
        if let Some(index) = out_of_range {
            self.error(
                &meta_path,
                "animation-frame",
                format!(
                    "the animation uses frame {index}, but the texture only has {count} frame(s)"
                ),
            );
        }
    }

    // ---- models ----

    fn check_model(&mut self, path: &str, json: &Value) {
        if let Some(parent) = json.get("parent").and_then(Value::as_str)
            && !is_builtin_model(parent)
            && let Some(parent_path) = self.reference(
                path,
                ("parent model", "missing-model"),
                ("models", ".json"),
                parent,
            )
        {
            self.parents.insert(path.to_owned(), parent_path);
        }
        let Some(textures) = json.get("textures").and_then(Value::as_object) else {
            return;
        };
        for (variable, value) in textures {
            // Newer versions also accept `{ "sprite": "..." }` in place of a plain name.
            let Some(texture) = value
                .as_str()
                .or_else(|| value.get("sprite").and_then(Value::as_str))
            else {
                self.error(
                    path,
                    "invalid-model",
                    format!("texture `{variable}` is not a texture name"),
                );
                continue;
            };
            // `#name` points at another variable, possibly one a child model fills in.
            if !texture.starts_with('#') {
                self.reference(
                    path,
                    ("texture", "missing-texture"),
                    ("textures", ".png"),
                    texture,
                );
            }
        }
    }

    /// Reports models whose chain of parents leads back to themselves.
    fn check_parent_cycles(&mut self) {
        let mut in_cycle: Vec<String> = self
            .parents
            .keys()
            .filter(|start| {
                let mut seen = HashSet::new();
                let mut current = *start;
                while let Some(parent) = self.parents.get(current) {
                    if parent == *start {
                        return true;
                    }
                    if !seen.insert(parent) {
                        return false; // a cycle further up, reported for its own members
                    }
                    current = parent;
                }
                false
            })
            .cloned()
            .collect();
        in_cycle.sort();
        for path in in_cycle {
            self.error(
                &path,
                "parent-cycle",
                "the model's parents form a loop that leads back to this model".to_owned(),
            );
        }
        self.issues
            .sort_by(|a, b| sort_key(&a.path).cmp(&sort_key(&b.path)));
    }

    // ---- blockstates and item definitions ----

    fn check_blockstate(&mut self, path: &str, json: &Value) {
        let mut models = Vec::new();
        let variants = json.get("variants").and_then(Value::as_object);
        let multipart = json.get("multipart").and_then(Value::as_array);
        for variant in variants.into_iter().flat_map(|v| v.values()) {
            variant_models(variant, &mut models);
        }
        for part in multipart.into_iter().flatten() {
            if let Some(apply) = part.get("apply") {
                variant_models(apply, &mut models);
            }
        }
        if variants.is_none() && multipart.is_none() {
            self.error(
                path,
                "invalid-blockstate",
                "the blockstate has neither `variants` nor `multipart`".to_owned(),
            );
        }
        let models: Vec<&str> = models.into_iter().filter_map(Value::as_str).collect();
        for model in models {
            self.model_reference(path, model);
        }
    }

    fn check_item(&mut self, path: &str, json: &Value) {
        if json.get("model").is_none() {
            self.error(
                path,
                "invalid-item",
                "the item definition has no `model`".to_owned(),
            );
            return;
        }
        let mut models = Vec::new();
        item_models(json, &mut models);
        for model in models {
            self.model_reference(path, model);
        }
    }

    // ---- sounds, fonts, languages ----

    fn check_sounds(&mut self, path: &str, json: &Value) {
        let events = json.as_object().into_iter().flat_map(|o| o.values());
        let mut names = Vec::new();
        for event in events {
            let sounds = event.get("sounds").and_then(Value::as_array);
            for sound in sounds.into_iter().flatten() {
                // `"type": "event"` names another sound event, not a file.
                if sound.get("type").and_then(Value::as_str) == Some("event") {
                    continue;
                }
                names.extend(
                    sound
                        .as_str()
                        .or_else(|| sound.get("name").and_then(Value::as_str)),
                );
            }
        }
        for name in names {
            self.reference(path, ("sound", "missing-sound"), ("sounds", ".ogg"), name);
        }
    }

    fn check_font(&mut self, path: &str, json: &Value) {
        let providers = json.get("providers").and_then(Value::as_array);
        let files: Vec<&str> = providers
            .into_iter()
            .flatten()
            .filter(|p| {
                let kind = p.get("type").and_then(Value::as_str).unwrap_or_default();
                kind.strip_prefix("minecraft:").unwrap_or(kind) == "bitmap"
            })
            .filter_map(|p| p.get("file").and_then(Value::as_str))
            .collect();
        for file in files {
            // Bitmap providers name the texture with its extension.
            self.reference(path, ("texture", "missing-texture"), ("textures", ""), file);
        }
    }

    fn check_lang(&mut self, path: &str, json: &Value) {
        let Some(entries) = json.as_object() else {
            self.error(
                path,
                "invalid-lang",
                "a language file must be a JSON object of translations".to_owned(),
            );
            return;
        };
        let mut not_text = entries.iter().filter(|(_, v)| !v.is_string());
        if let Some((key, _)) = not_text.next() {
            let more = not_text.count();
            let others = if more == 0 {
                String::new()
            } else {
                format!(" (and {more} more)")
            };
            self.warning(
                path,
                "invalid-lang",
                format!("the translation `{key}` is not text{others}"),
            );
        }
    }
}

/// `pack.mcmeta` first, then everything else by path.
fn sort_key(path: &str) -> (bool, &str) {
    (path != "pack.mcmeta", path)
}

/// Parents the game builds in code, which have no file.
fn is_builtin_model(parent: &str) -> bool {
    parent
        .strip_prefix("minecraft:")
        .unwrap_or(parent)
        .starts_with("builtin/")
}

/// The `model` of a blockstate variant, or of each of its weighted alternatives.
fn variant_models<'a>(variant: &'a Value, out: &mut Vec<&'a Value>) {
    match variant {
        Value::Array(list) => out.extend(list.iter().filter_map(|v| v.get("model"))),
        other => out.extend(other.get("model")),
    }
}

/// The textures an atlas's `paletted_permutations` sources create: each listed texture once
/// per palette, named `<texture><separator><palette>`.
fn generated_textures(atlas: &Value, out: &mut HashSet<String>) {
    let sources = atlas.get("sources").and_then(Value::as_array);
    for source in sources.into_iter().flatten() {
        let kind = source
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if kind.strip_prefix("minecraft:").unwrap_or(kind) != "paletted_permutations" {
            continue;
        }
        let separator = source
            .get("separator")
            .and_then(Value::as_str)
            .unwrap_or("_");
        let textures = source.get("textures").and_then(Value::as_array);
        let permutations = source.get("permutations").and_then(Value::as_object);
        for texture in textures.into_iter().flatten().filter_map(Value::as_str) {
            let Ok(location) = texture.parse::<ResourceLocation>() else {
                continue;
            };
            for palette in permutations.into_iter().flat_map(|p| p.keys()) {
                out.insert(format!(
                    "assets/{}/textures/{}{separator}{palette}.png",
                    location.namespace(),
                    location.path()
                ));
            }
        }
    }
}

/// Collects the models an item definition can show, however deeply they are nested in
/// conditions, selects and composites.
fn item_models<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::Object(map) => {
            let kind = map.get("type").and_then(Value::as_str).unwrap_or_default();
            let key = match kind.strip_prefix("minecraft:").unwrap_or(kind) {
                "model" => Some("model"),
                "special" => Some("base"),
                _ => None,
            };
            out.extend(key.and_then(|k| map.get(k)).and_then(Value::as_str));
            for child in map.values() {
                item_models(child, out);
            }
        }
        Value::Array(list) => {
            for child in list {
                item_models(child, out);
            }
        }
        _ => {}
    }
}

/// The size in a PNG file's header, or `None` if the file is not a PNG.
fn png_size(path: &Path) -> std::io::Result<Option<(u32, u32)>> {
    let mut header = [0u8; 24];
    match File::open(path)?.read_exact(&mut header) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    if header[..8] != PNG_SIGNATURE || &header[12..16] != b"IHDR" {
        return Ok(None);
    }
    let be = |at: usize| {
        u32::from_be_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    Ok(Some((be(16), be(20))))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first 24 bytes of a PNG of the given size: all that validation reads.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend(13u32.to_be_bytes());
        bytes.extend(b"IHDR");
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes
    }

    fn pack(files: &[(&str, &[u8])]) -> (tempfile::TempDir, Pack) {
        let dir = tempfile::tempdir().unwrap();
        let mut pack = Pack::create(dir.path().join("demo"), "Demo", 46).unwrap();
        for (path, bytes) in files {
            pack.write(path, bytes).unwrap();
        }
        pack.rescan().unwrap();
        (dir, pack)
    }

    fn check_with(pack: &Pack, options: &Options) -> Report {
        validate(pack, options, &mut Progress::none()).unwrap()
    }

    fn check(pack: &Pack) -> Report {
        check_with(pack, &Options::default())
    }

    /// `(code, path)` of every issue.
    fn found(report: &Report) -> Vec<(&'static str, &str)> {
        report
            .issues
            .iter()
            .map(|i| (i.code, i.path.as_str()))
            .collect()
    }

    #[test]
    fn a_consistent_pack_is_clean() {
        let ruby = png(16, 16);
        let (_dir, pack) = pack(&[
            ("assets/gems/textures/item/ruby.png", &ruby),
            (
                "assets/gems/models/item/ruby.json",
                br#"{"parent":"minecraft:item/generated","textures":{"layer0":"gems:item/ruby"}}"#,
            ),
            (
                "assets/gems/items/ruby.json",
                br#"{"model":{"type":"minecraft:model","model":"gems:item/ruby"}}"#,
            ),
            (
                "assets/gems/blockstates/ruby_block.json",
                br#"{"variants":{"":{"model":"gems:item/ruby"}}}"#,
            ),
            (
                "assets/gems/lang/en_us.json",
                br#"{"item.gems.ruby":"Ruby"}"#,
            ),
            (
                "assets/gems/README.md",
                b"Notes With Capitals are fine here",
            ),
        ]);
        let report = check(&pack);
        assert_eq!(found(&report), []);
        assert!(report.is_clean());
        assert_eq!(report.files_checked, 7);
        assert!(!report.vanilla_checked);
    }

    #[test]
    fn missing_files_in_the_packs_own_namespace_are_errors() {
        let (_dir, pack) = pack(&[
            (
                "assets/gems/models/item/ruby.json",
                br##"{"parent":"gems:item/base","textures":{"layer0":"gems:item/ruby","particle":"#layer0"}}"##,
            ),
            (
                "assets/gems/items/ruby.json",
                br#"{"model":{"type":"condition","on_true":{"type":"model","model":"gems:item/ruby"},
                    "on_false":{"type":"minecraft:special","base":"gems:item/nope","model":{"type":"x"}}}}"#,
            ),
            (
                "assets/gems/blockstates/ore.json",
                br#"{"multipart":[{"apply":[{"model":"gems:block/ore"},{"model":"gems:item/ruby"}]}]}"#,
            ),
        ]);
        let report = check(&pack);
        assert_eq!(
            found(&report),
            [
                ("missing-model", "assets/gems/blockstates/ore.json"),
                ("missing-model", "assets/gems/items/ruby.json"),
                ("missing-model", "assets/gems/models/item/ruby.json"),
                ("missing-texture", "assets/gems/models/item/ruby.json"),
            ]
        );
        assert_eq!(report.errors(), 4);
        assert!(
            report.issues[3]
                .message
                .contains("assets/gems/textures/item/ruby.png")
        );
    }

    #[test]
    fn vanilla_references_are_only_judged_against_default_resources() {
        let (_dir, pack) = pack(&[(
            "assets/minecraft/models/block/stone.json",
            br#"{"parent":"block/cube_all","textures":{"all":"block/stone"},
                "x":{"parent":"builtin/generated"}}"#,
        )]);
        assert!(check(&pack).is_clean(), "nothing to check against");
        // A mod's namespace this pack has no files in is never judged.
        pack.write(
            "assets/minecraft/models/block/dirt.json",
            br#"{"parent":"create:block/shaft"}"#,
        )
        .unwrap();

        let vanilla = tempfile::tempdir().unwrap();
        let model = vanilla.path().join("assets/minecraft/models/block");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("cube_all.json"), "{}").unwrap();
        let mut pack = pack;
        pack.rescan().unwrap();
        let options = Options {
            vanilla: Some(vanilla.path().to_owned()),
        };
        let report = check_with(&pack, &options);
        assert!(report.vanilla_checked);
        assert_eq!(
            found(&report),
            [(
                "missing-texture",
                "assets/minecraft/models/block/stone.json"
            )]
        );
    }

    #[test]
    fn textures_the_game_generates_count_as_existing() {
        let (_dir, pack) = pack(&[
            (
                "assets/gems/atlases/items.json",
                br#"{"sources":[{"type":"directory","source":"item","prefix":"item/"},
                    {"type":"minecraft:paletted_permutations","textures":["gems:trims/helmet"],
                     "palette_key":"gems:trim_base","permutations":{"ruby":"gems:trim/ruby"}}]}"#,
            ),
            (
                "assets/gems/models/item/helmet.json",
                br#"{"textures":{"layer0":"gems:trims/helmet_ruby","layer1":"gems:trims/helmet_jade",
                    "particle":"minecraft:missingno"}}"#,
            ),
            ("assets/gems/lang/deprecated.json", br#"{"removed":[],"renamed":{}}"#),
        ]);
        let vanilla = tempfile::tempdir().unwrap();
        let options = Options {
            vanilla: Some(vanilla.path().to_owned()),
        };
        let report = check_with(&pack, &options);
        assert_eq!(
            found(&report),
            [("missing-texture", "assets/gems/models/item/helmet.json")]
        );
        assert!(report.issues[0].message.contains("helmet_jade"));
    }

    #[test]
    fn broken_json_and_bad_names_are_reported() {
        let image = png(16, 16);
        let (_dir, pack) = pack(&[
            (
                "assets/gems/models/item/broken.json",
                b"{\n  \"parent\": \n}",
            ),
            ("assets/gems/textures/item/Ruby Shard.png", &image),
            ("assets/Shiny/textures/item/ruby.png", &image),
            (
                "assets/gems/models/item/odd.json",
                br#"{"textures":{"layer0":"gems:Item/Ruby","layer1":7}}"#,
            ),
            (
                "assets/gems/textures/item/fake.png",
                b"not an image at all, really",
            ),
            ("assets/gems/textures/item/tiny.png", b"x"),
        ]);
        let report = check(&pack);
        assert_eq!(
            found(&report),
            [
                ("invalid-name", "assets/Shiny/textures/item/ruby.png"),
                ("invalid-json", "assets/gems/models/item/broken.json"),
                ("invalid-reference", "assets/gems/models/item/odd.json"),
                ("invalid-model", "assets/gems/models/item/odd.json"),
                ("invalid-name", "assets/gems/textures/item/Ruby Shard.png"),
                ("invalid-png", "assets/gems/textures/item/fake.png"),
                ("invalid-png", "assets/gems/textures/item/tiny.png"),
            ]
        );
        assert!(report.issues[1].message.contains("line 3"));
    }

    #[test]
    fn parent_loops_are_found() {
        let (_dir, pack) = pack(&[
            ("assets/gems/models/a.json", br#"{"parent":"gems:b"}"#),
            ("assets/gems/models/b.json", br#"{"parent":"gems:a"}"#),
            // Leads into the loop without being part of it.
            ("assets/gems/models/c.json", br#"{"parent":"gems:a"}"#),
            ("assets/gems/models/self.json", br#"{"parent":"gems:self"}"#),
        ]);
        assert_eq!(
            found(&check(&pack)),
            [
                ("parent-cycle", "assets/gems/models/a.json"),
                ("parent-cycle", "assets/gems/models/b.json"),
                ("parent-cycle", "assets/gems/models/self.json"),
            ]
        );
    }

    #[test]
    fn animations_must_fit_their_texture() {
        let strip = png(16, 48);
        let uneven = png(16, 40);
        let (_dir, pack) = pack(&[
            ("assets/gems/textures/block/ok.png", &strip),
            (
                "assets/gems/textures/block/ok.png.mcmeta",
                br#"{"animation":{"frames":[0,{"index":2,"time":4}]}}"#,
            ),
            ("assets/gems/textures/block/far.png", &strip),
            (
                "assets/gems/textures/block/far.png.mcmeta",
                br#"{"animation":{"frames":[0,3]}}"#,
            ),
            ("assets/gems/textures/block/uneven.png", &uneven),
            (
                "assets/gems/textures/block/uneven.png.mcmeta",
                br#"{"animation":{}}"#,
            ),
            ("assets/gems/textures/block/bad.png", &strip),
            ("assets/gems/textures/block/bad.png.mcmeta", b"{"),
        ]);
        let report = check(&pack);
        assert_eq!(
            found(&report),
            [
                ("invalid-json", "assets/gems/textures/block/bad.png.mcmeta"),
                (
                    "animation-frame",
                    "assets/gems/textures/block/far.png.mcmeta"
                ),
                (
                    "animation-size",
                    "assets/gems/textures/block/uneven.png.mcmeta"
                ),
            ]
        );
        assert_eq!((report.errors(), report.warnings()), (2, 1));
    }

    #[test]
    fn sounds_fonts_and_languages() {
        let glyphs = png(128, 128);
        let (_dir, pack) = pack(&[
            (
                "assets/gems/sounds.json",
                br#"{"ruby.break":{"sounds":["gems:ruby/break1",{"name":"gems:ruby/break2"},
                    {"name":"gems:other.event","type":"event"}]}}"#,
            ),
            ("assets/gems/sounds/ruby/break1.ogg", b"ogg"),
            (
                "assets/gems/font/default.json",
                br#"{"providers":[{"type":"bitmap","file":"gems:font/glyphs.png","chars":["a"]},
                    {"type":"minecraft:bitmap","file":"gems:font/lost.png","chars":["b"]},
                    {"type":"space","advances":{" ":4}}]}"#,
            ),
            ("assets/gems/textures/font/glyphs.png", &glyphs),
            ("assets/gems/lang/en_us.json", br#"{"a":"A","b":1,"c":[]}"#),
            ("assets/gems/lang/de_de.json", b"[]"),
            ("assets/gems/blockstates/empty.json", b"{}"),
            ("assets/gems/items/empty.json", b"{}"),
        ]);
        let report = check(&pack);
        assert_eq!(
            found(&report),
            [
                ("invalid-blockstate", "assets/gems/blockstates/empty.json"),
                ("missing-texture", "assets/gems/font/default.json"),
                ("invalid-item", "assets/gems/items/empty.json"),
                ("invalid-lang", "assets/gems/lang/de_de.json"),
                ("invalid-lang", "assets/gems/lang/en_us.json"),
                ("missing-sound", "assets/gems/sounds.json"),
            ]
        );
        let lang = &report.issues[4];
        assert_eq!(lang.severity, Severity::Warning);
        assert!(lang.message.contains("`b`") && lang.message.contains("1 more"));
    }

    #[test]
    fn pack_mcmeta_problems_come_first() {
        let (_dir, mut pack) = pack(&[("assets/gems/lang/en_us.json", b"nope")]);
        pack.meta =
            serde_json::from_str(r#"{"pack":{"min_format":80,"max_format":[75,0]}}"#).unwrap();
        assert_eq!(
            found(&check(&pack)),
            [
                ("format-range", "pack.mcmeta"),
                ("no-description", "pack.mcmeta"),
                ("invalid-json", "assets/gems/lang/en_us.json"),
            ]
        );

        pack.meta = serde_json::from_str(r#"{"pack":{"description":"x"}}"#).unwrap();
        assert_eq!(found(&check(&pack))[0], ("no-format", "pack.mcmeta"));

        pack.meta =
            serde_json::from_str(r#"{"pack":{"description":"x","min_format":46,"max_format":75}}"#)
                .unwrap();
        assert_eq!(found(&check(&pack))[0], ("format-legacy", "pack.mcmeta"));

        pack.meta.set_format(75);
        assert_eq!(found(&check(&pack)).len(), 1);
    }

    #[test]
    fn cancelling_stops_the_check() {
        let (_dir, pack) = pack(&[("assets/gems/lang/en_us.json", b"{}")]);
        let cancel = std::sync::atomic::AtomicBool::new(true);
        let mut progress = Progress::new(&cancel, |_, _, _| {});
        assert!(matches!(
            validate(&pack, &Options::default(), &mut progress),
            Err(crate::Error::Cancelled)
        ));
    }
}
