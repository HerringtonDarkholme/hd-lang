//! `AppendVec` and frozen columns (data-structures.md §3.9.4).
//!
//! The design's `AppendVec` is a chunked column whose items never move:
//! chunk `k` holds `2^(k+10)` items, readers index without a lock, and
//! appends are serialized. This is the one audited `unsafe` module of the
//! workspace (owner, 2026-10-07): chunks are raw allocations of
//! `MaybeUninit<T>`, and the published length is the only thing a reader
//! trusts.
//!
//! Invariants:
//! 1. Appends hold `writer`, so at most one thread writes a slot or a chunk
//!    pointer at a time.
//! 2. A chunk pointer is stored (`Release`) before any item in it is
//!    published, and is never changed or freed until `drop`.
//! 3. Slot `i` is written exactly once, before `len` is raised past `i`
//!    with a `Release` store; readers load `len` with `Acquire` and read
//!    only slots below it. Published slots are never written again.
#![allow(unsafe_code)]

use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

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

/// Growable column whose items never move (data-structures.md §3.9.4):
/// dense `T` slots in doubling chunks, a published length, lock-free reads.
pub struct AppendVec<T> {
    chunks: [AtomicPtr<MaybeUninit<T>>; CHUNKS],
    len: AtomicU32,
    writer: Mutex<()>,
    /// Owns `T`s; the `Send`/`Sync` impls below state the real bounds.
    _owns: PhantomData<*const T>,
}

// SAFETY: an `AppendVec<T>` owns its `T`s; moving it to another thread
// moves them, which `T: Send` allows.
unsafe impl<T: Send> Send for AppendVec<T> {}
// SAFETY: through `&AppendVec<T>` other threads read `&T` (needs `T: Sync`)
// and push values that the owner later drops (needs `T: Send`). Writes are
// serialized by `writer` (invariant 1) and published by `len` (invariant 3).
unsafe impl<T: Send + Sync> Sync for AppendVec<T> {}

impl<T> Default for AppendVec<T> {
    fn default() -> Self {
        Self {
            chunks: std::array::from_fn(|_| AtomicPtr::new(ptr::null_mut())),
            len: AtomicU32::new(0),
            writer: Mutex::new(()),
            _owns: PhantomData,
        }
    }
}

/// Chunk and offset of item `i`: chunk k covers `[2^(k+10) - 2^10, 2^(k+11) - 2^10)`.
fn locate(i: u32) -> (usize, usize) {
    let biased = u64::from(i) + (1 << BASE_BITS);
    let k = biased.ilog2() - BASE_BITS;
    let start = (1u64 << (k + BASE_BITS)) - (1 << BASE_BITS);
    (
        k as usize,
        usize::try_from(u64::from(i) - start).expect("offset"),
    )
}

const fn chunk_cap(k: usize) -> usize {
    1usize << (k + BASE_BITS as usize)
}

impl<T> AppendVec<T> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one item and returns its index. Appends are serialized;
    /// readers are never blocked.
    pub fn push(&self, value: T) -> u32 {
        let _w = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let i = self.len.load(Ordering::Relaxed);
        let (k, off) = locate(i);
        assert!(k < CHUNKS, "AppendVec over capacity");
        let mut chunk = self.chunks[k].load(Ordering::Acquire);
        if chunk.is_null() {
            let fresh: Box<[MaybeUninit<T>]> =
                (0..chunk_cap(k)).map(|_| MaybeUninit::uninit()).collect();
            chunk = Box::into_raw(fresh).cast::<MaybeUninit<T>>();
            self.chunks[k].store(chunk, Ordering::Release);
        }
        // SAFETY: `chunk` points at `chunk_cap(k)` slots (allocated above or
        // earlier, never freed before drop) and `off < chunk_cap(k)` by
        // `locate`. Slot `i` is unpublished (`i == len`), so no reader looks
        // at it, and `writer` excludes other writers (invariant 1).
        unsafe { chunk.add(off).write(MaybeUninit::new(value)) };
        self.len.store(i + 1, Ordering::Release);
        i
    }

    #[must_use]
    pub fn get(&self, i: u32) -> Option<&T> {
        if i >= self.len.load(Ordering::Acquire) {
            return None;
        }
        let (k, off) = locate(i);
        let chunk = self.chunks[k].load(Ordering::Acquire);
        // SAFETY: `i < len` (Acquire) means slot `i` was written and its
        // chunk pointer stored before `len` was raised (invariants 2, 3), so
        // `chunk` is non-null, `off` is in bounds and the slot is
        // initialized. Published slots are never written again, so the
        // shared borrow lives as long as `&self`.
        Some(unsafe { (*chunk.add(off)).assume_init_ref() })
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

impl<T> Drop for AppendVec<T> {
    fn drop(&mut self) {
        let len = *self.len.get_mut();
        for i in 0..len {
            let (k, off) = locate(i);
            let chunk = *self.chunks[k].get_mut();
            // SAFETY: `&mut self` excludes readers and writers; slot `i < len`
            // is initialized (invariant 3) and dropped exactly once here.
            unsafe { (*chunk.add(off)).assume_init_drop() };
        }
        for (k, c) in self.chunks.iter_mut().enumerate() {
            let chunk = *c.get_mut();
            if !chunk.is_null() {
                // SAFETY: `chunk` came from `Box::into_raw` of a boxed slice
                // of `chunk_cap(k)` `MaybeUninit<T>`s in `push` and is freed
                // only here; `MaybeUninit` has no drop of its own.
                drop(unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(chunk, chunk_cap(k))) });
            }
        }
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    /// Std-threads stress test (loom is not a dependency): four writers and
    /// four readers race; every published item reads back intact, every
    /// push gets a distinct index, and drop runs once per item.
    #[test]
    fn concurrent_push_and_read() {
        const PER: u64 = 20_000;
        struct Counted(u64, Arc<AtomicUsize>);
        impl Drop for Counted {
            fn drop(&mut self) {
                self.1.fetch_add(1, Ordering::Relaxed);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let v: AppendVec<Counted> = AppendVec::new();

        std::thread::scope(|s| {
            for w in 0..4u64 {
                let v = &v;
                let drops = &drops;
                s.spawn(move || {
                    for n in 0..PER {
                        let i = v.push(Counted(w * PER + n, Arc::clone(drops)));
                        assert_eq!(v[i].0, w * PER + n);
                    }
                });
            }
            for _ in 0..4 {
                let v = &v;
                s.spawn(move || {
                    let mut seen = 0;
                    while seen < 4 * PER {
                        let len = v.len();
                        for i in 0..len {
                            let x = v.get(i).expect("published").0;
                            assert!(x < 4 * PER);
                        }
                        seen = u64::from(len);
                    }
                });
            }
        });
        let mut all: Vec<u64> = v.iter().map(|c| c.0).collect();
        all.sort_unstable();
        assert_eq!(all, (0..4 * PER).collect::<Vec<_>>());
        drop(v);
        assert_eq!(
            drops.load(Ordering::Relaxed),
            usize::try_from(4 * PER).expect("n")
        );
    }
}
