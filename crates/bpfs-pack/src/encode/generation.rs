use byteorder::{LittleEndian, WriteBytesExt};
use sha2::{Digest, Sha256};
use std::io::{self, Write};

use bpfs_core::constants::GENERATION_SUFFIX;
use bpfs_core::signing::sign_integrity_hash;
use bpfs_core::types::enums::CompressionType;
use bpfs_core::types::packed::{BlobEntry, DirectoryEntry, FileEntry};
use ed25519_dalek::SigningKey;

use crate::encode::{data, entries, strings};
use crate::io::writers::HashWriter;

/// One data section to be written into this generation: a name (as a string
/// table index), the compression to apply, and the raw (pre-compression)
/// concatenated bytes of every blob assigned to it, in `section_blob_idx`
/// order.
pub struct PendingDataSection {
    pub name_stridx: u32,
    pub compression: CompressionType,
    pub raw: Vec<u8>,
}

pub struct GenerationInput<'a> {
    pub created_at: u64,
    pub strings: &'a [&'a str],
    pub blobs: &'a [BlobEntry],
    pub blob_hashes: &'a [[u8; 32]],
    pub dirs: &'a [DirectoryEntry],
    pub files: &'a [FileEntry],
    pub data_sections: &'a [PendingDataSection],
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
/// Sha256 blobs_hashes[blob_count]
/// Sha256 previous_integrity_hash
/// Sha256 integrity_hash        // SHA-256 of everything above (through previous_integrity_hash)
/// u32 signature_type
/// u32 signature_size
/// u8  signature[signature_size] // signs integrity_hash; not itself covered by it
/// u32 suffix
/// ```
pub fn write_generation<W: Write>(
    writer: &mut W,
    input: &GenerationInput,
) -> io::Result<GenerationOutput> {
    assert_eq!(
        input.blobs.len(),
        input.blob_hashes.len(),
        "blobs and blob_hashes must be parallel arrays"
    );

    let mut hasher = Sha256::new();
    let mut written = 0usize;

    {
        let mut hw = HashWriter::new(writer, &mut hasher);

        hw.write_u64::<LittleEndian>(input.created_at)?;
        hw.write_u32::<LittleEndian>(u32::try_from(input.files.len()).unwrap())?;
        hw.write_u32::<LittleEndian>(u32::try_from(input.dirs.len()).unwrap())?;
        hw.write_u32::<LittleEndian>(u32::try_from(input.blobs.len()).unwrap())?;
        written += 8 + 4 + 4 + 4;

        let mut strings_buf = Vec::new();
        strings::write_string_section(&mut strings_buf, input.strings)?;
        hw.write_all(&strings_buf)?;
        written += strings_buf.len();

        for b in input.blobs {
            entries::write_blob_entry(&mut hw, b)?;
        }
        written += input.blobs.len() * entries::BLOB_ENTRY_SIZE;

        for d in input.dirs {
            entries::write_directory_entry(&mut hw, d)?;
        }
        written += input.dirs.len() * entries::DIRECTORY_ENTRY_SIZE;

        for f in input.files {
            entries::write_file_entry(&mut hw, f)?;
        }
        written += input.files.len() * entries::FILE_ENTRY_SIZE;

        hw.write_u32::<LittleEndian>(u32::try_from(input.data_sections.len()).unwrap())?;
        written += 4;

        for section in input.data_sections {
            let mut section_buf = Vec::new();
            data::write_data_section(
                &mut section_buf,
                section.name_stridx,
                section.compression,
                &section.raw,
            )?;
            hw.write_all(&section_buf)?;
            written += section_buf.len();
        }

        for h in input.blob_hashes {
            hw.write_all(h)?;
        }
        written += input.blob_hashes.len() * 32;

        hw.write_all(&input.previous_integrity_hash)?;
        written += 32;
    }

    let integrity_hash: [u8; 32] = hasher.finalize().into();
    writer.write_all(&integrity_hash)?;
    written += 32;

    match input.signing_key {
        Some(key) => {
            let sig = sign_integrity_hash(key, &integrity_hash);
            writer.write_u32::<LittleEndian>(1)?; // SignatureType::Ed25519
            writer.write_u32::<LittleEndian>(u32::try_from(sig.len()).unwrap())?;
            writer.write_all(&sig)?;
            written += 4 + 4 + sig.len();
        }
        None => {
            writer.write_u32::<LittleEndian>(0)?; // SignatureType::None
            writer.write_u32::<LittleEndian>(0)?;
            written += 8;
        }
    }

    writer.write_u32::<LittleEndian>(GENERATION_SUFFIX)?;
    written += 4;

    Ok(GenerationOutput {
        integrity_hash,
        bytes_written: written,
    })
}
