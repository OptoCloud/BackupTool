//! Progress reporting, logging and cancellation for long-running operations.

use std::io;
use std::path::Path;

/// A stage of a long-running operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Reading archive metadata.
    Opening,
    /// Walking the source tree. `done` counts entries found; `total` is 0.
    Scanning,
    /// Hashing and analyzing source files. Counted in bytes.
    Hashing,
    /// Reading, compressing and writing new data. Counted in uncompressed bytes.
    Packing,
    /// Re-hashing archive bytes on disk. Counted in bytes.
    Verifying,
    /// Decompressing blobs and checking their content hashes. Counted in bytes.
    VerifyingContent,
    /// Writing restored files. Counted in bytes.
    Extracting,
}

/// Progress through the file currently being processed.
#[derive(Clone, Copy, Debug)]
pub struct Item<'a> {
    pub path: &'a Path,
    pub done: u64,
    pub total: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Progress<'a> {
    pub phase: Phase,
    pub done: u64,
    /// 0 when the total is not known in advance.
    pub total: u64,
    pub item: Option<Item<'a>>,
}

/// Observes a long-running operation. Methods may be called from several
/// threads and very often, so implementations should be cheap or throttle.
pub trait Monitor: Sync {
    fn progress(&self, _progress: Progress<'_>) {}

    /// A human-readable milestone, e.g. "Found 1,204 files".
    fn log(&self, _message: &str) {}

    fn is_cancelled(&self) -> bool {
        false
    }

    /// Returns an `Interrupted` error once cancellation has been requested.
    fn checkpoint(&self) -> io::Result<()> {
        if self.is_cancelled() {
            Err(cancelled())
        } else {
            Ok(())
        }
    }
}

/// Ignores all progress and never cancels.
pub struct NoMonitor;
impl Monitor for NoMonitor {}

/// The error returned by [`Monitor::checkpoint`] after cancellation.
pub fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "operation cancelled")
}
