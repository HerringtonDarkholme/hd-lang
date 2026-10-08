//! The overlap check of coherence (§4.12.3, trait-solver.md §5.2), over
//! the impl heads of frozen interfaces. The header checks that need goals
//! are `hd_check::header`'s, asked through the solver.

use std::collections::HashMap;

use hd_base::DefId;
use hd_types::{InternPool, ParamRef, Ty, TyData, TyList};

use crate::iface::{ImplKind, Item, ItemData, Names};

/// The trait impls of a program graph, by trait.
pub struct Universe<'a> {
    pub names: Names<'a>,
    pub impls: HashMap<DefId, Vec<&'a Item>>,
}

impl<'a> Universe<'a> {
    #[must_use]
    pub fn new(names: Names<'a>, all: impl Iterator<Item = &'a Item>) -> Self {
        let mut impls: HashMap<DefId, Vec<&Item>> = HashMap::new();
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
        let pool = self.pool();
        let mut out = Vec::new();
        let mut traits: Vec<&DefId> = self.impls.keys().collect();
        traits.sort_by_key(|d| self.names.path_hash(**d));
        for tr in traits {
            let mut heads: Vec<(DefId, Ty, TyList)> = Vec::new();
            let mut list: Vec<&&Item> = self.impls[tr].iter().collect();
            list.sort_by_key(|i| order(i.def));
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
            let mut reported = std::collections::HashSet::new();
            for (j, b) in heads.iter().enumerate() {
                for a in &heads[..j] {
                    if a.0 == b.0 || reported.contains(&b.0) {
                        continue;
                    }
                    let mut sub = HashMap::new();
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
        out
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

    fn unify(
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
