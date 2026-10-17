//! The header rules of Ownership and Borrows (spec/lang/13-ownership.md)
//! that need the declarations a header names.

use hd_base::{DefId, HashMap, PathId, StageResult, Symbol};
use hd_diag::Code;
use hd_intern::PathKind;
use hd_resolve::{ItemData, KnownItems, Lookup, Names};
use hd_types::solver::Impls;
use hd_types::{Lifetime, Ty, TyData, Types};

/// Every part of `t`, `t` included, lifetimes too.
pub(crate) fn parts(pool: Types<'_>, t: Ty, out: &mut Vec<Ty>) {
    out.push(t);
    let kids = |l: hd_types::TyList, out: &mut Vec<Ty>| {
        for &x in pool.list_items(l) {
            parts(pool, x, out);
        }
    };
    match pool.get(t) {
        TyData::Own(i) | TyData::Mut(i) | TyData::Option(i) => parts(pool, i, out),
        TyData::Borrow { ty, lifetimes, .. } => {
            kids(lifetimes, out);
            parts(pool, ty, out);
        }
        TyData::Adt { args: l, .. }
        | TyData::Lifetimes(l)
        | TyData::Tuple { elems: l, .. }
        | TyData::TraitValue { args: l, .. } => kids(l, out),
        TyData::Fn { params, result, .. } => {
            kids(params, out);
            parts(pool, result, out);
        }
        _ => {}
    }
}

/// The plain positions of `t` that hold a named type: those that no
/// `own` or `^` wraps directly (`own.own-only.plain`).
pub(crate) fn plain_positions(pool: Types<'_>, t: Ty, marked: bool, out: &mut Vec<Ty>) {
    let kids = |l: hd_types::TyList, out: &mut Vec<Ty>| {
        for &x in pool.list_items(l) {
            plain_positions(pool, x, false, out);
        }
    };
    match pool.get(t) {
        TyData::Own(i) | TyData::Borrow { ty: i, .. } => plain_positions(pool, i, true, out),
        TyData::Mut(i) | TyData::Option(i) => plain_positions(pool, i, false, out),
        TyData::Adt { args: l, .. } | TyData::TraitValue { args: l, .. } => {
            if !marked {
                out.push(t);
            }
            kids(l, out);
        }
        TyData::Tuple { elems, .. } => kids(elems, out),
        TyData::Fn { params, result, .. } => {
            kids(params, out);
            plain_positions(pool, result, false, out);
        }
        _ => {}
    }
}

/// `own.header.field-path`: each field of a lifetime's field path exists and
/// is `pub`, so a signature names no private field.
pub(crate) fn field_paths(
    names: &Names<'_>,
    lookup: &Lookup<'_>,
    params: &[(Symbol, Ty)],
    t: Ty,
) -> Vec<(Code, String)> {
    let pool = names.pool.types();
    if !pool.has_mode(t) {
        return Vec::new();
    }
    let mut all = Vec::new();
    parts(pool, t, &mut all);
    let mut out = Vec::new();
    for p in all {
        let TyData::Lifetime(Lifetime::Field { input, path }) = pool.get(p) else {
            continue;
        };
        let Some(&(_, mut ty)) = params.get(usize::from(input)) else {
            continue;
        };
        let mut segs = Vec::new();
        let mut at = PathId::from_raw(path.raw());
        while at.get().is_some() && names.paths.kind(at) == PathKind::Member {
            segs.push(names.paths.segment(at).to_owned());
            at = names.paths.parent(at);
        }
        for seg in segs.iter().rev() {
            while let TyData::Own(i) | TyData::Mut(i) | TyData::Borrow { ty: i, .. } = pool.get(ty)
            {
                ty = i;
            }
            let fields = match pool.get(ty) {
                TyData::Adt { def, .. } => data_fields(lookup, def),
                _ => None,
            };
            let found = fields.and_then(|fs| fs.into_iter().find(|f| names.text(f.0) == seg));
            match found {
                None => {
                    let msg = format!(
                        "a lifetime path names no field `{seg}` of {}",
                        hd_resolve::show_ty(names, ty)
                    );
                    out.push((Code::UnknownField, msg));
                    break;
                }
                Some((_, _, false)) => {
                    let msg = format!(
                        "a lifetime path names only `pub` fields, and `{seg}` is not `pub`"
                    );
                    out.push((Code::PrivateMember, msg));
                    break;
                }
                Some((_, next, true)) => ty = next,
            }
        }
    }
    out
}

/// A data type's fields: name, type, and whether each is `pub`.
fn data_fields(lookup: &Lookup<'_>, def: DefId) -> Option<Vec<(Symbol, Ty, bool)>> {
    match &lookup.item(def)?.data {
        ItemData::Data(fs) => Some(fs.iter().map(|f| (f.name, f.ty, f.public)).collect()),
        _ => None,
    }
}

