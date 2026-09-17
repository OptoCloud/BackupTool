use crate::scan::types::{DirectoryEntry, FileEntry, SymlinkEntry};
use bpfs_core::filetimes::FileTimes;
use std::{collections::HashSet, fs::Metadata, io, path::PathBuf};

#[derive(Debug, Clone)]
pub struct FileCollector {
    pub directories: Vec<DirectoryEntry>,
    pub files: Vec<FileEntry>,
    pub symlinks: Vec<SymlinkEntry>,
    entries: HashSet<PathBuf>,
}

impl Default for FileCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl FileCollector {
    pub fn new() -> Self {
        Self {
            directories: Vec::new(),
            files: Vec::new(),
            symlinks: Vec::new(),
            entries: HashSet::new(),
        }
    }

    /// Returns an iterator which, *each time it finishes
    /// crawling a directory*, yields `Ok(total_files_so_far)`,
    /// or `Err(e)` if a filesystem error occurred.
    pub fn add_path(
        &mut self,
        fs_path: PathBuf,
        archive_path: PathBuf,
        dereference_symlinks: bool,
    ) -> FileCollectorIter<'_> {
        // initial task is “visit this src→dst at depth 0”
        let tasks = vec![Task::Entry {
            fs_path,
            archive_path,
            symlink_depth: 0,
            dereference_symlinks,
        }];
        FileCollectorIter {
            collector: self,
            tasks,
        }
    }

    // unchanged synchronous helpers, now used by the iterator:
    fn add_file_entry(
        &mut self,
        fs_path: PathBuf,
        archive_path: PathBuf,
        meta: &Metadata,
    ) -> io::Result<usize> {
        if self.entries.contains(&archive_path) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("Entry already added: {:?}", archive_path),
            ));
        }
        let index = self.files.len();
        self.files.push(FileEntry {
            fs_path,
            archive_path: archive_path.clone(),
            size_hint: meta.len(),
            filetimes: FileTimes::from_metadata(meta),
        });
        self.entries.insert(archive_path);
        Ok(index)
    }

    fn add_symlink_entry(
        &mut self,
        fs_path: PathBuf,
        archive_path: PathBuf,
        fs_target_path: PathBuf,
        meta: &Metadata,
    ) -> io::Result<usize> {
        if self.entries.contains(&archive_path) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("Entry already added: {:?}", archive_path),
            ));
        }
        let index = self.symlinks.len();
        self.symlinks.push(SymlinkEntry {
            fs_path,
            archive_path: archive_path.clone(),
            fs_target_path,
            filetimes: FileTimes::from_metadata(meta),
        });
        self.entries.insert(archive_path);
        Ok(index)
    }

    fn add_directory_entry(
        &mut self,
        fs_path: PathBuf,
        archive_path: PathBuf,
        meta: &Metadata,
    ) -> io::Result<usize> {
        if self.entries.contains(&archive_path) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("Entry already added: {:?}", archive_path),
            ));
        }

        let index = self.directories.len();
        self.directories.push(DirectoryEntry {
            fs_path,
            archive_path: archive_path.clone(),
            filetimes: FileTimes::from_metadata(meta),
        });

        // `false` here just means "not a symlink" (same as files). If you want to
        // distinguish dirs too, replace the bool with an enum.
        self.entries.insert(archive_path);
        Ok(index)
    }
}

/// Internal “work item” enum
enum Task {
    Entry {
        fs_path: PathBuf,
        archive_path: PathBuf,
        symlink_depth: u32,
        dereference_symlinks: bool,
    },
    /// Marker that a directory’s children have all been enqueued
    DirDone,
}

/// The iterator returned by `FileCollector::add_path`
pub struct FileCollectorIter<'a> {
    collector: &'a mut FileCollector,
    tasks: Vec<Task>,
}

