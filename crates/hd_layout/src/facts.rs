//! Type facts the ownership passes and the optimizer read from a type's
//! layout: whether it is counted, a reference's target and ownership,
//! the parts a value holds in place, and the referents a promoted copy
//! may take by value.

use hd_base::StageResult;
use hd_mono::ProgramEnv;
use hd_tir::ir::Proj;
use hd_types::{InternPool, Ty, TyData};

use crate::{Kind, kind};

/// Concrete values have no implicit owned-address class: recursive links
/// are explicit references, whose loan convention keeps their concrete keys.
#[must_use]
pub fn shared_reference_layout(_pool: &InternPool, _env: &dyn ProgramEnv, _ty: Ty) -> bool {
    false
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
    crate::holds_drop(pool, env, ty) || crate::tracked(pool, env, ty)
}

/// Whether the value's representation is one counted address at word zero.
#[must_use]
pub fn single_counted_address_type(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> bool {
    crate::vals(pool, env, ty).is_ok_and(|vs| vs == [crate::Val::I32])
        && crate::one_ref_first(pool, env, ty)
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
        Kind::Value => None,
    }
}

/// Whether every counted value of `ty` the ownership passes track is a
/// local reference's, thin or a `&dyn`, which counts nothing, with no
/// `std.rt.Drop` to run.
#[must_use]
pub fn plain_reference_type(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> bool {
    !crate::holds_drop(pool, env, ty)
        && crate::tracked(pool, env, ty)
        && !crate::counted(pool, env, ty)
}

/// Concrete layout fact: copying this value counts a collection's buffer.
#[must_use]
pub fn copies_collection_type(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    crate::coll_kind(pool, env, t).is_some() || hd_mono::holds_coll(pool, env, t)
}

/// A reference's ownership from one normalized layout query: None for a
/// non-reference, false for a local loan, true for a counted owner.
#[must_use]
pub fn reference_ownership(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> Option<bool> {
    match kind(pool, env, ty) {
        Kind::Ref { whole, .. } => Some(whole),
        Kind::Value => None,
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
    lent_referent(pool, env, t).filter(|&target| !crate::tracked(pool, env, target))
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

/// The part [`place_part`] names: its type, the root's values that hold
/// it, and per enum on the way whose niche reads 0 as another variant
/// than the one stepped into, (its tag's slot in the root, the enum, the
/// variant, its tag value in the enum, the niche's last valid value).
pub struct PlacePart {
    pub ty: Ty,
    pub slots: Vec<usize>,
    niches: Vec<(usize, Ty, u32, u32, u32)>,
}

/// The values a move zeroes, then each (value, repair) after the zeros
/// ([`PlacePart::empty_pattern`]).
pub type EmptyPattern = (Vec<usize>, Vec<(usize, Option<u32>)>);

/// The part at projection `proj` of a value of type `t` laid out in its
/// locals, through fields and variants' payload fields, or `None` where a
/// step leaves the locals: a dereference, an element, a reference's or a
/// boxed enum's part. O(d) cached layout probes for d steps.
pub fn place_part(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    t: Ty,
    proj: &[Proj],
) -> StageResult<Option<PlacePart>> {
    let slots = (0..crate::vals(pool, env, t)?.len()).collect();
    let mut part = PlacePart {
        ty: t,
        slots,
        niches: Vec::new(),
    };
    let mut rest = proj;
    while !rest.is_empty() {
        let ty = part.ty;
        let newtype = (pool.adt(ty)).and_then(|(def, args)| {
            Some(hd_mono::subst(pool, env, def, args, env.newtype_base(def)?))
        });
        let value = matches!(kind(pool, env, ty), Kind::Value);
        let payload = match rest {
            [Proj::Variant(_), ..] => {
                crate::flat(pool, env, ty)?.zip(crate::variants(pool, env, ty))
            }
            _ => None,
        };
        rest = match (rest, newtype, value, payload) {
            // A newtype's one field is all of its values, whatever it holds.
            ([Proj::Field(0), tail @ ..], Some(base), _, _) => {
                part.ty = base;
                tail
            }
            ([Proj::Field(f), tail @ ..], _, true, _) => {
                let members = crate::members(pool, env, ty)?;
                let at = members.parts[*f as usize];
                part.slots.drain(at.first + at.count..);
                part.slots.drain(..at.first);
                part.ty = members.tys[*f as usize];
                tail
            }
            ([Proj::Variant(v), Proj::Field(f), tail @ ..], _, true, Some((flat, vs))) => {
                let (v, f) = (*v, *f as usize);
                if let Some(d) = flat.cell()
                    && let crate::Enc::Niche { start, bytes, .. } = d.enc
                    && d.enc.reads(0) != v
                {
                    let last = start.wrapping_sub(1) & (u32::MAX >> (32 - 8 * u32::from(bytes)));
                    part.niches
                        .push((part.slots[d.value as usize], ty, v, d.value, last));
                }
                let at = &flat.positions[v as usize][f];
                part.slots = at.iter().map(|&k| part.slots[k as usize]).collect();
                part.ty = vs[v as usize][f];
                tail
            }
            _ => return Ok(None),
        };
    }
    Ok(Some(part))
}

impl PlacePart {
    /// The empty pattern a move out of this part of a `t` value leaves:
    /// zeros in the values that overlap the part's (other variants' too,
    /// so no copy of a moved counted address stays), then the repairs
    /// that keep each enum on the way reading its variant. A repaired
    /// cell the variant counts gets the static empty buffer (`None`),
    /// any other its niche's last valid value. One overlap pass over
    /// `t`'s leaves, then a binary search of the sorted zeros per enum.
    pub fn empty_pattern(
        &self,
        pool: &InternPool,
        env: &dyn ProgramEnv,
        t: Ty,
    ) -> StageResult<EmptyPattern> {
        let values = crate::mem(pool, env, t)?.leaves;
        let part: Vec<crate::Leaf> = self.slots.iter().map(|&k| values[k]).collect();
        let zeroed = crate::overlapping(&values, &part);
        let mut repairs = Vec::new();
        for &(slot, ty, v, value, last) in &self.niches {
            if zeroed.binary_search(&slot).is_err() {
                continue;
            }
            // The part needs a drop when its variant holds a reference
            // there, under a test of the enum's cell.
            let counted = crate::ref_leaves(pool, env, ty).iter().any(|l| {
                l.value == value
                    && l.guards
                        .first()
                        .is_some_and(|&(_, arm, _)| arm == v as usize)
            });
            repairs.push((slot, (!counted).then_some(last)));
        }
        Ok((zeroed, repairs))
    }
}
