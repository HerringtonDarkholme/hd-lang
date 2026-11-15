//! Persistent sets of locals for the liveness pass ([`super::Live`]).
//!
//! A body of `n` blocks with `n` locals live across all of them holds
//! `n²` facts if each block keeps its own copy of its live-in set. These
//! sets are big-endian Patricia tries (Okasaki and Gill, "Fast Mergeable
//! Integer Maps", 1998) in one arena: a block's set shares every subtree
//! its instructions leave alone with the sets it was built from. An insert
//! or removal copies one path, at most 32 nodes (one per key bit) and
//! about `log2 n` in practice. A union stops at subtrees the two sides
//! share, so joining the sets of two blocks that differ in `d` locals costs
//! about `d log2 n`, not their size. The recursion follows key bits, so it
//! is at most 32 deep, whatever the body.
//!
//! An arena from [`Sets::default`] hash-conses its nodes, as Filliâtre
//! and Conchon's hash-consed Patricia trees are ("Type-Safe Modular
//! Hash-Consing", 2006): a trie's shape depends only on its keys, so two
//! sets with the same keys are one node however they were built, and a
//! difference of sets built apart still stops where they agree. Only
//! [`super::coalesce`]'s differences need that; the passes that ask only
//! membership, union and intersection use [`Sets::unconsed`], which
//! hashes no node.

/// A set in a [`Sets`] arena: a node index, or [`Set::EMPTY`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Set(u32);

impl Set {
    pub(super) const EMPTY: Self = Self(u32::MAX);
}

/// A leaf holds one key and has `bit == 0`. A branch holds the keys that
/// agree with `prefix` above `bit`: those with `bit` clear under `zero`,
/// the others under `one`, `len` of them in all.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Node {
    prefix: u32,
    bit: u32,
    zero: Set,
    one: Set,
    len: u32,
}

/// The arena of every set one liveness pass builds. Nodes are never
/// changed once made, so a set stays valid while others grow from it.
#[derive(Default)]
pub(super) struct Sets {
    nodes: Vec<Node>,
    /// The hash-consing table: each node's index in `nodes`, hashed by the
    /// node it names, as `indexmap` and rustc's interners key a table by
    /// index. An entry is 4 bytes and a control byte, where a map from
    /// node to set held a copy of the 20-byte node and its 4-byte set
    /// (#2026).
    made: hashbrown::HashTable<u32>,
    /// The hasher of `made`'s nodes.
    hasher: hd_base::FixedState,
    /// Whether nodes are hash-consed through `made`; see [`Sets::unconsed`].
    unconsed: bool,
    /// Each node asked for, made or found, and each node a difference
    /// or a listing visits: the work of everything built on the arena.
    work: std::cell::Cell<usize>,
}

/// The bits of `key` above `bit`.
fn high(key: u32, bit: u32) -> u32 {
    key & !(bit | (bit - 1))
}

/// The highest bit where `a` and `b` differ; they must differ.
fn branch_bit(a: u32, b: u32) -> u32 {
    1 << (a ^ b).ilog2()
}

impl Sets {
    /// An arena that makes each node it is asked for, with no hash per
    /// node: two sets of one content may be two nodes, so `==` on sets
    /// means the same node, not the same keys. Every operation stays
    /// correct, and keeps sharing the subtrees it leaves alone; a caller
    /// that needs equal contents compares [`Sets::len`] of a set and a
    /// subset of it, as rule 11's meet does.
    pub(super) fn unconsed() -> Self {
        Sets {
            unconsed: true,
            ..Sets::default()
        }
    }

    /// The nodes made so far: the pass's storage.
    #[cfg(test)]
    pub(super) fn made(&self) -> usize {
        self.nodes.len()
    }

    /// The nodes asked for and visited so far: the pass's work.
    pub(super) fn work(&self) -> usize {
        self.work.get()
    }

    fn tick(&self) {
        self.work.set(self.work.get() + 1);
    }

