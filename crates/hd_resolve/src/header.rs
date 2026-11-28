//! The overlap check of coherence (§4.12.3, `crates/hd_check/DESIGN.md` (trait solver) §5.2), over
//! the impl heads of frozen interfaces. The header checks that need goals
//! are `hd_check::header`'s, asked through the solver.

use hd_base::HashMap;
use hd_base::SliceSort as _;
use hd_types::ANON_SUM;
use hd_types::AdtId;

use hd_base::{DefId, Hash128, StableHasher, StageResult};
use hd_types::{InternPool, ParamRef, Ty, TyData, TyList};

use crate::iface::{ImplKind, Item, ItemData, Names};

/// A head-index key of the overlap check: an outer constructor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HeadKey {
    Def(DefId),
    Trait(DefId),
    /// A constructor by number, and a tuple's arity.
    Ctor(u8, u32),
    /// A ground leaf type, such as a primitive.
    Ground(Ty),
}

/// The trait impls of a program graph, by trait.
pub struct Universe<'a> {
    pub names: Names<'a>,
    pub impls: HashMap<DefId, Vec<&'a Item>>,
}

impl<'a> Universe<'a> {
    #[must_use]
    pub fn new(names: Names<'a>, all: impl Iterator<Item = &'a Item>) -> Self {
        let mut impls: HashMap<DefId, Vec<&Item>> = HashMap::default();
        for it in all {
            if let ItemData::Impl { trait_, kind, .. } = it.data
                && trait_ != DefId::NONE
                && kind.is_impl()
            {
                impls.entry(trait_).or_default().push(it);
            }
        }
        Self { names, impls }
    }

    fn pool(&self) -> &InternPool {
        self.names.pool
    }

    #[must_use]
    pub fn show(&self, t: Ty) -> String {
        crate::iface::show_ty(&self.names, t)
    }

    /// The traits with an impl, in the order the overlap check reports
    /// them: by stable path hash.
    #[must_use]
    pub fn traits(&self) -> Vec<DefId> {
        let mut traits: Vec<DefId> = self.impls.keys().copied().collect();
        traits.sort_stable_by_key(|d| self.names.path_hash(*d));
        traits
    }

    /// The traits whose impls trait `tr`'s check reads besides its own: the
    /// bound of each numeric-family head ([`Self::family`]), sorted by
    /// stable path hash.
    #[must_use]
    pub fn family_traits(&self, tr: DefId) -> Vec<DefId> {
        let pool = self.pool();
        let mut out: Vec<DefId> = self
            .impls
            .get(&tr)
            .into_iter()
            .flatten()
            .filter_map(|imp| {
                let ItemData::Impl { self_ty, .. } = &imp.data else {
                    return None;
                };
                let TyData::Param(p) = pool.get(*self_ty) else {
                    return None;
                };
                if p.owner != imp.def {
                    return None;
                }
                imp.generics
                    .get(usize::from(p.index))?
                    .numeric_family(&self.names)
            })
            .collect();
        out.sort_stable_by_key(|d| self.names.path_hash(*d));
        out.dedup();
        out
    }

    /// The head hash of a group of impls (`crates/hd_cache/DESIGN.md`
    /// §5.3, `coh_key`): their whole records as an interface encodes them
    /// (parameters and their bounds, target, trait arguments, kind, stable
    /// path), each with its rank, the content order the check sorts it by.
    /// One encoding for the group costs less than one per impl.
    ///
    /// # Errors
    /// When a record holds a type no interface can encode.
    pub fn head_hash(&self, impls: &[&Item], ranks: &[String]) -> StageResult<Hash128> {
        let items: Vec<Item> = impls.iter().map(|i| (*i).clone()).collect();
        let bytes = crate::iface::encode_items(&self.names, &items, &[])?;
        let mut k = StableHasher::new("coh-head");
        k.bytes(&bytes);
        k.u32(u32::try_from(ranks.len()).unwrap_or(u32::MAX));
        for r in ranks {
            k.str(r);
        }
        Ok(k.finish())
    }

