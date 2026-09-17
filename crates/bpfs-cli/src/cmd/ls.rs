use anyhow::{Context, Result};
use std::fs::File;
use std::path::Path;

use bpfs_read::iter::current_files;
use bpfs_read::read_archive;

pub fn run(archive: &str, tree: bool) -> Result<()> {
    let file = File::open(Path::new(archive)).with_context(|| format!("opening {archive}"))?;
    let archive_data = read_archive(file).context("failed to parse/verify archive")?;
    let gen = archive_data.latest();

    let mut resolved = current_files(gen).context("failed to resolve file tree")?;
    resolved.sort_by(|a, b| a.path.cmp(&b.path));

    if tree {
        for r in &resolved {
            let depth = r.path.components().count().saturating_sub(1);
            let name = r
                .path
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default();
            println!("{}{}", "  ".repeat(depth), name);
        }
    } else {
        for r in &resolved {
            println!("{}", r.path.display());
        }
    }

    Ok(())
}
