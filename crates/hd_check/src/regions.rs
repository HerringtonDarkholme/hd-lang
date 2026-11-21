//! The region table, the constraint graph and the one location-insensitive
//! solver of the lifetime check (`crates/hd_check/DESIGN.md` §15.2). Each
//! value and local gets one region variable per slot of its type, each
//! instruction adds outlives edges between them, and
//! [`Constraints::solve`] computes every region's loan set at once: an
//! iterative Tarjan pass collapses strongly connected components, then one
//! pass builds each component's loan set once, in topological order
//! (`ref.dep.origin.set`).

use hd_base::SliceSort as _;
use hd_base::bitset::{Builder, HybridSet};
use std::ops::Range;

/// A region variable: a dense index into one body's [`RegionTable`].
pub(crate) type Region = u32;

/// A loan, what a region may point into: a dense index the caller assigns
/// to each origin (a parameter slot's placeholder, a local, a hidden local
/// of `&e`, a top-level binding).
pub(crate) type Loan = u32;

fn ix(i: usize) -> u32 {
    u32::try_from(i).expect("a region index")
}

/// The region variables of one body: each key (a value or a local) owns
/// one variable per slot of its type, `slots(key)`, and [`fresh`] makes
/// variables no key owns (a call's lifetimes, a hub, `'static`).
///
/// [`fresh`]: RegionTable::fresh
#[derive(Default)]
pub(crate) struct RegionTable {
    keys: Vec<Range<Region>>,
    len: Region,
}

impl RegionTable {
    /// Gives the next key `slots` variables and returns the key.
    pub(crate) fn push(&mut self, slots: u32) -> u32 {
        let r = self.fresh(slots);
        self.keys.push(r);
        ix(self.keys.len() - 1)
    }

    /// `n` new variables that belong to no key.
    pub(crate) fn fresh(&mut self, n: u32) -> Range<Region> {
        let start = self.len;
        self.len += n;
        start..self.len
    }

    /// The variables of `key`, one per slot in the slot layout's order.
    pub(crate) fn slots(&self, key: u32) -> Range<Region> {
        self.keys[key as usize].clone()
    }

    /// The variable of slot `slot` of `key`.
    #[cfg_attr(not(test), expect(dead_code, reason = "only the unit tests read it"))]
    pub(crate) fn region(&self, key: u32, slot: u32) -> Region {
        let r = self.slots(key);
        assert!(slot < r.end - r.start, "slot {slot} of key {key}");
        r.start + slot
    }

    /// How many variables there are.
    pub(crate) fn len(&self) -> u32 {
        self.len
    }
}

/// Why an edge exists. [`Category::Flow`] edges only propagate; every
/// other category is a checked edge, whose source's loans must all be
/// allowed in its target, and picks the diagnostic's message and code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Category {
    /// A plain flow: a read, a store, a projection, a call argument.
    Flow,
    /// A returned value or body value into the result's universal regions.
    Result,
    /// A store into a universal slot of a place the caller owns.
    CallerPlace,
    /// A call argument passed at one of the body's own lifetimes, as a
    /// bound writes it, into that lifetime's universal region.
    Bound,
    /// The value of a `box`, into `'static`.
    Box,
    /// A provider's value, into `'static`.
    Provider,
    /// A host call's argument, into `'static`.
    HostCall,
    /// A captured local into a struct field's capture slot.
    FieldCapture,
    /// What a suspension holds into a destination with no position for it,
    /// into `'static` (`ref.dep.suspension.local`).
    Held,
    /// An argument of a `'static` type parameter.
    StaticParam,
    /// A successful trait proof's deferred type-outlives requirement.
    Outlives,
    /// A value where a static reference is expected, into `'static`
    /// (`ref.static.source.error`).
    Static,
    /// A captured function value passed for a type parameter with no
    /// position for captures, which may take only the parameters'.
    Forward,
}

