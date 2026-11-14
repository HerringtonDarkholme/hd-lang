//! A module's block-local declarations, indexed for the lookups that name
//! resolution and checking make at each use (#1878).
//!
//! A local declaration is visible from its first byte to the end of its
//! suite, so the declarations of one name visible at a byte are those
//! whose extent holds it, and the one a use finds is the innermost: the
//! latest start. The extents of one name are cut once into segments that
//! each have one innermost owner, so a lookup is a binary search over that
//! name's segments, not a filter over every local of the module.

use hd_base::SliceSort as _;
use hd_base::{DefId, HashMap, NodeIdx, Symbol};

use crate::HeadKind;
use crate::lower::LocalItem;

/// For a set of extents `[lo, hi)`, the innermost one holding each byte:
/// the one with the latest start, the last of those listed on a tie.
#[derive(Clone, Debug, Default)]
pub struct Innermost {
    /// Where each segment starts, and its owner's index, or `NONE` where no
    /// extent holds the bytes. Starts increase.
    segments: Vec<(u32, usize)>,
}

impl Innermost {
    const NONE: usize = usize::MAX;

    /// Cuts `extents` (index, `[lo, hi)`) into segments: O(k log k) for k
    /// extents, by a sweep over their ends with a heap of the open ones.
    #[must_use]
    pub fn new(extents: &[(usize, u32, u32)]) -> Innermost {
        let mut ends: Vec<u32> = extents.iter().flat_map(|&(_, lo, hi)| [lo, hi]).collect();
        ends.sort_stable();
        ends.dedup();
        let mut order: Vec<usize> = (0..extents.len()).collect();
        order.sort_stable_by_key(|&i| extents[i].1);
        let mut order = order.into_iter().peekable();
        // The open extents by (start, index); an ended one leaves when it
        // comes to the top.
        let mut open = std::collections::BinaryHeap::new();
        let mut segments: Vec<(u32, usize)> = Vec::new();
        for at in ends {
            while let Some(&i) = order.peek()
                && extents[i].1 <= at
            {
                let (idx, lo, hi) = extents[i];
                if lo < hi {
                    open.push((lo, i, idx, hi));
                }
                order.next();
            }
            while open.peek().is_some_and(|&(_, _, _, hi)| hi <= at) {
                open.pop();
            }
            let owner = open.peek().map_or(Self::NONE, |&(_, _, idx, _)| idx);
            if segments.last().is_none_or(|&(_, o)| o != owner) {
                segments.push((at, owner));
            }
        }
        Innermost { segments }
    }

    /// The index of the innermost extent holding byte `at`.
    #[must_use]
    pub fn at(&self, at: u32) -> Option<usize> {
        let i = self.segments.partition_point(|&(lo, _)| lo <= at);
        let (_, owner) = *self.segments.get(i.checked_sub(1)?)?;
        (owner != Self::NONE).then_some(owner)
    }
}

/// The block-local declarations of a module, as name lookup and the
/// checker ask for them.
#[derive(Clone, Debug, Default)]
pub struct Locals {
    items: Vec<LocalItem>,
    /// By name, the types and traits (not impls) that a use can find.
    by_name: HashMap<Symbol, Innermost>,
    /// The first declaration starting at each byte.
    by_lo: HashMap<u32, usize>,
    /// Each declaration by its item.
    by_def: HashMap<DefId, usize>,
    /// For each suite and name, the first type or trait of that name
    /// declared in it.
    first_in_suite: HashMap<(NodeIdx, Symbol), usize>,
    /// The local impls' extents, which decide the impls a byte sees.
    impls: HashMap<DefId, (u32, u32)>,
    /// The local impls' starts and ends, sorted: between two of them the
    /// set of impls seen does not change.
    impl_ends: Vec<u32>,
}

