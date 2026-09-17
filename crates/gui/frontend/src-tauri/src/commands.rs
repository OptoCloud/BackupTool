use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use serde::Serialize;

use bpfs_pack::pack::{
    pack_directory, ExistingBlob, ExistingBlobLookup, NoExistingBlobs, PackOptions,
};
use bpfs_pack::policy::compression::DefaultCompressionPolicy;
use bpfs_read::iter::{current_files, extract_file_bytes};
use bpfs_read::read_archive;
use bpfs_read::verify::verify_blob_hashes;

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn map_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

struct BlobIndex(HashMap<[u8; 32], ExistingBlob>);
impl ExistingBlobLookup for BlobIndex {
    fn find(&self, hash: &[u8; 32]) -> Option<ExistingBlob> {
        self.0.get(hash).copied()
    }
}

#[derive(Serialize)]
pub struct PackSummaryDto {
    pub generation_idx: u32,
    pub file_count: usize,
    pub dir_count: usize,
    pub blob_count: usize,
    pub new_blob_bytes: u64,
    pub integrity_hash: String,
}

#[tauri::command]
pub fn pack_archive(src: String, out: String, no_compress: bool) -> Result<PackSummaryDto, String> {
    let src_path = Path::new(&src);
    let out_path = Path::new(&out);

    let policy = DefaultCompressionPolicy {
        incompressible_entropy: if no_compress {
            f64::NEG_INFINITY
        } else {
            96.25
        },
    };

    let appending = out_path.exists();

    let (generation_idx, previous_integrity_hash, existing_blobs): (
        u32,
        [u8; 32],
        Box<dyn ExistingBlobLookup>,
    ) = if appending {
        let file = File::open(out_path).map_err(map_err)?;
        let archive = read_archive(file).map_err(map_err)?;

        let mut map = HashMap::new();
        for gen in &archive.generations {
            for (i, b) in gen.blobs.iter().enumerate() {
                map.entry(gen.blob_hashes[i]).or_insert(ExistingBlob {
                    generation_idx: b.generation_idx,
                    section_idx: b.section_idx,
                    section_blob_idx: b.section_blob_idx,
                });
            }
        }

        (
            archive.generations.len() as u32,
            archive.latest().integrity_hash,
            Box::new(BlobIndex(map)),
        )
    } else {
        (0, [0u8; 32], Box::new(NoExistingBlobs))
    };

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(out_path)
        .map_err(map_err)?;

    let summary = pack_directory(
        &mut file,
        src_path,
        !appending,
        PackOptions {
            generation_idx,
            previous_integrity_hash,
            policy: &policy,
            signing_key: None,
            existing_blobs: existing_blobs.as_ref(),
        },
    )
    .map_err(map_err)?;

    Ok(PackSummaryDto {
        generation_idx,
        file_count: summary.file_count,
        dir_count: summary.dir_count,
        blob_count: summary.blob_count,
        new_blob_bytes: summary.new_blob_bytes,
        integrity_hash: to_hex(&summary.integrity_hash),
    })
}

#[derive(Serialize)]
pub struct FileEntryDto {
    pub path: String,
    pub size: u64,
}

#[tauri::command]
pub fn list_archive(path: String) -> Result<Vec<FileEntryDto>, String> {
    let file = File::open(Path::new(&path)).map_err(map_err)?;
    let archive = read_archive(file).map_err(map_err)?;
    let gen = archive.latest();
    let resolved = current_files(gen).map_err(map_err)?;

    let mut out = Vec::with_capacity(resolved.len());
    for r in &resolved {
        let file_entry = &gen.files[r.file_idx];
        let blob = &gen.blobs[file_entry.blob_idx as usize];
        out.push(FileEntryDto {
            path: r.path.to_string_lossy().into_owned(),
            size: blob.raw_size,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

#[tauri::command]
pub fn extract_archive(path: String, dest: String, overwrite: bool) -> Result<usize, String> {
    let file = File::open(Path::new(&path)).map_err(map_err)?;
    let archive = read_archive(file).map_err(map_err)?;
    let gen = archive.latest();
    let resolved = current_files(gen).map_err(map_err)?;

    let dest = Path::new(&dest);
    fs::create_dir_all(dest).map_err(map_err)?;

    let mut extracted = 0usize;
    for r in &resolved {
        let out_path: PathBuf = dest.join(&r.path);
        if out_path.exists() && !overwrite {
            return Err(format!("{} already exists", out_path.display()));
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(map_err)?;
        }
        let bytes = extract_file_bytes(&archive, gen, r).map_err(map_err)?;
        fs::write(&out_path, bytes).map_err(map_err)?;
        extracted += 1;
    }
    Ok(extracted)
}

#[derive(Serialize)]
pub struct VerifyResultDto {
    pub generations: usize,
    pub ok: bool,
    pub message: String,
}

#[tauri::command]
pub fn verify_archive(path: String, deep: bool) -> Result<VerifyResultDto, String> {
    let file = File::open(Path::new(&path)).map_err(map_err)?;
    let archive = match read_archive(file) {
        Ok(a) => a,
        Err(e) => {
            return Ok(VerifyResultDto {
                generations: 0,
                ok: false,
                message: e.to_string(),
            });
        }
    };

    if deep {
        if let Err(e) = verify_blob_hashes(&archive) {
            return Ok(VerifyResultDto {
                generations: archive.generations.len(),
                ok: false,
                message: e.to_string(),
            });
        }
    }

    Ok(VerifyResultDto {
        generations: archive.generations.len(),
        ok: true,
        message: "OK".to_string(),
    })
}
