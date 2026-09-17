use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read},
    path::Path,
};

use crate::analyze::shannon::EntropyStream;
use crate::scan::types::FileEntry;
use bpfs_core::constants::EMPTY_HASH;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

/// Files above this size are read in `LARGE_CHUNK`s instead of `SMALL_CHUNK`s.
const LARGE_FILE: u64 = 16 * 1024 * 1024;
const LARGE_CHUNK: usize = 4 * 1024 * 1024;
const SMALL_CHUNK: usize = 64 * 1024;

/// Called after each chunk of a file is analyzed with
/// `(path, file_bytes_done, file_size_hint, chunk_len)`. Returning an error
/// (e.g. on cancellation) aborts the analysis.
pub type ChunkCallback<'a> = dyn Fn(&Path, u64, u64, u64) -> io::Result<()> + Sync + 'a;

const BUCKET_SIZE: u64 = 64;

/// Perform detailed analysis of a file's contents.
///
/// Computes:
/// - SHA-256 hash of the entire file
/// - Shannon entropy percentage
/// - Sampled header bytes (length may be less than `HEADER_SAMPLE_SIZE` for short files)
/// - MIME-type inference (magic number + extension)
///
/// # Errors
///
/// Returns an `io::Error` if the file cannot be opened or read, or if
/// `on_chunk` returns one.
fn perform_data_analysis(
    file_path: &Path,
    size_hint: u64,
    on_chunk: &ChunkCallback,
) -> io::Result<BlobInfo> {
    let mut file = File::open(file_path)?;
    let mut sha256 = Sha256::new();
    let mut entropy_stream = EntropyStream::new();

    // Large files get a bigger buffer; plain reads (rather than mmap) keep
    // memory use flat regardless of file size.
    let chunk = if size_hint > LARGE_FILE {
        LARGE_CHUNK
    } else {
        SMALL_CHUNK
    };
    let mut buffer = vec![0u8; chunk];
    let mut actual_size = 0usize;
    loop {
        let read_bytes = file.read(&mut buffer)?;
        if read_bytes == 0 {
            break;
        }
        sha256.update(&buffer[..read_bytes]);
        entropy_stream.feed_slice(&buffer[..read_bytes]);
        actual_size += read_bytes;
        on_chunk(file_path, actual_size as u64, size_hint, read_bytes as u64)?;
    }

    Ok(BlobInfo {
        hash: sha256.finalize().into(),
        size: actual_size,
        entropy: entropy_stream.entropy() as f32,
    })
}

/// Analysis result for a file's contents.
#[derive(Clone, Copy, Debug)]
pub struct BlobInfo {
    /// SHA-256 digest of full file.
    pub hash: [u8; 32],
    /// Size of the blob
    pub size: usize,
    /// Shannon entropy percentage of file contents.
    pub entropy: f32,
}

#[derive(Debug)]
pub struct BlobGroup {
    pub info: BlobInfo,
    pub indices: Vec<usize>,
}

/// Deduplicates a list of files into blobs, returning each blob and the indices
/// of files (in `files`) that map to it.
///
/// `on_chunk` is called from several threads as files are analyzed.
pub fn collect_unique_blobs(
    files: &[FileEntry],
    on_chunk: &ChunkCallback,
) -> io::Result<Vec<BlobGroup>> {
    // 1) Split zero-sized from the rest
    let (empties, non_empty): (Vec<_>, Vec<_>) = files
        .iter()
        .enumerate()
        .partition(|(_, f)| f.size_hint == 0);

    // 2) Bucket by approximate sizes
    let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, f) in non_empty {
        buckets
            .entry(f.size_hint / BUCKET_SIZE)
            .or_default()
            .push(i);
    }

    // 3) Analyze each bucket in parallel -> partial hash->BlobGroup maps
    let results = buckets
        .into_par_iter()
        .map(|(_, idxs)| {
            let mut local = HashMap::with_capacity(idxs.len());
            for &i in &idxs {
                let entry = &files[i];
                let info = perform_data_analysis(&entry.fs_path, entry.size_hint, on_chunk)
                    .map_err(|e| {
                        if e.kind() == io::ErrorKind::Interrupted {
                            e
                        } else {
                            io::Error::new(e.kind(), format!("{}: {e}", entry.fs_path.display()))
                        }
                    })?;

                local
                    .entry(info.hash)
                    .or_insert_with(|| BlobGroup {
                        info,
                        indices: Vec::with_capacity(1),
                    })
                    .indices
                    .push(i);
            }
            Ok(local)
        })
        .collect::<Result<Vec<_>, io::Error>>()?;

    let mut collected: Vec<BlobGroup> = Vec::with_capacity(
        (if empties.is_empty() { 0 } else { 1 }) + results.iter().map(HashMap::len).sum::<usize>(),
    );

    // 4) Add an empty blob if needed
    if !empties.is_empty() {
        collected.push(BlobGroup {
            info: BlobInfo {
                hash: EMPTY_HASH,
                size: 0,
                entropy: 0.0,
            },
            indices: empties.into_iter().map(|(i, _)| i).collect(),
        });
    }

    // consume all of results and extend in one go
    collected.extend(results.into_iter().flat_map(HashMap::into_values));

    Ok(collected)
}
