//! Decode and encode errors.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported image format")]
    UnsupportedFormat,
    #[error("decode failed: {0}")]
    Decode(String),
    #[error("encode failed: {0}")]
    Encode(String),
    #[error("buffer size mismatch: got {len}, expected {expected}")]
    BadBufferSize { len: usize, expected: usize },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
