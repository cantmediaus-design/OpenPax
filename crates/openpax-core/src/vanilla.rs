//! Downloading and caching the default (vanilla) resources of a Minecraft: Java Edition version.
//!
//! The client jar is fetched from Mojang's servers, checked against its published SHA-1, and
//! only its resource-pack part (`assets/**` and `pack.png`) is extracted into a cache folder
//! that is itself a valid pack. The jar is deleted afterwards. Later requests for the same
//! version are served from the cache without touching the network.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use ureq::Agent;
use ureq::tls::{TlsConfig, TlsProvider};
use zip::ZipArchive;

use crate::{Error, PackMeta, Progress, Result};

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MANIFEST_FILE: &str = "manifest.json";
/// Written last into a version folder; folders without it are incomplete and ignored.
const COMPLETE_MARKER: &str = ".complete";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionKind {
    Release,
    Snapshot,
}

/// One downloadable game version from Mojang's version manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionEntry {
    pub id: String,
    pub kind: VersionKind,
    /// URL of the version's own JSON (which links the client jar).
    pub url: String,
    pub sha1: String,
    pub release_time: String,
}

/// The default cache folder: `%LOCALAPPDATA%\OpenPax\vanilla` (or the platform equivalent).
pub fn cache_root() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("OpenPax").join("vanilla")
}

fn agent() -> Agent {
    Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .build(),
        )
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_body(Some(Duration::from_secs(120)))
        .build()
        .into()
}

fn net(url: &str) -> impl FnOnce(ureq::Error) -> Error + '_ {
    move |e| Error::Network(format!("{url}: {e}"))
}

fn get_bytes(agent: &Agent, url: &str) -> Result<Vec<u8>> {
    agent
        .get(url)
        .call()
        .map_err(net(url))?
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(net(url))
}

fn sha1_hex(bytes: &[u8]) -> String {
    sha1_smol::Sha1::from(bytes).digest().to_string()
}

/// All release and snapshot versions, newest first.
///
/// Fetches Mojang's manifest and caches it; when offline, the cached copy is used.
pub fn fetch_manifest(cache: &Path) -> Result<Vec<VersionEntry>> {
    let path = cache.join(MANIFEST_FILE);
    let bytes = match get_bytes(&agent(), MANIFEST_URL) {
        Ok(bytes) => {
            fs::create_dir_all(cache).map_err(Error::io(cache))?;
            // A failed cache write is not worth failing the listing for.
            let _ = fs::write(&path, &bytes);
            bytes
        }
        Err(err) => fs::read(&path).map_err(|_| err)?,
    };
    parse_manifest(&bytes)
}

fn parse_manifest(bytes: &[u8]) -> Result<Vec<VersionEntry>> {
    let json: Value = serde_json::from_slice(bytes)
        .map_err(|e| Error::Format(format!("version manifest: {e}")))?;
    let versions = json["versions"]
        .as_array()
        .ok_or_else(|| Error::Format("version manifest has no `versions`".into()))?;
    Ok(versions
        .iter()
        .filter_map(|v| {
            let kind = match v["type"].as_str()? {
                "release" => VersionKind::Release,
                "snapshot" => VersionKind::Snapshot,
                _ => return None, // old_alpha / old_beta predate resource packs
            };
            Some(VersionEntry {
                id: v["id"].as_str()?.to_owned(),
                kind,
                url: v["url"].as_str()?.to_owned(),
                sha1: v["sha1"].as_str().unwrap_or_default().to_owned(),
                release_time: v["releaseTime"].as_str().unwrap_or_default().to_owned(),
            })
        })
        // Resource packs (and the `assets/` layout) arrived in 1.6.1 (2013-06-25).
        .filter(|v| v.release_time.as_str() >= "2013-06-25")
        .collect())
}

