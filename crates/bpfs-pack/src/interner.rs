use std::collections::HashMap;

#[derive(Default)]
pub struct StringInterner {
    map: HashMap<String, u32>, // single owned copy
    total_bytes: u32,
}

#[allow(dead_code)] // count/bytes_len/get_index kept for external callers; exercised by tests
impl StringInterner {
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.map.get(s) {
            return idx;
        }
        let len = self.map.len();
        assert!(len < u32::MAX as usize, "too many interned strings");
        let idx = len as u32;
        self.total_bytes = self
            .total_bytes
            .checked_add(s.len() as u32)
            .expect("total string bytes overflow");
        self.map.insert(s.to_owned(), idx);
        idx
    }

    #[inline]
    pub fn count(&self) -> u32 {
        self.map.len() as u32
    }

    #[inline]
    pub fn bytes_len(&self) -> u32 {
        self.total_bytes
    }

    /// Returns strings ordered by their assigned index (index == position).
    pub fn as_strs(&self) -> Vec<&str> {
        let mut out = vec![""; self.map.len()];
        for (s, &idx) in &self.map {
            out[idx as usize] = s;
        }
        out
    }

    #[inline]
    pub fn get_index(&self, s: &str) -> Option<u32> {
        self.map.get(s).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interns_dedupe() {
        let mut i = StringInterner::default();
        let a = i.intern("foo");
        let b = i.intern("foo");
        assert_eq!(a, b);
        assert_eq!(i.count(), 1);
    }

    #[test]
    fn indices_are_sequential() {
        let mut i = StringInterner::default();
        assert_eq!(i.intern("a"), 0);
        assert_eq!(i.intern("b"), 1);
        assert_eq!(i.intern("a"), 0);
        assert_eq!(i.count(), 2);
    }

    #[test]
    fn as_strs_matches_indices() {
        let mut i = StringInterner::default();
        let ia = i.intern("alpha");
        let ib = i.intern("beta");
        let strs = i.as_strs();
        assert_eq!(strs[ia as usize], "alpha");
        assert_eq!(strs[ib as usize], "beta");
    }

    #[test]
    fn bytes_len_counts_unique_bytes_only() {
        let mut i = StringInterner::default();
        i.intern("abc");
        i.intern("abc");
        i.intern("de");
        assert_eq!(i.bytes_len(), 5);
    }

    #[test]
    fn get_index_reflects_interned_state() {
        let mut i = StringInterner::default();
        assert_eq!(i.get_index("x"), None);
        let idx = i.intern("x");
        assert_eq!(i.get_index("x"), Some(idx));
    }
}
