use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, PackMeta, Result, TextureMeta};

/// Folder inside a pack where OpenPax keeps editor-only data (layers, etc.).
/// Minecraft ignores it and it is left out of exported zips.
pub const SIDECAR_DIR: &str = ".openpax";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AssetKind {
    Texture,
    Model,
    Blockstate,
    ItemDefinition,
    Sound,
    Lang,
    Font,
    Atlas,
    Shader,
    Json,
    Text,
    Other,
}

impl AssetKind {
    fn classify(category: &str, file_name: &str) -> Self {
        let ext = file_name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase());
        match (category, ext.as_deref()) {
            (_, Some("png")) => Self::Texture,
            ("models", Some("json")) => Self::Model,
            ("blockstates", Some("json")) => Self::Blockstate,
            ("items", Some("json")) => Self::ItemDefinition,
            ("lang", Some("json" | "lang")) => Self::Lang,
            ("font", _) => Self::Font,
            ("atlases", Some("json")) => Self::Atlas,
            ("shaders" | "post_effect", _) => Self::Shader,
            (_, Some("ogg")) => Self::Sound,
            (_, Some("json")) => Self::Json,
            (_, Some("txt" | "properties" | "md")) => Self::Text,
            _ => Self::Other,
        }
    }

    /// Whether the asset can be opened in the plain text editor.
    pub fn is_text(self) -> bool {
        !matches!(self, Self::Texture | Self::Sound | Self::Other)
    }
}

/// One file under `assets/<namespace>/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetEntry {
    pub namespace: String,
    /// First folder below the namespace (`textures`, `models`, ...); empty for loose files.
    pub category: String,
    /// Path below the category, `/`-separated, with extension.
    pub path: String,
    /// Path from the pack root, `/`-separated.
    pub rel_path: String,
    pub kind: AssetKind,
    /// Whether a sibling `.mcmeta` file exists.
    pub has_mcmeta: bool,
}

impl AssetEntry {
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

#[derive(Clone, Debug, Default)]
pub struct AssetIndex {
    /// Sorted by `rel_path`.
    pub entries: Vec<AssetEntry>,
}

impl AssetIndex {
    pub fn scan(root: &Path) -> Result<Self> {
        let mut files = Vec::new();
        let assets = root.join("assets");
        if assets.is_dir() {
            walk(&assets, "assets", &mut files)?;
        }
        files.sort();

        let mut entries = Vec::new();
        for rel_path in &files {
            if rel_path.ends_with(".mcmeta") {
                continue;
            }
            let mut parts = rel_path.splitn(4, '/').skip(1);
            let (Some(namespace), Some(second)) = (parts.next(), parts.next()) else {
                continue; // a loose file directly in assets/
            };
            let (category, path) = match parts.next() {
                Some(rest) => (second, rest),
                None => ("", second),
            };
            let file_name = path.rsplit('/').next().unwrap_or(path);
            entries.push(AssetEntry {
                namespace: namespace.to_owned(),
                category: category.to_owned(),
                path: path.to_owned(),
                rel_path: rel_path.clone(),
                kind: AssetKind::classify(category, file_name),
                has_mcmeta: files.binary_search(&format!("{rel_path}.mcmeta")).is_ok(),
            });
        }
        Ok(Self { entries })
    }

    pub fn namespaces(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.entries.iter().map(|e| e.namespace.as_str()).collect();
        out.dedup();
        out
    }
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(Error::io(dir))? {
        let entry = entry.map_err(Error::io(dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_rel = format!("{rel}/{name}");
        let path = entry.path();
        if path.is_dir() {
            walk(&path, &child_rel, out)?;
        } else {
            out.push(child_rel);
        }
    }
    Ok(())
}

/// A resource pack stored as a folder on disk.
#[derive(Clone, Debug)]
pub struct Pack {
    root: PathBuf,
    pub meta: PackMeta,
    pub index: AssetIndex,
}

impl Pack {
    /// Opens an existing pack folder (one containing `pack.mcmeta`).
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let meta_path = root.join("pack.mcmeta");
        if !meta_path.is_file() {
            return Err(Error::NotAPack(root));
        }
        let text = fs::read_to_string(&meta_path).map_err(Error::io(&meta_path))?;
        let meta = serde_json::from_str(&text).map_err(Error::json(&meta_path))?;
        let index = AssetIndex::scan(&root)?;
        Ok(Self { root, meta, index })
    }

    /// Creates a new pack in `root`, which must not already contain a pack.
    pub fn create(root: impl Into<PathBuf>, description: &str, format: u32) -> Result<Self> {
        let root = root.into();
        let meta_path = root.join("pack.mcmeta");
        if meta_path.exists() {
            return Err(Error::AlreadyExists(meta_path));
        }
        for dir in [
            "assets/minecraft/textures/block",
            "assets/minecraft/textures/item",
        ] {
            let dir = root.join(dir);
            fs::create_dir_all(&dir).map_err(Error::io(&dir))?;
        }
        let pack = Self {
            root,
            meta: PackMeta::new(description, format),
            index: AssetIndex::default(),
        };
        pack.save_meta()?;
        Ok(pack)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The pack's folder name.
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Absolute path for a pack-relative, `/`-separated path.
    pub fn abs(&self, rel_path: &str) -> PathBuf {
        self.root.join(rel_path)
    }

    pub fn save_meta(&self) -> Result<()> {
        let path = self.root.join("pack.mcmeta");
        fs::write(&path, self.meta.to_json()).map_err(Error::io(&path))
    }

    pub fn rescan(&mut self) -> Result<()> {
        self.index = AssetIndex::scan(&self.root)?;
        Ok(())
    }

    pub fn read(&self, rel_path: &str) -> Result<Vec<u8>> {
        let path = self.abs(rel_path);
        fs::read(&path).map_err(Error::io(&path))
    }

    /// Writes a file, creating parent folders as needed.
    pub fn write(&self, rel_path: &str, bytes: &[u8]) -> Result<()> {
        let path = self.abs(rel_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(Error::io(parent))?;
        }
        fs::write(&path, bytes).map_err(Error::io(&path))
    }

    pub fn exists(&self, rel_path: &str) -> bool {
        self.abs(rel_path).exists()
    }

    /// Removes a file if it exists.
    pub fn remove(&self, rel_path: &str) -> Result<()> {
        let path = self.abs(rel_path);
        match fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::io(&path)(e)),
            _ => Ok(()),
        }
    }