    /// `coh_key(T)` (`crates/hd_cache/DESIGN.md` §5.3): the stable path
    /// of `tr` and the sorted hashes `part` gives for it, then, for each of
    /// its [`Self::family_traits`], that trait's path and hashes. `part(t)`
    /// must cover every impl of `t`: the [`Self::head_hash`] of each, or
    /// of each group of them. These are every input
    /// [`Self::trait_overlaps`] reads, so two runs with one key report the
    /// same overlaps. `None` when `part` gives none.
    #[must_use]
    pub fn coh_key(
        &self,
        tr: DefId,
        part: &dyn Fn(DefId) -> Option<Vec<Hash128>>,
    ) -> Option<Hash128> {
        let mut k = StableHasher::new("coh");
        for t in std::iter::once(tr).chain(self.family_traits(tr)) {
            k.hash(self.names.path_hash(t));
            let mut hs = part(t)?;
            hs.sort_stable();
            k.u32(u32::try_from(hs.len()).unwrap_or(u32::MAX));
            for h in hs {
                k.hash(h);
            }
        }
        Some(k.finish())
    }

    /// The overlap check of trait `tr` alone (§4.12.3), and how many head
    /// pairs it unified. A head is compared only with the earlier heads
    /// whose target and first trait argument can match its own by their
    /// outer constructors (`head_key`), so many impls of one trait for
    /// distinct types cost time near-linear in their count; the result is
    /// the pairwise one. It reads the impls of `tr` and of its
    /// [`Self::family_traits`], nothing else.
    #[must_use]
    pub fn trait_overlaps(
        &self,
        tr: DefId,
        order: &dyn Fn(DefId) -> String,
    ) -> (Vec<(DefId, DefId, String)>, usize) {
        let pool = self.pool();
        let mut out = Vec::new();
        let mut tried = 0_usize;
        if let Some(impls) = self.impls.get(&tr) {
            let mut heads: Vec<(DefId, Ty, TyList)> = Vec::new();
            let mut list: Vec<&&Item> = impls.iter().collect();
            list.sort_stable_by_cached_key(|i| order(i.def));
            let mut reported = hd_base::HashSet::default();
            // A tuple template implements the trait for every tuple type,
            // so an implementation written for a tuple overlaps it
            // (`annot.template.tuple.overlap`).
            if let Some(template) = list.iter().find(|i| is_tuple_template(i)) {
                for imp in &list {
                    if let ItemData::Impl { self_ty, kind, .. } = &imp.data
                        && *kind != ImplKind::TupleTemplate
                        && matches!(pool.get(*self_ty), TyData::Adt(AdtId::Product { .. }, _))
                    {
                        out.push((template.def, imp.def, self.show(*self_ty)));
                        reported.insert(imp.def);
                    }
                }
            }
            if let Some(template) = list.iter().find(|i| is_union_template(i)) {
                for imp in &list {
                    if let ItemData::Impl { self_ty, kind, .. } = &imp.data
                        && *kind != ImplKind::UnionTemplate
                        && matches!(pool.get(*self_ty), TyData::Adt(ANON_SUM, _))
                    {
                        out.push((template.def, imp.def, self.show(*self_ty)));
                        reported.insert(imp.def);
                    }
                }
            }
            for imp in list {
                let ItemData::Impl {
                    self_ty,
                    trait_args,
                    kind,
                    ..
                } = &imp.data
                else {
                    continue;
                };
                if matches!(kind, ImplKind::TupleTemplate | ImplKind::UnionTemplate) {
                    continue;
                }
                match self.family(imp, *self_ty) {
                    Some((param, members)) => {
                        // One impl per member: the member replaces the
                        // parameter in the whole head.
                        for m in members {
                            let args: Vec<Ty> = pool
                                .list_items(*trait_args)
                                .iter()
                                .map(|&t| pool.subst(t, &|p: ParamRef| (p == param).then_some(m)))
                                .collect();
                            heads.push((imp.def, m, pool.list(&args)));
                        }
                    }
                    None => heads.push((imp.def, *self_ty, *trait_args)),
                }
            }
            // Earlier heads by the key of their target, and of their first
            // trait argument; a key of `None` matches every key.
            let keys: Vec<(Option<HeadKey>, Option<HeadKey>)> = heads
                .iter()
                .map(|h| {
                    let arg = pool.list_items(h.2).first().copied();
                    (self.head_key(h.1), arg.and_then(|t| self.head_key(t)))
                })
                .collect();
            let mut by_self: HashMap<HeadKey, Vec<usize>> = HashMap::default();
            let mut by_arg: HashMap<HeadKey, Vec<usize>> = HashMap::default();
            let (mut self_wild, mut arg_wild) = (Vec::new(), Vec::new());
            let fits =
                |x: Option<HeadKey>, y: Option<HeadKey>| x.is_none() || y.is_none() || x == y;
            for (j, b) in heads.iter().enumerate() {
                let (ks, ka) = keys[j];
                let mut cands: Vec<usize> = match (ks, ka) {
                    (Some(k), _) => by_self
                        .get(&k)
                        .into_iter()
                        .flatten()
                        .chain(&self_wild)
                        .copied()
                        .filter(|&i| fits(keys[i].1, ka))
                        .collect(),
                    (None, Some(k)) => by_arg
                        .get(&k)
                        .into_iter()
                        .flatten()
                        .chain(&arg_wild)
                        .copied()
                        .collect(),
                    (None, None) => (0..j).collect(),
                };
                cands.sort_stable();
                cands.dedup();
                match ks {
                    Some(k) => by_self.entry(k).or_default().push(j),
                    None => self_wild.push(j),
                }
                match ka {
                    Some(k) => by_arg.entry(k).or_default().push(j),
                    None => arg_wild.push(j),
                }
                for a in cands.into_iter().map(|i| &heads[i]) {
                    if a.0 == b.0 || reported.contains(&b.0) {
                        continue;
                    }
                    tried += 1;
                    let mut sub = HashMap::default();
                    if self.unify(a.1, b.1, a.0, b.0, &mut sub)
                        && self.unify_list(a.2, b.2, a.0, b.0, &mut sub)
                    {
                        let w =
                            pool.subst(b.1, &|p: ParamRef| sub.get(&(p.owner, p.index)).copied());
                        out.push((a.0, b.0, self.show(w)));
                        reported.insert(b.0);
                    }
                }
            }
        }
        (out, tried)
    }

