use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Compression error")]
    Compress,
    #[error("Decompression error")]
    Decompress,
    #[error("String table overflow")]
    StringTableOverflow,
    #[error("Archive format error: {0}")]
    Format(String),
    #[error("Checksum mismatch: {0}")]
    HashMismatch(&'static str),
    #[error("Unsupported compression type: {0}")]
    UnsupportedCompression(u8),
    #[error("Unsupported signature type: {0}")]
    UnsupportedSignature(u32),
    #[error("Signature verification failed")]
    InvalidSignature,
    #[error("Index out of bounds: {0}")]
    IndexOutOfBounds(&'static str),
}

pub type Result<T> = std::result::Result<T, ArchiveError>;
