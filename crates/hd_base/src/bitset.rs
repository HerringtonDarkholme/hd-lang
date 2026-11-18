//! A set of `u32` whose storage is its members, not its highest member
//! (#2154): a sorted vector while sparse, a bitset over the words its
//! members span once that is no larger, as rustc's `HybridBitSet` keeps a
//! sparse form below a threshold and a dense one above. Unlike rustc's,
//! the dense form spans only the words between its lowest and highest
//! member, so its size is bounded by its members too, never by the domain.
//!
//! Sets are built once by a [`Builder`] from members and other sets, which
//! is what a fixpoint over a topological order needs: a union costs the
//! parts added (a dense part by its words), never the set receiving them.

use crate::SliceSort as _;

/// A set of `u32`. Its [`size`](HybridSet::size), in 64-bit words, is at
/// most half its member count, rounded up: sparse sets store two members
/// per word, dense sets keep at least two members per word.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct HybridSet(Repr);

#[derive(Clone, PartialEq, Eq, Debug)]
enum Repr {
    /// The members, increasing.
    Sparse(Vec<u32>),
    /// Bit `b` of `words[i]` is member `(base + i) * 64 + b`; the first and
    /// last words are nonzero, and `2 * words.len() <= len`. A word index
    /// is a member over 64, so it and `base + i` fit in `u32`.
    Dense {
        base: u32,
        len: usize,
        words: Vec<u64>,
    },
}

impl Default for Repr {
    fn default() -> Self {
        Repr::Sparse(Vec::new())
    }
}

impl HybridSet {
    /// Whether `x` is a member: a binary search while sparse, one word
    /// read when dense.
    #[must_use]
    pub fn contains(&self, x: u32) -> bool {
        match &self.0 {
            Repr::Sparse(v) => v.binary_search(&x).is_ok(),
            Repr::Dense { base, words, .. } => (x / 64)
                .checked_sub(*base)
                .and_then(|i| words.get(i as usize))
                .is_some_and(|w| w & (1 << (x % 64)) != 0),
        }
    }

    /// The number of members.
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.0 {
            Repr::Sparse(v) => v.len(),
            Repr::Dense { len, .. } => *len,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The 64-bit words a full read touches: two members per word while
    /// sparse, the bitset's words when dense.
    #[must_use]
    pub fn size(&self) -> usize {
        match &self.0 {
            Repr::Sparse(v) => v.len().div_ceil(2),
            Repr::Dense { words, .. } => words.len(),
        }
    }

    /// The members, in increasing order.
    #[must_use]
    pub fn iter(&self) -> Iter<'_> {
        match &self.0 {
            Repr::Sparse(v) => Iter {
                sparse: v.iter(),
                words: &[],
                base: 0,
                at: 0,
                cur: 0,
            },
            Repr::Dense { base, words, .. } => Iter {
                sparse: [].iter(),
                words,
                base: *base,
                at: 0,
                cur: 0,
            },
        }
    }

    /// The set of sorted, distinct `members`, dense when that is no
    /// larger.
    fn from_sorted(members: Vec<u32>) -> Self {
        let (Some(&lo), Some(&hi)) = (members.first(), members.last()) else {
            return HybridSet::default();
        };
        let span = (hi / 64 - lo / 64) as usize + 1;
        if 2 * span > members.len() {
            return HybridSet(Repr::Sparse(members));
        }
        let mut words = vec![0u64; span];
        for &x in &members {
            words[(x / 64 - lo / 64) as usize] |= 1 << (x % 64);
        }
        HybridSet(Repr::Dense {
            base: lo / 64,
            len: members.len(),
            words,
        })
    }
}

impl<'a> IntoIterator for &'a HybridSet {
    type Item = u32;
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

/// The members of a [`HybridSet`], in increasing order.
pub struct Iter<'a> {
    sparse: std::slice::Iter<'a, u32>,
    words: &'a [u64],
    base: u32,
    /// The next word of `words` to load.
    at: u32,
    /// The bits of word `at - 1` not yet returned.
    cur: u64,
}

impl Iterator for Iter<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        if let Some(&x) = self.sparse.next() {
            return Some(x);
        }
        while self.cur == 0 {
            self.cur = *self.words.get(self.at as usize)?;
            self.at += 1;
        }
        let b = self.cur.trailing_zeros();
        self.cur &= self.cur - 1;
        Some((self.base + self.at - 1) * 64 + b)
    }
}

/// Collects members and sets, then makes one [`HybridSet`]. Adding costs
/// what is added: one entry per member, one per word of a dense set.
#[derive(Default, Debug)]
pub struct Builder {
    members: Vec<u32>,
    /// `(word index, bits)` of the dense sets added.
    words: Vec<(u32, u64)>,
}

impl Builder {
    pub fn insert(&mut self, x: u32) {
        self.members.push(x);
    }

    /// Adds every member of `s`, in `s.size()` words.
    pub fn union(&mut self, s: &HybridSet) {
        match &s.0 {
            Repr::Sparse(v) => self.members.extend_from_slice(v),
            Repr::Dense { base, words, .. } => self.words.extend(
                (*base..)
                    .zip(words)
                    .filter(|(_, w)| **w != 0)
                    .map(|(i, &w)| (i, w)),
            ),
        }
    }

    /// The words added so far: two members per word, and each nonzero
    /// word of a dense set.
    #[must_use]
    pub fn parts(&self) -> usize {
        self.members.len().div_ceil(2) + self.words.len()
    }

