//! Renaming, duplicating and deleting assets together with the files that belong to them:
//! the `.mcmeta` next to a texture and editor data in the `.openpax` sidecar folder.

use std::fs;
use std::path::Path;

use crate::{Error, Pack, Result, SIDECAR_DIR};

/// Folder inside the sidecar folder that deleted assets are moved to, one subfolder per delete.
pub const TRASH_DIR: &str = ".openpax/trash";

/// The parent folder and file name of a `/`-separated path.
fn split(rel_path: &str) -> (&str, &str) {
    rel_path.rsplit_once('/').unwrap_or(("", rel_path))
}

impl Pack {
    /// Files that belong to an asset, each with the suffix that follows the asset's own path
    /// (`""` for the asset itself). Only files that exist are listed.
    fn companions(&self, rel_path: &str) -> Result<Vec<(String, String)>> {
        let mut out = Vec::new();
        if self.abs(rel_path).is_file() {
            out.push((rel_path.to_owned(), String::new()));
        }
        let mcmeta = format!("{rel_path}.mcmeta");
        if self.abs(&mcmeta).is_file() {
            out.push((mcmeta, ".mcmeta".to_owned()));
        }
        // Sidecar files are named `<asset file name><suffix>` in a mirror of the asset's folder.
        let (folder, name) = split(rel_path);
        let sidecar_folder = if folder.is_empty() {
            SIDECAR_DIR.to_owned()
        } else {
            format!("{SIDECAR_DIR}/{folder}")
        };
        let dir = self.abs(&sidecar_folder);
        if dir.is_dir() {
            let prefix = format!("{name}.");
            for entry in fs::read_dir(&dir).map_err(Error::io(&dir))? {
                let entry = entry.map_err(Error::io(&dir))?;
                let file_name = entry.file_name().to_string_lossy().into_owned();
                if entry.path().is_file()
                    && let Some(rest) = file_name.strip_prefix(&prefix)
                {
                    out.push((
                        format!("{sidecar_folder}/{file_name}"),
                        format!("{SIDECAR_DIR}:.{rest}"),
                    ));
                }
            }
        }
        Ok(out)
    }

    /// Where a companion of `rel_path` with `suffix` lives for an asset at `target`.
    fn companion_path(target: &str, suffix: &str) -> String {
        match suffix.strip_prefix(&format!("{SIDECAR_DIR}:")) {
            Some(sidecar) => format!("{SIDECAR_DIR}/{target}{sidecar}"),
            None => format!("{target}{suffix}"),
        }
    }

    /// Pairs of (current path, new path) for moving or copying an asset to `to`. Fails if any
    /// destination already exists, so nothing is overwritten.
    fn plan(&self, from: &str, to: &str) -> Result<Vec<(String, String)>> {
        if from == to {
            return Err(Error::AlreadyExists(self.abs(to)));
        }
        let pairs: Vec<(String, String)> = self
            .companions(from)?
            .into_iter()
            .map(|(path, suffix)| {
                let target = Self::companion_path(to, &suffix);
                (path, target)
            })
            .collect();
        if !pairs.iter().any(|(path, _)| path == from) {
            let path = self.abs(from);
            return Err(Error::io(&path)(std::io::ErrorKind::NotFound.into()));
        }
        if let Some((_, target)) = pairs.iter().find(|(_, target)| self.exists(target)) {
            return Err(Error::AlreadyExists(self.abs(target)));
        }
        Ok(pairs)
    }

    /// Renames or moves an asset along with its `.mcmeta` and sidecar files.
    pub fn move_asset(&self, from: &str, to: &str) -> Result<()> {
        for (path, target) in self.plan(from, to)? {
            move_file(&self.abs(&path), &self.abs(&target))?;
        }
        Ok(())
    }

    /// Copies an asset along with its `.mcmeta` and sidecar files.
    pub fn copy_asset(&self, from: &str, to: &str) -> Result<()> {
        for (path, target) in self.plan(from, to)? {
            let (source, target) = (self.abs(&path), self.abs(&target));
            create_parent(&target)?;
            fs::copy(&source, &target).map_err(Error::io(&source))?;
        }
        Ok(())
    }

    /// Moves an asset and its companions into a new folder under [`TRASH_DIR`], keeping their
    /// paths, and returns that folder (pack-relative) for [`Self::restore_trashed`].
    pub fn trash_asset(&self, rel_path: &str) -> Result<String> {
        let companions = self.companions(rel_path)?;
        if companions.is_empty() {
            let path = self.abs(rel_path);
            return Err(Error::io(&path)(std::io::ErrorKind::NotFound.into()));
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let mut folder = format!("{TRASH_DIR}/{stamp}");
        let mut n = 1;
        while self.exists(&folder) {
            folder = format!("{TRASH_DIR}/{stamp}-{n}");
            n += 1;
        }
        for (path, _) in companions {
            move_file(&self.abs(&path), &self.abs(&format!("{folder}/{path}")))?;
        }
        Ok(folder)
    }

    /// Puts back everything [`Self::trash_asset`] moved into `trash_folder`, refusing if any
    /// of it would overwrite a file. Returns the restored paths.
    pub fn restore_trashed(&self, trash_folder: &str) -> Result<Vec<String>> {
        let root = self.abs(trash_folder);
        let mut files = Vec::new();
        collect_files(&root, "", &mut files)?;
        if let Some(existing) = files.iter().find(|f| self.exists(f)) {
            return Err(Error::AlreadyExists(self.abs(existing)));
        }
        for rel in &files {
            move_file(&root.join(rel), &self.abs(rel))?;
        }
        let _ = fs::remove_dir_all(&root);
        Ok(files)
    }
}

fn create_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(Error::io(parent))?;
    }
    Ok(())
}

