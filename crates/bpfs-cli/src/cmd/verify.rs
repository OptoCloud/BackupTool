use anyhow::{Context, Result};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use bpfs_read::read_archive_with_monitor;
use bpfs_read::verify::{verify_blob_hashes, verify_integrity};

use super::progress::StderrMonitor;

pub fn run(archive: &str, deep: bool) -> Result<()> {
    let monitor = StderrMonitor::new();
    let file = File::open(Path::new(archive)).with_context(|| format!("opening {archive}"))?;
    let archive_data = read_archive_with_monitor(BufReader::new(file), &monitor)
        .context("archive structure is invalid")?;

    verify_integrity(&archive_data, &monitor).context("integrity check failed")?;
    if deep {
        verify_blob_hashes(&archive_data, &monitor).context("content check failed")?;
    }
    drop(monitor);

    println!(
        "OK: {} generation(s) verified{}",
        archive_data.generations.len(),
        if deep { " (deep)" } else { "" }
    );
    Ok(())
}
