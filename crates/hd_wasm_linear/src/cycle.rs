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

use hd_base::{DefId, HashSet};
use hd_mono::ProgramEnv;
use hd_mono::layout::StdKind;
use hd_types::{InternPool, Ty, TyData};

use crate::layout::{Kind, kind, strip, tuple_members, value_parts, variants};

thread_local! {
    /// The link's closed-world set of types whose existing objects may be
    /// written. `None` keeps every reference edge conservatively capable.
    static MUTATION_TARGETS: std::cell::RefCell<Option<HashSet<Ty>>> =
        const { std::cell::RefCell::new(None) };
}

/// Sets the closed-world mutation fact until the returned guard drops.
pub(crate) fn mutation_targets(targets: Option<HashSet<Ty>>) -> impl Drop {
    struct Reset(Option<HashSet<Ty>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            MUTATION_TARGETS.set(self.0.take());
        }
    }
    Reset(MUTATION_TARGETS.replace(targets))
}

fn writable(pool: &InternPool, env: &dyn ProgramEnv, target: Ty) -> bool {
    let target = strip(pool, env, target);
    MUTATION_TARGETS.with(|targets| {
        targets
            .borrow()
            .as_ref()
            .is_none_or(|targets| targets.contains(&target))
    })
}

/// Whether an object of type `t` takes part: a list's buffer,
/// a map's table, a data value's object or a self-recursive enum's cell.
pub fn object(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    let t = strip(pool, env, t);
    reaches(pool, env, &contents(pool, env, t))
}

/// Whether releasing an object of type `t` can recursively reach another
/// object with the same release glue. This is independent of whether the
/// objects participate in cycle collection.
pub fn recursive_object(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    let root = strip(pool, env, t);
    let mut work: Vec<(Ty, Vec<DefId>)> = contents(pool, env, root)
        .into_iter()
        .map(|t| (t, Vec::new()))
        .collect();
    while let Some((t, path)) = work.pop() {
        let t = strip(pool, env, t);
        if t == root {
            return true;
        }
        if hd_mono::is_open(pool, t) {
            continue;
        }
        let children = match pool.get(t) {
            TyData::Prim(_) | TyData::Never => Vec::new(),
            TyData::Tuple { .. } => tuple_members(pool, t),
            TyData::Option(inner) => vec![inner],
            TyData::Adt { def, args } => {
                // A declaration that reaches itself at growing arguments
                // (`E[T?]` in `E[T]`) is one finite release-glue edge per
                // concrete type, not recursion back to this glue.
                if path.contains(&def) {
                    continue;
                }
                match env.std_kind(def) {
                    StdKind::List | StdKind::Map | StdKind::Buffer => {
                        pool.list_items(args).to_vec()
                    }
                    StdKind::Suspend => Vec::new(),
                    StdKind::Other => match kind(pool, env, t) {
                        Kind::Ref { target, .. } => vec![target],
                        Kind::Data(fields) => fields,
                        Kind::Boxed => variants(pool, env, t)
                            .unwrap_or_default()
                            .into_iter()
                            .flatten()
                            .collect(),
                        Kind::Value => value_parts(pool, env, t),
                    },
                }
            }
            _ => Vec::new(),
        };
        let mut child_path = path;
        if let TyData::Adt { def, .. } = pool.get(t) {
            child_path.push(def);
        }
        for child in children {
            work.push((child, child_path.clone()));
        }
    }
    false
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

/// Whether a walk from `start` over the types they hold meets a reference
/// or an open value. An exactly repeated instantiated data type ends that
/// branch. A declaration repeated at different arguments is unfolded four
/// times; its fifth appearance is conservatively capable, because
/// polymorphic recursion can expose a reference only in a deeper instance.
fn reaches(pool: &InternPool, env: &dyn ProgramEnv, start: &[Ty]) -> bool {
    let mut work: Vec<(Ty, Vec<Ty>)> = start.iter().map(|t| (*t, Vec::new())).collect();
    while let Some((t, path)) = work.pop() {
        let t = strip(pool, env, t);
        if hd_mono::is_open(pool, t) {
            return true;
        }
        let children = match pool.get(t) {
            TyData::Prim(_) | TyData::Never => Vec::new(),
            TyData::Tuple { .. } => tuple_members(pool, t),
            TyData::Option(inner) => vec![inner],
            TyData::Adt { def, args } => {
                if path.contains(&t) {
                    continue;
                }
                // A declaration at growing arguments may expose a reference
                // in a parameter position only at a deeper instance. Four
                // finite nestings are still walked exactly; polymorphic
                // recursion necessarily reaches a fifth.
                if path
                    .iter()
                    .filter(|ancestor| {
                        matches!(pool.get(**ancestor), TyData::Adt { def: seen, .. } if seen == def)
                    })
                    .count()
                    >= 4
                {
                    return true;
                }
                match env.std_kind(def) {
                    StdKind::List | StdKind::Map | StdKind::Buffer => {
                        pool.list_items(args).to_vec()
                    }
                    StdKind::Suspend => return true,
                    StdKind::Other => match kind(pool, env, t) {
                        Kind::Ref { target, .. } if writable(pool, env, target) => return true,
                        Kind::Ref { target, .. } => vec![target],
                        Kind::Data(fields) => fields,
                        Kind::Boxed => variants(pool, env, t)
                            .unwrap_or_default()
                            .into_iter()
                            .flatten()
                            .collect(),
                        Kind::Value => value_parts(pool, env, t),
                    },
                }
            }
            _ => return true,
        };
        let mut child_path = path;
        if matches!(pool.get(t), TyData::Adt { .. }) {
            child_path.push(t);
        }
        for child in children {
            work.push((child, child_path.clone()));
        }
    }
    false
}