impl Locals {
    /// Indexes `items`, in declaration order.
    #[must_use]
    pub fn new(items: Vec<LocalItem>) -> Locals {
        let mut named: HashMap<Symbol, Vec<(usize, u32, u32)>> = HashMap::default();
        let mut by_lo = HashMap::default();
        let mut by_def = HashMap::default();
        let mut first_in_suite = HashMap::default();
        let mut impls = HashMap::default();
        let mut impl_ends = Vec::new();
        for (i, l) in items.iter().enumerate() {
            by_lo.entry(l.lo).or_insert(i);
            by_def.entry(l.def).or_insert(i);
            if l.kind == HeadKind::Impl {
                impls.insert(l.def, (l.lo, l.hi));
                impl_ends.extend([l.lo, l.hi]);
                continue;
            }
            named.entry(l.name).or_default().push((i, l.lo, l.hi));
            let first = first_in_suite.entry((l.suite, l.name)).or_insert(i);
            if l.lo < items[*first].lo {
                *first = i;
            }
        }
        impl_ends.sort_stable();
        impl_ends.dedup();
        let by_name = named
            .into_iter()
            .map(|(name, extents)| (name, Innermost::new(&extents)))
            .collect();
        Locals {
            items,
            by_name,
            by_lo,
            by_def,
            first_in_suite,
            impls,
            impl_ends,
        }
    }

    /// The local type or trait `name` visible at byte `at`: the innermost
    /// declaration whose extent holds `at` (`names.local-type.name`,
    /// `names.local-type.not-visible`). Impls have no name to find.
    #[must_use]
    pub fn at(&self, name: Symbol, at: u32) -> Option<&LocalItem> {
        crate::work::local_probe();
        let i = self.by_name.get(&name)?.at(at)?;
        self.items.get(i)
    }

    /// The first declaration starting at byte `lo`.
    #[must_use]
    pub fn starting_at(&self, lo: u32) -> Option<&LocalItem> {
        crate::work::local_probe();
        self.items.get(*self.by_lo.get(&lo)?)
    }

    /// Whether the local declaration `def` is visible at byte `at`; false
    /// for an item that is no local declaration.
    #[must_use]
    pub fn sees(&self, def: DefId, at: u32) -> bool {
        crate::work::local_probe();
        self.by_def
            .get(&def)
            .is_some_and(|&i| self.items[i].covers(at))
    }

    /// Whether a type or trait of `l`'s name is declared before `l` in its
    /// suite (`names.local-type.duplicate`).
    #[must_use]
    pub fn declared_earlier(&self, l: &LocalItem) -> bool {
        crate::work::local_probe();
        self.first_in_suite
            .get(&(l.suite, l.name))
            .is_some_and(|&i| self.items[i].lo < l.lo)
    }

    /// The declarations, in declaration order.
    #[must_use]
    pub fn items(&self) -> &[LocalItem] {
        &self.items
    }

    /// Whether the module declares a local impl.
    #[must_use]
    pub fn has_impls(&self) -> bool {
        !self.impls.is_empty()
    }

    /// The local impls' extents, by impl.
    #[must_use]
    pub fn impl_extents(&self) -> &HashMap<DefId, (u32, u32)> {
        &self.impls
    }

    /// Which stretch of the module byte `at` is in, between the starts and
    /// ends of local impls: two bytes of one stretch see the same impls.
    #[must_use]
    pub fn impl_stretch(&self, at: u32) -> usize {
        self.impl_ends.partition_point(|&e| e <= at)
    }
}

#[cfg(test)]
mod tests {
    use super::Innermost;

    #[test]
    fn the_innermost_extent_is_the_latest_start_holding_the_byte() {
        // 0 holds [10, 100); 1 and 2 nest in it, 2 ends where 3 starts;
        // 4 is empty and owns nothing.
        let m = Innermost::new(&[
            (0, 10, 100),
            (1, 20, 50),
            (2, 30, 40),
            (3, 40, 45),
            (4, 60, 60),
        ]);
        let at = |b| m.at(b);
        assert_eq!(at(9), None);
        assert_eq!(at(10), Some(0));
        assert_eq!(at(25), Some(1));
        assert_eq!(at(39), Some(2));
        assert_eq!(at(40), Some(3));
        assert_eq!(at(45), Some(1));
        assert_eq!(at(60), Some(0));
        assert_eq!(at(100), None);
        // A tie in start goes to the one listed last.
        let tie = Innermost::new(&[(7, 5, 9), (8, 5, 9)]);
        assert_eq!(tie.at(6), Some(8));
    }
}
