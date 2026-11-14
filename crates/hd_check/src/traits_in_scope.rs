//! The traits a method call can find, by method name (#1930,
//! `trait.avail.module`, `trait.avail.scope`, `trait.avail.prelude`).
//!
//! A trait is a candidate for a dot call or a type-qualified call when its
//! name is bound at the call: a block-local trait whose extent holds the
//! call, or a trait the module scope (or the `tests:` block's) binds. Both
//! are indexed by method name once per module, so a call asks only the
//! traits of its name visible from it, not every trait of the program that
//! declares the name.
//!
//! Local extents nest or are disjoint (each runs from a declaration to the
//! end of its suite), so the local traits of one method name form a forest:
//! the innermost extent holding a byte is one binary search
//! ([`hd_resolve::Innermost`]), and the others holding it are its
//! ancestors.

use std::sync::OnceLock;

use hd_base::SliceSort as _;
use hd_base::{DefId, HashMap, HashSet, Symbol};
use hd_resolve::{BindingKind, HeadKind, ItemData, Locals, Lookup, ModuleScope};

/// The local traits declaring one method name.
struct LocalDeclarers {
    /// (trait, method, start, end, the nearest enclosing entry), sorted by
    /// start.
    traits: Vec<(DefId, DefId, u32, u32, usize)>,
    innermost: hd_resolve::Innermost,
}

const NO_PARENT: usize = usize::MAX;

impl LocalDeclarers {
    /// O(k log k) for k traits: a sort, a sweep with a stack of the open
    /// extents for each one's parent, and the segments.
    fn new(mut traits: Vec<(DefId, DefId, u32, u32, usize)>) -> LocalDeclarers {
        traits.sort_stable_by_key(|&(_, _, lo, hi, _)| (lo, std::cmp::Reverse(hi)));
        let mut open: Vec<usize> = Vec::new();
        for i in 0..traits.len() {
            let lo = traits[i].2;
            while open.last().is_some_and(|&j| traits[j].3 <= lo) {
                open.pop();
            }
            traits[i].4 = open.last().copied().unwrap_or(NO_PARENT);
            open.push(i);
        }
        let extents: Vec<(usize, u32, u32)> = traits
            .iter()
            .enumerate()
            .map(|(i, &(_, _, lo, hi, _))| (i, lo, hi))
            .collect();
        let innermost = hd_resolve::Innermost::new(&extents);
        LocalDeclarers { traits, innermost }
    }

    /// The traits whose extent holds `at`, innermost first: one binary
    /// search, then one step per trait found.
    fn at(&self, at: u32, out: &mut Vec<(DefId, DefId)>) {
        let mut i = self.innermost.at(at).unwrap_or(NO_PARENT);
        while let Some(&(tr, m, lo, hi, parent)) = self.traits.get(i) {
            crate::work::trait_lookup(1);
            if lo <= at && at < hi {
                out.push((tr, m));
            }
            i = parent;
        }
    }
}

/// The traits a scope binds, by method name, and the items it binds.
#[derive(Default)]
struct ScopeTraits {
    by_method: HashMap<Symbol, Vec<(DefId, DefId)>>,
    bound: HashSet<DefId>,
}

impl ScopeTraits {
    /// O(b + m) for b bindings and the m methods of the traits bound.
    fn new(scope: &ModuleScope, lookup: &Lookup<'_>) -> ScopeTraits {
        let mut out = ScopeTraits::default();
        crate::work::trait_lookup(scope.binding.len() as u64);
        for b in &scope.binding {
            if b.kind != BindingKind::Item {
                continue;
            }
            let tr = DefId::from_raw(b.value);
            if !out.bound.insert(tr) {
                continue;
            }
            let Some(ItemData::Trait(t)) = lookup.item(tr).map(|i| &i.data) else {
                continue;
            };
            crate::work::trait_lookup(t.methods.len() as u64);
            for &(n, m) in &t.methods {
                out.by_method.entry(n).or_default().push((tr, m));
            }
        }
        out
    }
}

/// A module's traits by method name and where they are bound, made on the
/// first method call that asks, and shared by the module's forked
/// contexts.
#[derive(Default)]
pub struct TraitsInScope {
    local: OnceLock<HashMap<Symbol, LocalDeclarers>>,
    module: OnceLock<ScopeTraits>,
    tests: OnceLock<ScopeTraits>,
}

impl TraitsInScope {
    fn local(&self, locals: &Locals, lookup: &Lookup<'_>) -> &HashMap<Symbol, LocalDeclarers> {
        self.local.get_or_init(|| {
            let mut by_method: HashMap<Symbol, Vec<_>> = HashMap::default();
            for l in locals.items().iter().filter(|l| l.kind == HeadKind::Trait) {
                let Some(ItemData::Trait(t)) = lookup.item(l.def).map(|i| &i.data) else {
                    continue;
                };
                crate::work::trait_lookup(t.methods.len() as u64 + 1);
                for &(n, m) in &t.methods {
                    by_method
                        .entry(n)
                        .or_default()
                        .push((l.def, m, l.lo, l.hi, NO_PARENT));
                }
            }
            by_method
                .into_iter()
                .map(|(n, traits)| (n, LocalDeclarers::new(traits)))
                .collect()
        })
    }

    fn scope(&self, tests: bool, scope: &ModuleScope, lookup: &Lookup<'_>) -> &ScopeTraits {
        let cell = if tests { &self.tests } else { &self.module };
        cell.get_or_init(|| ScopeTraits::new(scope, lookup))
    }
}

impl crate::body::BodyCx<'_> {
    /// The scope at byte `at`, and whether it is the `tests:` block's.
    fn scope_traits(&self, at: u32) -> &ScopeTraits {
        let scope = self.scope_at(at);
        let tests = !std::ptr::eq(scope, self.scope);
        self.traits_in_scope.scope(tests, scope, self.lookup)
    }

    /// The traits declaring a method `sym` whose names are bound at byte
    /// `at`, with their method, in the method index's order: the local
    /// ones whose extent holds `at` and the ones the scope binds.
    pub(crate) fn traits_available(&self, sym: Symbol, at: u32) -> Vec<(DefId, DefId)> {
        let mut out = Vec::new();
        if let Some(l) = self
            .traits_in_scope
            .local(self.locals, self.lookup)
            .get(&sym)
        {
            l.at(at, &mut out);
        }
        if let Some(bound) = self.scope_traits(at).by_method.get(&sym) {
            crate::work::trait_lookup(bound.len() as u64);
            out.extend(bound);
        }
        // Only the traits the index holds are candidates, as before the
        // index by scope.
        let mut ranked: Vec<_> = out
            .into_iter()
            .filter_map(|(tr, m)| Some((self.methods.rank(tr)?, tr, m)))
            .collect();
        ranked.sort_stable_by_key(|&(rank, ..)| rank);
        ranked.into_iter().map(|(_, tr, m)| (tr, m)).collect()
    }

    /// Whether trait `tr`'s name is bound at byte `at`: two probes.
    pub(crate) fn trait_bound_at(&self, tr: DefId, at: u32) -> bool {
        self.locals.sees(tr, at) || self.scope_traits(at).bound.contains(&tr)
    }
}
