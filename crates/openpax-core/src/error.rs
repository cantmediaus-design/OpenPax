use std::io;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error at {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("invalid resource location `{0}`")]
    InvalidLocation(String),
    #[error("{0} is not a resource pack (no pack.mcmeta found)")]
    NotAPack(PathBuf),
    #[error("{0} already exists")]
    AlreadyExists(PathBuf),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

impl Error {
    pub(crate) fn io(path: &Path) -> impl FnOnce(io::Error) -> Error + '_ {
        move |source| Error::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    pub(crate) fn json(path: &Path) -> impl FnOnce(serde_json::Error) -> Error + '_ {
        move |source| Error::Json {
            path: path.to_path_buf(),
            source,
        }
    }
}
