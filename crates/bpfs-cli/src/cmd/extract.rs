use anyhow::{Context, Result};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use bpfs_read::iter::{current_files, extract_file_bytes};
use bpfs_read::read_archive;

pub fn run(archive: &str, dest: &str, paths: &[String], overwrite: bool) -> Result<()> {
    let file = File::open(Path::new(archive)).with_context(|| format!("opening {archive}"))?;
    let archive_data = read_archive(file).context("failed to parse/verify archive")?;
    let gen = archive_data.latest();
    let resolved = current_files(gen).context("failed to resolve file tree")?;

    let dest = Path::new(dest);
    fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;

    let filter: Option<Vec<PathBuf>> = if paths.is_empty() {
        None
    } else {
        Some(paths.iter().map(PathBuf::from).collect())
    };

    let mut extracted = 0usize;
    for r in &resolved {
        if let Some(filter) = &filter {
            if !filter.iter().any(|p| r.path.starts_with(p)) {
                continue;
            }
        }

        let out_path = dest.join(&r.path);
        if out_path.exists() && !overwrite {
            anyhow::bail!(
                "{} already exists (pass --overwrite to replace it)",
                out_path.display()
            );
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let bytes = extract_file_bytes(&archive_data, gen, r)
            .with_context(|| format!("extracting {}", r.path.display()))?;
        fs::write(&out_path, bytes).with_context(|| format!("writing {}", out_path.display()))?;
        extracted += 1;
    }

    println!("extracted {extracted} file(s) to {}", dest.display());
    Ok(())
}
