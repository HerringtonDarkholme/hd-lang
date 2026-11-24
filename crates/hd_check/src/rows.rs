//! Requirement rows in bodies (`crates/hd_check/DESIGN.md` (type checking) §5.2 to §5.6): the
//! available-keys stack, closure rows, row subsumption, and the least
//! solution of a callee's row parameters.

use hd_base::SliceSort as _;
use hd_diag::Code;
use hd_resolve::{FnSig, HeadKind, Src};
use hd_syntax::{NodeRef, SyntaxKind};
use hd_types::{RowData, RowId, RowParamRef, Ty, TyData};

use crate::body::{BodyCx, Ck, RowFrame};
use crate::results::{RowEdge, RowFacts};
use crate::trial::BodyUndo;

/// A source type argument's identity across checks of the same local
/// body. Keeping its variable in the closure header also permits context
/// after the declaration to solve it.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum RowOrigin {
    Argument {
        at: hd_base::NodeIdx,
        owner: hd_base::DefId,
        index: Option<usize>,
    },
    Requirement {
        at: hd_base::NodeIdx,
        key: usize,
        path: Vec<usize>,
    },
    Inherited(Ty),
}

pub(crate) struct RowSeed {
    encoded: Vec<RowId>,
    vars: Vec<RowSeedVar>,
    scopes: hd_base::HashMap<hd_base::DefId, usize>,
}

enum RowSeedVar {
    Persistent(Ty),
    Body(RowOrigin, hd_types::VarKind),
}

/// The strict supertrait keys requirement key `have` supplies beside
/// itself (`req.row.entail.super`): each supertrait instance its trait
/// reaches, with the arguments the supertrait lists bind along the path
/// from `have`'s. Its bindings are those the lists write and `have`'s own,
/// which may name a supertrait's associated type
/// (`req.key.binding.names`), so a key that binds a subset of them is the
/// one supplied (`req.key.binding.identity`). One trait reached
/// with two argument lists, as under `Both < Store[i32] & Store[string]`,
/// gives two keys; an instance reached twice, as in a diamond, gives one.
///
/// Cost: one visit per distinct instance reached and one substitution per
/// supertrait edge of those, once per key and run: the run's memo
/// ([`hd_types::solver::GlobalMemo::supertrait_keys`]) serves every later
/// ask, from any body. The resolver cuts every supertrait cycle
/// (`trait.super.acyclic`), so the walk ends without a depth limit.
pub(crate) fn supertrait_keys(
    cx: &BodyCx<'_>,
    pool: hd_types::Types<'_>,
    have: Ty,
) -> std::sync::Arc<[Ty]> {
    let TyData::TraitValue {
        def,
        args,
        bindings: listed,
    } = pool.get(have)
    else {
        return std::sync::Arc::from([]);
    };
    cx.global.supertrait_keys(have, || {
        let mut out = Vec::new();
        let mut seen = hd_base::HashSet::default();
        let mut stack = vec![(def, args)];
        while let Some((def, args)) = stack.pop() {
            let Some(hd_resolve::ItemData::Trait(t)) = cx.lookup.item(def).map(|i| &i.data) else {
                continue;
            };
            let known = pool.list_items(args);
            for s in t.supers.iter().rev() {
                crate::work::scan_work(1);
                // `Self` stays the trait's own parameter: no key names it.
                let s = pool.subst(*s, &|p: hd_types::ParamRef| {
                    (p.owner == def && p.index > 0)
                        .then(|| known.get(usize::from(p.index) - 1).copied())
                        .flatten()
                });
                let TyData::TraitValue {
                    def: d,
                    args: a,
                    mut bindings,
                } = pool.get(s)
                else {
                    continue;
                };
                for b in &listed {
                    if !bindings.iter().any(|x| x.0 == b.0) {
                        bindings.push(*b);
                    }
                }
                let s = pool.intern_ty(&TyData::TraitValue {
                    def: d,
                    args: a,
                    bindings,
                });
                if seen.insert(s) {
                    out.push(s);
                    stack.push((d, a));
                }
            }
        }
        out
    })
}

/// The whole-argument index of a key's strict superkeys, shared by all
/// bodies and lexical scopes. The builder runs only once per whole key.
pub(crate) fn supertrait_key_index(
    cx: &BodyCx<'_>,
    pool: hd_types::Types<'_>,
    have: Ty,
) -> std::sync::Arc<hd_types::solver::SupertraitKeyIndex> {
    cx.global.supertrait_key_index(have, pool, || {
        let mut index: hd_types::solver::SupertraitKeyIndex = hd_base::HashMap::default();
        for &key in supertrait_keys(cx, pool, have).iter() {
            if let TyData::TraitValue { def, args, .. } = pool.get(key) {
                crate::work::scan_work(1);
                index.entry((def, args)).or_default().push(key);
            }
        }
        index
    })
}

/// Whether `inst`, the listed key `have` itself or one of its
/// [`supertrait_keys`], is the needed key `want`: the same trait and
/// arguments (`req.row.entail.super.arguments`), and each binding `want`
/// writes fixed so by `inst` (`req.key.binding.identity`). The listed key
/// itself also fits a key that binds nothing.
#[must_use]
pub fn key_is(pool: hd_types::Types<'_>, have: Ty, inst: Ty, want: Ty) -> bool {
    if inst == want {
        return true;
    }
    if pool.trait_value(inst) != pool.trait_value(want) || pool.trait_value(inst).is_none() {
        return false;
    }
    let (Some(have_bindings), Some(mut want_bindings)) = (
        pool.trait_value_bindings(inst),
        pool.trait_value_bindings(want),
    ) else {
        return false;
    };
    if inst == have {
        return have_bindings.len() == 0
            || want_bindings.len() == 0
            || have_bindings.eq(want_bindings);
    }
    want_bindings.all(|(name, ty)| pool.trait_value_binding(inst, name) == Some(ty))
}

/// Whether the listed key `have` supplies the needed key `want`: the same
/// key (`req.row.entail.concrete`), or one of its supertrait keys
/// (`req.row.entail.super`).
pub(crate) fn key_supplies(cx: &BodyCx<'_>, pool: hd_types::Types<'_>, have: Ty, want: Ty) -> bool {
    if key_is(pool, have, have, want) {
        return true;
    }
    match (pool.get(have), pool.get(want)) {
        (TyData::TraitValue { def: d1, .. }, TyData::TraitValue { def: d2, .. }) if d1 != d2 => {
            supertrait_keys(cx, pool, have)
                .iter()
                .any(|&s| key_is(pool, have, s, want))
        }
        _ => false,
    }
}

