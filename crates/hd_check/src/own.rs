//! The header rules of Ownership and Borrows (spec/lang/13-ownership.md)
//! that need the declarations a header names.

use hd_base::{DefId, PathId, Symbol};
use hd_diag::Code;
use hd_intern::PathKind;
use hd_resolve::{ItemData, Lookup, Names};
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