/// One constraint `from ⊑ to`: everything `from` may point into, `to` may
/// too. `inst` is the TIR instruction that added it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Edge {
    pub(crate) from: Region,
    pub(crate) to: Region,
    pub(crate) inst: u32,
    pub(crate) cat: Category,
}

/// The constraint graph of one body: outlives edges and seed loans.
#[derive(Default)]
pub(crate) struct Constraints {
    edges: Vec<Edge>,
    seeds: Vec<(Region, Loan)>,
}

/// A checked edge whose source holds a loan its target does not allow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Violation {
    pub(crate) edge: Edge,
    /// The lowest offending loan.
    pub(crate) loan: Loan,
}

/// Every region's solved value: `comp` maps a region to its component.
pub(crate) struct Solution {
    comp: Vec<u32>,
    values: Vec<HybridSet>,
}

impl Constraints {
    /// Adds `from ⊑ to`.
    pub(crate) fn outlives(&mut self, from: Region, to: Region, inst: u32, cat: Category) {
        self.edges.push(Edge {
            from,
            to,
            inst,
            cat,
        });
    }

    /// Adds both `a ⊑ b` and `b ⊑ a`, as an invariant slot asks.
    pub(crate) fn equal(&mut self, a: Region, b: Region, inst: u32) {
        self.outlives(a, b, inst, Category::Flow);
        self.outlives(b, a, inst, Category::Flow);
    }

    /// Puts loan `l` in region `r`'s value.
    pub(crate) fn seed(&mut self, r: Region, l: Loan) {
        self.seeds.push((r, l));
    }

    /// Solves the graph over `regions` variables; checked edges propagate
    /// like the others. Linear in regions plus edges, plus each source
    /// set's size once per component it reaches by an edge; a set whose
    /// parts are too spread for a bitmap sorts them ([`Builder::finish`]).
    pub(crate) fn solve(&self, regions: u32) -> Solution {
        let n = regions as usize;
        // Out-edges per region, in edge order (compressed rows).
        let mut off = vec![0u32; n + 1];
        for e in &self.edges {
            off[e.from as usize + 1] += 1;
        }
        for i in 0..n {
            off[i + 1] += off[i];
        }
        let mut adj = vec![0; self.edges.len()];
        let mut at = off.clone();
        for e in &self.edges {
            adj[at[e.from as usize] as usize] = e.to;
            at[e.from as usize] += 1;
        }
        let sccs = hd_base::scc::sccs(n, |v, out| {
            out.extend_from_slice(&adj[off[v as usize] as usize..off[v as usize + 1] as usize]);
        });
        // Tarjan finishes a component after every component it reaches, so
        // the highest number is a source and every edge into a component
        // comes from a higher one: build each set once, from its seeds and
        // the finished sets of its sources, each source once.
        let mut parts: Vec<Builder> = (0..sccs.len()).map(|_| Builder::default()).collect();
        for &(r, l) in &self.seeds {
            parts[sccs.comp[r as usize] as usize].insert(l);
        }
        let mut from = vec![u32::MAX; sccs.len()];
        let mut values = vec![HybridSet::default(); sccs.len()];
        for c in (0..sccs.len()).rev() {
            let part = std::mem::take(&mut parts[c]);
            crate::work::loan_words(part.parts());
            let value = part.finish();
            for &r in sccs.members(c) {
                for &t in &adj[off[r as usize] as usize..off[r as usize + 1] as usize] {
                    let d = sccs.comp[t as usize] as usize;
                    if d != c && from[d] != ix(c) {
                        from[d] = ix(c);
                        crate::work::loan_words(value.size());
                        parts[d].union(&value);
                    }
                }
            }
            values[c] = value;
        }
        Solution {
            comp: sccs.comp,
            values,
        }
    }
}

