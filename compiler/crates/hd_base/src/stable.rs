//! Stable hashing (data-structures.md §3.2): a 128-bit streaming hasher with
//! a fixed seed. Integers hash little-endian at their declared width; lists
//! and strings hash their length first. No `usize` is hashed, and run IDs
//! have no `StableHash` impl, so they cannot enter a key by accident.

use crate::Hash128;

/// 128-bit streaming hasher. The design names xxh3-128; the workspace has
/// no xxhash dependency yet, so this is a fixed-seed FNV-style mix, the same
/// one `hash128` uses (architecture skeleton, SK-7).
#[derive(Clone, Debug)]
pub struct StableHasher {
    lo: u64,
    hi: u64,
}

impl Default for StableHasher {
    fn default() -> Self {
        Self {
            lo: 0xcbf2_9ce4_8422_2325,
            hi: 0x9e37_79b9_7f4a_7c15,
        }
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
        for &b in bytes {
            self.lo ^= u64::from(b);
            self.lo = self.lo.wrapping_mul(0x0000_0100_0000_01b3);
            self.hi ^= self.lo.rotate_left(17).wrapping_add(u64::from(b));
            self.hi = self.hi.wrapping_mul(0x9ddf_ea08_eb38_2d69);
        }
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
        Hash128(u128::from(self.lo) | (u128::from(self.hi) << 64))
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
}