/// The keys of a row, indexed for [`key_supplies`] questions: each key
/// with its [`supertrait_keys`], so asking which key supplies a needed key
/// compares keys of that trait only. A row of at most [`KeyIndex::SMALL`]
/// keys is scanned, as building the maps would cost more than the scan.
///
/// Cost: one memo ask per key to build, and for an indexed row one entry
/// per supertrait key; then one probe and the candidates' comparisons per
/// question, where asking each key would compare all its supertrait keys.
/// Ordered candidates, narrowed by the least-populated requested binding.
/// Like rustc's typed impl buckets, this filters before checking candidates.
#[derive(Default)]
pub struct KeyCandidates {
    values: Vec<(Ty, Ty)>,
    bindings: hd_base::HashMap<(hd_base::DefId, Ty), Vec<Ty>>,
    groups: hd_base::HashMap<Ty, (usize, Option<usize>)>,
    direct: Option<Ty>,
    plain: Option<usize>,
}

impl KeyCandidates {
    pub fn push(&mut self, pool: hd_types::Types<'_>, have: Ty, inst: Ty) {
        let at = self.values.len();
        self.values.push((have, inst));
        let Some(bindings) = pool.trait_value_bindings(inst) else {
            return;
        };
        if have == inst {
            self.direct.get_or_insert(have);
            if bindings.len() == 0 {
                self.plain.get_or_insert(at);
            }
        }
        if let Some(group) = self.groups.get_mut(&inst) {
            if have != inst {
                group.1.get_or_insert(at);
            }
            return;
        }
        self.groups.insert(inst, (at, (have != inst).then_some(at)));
        // A shared superkey's binding facts are indexed once, not per H.
        for binding in bindings {
            crate::work::scan_work(1);
            self.bindings.entry(binding).or_default().push(inst);
        }
    }

    #[must_use]
    pub fn supplier(&self, pool: hd_types::Types<'_>, want: Ty) -> Option<Ty> {
        if let Some(&(have, inst)) = self.values.first() {
            crate::work::scan_work(1);
            if key_is(pool, have, inst, want) {
                return Some(have);
            }
        }
        let bindings = pool.trait_value_bindings(want)?;
        if bindings.len() == 0 {
            return None;
        }
        let mut best = self.plain;
        let mut smallest: Option<&[Ty]> = None;
        for binding in bindings {
            crate::work::scan_work(1);
            let Some(keys) = self.bindings.get(&binding) else {
                return best.map(|at| self.values[at].0);
            };
            if smallest.is_none_or(|old| keys.len() < old.len()) {
                smallest = Some(keys);
            }
        }
        for &inst in smallest.unwrap_or(&[]) {
            let (first, projected) = self.groups[&inst];
            // Groups occur in first-value order; no later group can beat best.
            if best.is_some_and(|at| at <= first) {
                break;
            }
            let Some(at) = (inst == want).then_some(first).or(projected) else {
                continue;
            };
            let have = self.values[at].0;
            crate::work::scan_work(1);
            if key_is(pool, have, inst, want) && best.is_none_or(|old| at < old) {
                best = Some(at);
            }
        }
        best.map(|at| self.values[at].0)
    }
}

pub(crate) enum KeyIndex {
    Small(Vec<(Ty, std::sync::Arc<hd_types::solver::SupertraitKeyIndex>)>),
    Indexed {
        all: hd_base::HashSet<Ty>,
        /// Under each trait, the listed keys that are or supply a key of
        /// it, each with that key, in the row's order.
        by_trait: hd_base::HashMap<(hd_base::DefId, hd_types::TyList), KeyCandidates>,
    },
}

impl KeyIndex {
    /// The most keys a row has to be scanned instead of indexed.
    const SMALL: usize = 8;

    pub(crate) fn new(cx: &BodyCx<'_>, pool: hd_types::Types<'_>, keys: &[Ty]) -> Self {
        crate::work::scan_work(keys.len());
        if keys.len() <= Self::SMALL {
            return Self::Small(
                keys.iter()
                    .map(|&h| (h, supertrait_key_index(cx, pool, h)))
                    .collect(),
            );
        }
        let mut all = hd_base::HashSet::default();
        let mut by_trait: hd_base::HashMap<(hd_base::DefId, hd_types::TyList), KeyCandidates> =
            hd_base::HashMap::default();
        for &h in keys {
            all.insert(h);
            for &inst in std::iter::once(&h).chain(supertrait_keys(cx, pool, h).iter()) {
                if let TyData::TraitValue { def, args, .. } = pool.get(inst) {
                    crate::work::scan_work(1);
                    by_trait.entry((def, args)).or_default().push(pool, h, inst);
                }
            }
        }
        Self::Indexed { all, by_trait }
    }

    /// The first key, in the row's order, that supplies `want`.
    pub(crate) fn supplier(&self, pool: hd_types::Types<'_>, want: Ty) -> Option<Ty> {
        match self {
            Self::Small(keys) => keys
                .iter()
                .find(|(h, supers)| {
                    crate::work::scan_work(1);
                    key_is(pool, *h, *h, want)
                        || match pool.get(want) {
                            TyData::TraitValue { def, args, .. } => {
                                supers.get(&(def, args)).is_some_and(|keys| {
                                    keys.iter().any(|&s| {
                                        crate::work::scan_work(1);
                                        key_is(pool, *h, s, want)
                                    })
                                })
                            }
                            _ => false,
                        }
                })
                .map(|(h, _)| *h),
            Self::Indexed { all, by_trait } => match pool.get(want) {
                TyData::TraitValue { def, args, .. } => by_trait
                    .get(&(def, args))
                    .and_then(|held| held.supplier(pool, want)),
                _ => all.contains(&want).then_some(want),
            },
        }
    }

    /// A key explicitly listed for this trait instance, not supplied by
    /// another trait. Exact lexical bindings keep their identity even when
    /// a nearer scope binds a subtrait that could supply them.
    pub(crate) fn direct_supplier(&self, pool: hd_types::Types<'_>, want: Ty) -> Option<Ty> {
        match self {
            Self::Small(keys) => keys.iter().find_map(|(have, _)| {
                crate::work::scan_work(1);
                key_is(pool, *have, *have, want).then_some(*have)
            }),
            Self::Indexed { all, by_trait } => {
                if all.contains(&want) {
                    return Some(want);
                }
                let TyData::TraitValue { def, args, .. } = pool.get(want) else {
                    return None;
                };
                let held = by_trait.get(&(def, args))?;
                let bindings = pool.trait_value_bindings(want)?;
                if bindings.len() == 0 {
                    held.direct
                } else {
                    held.plain.map(|at| held.values[at].0)
                }
            }
        }
    }

    /// Whether some key supplies `want`.
    pub(crate) fn supplies(&self, pool: hd_types::Types<'_>, want: Ty) -> bool {
        self.supplier(pool, want).is_some()
    }
}