    /// Reads the `.mcmeta` next to a texture, if there is one.
    pub fn texture_meta(&self, texture_rel_path: &str) -> Result<Option<TextureMeta>> {
        let path = self.abs(&format!("{texture_rel_path}.mcmeta"));
        if !path.is_file() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path).map_err(Error::io(&path))?;
        serde_json::from_str(&text)
            .map(Some)
            .map_err(Error::json(&path))
    }

    /// Writes the `.mcmeta` next to a texture; an empty meta removes the file.
    pub fn write_texture_meta(&self, texture_rel_path: &str, meta: &TextureMeta) -> Result<()> {
        let rel = format!("{texture_rel_path}.mcmeta");
        if meta.is_empty() {
            self.remove(&rel)
        } else {
            self.write(&rel, meta.to_json().as_bytes())
        }
    }

    /// Pack-relative path for editor-only data belonging to an asset.
    pub fn sidecar_path(rel_path: &str, suffix: &str) -> String {
        format!("{SIDECAR_DIR}/{rel_path}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Animation;

    fn sample() -> (tempfile::TempDir, Pack) {
        let dir = tempfile::tempdir().unwrap();
        let pack = Pack::create(dir.path().join("demo"), "Demo pack", 46).unwrap();
        pack.write("assets/minecraft/textures/block/stone.png", b"png")
            .unwrap();
        pack.write("assets/minecraft/textures/block/lava.png", b"png")
            .unwrap();
        pack.write(
            "assets/minecraft/textures/block/lava.png.mcmeta",
            b"{\"animation\":{}}",
        )
        .unwrap();
        pack.write("assets/minecraft/models/item/ruby.json", b"{}")
            .unwrap();
        pack.write("assets/minecraft/sounds.json", b"{}").unwrap();
        pack.write("assets/mypack/items/ruby.json", b"{}").unwrap();
        pack.write("assets/mypack/lang/en_us.json", b"{}").unwrap();
        (dir, pack)
    }

    #[test]
    fn create_then_open_and_index() {
        let (_dir, pack) = sample();
        let pack = Pack::open(pack.root()).unwrap();
        assert_eq!(pack.name(), "demo");
        assert_eq!(pack.meta.format(), Some(46));
        assert_eq!(pack.meta.description_text(), Some("Demo pack"));
        assert_eq!(pack.index.namespaces(), vec!["minecraft", "mypack"]);

        let find = |rel: &str| {
            pack.index
                .entries
                .iter()
                .find(|e| e.rel_path == rel)
                .unwrap()
        };
        let lava = find("assets/minecraft/textures/block/lava.png");
        assert_eq!((lava.kind, lava.has_mcmeta), (AssetKind::Texture, true));
        assert_eq!(
            (lava.category.as_str(), lava.path.as_str()),
            ("textures", "block/lava.png")
        );
        assert!(!find("assets/minecraft/textures/block/stone.png").has_mcmeta);
        assert_eq!(
            find("assets/minecraft/models/item/ruby.json").kind,
            AssetKind::Model
        );
        assert_eq!(
            find("assets/mypack/items/ruby.json").kind,
            AssetKind::ItemDefinition
        );
        assert_eq!(find("assets/mypack/lang/en_us.json").kind, AssetKind::Lang);
        let sounds = find("assets/minecraft/sounds.json");
        assert_eq!(
            (sounds.kind, sounds.category.as_str()),
            (AssetKind::Json, "")
        );
        assert_eq!(
            pack.index.entries.len(),
            6,
            "mcmeta files are not listed as assets"
        );
    }

    #[test]
    fn create_refuses_existing_pack() {
        let (_dir, pack) = sample();
        assert!(matches!(
            Pack::create(pack.root(), "x", 46),
            Err(Error::AlreadyExists(_))
        ));
    }

    #[test]
    fn open_requires_pack_mcmeta() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(Pack::open(dir.path()), Err(Error::NotAPack(_))));
    }

    #[test]
    fn texture_meta_write_read_remove() {
        let (_dir, pack) = sample();
        let rel = "assets/minecraft/textures/block/stone.png";
        assert_eq!(pack.texture_meta(rel).unwrap(), None);

        let meta = TextureMeta {
            animation: Some(Animation {
                frametime: Some(3),
                ..Default::default()
            }),
            ..Default::default()
        };
        pack.write_texture_meta(rel, &meta).unwrap();
        assert_eq!(pack.texture_meta(rel).unwrap(), Some(meta));

        pack.write_texture_meta(rel, &TextureMeta::default())
            .unwrap();
        assert!(!pack.exists(&format!("{rel}.mcmeta")));
    }
}
