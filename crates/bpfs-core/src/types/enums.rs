#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressionType {
    None = 0,   // No compression
    Lz4 = 1,    // Very fast, modest ratio
    Zstd = 2,   // Fast with a good ratio (default)
    Brotli = 3, // Great for web assets
    Lzma = 5,   // High ratio, slow (archive)
}

impl TryFrom<u8> for CompressionType {
    type Error = u8;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(CompressionType::None),
            1 => Ok(CompressionType::Lz4),
            2 => Ok(CompressionType::Zstd),
            3 => Ok(CompressionType::Brotli),
            5 => Ok(CompressionType::Lzma),
            other => Err(other),
        }
    }
}

/// Which physical `DataSection` of a generation a blob's bytes were written into.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    /// Bytes known/assumed to already be compressed (media, archives, encrypted data) —
    /// stored as-is, concatenated with no further compression.
    Stored = 0,
    /// Everything else — concatenated then compressed as one unit for better ratio.
    Compressed = 1,
}

impl SectionKind {
    pub const COUNT: usize = 2;

    pub fn name(self) -> &'static str {
        match self {
            SectionKind::Stored => "stored",
            SectionKind::Compressed => "compressed",
        }
    }
}

impl TryFrom<u32> for SectionKind {
    type Error = u32;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(SectionKind::Stored),
            1 => Ok(SectionKind::Compressed),
            other => Err(other),
        }
    }
}

/// Signature scheme used to sign a generation's integrity hash.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureType {
    None = 0,
    Ed25519 = 1,
}

impl TryFrom<u32> for SignatureType {
    type Error = u32;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(SignatureType::None),
            1 => Ok(SignatureType::Ed25519),
            other => Err(other),
        }
    }
}