/// One requirement a body needs: a key or a row parameter.
#[derive(Clone, Copy)]
enum Need {
    Key(Ty),
    Param(RowParamRef),
}

impl Ck<'_, '_> {
    /// Reuse just the body variables that escaped into a recursive row.
    /// Ordinary checking, with no row trial open, adds no bookkeeping.
    pub(crate) fn row_argument_vars(&mut self, owner: hd_base::DefId, vars: &[Ty], n: NodeRef<'_>) {
        if self.row_banks.is_empty() {
            return;
        }
        for (index, &v) in vars.iter().enumerate() {
            self.row_argument_var(owner, Some(index), v, n);
        }
    }

    pub(crate) fn row_argument_self(&mut self, owner: hd_base::DefId, v: Ty, n: NodeRef<'_>) {
        self.row_argument_var(owner, None, v, n);
    }

    fn row_argument_var(
        &mut self,
        owner: hd_base::DefId,
        index: Option<usize>,
        v: Ty,
        n: NodeRef<'_>,
    ) {
        if self.row_banks.is_empty() {
            return;
        }
        let origin = RowOrigin::Argument {
            at: n.index(),
            owner,
            index,
        };
        self.row_source(v, origin);
    }

    fn row_source(&mut self, v: Ty, origin: RowOrigin) {
        let pool = self.pool();
        if let Some(&persistent) = self
            .row_banks
            .iter()
            .rev()
            .find_map(|bank| bank.get(&origin))
        {
            let snap = self.infer.snapshot();
            if self.infer.unify(pool, persistent, v).is_err() {
                self.infer.rollback(snap);
            }
        }
        self.row_sources.push((v, origin));
    }
    /// Whether the listed key `have` supplies the needed key `want`.
    fn key_supplies(&self, have: Ty, want: Ty) -> bool {
        key_supplies(self.cx, self.pool(), have, want)
    }

    /// `key_supplies`, solving type variables inside a generic key: the
    /// pattern `Repo[T]` against `Repo[User]` fixes `T`.
    fn key_fits(&mut self, have: Ty, want: Ty) -> bool {
        if self.key_supplies(have, want) {
            return true;
        }
        let pool = self.pool();
        let same_trait = matches!(
            (pool.get(have), pool.get(want)),
            (TyData::TraitValue { def: a, .. }, TyData::TraitValue { def: b, .. }) if a == b
        );
        if !same_trait || !(pool.has_infer(have) || pool.has_infer(want)) {
            return false;
        }
        let snap = self.infer.snapshot();
        if self.infer.unify(pool, have, want).is_ok() {
            return true;
        }
        self.infer.rollback(snap);
        false
    }

    /// Whether matching the rows' keys has two distinct solutions
    /// (`types.infer.ambiguous`): inference must not silently commit to
    /// one. Counts complete bijections by backtracking, each pair
    /// trial-unified and rolled back, deduplicated by the solutions they
    /// give the inference variables. Pure: callers commit afterwards.
    fn ambiguous_row_fit(&mut self, got_keys: &[Ty], want_keys: &[Ty]) -> bool {
        let vars = self.collect_infer(got_keys.iter().chain(want_keys).copied());
        if vars.is_empty() {
            return false;
        }
        let mut used = vec![false; want_keys.len()];
        let mut solutions = Vec::new();
        self.count_row_matchings(got_keys, want_keys, 0, &mut used, &vars, &mut solutions);
        solutions.len() >= 2
    }

    /// The inference variables mentioned by these types, in first-seen
    /// order. Visit each type once; a captured aggregate may share large
    /// type subtrees, and its variables need not be near the root.
    fn collect_infer(&self, roots: impl IntoIterator<Item = Ty>) -> Vec<Ty> {
        let pool = self.pool();
        let mut work: Vec<_> = roots.into_iter().collect();
        work.reverse();
        let mut seen = hd_base::HashSet::default();
        let mut vars = Vec::new();
        while let Some(t) = work.pop() {
            if !pool.has_infer(t) || !seen.insert(t) {
                continue;
            }
            if matches!(pool.get(t), TyData::Infer(_)) {
                vars.push(t);
            }
            let children = self.row_type_children(t);
            work.extend(children.into_iter().rev());
        }
        vars
    }

    fn row_type_children(&self, t: Ty) -> Vec<Ty> {
        let mut children = Vec::new();
        self.pool().children(t, &mut children);
        children
    }

    /// A row carried across a recursive closure body's rollback must
    /// retain variables from its header, captures and requirement uses.
    /// Union-find may have chosen a newer variable as their representative.
    /// Encode those representatives as parameters, resolve the
    /// row, then restore the original, persistent variables with subst.
    pub(crate) fn recursive_row_seeds(
        &mut self,
        rows: &[RowId],
        roots: &[Ty],
        sources: usize,
    ) -> RowSeed {
        let pool = self.pool();
        let rows: Vec<_> = rows
            .iter()
            .map(|&r| self.infer.resolve_row(pool, r))
            .collect();
        let vars = self.collect_infer(rows.iter().flat_map(|&r| pool.row_data(r).keys));
        if vars.is_empty() {
            return RowSeed {
                encoded: rows,
                vars: Vec::new(),
                scopes: hd_base::HashMap::default(),
            };
        }
        let persistent = self.collect_infer(roots.iter().copied());
        let mut anchors = hd_base::HashMap::default();
        for v in persistent {
            if let Some(class) = self.infer.class_of(pool, v) {
                anchors.entry(class).or_insert(v);
            }
        }
        let mut origins = hd_base::HashMap::default();
        for (v, origin) in &self.row_sources[sources..] {
            if let Some(class) = self.infer.class_of(pool, *v) {
                origins.entry(class).or_insert_with(|| origin.clone());
            }
        }
        let originals: Vec<RowSeedVar> = vars
            .iter()
            .map(|&v| {
                let class = self.infer.class_of(pool, v);
                if let Some(&anchor) = class.and_then(|class| anchors.get(&class)) {
                    RowSeedVar::Persistent(anchor)
                } else if let Some(kind) = self.infer.kind_of(pool, v) {
                    let origin = class
                        .and_then(|class| origins.get(&class))
                        .cloned()
                        .unwrap_or(RowOrigin::Inherited(v));
                    RowSeedVar::Body(origin, kind)
                } else {
                    RowSeedVar::Persistent(self.infer.resolve(pool, v))
                }
            })
            .collect();
        let mut owner = hd_base::DefId::NONE;
        let mut scopes = hd_base::HashMap::default();
        let snap = self.infer.snapshot();
        for ((i, &v), index) in vars.iter().enumerate().zip((0..=u16::MAX).cycle()) {
            // A parameter index is u16; additional parameter owners keep
            // that representation from limiting the number of row keys.
            if i > 0 && index == 0 {
                owner = self.cx.names.member(
                    self.gen_owner,
                    hd_intern::PathKind::Hidden,
                    &format!("$row-proxy-{}", scopes.len()),
                );
                scopes.insert(owner, i);
            }
            let p = hd_types::ParamRef { owner, index };
            self.infer
                .rebind(pool, v, pool.intern_ty(&TyData::Param(p)));
        }
        let encoded = rows
            .into_iter()
            .map(|r| self.infer.resolve_row(pool, r))
            .collect();
        self.infer.rollback(snap);
        RowSeed {
            encoded,
            vars: originals,
            scopes,
        }
    }