    /// The outer constructor of `t` as a head-index key. `None` for a
    /// parameter, or any other type that may unify with many: such a head
    /// is compared with every earlier one.
    pub(crate) fn head_key(&self, t: Ty) -> Option<HeadKey> {
        let pool = self.pool();
        // `unify` matches outer constructors, so a key is the constructor
        // alone; a primitive is its own constructor.
        match pool.get(t) {
            TyData::Adt(AdtId::Declared(def), _) => Some(HeadKey::Def(def)),
            TyData::TraitValue { def, .. } => Some(HeadKey::Trait(def)),
            TyData::Ref {
                mutable, counted, ..
            } => Some(HeadKey::Ctor(
                4,
                u32::from(mutable) | u32::from(counted) << 1,
            )),
            TyData::Fn { .. } => Some(HeadKey::Ctor(2, 0)),
            TyData::Adt(AdtId::Product { rest: false }, elems) => Some(HeadKey::Ctor(
                3,
                u32::try_from(pool.list_items(elems).len()).unwrap_or(u32::MAX),
            )),
            TyData::Param(_) | TyData::Adt(AdtId::Product { .. }, _) => None,
            _ if !pool.has_param(t) => Some(HeadKey::Ground(t)),
            _ => None,
        }
    }

    /// A numeric-family head: `impl[N < B] Tr for N` where every impl of
    /// `B` has a ground target.
    fn family(&self, imp: &Item, self_ty: Ty) -> Option<(ParamRef, Vec<Ty>)> {
        let pool = self.pool();
        let TyData::Param(p) = pool.get(self_ty) else {
            return None;
        };
        if p.owner != imp.def {
            return None;
        }
        let g = imp.generics.get(usize::from(p.index))?;
        let b = g.numeric_family(&self.names)?;
        let members: Vec<Ty> = self
            .impls
            .get(&b)?
            .iter()
            .filter_map(|i| match &i.data {
                ItemData::Impl { self_ty, .. } if !pool.has_param(*self_ty) => Some(*self_ty),
                _ => None,
            })
            .collect();
        (!members.is_empty()).then_some((p, members))
    }

