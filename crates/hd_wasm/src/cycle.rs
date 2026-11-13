//! Which objects take part in cycle collection
//! (`crates/hd_wasm/DESIGN.md` §23.28,
//! `docs/future-work/REFCOUNT_RUNTIME.md` §7.1), decided per concrete type
//! when the object's allocation is emitted. An object that takes part has
//! [`crate::module::CYCLIC`] set in its descriptor word; any other is
//! released with a plain decrement and never becomes a candidate.
//!
//! An object can be on a cycle only if what it holds can reach, through
//! counted references, a value that can hold the object again. A walk
//! from the object's contents over the types they hold decides it:
//!
//! - A reference to a type whose existing objects may be written ends
//!   the walk as capable: a write can add a back edge. Otherwise the
//!   walk follows the referent's contents. Without the link's closed-world
//!   mutation facts, every reference edge is conservatively capable.
//! - So does an open value: a function value (a closure may capture
//!   anything), a trait value, a context, a suspension, an erased body's
//!   open type, and any type this walk does not know.
//! - Value edges do not close a cycle. A freshly constructed value holds
//!   older values only. A write to a shared collection or value first
//!   copies its shared buffer/cell, so that write cannot add a back edge
//!   to the old object. Recursive value-only paths therefore stop at
//!   already visited types without marking them capable.
//! - Scalars and strings hold nothing; tuples, unions, structs, enums
//!   (`Option` as any other), lists and maps are walked through their
//!   parts.
//!
//! A closure literal is decided from its actual captures: a shared cell
//! keeps its environment capable even when the cell's value type is a
//! scalar. A copied/moved capture is walked as a value. An ordinary `fn`
//! field remains open and capable: no callee/capture summary is inferred.

use crate::cycle_graph::{Facts, Graph, Ranks, Shape};
use hd_base::HashSet;
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

thread_local! {
    static ANALYSIS: std::cell::RefCell<Option<Analysis>> = const { std::cell::RefCell::new(None) };
}

#[derive(Default)]
struct Analysis {
    graph: Graph,
    ranks: Ranks,
}

fn facts(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Facts {
    let t = strip(pool, env, t);
    let classify = |ty, ranks: &mut Ranks| {
        let (parts, value_open, object_open) = match holds(pool, env, ty) {
            Holds::Parts(parts) => (parts, false, false),
            Holds::Ref(target) => (vec![target], writable(pool, env, target), false),
            Holds::Open => (vec![], true, true),
        };
        Shape {
            parts: parts.into_iter().map(|t| strip(pool, env, t)).collect(),
            value_open,
            object_open,
            rank: ranks.get(pool, ty),
            declaration: match pool.get(ty) {
                TyData::Adt { def, .. } => Some(def),
                _ => None,
            },
        }
    };
    ANALYSIS.with(|analysis| {
        let mut analysis = analysis.borrow_mut();
        if let Some(Analysis { graph, ranks }) = analysis.as_mut() {
            graph.get(t, UNFOLDS, |ty| classify(ty, ranks))
        } else {
            let mut ranks = Ranks::default();
            Graph::default().get(t, UNFOLDS, |ty| classify(ty, &mut ranks))
        }
    })
}

/// Sets the closed-world mutation fact until the returned guard drops.
pub(crate) fn mutation_targets(targets: Option<HashSet<Ty>>) -> impl Drop {
    struct Reset(Option<HashSet<Ty>>, Option<Analysis>);
    impl Drop for Reset {
        fn drop(&mut self) {
            MUTATION_TARGETS.set(self.0.take());
            ANALYSIS.set(self.1.take());
        }
    }
    Reset(
        MUTATION_TARGETS.replace(targets),
        ANALYSIS.replace(Some(Analysis::default())),
    )
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

/// Whether a counted object of type `t` can participate in a reference cycle.
pub fn object(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    facts(pool, env, t).object
}

/// Whether release may reach the same glue again. This is independent of
/// cycle collection. Open values add no known edge; a growing family that
/// reaches the unfolding bound stays conservative, never proving that a
/// recursion-safe release path is unnecessary.
pub fn recursive_object(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    facts(pool, env, t).recursive
}

/// Whether an object holding values of `parts` takes part, when only
/// values of an open type can hold it: a closure's environment, a local's
/// cell, a trait value's or an open value's box, an iterator's
/// environment.
pub fn holding(pool: &InternPool, env: &dyn ProgramEnv, parts: &[Ty]) -> bool {
    parts.iter().any(|&t| facts(pool, env, t).value)
}

/// What a value of a stripped type holds, one rule for every type form:
/// a tuple its members, a union its members, an enum or struct its
/// payload fields (`Option` among them, as an ordinary enum), a list or
/// map its items; recursive links are explicit references.
enum Holds {
    /// Values of these types, in place or in an object it owns.
    Parts(Vec<Ty>),
    /// A reference to a place of this type.
    Ref(Ty),
    /// Anything: a function, trait or context value, a suspension, an
    /// erased body's open type, any type this walk does not know.
    Open,
}

fn holds(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Holds {
    match pool.get(t) {
        TyData::Prim(_) | TyData::Never => Holds::Parts(Vec::new()),
        TyData::Tuple { .. } => Holds::Parts(tuple_members(pool, t)),
        TyData::Union(members) => Holds::Parts(pool.list_items(members).to_vec()),
        TyData::Ref { inner, .. } => Holds::Ref(inner),
        TyData::Adt { def, args } => match env.std_kind(def) {
            StdKind::List | StdKind::Map => Holds::Parts(pool.list_items(args).to_vec()),
            StdKind::Suspend => Holds::Open,
            StdKind::Other if env.raw_buffer(def) => Holds::Parts(pool.list_items(args).to_vec()),
            StdKind::Other => match kind(pool, env, t) {
                Kind::Ref { target, .. } => Holds::Ref(target),
                Kind::Boxed => Holds::Parts(
                    variants(pool, env, t)
                        .unwrap_or_default()
                        .into_iter()
                        .flatten()
                        .collect(),
                ),
                Kind::Value => Holds::Parts(value_parts(pool, env, t)),
            },
        },
        _ => Holds::Open,
    }
}

/// The unfolding guard is indexed by declaration and structural type height.
/// Proper argument/member descent strictly lowers that rank. A family cannot
/// descend forever; non-decreasing re-entries reach this fixed bound.
const UNFOLDS: usize = 4;