    /// Allocate promoted variables after rollback, before the next body
    /// checkpoint. Their source arguments meet the same variables on the
    /// next pass, even when no surrounding context has solved them yet.
    pub(crate) fn restore_row_seeds(&mut self, seed: RowSeed, bank: usize) -> Vec<RowId> {
        let pool = self.pool();
        let originals: Vec<_> = seed
            .vars
            .into_iter()
            .map(|v| match v {
                RowSeedVar::Persistent(t) => t,
                RowSeedVar::Body(origin, kind) => {
                    if let Some(&v) = self
                        .row_banks
                        .iter()
                        .rev()
                        .find_map(|bank| bank.get(&origin))
                    {
                        return v;
                    }
                    let v = self.infer.fresh(pool, kind);
                    self.row_banks[bank].insert(origin, v);
                    v
                }
            })
            .collect();
        seed.encoded
            .into_iter()
            .map(|row| {
                pool.subst_row(row, &|p| {
                    let base = if p.owner == hd_base::DefId::NONE {
                        0
                    } else {
                        *seed.scopes.get(&p.owner)?
                    };
                    originals.get(base + usize::from(p.index)).copied()
                })
            })
            .collect()
    }

    /// Counts distinct solutions for matching `got[index..]` against the
    /// unused `want` keys, stopping at two.
    fn count_row_matchings(
        &mut self,
        got: &[Ty],
        want: &[Ty],
        index: usize,
        used: &mut [bool],
        vars: &[Ty],
        solutions: &mut Vec<Vec<Ty>>,
    ) {
        if solutions.len() >= 2 {
            return;
        }
        if index == got.len() {
            let pool = self.pool();
            let solution: Vec<Ty> = vars.iter().map(|v| self.infer.resolve(pool, *v)).collect();
            if !solutions.contains(&solution) {
                solutions.push(solution);
            }
            return;
        }
        let pool = self.pool();
        for (j, w) in want.iter().enumerate() {
            if used[j] {
                continue;
            }
            let snap = self.infer.snapshot();
            if self.infer.unify(pool, got[index], *w).is_ok() {
                used[j] = true;
                self.count_row_matchings(got, want, index + 1, used, vars, solutions);
                used[j] = false;
            }
            self.infer.rollback(snap);
        }
    }

    /// Whether two requirement rows are the same row after resolution
    /// (`types.fn.same`): each lists only keys the other supplies, with the
    /// same row parameters. A row variable still open is a key that only
    /// itself supplies: [`Ck::solve_nested_rows`] solves the variables of
    /// a value's type before this compares.
    pub(crate) fn rows_equal(&self, a: RowId, b: RowId) -> bool {
        let pool = self.pool();
        let (a, b) = (
            self.infer.resolve_row(pool, a),
            self.infer.resolve_row(pool, b),
        );
        if a == b || self.local_row_open(a) || self.local_row_open(b) {
            return true;
        }
        let (da, db) = (pool.row_data(a), pool.row_data(b));
        let within = |x: &hd_types::RowData, y: &hd_types::RowData| {
            let have = KeyIndex::new(self.cx, pool, &y.keys);
            x.keys.iter().all(|k| have.supplies(pool, *k))
        };
        da.params == db.params && within(&da, &db) && within(&db, &da)
    }

    /// Solves the row variables of the function types that `got` and
    /// `want` hold inside type arguments, pairing them as `perm_fits`
    /// does. The row is invariant there (`types.variance.function`), so a
    /// variable takes exactly what the other row lists beyond the keys
    /// both name. A variable an earlier occurrence solved is already its
    /// row, and a disagreeing row stays unequal for `perm_fits` to report.
    /// A function type at the top keeps `fit_row`'s least solution.
    pub(crate) fn solve_nested_rows(&mut self, got: Ty, want: Ty, depth: u32) {
        let pool = self.pool();
        hd_base::stack::all((got, want, depth), |(got, want, depth), stack| {
            let (g, w) = (
                self.infer.resolve(pool, got),
                self.infer.resolve(pool, want),
            );
            if depth > hd_base::limits::TYPE_DEPTH || g == w {
                return true;
            }
            let d = depth + 1;
            let pairs = |stack: &mut Vec<_>, a: hd_types::TyList, b: hd_types::TyList| {
                let (a, b) = (pool.list_items(a), pool.list_items(b));
                stack.extend(a.iter().zip(b).rev().map(|(&x, &y)| (x, y, d)));
            };
            match (pool.get(g), pool.get(w)) {
                (
                    TyData::Adt(hd_types::AdtId::Product { .. }, _),
                    TyData::Adt(hd_types::AdtId::Product { .. }, _),
                ) if let (Some((a, _)), Some((b, _))) =
                    (pool.product_parts(g), pool.product_parts(w)) =>
                {
                    stack.extend(a.iter().zip(b).rev().map(|(&x, &y)| (x, y, d)));
                }
                (TyData::Adt(def, a), TyData::Adt(e, b)) if def == e => {
                    pairs(stack, a, b);
                }
                (TyData::TraitValue { args: a, .. }, TyData::TraitValue { args: b, .. }) => {
                    pairs(stack, a, b);
                }
                (
                    TyData::Fn {
                        params: a,
                        result: ar,
                        row: aw,
                        ..
                    },
                    TyData::Fn {
                        params: b,
                        result: br,
                        row: bw,
                        ..
                    },
                ) => {
                    stack.push((ar, br, d));
                    pairs(stack, a, b);
                    if depth > 0 {
                        self.solve_exact_row(aw, bw);
                    }
                }
                _ => return true,
            }
            true
        });
    }