    fn node(&self, s: Set) -> Node {
        self.nodes[s.0 as usize]
    }

    fn push(&mut self, node: Node) -> Set {
        use hashbrown::hash_table::Entry;
        use std::hash::BuildHasher as _;
        self.tick();
        // ruled out: more than `u32::MAX - 1` nodes; a pass makes at most
        // 32 per instruction, and a body's length is a u32.
        let id = u32::try_from(self.nodes.len()).expect("live set nodes");
        assert_ne!(id, u32::MAX, "live set nodes");
        if !self.unconsed {
            let (nodes, hasher) = (&self.nodes, self.hasher);
            match self.made.entry(
                hasher.hash_one(node),
                |&i| nodes[i as usize] == node,
                |&i| hasher.hash_one(nodes[i as usize]),
            ) {
                Entry::Occupied(found) => return Set(*found.get()),
                Entry::Vacant(slot) => {
                    slot.insert(id);
                }
            }
        }
        self.nodes.push(node);
        Set(id)
    }

    fn leaf(&mut self, key: u32) -> Set {
        self.push(Node {
            prefix: key,
            bit: 0,
            zero: Set::EMPTY,
            one: Set::EMPTY,
            len: 1,
        })
    }

    fn branch(&mut self, prefix: u32, bit: u32, zero: Set, one: Set) -> Set {
        let len = self.len(zero) + self.len(one);
        self.push(Node {
            prefix,
            bit,
            zero,
            one,
            // ruled out: a set holds local indices, which are u32.
            len: u32::try_from(len).expect("live set size"),
        })
    }

    /// A set of `a`, whose keys agree with `ka` above its bit, and `b`,
    /// whose keys agree with `kb` above its bit, where the two disagree
    /// above both bits.
    fn join(&mut self, ka: u32, a: Set, kb: u32, b: Set) -> Set {
        let bit = branch_bit(ka, kb);
        if ka & bit == 0 {
            self.branch(high(ka, bit), bit, a, b)
        } else {
            self.branch(high(ka, bit), bit, b, a)
        }
    }

    pub(super) fn len(&self, s: Set) -> usize {
        if s == Set::EMPTY {
            0
        } else {
            self.node(s).len as usize
        }
    }

    pub(super) fn contains(&self, mut s: Set, key: u32) -> bool {
        while s != Set::EMPTY {
            let n = self.node(s);
            if n.bit == 0 {
                return n.prefix == key;
            }
            if high(key, n.bit) != n.prefix {
                return false;
            }
            s = if key & n.bit == 0 { n.zero } else { n.one };
        }
        false
    }

    /// The set of `keys`, which are sorted and distinct, made in one
    /// pass: each of its `2k - 1` nodes once, where `k` inserts would
    /// copy a path of about `log2 k` nodes each. The split follows key
    /// bits, so the recursion is at most 32 deep.
    pub(super) fn of_sorted(&mut self, keys: &[u32]) -> Set {
        match keys {
            [] => Set::EMPTY,
            [key] => self.leaf(*key),
            [first, .., last] => {
                let bit = branch_bit(*first, *last);
                let split = keys.partition_point(|k| k & bit == 0);
                let zero = self.of_sorted(&keys[..split]);
                let one = self.of_sorted(&keys[split..]);
                self.branch(high(*first, bit), bit, zero, one)
            }
        }
    }

    /// `s` with `key`; `s` itself when it holds `key` already.
    pub(super) fn insert(&mut self, s: Set, key: u32) -> Set {
        if s == Set::EMPTY {
            return self.leaf(key);
        }
        let n = self.node(s);
        if n.bit == 0 {
            if n.prefix == key {
                return s;
            }
            let leaf = self.leaf(key);
            return self.join(key, leaf, n.prefix, s);
        }
        if high(key, n.bit) != n.prefix {
            let leaf = self.leaf(key);
            return self.join(key, leaf, n.prefix, s);
        }
        if key & n.bit == 0 {
            let zero = self.insert(n.zero, key);
            if zero == n.zero {
                s
            } else {
                self.branch(n.prefix, n.bit, zero, n.one)
            }
        } else {
            let one = self.insert(n.one, key);
            if one == n.one {
                s
            } else {
                self.branch(n.prefix, n.bit, n.zero, one)
            }
        }
    }

