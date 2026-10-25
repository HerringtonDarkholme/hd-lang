//! Resolves opaque results from body facts, outside head-only checking.

use hd_base::{DefId, HashMap, NodeIdx};
use hd_tir::ir::{Body, Tag};
use hd_types::{InternPool, ParamRef, RowId, Ty, TyData, TyList};

/// The declared argument templates and the concrete result of one body.
pub type HiddenResult = (TyList, Ty);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Recursive(Vec<(DefId, u32)>),
    Missing(DefId),
    TooComplex(DefId),
}

/// Reveal body results recursively. A cycle names the declaration whose
/// hidden type would contain itself, rather than overflowing the compiler.
pub fn reveal(
    pool: &InternPool,
    ty: Ty,
    hidden: &dyn Fn(DefId, u32) -> Option<HiddenResult>,
) -> Result<Ty, Error> {
    Reveal {
        pool,
        hidden,
        stack: vec![],
        memo: HashMap::default(),
        depth: 0,
    }
    .ty(ty)
}

/// A result descriptor and the corresponding raw sub-body result.
#[derive(Clone, Copy)]
pub struct Fact {
    pub site: u32,
    pub opaque: Ty,
    pub hidden: Ty,
    pub syn: NodeIdx,
}

/// Read result facts from the existing body columns; no TIR metadata is added.
#[must_use]
pub fn facts(pool: &InternPool, body: &Body, declared: Option<Ty>) -> Vec<Fact> {
    let mut out = Vec::new();
    if let Some(declared) = declared
        && pool.has_opaque(declared)
        && let Some(&root) = body.sub_root.first()
    {
        result_facts(
            pool,
            declared,
            body.ty[root as usize],
            body.syn[root as usize],
            &mut out,
        );
    }
    for (i, tag) in body.tags.iter().enumerate() {
        if *tag != Tag::Closure || body.is_instantiation(i) {
            continue;
        }
        let TyData::Fn {
            result: declared, ..
        } = pool.get(body.ty[i])
        else {
            continue;
        };
        if !pool.has_opaque(declared) {
            continue;
        }
        if let Some(&root) = body
            .sub_root
            .iter()
            .find(|&&r| body.syn[r as usize] == body.syn[i])
        {
            result_facts(
                pool,
                declared,
                body.ty[root as usize],
                body.syn[root as usize],
                &mut out,
            );
        }
    }
    out
}

fn result_facts(pool: &InternPool, declared: Ty, hidden: Ty, syn: NodeIdx, out: &mut Vec<Fact>) {
    if !pool.has_opaque(declared) {
        return;
    }
    if let TyData::Opaque { site, .. } = pool.get(declared) {
        out.push(Fact {
            site,
            opaque: declared,
            hidden,
            syn,
        });
        return;
    }
    let mut list = |declared, hidden| {
        for (&declared, &hidden) in pool
            .list_items(declared)
            .iter()
            .zip(pool.list_items(hidden))
        {
            result_facts(pool, declared, hidden, syn, out);
        }
    };
    match (pool.get(declared), pool.get(hidden)) {
        (
            TyData::Adt {
                def: a,
                args: declared,
            },
            TyData::Adt {
                def: b,
                args: hidden,
            },
        ) if a == b => list(declared, hidden),
        (
            TyData::Tuple {
                elems: declared,
                rest: a,
            },
            TyData::Tuple {
                elems: hidden,
                rest: b,
            },
        ) => {
            list(declared, hidden);
            if let (Some(declared), Some(hidden)) = (a, b) {
                result_facts(pool, declared, hidden, syn, out);
            }
        }
        (TyData::Option(declared), TyData::Option(hidden))
        | (TyData::Mut(declared), TyData::Mut(hidden))
        | (
            TyData::Ref {
                inner: declared, ..
            },
            TyData::Ref { inner: hidden, .. },
        ) => result_facts(pool, declared, hidden, syn, out),
        (
            TyData::TraitValue {
                def: a,
                args: declared,
                bindings: a_bindings,
            },
            TyData::TraitValue {
                def: b,
                args: hidden,
                bindings: b_bindings,
            },
        ) if a == b => {
            list(declared, hidden);
            for ((a, declared), (b, hidden)) in a_bindings.into_iter().zip(b_bindings) {
                if a == b {
                    result_facts(pool, declared, hidden, syn, out);
                }
            }
        }
        _ => {}
    }
}

