//! Folders for the user's own brushes, palettes, scripts and export presets.

use std::path::PathBuf;

/// `%APPDATA%\OpenPax\<sub>` on Windows, `~/.config/openpax/<sub>` elsewhere. Created on
/// first use.
pub fn dir(sub: &str) -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(|p| PathBuf::from(p).join("OpenPax"))
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(|p| PathBuf::from(p).join("openpax")))
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config/openpax")))
        .unwrap_or_else(|| PathBuf::from(".openpax-user"));
    let dir = base.join(sub);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// The scripts folder, seeded with the example scripts the first time.
pub fn scripts_dir() -> PathBuf {
    let dir = dir("scripts");
    let empty = std::fs::read_dir(&dir)
        .map(|mut d| d.next().is_none())
        .unwrap_or(true);
    if empty {
        for (name, source) in openpax_script::EXAMPLES {
            let _ = std::fs::write(dir.join(name), source);
        }
    }
    dir
}

/// Opens a folder in the system file browser.
pub fn reveal(path: &std::path::Path) {
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    let _ = std::process::Command::new(program).arg(path).spawn();
}
