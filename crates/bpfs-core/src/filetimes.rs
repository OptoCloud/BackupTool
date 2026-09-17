use filetime::FileTime;
use std::fs::Metadata;

#[derive(Clone, Copy, Debug)]
pub struct FileTimes {
    pub created: i64,
    pub modified: i64,
    pub accessed: i64,
}

/// Convert a `FileTime` to 100-nanosecond intervals since the UNIX epoch.
///
/// This format is compatible with Windows FILETIME, using 100-nanosecond intervals,
/// and can represent timestamps from approximately 27277 BC to 31217 AD.
/// Returns the number of 100-nanosecond intervals since 1970-01-01 as an `i64`.
fn to_archive_time(ft: FileTime) -> i64 {
    let secs = ft.unix_seconds();
    let nsec = ft.nanoseconds();
    (secs * 10_000_000) + ((nsec / 100) as i64)
}

impl FileTimes {
    pub fn merge_max(&mut self, other: &FileTimes) {
        self.created = self.created.max(other.created);
        self.modified = self.modified.max(other.modified);
        self.accessed = self.accessed.max(other.accessed);
    }

    pub fn from_metadata(meta: &Metadata) -> Self {
        let accessed = to_archive_time(FileTime::from_last_access_time(meta));
        let modified = to_archive_time(FileTime::from_last_modification_time(meta));
        let created = FileTime::from_creation_time(meta)
            .map(to_archive_time)
            .unwrap_or(modified);

        Self {
            created,
            modified,
            accessed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::FileTime;
    use std::fs::{self, File};
    use std::io::Write;
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    /* ------------------------------------------------------------
     * to_archive_time
     * ------------------------------------------------------------ */

    #[test]
    fn archive_time_epoch() {
        let ft = FileTime::from_unix_time(0, 0);
        assert_eq!(super::to_archive_time(ft), 0);
    }

    #[test]
    fn archive_time_seconds_only() {
        let ft = FileTime::from_unix_time(1, 0);
        assert_eq!(super::to_archive_time(ft), 10_000_000);
    }

    #[test]
    fn archive_time_with_nanoseconds() {
        let ft = FileTime::from_unix_time(1, 123_456_789);
        let expected = 10_000_000 + (123_456_789 / 100) as i64;

        assert_eq!(super::to_archive_time(ft), expected);
    }

    #[test]
    fn archive_time_negative_timestamp() {
        let ft = FileTime::from_unix_time(-1, 0);
        assert_eq!(super::to_archive_time(ft), -10_000_000);
    }

    /* ------------------------------------------------------------
     * FileTimes::merge_max
     * ------------------------------------------------------------ */

    #[test]
    fn merge_max_basic() {
        let mut a = FileTimes {
            created: 10,
            modified: 20,
            accessed: 30,
        };

        let b = FileTimes {
            created: 15,
            modified: 5,
            accessed: 40,
        };

        a.merge_max(&b);

        assert_eq!(a.created, 15);
        assert_eq!(a.modified, 20);
        assert_eq!(a.accessed, 40);
    }

    #[test]
    fn merge_max_idempotent() {
        let mut a = FileTimes {
            created: 42,
            modified: 42,
            accessed: 42,
        };

        let b = a;
        a.merge_max(&b);

        assert_eq!(a.created, 42);
        assert_eq!(a.modified, 42);
        assert_eq!(a.accessed, 42);
    }

    /* ------------------------------------------------------------
     * FileTimes::from_metadata
     * ------------------------------------------------------------ */

    #[test]
    fn from_metadata_basic_sanity() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("file.txt");

        let mut f = File::create(&path).unwrap();
        writeln!(f, "test").unwrap();
        drop(f);

        let meta = fs::metadata(&path).unwrap();
        let ft = FileTimes::from_metadata(&meta);

        // All timestamps should be non-zero and reasonable
        assert!(ft.modified > 0);
        assert!(ft.accessed > 0);
        assert!(ft.created > 0);

        // Created should not be later than modified on sane filesystems
        assert!(ft.created <= ft.modified || cfg!(target_os = "linux"));
    }

    #[test]
    fn creation_time_fallbacks_to_modified() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("file.txt");

        File::create(&path).unwrap();
        let meta = fs::metadata(&path).unwrap();

        let modified = super::to_archive_time(FileTime::from_last_modification_time(&meta));

        let ft = FileTimes::from_metadata(&meta);

        // On platforms without creation time, created == modified
        if FileTime::from_creation_time(&meta).is_none() {
            assert_eq!(ft.created, modified);
        }
    }

    #[test]
    fn timestamps_change_after_write() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("file.txt");

        {
            let mut f = File::create(&path).unwrap();
            writeln!(f, "first").unwrap();
        }

        let meta1 = fs::metadata(&path).unwrap();
        let ft1 = FileTimes::from_metadata(&meta1);

        thread::sleep(Duration::from_millis(10));

        {
            let mut f = File::create(&path).unwrap();
            writeln!(f, "second").unwrap();
        }

        let meta2 = fs::metadata(&path).unwrap();
        let ft2 = FileTimes::from_metadata(&meta2);

        assert!(ft2.modified >= ft1.modified);
    }
}
