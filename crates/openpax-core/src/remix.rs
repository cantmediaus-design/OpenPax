//! Combining several resource packs into one ("remixing").
//!
//! Sources are ordered by priority, top first: for any file more than one source provides, the
//! highest-priority source wins unless the user picks another one for that file. Files that
//! Minecraft itself stacks across packs (language files, `sounds.json`, atlases and fonts) are
//! deep-merged instead, so no source's entries are lost.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use zip::ZipArchive;

use crate::{Error, PackMeta, Progress, Result, SIDECAR_DIR};

/// Where a remix source's files come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Folder(PathBuf),
    Zip(PathBuf),
}

impl Source {
    /// A default label: the folder or zip file name.
    pub fn default_label(&self) -> String {
        let (Self::Folder(p) | Self::Zip(p)) = self;
        p.file_name().map_or_else(
            || p.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }
}

/// The scanned file list of one source.
#[derive(Clone, Debug)]
pub struct SourceFiles {
    pub label: String,
    pub source: Source,
    /// Path inside the folder or zip at which the pack starts (`""` or `"Wrapper/"`).
    prefix: String,
    /// Pack-relative file paths, `/`-separated.
    pub files: Vec<String>,
}

impl SourceFiles {
    /// Reads one pack-relative file. Opens the zip each call; use a [`Reader`] for bulk reads.
    pub fn read(&self, rel: &str) -> Result<Vec<u8>> {
        Reader::open(self)?.read(rel)
    }

    pub fn contains(&self, rel: &str) -> bool {
        self.files.binary_search_by(|f| f.as_str().cmp(rel)).is_ok()
    }
}

/// An open source for repeated reads.
enum Reader {
    Folder(PathBuf),
    Zip(ZipArchive<BufReader<File>>, String, PathBuf),
}

impl Reader {
    fn open(src: &SourceFiles) -> Result<Self> {
        Ok(match &src.source {
            Source::Folder(root) => Self::Folder(root.join(&src.prefix)),
            Source::Zip(path) => {
                let file = File::open(path).map_err(Error::io(path))?;
                Self::Zip(
                    ZipArchive::new(BufReader::new(file))?,
                    src.prefix.clone(),
                    path.clone(),
                )
            }
        })
    }