/// What decides ownership by structure (`own.structure.*`): the type
/// pool, the declarations, the impls, and a proof by a written impl.
pub(crate) struct Classify<'x, 'a> {
    pub pool: Types<'a>,
    pub lookup: &'x Lookup<'a>,
    pub impls: Impls<'x>,
    pub known: &'x KnownItems,
    pub memo: &'x mut StructureMemo,
    /// Whether a written impl proves the type `OwnOnly`.
    pub written: &'x mut dyn FnMut(Ty) -> StageResult<bool>,
}

/// The structure summaries of declared types, computed once per run of a
/// checker: each least fixed point is final once computed.
#[derive(Default)]
pub(crate) struct StructureMemo {
    done: HashMap<DefId, Sum>,
}

/// When an instance of a type that declares `OwnOnly` by structure owns:
/// always, or when one of the arguments at `via` owns
/// (`own.structure.owns`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Sum {
    always: bool,
    via: Vec<u16>,
}

impl Sum {
    fn join(&mut self, other: &Sum) {
        self.always |= other.always;
        for &v in &other.via {
            if !self.via.contains(&v) {
                self.via.push(v);
            }
        }
    }
}

impl Classify<'_, '_> {
    /// Whether a value of type `t` owns (`own.structure.owns`): an `own`
    /// part, or an instance of a type that implements `OwnOnly`, written
    /// or by structure. A borrow, a function type and a type parameter
    /// own nothing.
    pub fn owns(&mut self, t: Ty) -> StageResult<bool> {
        let pool = self.pool;
        Ok(match pool.get(t) {
            TyData::Own(_) => true,
            TyData::Mut(i) | TyData::Option(i) => self.owns(i)?,
            TyData::Tuple { elems, rest } => {
                for &e in pool.list_items(elems).iter().chain(rest.iter()) {
                    if self.owns(e)? {
                        return Ok(true);
                    }
                }
                false
            }
            TyData::Adt { def, args } => {
                let args = pool.list_items(args);
                if let Some(at) = self.container_item(def) {
                    match args.get(at) {
                        Some(&a) => self.owns(a)?,
                        None => false,
                    }
                } else if self.impls.by_structure(pool, self.known.own_only, t) {
                    let sum = self.summary(def)?;
                    if sum.always {
                        return Ok(true);
                    }
                    for v in sum.via {
                        if let Some(&a) = args.get(usize::from(v))
                            && self.owns(a)?
                        {
                            return Ok(true);
                        }
                    }
                    false
                } else {
                    (self.written)(t)?
                }
            }
            TyData::TraitValue { .. } => (self.written)(t)?,
            _ => false,
        })
    }

    /// Whether `D[args]` admits an owned argument at `index`
    /// (`own.structure.admission`): a built-in container's item, a type
    /// that declares `OwnOnly` by structure, or an instance a written impl
    /// makes `OwnOnly`.
    pub fn admits(&mut self, t: Ty, index: usize) -> StageResult<bool> {
        let TyData::Adt { def, .. } = self.pool.get(t) else {
            return Ok(true);
        };
        if let Some(at) = self.container_item(def) {
            return Ok(at == index);
        }
        Ok(self.impls.by_structure(self.pool, self.known.own_only, t) || (self.written)(t)?)
    }

    /// The item argument of a built-in container: a list's element, a
    /// map's value (`own.structure.builtin`).
    fn container_item(&self, def: DefId) -> Option<usize> {
        if def == self.known.list {
            Some(0)
        } else if def == self.known.map {
            Some(1)
        } else {
            None
        }
    }

    /// The summary of `def`, the least fixed point over every type that
    /// declares `OwnOnly` by structure and that `def`'s fields reach
    /// (`own.structure.recursive`).
    fn summary(&mut self, def: DefId) -> StageResult<Sum> {
        if let Some(s) = self.memo.done.get(&def) {
            return Ok(s.clone());
        }
        let mut work: Vec<(DefId, Sum)> = vec![(def, Sum::default())];
        loop {
            let mut changed = false;
            let mut i = 0;
            while i < work.len() {
                let d = work[i].0;
                let mut sum = Sum::default();
                for f in self.field_types(d) {
                    let s = self.sum_ty(d, f, &mut work)?;
                    sum.join(&s);
                }
                if sum != work[i].1 {
                    work[i].1 = sum;
                    changed = true;
                }
                i += 1;
            }
            if !changed {
                break;
            }
        }
        for (d, s) in work {
            self.memo.done.insert(d, s);
        }
        Ok(self.memo.done.get(&def).cloned().unwrap_or_default())
    }

