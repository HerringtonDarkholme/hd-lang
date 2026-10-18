//! Drop glue at collection (spec/lang/13-ownership.md, Drops; ownership
//! prototype slice 10). A concrete type **has glue** when dropping one of
//! its values runs some `drop`: it implements `DropOwn`, or a part it holds
//! by value has glue (a data type's field, an enum's payload, an optional's
//! payload, a tuple's element, a list's element, a map's value). A borrow,
//! a function and a trait value hold none. Every drop of a value with glue,
//! implicit or written as `drop(x)`, calls the instance of
//! `std.ownership.drop` at the value's type, whose body the compiler
//! supplies (`ProgramEnv::drop_body`); a value without glue drops nothing.

use std::sync::Arc;

use hd_base::{DefId, InstId, StageResult};
use hd_types::{ParamRef, Ty, TyData, TyList};

use crate::layout::{StdKind, Sub};
use crate::{CallTarget, Cx, Picked, TargetKind, subst};

impl Cx<'_> {
    /// Whether dropping a value of the concrete type `t` runs a `drop`.
    pub(crate) fn has_glue(&mut self, t: Ty) -> bool {
        let mut open = Vec::new();
        self.glue_of(t, &mut open).0
    }

    /// [`Self::has_glue`] with the types being decided on `open`: a
    /// recursive type has glue only when a part reaches some outside the
    /// recursion (the least solution). The second result says whether the
    /// answer leaned on an open type, so it is kept only once that type is
    /// decided.
    fn glue_of(&mut self, t: Ty, open: &mut Vec<Ty>) -> (bool, bool) {
        if let Some(&g) = self.glue.get(&t) {
            return (g, false);
        }
        if open.contains(&t) {
            return (false, true);
        }
        let pool = self.pool;
        let parts: Vec<Ty> = match pool.get(t) {
            TyData::Own(i) | TyData::Mut(i) | TyData::Option(i) => vec![i],
            TyData::Tuple { elems, rest } => {
                pool.list_items(elems).iter().copied().chain(rest).collect()
            }
            TyData::Adt { def, args } => {
                let argv = pool.list_items(args).to_vec();
                match self.env.std_kind(def) {
                    StdKind::List => argv.first().copied().into_iter().collect(),
                    StdKind::Map => argv.get(1).copied().into_iter().collect(),
                    StdKind::Suspend => Vec::new(),
                    StdKind::Other => {
                        if self.drop_impl(t).is_some() {
                            self.glue.insert(t, true);
                            return (true, false);
                        }
                        let sub = |x: Ty| {
                            pool.subst(x, &|p: ParamRef| {
                                (p.owner == def).then(|| argv.get(usize::from(p.index)).copied())?
                            })
                        };
                        if let Some(vs) = self.env.enum_variants(def, args) {
                            vs.into_iter().flatten().collect()
                        } else if let Some(base) = self.env.newtype_base(def) {
                            vec![sub(base)]
                        } else {
                            self.env
                                .data_fields(def)
                                .unwrap_or_default()
                                .into_iter()
                                .map(sub)
                                .collect()
                        }
                    }
                }
            }
            _ => Vec::new(),
        };
        open.push(t);
        let mut glue = false;
        let mut leaned = false;
        for p in parts {
            let (g, l) = self.glue_of(p, open);
            leaned |= l;
            if g {
                glue = true;
                leaned = false;
                break;
            }
        }
        open.pop();
        if !leaned || open.is_empty() {
            self.glue.insert(t, glue);
        }
        (glue, leaned)
    }

    /// The `DropOwn` implementation of the concrete nominal type under
    /// `t`'s modes, if it has one.
    fn drop_impl(&mut self, t: Ty) -> Option<DefId> {
        let (trait_, _, _) = self.env.drop_items()?;
        let pool = self.pool;
        let mut core = t;
        while let TyData::Own(i) | TyData::Mut(i) = pool.get(core) {
            core = i;
        }
        if !matches!(pool.get(core), TyData::Adt { .. }) {
            return None;
        }
        match self.select(trait_, core, TyList::EMPTY, None) {
            Ok(Picked::Impl(d, _)) => Some(d),
            _ => None,
        }
    }

    /// The call that drops a value of the concrete type `t`, or `None`
    /// when `t` has no glue.
    pub(crate) fn drop_target(&mut self, t: Ty, parent: InstId) -> StageResult<Option<CallTarget>> {
        if !self.has_glue(t) {
            return Ok(None);
        }
        let Some((_, _, drop)) = self.env.drop_items() else {
            return Ok(None);
        };
        self.glue_call(drop, t, parent).map(Some)
    }

    /// The instance of `drop[t]`, keyed like any other with no A1 class:
    /// its body reads `t`'s declaration (`ProgramEnv::drop_body`).
    pub(crate) fn glue_call(
        &mut self,
        drop: DefId,
        t: Ty,
        parent: InstId,
    ) -> StageResult<CallTarget> {
        if !self.out.supplied.contains_key(&(drop, t)) {
            let body = if self.has_glue(t) {
                let d = self.drop_impl(t);
                self.env.drop_body(t, d)?
            } else {
                self.env.drop_body(t, None)?
            };
            self.out.supplied.insert((drop, t), Arc::new(body));
        }
        let args = self.pool.list(&[t]);
        let key = self.push(drop, Sub::Body(0), args, args, parent)?;
        Ok(CallTarget {
            key,
            item: drop,
            args,
            ret: subst(self.pool, self.env, drop, args, Ty::VOID),
            kind: TargetKind::Instance,
        })
    }
}
