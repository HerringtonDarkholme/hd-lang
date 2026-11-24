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
pub type Region = u32;

/// A loan, what a region may point into: a dense index the caller assigns
/// to each origin (a parameter slot's placeholder, a local, a hidden local
/// of `&e`, a top-level binding).
pub type Loan = u32;

fn ix(i: usize) -> u32 {
    u32::try_from(i).expect("a region index")
}

/// The region variables of one body: each key (a value or a local) owns
/// one variable per slot of its type, `slots(key)`, and [`fresh`] makes
/// variables no key owns (a call's lifetimes, a hub, `'static`).
///
/// [`fresh`]: RegionTable::fresh
#[derive(Default)]
pub struct RegionTable {
    keys: Vec<Range<Region>>,
    len: Region,
}

impl RegionTable {
    /// Gives the next key `slots` variables and returns the key.
    pub fn push(&mut self, slots: u32) -> u32 {
        let r = self.fresh(slots);
        self.keys.push(r);
        ix(self.keys.len() - 1)
    }

    /// `n` new variables that belong to no key.
    pub fn fresh(&mut self, n: u32) -> Range<Region> {
        let start = self.len;
        self.len += n;
        start..self.len
    }

    /// The variables of `key`, one per slot in the slot layout's order.
    #[must_use]
    pub fn slots(&self, key: u32) -> Range<Region> {
        self.keys[key as usize].clone()
    }

    /// The variable of slot `slot` of `key`.
    #[cfg_attr(
        not(feature = "testing"),
        expect(dead_code, reason = "only the unit tests read it")
    )]
    #[must_use]
    pub fn region(&self, key: u32, slot: u32) -> Region {
        let r = self.slots(key);
        assert!(slot < r.end - r.start, "slot {slot} of key {key}");
        r.start + slot
    }

    /// How many variables there are.
    #[must_use]
    #[cfg_attr(
        feature = "testing",
        expect(
            clippy::len_without_is_empty,
            reason = "no caller asks whether it is empty"
        )
    )]
    pub fn len(&self) -> u32 {
        self.len
    }
}

/// Why an edge exists. [`Category::Flow`] edges only propagate; every
/// other category is a checked edge, whose source's loans must all be
/// allowed in its target, and picks the diagnostic's message and code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
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
    /// An argument constrained by a deferred static trait proof.
    StaticProof,
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
pub struct Edge {
    pub(crate) from: Region,
    pub to: Region,
    pub inst: u32,
    pub cat: Category,
}

/// The constraint graph of one body: outlives edges and seed loans.
#[derive(Default)]
pub struct Constraints {
    edges: Vec<Edge>,
    seeds: Vec<(Region, Loan)>,
}

/// A checked edge whose source holds a loan its target does not allow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Violation {
    pub edge: Edge,
    /// The lowest offending loan.
    pub loan: Loan,
}

/// Every region's solved value: `comp` maps a region to its component.
pub struct Solution {
    comp: Vec<u32>,
    values: Vec<HybridSet>,
}

impl Constraints {
    /// Adds `from ⊑ to`.
    pub fn outlives(&mut self, from: Region, to: Region, inst: u32, cat: Category) {
        self.edges.push(Edge {
            from,
            to,
            inst,
            cat,
        });
    }

    /// Adds both `a ⊑ b` and `b ⊑ a`, as an invariant slot asks.
    pub fn equal(&mut self, a: Region, b: Region, inst: u32) {
        self.outlives(a, b, inst, Category::Flow);
        self.outlives(b, a, inst, Category::Flow);
    }

    /// Puts loan `l` in region `r`'s value.
    pub fn seed(&mut self, r: Region, l: Loan) {
        self.seeds.push((r, l));
    }

    /// Solves the graph over `regions` variables; checked edges propagate
    /// like the others. Linear in regions plus edges, plus each source
    /// set's size once per component it reaches by an edge; a set whose
    /// parts are too spread for a bitmap sorts them ([`Builder::finish`]).
    #[must_use]
    pub fn solve(&self, regions: u32) -> Solution {
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
    #[must_use]
    pub fn loans(&self, r: Region) -> hd_base::bitset::Iter<'_> {
        let set = &self.values[self.comp[r as usize] as usize];
        crate::work::loan_words(set.size());
        set.iter()
    }

    /// Whether region `r` may point into loan `l`.
    #[must_use]
    pub fn holds(&self, r: Region, l: Loan) -> bool {
        crate::work::loan_words(1);
        self.values[self.comp[r as usize] as usize].contains(l)
    }

    /// Whether `a` and `b` ended in one component, and so share a value.
    #[must_use]
    pub fn same(&self, a: Region, b: Region) -> bool {
        self.comp[a as usize] == self.comp[b as usize]
    }

    /// The component `r` ended in: one key for every region sharing its
    /// value.
    /// The words region `r`'s loan set takes (feature `testing`).
    #[cfg(feature = "testing")]
    #[must_use]
    pub fn words(&self, r: Region) -> usize {
        self.values[self.component(r) as usize].size()
    }

    /// The number of solved components (feature `testing`).
    #[cfg(feature = "testing")]
    #[must_use]
    pub fn components(&self) -> usize {
        self.values.len()
    }

    pub(crate) fn component(&self, r: Region) -> u32 {
        self.comp[r as usize]
    }

    /// The checked edges whose source holds a loan that `allowed` rejects
    /// for that edge, by instruction, then in edge order: the first is the
    /// one to report.
    pub fn violations(
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