    /// The field and payload types of a data type or an enum.
    fn field_types(&self, def: DefId) -> Vec<Ty> {
        match self.lookup.item(def).map(|i| &i.data) {
            Some(ItemData::Data(fs)) => fs.iter().map(|f| f.ty).collect(),
            Some(ItemData::Enum { variants, .. }) => variants
                .iter()
                .flat_map(|v| v.fields.iter().map(|f| f.ty))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// When a field type `t` of `owner` owns, in terms of `owner`'s type
    /// parameters, reading the current summaries in `work`.
    fn sum_ty(&mut self, owner: DefId, t: Ty, work: &mut Vec<(DefId, Sum)>) -> StageResult<Sum> {
        let pool = self.pool;
        let mut out = Sum::default();
        match pool.get(t) {
            TyData::Own(_) => out.always = true,
            TyData::Mut(i) | TyData::Option(i) => out = self.sum_ty(owner, i, work)?,
            TyData::Tuple { elems, rest } => {
                for &e in pool.list_items(elems).iter().chain(rest.iter()) {
                    let s = self.sum_ty(owner, e, work)?;
                    out.join(&s);
                }
            }
            TyData::Param(p) if p.owner == owner => out.via.push(p.index),
            TyData::Adt { def, args } => {
                let args = pool.list_items(args);
                let inner = if let Some(at) = self.container_item(def) {
                    Some(Sum {
                        always: false,
                        via: u16::try_from(at).into_iter().collect(),
                    })
                } else if self.impls.by_structure(pool, self.known.own_only, t) {
                    Some(if let Some(s) = self.memo.done.get(&def) {
                        s.clone()
                    } else if let Some((_, s)) = work.iter().find(|(d, _)| *d == def) {
                        s.clone()
                    } else {
                        work.push((def, Sum::default()));
                        Sum::default()
                    })
                } else {
                    None
                };
                match inner {
                    Some(s) => {
                        out.always = s.always;
                        for v in s.via {
                            if let Some(&a) = args.get(usize::from(v)) {
                                let s = self.sum_ty(owner, a, work)?;
                                out.join(&s);
                            }
                        }
                    }
                    None => out.always = (self.written)(t)?,
                }
            }
            TyData::TraitValue { .. } => out.always = (self.written)(t)?,
            _ => {}
        }
        Ok(out)
    }
}

/// Whether a field type has an `own` or `^mut` part that the holder holds
/// directly: through `?`, `mut` and tuple elements, not inside another
/// type's arguments, a shared borrow, or a function type
/// (`own.own-only.field`).
pub(crate) fn exclusive_part(pool: Types<'_>, t: Ty) -> bool {
    match pool.get(t) {
        TyData::Own(_) | TyData::Borrow { mutable: true, .. } => true,
        TyData::Mut(i) | TyData::Option(i) => exclusive_part(pool, i),
        TyData::Tuple { elems, rest } => pool
            .list_items(elems)
            .iter()
            .chain(rest.iter())
            .any(|&e| exclusive_part(pool, e)),
        _ => false,
    }
}

/// The `D[args]` parts of `t` and the indices of their `own` arguments
/// (`own.structure.admission`); a function type's inputs and result
/// count, a trait value's arguments do not.
pub(crate) fn owned_arguments(pool: Types<'_>, t: Ty) -> Vec<(Ty, usize)> {
    let mut all = Vec::new();
    parts(pool, t, &mut all);
    let mut out = Vec::new();
    for p in all {
        if let TyData::Adt { args, .. } = pool.get(p) {
            for (i, &a) in pool.list_items(args).iter().enumerate() {
                if matches!(pool.get(a), TyData::Own(_)) {
                    out.push((p, i));
                }
            }
        }
    }
    out
}

/// Whether `t` holds an owned value outside any borrow: an owned type, or
/// an optional, tuple, or type argument that holds one. A value of such a
/// type may need a drop (the TIR's `DropLocal`, invariant 18).
pub(crate) fn holds_owned(pool: Types<'_>, t: Ty) -> bool {
    if !pool.has_mode(t) {
        return false;
    }
    match pool.get(t) {
        TyData::Own(_) => true,
        TyData::Option(i) | TyData::Mut(i) => holds_owned(pool, i),
        TyData::Tuple { elems, rest } => pool
            .list_items(elems)
            .iter()
            .chain(rest.iter())
            .any(|&e| holds_owned(pool, e)),
        TyData::Adt { args, .. } => pool.list_items(args).iter().any(|&a| holds_owned(pool, a)),
        _ => false,
    }
}