impl<'a> Iterator for FileCollectorIter<'a> {
    type Item = io::Result<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(task) = self.tasks.pop() {
            match task {
                Task::Entry {
                    fs_path,
                    archive_path,
                    symlink_depth,
                    dereference_symlinks,
                } => {
                    // Try to read metadata
                    let meta = match fs_path.symlink_metadata() {
                        Err(e) => return Some(Err(e)),
                        Ok(m) => m,
                    };

                    if meta.is_file() {
                        // regular file → record it
                        if let Err(e) = self.collector.add_file_entry(fs_path, archive_path, &meta)
                        {
                            return Some(Err(e));
                        }
                    } else if meta.file_type().is_symlink() {
                        if symlink_depth >= 10 {
                            return Some(Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("Symlink depth exceeded for {:?}", fs_path),
                            )));
                        }

                        // resolve symlink
                        let fs_target_path = match fs_path.read_link() {
                            Err(e) => return Some(Err(e)),
                            Ok(path) => path,
                        };

                        if dereference_symlinks {
                            self.tasks.push(Task::Entry {
                                fs_path: fs_target_path,
                                archive_path,
                                symlink_depth: symlink_depth + 1,
                                dereference_symlinks,
                            });
                        } else {
                            // record symlink itself
                            if let Err(e) = self.collector.add_symlink_entry(
                                fs_path,
                                archive_path,
                                fs_target_path,
                                &meta,
                            ) {
                                return Some(Err(e));
                            }
                        }
                    } else if meta.is_dir() {
                        // record the directory itself
                        if let Err(e) = self.collector.add_directory_entry(
                            fs_path.clone(),
                            archive_path.clone(),
                            &meta,
                        ) {
                            return Some(Err(e));
                        }

                        let read = match fs_path.read_dir() {
                            Err(e) => return Some(Err(e)),
                            Ok(rd) => rd,
                        };

                        for entry in read {
                            match entry {
                                Err(e) => return Some(Err(e)),
                                Ok(de) => {
                                    self.tasks.push(Task::Entry {
                                        fs_path: de.path(),
                                        archive_path: archive_path.join(de.file_name()),
                                        symlink_depth,
                                        dereference_symlinks,
                                    });
                                }
                            }
                        }

                        self.tasks.push(Task::DirDone);
                    } else {
                        return Some(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("Unsupported file type: {:?}", fs_path),
                        )));
                    }
                }
                // A directory was just fully enqueued ⇒ yield progress
                Task::DirDone => {
                    return Some(Ok(self.collector.entries.len()));
                }
            }
        }
        // No more work
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn single_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"hello").unwrap();

        let mut collector = FileCollector::new();
        let results: Vec<_> = collector
            .add_path(dir.path().to_path_buf(), PathBuf::new(), false)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(!results.is_empty());

        assert_eq!(collector.directories.len(), 1); // the root itself
        assert_eq!(collector.files.len(), 1);
        assert_eq!(collector.files[0].archive_path, PathBuf::from("a.txt"));
        assert_eq!(collector.files[0].size_hint, 5);
    }

    #[test]
    fn nested_directories() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub/inner")).unwrap();
        fs::write(dir.path().join("sub/inner/f.bin"), b"1234").unwrap();

        let mut collector = FileCollector::new();
        for r in collector.add_path(dir.path().to_path_buf(), PathBuf::new(), false) {
            r.unwrap();
        }

        // root, sub, sub/inner
        assert_eq!(collector.directories.len(), 3);
        assert_eq!(collector.files.len(), 1);
        assert_eq!(
            collector.files[0].archive_path,
            PathBuf::from("sub/inner/f.bin")
        );

        // parent must always be recorded before child in `directories`
        let idx_of = |p: &str| {
            collector
                .directories
                .iter()
                .position(|d| d.archive_path.as_path() == std::path::Path::new(p))
                .unwrap()
        };
        assert!(idx_of("") < idx_of("sub"));
        assert!(idx_of("sub") < idx_of("sub/inner"));
    }

    #[test]
    fn empty_directory_is_recorded_with_no_files() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("empty")).unwrap();

        let mut collector = FileCollector::new();
        for r in collector.add_path(dir.path().to_path_buf(), PathBuf::new(), false) {
            r.unwrap();
        }

        assert_eq!(collector.directories.len(), 2); // root + empty
        assert!(collector.files.is_empty());
    }

    #[test]
    fn duplicate_archive_path_is_rejected() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"x").unwrap();

        let mut collector = FileCollector::new();
        for r in collector.add_path(dir.path().to_path_buf(), PathBuf::new(), false) {
            r.unwrap();
        }
        // Adding the same fs tree at the same archive path again must fail.
        let err = collector
            .add_path(dir.path().to_path_buf(), PathBuf::new(), false)
            .collect::<Result<Vec<_>, _>>();
        assert!(err.is_err());
    }

    #[test]
    fn multiple_files_all_collected() {
        let dir = tempdir().unwrap();
        for i in 0..10 {
            fs::write(dir.path().join(format!("f{i}.txt")), b"data").unwrap();
        }

        let mut collector = FileCollector::new();
        for r in collector.add_path(dir.path().to_path_buf(), PathBuf::new(), false) {
            r.unwrap();
        }
        assert_eq!(collector.files.len(), 10);
    }
}
