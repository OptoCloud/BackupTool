//! Shared helpers for a generation's decoded string table. Strings are
//! generation-local: a `name_stridx` on a `DirectoryEntry`/`FileEntry` only
//! makes sense relative to the string table of the generation it was
//! declared in.

use crate::errors::{ArchiveError, Result};

#[derive(Clone, Debug, Default)]
pub struct StringTable {
    strings: Vec<String>,
}

impl StringTable {
    pub fn new(strings: Vec<String>) -> Self {
        Self { strings }
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    pub fn get(&self, idx: u32) -> Result<&str> {
        self.strings
            .get(idx as usize)
            .map(String::as_str)
            .ok_or(ArchiveError::IndexOutOfBounds("StringTable index"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_in_bounds() {
        let t = StringTable::new(vec!["a".into(), "b".into()]);
        assert_eq!(t.get(0).unwrap(), "a");
        assert_eq!(t.get(1).unwrap(), "b");
    }

    #[test]
    fn get_out_of_bounds_errors() {
        let t = StringTable::new(vec!["a".into()]);
        assert!(t.get(1).is_err());
    }

    #[test]
    fn empty_table() {
        let t = StringTable::default();
        assert!(t.is_empty());
        assert!(t.get(0).is_err());
    }
}