    fn read(&mut self, rel: &str) -> Result<Vec<u8>> {
        match self {
            Self::Folder(root) => {
                let path = root.join(rel);
                fs::read(&path).map_err(Error::io(&path))
            }
            Self::Zip(archive, prefix, path) => {
                let mut entry = archive.by_name(&format!("{prefix}{rel}"))?;
                let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
                entry.read_to_end(&mut bytes).map_err(Error::io(path))?;
                Ok(bytes)
            }
        }
    }
}

/// Lists the files of a pack folder or zip, finding the pack inside a single wrapper folder.
pub fn scan_source(source: &Source) -> Result<SourceFiles> {
    let all = match source {
        Source::Folder(root) => {
            let mut out = Vec::new();
            walk(root, "", &mut out)?;
            out
        }
        Source::Zip(path) => {
            let file = File::open(path).map_err(Error::io(path))?;
            let archive = ZipArchive::new(BufReader::new(file))?;
            archive
                .file_names()
                .filter_map(|n| n.ok().map(std::borrow::Cow::into_owned))
                .filter(|n| !n.ends_with('/'))
                .collect()
        }
    };

    let root_path = match source {
        Source::Folder(p) | Source::Zip(p) => p.clone(),
    };
    let prefix = find_prefix(&all).ok_or(Error::NotAPack(root_path))?;
    let mut files: Vec<String> = all
        .iter()
        .filter_map(|f| f.strip_prefix(prefix.as_str()))
        .filter(|f| {
            !f.is_empty() && !f.starts_with('.') && !f.contains("/.") && !f.starts_with(SIDECAR_DIR)
        })
        .filter(|f| *f == "pack.mcmeta" || *f == "pack.png" || f.starts_with("assets/"))
        .map(str::to_owned)
        .collect();
    files.sort();
    files.dedup();
    Ok(SourceFiles {
        label: source.default_label(),
        source: source.clone(),
        prefix,
        files,
    })
}

/// The shallowest folder holding `pack.mcmeta` (or, failing that, an `assets/` folder).
fn find_prefix(files: &[String]) -> Option<String> {
    let shallowest = |found: &mut dyn Iterator<Item = &str>| {
        found
            .min_by_key(|p| p.matches('/').count())
            .map(str::to_owned)
    };
    shallowest(
        &mut files
            .iter()
            .filter_map(|f| f.strip_suffix("pack.mcmeta"))
            .filter(|p| p.is_empty() || p.ends_with('/')),
    )
    .or_else(|| {
        shallowest(&mut files.iter().filter_map(|f| {
            let idx = if f.starts_with("assets/") {
                0
            } else {
                f.find("/assets/")? + 1
            };
            Some(&f[..idx])
        }))
    })
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(Error::io(dir))? {
        let entry = entry.map_err(Error::io(dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let child = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if entry.file_type().map_err(Error::io(dir))?.is_dir() {
            walk(&entry.path(), &child, out)?;
        } else {
            out.push(child);
        }
    }
    Ok(())
}

/// How a file that several sources provide is combined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeKind {
    Lang,
    Sounds,
    Atlas,
    Font,
}

impl MergeKind {
    pub fn of(rel: &str) -> Option<Self> {
        let rest = rel.strip_prefix("assets/")?;
        let (_namespace, rest) = rest.split_once('/')?;
        if !rest.ends_with(".json") {
            return None;
        }
        if rest == "sounds.json" {
            Some(Self::Sounds)
        } else if rest.starts_with("lang/") {
            Some(Self::Lang)
        } else if rest.starts_with("atlases/") {
            Some(Self::Atlas)
        } else if rest.starts_with("font/") {
            Some(Self::Font)
        } else {
            None
        }
    }
}

/// A file more than one source provides.
#[derive(Clone, Debug)]
pub struct Conflict {
    pub rel_path: String,
    /// Indices into [`RemixPlan::sources`], highest priority first.
    pub candidates: Vec<usize>,
    /// Combined from all candidates unless overridden.
    pub merge: Option<MergeKind>,
}

/// The sources to combine and how conflicts are resolved.
#[derive(Clone, Debug)]
pub struct RemixPlan {
    /// Highest priority first.
    pub sources: Vec<SourceFiles>,
    pub conflicts: Vec<Conflict>,
    /// Per-file choice of source, overriding priority order (and merging).
    pub overrides: HashMap<String, usize>,
    providers: BTreeMap<String, Vec<usize>>,
}

/// Builds a plan for `sources`, ordered highest priority first.
pub fn plan(sources: Vec<SourceFiles>) -> RemixPlan {
    let mut providers: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, src) in sources.iter().enumerate() {
        for f in &src.files {
            providers.entry(f.clone()).or_default().push(i);
        }
    }
    let conflicts = providers
        .iter()
        // pack.mcmeta is generated; a texture's .mcmeta follows its texture.
        .filter(|(path, owners)| {
            owners.len() > 1 && path.as_str() != "pack.mcmeta" && !path.ends_with(".png.mcmeta")
        })
        .map(|(path, owners)| Conflict {
            rel_path: path.clone(),
            candidates: owners.clone(),
            merge: MergeKind::of(path),
        })
        .collect();
    RemixPlan {
        sources,
        conflicts,
        overrides: HashMap::new(),
        providers,
    }
}

impl RemixPlan {
    /// The sources providing `rel`, highest priority first.
    pub fn providers(&self, rel: &str) -> &[usize] {
        self.providers.get(rel).map_or(&[], Vec::as_slice)
    }

    /// Every distinct file across all sources.
    pub fn file_count(&self) -> usize {
        self.providers.len()
    }