    /// Makes the rows `a` and `b` one row by solving the variables in them.
    /// A variable in one row takes the keys and row parameters the other
    /// row lists that this row does not; a second variable in the same row
    /// takes the empty row. Two rows that are each one variable join.
    fn solve_exact_row(&mut self, a: RowId, b: RowId) {
        let pool = self.pool();
        let (a, b) = (
            self.infer.resolve_row(pool, a),
            self.infer.resolve_row(pool, b),
        );
        if a == b {
            return;
        }
        let (da, db) = (pool.row_data(a), pool.row_data(b));
        let is_var = |k: &Ty| matches!(pool.get(*k), TyData::Infer(_));
        let (va, vb): (Vec<Ty>, Vec<Ty>) = (
            da.keys.iter().copied().filter(is_var).collect(),
            db.keys.iter().copied().filter(is_var).collect(),
        );
        if va.is_empty() && vb.is_empty() {
            return;
        }
        let (ka, kb): (Vec<Ty>, Vec<Ty>) = (
            da.keys.iter().copied().filter(|k| !is_var(k)).collect(),
            db.keys.iter().copied().filter(|k| !is_var(k)).collect(),
        );
        // What each row lists that the other does not.
        let mut beyond_a = RowData::default();
        for k in &ka {
            if !kb.iter().any(|h| self.key_fits(*h, *k)) {
                beyond_a.keys.push(*k);
            }
        }
        beyond_a
            .params
            .extend(da.params.iter().copied().filter(|p| !db.params.contains(p)));
        let mut beyond_b = RowData::default();
        for k in &kb {
            if !ka.iter().any(|h| self.key_fits(*h, *k)) {
                beyond_b.keys.push(*k);
            }
        }
        beyond_b
            .params
            .extend(db.params.iter().copied().filter(|p| !da.params.contains(p)));
        let empty = pool.intern_ty(&TyData::Row(RowId::EMPTY));
        match (va.split_first(), vb.split_first()) {
            (Some((x, _)), Some((y, _)))
                if beyond_a.keys.is_empty()
                    && beyond_a.params.is_empty()
                    && beyond_b.keys.is_empty()
                    && beyond_b.params.is_empty() =>
            {
                let _ = self.infer.unify(pool, *x, *y);
            }
            (Some(_), Some(_)) => {}
            (None, Some((first, rest))) => {
                let sol = pool.intern_ty(&TyData::Row(pool.row(&beyond_a)));
                let _ = self.infer.unify(pool, *first, sol);
                for v in rest {
                    let _ = self.infer.unify(pool, *v, empty);
                }
            }
            (Some((first, rest)), None) => {
                let sol = pool.intern_ty(&TyData::Row(pool.row(&beyond_b)));
                let _ = self.infer.unify(pool, *first, sol);
                for v in rest {
                    let _ = self.infer.unify(pool, *v, empty);
                }
            }
            (None, None) => {}
        }
    }

    fn row_supplies(&self, row: RowId, need: Need) -> bool {
        match need {
            Need::Key(k) => self.row_key_supplier(row, k).is_some(),
            Need::Param(p) => self.pool().row_data(row).params.contains(&p),
        }
    }

    /// The first key of `row` that supplies `k`, from the body's index of
    /// the row's keys ([`KeyIndex`]), made on the row's first question.
    fn row_key_index(&self, row: RowId) -> std::rc::Rc<KeyIndex> {
        let pool = self.pool();
        let index = self.row_keys.borrow().get(&row).cloned();
        index.unwrap_or_else(|| {
            let built = std::rc::Rc::new(KeyIndex::new(self.cx, pool, &pool.row_data(row).keys));
            self.row_keys
                .borrow_mut()
                .insert(row, std::rc::Rc::clone(&built));
            built
        })
    }

    fn row_key_supplier(&self, row: RowId, k: Ty) -> Option<Ty> {
        self.row_key_index(row).supplier(self.pool(), k)
    }

    /// Freeze the whole supplier key while its lexical checking scopes are
    /// present. Inferred closures stop here: their own row must record `k`,
    /// rather than silently capture a provider from an enclosing function.
    pub(crate) fn selected_key_supplier(&self, key: Ty) -> Ty {
        let mut supplied = None;
        for frame in self.rows.iter().rev() {
            let (row, boundary) = match frame {
                RowFrame::With(row) => (*row, false),
                RowFrame::Declared(row)
                | RowFrame::Profile { row, .. }
                | RowFrame::Closure {
                    written: Some(row), ..
                } => (*row, true),
                RowFrame::Any
                | RowFrame::Inferred { .. }
                | RowFrame::Closure { written: None, .. } => return supplied.unwrap_or(key),
            };
            let index = self.row_key_index(row);
            if let Some(exact) = index.direct_supplier(self.pool(), key) {
                return exact;
            }
            if supplied.is_none() {
                supplied = index.supplier(self.pool(), key);
            }
            if boundary {
                break;
            }
        }
        supplied.unwrap_or(key)
    }

    /// Final checked evidence for function-row adaptation. All producers
    /// reserve `RowSubsume` while inference is open; one final pass chooses
    /// the allocating wrapper only when a source key needs projection.
    pub(crate) fn finalize_row_adaptations(&mut self) {
        use hd_tir::ir::{Coercion, Ref, Tag};
        use hd_types::TyList;
        let pool = self.pool();
        let mut recipes: hd_base::HashMap<(RowId, RowId), Option<TyList>> =
            hd_base::HashMap::default();
        for i in 0..self.b.body().len() {
            let body = self.b.body();
            if body.tags[i] != Tag::Coerce {
                continue;
            }
            let record = body.data[i][1] as usize + 1;
            if ![Coercion::RowSubsume as u32, Coercion::Variance as u32]
                .contains(&body.extra[record])
            {
                continue;
            }
            let source = Ref(body.data[i][0]);
            let got = source.as_inst().map_or_else(
                || body.consts[(source.0 & !Ref::CONST_BIT) as usize].0,
                |inst| body.ty[inst.0 as usize],
            );
            let (TyData::Fn { row: source, .. }, TyData::Fn { row: target, .. }) =
                (pool.get(got), pool.get(body.ty[i]))
            else {
                continue;
            };
            if source == target {
                continue;
            }
            let recipe = *recipes.entry((source, target)).or_insert_with(|| {
                let keys = pool.row_data(source).keys;
                let index = self.row_key_index(target);
                let mut pairs = Vec::with_capacity(keys.len() * 2);
                let mut projects = false;
                for key in keys {
                    // A failed fit already has type-mismatch; its recovery
                    // conversion must not manufacture unchecked evidence.
                    let supplier = index
                        .direct_supplier(pool, key)
                        .or_else(|| index.supplier(pool, key))?;
                    projects |= supplier != key;
                    pairs.extend([key, supplier]);
                }
                projects.then(|| pool.list(&pairs))
            });
            if let Some(recipe) = recipe {
                let body = self.b.body_mut();
                body.extra[record] = Coercion::RowAdapt as u32;
                body.extra[record + 1] = recipe.0;
            }
        }
    }

