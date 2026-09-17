use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::Path;

use anyhow::{Context, Result};

use bpfs_pack::pack::{
    pack_directory, ExistingBlob, ExistingBlobLookup, NoExistingBlobs, PackOptions,
};
use bpfs_pack::policy::compression::DefaultCompressionPolicy;
use bpfs_read::read_archive;

struct BlobIndex(HashMap<[u8; 32], ExistingBlob>);

impl ExistingBlobLookup for BlobIndex {
    fn find(&self, hash: &[u8; 32]) -> Option<ExistingBlob> {
        self.0.get(hash).copied()
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn run(src: &str, out: &str, final_hash: bool, no_compress: bool) -> Result<()> {
    let src_path = Path::new(src);
    let out_path = Path::new(out);

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
        let file = File::open(out_path).with_context(|| format!("opening {out}"))?;
        let archive = read_archive(file).context("reading existing archive")?;

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
        .with_context(|| format!("opening {out} for writing"))?;

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
    .context("pack failed")?;

    println!(
        "wrote generation {generation_idx}: {} files, {} dirs, {} blobs ({} new bytes)",
        summary.file_count, summary.dir_count, summary.blob_count, summary.new_blob_bytes
    );
    if final_hash {
        println!("integrity_hash: {}", to_hex(&summary.integrity_hash));
    }
    Ok(())
}
