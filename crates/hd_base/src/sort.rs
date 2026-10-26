//! `SliceSort`: the workspace's slice sort, in place of `<[T]>::sort` and
//! its relatives (clippy's `disallowed-methods` points here).
//!
//! The standard library's sorts are fast but large: each instantiation, a
//! pair of element type and comparison closure, carries its own copy of
//! driftsort or ipnsort, about 1.6 KB of the browser compiler's gzip
//! download; replacing ninety of them cut 109 KB (#49). This sort stays
//! generic over the closure, so comparisons are still inlined, but each
//! instantiation is small: insertion sort up to `SMALL` elements, and
//! above that a bottom-up merge sort of `u32` positions followed by one
//! in-place permutation of the slice. `core` makes the same trade under
//! its `optimize_for_size` feature, which a stable toolchain cannot select.
//!
//! The cost is speed on long slices: natively, about 2.5 times std's time
//! per element from 30 to 2,000 elements and about 10 times at 100,000.
//! The compiler's sorts are short and few; checking and building a
//! 10,000- or 50,000-line package took the same time within noise.
//!
//! Every method is stable. The `unstable` std methods map to them too: a
//! stable order is one of the orders an unstable sort may produce, and it
//! keeps output deterministic.

use std::cmp::Ordering;

/// Lengths up to this sort in place by insertion, without allocating.
const SMALL: usize = 20;

/// Stable in-place sorts of a slice. Import it as `use hd_base::SliceSort as _;`.
pub trait SliceSort<T> {
    /// Sorts by `Ord`, keeping equal elements in order.
    fn sort_stable(&mut self)
    where
        T: Ord;
    /// Sorts by `cmp`, keeping elements it calls equal in order.
    fn sort_stable_by<F: FnMut(&T, &T) -> Ordering>(&mut self, cmp: F);
    /// Sorts by the key `f` gives, which it computes at every comparison.
    fn sort_stable_by_key<K: Ord, F: FnMut(&T) -> K>(&mut self, f: F);
    /// Sorts by the key `f` gives, computing it once per element.
    fn sort_stable_by_cached_key<K: Ord, F: FnMut(&T) -> K>(&mut self, f: F);
}

impl<T> SliceSort<T> for [T] {
    fn sort_stable(&mut self)
    where
        T: Ord,
    {
        sort_by_less(self, |a, b| a < b);
    }

    fn sort_stable_by<F: FnMut(&T, &T) -> Ordering>(&mut self, mut cmp: F) {
        sort_by_less(self, |a, b| cmp(a, b) == Ordering::Less);
    }

    fn sort_stable_by_key<K: Ord, F: FnMut(&T) -> K>(&mut self, mut f: F) {
        sort_by_less(self, |a, b| f(a) < f(b));
    }

    fn sort_stable_by_cached_key<K: Ord, F: FnMut(&T) -> K>(&mut self, f: F) {
        if self.len() < 2 {
            return;
        }
        let keys: Vec<K> = self.iter().map(f).collect();
        let mut order = positions(self.len(), |a, b| keys[a as usize] < keys[b as usize]);
        permute(self, &mut order);
    }
}

fn sort_by_less<T>(v: &mut [T], mut less: impl FnMut(&T, &T) -> bool) {
    if v.len() <= SMALL {
        for i in 1..v.len() {
            let mut j = i;
            while j > 0 && less(&v[j], &v[j - 1]) {
                v.swap(j - 1, j);
                j -= 1;
            }
        }
        return;
    }
    let mut order = positions(v.len(), |a, b| less(&v[a as usize], &v[b as usize]));
    permute(v, &mut order);
}

/// The positions `0..n` stably sorted by `less`.
fn positions(n: usize, mut less: impl FnMut(u32, u32) -> bool) -> Vec<u32> {
    let len = u32::try_from(n).expect("a sorted slice has fewer than 2^32 elements");
    let mut src: Vec<u32> = (0..len).collect();
    let mut dst = vec![0; n];
    let mut width = 1;
    while width < n {
        for start in (0..n).step_by(2 * width) {
            let mid = (start + width).min(n);
            let end = (start + 2 * width).min(n);
            let (left, right) = src[start..end].split_at(mid - start);
            let out = &mut dst[start..end];
            let (mut i, mut j) = (0, 0);
            for slot in out.iter_mut() {
                if i < left.len() && (j == right.len() || !less(right[j], left[i])) {
                    *slot = left[i];
                    i += 1;
                } else {
                    *slot = right[j];
                    j += 1;
                }
            }
        }
        std::mem::swap(&mut src, &mut dst);
        width *= 2;
    }
    src
}

/// Moves `v[order[i]]` to position `i`, following each cycle of `order`
/// with swaps. Leaves `order` as the identity.
fn permute<T>(v: &mut [T], order: &mut [u32]) {
    for start in 0..v.len() {
        let mut at = start;
        loop {
            let from = order[at] as usize;
            order[at] = u32::try_from(at).unwrap_or(u32::MAX);
            if from == start {
                break;
            }
            v.swap(at, from);
            at = from;
        }
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // std's sorts are the reference
mod tests {
    use super::SliceSort;

    /// A small linear congruential generator: deterministic test inputs.
    fn numbers(n: usize, seed: u64, range: u64) -> Vec<u64> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                (x >> 33) % range
            })
            .collect()
    }

    #[test]
    fn matches_std_stable_sort() {
        for n in [
            0, 1, 2, 3, 15, 16, 17, 20, 21, 31, 32, 33, 100, 257, 1000, 4099,
        ] {
            for range in [2, 10, 1 << 30] {
                let keys = numbers(n, n as u64 * 31 + range, range);
                // Pair each key with its position, so stability shows.
                let items: Vec<(u64, usize)> = keys.iter().copied().zip(0..).collect();
                let mut expected = items.clone();
                expected.sort_by_key(|p| p.0);
                let mut by_key = items.clone();
                by_key.sort_stable_by_key(|p| p.0);
                assert_eq!(by_key, expected, "by_key n={n} range={range}");
                let mut cached = items.clone();
                cached.sort_stable_by_cached_key(|p| p.0);
                assert_eq!(cached, expected, "cached n={n} range={range}");
                let mut by = items.clone();
                by.sort_stable_by(|a, b| a.0.cmp(&b.0));
                assert_eq!(by, expected, "by n={n} range={range}");
                let mut plain = keys.clone();
                plain.sort_stable();
                let mut std_plain = keys.clone();
                std_plain.sort_unstable();
                assert_eq!(plain, std_plain, "plain n={n} range={range}");
            }
        }
    }

    #[test]
    fn sorts_owned_values_and_presorted_input() {
        let mut words: Vec<String> = numbers(300, 7, 50)
            .iter()
            .map(|n| format!("w{n}"))
            .collect();
        let mut expected = words.clone();
        expected.sort();
        words.sort_stable();
        assert_eq!(words, expected);
        let mut ascending: Vec<u32> = (0..500).collect();
        ascending.sort_stable();
        assert!(ascending.windows(2).all(|w| w[0] < w[1]));
        let mut descending: Vec<u32> = (0..500).rev().collect();
        descending.sort_stable();
        assert_eq!(descending, ascending);
    }
}