    pub(crate) fn unify(
        &self,
        a: Ty,
        b: Ty,
        oa: DefId,
        ob: DefId,
        sub: &mut HashMap<(DefId, u16), Ty>,
    ) -> bool {
        if a == b {
            return true;
        }
        let pool = self.pool();
        let mut path = Vec::new();
        let mut resolve = |mut t: Ty, sub: &mut HashMap<(DefId, u16), Ty>| {
            path.clear();
            // Bindings join resolved roots, so bare-parameter alias chains
            // are acyclic. Compress every followed link to the final root.
            while let TyData::Param(p) = pool.get(t)
                && (p.owner == oa || p.owner == ob)
                && let Some(&next) = sub.get(&(p.owner, p.index))
            {
                crate::work::declaration_type_visit();
                path.push((p.owner, p.index));
                t = next;
            }
            for &key in &path {
                sub.insert(key, t);
            }
            t
        };
        let list = |stack: &mut Vec<_>, x, y| {
            let (x, y) = (pool.list_items(x), pool.list_items(y));
            stack.extend(x.iter().zip(y).rev().map(|(&a, &b)| (a, b)));
            x.len() == y.len()
        };
        let mut rows = HashMap::default();
        let mut row_param = |r| {
            *rows.entry(r).or_insert_with(|| {
                let d = pool.row_data(r);
                match (d.keys.as_slice(), d.params.as_slice()) {
                    ([], [p]) => Some(*p),
                    _ => None,
                }
            })
        };
        // Equivalent term nodes need no repeated decomposition. Union by
        // rank and path halving keep graph unification near-linear, even
        // when different shared paths compare one node with many others.
        let mut classes = HashMap::<Ty, (Ty, u8)>::default();
        let root = |mut t, classes: &mut HashMap<Ty, (Ty, u8)>| {
            while let Some(&(parent, _)) = classes.get(&t)
                && parent != t
            {
                let grandparent = classes.get(&parent).map_or(parent, |&(p, _)| p);
                classes.entry(t).and_modify(|(p, _)| *p = grandparent);
                t = grandparent;
            }
            t
        };
        hd_base::stack::all((a, b), |(a, b), stack| {
            let (a, b) = (resolve(a, sub), resolve(b, sub));
            if a == b {
                return true;
            }
            let (ra, rb) = (root(a, &mut classes), root(b, &mut classes));
            if ra == rb {
                return true;
            }
            let rank_a = classes.get(&ra).map_or(0, |&(_, r)| r);
            let rank_b = classes.get(&rb).map_or(0, |&(_, r)| r);
            // Rank is at most log2(the number of distinct 32-bit type IDs).
            let (child, parent, rank) = if rank_a > rank_b {
                (rb, ra, rank_a)
            } else {
                (ra, rb, rank_b + u8::from(rank_a == rank_b))
            };
            classes.insert(child, (parent, 0));
            classes.insert(parent, (parent, rank));
            crate::work::declaration_type_visit();
            match (pool.get(a), pool.get(b)) {
                (TyData::Param(p), _) if p.owner == oa || p.owner == ob => {
                    sub.insert((p.owner, p.index), b);
                    true
                }
                (_, TyData::Param(p)) if p.owner == oa || p.owner == ob => {
                    sub.insert((p.owner, p.index), a);
                    true
                }
                (TyData::Adt(x, xa), TyData::Adt(y, ya)) => x == y && list(stack, xa, ya),
                // A row-valued data argument may be one bare parameter in an
                // implementation head (`req.row.param.data-substitute`). Bind it
                // to the other head's whole row so a generic and a concrete row
                // instance overlap. Row extensions were rejected while lowering
                // the head.
                (TyData::Row(x), TyData::Row(y)) => {
                    if let Some(p) = row_param(x)
                        && (p.owner == oa || p.owner == ob)
                    {
                        let param = pool.intern_ty(&TyData::Param(ParamRef {
                            owner: p.owner,
                            index: p.index,
                        }));
                        let row = pool.intern_ty(&TyData::Row(y));
                        stack.push((param, row));
                        return true;
                    }
                    if let Some(p) = row_param(y)
                        && (p.owner == oa || p.owner == ob)
                    {
                        let param = pool.intern_ty(&TyData::Param(ParamRef {
                            owner: p.owner,
                            index: p.index,
                        }));
                        let row = pool.intern_ty(&TyData::Row(x));
                        stack.push((row, param));
                        return true;
                    }
                    x == y
                }
                (
                    // An impl on `*T` and one on `&T` never overlap
                    // (`ref.type.kind-impl`).
                    TyData::Ref {
                        inner: x,
                        mutable: mx,
                        counted: cx,
                    },
                    TyData::Ref {
                        inner: y,
                        mutable: my,
                        counted: cy,
                    },
                ) => {
                    stack.push((x, y));
                    mx == my && cx == cy
                }

                (
                    TyData::TraitValue {
                        def: x, args: xa, ..
                    },
                    TyData::TraitValue {
                        def: y, args: ya, ..
                    },
                ) => x == y && list(stack, xa, ya),
                (
                    TyData::Fn {
                        params: x,
                        result: rx,
                        suspends: sx,
                        ..
                    },
                    TyData::Fn {
                        params: y,
                        result: ry,
                        suspends: sy,
                        ..
                    },
                ) => {
                    stack.push((rx, ry));
                    sx == sy && list(stack, x, y)
                }
                _ => false,
            }
        })
    }

