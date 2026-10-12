//! Stable hashing (data-structures.md §3.2): xxh3-128, streaming, default
//! seed. Integers hash little-endian at their declared width; lists and
//! strings hash their length first. No `usize` is hashed, and run IDs
//! have no `StableHash` impl, so they cannot enter a key by accident.

use crate::Hash128;

/// 128-bit streaming hasher: xxh3-128.
#[derive(Clone)]
pub struct StableHasher(xxhash_rust::xxh3::Xxh3);

impl core::fmt::Debug for StableHasher {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("StableHasher")
    }
}

impl Default for StableHasher {
    fn default() -> Self {
        Self(xxhash_rust::xxh3::Xxh3::new())
    }
}

impl StableHasher {
    #[must_use]
    pub fn new(tag: &str) -> Self {
        let mut h = Self::default();
        h.str(tag);
        h
    }
    pub fn bytes(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    pub fn u8(&mut self, v: u8) {
        self.bytes(&[v]);
    }
    pub fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn hash(&mut self, v: Hash128) {
        self.bytes(&v.0.to_le_bytes());
    }
    /// Length (u32) then bytes.
    pub fn str(&mut self, s: &str) {
        self.u32(u32::try_from(s.len()).expect("string over 4 GiB"));
        self.bytes(s.as_bytes());
    }
    #[must_use]
    pub fn finish(&self) -> Hash128 {
        Hash128(self.0.digest128())
    }
}

/// A value with a stable, content-only hash.
pub trait StableHash {
    fn stable_hash(&self, h: &mut StableHasher);
}

impl StableHash for str {
    fn stable_hash(&self, h: &mut StableHasher) {
        h.str(self);
    }
}

impl StableHash for &str {
    fn stable_hash(&self, h: &mut StableHasher) {
        h.str(self);
    }
}

impl StableHash for Hash128 {
    fn stable_hash(&self, h: &mut StableHasher) {
        h.hash(*self);
    }
}

impl<T: StableHash> StableHash for [T] {
    fn stable_hash(&self, h: &mut StableHasher) {
        h.u32(u32::try_from(self.len()).expect("list over u32"));
        for x in self {
            x.stable_hash(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{StableHash, StableHasher};

    #[test]
    fn length_prefix_separates_fields() {
        let mut a = StableHasher::new("t");
        a.str("ab");
        a.str("c");
        let mut b = StableHasher::new("t");
        b.str("a");
        b.str("bc");
        assert_ne!(a.finish(), b.finish());
        let mut c = StableHasher::new("t");
        ["x", "y"][..].stable_hash(&mut c);
        assert_ne!(c.finish(), a.finish());
    }

    #[test]
    fn streaming_equals_one_shot_xxh3() {
        let mut h = StableHasher::default();
        h.bytes(b"hello ");
        h.bytes(b"world");
        assert_eq!(h.finish(), crate::hash128(b"hello world"));
    }
}
