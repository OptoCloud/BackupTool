pub const MAGIC: [u8; 4] = *b"BPFS";
pub const VERSION: u16 = 2;

/// Default uncompressed size of one data section block.
pub const DATA_BLOCK_SIZE: u32 = 64 * 1024 * 1024;

/// Marks the end of a Generation record. Distinct from `MAGIC` so a corrupt
/// stream that gets desynced doesn't silently re-align on the wrong marker.
pub const GENERATION_SUFFIX: u32 = 0x42504647; // "BPFG" (little-endian bytes)

/// SHA-256 hash of an empty input (zero-length file)
pub const EMPTY_HASH: [u8; 32] = [
    0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
    0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
];

/// SHA-256 of nothing, used as the `previous_integrity_hash` of generation 0.
pub const ZERO_HASH: [u8; 32] = [0u8; 32];
