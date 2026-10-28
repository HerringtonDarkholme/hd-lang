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
//! - Value edges do not close a cycle. A freshly constructed value holds
//!   older values only. A write to a shared collection or value first
//!   copies its shared buffer/cell, so that write cannot add a back edge
//!   to the old object. Recursive value-only paths therefore stop at
//!   already visited types without marking them capable.
//! - Scalars and strings hold nothing; tuples, options, structs, enums,
//!   lists and maps are walked through their value parts.
//!
//! A closure literal is decided from its actual captures: a shared cell
//! keeps its environment capable even when the cell's value type is a
//! scalar. A copied/moved capture is walked as a value. An ordinary `fn`
//! field remains open and capable: no callee/capture summary is inferred.

use hd_base::HashSet;
use hd_mono::ProgramEnv;
use hd_mono::layout::StdKind;
use hd_types::{InternPool, Ty, TyData};

use crate::layout::{Kind, Walk, kind, strip, tuple_members, value_parts, variants};

/// The largest estimated body, in encoded Wasm bytes, for a release drain
/// specialized to one fixed-class type. The estimate charges every payload
/// leaf that the glue loads as well as each inlined known-child count path,
/// so scalar fields cannot make a selected body grow without bound. The two
/// nullable children of `binary_trees` fit, while a third child and wide serde
/// walks stay on the generic drain. The ceiling experiment for #939 measured
/// the two-child function at 227 bytes.
pub(crate) const TYPE_DRAIN_MAX_BYTES: usize = 256;

/// Whether `walk` is a compact walk of known payload starts, and therefore
/// has a bounded per-type drain under [`TYPE_DRAIN_MAX_BYTES`]. A reference
/// that needs `block_of`, or a tag-dispatched walk, keeps the generic path.
pub(crate) fn compact_drain(leaves: usize, walk: &Walk) -> bool {
    const BASE: usize = 32;
    const LEAF: usize = 12;
    const KNOWN: usize = 80;
    const NULLABLE: usize = 4;

    fn bytes(walk: &Walk) -> Option<usize> {
        match walk {
            Walk::Known(_) => Some(KNOWN),
            Walk::Seq(walks) => walks
                .iter()
                .try_fold(0usize, |n, walk| n.checked_add(bytes(walk)?)),
            Walk::Nullable(walk) => NULLABLE.checked_add(bytes(walk)?),
            Walk::Ref(_) | Walk::Switch(_, _) => None,
        }
    }

    leaves
        .checked_mul(LEAF)
        .and_then(|n| BASE.checked_add(n))
        .and_then(|n| n.checked_add(bytes(walk)?))
        .is_some_and(|n| n <= TYPE_DRAIN_MAX_BYTES)
}

/// Whether an object of type `t` takes part: a list's buffer,
/// a map's table, a data value's object or a self-recursive enum's cell.
pub fn object(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    let t = strip(pool, env, t);
    reaches(pool, env, &contents(pool, env, t))
}

/// Whether an object holding values of `parts` takes part, when only
/// values of an open type can hold it: a closure's environment, a local's
/// cell, a trait value's or an open value's box, an iterator's
/// environment.
pub fn holding(pool: &InternPool, env: &dyn ProgramEnv, parts: &[Ty]) -> bool {
    reaches(pool, env, parts)
}

/// The types an object of type `t` holds.
fn contents(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Vec<Ty> {
    match pool.get(t) {
        TyData::Adt { def, args }
            if matches!(
                env.std_kind(def),
                StdKind::List | StdKind::Map | StdKind::Buffer
            ) =>
        {
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
/// reference or an open value. Repeated value types end that branch.
fn reaches(pool: &InternPool, env: &dyn ProgramEnv, start: &[Ty]) -> bool {
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
                StdKind::List | StdKind::Map | StdKind::Buffer => {
                    work.extend(pool.list_items(args).iter().copied());
                }
                StdKind::Suspend => return true,
                StdKind::Other => match kind(pool, env, t) {
                    Kind::Ref { .. } => return true,
                    Kind::Data(fields) => {
                        work.extend(fields);
                    }
                    Kind::Boxed => {
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

#[cfg(test)]
mod tests {
    use super::compact_drain;
    use crate::layout::Walk;

    #[test]
    fn only_compact_known_child_walks_get_a_type_drain() {
        let binary_tree = Walk::Seq(vec![
            Walk::Nullable(Box::new(Walk::Known(0))),
            Walk::Nullable(Box::new(Walk::Known(1))),
        ]);
        assert!(compact_drain(2, &binary_tree));
        assert!(!compact_drain(
            3,
            &Walk::Seq(vec![Walk::Known(0), Walk::Known(1), Walk::Known(2),])
        ));
        assert!(!compact_drain(1, &Walk::Ref(0)));
        assert!(!compact_drain(
            2,
            &Walk::Switch(0, vec![Some(Walk::Known(1))],)
        ));
        assert!(!compact_drain(16, &Walk::Known(0)));
    }
}