    /// `s` without `key`; `s` itself when it does not hold `key`.
    pub(super) fn remove(&mut self, s: Set, key: u32) -> Set {
        if s == Set::EMPTY {
            return s;
        }
        let n = self.node(s);
        if n.bit == 0 {
            return if n.prefix == key { Set::EMPTY } else { s };
        }
        if high(key, n.bit) != n.prefix {
            return s;
        }
        let (zero, one) = if key & n.bit == 0 {
            (self.remove(n.zero, key), n.one)
        } else {
            (n.zero, self.remove(n.one, key))
        };
        if zero == n.zero && one == n.one {
            s
        } else if zero == Set::EMPTY {
            one
        } else if one == Set::EMPTY {
            zero
        } else {
            self.branch(n.prefix, n.bit, zero, one)
        }
    }

    /// The union of `a` and `b`, made only along the paths where they
    /// differ: a subtree the two share is kept as it is.
    pub(super) fn union(&mut self, a: Set, b: Set) -> Set {
        if a == b || b == Set::EMPTY {
            return a;
        }
        if a == Set::EMPTY {
            return b;
        }
        let (x, y) = (self.node(a), self.node(b));
        if x.bit == 0 {
            return self.insert(b, x.prefix);
        }
        if y.bit == 0 {
            return self.insert(a, y.prefix);
        }
        if x.bit == y.bit && x.prefix == y.prefix {
            let zero = self.union(x.zero, y.zero);
            let one = self.union(x.one, y.one);
            return if zero == x.zero && one == x.one {
                a
            } else if zero == y.zero && one == y.one {
                b
            } else {
                self.branch(x.prefix, x.bit, zero, one)
            };
        }
        if x.bit > y.bit && high(y.prefix, x.bit) == x.prefix {
            // `b` lies under one side of `a`.
            return if y.prefix & x.bit == 0 {
                let zero = self.union(x.zero, b);
                if zero == x.zero {
                    a
                } else {
                    self.branch(x.prefix, x.bit, zero, x.one)
                }
            } else {
                let one = self.union(x.one, b);
                if one == x.one {
                    a
                } else {
                    self.branch(x.prefix, x.bit, x.zero, one)
                }
            };
        }
        if y.bit > x.bit && high(x.prefix, y.bit) == y.prefix {
            return if x.prefix & y.bit == 0 {
                let zero = self.union(a, y.zero);
                if zero == y.zero {
                    b
                } else {
                    self.branch(y.prefix, y.bit, zero, y.one)
                }
            } else {
                let one = self.union(a, y.one);
                if one == y.one {
                    b
                } else {
                    self.branch(y.prefix, y.bit, y.zero, one)
                }
            };
        }
        self.join(x.prefix, a, y.prefix, b)
    }

    /// The keys `a` and `b` share, made only along the paths where they
    /// differ: a subtree the two share is kept as it is, so meeting two
    /// sets that differ in `d` keys costs about `d log2 n`.
    pub(super) fn intersection(&mut self, a: Set, b: Set) -> Set {
        if a == b {
            return a;
        }
        if a == Set::EMPTY || b == Set::EMPTY {
            return Set::EMPTY;
        }
        self.tick();
        let (x, y) = (self.node(a), self.node(b));
        if x.bit == 0 {
            return if self.contains(b, x.prefix) {
                a
            } else {
                Set::EMPTY
            };
        }
        if y.bit == 0 {
            return if self.contains(a, y.prefix) {
                b
            } else {
                Set::EMPTY
            };
        }
        if x.bit == y.bit && x.prefix == y.prefix {
            let zero = self.intersection(x.zero, y.zero);
            let one = self.intersection(x.one, y.one);
            return if zero == Set::EMPTY {
                one
            } else if one == Set::EMPTY {
                zero
            } else if zero == x.zero && one == x.one {
                a
            } else if zero == y.zero && one == y.one {
                b
            } else {
                self.branch(x.prefix, x.bit, zero, one)
            };
        }
        if x.bit > y.bit && high(y.prefix, x.bit) == x.prefix {
            // `b` lies under one side of `a`.
            let side = if y.prefix & x.bit == 0 { x.zero } else { x.one };
            return self.intersection(side, b);
        }
        if y.bit > x.bit && high(x.prefix, y.bit) == y.prefix {
            let side = if x.prefix & y.bit == 0 { y.zero } else { y.one };
            return self.intersection(a, side);
        }
        Set::EMPTY
    }

