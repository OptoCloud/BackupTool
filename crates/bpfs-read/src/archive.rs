use crate::io::ReadLeExt;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex};

use bpfs_core::constants::{MAGIC, VERSION};
use bpfs_core::errors::{ArchiveError, Result};
use bpfs_core::progress::{Monitor, NoMonitor, Phase, Progress};
use bpfs_core::types::packed::BlobEntry;

use crate::decode::data::read_block;
use crate::generation::{read_generation, Generation, NOT_OWNED};

/// An open BPFS archive: every generation's metadata in memory, data read
/// from `R` on demand.
pub struct Archive<R> {
    pub version: u16,
    pub flags: u16,
    pub generations: Vec<Generation>,
    source: Mutex<Source<R>>,
}

/// (generation, section, block)
type BlockKey = (usize, usize, usize);

struct Source<R> {
    reader: R,
    /// The most recently decompressed block.
    cached: Option<(BlockKey, Arc<Vec<u8>>)>,
}

/// Opens a BPFS archive: parses the header and every generation's metadata
/// up to EOF and checks the recorded hash chain. Data is not read; use
/// [`crate::verify`] to check hashes.
pub fn read_archive<R: Read + Seek>(reader: R) -> Result<Archive<R>> {
    read_archive_with_monitor(reader, &NoMonitor)
}

/// Like [`read_archive`], reporting each generation opened.
pub fn read_archive_with_monitor<R: Read + Seek>(
    mut reader: R,
    monitor: &dyn Monitor,
) -> Result<Archive<R>> {
    let start = reader.stream_position()?;
    let total_len = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(start))?;

    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(ArchiveError::Format("not a BPFS archive".into()));
    }
    let version = reader.read_u16_le()?;
    if version != VERSION {
        return Err(ArchiveError::Format(format!(
            "unsupported archive version {version} (this build reads version {VERSION})"
        )));
    }
    let flags = reader.read_u16_le()?;
    let mut reserved = [0u8; 8];
    reader.read_exact(&mut reserved)?;

    let mut generations: Vec<Generation> = Vec::new();
    loop {
        let pos = reader.stream_position()?;
        if pos >= total_len {
            break;
        }
        monitor.checkpoint()?;
        monitor.progress(Progress {
            phase: Phase::Opening,
            done: pos,
            total: total_len,
            item: None,
        });
        let idx = u32::try_from(generations.len())
            .map_err(|_| ArchiveError::Format("too many generations".into()))?;
        let gen = read_generation(&mut reader, idx)?;

        let expected_previous = generations.last().map_or([0u8; 32], |g| g.integrity_hash);
        if gen.previous_integrity_hash != expected_previous {
            return Err(ArchiveError::HashMismatch("generation hash chain"));
        }
        generations.push(gen);
    }

    if generations.is_empty() {
        return Err(ArchiveError::Format(
            "archive contains no generations".into(),
        ));
    }

    Ok(Archive {
        version,
        flags,
        generations,
        source: Mutex::new(Source {
            reader,
            cached: None,
        }),
    })
}

impl<R> Archive<R> {
    pub fn latest(&self) -> &Generation {
        self.generations
            .last()
            .expect("archives always have at least one generation")
    }
}

impl<R: Read + Seek> Archive<R> {
    /// Opens a reader over a blob's bytes, wherever they are stored (a blob
    /// may be deduplicated against an earlier generation).
    pub fn blob_reader(&self, blob: &BlobEntry) -> Result<BlobReader<'_, R>> {
        let gen_idx = blob.generation_idx as usize;
        let owner = self
            .generations
            .get(gen_idx)
            .ok_or(ArchiveError::IndexOutOfBounds("BlobEntry.generation_idx"))?;
        let local = *owner
            .section_blobs
            .get(blob.section_idx as usize)
            .and_then(|s| s.get(blob.section_blob_idx as usize))
            .ok_or_else(|| ArchiveError::Format("blob not found in its generation".into()))?;
        let offset = owner.blob_offsets[local];
        if offset == NOT_OWNED || owner.blobs[local].raw_size != blob.raw_size {
            return Err(ArchiveError::Format("inconsistent blob reference".into()));
        }
        Ok(BlobReader {
            archive: self,
            gen_idx,
            section_idx: blob.section_idx as usize,
            pos: offset,
            remaining: blob.raw_size,
        })
    }

    /// Reads a whole blob into memory.
    pub fn read_blob(&self, blob: &BlobEntry) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(blob.raw_size as usize);
        self.blob_reader(blob)?.read_to_end(&mut out)?;
        Ok(out)
    }

    fn block(
        &self,
        gen_idx: usize,
        section_idx: usize,
        block_idx: usize,
    ) -> io::Result<Arc<Vec<u8>>> {
        let mut source = self.source.lock().unwrap();
        let key = (gen_idx, section_idx, block_idx);
        if let Some((cached_key, data)) = &source.cached {
            if *cached_key == key {
                return Ok(data.clone());
            }
        }
        let section = &self.generations[gen_idx].data_sections[section_idx];
        let data = Arc::new(read_block(
            &mut source.reader,
            section.compression,
            &section.blocks[block_idx],
        )?);
        source.cached = Some((key, data.clone()));
        Ok(data)
    }

    /// Runs `f` with exclusive access to the underlying reader.
    pub(crate) fn with_reader<T>(&self, f: impl FnOnce(&mut R) -> io::Result<T>) -> io::Result<T> {
        f(&mut self.source.lock().unwrap().reader)
    }
}

