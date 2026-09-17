use std::str;
use std::sync::{Arc, RwLock};

/* ============================================================
 * Public handle
 * ============================================================ */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StrRef {
    pub offset: u32,
    pub len: u32,
}

/* ============================================================
 * Constants
 * ============================================================ */

const CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4 MiB

/* ============================================================
 * Hashing (FNV-1a, 32-bit)
 * ============================================================ */

#[inline]
fn hash_bytes(bytes: &[u8]) -> u32 {
    let mut h = 0x811C9DC5u32;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

#[inline]
fn store_hash(h: u32) -> u32 {
    h | 1 // sentinel: never zero
}

/* ============================================================
 * SIMD byte comparison
 * ============================================================ */

#[inline]
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::*;
        let mut i = 0;
        unsafe {
            while i + 32 <= a.len() {
                let va = _mm256_loadu_si256(a.as_ptr().add(i) as *const __m256i);
                let vb = _mm256_loadu_si256(b.as_ptr().add(i) as *const __m256i);
                if _mm256_movemask_epi8(_mm256_cmpeq_epi8(va, vb)) != -1 {
                    return false;
                }
                i += 32;
            }
        }
        a[i..] == b[i..]
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        a == b
    }
}

/* ============================================================
 * Chunked arena (NO string crosses chunks)
 * ============================================================ */

struct Chunk {
    data: Vec<u8>,
}

struct ChunkedArena {
    chunks: Vec<Chunk>,
    len: u32,
}

impl ChunkedArena {
    fn new() -> Self {
        // Dummy byte so offset 0 is always valid (empty string)
        Self {
            chunks: vec![Chunk { data: vec![0] }],
            len: 1,
        }
    }

    fn push(&mut self, bytes: &[u8]) -> StrRef {
        if bytes.is_empty() {
            return StrRef { offset: 0, len: 0 };
        }

        let len = bytes.len() as u32;
        let new_len = self.len.checked_add(len).expect("u32 overflow");

        // IMPORTANT: never split a string across chunks
        let needs_new_chunk = self
            .chunks
            .last()
            .is_none_or(|c| c.data.len() + bytes.len() > CHUNK_SIZE);

        if needs_new_chunk {
            self.chunks.push(Chunk {
                data: Vec::with_capacity(CHUNK_SIZE.max(bytes.len())),
            });
        }

        let offset = self.len;
        self.chunks
            .last_mut()
            .unwrap()
            .data
            .extend_from_slice(bytes);
        self.len = new_len;

        StrRef { offset, len }
    }

    fn get_bytes(&self, r: StrRef) -> &[u8] {
        let mut off = r.offset as usize;
        let len = r.len as usize;

        for c in &self.chunks {
            if off < c.data.len() {
                return &c.data[off..off + len];
            }
            off -= c.data.len();
        }

        unreachable!("invalid StrRef")
    }

    fn get_str(&self, r: StrRef) -> &str {
        unsafe { str::from_utf8_unchecked(self.get_bytes(r)) }
    }
}

/* ============================================================
 * Sentinel hash table
 * ============================================================ */

#[derive(Clone, Copy)]
struct Entry {
    hash: u32, // 0 = empty
    offset: u32,
    len: u32,
}

struct HashTable {
    entries: Vec<Entry>,
    used: usize,
}

impl HashTable {
    fn new(cap: usize) -> Self {
        let size = cap.next_power_of_two().max(16);
        Self {
            entries: vec![
                Entry {
                    hash: 0,
                    offset: 0,
                    len: 0
                };
                size
            ],
            used: 0,
        }
    }

    fn find<F>(&self, hash: u32, mut matches: F) -> Option<StrRef>
    where
        F: FnMut(Entry) -> bool,
    {
        let stored = store_hash(hash);
        let mask = self.entries.len() - 1;
        let mut idx = stored as usize & mask;

        loop {
            let e = self.entries[idx];
            if e.hash == 0 {
                return None;
            }
            if e.hash == stored && matches(e) {
                return Some(StrRef {
                    offset: e.offset,
                    len: e.len,
                });
            }
            idx = (idx + 1) & mask;
        }
    }

    fn insert(&mut self, hash: u32, offset: u32, len: u32) {
        if self.used * 4 >= self.entries.len() * 3 {
            self.rehash();
        }

        let stored = store_hash(hash);
        let mask = self.entries.len() - 1;
        let mut idx = stored as usize & mask;

        loop {
            if self.entries[idx].hash == 0 {
                self.entries[idx] = Entry {
                    hash: stored,
                    offset,
                    len,
                };
                self.used += 1;
                return;
            }
            idx = (idx + 1) & mask;
        }
    }