    /// The set of everything added. With `t` members and words added and `m`
    /// members out: O(t + m) when they span at most `2t` words, else a sort,
    /// O((t + m) log(t + m)).
    #[must_use]
    pub fn finish(self) -> HybridSet {
        let Builder { mut members, words } = self;
        let word_ixs = (members.iter().map(|x| x / 64)).chain(words.iter().map(|w| w.0));
        let (Some(lo), Some(hi)) = (word_ixs.clone().min(), word_ixs.max()) else {
            return HybridSet::default();
        };
        let span = (hi - lo) as usize + 1;
        if span > 2 * (members.len() + words.len()) {
            // Too spread for a bitmap the parts pay for: sort the members.
            for &(i, mut w) in &words {
                while w != 0 {
                    members.push(i * 64 + w.trailing_zeros());
                    w &= w - 1;
                }
            }
            members.sort_stable();
            members.dedup();
            return HybridSet::from_sorted(members);
        }
        let mut bits = vec![0u64; span];
        for &(i, w) in &words {
            bits[(i - lo) as usize] |= w;
        }
        for &x in &members {
            bits[(x / 64 - lo) as usize] |= 1 << (x % 64);
        }
        let len = bits.iter().map(|w| w.count_ones() as usize).sum();
        let set = HybridSet(Repr::Dense {
            base: lo,
            len,
            words: bits,
        });
        if 2 * span <= len {
            return set;
        }
        HybridSet(Repr::Sparse(set.iter().collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::{Builder, HybridSet, Repr};

    fn build(members: &[u32], sets: &[&HybridSet]) -> HybridSet {
        let mut b = Builder::default();
        for &x in members {
            b.insert(x);
        }
        for s in sets {
            b.union(s);
        }
        b.finish()
    }

    fn dense(s: &HybridSet) -> bool {
        matches!(s.0, Repr::Dense { .. })
    }

    #[test]
    fn empty_sets_hold_nothing() {
        for s in [HybridSet::default(), Builder::default().finish()] {
            assert!(s.is_empty() && s.size() == 0 && !s.contains(0));
            assert_eq!(s.iter().next(), None);
        }
    }

    #[test]
    fn spread_members_stay_sparse_and_cost_their_count() {
        // Members far apart: a bitmap would span 1,000 words for 3 members.
        let s = build(&[64_000, 5, 5, 700], &[]);
        assert!(!dense(&s));
        assert_eq!((s.len(), s.size()), (3, 2));
        assert_eq!(s.iter().collect::<Vec<_>>(), [5, 700, 64_000]);
        assert!(s.contains(700) && !s.contains(6) && !s.contains(u32::MAX));
    }

    #[test]
    fn packed_members_go_dense_over_their_own_span() {
        // 100 members in words 10 and 11: two words, based at word 10.
        let xs: Vec<u32> = (640..740).collect();
        let s = build(&xs, &[]);
        assert!(dense(&s));
        assert_eq!((s.len(), s.size()), (100, 2));
        assert_eq!(s.iter().collect::<Vec<_>>(), xs);
        assert!(s.contains(640) && s.contains(739));
        assert!(!s.contains(639) && !s.contains(740) && !s.contains(0) && !s.contains(5_000));
    }

    #[test]
    fn unions_of_dense_and_sparse_sets() {
        let a = build(&(0..64).collect::<Vec<_>>(), &[]);
        let b = build(&(64..128).collect::<Vec<_>>(), &[]);
        assert!(dense(&a) && dense(&b));
        // Dense words OR into one bitmap, with a sparse part and a member.
        let c = build(&[3, 130], &[&a, &b, &build(&[129], &[])]);
        assert!(dense(&c));
        assert_eq!(c.len(), 130);
        assert_eq!(
            c.iter().collect::<Vec<_>>(),
            (0..131).filter(|&x| x != 128).collect::<Vec<_>>()
        );
        // A dense part with one far member: the span is too wide for a
        // bitmap, so the words expand to members and sort.
        let d = build(&[1_000_000], &[&a]);
        assert!(!dense(&d));
        assert_eq!(d.len(), 65);
        assert_eq!(d.iter().last(), Some(1_000_000));
        // A bitmap the parts pay for, with too few members to stay dense.
        let e = build(&[0, 70, 140, 200], &[]);
        assert!(!dense(&e));
        assert_eq!(e.iter().collect::<Vec<_>>(), [0, 70, 140, 200]);
        let f = build(&[], &[&e, &build(&[1], &[])]);
        assert_eq!(f.iter().collect::<Vec<_>>(), [0, 1, 70, 140, 200]);
        // Two full words 29 apart: too spread for a bitmap of the parts,
        // yet 128 sorted members pay for their 30 words.
        let g = build(&(29 * 64..30 * 64).collect::<Vec<_>>(), &[]);
        let h = build(&[], &[&a, &g]);
        assert!(dense(&h));
        assert_eq!((h.len(), h.size()), (128, 30));
        assert!(h.contains(29 * 64) && !h.contains(64));
    }

    #[test]
    fn a_builder_counts_its_parts() {
        let a = build(&(0..128).collect::<Vec<_>>(), &[]);
        let mut b = Builder::default();
        b.insert(7);
        b.union(&a);
        b.union(&build(&[9, 10_000], &[]));
        // Members 7, 9 and 10,000 in two words, and `a`'s two words.
        assert_eq!((a.size(), b.parts()), (2, 4));
        assert_eq!(b.finish().len(), 129);
    }
}
