//! A derivation's `Structure` at check time (annotations, "The Structure
//! Trait", "Members And Variants", "Self References"; codegen.md §12.3
//! "Derives"): the target's member list, built from the data type or enum
//! and the derivation's omitted members, for the one generator in
//! `hd_structure`, which writes the bodies as hidden methods of the
//! derived implementation. Collection calls the same generator for a
//! tuple's `Structure` (codegen.md §13.6).
//!
//! Not generated yet, each a structured "not implemented" that leaves the
//! derivation's `Structure` without bodies (a call of it then stops at
//! collection): facts (declaration facts and member lines) and shared
//! constructor data.

use hd_base::{DefId, Stage, StageResult};
use hd_resolve::ItemData;
use hd_structure::{Mem, Shape, Target, Var};
use hd_types::{ParamRef, Ty, TyData, TyList};

use crate::body::{BodyCx, unsupported};
use crate::derive::OptIn;

pub use hd_structure::Generated;

/// The `Structure` bodies of `opt`. `cx` sees the target's module and
/// `std.structure`. `has_facts`: the derivation sees a fact (a decorator
/// on the target or a member, a member line, or a trait-less block).
pub fn structure_bodies(cx: &BodyCx<'_>, opt: &OptIn, has_facts: bool) -> StageResult<Generated> {
    if has_facts {
        return unsupported("the facts of a derivation's `Structure`");
    }
    let names = &cx.names;
    let name = cx
        .lookup
        .item(opt.data)
        .map(|i| names.text(i.name).to_owned())
        .unwrap_or_default();
    let target = Target {
        ty: opt.target,
        name,
        vars: variants(cx, opt)?,
    };
    let gcx = hd_structure::Cx {
        names,
        items: cx.lookup,
        stage: Stage::Body,
    };
    hd_structure::derivation(&gcx, &target, opt.impl_)
}

/// The target's variants and members, with the derivation's omitted
/// members marked (`annot.omit.skip`).
fn variants(cx: &BodyCx<'_>, opt: &OptIn) -> StageResult<Vec<Var>> {
    let names = &cx.names;
    let pool = names.pool;
    let Some(item) = cx.lookup.item(opt.data) else {
        return unsupported("a derivation target outside the module's view");
    };
    let TyData::Adt { args, .. } = pool.get(opt.target) else {
        return unsupported("a derivation target that is not a data type or enum");
    };
    let args = pool.list_items(args).to_vec();
    let data = opt.data;
    let ty = |t: Ty| {
        pool.subst(t, &|p: ParamRef| {
            (p.owner == data).then(|| args.get(usize::from(p.index)).copied())?
        })
    };
    let member = |i: usize, f: &hd_resolve::Field, omitted: bool| {
        let declared = names.text(f.name).to_owned();
        let positional = declared.starts_with(|c: char| c.is_ascii_digit());
        let t = ty(f.ty);
        Mem {
            name: if positional {
                format!("_{declared}")
            } else {
                declared.clone()
            },
            declared,
            position: u32::try_from(i).unwrap_or(0),
            field: u32::try_from(i).unwrap_or(0),
            ty: t,
            embedded: f.embedded,
            positional,
            has_default: f.has_default,
            omitted,
            rest: false,
            self_ref: SelfRefs::new(cx, data).member(t),
        }
    };
    let mut out = Vec::new();
    match &item.data {
        ItemData::Data(fields) => {
            let members: Vec<Mem> = fields
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let n = names.text(f.name);
                    let label = if n.starts_with(|c: char| c.is_ascii_digit()) {
                        format!("_{n}")
                    } else {
                        n.to_owned()
                    };
                    member(i, f, opt.omitted.contains(&label))
                })
                .collect();
            out.push(Var {
                name: names.text(item.name).to_owned(),
                index: 0,
                def: data,
                shape: Shape::Data,
                self_ref: members.iter().map(|m| m.self_ref).max().unwrap_or(0),
                members,
            });
        }
        ItemData::Enum { shared, variants } => {
            if !shared.is_empty() {
                return unsupported("the shared constructor data of a derivation's variants");
            }
            for (k, v) in variants.iter().enumerate() {
                let members: Vec<Mem> = v
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| member(i, f, false))
                    .collect();
                out.push(Var {
                    name: names.text(v.name).to_owned(),
                    index: u32::try_from(k).unwrap_or(0),
                    def: v.def,
                    shape: Shape::Enum,
                    self_ref: members.iter().map(|m| m.self_ref).max().unwrap_or(0),
                    members,
                });
            }
        }
        _ => return unsupported("a derivation target that is not a data type or enum"),
    }
    Ok(out)
}