    /// The source whose copy of `rel` ends up in the output, or `None` if it is left out.
    pub fn winner(&self, rel: &str) -> Option<usize> {
        if let Some(png) = rel.strip_suffix(".mcmeta").filter(|p| p.ends_with(".png")) {
            // Animation and filtering data belong to one specific image.
            if !self.providers(png).is_empty() {
                let owner = self.winner(png)?;
                return self.providers(rel).contains(&owner).then_some(owner);
            }
        }
        let owners = self.providers(rel);
        match self.overrides.get(rel) {
            Some(i) if owners.contains(i) => Some(*i),
            _ => owners.first().copied(),
        }
    }

    /// Whether `rel` will be produced by merging several sources.
    pub fn merges(&self, rel: &str) -> bool {
        MergeKind::of(rel).is_some()
            && self.providers(rel).len() > 1
            && !self.overrides.contains_key(rel)
    }
}

#[derive(Clone, Debug)]
pub struct RemixOptions {
    /// Only copy `assets/*/textures/**` (plus `pack.png`).
    pub textures_only: bool,
    pub description: String,
    pub format: u32,
}

fn is_texture(rel: &str) -> bool {
    rel == "pack.png"
        || rel
            .strip_prefix("assets/")
            .and_then(|r| r.split_once('/'))
            .is_some_and(|(_, r)| r.starts_with("textures/"))
}

/// Writes the combined pack to `dest`, which must be empty or not exist yet.
///
/// Returns the number of files written (including the generated `pack.mcmeta`).
pub fn execute(
    plan: &RemixPlan,
    opts: &RemixOptions,
    dest: &Path,
    progress: &mut Progress,
) -> Result<usize> {
    if dest.exists()
        && fs::read_dir(dest)
            .map_err(Error::io(dest))?
            .next()
            .is_some()
    {
        return Err(Error::AlreadyExists(dest.to_path_buf()));
    }
    let mut readers = plan
        .sources
        .iter()
        .map(Reader::open)
        .collect::<Result<Vec<_>>>()?;

    let paths: Vec<&String> = plan
        .providers
        .keys()
        .filter(|p| p.as_str() != "pack.mcmeta" && (!opts.textures_only || is_texture(p)))
        .collect();
    let total = paths.len() as u64;
    let mut written = 0;
    for (n, rel) in paths.into_iter().enumerate() {
        if n % 32 == 0 {
            progress.step("Writing remixed pack", n as u64, total)?;
        }
        let bytes = if plan.merges(rel) {
            merged(plan, &mut readers, rel)?
        } else if let Some(i) = plan.winner(rel) {
            readers[i].read(rel)?
        } else {
            continue;
        };
        let out = dest.join(rel);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(Error::io(parent))?;
        }
        fs::write(&out, bytes).map_err(Error::io(&out))?;
        written += 1;
    }

    fs::create_dir_all(dest).map_err(Error::io(dest))?;
    let mcmeta = dest.join("pack.mcmeta");
    let meta = PackMeta::new(&opts.description, opts.format);
    fs::write(&mcmeta, meta.to_json()).map_err(Error::io(&mcmeta))?;
    Ok(written + 1)
}