    /// The keys of `b` that `a` lacks, appended to `out` in any order,
    /// found only along the paths where the two differ: a subtree they
    /// share is skipped, so a `b` made from `a` by adding `d` keys costs
    /// about `d log2 n`.
    pub(super) fn difference(&self, b: Set, a: Set, out: &mut Vec<u32>) {
        self.difference_upto(b, a, out, usize::MAX);
    }

    /// [`Sets::difference`], stopping once `out` holds `limit` keys: the
    /// walk then costs about the keys it finds, not all of `b`'s that `a`
    /// lacks.
    pub(super) fn difference_upto(&self, b: Set, a: Set, out: &mut Vec<u32>, limit: usize) {
        self.tick();
        if a == b || b == Set::EMPTY || out.len() >= limit {
            return;
        }
        if a == Set::EMPTY {
            return self.members_upto(b, out, limit);
        }
        let (x, y) = (self.node(b), self.node(a));
        if x.bit == 0 {
            if !self.contains(a, x.prefix) {
                out.push(x.prefix);
            }
            return;
        }
        if y.bit == 0 {
            // `a` is one key: `b`'s keys but that one, from one more.
            let from = out.len();
            self.members_upto(b, out, limit.saturating_add(1));
            if let Some(i) = out[from..].iter().position(|&k| k == y.prefix) {
                out.remove(from + i);
            }
            out.truncate(limit.max(from));
            return;
        }
        if x.bit == y.bit && x.prefix == y.prefix {
            self.difference_upto(x.zero, y.zero, out, limit);
            self.difference_upto(x.one, y.one, out, limit);
        } else if x.bit > y.bit && high(y.prefix, x.bit) == x.prefix {
            // `a` lies under one side of `b`.
            if y.prefix & x.bit == 0 {
                self.difference_upto(x.zero, a, out, limit);
                self.members_upto(x.one, out, limit);
            } else {
                self.members_upto(x.zero, out, limit);
                self.difference_upto(x.one, a, out, limit);
            }
        } else if y.bit > x.bit && high(x.prefix, y.bit) == y.prefix {
            // `b` lies under one side of `a`.
            let side = if x.prefix & y.bit == 0 { y.zero } else { y.one };
            self.difference_upto(b, side, out, limit);
        } else {
            self.members_upto(b, out, limit);
        }
    }

    /// The keys of `s`, ascending, appended to `out`.
    pub(super) fn members(&self, s: Set, out: &mut Vec<u32>) {
        self.members_upto(s, out, usize::MAX);
    }