/// Self references (`annot.self-ref.*`): whether a member's type refers
/// to, or needs, the enclosing type `data`.
struct SelfRefs<'a, 'c> {
    cx: &'a BodyCx<'c>,
    data: DefId,
}

impl<'a, 'c> SelfRefs<'a, 'c> {
    fn new(cx: &'a BodyCx<'c>, data: DefId) -> Self {
        Self { cx, data }
    }

    /// `SelfRef`'s index for a member of type `t`.
    fn member(&self, t: Ty) -> u32 {
        if self.needs(t, &mut Vec::new()) {
            2
        } else {
            u32::from(self.refers(t, &mut Vec::new()))
        }
    }

    /// The fields of a data type or each variant of an enum, with `args`
    /// substituted.
    fn fields(&self, def: DefId, args: TyList) -> Option<(bool, Vec<Vec<Ty>>)> {
        let pool = self.cx.names.pool;
        let args = pool.list_items(args).to_vec();
        let ty = |t: Ty| {
            pool.subst(t, &|p: ParamRef| {
                (p.owner == def).then(|| args.get(usize::from(p.index)).copied())?
            })
        };
        match &self.cx.lookup.item(def)?.data {
            ItemData::Data(fs) => Some((true, vec![fs.iter().map(|f| ty(f.ty)).collect()])),
            ItemData::Enum { variants, .. } => Some((
                false,
                variants
                    .iter()
                    .map(|v| v.fields.iter().map(|f| ty(f.ty)).collect())
                    .collect(),
            )),
            _ => None,
        }
    }

    /// `annot.self-ref.refers` and `.refers.members`.
    fn refers(&self, t: Ty, seen: &mut Vec<Ty>) -> bool {
        let pool = self.cx.names.pool;
        if seen.contains(&t) {
            return false;
        }
        seen.push(t);
        match pool.get(t) {
            TyData::Mut(x) | TyData::Option(x) => self.refers(x, seen),
            TyData::Tuple { elems, rest } => {
                pool.list_items(elems).iter().any(|e| self.refers(*e, seen))
                    || rest.is_some_and(|r| self.refers(r, seen))
            }
            TyData::Adt { def, args } => {
                def == self.data
                    || pool.list_items(args).iter().any(|a| self.refers(*a, seen))
                    || self
                        .fields(def, args)
                        .is_some_and(|(_, vs)| vs.iter().flatten().any(|f| self.refers(*f, seen)))
            }
            _ => false,
        }
    }

    /// `annot.self-ref.needs.*`.
    fn needs(&self, t: Ty, seen: &mut Vec<Ty>) -> bool {
        let pool = self.cx.names.pool;
        let k = &self.cx.names.known;
        if seen.contains(&t) {
            return false;
        }
        seen.push(t);
        match pool.get(t) {
            TyData::Mut(x) => self.needs(x, seen),
            TyData::Tuple { elems, rest } => {
                pool.list_items(elems).iter().any(|e| self.needs(*e, seen))
                    || rest.is_some_and(|r| self.needs(r, seen))
            }
            TyData::Adt { def, args } => {
                if def == self.data {
                    return true;
                }
                if def == k.list || def == k.map {
                    return false;
                }
                if def == k.result {
                    let first = pool.list_items(args).first().copied();
                    return first.is_some_and(|x| self.needs(x, seen));
                }
                match self.fields(def, args) {
                    Some((true, vs)) => vs.iter().flatten().any(|f| self.needs(*f, seen)),
                    Some((false, vs)) => {
                        !vs.is_empty()
                            && vs
                                .iter()
                                .all(|v| v.iter().any(|f| self.needs(*f, &mut seen.clone())))
                    }
                    None => false,
                }
            }
            _ => false,
        }
    }
}