/// Folder-safe form of a version id.
fn folder_name(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The cached pack folder for `id`, if a complete one exists.
pub fn cached_version(cache: &Path, id: &str) -> Option<PathBuf> {
    let dir = cache.join(folder_name(id));
    dir.join(COMPLETE_MARKER).is_file().then_some(dir)
}

/// Ids of all versions with a complete cache folder.
pub fn cached_versions(cache: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(cache) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().join(COMPLETE_MARKER).is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    ids.sort();
    ids
}

/// Returns the cached pack folder for `version`, downloading and extracting it first if needed.
pub fn ensure_version(
    cache: &Path,
    version: &VersionEntry,
    progress: &mut Progress,
) -> Result<PathBuf> {
    if let Some(dir) = cached_version(cache, &version.id) {
        return Ok(dir);
    }
    fs::create_dir_all(cache).map_err(Error::io(cache))?;
    let agent = agent();

    progress.step("Fetching version info", 0, 0)?;
    let info_bytes = get_bytes(&agent, &version.url)?;
    if !version.sha1.is_empty() && sha1_hex(&info_bytes) != version.sha1 {
        return Err(Error::Integrity(format!("{} version info", version.id)));
    }
    let info: Value = serde_json::from_slice(&info_bytes)
        .map_err(|e| Error::Format(format!("{} version info: {e}", version.id)))?;
    let client = &info["downloads"]["client"];
    let url = client["url"]
        .as_str()
        .ok_or_else(|| Error::Format(format!("{} has no client download", version.id)))?;
    let sha1 = client["sha1"].as_str().unwrap_or_default();
    let size = client["size"].as_u64().unwrap_or(0);

    let jar = cache.join(format!("{}.jar.part", folder_name(&version.id)));
    let result = download(&agent, url, &jar, sha1, size, progress)
        .and_then(|()| install_from_jar(cache, &version.id, &jar, progress));
    let _ = fs::remove_file(&jar);
    result
}

fn download(
    agent: &Agent,
    url: &str,
    dest: &Path,
    sha1: &str,
    size: u64,
    progress: &mut Progress,
) -> Result<()> {
    let label = "Downloading client jar";
    progress.step(label, 0, size)?;
    let mut response = agent.get(url).call().map_err(net(url))?;
    let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
    let mut out = BufWriter::new(File::create(dest).map_err(Error::io(dest))?);
    let mut hasher = sha1_smol::Sha1::new();
    let mut buf = vec![0; 256 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| Error::Network(format!("{url}: {e}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(Error::io(dest))?;
        done += n as u64;
        progress.step(label, done, size.max(done))?;
    }
    out.flush().map_err(Error::io(dest))?;
    if !sha1.is_empty() && hasher.digest().to_string() != sha1 {
        return Err(Error::Integrity("client jar".into()));
    }
    Ok(())
}

/// Extracts the resource-pack part of a client jar into `cache/<id>` and marks it complete.
///
/// Works in a temporary folder that is renamed into place at the end, so an interrupted
/// extraction never looks like a finished one. The jar itself is left for the caller to delete.
pub fn install_from_jar(
    cache: &Path,
    id: &str,
    jar: &Path,
    progress: &mut Progress,
) -> Result<PathBuf> {
    let name = folder_name(id);
    let final_dir = cache.join(&name);
    let tmp = cache.join(format!("{name}.tmp"));
    let _ = fs::remove_dir_all(&tmp);
    let result = extract(id, jar, &tmp, progress).and_then(|()| {
        let _ = fs::remove_dir_all(&final_dir);
        fs::rename(&tmp, &final_dir).map_err(Error::io(&final_dir))?;
        let marker = final_dir.join(COMPLETE_MARKER);
        fs::write(&marker, id).map_err(Error::io(&marker))?;
        Ok(final_dir)
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    result
}

fn extract(id: &str, jar: &Path, dest: &Path, progress: &mut Progress) -> Result<()> {
    let file = File::open(jar).map_err(Error::io(jar))?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;

    let wanted: Vec<usize> = (0..archive.len())
        .filter(|&i| {
            archive
                .name_for_index(i)
                .and_then(|r| r.ok())
                .is_some_and(|n| {
                    let file = n.rsplit('/').next().unwrap_or_default();
                    !n.ends_with('/')
                        && (n == "pack.png" || (n.starts_with("assets/") && !file.starts_with('.')))
                })
        })
        .collect();
    if wanted.is_empty() {
        return Err(Error::Format(format!(
            "{id} contains no resource-pack assets"
        )));
    }

    let total = wanted.len() as u64;
    for (n, &i) in wanted.iter().enumerate() {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(rel);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(Error::io(parent))?;
        }
        let mut out = File::create(&out_path).map_err(Error::io(&out_path))?;
        std::io::copy(&mut entry, &mut out).map_err(Error::io(&out_path))?;
        if n % 64 == 0 {
            progress.step("Extracting default resources", n as u64, total)?;
        }
    }

    let format = archive
        .by_name("version.json")
        .ok()
        .and_then(|mut f| {
            let mut s = String::new();
            f.read_to_string(&mut s).ok()?;
            serde_json::from_str::<Value>(&s).ok()
        })
        .and_then(|v| resource_format_from_version_json(&v))
        .or_else(|| legacy_format(id))
        .unwrap_or(1);
    let meta = PackMeta::new(&format!("Minecraft {id} default resources"), format);
    let mcmeta = dest.join("pack.mcmeta");
    fs::write(&mcmeta, meta.to_json()).map_err(Error::io(&mcmeta))?;
    Ok(())
}

/// The resource pack format a client jar's `version.json` declares, in any of its historical shapes.
fn resource_format_from_version_json(v: &Value) -> Option<u32> {
    let pv = &v["pack_version"];
    let n = pv
        .as_u64()
        .or_else(|| pv["resource"].as_u64())
        .or_else(|| pv["resource_major"].as_u64())?;
    u32::try_from(n).ok()
}

/// Formats for releases whose jars predate `version.json` (before 1.14).
fn legacy_format(id: &str) -> Option<u32> {
    let mut parts = id.split('.');
    if parts.next()? != "1" {
        return None;
    }
    let minor: u32 = parts.next()?.parse().ok()?;
    Some(match minor {
        6..=8 => 1,
        9..=10 => 2,
        11..=12 => 3,
        13..=14 => 4,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Pack;
    use zip::write::SimpleFileOptions;

    fn make_jar(path: &Path, version_json: Option<&str>) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = SimpleFileOptions::default();
        let mut add = |name: &str, bytes: &[u8]| {
            zip.start_file(name, opts).unwrap();
            zip.write_all(bytes).unwrap();
        };
        add("net/minecraft/Main.class", b"code");
        add("data/minecraft/recipe/x.json", b"{}");
        add("assets/.mcassetsroot", b"");
        add("assets/minecraft/textures/block/stone.png", b"png");
        add("assets/minecraft/lang/en_us.json", br#"{"a":"b"}"#);
        add("pack.png", b"icon");
        if let Some(v) = version_json {
            add("version.json", v.as_bytes());
        }
        zip.finish().unwrap();
    }

    #[test]
    fn installs_only_resources_and_marks_complete() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        fs::create_dir_all(&cache).unwrap();
        let jar = dir.path().join("client.jar");
        make_jar(&jar, Some(r#"{"pack_version":{"resource":46,"data":61}}"#));

        let out = install_from_jar(&cache, "1.21.4", &jar, &mut Progress::none()).unwrap();
        assert_eq!(out, cache.join("1.21.4"));
        assert_eq!(cached_version(&cache, "1.21.4"), Some(out.clone()));
        assert_eq!(cached_versions(&cache), ["1.21.4"]);
        assert!(!cache.join("1.21.4.tmp").exists());

        let pack = Pack::open(&out).unwrap();
        assert_eq!(pack.meta.format(), Some(46));
        assert!(
            out.join("assets/minecraft/textures/block/stone.png")
                .is_file()
        );
        assert!(out.join("pack.png").is_file());
        assert!(!out.join("data").exists());
        assert!(!out.join("net").exists());
        assert!(!out.join("assets/.mcassetsroot").exists());
    }

    #[test]
    fn failed_install_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("broken.jar");
        fs::write(&jar, b"not a zip").unwrap();
        assert!(install_from_jar(dir.path(), "1.20", &jar, &mut Progress::none()).is_err());
        assert!(!dir.path().join("1.20.tmp").exists());
        assert!(cached_version(dir.path(), "1.20").is_none());
    }

    #[test]
    fn reads_every_pack_version_shape() {
        let f = |s: &str| resource_format_from_version_json(&serde_json::from_str(s).unwrap());
        assert_eq!(f(r#"{"pack_version":5}"#), Some(5));
        assert_eq!(f(r#"{"pack_version":{"resource":34,"data":48}}"#), Some(34));
        assert_eq!(
            f(r#"{"pack_version":{"resource_major":69,"resource_minor":0}}"#),
            Some(69)
        );
        assert_eq!(f(r#"{}"#), None);
        assert_eq!(legacy_format("1.12.2"), Some(3));
        assert_eq!(legacy_format("1.8.9"), Some(1));
        assert_eq!(legacy_format("24w10a"), None);
    }

    /// Talks to Mojang's servers; run with `cargo test -p openpax-core -- --ignored`.
    #[test]
    #[ignore = "needs network"]
    fn downloads_a_real_version() {
        let dir = tempfile::tempdir().unwrap();
        let versions = fetch_manifest(dir.path()).unwrap();
        assert!(dir.path().join(MANIFEST_FILE).is_file());
        let v = versions.iter().find(|v| v.id == "1.21.4").unwrap();
        let out = ensure_version(dir.path(), v, &mut Progress::none()).unwrap();
        assert!(
            out.join("assets/minecraft/textures/block/stone.png")
                .is_file()
        );
        assert_eq!(crate::Pack::open(&out).unwrap().meta.format(), Some(46));
        assert!(!dir.path().join("1.21.4.jar.part").exists());
    }

    #[test]
    fn manifest_keeps_releases_and_snapshots_with_assets() {
        let json = br#"{"versions":[
            {"id":"1.21.4","type":"release","url":"u1","sha1":"s1","releaseTime":"2024-12-03T10:12:57+00:00"},
            {"id":"24w14a","type":"snapshot","url":"u2","sha1":"s2","releaseTime":"2024-04-03T00:00:00+00:00"},
            {"id":"b1.7.3","type":"old_beta","url":"u3","sha1":"s3","releaseTime":"2011-07-08T00:00:00+00:00"},
            {"id":"1.5.2","type":"release","url":"u4","sha1":"s4","releaseTime":"2013-04-25T00:00:00+00:00"}
        ]}"#;
        let versions = parse_manifest(json).unwrap();
        let ids: Vec<&str> = versions.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, ["1.21.4", "24w14a"]);
        assert_eq!(versions[1].kind, VersionKind::Snapshot);
    }
}