    /// [`Sets::members`] until `out` holds `limit` keys.
    fn members_upto(&self, s: Set, out: &mut Vec<u32>, limit: usize) {
        let mut stack = vec![s];
        while let Some(s) = stack.pop() {
            self.tick();
            if s == Set::EMPTY || out.len() >= limit {
                continue;
            }
            let n = self.node(s);
            if n.bit == 0 {
                out.push(n.prefix);
            } else {
                stack.push(n.one);
                stack.push(n.zero);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Set, Sets};
    use hd_base::SliceSort as _;

    fn keys(sets: &Sets, s: Set) -> Vec<u32> {
        let mut out = Vec::new();
        sets.members(s, &mut out);
        out
    }

    #[test]
    fn a_set_of_sorted_keys_is_the_set_their_inserts_make() {
        let mut sets = Sets::default();
        for ks in [
            &[][..],
            &[7],
            &[0, 1, 2, 3, 4],
            &[3, 9, 10, 64, 65, 1000, u32::MAX - 1],
        ] {
            let bulk = sets.of_sorted(ks);
            let one_by_one = ks.iter().fold(Set::EMPTY, |s, &k| sets.insert(s, k));
            assert_eq!(bulk, one_by_one, "{ks:?}");
            let mut got = keys(&sets, bulk);
            got.sort_stable();
            assert_eq!(got, ks.to_vec());
        }
        let mut fresh = Sets::default();
        let all: Vec<u32> = (100..1100).collect();
        fresh.of_sorted(&all);
        assert_eq!(fresh.made(), 2 * all.len() - 1, "each node once");
        assert_eq!(fresh.work(), fresh.made(), "each asked for once");
    }

    #[test]
    fn an_unconsed_arena_builds_the_same_keys_with_no_table() {
        let mut sets = Sets::unconsed();
        let a = sets.of_sorted(&[1, 4, 9, 30]);
        let b = sets.of_sorted(&[1, 4, 9, 30]);
        assert_ne!(a, b, "two nodes of one content");
        let c = sets.insert(a, 12);
        let d = sets.remove(b, 4);
        let meet = sets.intersection(c, d);
        let mut got = keys(&sets, meet);
        got.sort_stable();
        assert_eq!(got, [1, 9, 30]);
        let equal = sets.intersection(a, b);
        assert_eq!(sets.len(equal), 4, "a meet of equals keeps their size");
        assert!(sets.made.is_empty(), "no node was hashed");
    }

    #[test]
    fn sets_insert_remove_and_join_like_sorted_lists() {
        let mut sets = Sets::default();
        // Keys that differ in low, high and middle bits, the top bit too.
        let all = [5u32, 0, 7, 1 << 31, 6, 64, 65, u32::MAX - 1, 3];
        let mut s = Set::EMPTY;
        for &k in &all {
            s = sets.insert(s, k);
            assert!(sets.contains(s, k));
        }
        let mut sorted = all.to_vec();
        sorted.sort_stable();
        assert_eq!(keys(&sets, s), sorted);
        assert_eq!(sets.len(s), all.len());
        // Inserting a member or removing a stranger makes nothing.
        let made = sets.made();
        assert_eq!(sets.insert(s, 64), s);
        assert_eq!(sets.remove(s, 2), s);
        assert_eq!(sets.remove(s, 1 << 30), s);
        assert_eq!(sets.made(), made);
        let t = sets.remove(s, 64);
        assert!(!sets.contains(t, 64) && sets.contains(s, 64));
        assert_eq!(sets.len(t), all.len() - 1);
        let mut u = Set::EMPTY;
        for k in [2u32, 64, 1 << 20, 7] {
            u = sets.insert(u, k);
        }
        let both = sets.union(t, u);
        let mut want: Vec<u32> = sorted.iter().copied().chain([2, 1 << 20]).collect();
        want.sort_stable();
        assert_eq!(keys(&sets, both), want);
        assert_eq!(sets.len(both), want.len());
        let other = sets.union(u, t);
        assert_eq!(keys(&sets, other), want);
        // A union with a subset or with itself is the larger set as it is.
        assert_eq!(sets.union(s, t), s);
        assert_eq!(sets.union(t, s), s);
        assert_eq!(sets.union(both, Set::EMPTY), both);
        assert_eq!(sets.union(Set::EMPTY, both), both);
        // Emptying a set one key at a time.
        let mut e = both;
        for k in want {
            e = sets.remove(e, k);
        }
        assert_eq!(e, Set::EMPTY);
    }

    #[test]
    fn sets_of_one_content_are_one_node_and_a_difference_stops_at_its_limit() {
        let mut sets = Sets::default();
        let (mut up, mut down) = (Set::EMPTY, Set::EMPTY);
        for k in 0..100u32 {
            up = sets.insert(up, k);
            down = sets.insert(down, 99 - k);
        }
        assert_eq!(up, down);
        let fewer = sets.remove(up, 50);
        let mut out = Vec::new();
        sets.difference_upto(up, fewer, &mut out, 0);
        assert!(out.is_empty());
        sets.difference(up, fewer, &mut out);
        assert_eq!(out, [50]);
        // Against a lone key, the rest of the other set, up to the limit.
        let one = sets.insert(Set::EMPTY, 3);
        out.clear();
        sets.difference_upto(up, one, &mut out, 5);
        assert_eq!(out, [0, 1, 2, 4, 5]);
        out.clear();
        sets.difference_upto(fewer, Set::EMPTY, &mut out, 2);
        assert_eq!(out, [0, 1]);
        // The work of a difference of sets one key apart is a path.
        let work = sets.work();
        out.clear();
        sets.difference(fewer, up, &mut out);
        assert!(out.is_empty() && sets.work() - work <= 2 * 32);
    }

    #[test]
    fn an_intersection_keeps_the_shared_keys_and_shares_what_agrees() {
        let mut sets = Sets::default();
        let mut base = Set::EMPTY;
        for k in 0..64u32 {
            base = sets.insert(base, k);
        }
        let a = sets.remove(base, 5);
        let a = sets.insert(a, 1 << 20);
        let b = sets.remove(base, 40);
        let both = sets.intersection(a, b);
        let want: Vec<u32> = (0..64).filter(|&k| k != 5 && k != 40).collect();
        assert_eq!(keys(&sets, both), want);
        assert_eq!(sets.intersection(b, a), both);
        // With itself, a subset, a stranger, a lone key, or nothing.
        assert_eq!(sets.intersection(a, a), a);
        assert_eq!(sets.intersection(base, b), b);
        let far = sets.insert(Set::EMPTY, 1 << 30);
        assert_eq!(sets.intersection(base, far), Set::EMPTY);
        let one = sets.insert(Set::EMPTY, 7);
        assert_eq!(sets.intersection(base, one), one);
        assert_eq!(sets.intersection(one, base), one);
        assert_eq!(sets.intersection(far, one), Set::EMPTY);
        assert_eq!(sets.intersection(Set::EMPTY, base), Set::EMPTY);
        // Disjoint halves under one branch, and a set under one side.
        let mut low = Set::EMPTY;
        for k in 0..8u32 {
            low = sets.insert(low, k);
        }
        let mut high = Set::EMPTY;
        for k in 32..40u32 {
            high = sets.insert(high, k);
        }
        assert_eq!(sets.intersection(low, high), Set::EMPTY);
        assert_eq!(sets.intersection(base, high), high);
        assert_eq!(sets.intersection(high, base), high);
    }

    #[test]
    fn a_union_of_two_sets_from_one_base_makes_only_their_differing_paths() {
        for n in [1_000u32, 10_000] {
            let mut sets = Sets::default();
            let mut base = Set::EMPTY;
            for k in 0..n {
                base = sets.insert(base, k);
            }
            let a = sets.remove(base, 3);
            let a = sets.insert(a, n + 1);
            let b = sets.remove(base, n / 2);
            let made = sets.made();
            // The table holds an index and a control byte per slot, not
            // a copy of each node: at most 8 slots per 7 entries, doubled
            // by the last growth.
            let table = sets.made.allocation_size();
            assert!(
                table <= made * 5 * 8 * 2 / 7 + 64,
                "{n}: {table} table bytes for {made} nodes"
            );
            let both = sets.union(a, b);
            assert_eq!(sets.len(both), n as usize + 1);
            // About two paths of `log2 n` nodes, not `n`.
            assert!(
                sets.made() - made <= 64,
                "{n}: {} nodes",
                sets.made() - made
            );
        }
    }
}