    /// Finds `need` in the available-keys stack (`req.row.entail`); a
    /// closure that infers its row takes the key into it instead.
    fn available(&mut self, need: Need, n: NodeRef<'_>) -> bool {
        for i in (0..self.rows.len()).rev() {
            let supplied = match &self.rows[i] {
                RowFrame::With(r) => {
                    if self.row_supplies(*r, need) {
                        return true;
                    }
                    continue;
                }
                RowFrame::Any => return true,
                RowFrame::Profile { row, .. } => {
                    let row = *row;
                    let have = match need {
                        Need::Key(k) => self.row_key_supplier(row, k),
                        Need::Param(_) => None,
                    };
                    if let (Some(h), RowFrame::Profile { used, .. }) = (have, &mut self.rows[i]) {
                        used.keys.push(h);
                        self.log_undo(BodyUndo::RowKey(i));
                    }
                    have.is_some()
                }
                RowFrame::Declared(r)
                | RowFrame::Closure {
                    written: Some(r), ..
                } => self.row_supplies(*r, need),
                RowFrame::Closure { written: None, .. } | RowFrame::Inferred { .. } => {
                    if let Need::Key(k) = need
                        && (!self.row_banks.is_empty() || crate::results::rechecking_rows(self.cx))
                        && crate::results::depth(self.pool(), k, 0) > crate::results::MAX_KEY_DEPTH
                    {
                        // A generic local call's instantiation is the
                        // callee expression, as in inst_cycles' TIR edge.
                        // Use that same site so it retains this one report.
                        let at = if n.kind() == SyntaxKind::CallExpr {
                            n.children()
                                .next()
                                .map(crate::elem_refs::strip_parens)
                                .filter(|callee| {
                                    callee.kind() == SyntaxKind::TypeArgsExpr
                                        && callee.children().next().is_some_and(|base| {
                                            base.kind() == SyntaxKind::NameExpr
                                                && self.find_local(self.sym_of(base)).is_some_and(
                                                    |(l, _)| self.local_generics.contains_key(&l),
                                                )
                                        })
                                })
                                .unwrap_or(n)
                        } else {
                            n
                        };
                        self.err(
                            Code::InstantiationTooDeep,
                            at,
                            "this recursive call grows an inferred requirement key past the instantiation depth limit; write the function's `$` clause",
                        );
                        return true;
                    }
                    if let RowFrame::Closure { used, .. } | RowFrame::Inferred { used, .. } =
                        &mut self.rows[i]
                    {
                        let undo = match need {
                            Need::Key(k) => {
                                used.keys.push(k);
                                BodyUndo::RowKey(i)
                            }
                            Need::Param(p) => {
                                used.params.push(p);
                                BodyUndo::RowParam(i)
                            }
                        };
                        self.log_undo(undo);
                    }
                    true
                }
            };
            return supplied;
        }
        false
    }

    /// A requirement the body cannot name. In a default body, whose row is
    /// empty, it is `requirement-in-default`, once for the default
    /// (`data.default.requirement-free`, `fn.default.requirement-free`).
    fn missing_requirement(&mut self, n: NodeRef<'_>, msg: &str) {
        match self.default_row {
            Some(true) => {}
            Some(false) => {
                self.default_row = Some(true);
                self.err(
                    Code::RequirementInDefault,
                    n,
                    "a default cannot use a provider",
                );
            }
            None => self.err(Code::MissingRequirement, n, msg),
        }
    }

    /// A key the body uses must be available (`req.row.set.call`):
    /// `missing-requirement` otherwise.
    pub(crate) fn require_key(&mut self, key: Ty, n: NodeRef<'_>, source: NodeRef<'_>) {
        self.require_key_at(key, n, source, 0);
    }

    fn require_key_at(&mut self, key: Ty, n: NodeRef<'_>, source: NodeRef<'_>, slot: usize) {
        // A key is known where it is required: an open literal class in it
        // takes its default type here, as a method receiver's does
        // (`types.literal.local.method`).
        let key = self.settle_literals_in(key);
        if !self.row_banks.is_empty() {
            let pool = self.pool();
            let mut work = vec![(self.infer.resolve(pool, key), Vec::new())];
            let mut seen = hd_base::HashSet::default();
            while let Some((t, path)) = work.pop() {
                if !pool.has_infer(t) || !seen.insert(t) {
                    continue;
                }
                if matches!(pool.get(t), TyData::Infer(_)) {
                    self.row_source(
                        t,
                        RowOrigin::Requirement {
                            at: source.index(),
                            key: slot,
                            path,
                        },
                    );
                } else {
                    for (index, child) in self.row_type_children(t).into_iter().enumerate().rev() {
                        let mut path = path.clone();
                        path.push(index);
                        work.push((child, path));
                    }
                }
            }
        }
        if !self.available(Need::Key(key), n) {
            let shown = hd_resolve::show_ty_in(&self.cx.names, self.pool(), key);
            // `module.testing.unit-row.test-runner`,
            // `module.testing.integration-row.unbound.error`.
            let msg = if matches!(self.rows.first(), Some(RowFrame::Profile { .. })) {
                format!(
                    "this needs `$ {shown}`, which the runner does not bind for this test case: bind it with $.with({shown}=...)"
                )
            } else {
                let note = self.expanded_row_note().unwrap_or_default();
                format!(
                    "this needs `$ {shown}`, which the enclosing function's row does not name{note}"
                )
            };
            self.missing_requirement(n, &msg);
        }
    }

    /// When the function's own row decides a missing key and its header
    /// writes the row with an alias: the row as written and its expanded
    /// keys (`req.row.alias.diagnostics`,
    /// `req.row.alias.diagnostics.expanded`).
    fn expanded_row_note(&self) -> Option<String> {
        let decided = self
            .rows
            .iter()
            .rev()
            .find(|f| !matches!(f, RowFrame::With(_)))?;
        let RowFrame::Declared(row) = decided else {
            return None;
        };
        let written = self.cx.src.parse.tree.node(self.written_row?);
        let uses_alias = written
            .children()
            .filter(|c| c.kind() == SyntaxKind::NamedType)
            .any(|c| {
                self.resolve_path(&self.segments(c))
                    .is_some_and(|d| self.kind_of_item(d) == Some(HeadKind::Alias))
            });
        if !uses_alias {
            return None;
        }
        let span = self.cx.src.span(written);
        let text = self.cx.src.text.get(span.lo as usize..span.hi as usize)?;
        let d = self.pool().row_data(*row);
        let mut keys: Vec<String> = d
            .keys
            .iter()
            .map(|k| hd_resolve::show_ty_in(&self.cx.names, self.pool(), *k))
            .collect();
        keys.sort_stable();
        let mut params: Vec<String> = d
            .params
            .iter()
            .filter_map(|p| self.row_gens.iter().find(|(_, g)| g == p))
            .map(|(s, _)| self.cx.names.text(*s).to_owned())
            .collect();
        params.sort_stable();
        keys.extend(params);
        let expanded = if keys.is_empty() {
            "$()".to_owned()
        } else {
            format!("$ {}", keys.join(" + "))
        };
        Some(format!("; `{text}` is `{expanded}`"))
    }

