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
//! - Scalars and strings hold nothing; tuples, unions, structs, enums
//!   (`Option` as any other), lists and maps are walked through their
//!   parts.
//!
//! A closure literal is decided from its actual captures: a shared cell
//! keeps its environment capable even when the cell's value type is a
//! scalar. A copied/moved capture is walked as a value. An ordinary `fn`
//! field remains open and capable: no callee/capture summary is inferred.

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

/// Whether a counted object of type `t` can participate in a reference cycle.
pub fn object(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    let t = strip(pool, env, t);
    reaches(pool, env, &contents(pool, env, t))
}

/// Whether releasing an object of type `t` can recursively reach another
/// object with the same release glue. This is independent of whether the
/// objects participate in cycle collection. An open value is an edge the
/// walk cannot follow; it adds none.
pub fn recursive_object(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> bool {
    let root = strip(pool, env, t);
    let mut walk = Walk::new(contents(pool, env, root));
    while let Some((t, parent)) = walk.next(pool, env) {
        if t == root {
            return true;
        }
        let parts = match holds(pool, env, t) {
            Holds::Parts(parts) => parts,
            Holds::Ref(target) => vec![target],
            Holds::Open => continue,
        };
        // A declaration that reaches itself at growing arguments
        // (`E[T?]` in `E[T]`) past the unfolding bound is one finite
        // release-glue edge per concrete type, not recursion back to
        // this glue.
        if walk.unfolds(pool, parent, t) >= UNFOLDS {
            continue;
        }
        walk.push(pool, t, parent, parts);
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

/// The types an object of type `t` holds; an open `t` is its own content,
/// which [`reaches`] decides as open.
fn contents(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Vec<Ty> {
    match holds(pool, env, t) {
        Holds::Parts(parts) => parts,
        Holds::Ref(target) => vec![target],
        Holds::Open => vec![t],
    }
}

/// How many times one declaration may be re-entered at arguments that are
/// not part of an ancestor's before a walk stops unfolding it: polymorphic
/// recursion (`E[T?]` in `E[T]`) re-enters at growing arguments forever,
/// and may expose a reference only in a deeper instance.
const UNFOLDS: usize = 4;

/// A walk over the types values hold, each concrete type visited once.
/// Each data or reference type it expands is a node with its parent's
/// index, so the ancestors of a type are read back without copying a path.
///
/// Cost: one visit per distinct type reached. A visit reads its ancestors
/// (at most the walk's depth) and, for each one of the same declaration,
/// searches that ancestor's arguments once ([`occurs`]).
struct Walk {
    nodes: Vec<(Ty, usize)>,
    work: Vec<(Ty, usize)>,
    seen: HashSet<Ty>,
}

const NO_PARENT: usize = usize::MAX;

impl Walk {
    fn new(start: Vec<Ty>) -> Self {
        Walk {
            nodes: Vec::new(),
            work: start.into_iter().map(|t| (t, NO_PARENT)).collect(),
            seen: HashSet::default(),
        }
    }

    /// The next type not visited yet, stripped, and its parent node.
    fn next(&mut self, pool: &InternPool, env: &dyn ProgramEnv) -> Option<(Ty, usize)> {
        while let Some((t, parent)) = self.work.pop() {
            let t = strip(pool, env, t);
            if self.seen.insert(t) {
                return Some((t, parent));
            }
        }
        None
    }

    /// How many ancestors of `t` (from node `parent` up) are instances of
    /// `t`'s declaration that `t` is not part of. A type found inside an
    /// ancestor's arguments (`W[E]` in `W[W[E]]`) is smaller than it, so
    /// re-entering at it is finite and costs nothing; any other
    /// re-entry counts. Along an endless chain of distinct types some
    /// declaration recurs without shrinking, so the count bounds the walk.
    fn unfolds(&self, pool: &InternPool, mut parent: usize, t: Ty) -> usize {
        let TyData::Adt { def, .. } = pool.get(t) else {
            return 0;
        };
        let mut n = 0;
        while parent != NO_PARENT {
            let (ancestor, up) = self.nodes[parent];
            if matches!(pool.get(ancestor), TyData::Adt { def: seen, .. } if seen == def)
                && !occurs(pool, t, ancestor)
            {
                n += 1;
            }
            parent = up;
        }
        n
    }

    /// Queues `parts`, the types `t` holds, under `t`'s node when `t` is a
    /// data or reference type.
    fn push(&mut self, pool: &InternPool, t: Ty, parent: usize, parts: Vec<Ty>) {
        let at = if matches!(pool.get(t), TyData::Adt { .. } | TyData::Ref { .. }) {
            self.nodes.push((t, parent));
            self.nodes.len() - 1
        } else {
            parent
        };
        self.work.extend(parts.into_iter().map(|p| (p, at)));
    }
}

/// Whether `needle` occurs inside `hay`'s arguments, members or operands
/// (a proper part of it), each distinct part searched once.
fn occurs(pool: &InternPool, needle: Ty, hay: Ty) -> bool {
    let mut seen: HashSet<Ty> = HashSet::default();
    let mut stack = vec![hay];
    while let Some(t) = stack.pop() {
        let parts: Vec<Ty> = match pool.get(t) {
            TyData::Adt { args, .. } | TyData::TraitValue { args, .. } => {
                pool.list_items(args).to_vec()
            }
            TyData::Tuple { elems, rest } => {
                pool.list_items(elems).iter().copied().chain(rest).collect()
            }
            TyData::Union(members) => pool.list_items(members).to_vec(),
            TyData::Ref { inner, .. } => vec![inner],
            TyData::Fn { params, result, .. } => pool
                .list_items(params)
                .iter()
                .copied()
                .chain([result])
                .collect(),
            _ => Vec::new(),
        };
        for part in parts {
            if part == needle {
                return true;
            }
            if seen.insert(part) {
                stack.push(part);
            }
        }
    }
    false
}

/// Whether a walk from `start` over the types they hold meets a reference
/// that may be written, or an open value. Each concrete type is visited
/// once. A declaration re-entered [`UNFOLDS`] times at arguments not part
/// of an earlier instance's is conservatively capable, because
/// polymorphic recursion can expose a reference only in a deeper instance;
/// nesting that shrinks (`W[W[E]]` to `W[E]`) is walked exactly, for every
/// declaration alike.
fn reaches(pool: &InternPool, env: &dyn ProgramEnv, start: &[Ty]) -> bool {
    let mut walk = Walk::new(start.to_vec());
    while let Some((t, parent)) = walk.next(pool, env) {
        let parts = match holds(pool, env, t) {
            Holds::Open => return true,
            Holds::Ref(target) if writable(pool, env, target) => return true,
            Holds::Ref(target) => vec![target],
            Holds::Parts(parts) => parts,
        };
        if walk.unfolds(pool, parent, t) >= UNFOLDS {
            return true;
        }
        walk.push(pool, t, parent, parts);
    }
    false
}
