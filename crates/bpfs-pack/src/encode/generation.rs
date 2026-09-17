use crate::io::le::WriteLeExt;
use sha2::{Digest, Sha256};
use std::io::{self, Write};

use bpfs_core::constants::GENERATION_SUFFIX;
use bpfs_core::signing::sign_integrity_hash;
use bpfs_core::types::enums::CompressionType;
use bpfs_core::types::packed::{BlobEntry, DirectoryEntry, FileEntry};
use ed25519_dalek::SigningKey;

use crate::encode::data::SectionEncoder;
use crate::encode::{entries, strings};
use crate::io::writers::{CountingWriter, HashWriter};

/// One data section of a generation: its name (a string table index) and the
/// compression applied to its blocks.
pub struct SectionSpec {
    pub name_stridx: u32,
    pub compression: CompressionType,
}

/// Supplies the raw (uncompressed) bytes of each data section: every blob
/// assigned to it, concatenated in `section_blob_idx` order.
pub trait SectionSource {
    fn write_section(&mut self, section_idx: usize, out: &mut dyn Write) -> io::Result<()>;
}

/// In-memory section contents, indexed by section.
impl SectionSource for Vec<Vec<u8>> {
    fn write_section(&mut self, section_idx: usize, out: &mut dyn Write) -> io::Result<()> {
        out.write_all(&self[section_idx])
    }
}

pub struct GenerationInput<'a> {
    pub created_at: u64,
    pub strings: &'a [&'a str],
    pub blobs: &'a [BlobEntry],
    pub blob_hashes: &'a [[u8; 32]],
    pub dirs: &'a [DirectoryEntry],
    pub files: &'a [FileEntry],
    pub data_sections: &'a [SectionSpec],
    pub block_size: u32,
    pub previous_integrity_hash: [u8; 32],
    pub signing_key: Option<&'a SigningKey>,
}

pub struct GenerationOutput {
    pub integrity_hash: [u8; 32],
    pub bytes_written: usize,
}

/// Writes one full `Generation` record. Layout (see `imhex.pattern`):
///
/// ```text
/// u64 created_at
/// u32 file_count, dir_count, blob_count
/// StringSection strings
/// BlobEntry blobs[blob_count]
/// DirectoryEntry dirs[dir_count]
/// FileEntry files[file_count]
/// u32 data_section_count
/// DataSection data_sections[data_section_count]
/// u8  blob_hashes[blob_count][32]
/// u8  previous_integrity_hash[32]
/// u8  integrity_hash[32]          // SHA-256 of everything above
/// u32 signature_type
/// u32 signature_size
/// u8  signature[signature_size]   // signs integrity_hash; not itself covered by it
/// u32 suffix
/// ```
pub fn write_generation<W: Write>(
    writer: &mut W,
    input: &GenerationInput,
    sections: &mut dyn SectionSource,
) -> io::Result<GenerationOutput> {
    write_generation_with_progress(writer, input, sections, &|_| {})
}

/// Like [`write_generation`], calling `on_block_done` (possibly from worker
/// threads) with the uncompressed size of each data block once compressed.
pub fn write_generation_with_progress<W: Write>(
    writer: &mut W,
    input: &GenerationInput,
    sections: &mut dyn SectionSource,
    on_block_done: &(dyn Fn(u64) + Sync),
) -> io::Result<GenerationOutput> {
    assert_eq!(
        input.blobs.len(),
        input.blob_hashes.len(),
        "blobs and blob_hashes must be parallel arrays"
    );

    let mut out = CountingWriter::new(writer);
    let mut hasher = Sha256::new();

    {
        let mut hw = HashWriter::new(&mut out, &mut hasher);

        hw.write_u64_le(input.created_at)?;
        hw.write_u32_le(u32::try_from(input.files.len()).unwrap())?;
        hw.write_u32_le(u32::try_from(input.dirs.len()).unwrap())?;
        hw.write_u32_le(u32::try_from(input.blobs.len()).unwrap())?;

        let mut strings_buf = Vec::new();
        strings::write_string_section(&mut strings_buf, input.strings)?;
        hw.write_all(&strings_buf)?;

        for b in input.blobs {
            entries::write_blob_entry(&mut hw, b)?;
        }
        for d in input.dirs {
            entries::write_directory_entry(&mut hw, d)?;
        }
        for f in input.files {
            entries::write_file_entry(&mut hw, f)?;
        }

        hw.write_u32_le(u32::try_from(input.data_sections.len()).unwrap())?;
        for (idx, spec) in input.data_sections.iter().enumerate() {
            let mut encoder = SectionEncoder::new(
                &mut hw,
                spec.name_stridx,
                spec.compression,
                input.block_size,
                on_block_done,
            )?;
            sections.write_section(idx, &mut encoder)?;
            encoder.finish()?;
        }

        for h in input.blob_hashes {
            hw.write_all(h)?;
        }
        hw.write_all(&input.previous_integrity_hash)?;
    }

    let integrity_hash: [u8; 32] = hasher.finalize().into();
    out.write_all(&integrity_hash)?;

    match input.signing_key {
        Some(key) => {
            let sig = sign_integrity_hash(key, &integrity_hash);
            out.write_u32_le(1)?; // SignatureType::Ed25519
            out.write_u32_le(u32::try_from(sig.len()).unwrap())?;
            out.write_all(&sig)?;
        }
        None => {
            out.write_u32_le(0)?; // SignatureType::None
            out.write_u32_le(0)?;
        }
    }

    out.write_u32_le(GENERATION_SUFFIX)?;

    Ok(GenerationOutput {
        integrity_hash,
        bytes_written: out.bytes_written(),
    })
}