    /// Every key and row parameter of a callee's row, as seen from this
    /// call, must be available here.
    pub(crate) fn check_row(&mut self, row: RowId, n: NodeRef<'_>) {
        let pool = self.pool();
        let row = self.infer.resolve_row(pool, row);
        let mut d = pool.row_data(row);
        // Diagnostics in content order (`crates/hd_sched/DESIGN.md` §6.5), not by the
        // order this run interned the keys.
        let mut keys: Vec<(String, Ty)> = d
            .keys
            .into_iter()
            .filter(|k| matches!(pool.get(*k), TyData::TraitValue { .. }))
            .map(|k| {
                (
                    hd_resolve::show_ty_full_in(&self.cx.names, self.pool(), k),
                    k,
                )
            })
            .collect();
        keys.sort_stable_by(|a, b| a.0.cmp(&b.0));
        for (slot, (_, k)) in keys.into_iter().enumerate() {
            self.require_key_at(k, n, n, slot);
        }
        let gens = self.row_gens.clone();
        let name_of = |p: &RowParamRef| {
            gens.iter()
                .find(|(_, g)| g == p)
                .map_or_else(String::new, |(s, _)| self.cx.names.text(*s).to_owned())
        };
        d.params.sort_stable_by_cached_key(&name_of);
        for p in d.params {
            if self.local_row_requirement(p, n) {
                continue;
            }
            // Only this body's own row parameters reach here substituted;
            // another owner's are a callee's that was not instantiated.
            if !self.row_gens.iter().any(|(_, g)| *g == p) {
                continue;
            }
            if !self.available(Need::Param(p), n) {
                let name = self
                    .row_gens
                    .iter()
                    .find(|(_, g)| *g == p)
                    .map_or_else(String::new, |(s, _)| self.cx.names.text(*s).to_owned());
                let msg = format!(
                    "this needs the row `$ {name}`, which the enclosing function's row does not name"
                );
                self.missing_requirement(n, &msg);
            }
        }
    }

    /// A function value of row `got` checked against an expected function
    /// type of row `want`. A row variable still open in `want` takes the
    /// least solution (`req.row.least.solution`); otherwise `want` must
    /// entail every part of `got` (`req.row.subsume`), else `type-mismatch`.
    pub(crate) fn fit_row(&mut self, got: RowId, want: RowId, n: NodeRef<'_>, what: &str) {
        let pool = self.pool();
        let got = self.infer.resolve_row(pool, got);
        let want = self.infer.resolve_row(pool, want);
        if got == want {
            return;
        }
        let (g, w) = (pool.row_data(got), pool.row_data(want));
        let open: Vec<Ty> = w
            .keys
            .iter()
            .copied()
            .filter(|k| matches!(pool.get(*k), TyData::Infer(_)))
            .collect();
        let concrete: Vec<Ty> = w
            .keys
            .iter()
            .copied()
            .filter(|k| !matches!(pool.get(*k), TyData::Infer(_)))
            .collect();
        if let Some((first, rest)) = open.split_first() {
            // The least solution: what `got` lists beyond the pattern.
            let mut beyond = Vec::new();
            for k in &g.keys {
                if !concrete.iter().any(|c| self.key_fits(*c, *k)) {
                    beyond.push(*k);
                }
            }
            let sol = RowData {
                keys: beyond,
                params: g
                    .params
                    .iter()
                    .copied()
                    .filter(|p| !w.params.contains(p))
                    .collect(),
            };
            let sol = pool.intern_ty(&TyData::Row(pool.row(&sol)));
            let _ = self.infer.unify(pool, *first, sol);
            let empty = pool.intern_ty(&TyData::Row(RowId::EMPTY));
            for v in rest {
                let _ = self.infer.unify(pool, *v, empty);
            }
            return;
        }
        // Discovery records the unknown part as an equation. Concrete
        // row subsumption is checked on the group's final traversal.
        if self.local_row_open(got) || self.local_row_open(want) {
            return;
        }
        let mut missing = Vec::new();
        // Inference with two solutions for the keys is `ambiguous-type`,
        // not a silent first match (`types.infer.ambiguous`). Only pure
        // key bijections count: no row variables, no row parameters, and
        // the same count, so subsumption and open rows keep their paths.
        let bare_var = |k: &Ty| matches!(pool.get(*k), TyData::Infer(_));
        if g.keys.len() == w.keys.len()
            && (g.keys.iter().any(|k| pool.has_infer(*k))
                || w.keys.iter().any(|k| pool.has_infer(*k)))
            && g.params.is_empty()
            && w.params.is_empty()
            && !g.keys.iter().any(bare_var)
            && !w.keys.iter().any(bare_var)
            && self.ambiguous_row_fit(&g.keys, &w.keys)
        {
            self.err(
                Code::AmbiguousType,
                n,
                "these requirement keys fit two ways; write the type arguments explicitly",
            );
            for v in self.collect_infer(g.keys.iter().chain(&w.keys).copied()) {
                self.infer
                    .rebind(pool, self.infer.shallow(pool, v), Ty::POISON);
            }
            return;
        }
        for k in &g.keys {
            if matches!(pool.get(*k), TyData::Infer(_)) {
                // An open row variable in the value's own row: the least
                // row that fits, the empty one.
                let empty = pool.intern_ty(&TyData::Row(RowId::EMPTY));
                let _ = self.infer.unify(pool, *k, empty);
                continue;
            }
            if !concrete.iter().any(|c| self.key_fits(*c, *k)) {
                missing.push(hd_resolve::show_ty_in(&self.cx.names, self.pool(), *k));
            }
        }
        for p in &g.params {
            if !w.params.contains(p) {
                let name = self.row_gens.iter().find(|(_, x)| x == p).map_or_else(
                    || "R".to_owned(),
                    |(s, _)| self.cx.names.text(*s).to_owned(),
                );
                missing.push(name);
            }
        }
        missing.sort_stable();
        if let Some(k) = missing.first() {
            let mut msg = format!(
                "in {what}: the function's row lists `{k}`, which the expected row does not"
            );
            if what == "result" {
                msg.push_str(
                    "; `(fn(...) -> T $ Row)` keeps a returned function's row inside its type",
                );
            }
            self.err(Code::TypeMismatch, n, &msg);
        }
    }

