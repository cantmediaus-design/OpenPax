use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{Error, Result};

/// Writes the pack folder at `root` to a zip Minecraft can load.
///
/// Top-level entries starting with `.` (`.openpax`, `.git`, ...) are left out.
/// Returns the number of files written.
pub fn export_zip(root: &Path, dest: &Path) -> Result<usize> {
    let mut files = Vec::new();
    collect(root, "", &mut files)?;
    files.sort();

    let file = File::create(dest).map_err(Error::io(dest))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for rel in &files {
        let path = root.join(rel);
        let bytes = fs::read(&path).map_err(Error::io(&path))?;
        zip.start_file(rel.as_str(), options)?;
        zip.write_all(&bytes).map_err(Error::io(dest))?;
    }
    zip.finish()?;
    Ok(files.len())
}

fn collect(dir: &Path, rel: &str, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(Error::io(dir))? {
        let entry = entry.map_err(Error::io(dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if rel.is_empty() && name.starts_with('.') {
            continue;
        }
        let child_rel = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let path = entry.path();
        if path.is_dir() {
            collect(&path, &child_rel, out)?;
        } else {
            out.push(child_rel);
        }
    }
    Ok(())
}

/// Extracts a pack zip into `dest`, which must be empty or not exist yet.
///
/// Entries whose paths would escape `dest` are skipped.
pub fn import_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    if dest.exists()
        && fs::read_dir(dest)
            .map_err(Error::io(dest))?
            .next()
            .is_some()
    {
        return Err(Error::AlreadyExists(dest.to_path_buf()));
    }
    let file = File::open(zip_path).map_err(Error::io(zip_path))?;
    let mut archive = ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(Error::io(&out_path))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(Error::io(parent))?;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(Error::io(zip_path))?;
        let mut out = File::create(&out_path).map_err(Error::io(&out_path))?;
        io::copy(&mut bytes.as_slice(), &mut out).map_err(Error::io(&out_path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Pack;

    #[test]
    fn export_then_import_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let pack = Pack::create(dir.path().join("src"), "Zip me", 46).unwrap();
        pack.write("assets/minecraft/textures/block/stone.png", &[1, 2, 3, 4])
            .unwrap();
        pack.write(
            ".openpax/assets/minecraft/textures/block/stone.png.layers.json",
            b"{}",
        )
        .unwrap();

        let zip_path = dir.path().join("out.zip");
        assert_eq!(export_zip(pack.root(), &zip_path).unwrap(), 2);

        let dest = dir.path().join("imported");
        import_zip(&zip_path, &dest).unwrap();
        let imported = Pack::open(&dest).unwrap();
        assert_eq!(imported.meta, pack.meta);
        assert_eq!(
            imported
                .read("assets/minecraft/textures/block/stone.png")
                .unwrap(),
            [1, 2, 3, 4]
        );
        assert!(
            !dest.join(".openpax").exists(),
            "editor data must not be exported"
        );

        assert!(matches!(
            import_zip(&zip_path, &dest),
            Err(Error::AlreadyExists(_))
        ));
    }
}
