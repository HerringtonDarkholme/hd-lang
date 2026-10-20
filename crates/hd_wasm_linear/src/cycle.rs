//! Which objects take part in cycle collection
//! (`crates/hd_wasm_linear/DESIGN.md` §23.28,
//! `docs/future-work/REFCOUNT_RUNTIME.md` §7.1), decided per concrete type
//! when the object's allocation is emitted. An object that takes part has
//! [`crate::module::CYCLIC`] set in its descriptor word; any other is
//! released with a plain decrement and never becomes a candidate.
//!
//! An object can be on a cycle only if what it holds can reach, through
//! counted references, a value that can hold the object again. A walk
//! from the object's contents over the types they hold decides it:
//!
//! - A reference (`&T`, `&mut T`) ends the walk as capable: it may be a
//!   view into any object with a place of its target's type, so the type
//!   graph cannot say what it reaches.
//! - So does an open value: a function value (a closure may capture
//!   anything), a trait value, a context, a suspension, an erased body's
//!   open type, and any type this walk does not know.
//! - A list, a map, a data value or a self-recursive enum is an object of
//!   its own type: reaching the deciding object's own type is capable, and
//!   the walk goes on into what it holds.
//! - Scalars and strings hold nothing; a tuple, an option, a struct and a
//!   value enum are their parts.

use hd_base::HashSet;
use hd_mono::ProgramEnv;
use hd_mono::layout::StdKind;
use hd_types::{InternPool, Ty, TyData};

use crate::layout::{Kind, kind, strip, tuple_members, value_parts, variants};

/// Whether an object of type `t` takes part: a list's header and buffer,
/// a map's table, a data value's object or a self-recursive enum's cell.
pub fn object(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    let t = strip(pool, env, t);
    reaches(pool, env, Some(t), &contents(pool, env, t))
}

/// Whether an object holding values of `parts` takes part, when only
/// values of an open type can hold it: a closure's environment, a local's
/// cell, a trait value's or an open value's box, an iterator's
/// environment.
pub fn holding(pool: &InternPool, env: &dyn ProgramEnv, parts: &[Ty]) -> bool {
    reaches(pool, env, None, parts)
}

/// The types an object of type `t` holds.
fn contents(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Vec<Ty> {
    match pool.get(t) {
        TyData::Adt { def, args } if matches!(env.std_kind(def), StdKind::List | StdKind::Map) => {
            pool.list_items(args).to_vec()
        }
        TyData::Adt { .. } => match kind(pool, env, t) {
            Kind::Data(fields) => fields,
            Kind::Boxed => variants(pool, env, t)
                .unwrap_or_default()
                .into_iter()
                .flatten()
                .collect(),
            Kind::Ref { target, .. } => vec![target],
            Kind::Value => value_parts(pool, env, t),
        },
        _ => vec![t],
    }
}

/// Whether a walk from `start` over the types they hold meets a
/// reference, an open value, or an object of type `own`.
fn reaches(pool: &InternPool, env: &dyn ProgramEnv, own: Option<Ty>, start: &[Ty]) -> bool {
    let mut seen: HashSet<Ty> = HashSet::default();
    let mut work: Vec<Ty> = start.to_vec();
    while let Some(t) = work.pop() {
        let t = strip(pool, env, t);
        if !seen.insert(t) {
            continue;
        }
        if hd_mono::is_open(pool, t) {
            return true;
        }
        match pool.get(t) {
            TyData::Prim(_) | TyData::Never => {}
            TyData::Tuple { .. } => work.extend(tuple_members(pool, t)),
            TyData::Option(inner) => work.push(inner),
            TyData::Adt { def, args } => match env.std_kind(def) {
                StdKind::List | StdKind::Map => {
                    if own == Some(t) {
                        return true;
                    }
                    work.extend(pool.list_items(args).iter().copied());
                }
                StdKind::Suspend => return true,
                StdKind::Other => match kind(pool, env, t) {
                    Kind::Ref { .. } => return true,
                    Kind::Data(fields) => {
                        if own == Some(t) {
                            return true;
                        }
                        work.extend(fields);
                    }
                    Kind::Boxed => {
                        if own == Some(t) {
                            return true;
                        }
                        work.extend(
                            variants(pool, env, t)
                                .unwrap_or_default()
                                .into_iter()
                                .flatten(),
                        );
                    }
                    Kind::Value => work.extend(value_parts(pool, env, t)),
                },
            },
            _ => return true,
        }
    }
    false
}