struct Reveal<'a> {
    pool: &'a InternPool,
    hidden: &'a dyn Fn(DefId, u32) -> Option<HiddenResult>,
    stack: Vec<Ty>,
    memo: HashMap<Ty, Ty>,
    depth: u32,
}

impl Reveal<'_> {
    fn list(&mut self, args: TyList) -> Result<TyList, Error> {
        let args = self
            .pool
            .list_items(args)
            .iter()
            .map(|&t| self.ty(t))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.pool.list(&args))
    }

    fn row(&mut self, row: RowId) -> Result<RowId, Error> {
        let mut row = self.pool.row_data(row);
        row.keys = row
            .keys
            .into_iter()
            .map(|t| self.ty(t))
            .collect::<Result<_, _>>()?;
        Ok(self.pool.row(&row))
    }

    fn ty(&mut self, ty: Ty) -> Result<Ty, Error> {
        if !self.pool.has_opaque(ty) {
            return Ok(ty);
        }
        if let Some(&done) = self.memo.get(&ty) {
            return Ok(done);
        }
        if self.depth >= crate::layout::MAX_CHAIN {
            return Err(Error::TooComplex(self.stack.last().map_or(
                DefId::NONE,
                |&t| match self.pool.get(t) {
                    TyData::Opaque { owner, .. } => owner,
                    _ => DefId::NONE,
                },
            )));
        }
        self.depth += 1;
        let result = self.inner(ty);
        self.depth -= 1;
        if let Ok(done) = result {
            self.memo.insert(ty, done);
        }
        result
    }

    fn inner(&mut self, ty: Ty) -> Result<Ty, Error> {
        let pool = self.pool;
        let data = match pool.get(ty) {
            TyData::Opaque {
                owner, site, args, ..
            } => {
                if let Some(start) = self.stack.iter().position(|&key| key == ty) {
                    let cycle = self.stack[start..]
                        .iter()
                        .filter_map(|&t| match pool.get(t) {
                            TyData::Opaque { owner, site, .. } => Some((owner, site)),
                            _ => None,
                        })
                        .collect();
                    return Err(Error::Recursive(cycle));
                }
                let Some((formal, result)) = (self.hidden)(owner, site) else {
                    return Err(Error::Missing(owner));
                };
                let actual = pool.list_items(args);
                let formal = pool.list_items(formal);
                if formal.len() != actual.len() {
                    return Err(Error::Missing(owner));
                }
                // Keep arguments lazy: an unused argument does not make the
                // concrete result recursive merely by naming an opaque type.
                let result = pool.subst(result, &|p: ParamRef| {
                    formal.iter().zip(actual).find_map(|(&key, &value)| {
                        let matches = match pool.get(key) {
                            TyData::Param(k) => k == p,
                            TyData::Row(r) => {
                                let r = pool.row_data(r);
                                r.keys.is_empty()
                                    && r.params.len() == 1
                                    && r.params[0].owner == p.owner
                                    && r.params[0].index == p.index
                            }
                            _ => false,
                        };
                        matches.then_some(value)
                    })
                });
                self.stack.push(ty);
                let result = self.ty(result);
                self.stack.pop();
                return result;
            }
            TyData::Adt { def, args } => TyData::Adt {
                def,
                args: self.list(args)?,
            },
            TyData::Tuple { elems, rest } => TyData::Tuple {
                elems: self.list(elems)?,
                rest: rest.map(|t| self.ty(t)).transpose()?,
            },
            TyData::Option(t) => TyData::Option(self.ty(t)?),
            TyData::Mut(t) => TyData::Mut(self.ty(t)?),
            TyData::Ref {
                inner,
                mutable,
                counted,
            } => TyData::Ref {
                inner: self.ty(inner)?,
                mutable,
                counted,
            },
            TyData::Fn {
                params,
                result,
                row,
                suspends,
                inputs,
            } => TyData::Fn {
                params: self.list(params)?,
                result: self.ty(result)?,
                row: self.row(row)?,
                suspends,
                inputs,
            },
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => TyData::TraitValue {
                def,
                args: self.list(args)?,
                bindings: bindings
                    .into_iter()
                    .map(|(d, t)| Ok((d, self.ty(t)?)))
                    .collect::<Result<_, Error>>()?,
            },
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => TyData::Assoc {
                assoc,
                trait_,
                self_ty: self.ty(self_ty)?,
                args: self.list(args)?,
            },
            TyData::Row(r) => TyData::Row(self.row(r)?),
            TyData::Context(r) => TyData::Context(self.row(r)?),
            other => other,
        };
        Ok(pool.intern_ty(&data))
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, reveal};
    use hd_base::DefId;
    use hd_types::{InternPool, ParamRef, Ty, TyData, TyList};

    #[test]
    fn opaque_reveal_substitutes_parameters_and_detects_cycles() {
        let pool = InternPool::new();
        let f = DefId::from_raw(41);
        let g = DefId::from_raw(42);
        let t = pool.intern_ty(&TyData::Param(ParamRef { owner: f, index: 0 }));
        let make = |owner, args| {
            pool.intern_ty(&TyData::Opaque {
                owner,
                site: 0,
                args,
                bound: Ty::BOOL,
            })
        };
        let applied = make(f, pool.list(&[Ty::I32]));
        assert_eq!(
            reveal(&pool, applied, &|_, _| Some((pool.list(&[t]), t))),
            Ok(Ty::I32)
        );
        let nested = make(f, pool.list(&[applied]));
        assert_eq!(
            reveal(&pool, nested, &|_, _| Some((pool.list(&[t]), t))),
            Ok(Ty::I32)
        );
        let a = make(f, TyList::EMPTY);
        let b = make(g, TyList::EMPTY);
        assert_eq!(
            reveal(&pool, a, &|owner, _| Some((
                TyList::EMPTY,
                if owner == f { b } else { a }
            ))),
            Err(Error::Recursive(vec![(f, 0), (g, 0)]))
        );
        assert_eq!(reveal(&pool, a, &|_, _| None), Err(Error::Missing(f)));
        // An argument unused by the hidden result is not recursively revealed.
        let unused = make(f, pool.list(&[a]));
        assert_eq!(
            reveal(&pool, unused, &|_, _| Some((pool.list(&[t]), Ty::I32))),
            Ok(Ty::I32)
        );
    }
    #[test]
    fn opaque_reveal_bounds_work_on_deep_acyclic_aliases() {
        let pool = InternPool::new();
        let make = |owner| {
            pool.intern_ty(&TyData::Opaque {
                owner: DefId::from_raw(owner),
                site: 0,
                args: TyList::EMPTY,
                bound: Ty::BOOL,
            })
        };
        let out = reveal(&pool, make(1), &|owner, _| {
            Some((
                TyList::EMPTY,
                if owner.raw() < 10_000 {
                    make(owner.raw() + 1)
                } else {
                    Ty::I32
                },
            ))
        });
        assert!(matches!(out, Err(Error::TooComplex(_))));
    }

    #[test]
    fn opaque_reveal_shares_repeated_result_equations() {
        let pool = InternPool::new();
        let make = |owner| {
            pool.intern_ty(&TyData::Opaque {
                owner: DefId::from_raw(owner),
                site: 0,
                args: TyList::EMPTY,
                bound: Ty::BOOL,
            })
        };
        let reads = std::cell::Cell::new(0);
        let out = reveal(&pool, make(30), &|owner, _| {
            reads.set(reads.get() + 1);
            let result = if owner.raw() == 1 {
                Ty::I32
            } else {
                let child = make(owner.raw() - 1);
                pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&[child, child]),
                    rest: None,
                })
            };
            Some((TyList::EMPTY, result))
        });
        assert!(out.is_ok());
        assert_eq!(reads.get(), 30);
    }
}