    fn rehash(&mut self) {
        let new_size = self.entries.len() * 2;
        let mut new_entries = vec![
            Entry {
                hash: 0,
                offset: 0,
                len: 0
            };
            new_size
        ];
        let mask = new_size - 1;

        for e in self.entries.iter().copied() {
            if e.hash == 0 {
                continue;
            }
            let mut idx = e.hash as usize & mask;
            while new_entries[idx].hash != 0 {
                idx = (idx + 1) & mask;
            }
            new_entries[idx] = e;
        }

        self.entries = new_entries;
    }
}

/* ============================================================
 * Arena + interner
 * ============================================================ */

pub struct StringArena {
    arena: ChunkedArena,
    table: HashTable,
}

impl Default for StringArena {
    fn default() -> Self {
        Self::new()
    }
}

impl StringArena {
    pub fn new() -> Self {
        Self {
            arena: ChunkedArena::new(),
            table: HashTable::new(1024),
        }
    }

    pub fn intern(&mut self, s: &str) -> StrRef {
        self.intern_bytes(s.as_bytes())
    }

    pub fn intern_bytes(&mut self, bytes: &[u8]) -> StrRef {
        let _ = str::from_utf8(bytes).expect("invalid UTF-8");
        let hash = hash_bytes(bytes);

        if let Some(r) = self.table.find(hash, |e| {
            bytes_eq(
                self.arena.get_bytes(StrRef {
                    offset: e.offset,
                    len: e.len,
                }),
                bytes,
            )
        }) {
            return r;
        }

        let r = self.arena.push(bytes);
        self.table.insert(hash, r.offset, r.len);
        r
    }

    pub fn get(&self, r: StrRef) -> &str {
        self.arena.get_str(r)
    }
}

/* ============================================================
 * Thread-safe wrapper
 * ============================================================ */

#[derive(Clone)]
pub struct SyncStringArena {
    inner: Arc<RwLock<StringArena>>,
}

impl Default for SyncStringArena {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncStringArena {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(StringArena::new())),
        }
    }

    pub fn intern(&self, s: &str) -> StrRef {
        self.inner.write().unwrap().intern(s)
    }

    pub fn get(&self, r: StrRef) -> String {
        self.inner.read().unwrap().get(r).to_owned()
    }
}

/* ============================================================
 * Tests
 * ============================================================ */

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn empty_string_is_stable() {
        let mut arena = StringArena::new();
        let r1 = arena.intern("");
        let r2 = arena.intern("");
        assert_eq!(r1, r2);
        assert_eq!(arena.get(r1), "");
        assert_eq!(r1.len, 0);
    }

    #[test]
    fn basic_roundtrip() {
        let mut arena = StringArena::new();
        let r = arena.intern("hello");
        assert_eq!(arena.get(r), "hello");
    }

    #[test]
    fn utf8_multibyte() {
        let mut arena = StringArena::new();
        let s = "🌍🚀✨";
        let r = arena.intern(s);
        assert_eq!(arena.get(r), s);
    }

    #[test]
    fn deduplication() {
        let mut arena = StringArena::new();
        let a = arena.intern("dup");
        let b = arena.intern("dup");
        assert_eq!(a, b);
    }

    #[test]
    fn prefix_not_equal() {
        let mut arena = StringArena::new();
        let a = arena.intern("abc");
        let b = arena.intern("abcd");
        assert_ne!(a, b);
    }

    #[test]
    fn chunk_boundary() {
        let mut arena = StringArena::new();
        let big = "x".repeat(CHUNK_SIZE + 100);
        let r = arena.intern(&big);
        assert_eq!(arena.get(r), big);
    }

    #[test]
    fn rehash_stress() {
        let mut arena = StringArena::new();
        let mut refs = Vec::new();

        for i in 0..50_000 {
            refs.push(arena.intern(&format!("key_{i}")));
        }

        for (i, r) in refs.iter().enumerate() {
            assert_eq!(arena.get(*r), format!("key_{i}"));
        }
    }

    #[test]
    fn thread_safe_usage() {
        let arena = SyncStringArena::new();

        let handles: Vec<_> = (0..16)
            .map(|i| {
                let a = arena.clone();
                thread::spawn(move || a.intern(&format!("thread_{i}")))
            })
            .collect();

        for (i, h) in handles.into_iter().enumerate() {
            let r = h.join().unwrap();
            assert_eq!(arena.get(r), format!("thread_{i}"));
        }
    }

    #[test]
    fn many_empty_and_nonempty() {
        let mut arena = StringArena::new();

        for _ in 0..1000 {
            let e = arena.intern("");
            let n = arena.intern("x");
            assert_eq!(arena.get(e), "");
            assert_eq!(arena.get(n), "x");
        }
    }
}
