//! `AppendVec` and frozen columns (data-structures.md §3.9.4).
//!
//! The design's `AppendVec` is a chunked column whose items never move:
//! chunk `k` holds `2^(k+10)` items, readers index without a lock and only
//! the owner appends. Without `unsafe` the chunk slots are `OnceLock`s:
//! an item is published once and then read lock-free.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

const CHUNKS: usize = 22;
const BASE_BITS: u32 = 10;

/// Exact-size frozen column: the result of a finished task.
pub type Col<T> = Box<[T]>;

/// A range of `extra` words or of another column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Range32 {
    pub start: u32,
    pub len: u32,
}

impl Range32 {
    #[must_use]
    pub const fn new(start: u32, len: u32) -> Self {
        Self { start, len }
    }
    #[must_use]
    pub fn range(self) -> core::ops::Range<usize> {
        self.start as usize..(self.start + self.len) as usize
    }
}

/// Growable column whose items never move (data-structures.md §3.9.4).
pub struct AppendVec<T> {
    chunks: [OnceLock<Box<[OnceLock<T>]>>; CHUNKS],
    len: AtomicU32,
}

impl<T> Default for AppendVec<T> {
    fn default() -> Self {
        Self { chunks: std::array::from_fn(|_| OnceLock::new()), len: AtomicU32::new(0) }
    }
}

/// Chunk and offset of item `i`: chunk k covers `[2^(k+10) - 2^10, 2^(k+11) - 2^10)`.
fn locate(i: u32) -> (usize, usize) {
    let biased = u64::from(i) + (1 << BASE_BITS);
    let k = biased.ilog2() - BASE_BITS;
    let start = (1u64 << (k + BASE_BITS)) - (1 << BASE_BITS);
    (k as usize, usize::try_from(u64::from(i) - start).expect("offset"))
}

impl<T> AppendVec<T> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one item (owner only) and returns its index.
    pub fn push(&self, value: T) -> u32 {
        let i = self.len.load(Ordering::Relaxed);
        let (k, off) = locate(i);
        assert!(k < CHUNKS, "AppendVec over capacity");
        let chunk = self.chunks[k]
            .get_or_init(|| (0..(1usize << (k + BASE_BITS as usize))).map(|_| OnceLock::new()).collect());
        assert!(chunk[off].set(value).is_ok(), "AppendVec slot written twice");
        self.len.store(i + 1, Ordering::Release);
        i
    }

    #[must_use]
    pub fn get(&self, i: u32) -> Option<&T> {
        if i >= self.len.load(Ordering::Acquire) {
            return None;
        }
        let (k, off) = locate(i);
        self.chunks[k].get().and_then(|c| c[off].get())
    }

    #[must_use]
    pub fn len(&self) -> u32 {
        self.len.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        (0..self.len()).filter_map(|i| self.get(i))
    }
}

impl<T> core::ops::Index<u32> for AppendVec<T> {
    type Output = T;
    fn index(&self, i: u32) -> &T {
        self.get(i).expect("AppendVec index out of range")
    }
}

#[cfg(test)]
mod tests {
    use super::{AppendVec, locate};

    #[test]
    fn chunks_double() {
        assert_eq!(locate(0), (0, 0));
        assert_eq!(locate(1023), (0, 1023));
        assert_eq!(locate(1024), (1, 0));
        assert_eq!(locate(1024 + 2047), (1, 2047));
        assert_eq!(locate(1024 + 2048), (2, 0));
    }

    #[test]
    fn items_never_move() {
        let v = AppendVec::new();
        for i in 0..5000u32 {
            assert_eq!(v.push(i * 2), i);
        }
        let first: *const u32 = &raw const v[0];
        v.push(1);
        assert!(core::ptr::eq(first, &raw const v[0]));
        assert_eq!(v[4999], 9998);
        assert_eq!(v.len(), 5001);
    }
}
