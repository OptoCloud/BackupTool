use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader, Read},
    path::Path,
};

use crate::analyze::shannon::EntropyStream;
use crate::scan::types::FileEntry;
use bpfs_core::constants::EMPTY_HASH;
use memmap2::MmapOptions;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

/// Minimum size for using memory-mapped I/O (in bytes).
const MMAP_THRESHOLD: u64 = 100 * 1024 * 1024; // 100 MiB

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
/// Returns an `io::Error` if the file cannot be opened or read.
fn perform_data_analysis(file_path: &Path, size_hint: u64) -> io::Result<BlobInfo> {
    let file = File::open(file_path)?;
    let mut sha256 = Sha256::new();
    let mut entropy_stream = EntropyStream::new();

    let mut actual_size;

    if size_hint > MMAP_THRESHOLD {
        // Memory-map large files
        let mmap = unsafe { MmapOptions::new().map(&file)? };
        let full = &mmap[..];

        actual_size = full.len();

        // Process entire file
        sha256.update(full);
        entropy_stream.feed_slice(full);
    } else {
        // Buffered read for smaller files
        let mut reader = BufReader::new(&file);
        let mut buffer = [0u8; 32 * 1024];

        actual_size = 0;

        loop {
            let read_bytes = reader.read(&mut buffer)?;
            if read_bytes == 0 {
                break;
            }

            sha256.update(&buffer[..read_bytes]);
            entropy_stream.feed_slice(&buffer[..read_bytes]);

            actual_size += read_bytes;
        }
    }

    // Finalize hash and entropy
    let hash_result = sha256.finalize();
    let entropy = entropy_stream.entropy() as f32;

    Ok(BlobInfo {
        hash: hash_result.into(),
        size: actual_size,
        entropy,
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
pub fn collect_unique_blobs(files: &[FileEntry]) -> anyhow::Result<Vec<BlobGroup>> {
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
                let info = perform_data_analysis(&entry.fs_path, entry.size_hint)?;

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