    fn unify_list(
        &self,
        x: TyList,
        y: TyList,
        oa: DefId,
        ob: DefId,
        sub: &mut HashMap<(DefId, u16), Ty>,
    ) -> bool {
        let tuple = |elems| self.pool().product(elems, None);
        self.unify(tuple(x), tuple(y), oa, ob, sub)
    }
}

/// Whether an item is a tuple template, an implementation for every tuple.
fn is_tuple_template(it: &Item) -> bool {
    matches!(
        it.data,
        ItemData::Impl {
            kind: ImplKind::TupleTemplate,
            ..
        }
    )
}

/// Whether an item is a union template, an implementation for every union.
fn is_union_template(it: &Item) -> bool {
    matches!(
        it.data,
        ItemData::Impl {
            kind: ImplKind::UnionTemplate,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KnownItems;
    use hd_intern::{PathTable, ShardedInterner};

    #[test]
    fn head_unification_at_depth_1000_fits_a_small_stack() {
        std::thread::Builder::new()
            // cost: one 96 KiB thread for this test.
            .stack_size(96 << 10)
            .spawn(|| {
                let pool = InternPool::new();
                let paths = PathTable::new();
                let syms = ShardedInterner::default();
                let known = KnownItems::new(&paths);
                let universe = Universe::new(
                    Names {
                        pool: &pool,
                        paths: &paths,
                        syms: &syms,
                        known: &known,
                    },
                    std::iter::empty(),
                );
                let (oa, ob) = (DefId::from_raw(1), DefId::from_raw(2));
                let param = |index| pool.intern_ty(&TyData::Param(ParamRef { owner: oa, index }));
                for depth in [100, 1_000] {
                    let mut types = [param(0), Ty::I32, Ty::BOOL];
                    for _ in 0..depth {
                        for t in &mut types {
                            *t = pool.product(pool.list(&[*t, *t]), None);
                        }
                    }
                    let mut sub = HashMap::default();
                    let (matches, work) = crate::work::count_declaration_type_visits(|| {
                        universe.unify(types[0], types[1], oa, ob, &mut sub)
                    });
                    assert!(matches);
                    assert_eq!(sub.get(&(oa, 0)), Some(&Ty::I32));
                    assert_eq!(
                        work,
                        depth + 2,
                        "one comparison per shared pair, one resolved alias"
                    );
                    assert!(!universe.unify(types[1], types[2], oa, ob, &mut HashMap::default()));
                    let vars: Vec<_> = (0..depth)
                        .map(|i| param(u16::try_from(i).unwrap()))
                        .collect();
                    let mut aliases: HashMap<_, _> = vars
                        .iter()
                        .enumerate()
                        .map(|(i, _)| {
                            (
                                (oa, u16::try_from(i).unwrap()),
                                vars.get(i + 1).copied().unwrap_or(Ty::I32),
                            )
                        })
                        .collect();
                    let (matches, work) = crate::work::count_declaration_type_visits(|| {
                        universe.unify(vars[0], Ty::I32, oa, ob, &mut aliases)
                    });
                    assert!(matches);
                    assert_eq!(work, depth);
                    assert!(aliases.values().all(|&t| t == Ty::I32));
                    let (_, work) = crate::work::count_declaration_type_visits(|| {
                        universe.unify(vars[0], Ty::I32, oa, ob, &mut aliases)
                    });
                    assert_eq!(work, 1, "compressed alias lookup");
                }
                for depth in [100, 1_000] {
                    let mut types = [param(0), Ty::I32];
                    for _ in 0..depth {
                        for t in &mut types {
                            *t = pool.product(
                                TyList::EMPTY,
                                Some(pool.intern_ty(&TyData::Adt(
                                    AdtId::Declared(known.list),
                                    pool.list(&[*t]),
                                ))),
                            );
                        }
                    }
                    let mut sub = HashMap::default();
                    let (matches, work) = crate::work::count_declaration_type_visits(|| {
                        universe.unify(types[0], types[1], oa, ob, &mut sub)
                    });
                    assert!(matches);
                    assert_eq!(sub.get(&(oa, 0)), Some(&Ty::I32));
                    assert_eq!(work, 2 * depth + 1, "one tuple and List pair per level");
                }
                let mut costs = Vec::new();
                for width in [100, 1_000] {
                    let others: Vec<_> = (0..width)
                        .map(|index| pool.intern_ty(&TyData::Param(ParamRef { owner: ob, index })))
                        .collect();
                    let pair = |t| pool.product(pool.list(&[t, t]), None);
                    let a =
                        pool.product(pool.list(&vec![pair(param(0)); usize::from(width)]), None);
                    let b = pool.product(
                        pool.list(&others.iter().map(|&t| pair(t)).collect::<Vec<_>>()),
                        None,
                    );
                    let mut sub = HashMap::default();
                    let (matches, work) = crate::work::count_declaration_type_visits(|| {
                        universe.unify(a, b, oa, ob, &mut sub)
                    });
                    assert!(matches);
                    assert_eq!(sub.get(&(oa, 0)), others.last());
                    assert!(work <= 8 * u64::from(width) + 1);
                    costs.push(work);
                }
                assert!(costs[1] <= 11 * costs[0], "shared-head widths: {costs:?}");
                let row = pool.intern_ty(&TyData::Row(pool.row(&hd_types::RowData {
                    keys: vec![pool.intern_ty(&TyData::TraitValue {
                        def: DefId::from_raw(3),
                        args: TyList::EMPTY,
                        bindings: vec![],
                    })],
                    params: vec![],
                })));
                let row_param = pool.intern_ty(&TyData::Row(pool.row(&hd_types::RowData {
                    keys: vec![],
                    params: vec![hd_types::RowParamRef {
                        owner: oa,
                        index: 0,
                    }],
                })));
                let mut sub = HashMap::default();
                assert!(universe.unify_list(
                    pool.list(&[row_param, param(0)]),
                    pool.list(&[row, row]),
                    oa,
                    ob,
                    &mut sub
                ));
                assert_eq!(sub.get(&(oa, 0)), Some(&row));
                assert!(!universe.unify_list(
                    pool.list(&[Ty::I32]),
                    TyList::EMPTY,
                    oa,
                    ob,
                    &mut sub
                ));
                let function = |input, result| {
                    pool.intern_ty(&TyData::Fn {
                        params: pool.list(&[input]),
                        result,
                        row: hd_types::RowId::EMPTY,
                        suspends: false,
                        inputs: hd_types::Inputs::Fixed,
                        deps: hd_types::FnDeps::NONE,
                    })
                };
                let mut sub = HashMap::default();
                assert!(!universe.unify(
                    function(param(0), param(0)),
                    function(Ty::BOOL, Ty::I32),
                    oa,
                    ob,
                    &mut sub
                ));
                assert_eq!(
                    sub.get(&(oa, 0)),
                    Some(&Ty::BOOL),
                    "parameters bind before the result is compared"
                );
            })
            .expect("thread")
            .join()
            .expect("no stack overflow");
    }
}
