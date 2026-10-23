//! In-run hash tables with a fixed seed (`docs/compiler/data-structures.md`
//! §3.2). Every crate uses these aliases instead of `std`'s randomly
//! seeded tables, which `clippy.toml` bans outside this module, so a
//! table's order is the same in every run of one build. The seed is
//! 0 unless a determinism test sets another with `set_hash_seed`: a
//! table iterated into output then changes that output, and the test
//! fails.
#![allow(clippy::disallowed_types)]

use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

static SEED: AtomicU64 = AtomicU64::new(0);

/// Sets the seed of every table built after this call; tables built
/// before keep theirs. Only tests call it.
pub fn set_hash_seed(seed: u64) {
    SEED.store(seed, Ordering::Relaxed);
}

/// A table's hasher state: the seed at construction, which every hash
/// of the table starts from.
#[derive(Clone, Copy, Debug)]
pub struct FixedState(u64);

impl Default for FixedState {
    fn default() -> Self {
        FixedState(SEED.load(Ordering::Relaxed))
    }
}

impl BuildHasher for FixedState {
    type Hasher = FixedHasher;

    #[inline]
    fn build_hasher(&self) -> FixedHasher {
        FixedHasher(self.0)
    }
}

/// A small seeded word mixer for in-run tables. Persistent cache keys
/// use `StableHasher`, independently of this table order.
#[derive(Clone, Debug)]
pub struct FixedHasher(u64);

impl FixedHasher {
    #[inline]
    fn word(&mut self, word: u64) {
        self.0 = self
            .0
            .wrapping_add(word)
            .wrapping_mul(0xf135_7aea_2e62_a9c5);
    }
}

impl Hasher for FixedHasher {
    #[inline]
    fn finish(&self) -> u64 {
        // Move the high-entropy product bits into the table's bucket bits.
        self.0.rotate_left(26)
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        // Include the length so zero-padded tails remain distinct.
        self.word(bytes.len() as u64);
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.word(u64::from_le_bytes(chunk.try_into().expect("eight bytes")));
        }
        let tail = chunks.remainder();
        if !tail.is_empty() {
            let mut last = [0; 8];
            last[..tail.len()].copy_from_slice(tail);
            self.word(u64::from_le_bytes(last));
        }
    }

    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.word(u64::from(value));
    }

    #[inline]
    fn write_u16(&mut self, value: u16) {
        self.word(u64::from(value));
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.word(u64::from(value));
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.word(value);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.word(value as u64);
    }
}

/// `std`'s `HashMap` with a fixed seed.
pub type HashMap<K, V> = std::collections::HashMap<K, V, FixedState>;

/// `std`'s `HashSet` with a fixed seed.
pub type HashSet<T> = std::collections::HashSet<T, FixedState>;

#[cfg(test)]
mod tests {
    use std::hash::{BuildHasher, Hasher};

    use super::{FixedState, HashSet};

    #[test]
    fn one_seed_gives_one_order() {
        let a: HashSet<u32> = (0..64).collect();
        let b: HashSet<u32> = (0..64).collect();
        assert_eq!(
            a.iter().copied().collect::<Vec<_>>(),
            b.iter().copied().collect::<Vec<_>>()
        );
    }

    #[test]
    fn two_seeds_give_two_orders() {
        let order = |seed| {
            let mut s = HashSet::with_hasher(FixedState(seed));
            s.extend(0..64u32);
            s.iter().copied().collect::<Vec<_>>()
        };
        assert_ne!(order(1), order(2));
    }

    #[test]
    fn a_factory_and_its_clone_keep_the_same_seed() {
        for seed in [0, 1, 0x5eed, u64::MAX] {
            let state = FixedState(seed);
            let same = state;
            assert_eq!(
                state.hash_one((7u32, "path/to/item")),
                same.hash_one((7u32, "path/to/item"))
            );
            assert_eq!(state.hash_one(31u64), state.hash_one(31u64));
            assert_ne!(state.hash_one(31u64), FixedState(seed ^ 1).hash_one(31u64));
        }
    }

    #[test]
    fn every_byte_and_tail_length_enters_the_hash() {
        let hash = |bytes: &[u8]| {
            let mut h = FixedState(0).build_hasher();
            h.write(bytes);
            h.finish()
        };
        for len in 0..=40 {
            let bytes = vec![0; len];
            let original = hash(&bytes);
            assert_ne!(original, hash(&vec![0; len + 1]), "length {len}");
            for at in 0..len {
                let mut changed = bytes.clone();
                changed[at] = 1;
                assert_ne!(original, hash(&changed), "length {len}, byte {at}");
            }
        }
    }
}
