//! Type facts the ownership passes and the optimizer read from a type's
//! layout: whether it is counted, a reference's target and ownership,
//! the parts a value holds in place, and the referents a promoted copy
//! may take by value.

use hd_mono::ProgramEnv;
use hd_types::{InternPool, Ty, TyData};

use crate::{Kind, kind};

/// A1's shared owned-address class on this backend. Boxed values have
/// the class's owned parameter convention. References keep concrete keys,
/// including when their layout is one address: local references borrow,
/// while counted references carry an owner.
#[must_use]
pub fn shared_reference_layout(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> bool {
    matches!(kind(pool, env, ty), Kind::Boxed)
}

/// The types a value of `ty` holds by value, `ty` stripped of newtypes
/// first: a tuple's or a union's members, a value enum's payload fields,
/// a struct's fields (`crate::value_parts`). A collection, a reference
/// or a scalar holds none in place.
#[must_use]
pub fn value_part_types(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> Vec<Ty> {
    crate::value_parts(pool, env, crate::strip_newtype(pool, env, ty))
}

/// Whether `ty` needs ownership tracking: a counted leaf or a Drop hook.
/// This answers the ownership passes' `OwnCx::counted` query for a concrete type.
#[must_use]
pub fn counted_type(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> bool {
    crate::holds_drop(pool, env, ty) || crate::walk(pool, env, ty).is_some()
}

/// Whether the value's representation is one counted address at word zero.
#[must_use]
pub fn single_counted_address_type(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> bool {
    crate::vals(pool, env, ty).is_ok_and(|vs| vs == [crate::Val::I32])
        && crate::walk(pool, env, ty).and_then(|w| w.single()) == Some(0)
}

/// Whether the concrete type is a thin counted reference to a whole object.
#[must_use]
pub fn thin_counted_reference_type(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> bool {
    matches!(
        kind(pool, env, ty),
        Kind::Ref {
            whole: true,
            fat: false,
            ..
        }
    )
}

/// A reference's concrete target type, including counted and trait references.
#[must_use]
pub fn reference_target_type(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> Option<Ty> {
    match kind(pool, env, ty) {
        Kind::Ref { target, .. } => Some(target),
        _ => None,
    }
}

/// Whether every counted value of `ty` the ownership passes track is a
/// local reference's, thin or a `&dyn`, which counts nothing, with no
/// `std.rt.Drop` to run.
#[must_use]
pub fn plain_reference_type(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> bool {
    !crate::holds_drop(pool, env, ty)
        && crate::walk(pool, env, ty).is_some()
        && crate::count_walk(pool, env, ty).is_none()
}

/// Concrete layout fact: copying this value runs collection copy glue.
#[must_use]
pub fn copies_collection_type(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    crate::coll_kind(pool, env, t).is_some() || !crate::coll_leaves(pool, env, t).is_empty()
}

/// A reference's ownership from one normalized layout query: None for a
/// non-reference, false for a local loan, true for a counted owner.
#[must_use]
pub fn reference_ownership(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> Option<bool> {
    match kind(pool, env, ty) {
        Kind::Ref { whole, .. } => Some(whole),
        _ => None,
    }
}

/// Whether a same-value coercion from `from` to `to` lends a thin counted
/// reference as a thin local one (`ref.convert.lend`): one address, which
/// the ownership passes' `OwnCx::lends` treats as no consumer.
#[must_use]
pub fn lending_types(pool: &InternPool, env: &dyn ProgramEnv, from: Ty, to: Ty) -> bool {
    thin_counted_reference_type(pool, env, from)
        && matches!(
            kind(pool, env, to),
            Kind::Ref {
                whole: false,
                fat: false,
                ..
            }
        )
}

/// The parts of a value of `ty`, which names no parameter, laid out in
/// this backend's locals (`Kind::Value`), into `out`: a newtype's
/// one field, a struct's fields, a tuple's members, or the payload of
/// variant `variant` of an enum that does not contain itself. False for
/// any other type, whose parts no `move` or `Drop` can take one by one:
/// a scalar, a string, a list, a map, a trait or function value, a
/// reference, a recursive enum. The ownership passes'
/// `OwnCx::parts`.
pub fn in_place_parts(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    ty: Ty,
    variant: Option<u32>,
    out: &mut Vec<Ty>,
) -> bool {
    out.clear();
    let t = ty;
    if let TyData::Adt { def, args } = pool.get(t)
        && env.newtype_base(def).is_some()
    {
        let Some(&base) = env.data_fields(def).unwrap_or_default().first() else {
            return false;
        };
        if variant.is_some() {
            return false;
        }
        out.push(hd_mono::subst(pool, env, def, args, base));
        return true;
    }
    if !matches!(kind(pool, env, t), Kind::Value) {
        return false;
    }
    let s = crate::strip_newtype(pool, env, t);
    if let TyData::Tuple { .. } = pool.get(s) {
        out.extend(crate::tuple_members(pool, s));
        return variant.is_none();
    }
    if let Some(vs) = crate::variants(pool, env, s) {
        let Some(fs) = variant.and_then(|v| vs.get(v as usize)) else {
            return false;
        };
        out.extend(fs.iter().copied());
        return true;
    }
    match pool.get(s) {
        TyData::Adt { def, args }
            if variant.is_none()
                && env.std_kind(def) == hd_mono::layout::StdKind::Other
                && env.is_struct(def) =>
        {
            let Some(fs) = env.data_fields(def) else {
                return false;
            };
            out.extend(
                fs.into_iter()
                    .map(|f| hd_mono::subst(pool, env, def, args, f)),
            );
            true
        }
        _ => false,
    }
}

/// Classifies all variants of an in-place enum in one layout query.
/// Each flag says that the variant has no counted field. As with
/// `in_place_parts(_, Some(_), _)`, newtypes and boxed layouts do not
/// expose variants. The caller excludes open types and runtime Drop hooks.
pub fn in_place_empty_variants(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    ty: Ty,
    out: &mut Vec<bool>,
    mut counted: impl FnMut(Ty) -> bool,
) {
    out.clear();
    if matches!(pool.get(ty), TyData::Adt { def, .. } if env.newtype_base(def).is_some())
        || !matches!(kind(pool, env, ty), Kind::Value)
    {
        return;
    }
    if let Some(variants) = crate::variants(pool, env, ty) {
        out.extend(
            variants
                .iter()
                .map(|fields| !fields.iter().copied().any(&mut counted)),
        );
    }
}

/// The referent of `t` when a reference of type `t` may be passed by
/// value (§23.40): a thin, read-only reference to a value held in
/// locals with no counted leaf and at most four scalar values, such as a
/// number, a `bool`, a `char`, or a small struct or tuple of them.
#[must_use]
pub fn promotable_referent(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<Ty> {
    lent_referent(pool, env, t).filter(|&target| crate::walk(pool, env, target).is_none())
}

/// [`promotable_referent`], counted parts allowed: the referent of a
/// reference a promoted copy may take by value, lent with no count
/// (§23.40), such as a `string` or a small struct holding one.
#[must_use]
pub fn lent_referent(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<Ty> {
    small_referent(pool, env, t).filter(|&r| !crate::holds_drop(pool, env, r))
}

/// The small referent of `t`, as [`lent_referent`] gives it, that holds
/// a value with a `std.rt.Drop` (a `Buffer`, a `UniqList`): a value no
/// copy may take but where nothing can drop it while the copy reads it,
/// through its caller's reference parameter (`hd_opt::inline`).
#[must_use]
pub fn rooted_referent(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<Ty> {
    small_referent(pool, env, t).filter(|&r| crate::holds_drop(pool, env, r))
}

fn small_referent(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<Ty> {
    let Kind::Ref {
        target, fat: false, ..
    } = kind(pool, env, t)
    else {
        return None;
    };
    if crate::ref_mutable(pool, env, t) || !matches!(kind(pool, env, target), Kind::Value) {
        return None;
    }
    let n = crate::vals(pool, env, target).ok()?.len();
    if !(1..=4).contains(&n) {
        return None;
    }
    // The layout's target has its newtypes erased; the promoted value keeps
    // the declared referent, so a path through it steps into a newtype's
    // one field, as `i32(*m)` with `m: &Meters` does.
    Some(declared_referent(pool, env, t).unwrap_or(target))
}

/// The type a reference type `t` refers to, as declared: newtypes kept.
fn declared_referent(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Option<Ty> {
    let t = crate::strip_newtype(pool, env, t);
    match pool.get(t) {
        TyData::Ref { inner, .. } => Some(inner),
        _ => None,
    }
}
