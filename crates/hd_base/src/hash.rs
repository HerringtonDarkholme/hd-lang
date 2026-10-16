//! In-run hash tables with a fixed seed (`docs/compiler/data-structures.md`
//! §3.2). Every crate uses these aliases instead of `std`'s randomly
//! seeded tables, which `clippy.toml` bans outside this module, so a
//! table's order is the same in every run of one build. The seed is
//! 0 unless a determinism test sets another with `set_hash_seed`: a
//! table iterated into output then changes that output, and the test
//! fails.
#![allow(clippy::disallowed_types)]

use std::hash::{BuildHasher, DefaultHasher, Hasher};
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
    type Hasher = DefaultHasher;

    fn build_hasher(&self) -> DefaultHasher {
        let mut h = DefaultHasher::new();
        h.write_u64(self.0);
        h
    }
}

/// `std`'s `HashMap` with a fixed seed.
pub type HashMap<K, V> = std::collections::HashMap<K, V, FixedState>;

/// `std`'s `HashSet` with a fixed seed.
pub type HashSet<T> = std::collections::HashSet<T, FixedState>;

#[cfg(test)]
mod tests {
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
}