impl Solution {
    /// The loans region `r` may point into, in increasing order: one read
    /// of its set's [`size`](HybridSet::size).
    pub(crate) fn loans(&self, r: Region) -> hd_base::bitset::Iter<'_> {
        let set = &self.values[self.comp[r as usize] as usize];
        crate::work::loan_words(set.size());
        set.iter()
    }

    /// Whether region `r` may point into loan `l`.
    pub(crate) fn holds(&self, r: Region, l: Loan) -> bool {
        crate::work::loan_words(1);
        self.values[self.comp[r as usize] as usize].contains(l)
    }

    /// Whether `a` and `b` ended in one component, and so share a value.
    pub(crate) fn same(&self, a: Region, b: Region) -> bool {
        self.comp[a as usize] == self.comp[b as usize]
    }

    /// The component `r` ended in: one key for every region sharing its
    /// value.
    pub(crate) fn component(&self, r: Region) -> u32 {
        self.comp[r as usize]
    }

    /// The checked edges whose source holds a loan that `allowed` rejects
    /// for that edge, by instruction, then in edge order: the first is the
    /// one to report.
    pub(crate) fn violations(
        &self,
        cons: &Constraints,
        mut allowed: impl FnMut(&Edge, Loan) -> bool,
    ) -> Vec<Violation> {
        let mut out: Vec<Violation> = cons
            .edges
            .iter()
            .filter(|e| e.cat != Category::Flow)
            .filter_map(|e| {
                let loan = self.loans(e.from).find(|&l| !allowed(e, l))?;
                Some(Violation { edge: *e, loan })
            })
            .collect();
        out.sort_stable_by_key(|v| v.edge.inst);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loans(s: &Solution, r: Region) -> Vec<Loan> {
        s.loans(r).collect()
    }

    fn flow(c: &mut Constraints, a: Region, b: Region) {
        c.outlives(a, b, 0, Category::Flow);
    }

    #[test]
    fn empty_graph() {
        let s = Constraints::default().solve(0);
        assert!(s.values.is_empty());
        let s = Constraints::default().solve(3);
        assert!((0..3).all(|r| s.loans(r).next().is_none() && !s.same(r, (r + 1) % 3)));
    }

    #[test]
    fn region_table_keys_and_fresh() {
        let mut t = RegionTable::default();
        let a = t.push(2);
        let hub = t.fresh(1);
        let b = t.push(0);
        let c = t.push(3);
        assert_eq!(
            (t.slots(a), hub, t.slots(b), t.slots(c)),
            (0..2, 2..3, 3..3, 3..6)
        );
        assert_eq!((t.region(c, 2), t.len()), (5, 6));
    }

    #[test]
    fn chain_reaches_the_end_only_forward() {
        let mut c = Constraints::default();
        for r in 0..4 {
            flow(&mut c, r, r + 1);
            c.seed(r, r);
        }
        let s = c.solve(5);
        assert_eq!(loans(&s, 4), [0, 1, 2, 3]);
        assert_eq!(loans(&s, 2), [0, 1, 2]);
        assert_eq!(loans(&s, 0), [0]);
    }

    #[test]
    fn edges_given_backwards_still_solve_in_one_pass() {
        // Edges listed sink first, so Tarjan visits the end of the chain first.
        let mut c = Constraints::default();
        for r in (0..4).rev() {
            flow(&mut c, r, r + 1);
        }
        c.seed(0, 7);
        let s = c.solve(5);
        assert!((0..5).all(|r| loans(&s, r) == [7]));
    }

    #[test]
    fn diamond_unions_both_arms() {
        // 0 -> 1 -> 3, 0 -> 2 -> 3: an `if` whose arms both flow to the result.
        let mut c = Constraints::default();
        for (a, b) in [(0, 1), (0, 2), (1, 3), (2, 3)] {
            flow(&mut c, a, b);
        }
        c.seed(0, 0);
        c.seed(1, 1);
        c.seed(2, 2);
        let s = c.solve(4);
        assert_eq!(loans(&s, 3), [0, 1, 2]);
        assert_eq!((loans(&s, 1), loans(&s, 2)), (vec![0, 1], vec![0, 2]));
    }

    #[test]
    fn cycle_collapses_into_one_value() {
        // 0 -> 1 -> 2 -> 0 with a tail 2 -> 3 and an entry 4 -> 1.
        let mut c = Constraints::default();
        for (a, b) in [(0, 1), (1, 2), (2, 0), (2, 3), (4, 1)] {
            flow(&mut c, a, b);
        }
        c.seed(0, 0);
        c.seed(4, 4);
        c.seed(3, 3);
        let s = c.solve(5);
        assert!(s.same(0, 1) && s.same(1, 2) && !s.same(2, 3) && !s.same(4, 0));
        assert!((0..3).all(|r| loans(&s, r) == [0, 4]));
        assert_eq!((loans(&s, 3), loans(&s, 4)), (vec![0, 3, 4], vec![4]));
    }

    #[test]
    fn equal_and_self_loops() {
        let mut c = Constraints::default();
        flow(&mut c, 0, 0);
        c.equal(1, 2, 0);
        c.outlives(2, 2, 0, Category::Result);
        c.seed(0, 1);
        c.seed(2, 2);
        let s = c.solve(3);
        assert!(s.same(1, 2) && !s.same(0, 1));
        assert_eq!((loans(&s, 0), loans(&s, 1)), (vec![1], vec![2]));
    }

    #[test]
    fn loan_sets_cost_their_members() {
        let mut c = Constraints::default();
        flow(&mut c, 0, 2);
        flow(&mut c, 1, 2);
        for l in [0, 63, 64, 200] {
            c.seed(0, l);
        }
        c.seed(1, 130);
        c.seed(1, 100_000);
        let s = c.solve(3);
        assert_eq!(loans(&s, 2), [0, 63, 64, 130, 200, 100_000]);
        assert!(s.holds(2, 200) && !s.holds(2, 65));
        assert!(!s.holds(1, 1000) && s.holds(1, 100_000));
        // A far loan costs half a word, not the 1,562 words below it.
        let size = |r: Region| s.values[s.component(r) as usize].size();
        assert_eq!((size(0), size(1), size(2)), (2, 1, 3));
    }

    #[test]
    fn deep_chain_needs_no_recursion() {
        let n = 100_000;
        let mut c = Constraints::default();
        for r in 0..n - 1 {
            flow(&mut c, r, r + 1);
        }
        flow(&mut c, n - 1, 0);
        c.seed(n / 2, 5);
        let s = c.solve(n);
        assert!(s.same(0, n - 1) && loans(&s, 0) == [5]);
    }

    #[test]
    fn checked_edge_reports_the_first_instruction() {
        // Loans 0 and 1 are placeholders of the universal region 5, loan 2 a
        // local. Local 2 reaches region 3 at instruction 4; a return at 9 and
        // a store into a caller's place at 7 both carry it on.
        let mut c = Constraints::default();
        c.seed(0, 0);
        c.seed(1, 2);
        c.seed(5, 1);
        c.outlives(0, 3, 2, Category::Flow);
        c.outlives(1, 3, 4, Category::Flow);
        c.outlives(3, 5, 9, Category::Result);
        c.outlives(3, 5, 7, Category::CallerPlace);
        c.outlives(0, 5, 1, Category::Result);
        let s = c.solve(6);
        let allowed = |e: &Edge, l: Loan| e.to == 5 && l < 2;
        let v = s.violations(&c, allowed);
        let got: Vec<_> = v
            .iter()
            .map(|v| (v.edge.inst, v.edge.cat, v.loan))
            .collect();
        assert_eq!(
            got,
            [(7, Category::CallerPlace, 2), (9, Category::Result, 2)]
        );
        // Checked edges propagate too, and a flow edge is never reported.
        assert_eq!(loans(&s, 5), [0, 1, 2]);
        assert!(s.violations(&c, |_, l| l != 1).is_empty());
    }
}