    /// The callee's row as seen from this call, its row parameters
    /// replaced by the call's row variables; a row variable that no
    /// argument constrained is the empty row (`req.row.least.examples`).
    pub(crate) fn call_row(
        &mut self,
        def: hd_base::DefId,
        sig: &FnSig,
        vars: &[Ty],
        skip: usize,
    ) -> RowId {
        let pool = self.pool();
        let empty = pool.intern_ty(&TyData::Row(RowId::EMPTY));
        for (i, g) in sig.generics.iter().enumerate() {
            if g.row
                && let Some(v) = vars.get(i + skip)
                && matches!(pool.get(self.infer.shallow(pool, *v)), TyData::Infer(_))
            {
                let _ = self.infer.unify(pool, *v, empty);
            }
        }
        let row = pool.subst_row(sig.row, &|p| {
            (p.owner == def)
                .then(|| vars.get(p.index as usize + skip).copied())
                .flatten()
        });
        self.infer.resolve_row(pool, row)
    }

    /// `req.row.least.ambiguous`: a parameter's row pattern may list at most
    /// one row parameter that no other parameter's pattern fixes.
    pub(crate) fn check_row_patterns(&mut self, sig: &FnSig, node: NodeRef<'_>) {
        let pool = self.pool();
        let patterns: Vec<Vec<RowParamRef>> = sig
            .params
            .iter()
            .map(|(_, t)| match pool.get(*t) {
                TyData::Fn { row, .. } => pool.row_data(row).params,
                _ => Vec::new(),
            })
            .collect();
        let mut fixed: Vec<RowParamRef> = Vec::new();
        loop {
            let before = fixed.len();
            for p in &patterns {
                let open: Vec<RowParamRef> =
                    p.iter().copied().filter(|r| !fixed.contains(r)).collect();
                if let [one] = open.as_slice() {
                    fixed.push(*one);
                }
            }
            if fixed.len() == before {
                break;
            }
        }
        let params: Vec<NodeRef<'_>> = Src::child(node, SyntaxKind::ParameterList)
            .map(|pl| {
                pl.children()
                    .filter(|c| c.kind() == SyntaxKind::Parameter)
                    .collect()
            })
            .unwrap_or_default();
        for (i, p) in patterns.iter().enumerate() {
            if p.iter().filter(|r| !fixed.contains(r)).count() >= 2 {
                let at = params.get(i).copied().unwrap_or(node);
                self.err(
                    Code::AmbiguousRowPattern,
                    at,
                    "this row pattern lists two row parameters that no other parameter fixes",
                );
            }
        }
    }

    /// An inherent method call's row: the method's row under the call's
    /// type arguments, the implementation's and the method's own, so a
    /// generic key keeps them (`req.row.entail.generic`).
    pub(crate) fn inherent_row(
        &mut self,
        method: hd_base::DefId,
        sig: &FnSig,
        (owner, owner_args): (hd_base::DefId, &[Ty]),
        vars: &[Ty],
        n: NodeRef<'_>,
    ) {
        let pool = self.pool();
        let row = self.call_row(method, sig, vars, 0);
        let row = pool.subst_row(row, &|p| {
            (p.owner == owner)
                .then(|| owner_args.get(p.index as usize).copied())
                .flatten()
        });
        let row = self.infer.resolve_row(pool, row);
        let args = vec![(method, vars.to_vec()), (owner, owner_args.to_vec())];
        self.callee_row(method, row, args, n);
    }

    /// The row a call of `callee` needs, `row` as its signature shows it
    /// with the call's type arguments `args` (by owner) applied. A written
    /// or solved row is checked here. A private callable's row that is
    /// still open (`crates/hd_tir/DESIGN.md` "M3: inferred rows") is recorded
    /// instead: an inferred row takes an edge to it, and a written row
    /// checks it once it is solved, each without the keys of the `$.with`
    /// blocks around the call. A closure or a test case needs the row now,
    /// so the callee's body is checked first.
    pub(crate) fn callee_row(
        &mut self,
        callee: hd_base::DefId,
        row: RowId,
        args: Vec<(hd_base::DefId, Vec<Ty>)>,
        n: NodeRef<'_>,
    ) {
        if !crate::results::row_open(self.cx, callee) {
            self.check_row(row, n);
            return;
        }
        let pool = self.pool();
        let mut minus = Vec::new();
        for i in (0..self.rows.len()).rev() {
            match &mut self.rows[i] {
                RowFrame::With(r) => minus.extend(pool.row_data(*r).keys),
                RowFrame::Any => return,
                RowFrame::Inferred { edges, .. } => {
                    edges.push(RowEdge {
                        callee,
                        args,
                        minus,
                        at: n.index(),
                    });
                    self.log_undo(BodyUndo::RowEdge(i));
                    return;
                }
                RowFrame::Declared(r) => {
                    let row = *r;
                    let edge = RowEdge {
                        callee,
                        args,
                        minus,
                        at: n.index(),
                    };
                    self.row_waits.push((edge, row));
                    return;
                }
                RowFrame::Closure { .. } | RowFrame::Profile { .. } => break,
            }
        }
        // This path has an omitted clause; its written row is empty.
        let solved = crate::results::row_now(self.cx, callee, RowId::EMPTY);
        let row = pool.subst_row(solved, &|p| {
            args.iter()
                .find(|(o, _)| *o == p.owner)
                .and_then(|(_, a)| a.get(p.index as usize).copied())
        });
        let row = self.infer.resolve_row(pool, row);
        self.check_row(row, n);
    }

    /// The body's row facts at its end, with their types resolved: the
    /// keys and row parameters an inferred row used, and its edges.
    pub(crate) fn row_facts(&mut self) -> RowFacts {
        let pool = self.pool();
        let (used, edges) = match self.rows.first_mut() {
            Some(RowFrame::Inferred { used, edges }) => {
                (std::mem::take(used), std::mem::take(edges))
            }
            _ => Default::default(),
        };
        let mut keys = Vec::new();
        for k in used.keys {
            let k = self.zonk(k);
            if !pool.has_poison(k) && !keys.contains(&k) {
                keys.push(k);
            }
        }
        let mut params = used.params;
        params.sort_stable_by_key(|p| (p.owner.raw(), p.index));
        params.dedup();
        let edges = edges.into_iter().map(|e| self.zonk_edge(e)).collect();
        RowFacts {
            used: hd_types::RowData { keys, params },
            edges,
            fuel_left: Some(self.fuel.left()),
        }
    }

    /// The written row's calls of open rows at the body's end, with their
    /// types resolved.
    pub(crate) fn row_waits(&mut self) -> Vec<(RowEdge, RowId)> {
        std::mem::take(&mut self.row_waits)
            .into_iter()
            .map(|(e, r)| (self.zonk_edge(e), r))
            .collect()
    }

    fn zonk_edge(&mut self, mut e: RowEdge) -> RowEdge {
        for (_, a) in &mut e.args {
            for t in a.iter_mut() {
                *t = self.zonk(*t);
            }
        }
        for t in &mut e.minus {
            *t = self.zonk(*t);
        }
        e
    }
}
