use std::path::PathBuf;

/// Errors from the comic IO layer.
///
/// Mirrors the failure surface of the C# providers: the readers there
/// swallow almost everything into `null` returns; the writers raise
/// `WriteErrorException` for user-actionable failures. We keep the
/// distinction so Phase 1 write-back can map it the same way.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("no provider supports '{0}'")]
    UnsupportedFormat(PathBuf),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("archive access failed: {0}")]
    Access(String),
}

pub type Result<T> = std::result::Result<T, Error>;
