use anyhow::{Context, Result};
use std::fs::File;
use std::path::Path;

use bpfs_read::read_archive;
use bpfs_read::verify::verify_blob_hashes;

pub fn run(archive: &str, deep: bool) -> Result<()> {
    let file = File::open(Path::new(archive)).with_context(|| format!("opening {archive}"))?;
    // Parsing an archive already verifies every section hash and the
    // generation hash chain; this is where structural corruption surfaces.
    let archive_data = read_archive(file).context("archive/hash-chain verification failed")?;

    if deep {
        verify_blob_hashes(&archive_data).context("deep blob content verification failed")?;
    }

    println!(
        "OK: {} generation(s) verified{}",
        archive_data.generations.len(),
        if deep { " (deep)" } else { "" }
    );
    Ok(())
}