/// Deep-merges every source's copy of `rel`, lowest priority first.
///
/// A copy that is not valid JSON is skipped; if none is, the winner's bytes are used as-is.
fn merged(plan: &RemixPlan, readers: &mut [Reader], rel: &str) -> Result<Vec<u8>> {
    let kind = MergeKind::of(rel).expect("only mergeable files are merged");
    let mut acc: Option<Value> = None;
    for &i in plan.providers(rel).iter().rev() {
        let bytes = readers[i].read(rel)?;
        let Ok(value) = serde_json::from_slice::<Value>(strip_bom(&bytes)) else {
            continue;
        };
        acc = Some(match acc {
            None => value,
            Some(mut base) => {
                merge_json(&mut base, value, kind);
                base
            }
        });
    }
    match acc {
        Some(v) => Ok(serde_json::to_vec_pretty(&v).expect("JSON values always serialise")),
        None => {
            let winner = plan.winner(rel).expect("a provided file has a winner");
            readers[winner].read(rel)
        }
    }
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

/// Merges `top` (higher priority) into `base`.
pub fn merge_json(base: &mut Value, top: Value, kind: MergeKind) {
    match (base, top) {
        (Value::Object(base), Value::Object(top)) => merge_objects(base, top, kind),
        (base, top) => *base = top,
    }
}

fn merge_objects(base: &mut Map<String, Value>, top: Map<String, Value>, kind: MergeKind) {
    for (key, value) in top {
        match base.get_mut(&key) {
            None => {
                base.insert(key, value);
            }
            Some(existing) => match (kind, key.as_str(), existing, value) {
                // A sound event that replaces lower packs' sounds wins outright.
                (MergeKind::Sounds, _, existing @ Value::Object(_), Value::Object(event))
                    if event.get("replace").and_then(Value::as_bool) == Some(true) =>
                {
                    *existing = Value::Object(event);
                }
                (MergeKind::Sounds, "sounds", Value::Array(lower), Value::Array(higher))
                | (MergeKind::Atlas, "sources", Value::Array(lower), Value::Array(higher)) => {
                    // Later atlas sources and added sound variants extend the lower pack's list.
                    for v in higher {
                        if !lower.contains(&v) {
                            lower.push(v);
                        }
                    }
                }
                (MergeKind::Font, "providers", Value::Array(lower), Value::Array(higher)) => {
                    // Font providers are searched in order, so the higher pack's come first.
                    let mut combined = higher;
                    for v in lower.drain(..) {
                        if !combined.contains(&v) {
                            combined.push(v);
                        }
                    }
                    *lower = combined;
                }
                (_, _, existing @ Value::Object(_), value @ Value::Object(_)) => {
                    merge_json(existing, value, kind);
                }
                (_, _, existing, value) => *existing = value,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Pack;
    use serde_json::json;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn folder_pack(root: &Path, files: &[(&str, &[u8])]) -> SourceFiles {
        let pack = Pack::create(root, "src", 46).unwrap();
        for (rel, bytes) in files {
            pack.write(rel, bytes).unwrap();
        }
        scan_source(&Source::Folder(root.to_path_buf())).unwrap()
    }

    const STONE: &str = "assets/minecraft/textures/block/stone.png";
    const DIRT: &str = "assets/minecraft/textures/block/dirt.png";
    const LANG: &str = "assets/minecraft/lang/en_us.json";

    #[test]
    fn priority_overrides_and_merging() {
        let dir = tempfile::tempdir().unwrap();
        let top = folder_pack(
            &dir.path().join("top"),
            &[
                (STONE, b"top-stone"),
                (LANG, br#"{"a":"top","b":"top"}"#),
                (".openpax/x.json", b"{}"),
            ],
        );
        let bottom = folder_pack(
            &dir.path().join("bottom"),
            &[
                (STONE, b"bottom-stone"),
                (&format!("{STONE}.mcmeta"), b"{\"animation\":{}}"),
                (DIRT, b"bottom-dirt"),
                (LANG, br#"{"a":"bottom","c":"bottom"}"#),
                ("assets/minecraft/models/block/stone.json", b"{}"),
            ],
        );
        let mut plan = plan(vec![top, bottom]);
        let conflicts: Vec<&str> = plan.conflicts.iter().map(|c| c.rel_path.as_str()).collect();
        assert_eq!(conflicts, [LANG, STONE]);
        assert_eq!(plan.conflicts[0].merge, Some(MergeKind::Lang));

        let opts = RemixOptions {
            textures_only: false,
            description: "Mix".into(),
            format: 46,
        };
        let out = dir.path().join("out");
        execute(&plan, &opts, &out, &mut Progress::none()).unwrap();
        assert_eq!(fs::read(out.join(STONE)).unwrap(), b"top-stone");
        // The bottom pack's animation belonged to the bottom pack's image.
        assert!(!out.join(format!("{STONE}.mcmeta")).exists());
        assert_eq!(fs::read(out.join(DIRT)).unwrap(), b"bottom-dirt");
        let lang: Value = serde_json::from_slice(&fs::read(out.join(LANG)).unwrap()).unwrap();
        assert_eq!(lang, json!({"a":"top","c":"bottom","b":"top"}));
        assert!(!out.join(".openpax").exists());
        let meta = Pack::open(&out).unwrap().meta;
        assert_eq!(meta.description_text(), Some("Mix"));

        plan.overrides.insert(STONE.into(), 1);
        plan.overrides.insert(LANG.into(), 1);
        let out2 = dir.path().join("out2");
        execute(
            &plan,
            &RemixOptions {
                textures_only: true,
                ..opts
            },
            &out2,
            &mut Progress::none(),
        )
        .unwrap();
        assert_eq!(fs::read(out2.join(STONE)).unwrap(), b"bottom-stone");
        assert!(out2.join(format!("{STONE}.mcmeta")).exists());
        assert!(!out2.join(LANG).exists(), "textures only");
        assert!(!out2.join("assets/minecraft/models").exists());

        assert!(matches!(
            execute(&plan, &opts_default(), &out, &mut Progress::none()),
            Err(Error::AlreadyExists(_))
        ));
    }

    fn opts_default() -> RemixOptions {
        RemixOptions {
            textures_only: false,
            description: String::new(),
            format: 46,
        }
    }

    #[test]
    fn zip_with_wrapper_folder() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("p.zip");
        let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
        let opts = SimpleFileOptions::default();
        for (name, bytes) in [
            (
                "Wrapper/pack.mcmeta",
                &br#"{"pack":{"pack_format":46,"description":"z"}}"#[..],
            ),
            (
                "Wrapper/assets/minecraft/textures/block/stone.png",
                b"zip-stone",
            ),
            ("Wrapper/assets/minecraft/pack.mcmeta", b"{}"),
            ("__MACOSX/._junk", b""),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();

        let src = scan_source(&Source::Zip(zip_path)).unwrap();
        assert_eq!(src.label, "p.zip");
        assert!(src.contains(STONE));
        assert!(src.contains("pack.mcmeta"));
        assert_eq!(src.read(STONE).unwrap(), b"zip-stone");

        let plan = plan(vec![src]);
        let out = dir.path().join("out");
        assert_eq!(
            execute(&plan, &opts_default(), &out, &mut Progress::none()).unwrap(),
            3
        );
        assert_eq!(fs::read(out.join(STONE)).unwrap(), b"zip-stone");
    }

    #[test]
    fn merges_atlases_fonts_and_sounds() {
        let mut atlas = json!({"sources":[{"type":"directory","source":"block"}]});
        merge_json(
            &mut atlas,
            json!({"sources":[{"type":"directory","source":"block"},{"type":"single","resource":"x"}]}),
            MergeKind::Atlas,
        );
        assert_eq!(atlas["sources"].as_array().unwrap().len(), 2);

        let mut font = json!({"providers":[{"type":"bitmap","file":"low"}]});
        merge_json(
            &mut font,
            json!({"providers":[{"type":"bitmap","file":"high"}]}),
            MergeKind::Font,
        );
        assert_eq!(font["providers"][0]["file"], "high");
        assert_eq!(font["providers"][1]["file"], "low");

        let mut sounds = json!({"a":{"sounds":["x"]},"b":{"sounds":["y"]}});
        merge_json(
            &mut sounds,
            json!({"a":{"sounds":["z"]},"b":{"replace":true,"sounds":["w"]}}),
            MergeKind::Sounds,
        );
        assert_eq!(sounds["a"]["sounds"], json!(["x", "z"]));
        assert_eq!(sounds["b"]["sounds"], json!(["w"]));
    }

    #[test]
    fn merge_kinds() {
        assert_eq!(MergeKind::of(LANG), Some(MergeKind::Lang));
        assert_eq!(
            MergeKind::of("assets/foo/sounds.json"),
            Some(MergeKind::Sounds)
        );
        assert_eq!(
            MergeKind::of("assets/minecraft/atlases/blocks.json"),
            Some(MergeKind::Atlas)
        );
        assert_eq!(
            MergeKind::of("assets/minecraft/font/include/space.json"),
            Some(MergeKind::Font)
        );
        assert_eq!(MergeKind::of(STONE), None);
        assert_eq!(MergeKind::of("assets/minecraft/models/x.json"), None);
    }
}
