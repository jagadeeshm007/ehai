use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Http(String),
    #[error("GitHub API error ({status}): {body}")]
    Api { status: u16, body: String },
    #[error("checksum mismatch for {name}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        name: String,
        expected: String,
        actual: String,
    },
    #[error("{name} not found in SHA256SUMS")]
    ChecksumMissing { name: String },
    #[error("signature verification failed: {0}")]
    SignatureInvalid(String),
    #[error("no release asset for this platform ({target})")]
    NoAsset { target: String },
    #[error("archive did not contain the ehai binary")]
    BinaryMissing,
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Core(#[from] ehai_core::Error),
}

impl Error {
    pub fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Error {
        let path = path.into();
        move |source| Error::Io { path, source }
    }
}
