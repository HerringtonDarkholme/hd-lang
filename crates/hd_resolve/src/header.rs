//! The overlap check of coherence (§4.12.3, `crates/hd_check/DESIGN.md` (trait solver) §5.2), over
//! the impl heads of frozen interfaces. The header checks that need goals
//! are `hd_check::header`'s, asked through the solver.

use hd_base::HashMap;
use hd_base::SliceSort as _;

use hd_base::{DefId, Hash128, StableHasher, StageResult};
use hd_types::{InternPool, ParamRef, Ty, TyData, TyList};

use crate::iface::{ImplKind, Item, ItemData, Names};

/// A head-index key of the overlap check: an outer constructor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum HeadKey {
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

    /// The overlap check (§4.12.3): per trait, heads that unify after
    /// renaming apart overlap. A head whose target is a bare parameter
    /// bounded by a trait with only ground impls (a numeric family) is
    /// expanded to those targets first. Returns (earlier, later, witness)
    /// in content order.
    #[must_use]
    pub fn overlaps(&self, order: &dyn Fn(DefId) -> String) -> Vec<(DefId, DefId, String)> {
        self.overlaps_counted(order).0
    }

    /// [`Self::overlaps`], and how many head pairs it unified: each
    /// trait's [`Self::trait_overlaps`], in [`Self::traits`] order.
    #[must_use]
    pub fn overlaps_counted(
        &self,
        order: &dyn Fn(DefId) -> String,
    ) -> (Vec<(DefId, DefId, String)>, usize) {
        let mut out = Vec::new();
        let mut tried = 0_usize;
        for tr in self.traits() {
            let (found, n) = self.trait_overlaps(tr, order);
            out.extend(found);
            tried += n;
        }
        (out, tried)
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
                imp.generics.get(usize::from(p.index))?.bound
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
                        && matches!(pool.get(*self_ty), TyData::Tuple { .. })
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
                if *kind == ImplKind::TupleTemplate {
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
    fn head_key(&self, t: Ty) -> Option<HeadKey> {
        let pool = self.pool();
        // `unify` matches outer constructors, so a key is the constructor
        // alone; a primitive is its own constructor.
        match pool.get(t) {
            TyData::Adt { def, .. } => Some(HeadKey::Def(def)),
            TyData::TraitValue { def, .. } => Some(HeadKey::Trait(def)),
            TyData::Option(_) => Some(HeadKey::Ctor(0, 0)),
            TyData::Mut(_) => Some(HeadKey::Ctor(1, 0)),
            TyData::Ref { mutable, .. } => Some(HeadKey::Ctor(4, u32::from(mutable))),
            TyData::Fn { .. } => Some(HeadKey::Ctor(2, 0)),
            TyData::Tuple { elems, rest: None } => Some(HeadKey::Ctor(
                3,
                u32::try_from(pool.list_items(elems).len()).unwrap_or(u32::MAX),
            )),
            TyData::Param(_) | TyData::Tuple { .. } => None,
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
        let b = g.bound?;
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
        let pool = self.pool();
        let resolve = |t: Ty, sub: &HashMap<(DefId, u16), Ty>| -> Ty {
            let mut t = t;
            for _ in 0..64 {
                match pool.get(t) {
                    TyData::Param(p) if p.owner == oa || p.owner == ob => {
                        match sub.get(&(p.owner, p.index)) {
                            Some(&x) => t = x,
                            None => break,
                        }
                    }
                    _ => break,
                }
            }
            t
        };
        let (a, b) = (resolve(a, sub), resolve(b, sub));
        if a == b {
            return true;
        }
        match (pool.get(a), pool.get(b)) {
            (TyData::Param(p), _) if p.owner == oa || p.owner == ob => {
                sub.insert((p.owner, p.index), b);
                true
            }
            (_, TyData::Param(p)) if p.owner == oa || p.owner == ob => {
                sub.insert((p.owner, p.index), a);
                true
            }
            (TyData::Adt { def: x, args: xa }, TyData::Adt { def: y, args: ya }) => {
                x == y && self.unify_list(xa, ya, oa, ob, sub)
            }
            (TyData::Option(x), TyData::Option(y)) | (TyData::Mut(x), TyData::Mut(y)) => {
                self.unify(x, y, oa, ob, sub)
            }
            (
                // One impl covers both kinds of reference
                // (`ref.type.kind-runtime`), so they overlap.
                TyData::Ref {
                    inner: x,
                    mutable: mx,
                    ..
                },
                TyData::Ref {
                    inner: y,
                    mutable: my,
                    ..
                },
            ) => mx == my && self.unify(x, y, oa, ob, sub),
            (
                TyData::Tuple {
                    elems: x,
                    rest: None,
                },
                TyData::Tuple {
                    elems: y,
                    rest: None,
                },
            ) => self.unify_list(x, y, oa, ob, sub),
            (
                TyData::TraitValue {
                    def: x, args: xa, ..
                },
                TyData::TraitValue {
                    def: y, args: ya, ..
                },
            ) => x == y && self.unify_list(xa, ya, oa, ob, sub),
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
            ) => sx == sy && self.unify_list(x, y, oa, ob, sub) && self.unify(rx, ry, oa, ob, sub),
            _ => false,
        }
    }

    fn unify_list(
        &self,
        x: TyList,
        y: TyList,
        oa: DefId,
        ob: DefId,
        sub: &mut HashMap<(DefId, u16), Ty>,
    ) -> bool {
        let (x, y) = (self.pool().list_items(x), self.pool().list_items(y));
        x.len() == y.len()
            && x.iter()
                .zip(y)
                .all(|(a, b)| self.unify(*a, *b, oa, ob, sub))
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