fn move_file(from: &Path, to: &Path) -> Result<()> {
    create_parent(to)?;
    fs::rename(from, to).map_err(Error::io(from))
}

fn collect_files(dir: &Path, rel: &str, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(Error::io(dir))? {
        let entry = entry.map_err(Error::io(dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if entry.path().is_dir() {
            collect_files(&entry.path(), &child, out)?;
        } else {
            out.push(child);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STONE: &str = "assets/minecraft/textures/block/stone.png";

    fn pack() -> (tempfile::TempDir, Pack) {
        let dir = tempfile::tempdir().unwrap();
        let pack = Pack::create(dir.path().join("p"), "test", 46).unwrap();
        pack.write(STONE, b"png").unwrap();
        pack.write(&format!("{STONE}.mcmeta"), b"{}").unwrap();
        pack.write(&Pack::sidecar_path(STONE, ".layers.json"), b"[]")
            .unwrap();
        pack.write(&Pack::sidecar_path(STONE, ".layers.png"), b"strip")
            .unwrap();
        // Similar names that do not belong to stone.png.
        pack.write("assets/minecraft/textures/block/stone.png2", b"x")
            .unwrap();
        pack.write(
            &Pack::sidecar_path("assets/minecraft/textures/block/stone2.png", ".layers.json"),
            b"[]",
        )
        .unwrap();
        (dir, pack)
    }

    #[test]
    fn moving_takes_the_metadata_and_layers_along() {
        let (_dir, pack) = pack();
        let to = "assets/minecraft/textures/item/rock.png";
        pack.move_asset(STONE, to).unwrap();
        assert!(!pack.exists(STONE));
        assert_eq!(pack.read(to).unwrap(), b"png");
        assert!(pack.exists(&format!("{to}.mcmeta")));
        assert_eq!(
            pack.read(&Pack::sidecar_path(to, ".layers.png")).unwrap(),
            b"strip"
        );
        assert!(!pack.exists(&Pack::sidecar_path(STONE, ".layers.json")));
        assert!(pack.exists("assets/minecraft/textures/block/stone.png2"));
        assert!(pack.exists(&Pack::sidecar_path(
            "assets/minecraft/textures/block/stone2.png",
            ".layers.json"
        )));
    }

    #[test]
    fn copying_keeps_the_original_and_never_overwrites() {
        let (_dir, pack) = pack();
        let to = "assets/minecraft/textures/block/stone_copy.png";
        pack.copy_asset(STONE, to).unwrap();
        assert!(pack.exists(STONE) && pack.exists(to));
        assert!(pack.exists(&Pack::sidecar_path(to, ".layers.json")));
        assert!(matches!(
            pack.copy_asset(STONE, to),
            Err(Error::AlreadyExists(_))
        ));
        assert!(matches!(
            pack.move_asset(STONE, STONE),
            Err(Error::AlreadyExists(_))
        ));
        assert!(pack.move_asset("assets/nothing.png", to).is_err());
    }

    #[test]
    fn trash_and_restore_round_trip() {
        let (_dir, pack) = pack();
        let folder = pack.trash_asset(STONE).unwrap();
        assert!(folder.starts_with(TRASH_DIR));
        assert!(!pack.exists(STONE));
        assert!(!pack.exists(&format!("{STONE}.mcmeta")));
        assert!(!pack.exists(&Pack::sidecar_path(STONE, ".layers.json")));

        // A new file in the old place blocks the restore rather than being overwritten.
        pack.write(STONE, b"new").unwrap();
        assert!(pack.restore_trashed(&folder).is_err());
        pack.remove(STONE).unwrap();

        let restored = pack.restore_trashed(&folder).unwrap();
        assert_eq!(restored.len(), 4);
        assert_eq!(pack.read(STONE).unwrap(), b"png");
        assert!(pack.exists(&Pack::sidecar_path(STONE, ".layers.png")));
        assert!(!pack.exists(&folder));
    }

    #[test]
    fn trash_is_left_out_of_the_asset_index() {
        let (_dir, mut pack) = pack();
        pack.trash_asset(STONE).unwrap();
        pack.rescan().unwrap();
        assert!(
            pack.index
                .entries
                .iter()
                .all(|e| !e.rel_path.contains("stone.png."))
        );
        assert!(!pack.index.entries.iter().any(|e| e.rel_path == STONE));
    }
}