/// Streams one blob's bytes, decompressing only the blocks it spans.
pub struct BlobReader<'a, R> {
    archive: &'a Archive<R>,
    gen_idx: usize,
    section_idx: usize,
    /// Current offset within the section's raw stream.
    pos: u64,
    remaining: u64,
}

impl<R: Read + Seek> Read for BlobReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 || buf.is_empty() {
            return Ok(0);
        }
        let section = &self.archive.generations[self.gen_idx].data_sections[self.section_idx];
        let block_idx = section.block_at(self.pos).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "blob extends past its data section",
            )
        })?;
        let block_start = section.blocks[block_idx].raw_offset;
        let data = self
            .archive
            .block(self.gen_idx, self.section_idx, block_idx)?;

        let within = (self.pos - block_start) as usize;
        let n = buf
            .len()
            .min(data.len() - within)
            .min(self.remaining.min(usize::MAX as u64) as usize);
        buf[..n].copy_from_slice(&data[within..within + n]);
        self.pos += n as u64;
        self.remaining -= n as u64;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    use bpfs_core::constants::DATA_BLOCK_SIZE;
    use bpfs_core::types::enums::CompressionType;
    use bpfs_pack::pack::{
        pack_directory, ExistingBlob, ExistingBlobLookup, NoExistingBlobs, PackOptions,
    };
    use bpfs_pack::policy::compression::DefaultCompressionPolicy;
    use std::fs;
    use tempfile::tempdir;

    const POLICY: DefaultCompressionPolicy = DefaultCompressionPolicy {
        incompressible_entropy: 96.25,
    };

    fn options() -> PackOptions<'static> {
        PackOptions {
            generation_idx: 0,
            previous_integrity_hash: [0u8; 32],
            policy: &POLICY,
            compression: CompressionType::Zstd,
            block_size: DATA_BLOCK_SIZE,
            signing_key: None,
            existing_blobs: &NoExistingBlobs,
        }
    }

    #[test]
    fn parses_single_generation_archive() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"hello world").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.bin"), b"\x00\x01binary").unwrap();

        let mut buf = Vec::new();
        pack_directory(&mut buf, dir.path(), true, options()).unwrap();

        let archive = read_archive(Cursor::new(buf)).unwrap();
        assert_eq!(archive.generations.len(), 1);
        assert_eq!(archive.latest().files.len(), 2);
    }

    #[test]
    fn rejects_bad_magic() {
        let buf = vec![0u8; 32];
        assert!(read_archive(Cursor::new(buf)).is_err());
    }

    #[test]
    fn rejects_other_versions() {
        let dir = tempdir().unwrap();
        let mut buf = Vec::new();
        pack_directory(&mut buf, dir.path(), true, options()).unwrap();
        buf[4] = 1; // version 1
        let err = read_archive(Cursor::new(buf)).err().unwrap();
        assert!(err.to_string().contains("version 1"), "{err}");
    }

    #[test]
    fn blobs_resolve_to_their_bytes_across_blocks_and_codecs() {
        let dir = tempdir().unwrap();
        let files: Vec<(String, Vec<u8>)> = (0..12)
            .map(|i| {
                let len = 100 + i * 377;
                let body = (0..len).map(|j| ((j * (i + 1)) % 251) as u8).collect();
                (format!("f{i}.txt"), body)
            })
            .collect();
        for (name, body) in &files {
            fs::write(dir.path().join(name), body).unwrap();
        }
        fs::write(dir.path().join("photo.jpg"), vec![9u8; 5000]).unwrap();

        for compression in [
            CompressionType::None,
            CompressionType::Brotli,
            CompressionType::Zstd,
        ] {
            let mut buf = Vec::new();
            let opts = PackOptions {
                compression,
                block_size: 1000, // files straddle many blocks
                ..options()
            };
            pack_directory(&mut buf, dir.path(), true, opts).unwrap();

            let archive = read_archive(Cursor::new(buf)).unwrap();
            let gen = archive.latest();
            assert!(gen.data_sections.iter().any(|s| s.blocks.len() > 1));
            for f in &gen.files {
                let name = gen.strings.get(f.name_stridx).unwrap();
                let blob = &gen.blobs[f.blob_idx as usize];
                let expected = fs::read(dir.path().join(name)).unwrap();
                assert_eq!(
                    archive.read_blob(blob).unwrap(),
                    expected,
                    "{name} {compression:?}"
                );
            }
        }
    }

    #[test]
    fn multi_generation_chain_verifies() {
        let dir1 = tempdir().unwrap();
        fs::write(dir1.path().join("a.txt"), b"generation zero").unwrap();

        let mut buf = Vec::new();
        let summary0 = pack_directory(&mut buf, dir1.path(), true, options()).unwrap();

        let dir2 = tempdir().unwrap();
        fs::write(dir2.path().join("b.txt"), b"generation one").unwrap();
        let opts = PackOptions {
            generation_idx: 1,
            previous_integrity_hash: summary0.integrity_hash,
            ..options()
        };
        pack_directory(&mut buf, dir2.path(), false, opts).unwrap();

        let archive = read_archive(Cursor::new(buf)).unwrap();
        assert_eq!(archive.generations.len(), 2);
        assert_eq!(
            archive.generations[1].previous_integrity_hash,
            archive.generations[0].integrity_hash
        );
    }

    #[test]
    fn broken_hash_chain_is_rejected() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"x").unwrap();

        let mut buf = Vec::new();
        pack_directory(&mut buf, dir.path(), true, options()).unwrap();
        let opts = PackOptions {
            generation_idx: 1,
            previous_integrity_hash: [7u8; 32],
            ..options()
        };
        pack_directory(&mut buf, dir.path(), false, opts).unwrap();
        assert!(read_archive(Cursor::new(buf)).is_err());
    }

    #[test]
    fn cross_generation_blob_dedup_avoids_rewriting_bytes() {
        let dir1 = tempdir().unwrap();
        fs::write(dir1.path().join("a.txt"), b"shared content").unwrap();

        let mut buf = Vec::new();
        let summary0 = pack_directory(&mut buf, dir1.path(), true, options()).unwrap();

        let archive_gen0 = read_archive(Cursor::new(buf.clone())).unwrap();
        let gen0 = archive_gen0.latest();

        struct LookupGen0 {
            hash: [u8; 32],
            blob: BlobEntry,
        }
        impl ExistingBlobLookup for LookupGen0 {
            fn find(&self, hash: &[u8; 32]) -> Option<ExistingBlob> {
                (*hash == self.hash).then_some(ExistingBlob {
                    generation_idx: self.blob.generation_idx,
                    section_idx: self.blob.section_idx,
                    section_blob_idx: self.blob.section_blob_idx,
                })
            }
        }
        let lookup = LookupGen0 {
            hash: gen0.blob_hashes[0],
            blob: gen0.blobs[0],
        };

        let dir2 = tempdir().unwrap();
        fs::write(dir2.path().join("a_again.txt"), b"shared content").unwrap();
        let opts = PackOptions {
            generation_idx: 1,
            previous_integrity_hash: summary0.integrity_hash,
            existing_blobs: &lookup,
            ..options()
        };
        let summary1 = pack_directory(&mut buf, dir2.path(), false, opts).unwrap();
        assert_eq!(summary1.new_blob_bytes, 0);

        let archive = read_archive(Cursor::new(buf)).unwrap();
        let blob = &archive.generations[1].blobs[0];
        assert_eq!(blob.generation_idx, 0); // still points back at generation 0
        assert_eq!(archive.read_blob(blob).unwrap(), b"shared content");
    }
}
