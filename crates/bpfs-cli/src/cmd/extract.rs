use anyhow::{Context, Result};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use bpfs_read::iter::{current_files, extract_files};
use bpfs_read::read_archive_with_monitor;

use super::progress::StderrMonitor;

pub fn run(archive: &str, dest: &str, paths: &[String], overwrite: bool) -> Result<()> {
    let monitor = StderrMonitor::new();
    let file = File::open(Path::new(archive)).with_context(|| format!("opening {archive}"))?;
    let archive_data = read_archive_with_monitor(BufReader::new(file), &monitor)
        .context("failed to read archive")?;
    let gen = archive_data.latest();
    let mut resolved = current_files(gen).context("failed to resolve file tree")?;

    if !paths.is_empty() {
        let filter: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        resolved.retain(|r| filter.iter().any(|p| r.path.starts_with(p)));
    }

    let dest = Path::new(dest);
    let summary = extract_files(&archive_data, gen, &resolved, dest, overwrite, &monitor)
        .context("extraction failed (pass --overwrite to replace existing files)")?;
    drop(monitor);

    println!("extracted {} file(s) to {}", summary.files, dest.display());
    Ok(())
}
