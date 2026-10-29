//! The inliner (`docs/compiler/tir-v2.md`, "The inliner"): before the
//! ownership passes, each call of a small function is copied into its
//! caller, the callee's own calls copied first in a release build. It
//! rewrites TIR and reads the program only through [`InlineEnv`], which
//! the driver implements; [`bottom_up`] is the call-graph order a release
//! build owns bodies in, callees first.

use std::borrow::Cow;
use std::sync::{Mutex, MutexGuard, PoisonError};

use hd_base::{DefId, Hash128, HashMap, HashSet, StableHasher};
use hd_mono::ProgramEnv;
use hd_tir::inline::Site;
use hd_tir::ir::{Body, Callee, Ref, Tag, place};
use hd_types::{InternPool, RefCells, Ty, TyData, TyList};

/// The most instructions a callee's body may have for a call of it to
/// inline, in std and outside it alike (`docs/compiler/tir-v2.md`, "The
/// inliner", tuned at 16 over 8, 12, 16 and 20 in #856: 12 leaves
/// `spectral_norm`'s `a(i, j)` (15 instructions) a call, which V8's
/// baseline tier runs twice as slowly, and 20 only grows code); 0 turns
/// the inliner off.
pub const INLINE_LIMIT: usize = 16;

/// The most instructions a function marked `@inline` or
/// `@inline(.Auto)` may have and still be copied.
pub const AUTO_INLINE_LIMIT: usize = 64;

/// A declaration's inlining choice, decoded by the driver.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InlineHint {
    /// No decorator: use the compiler's ordinary profile-dependent rule.
    #[default]
    Default,
    /// Prefer a copy in every build, up to [`AUTO_INLINE_LIMIT`].
    Auto,
    /// Require a copy at every statically known call.
    Always,
    /// Keep every call as a call.
    Never,
}

/// What the inliner reads of the program beyond collection's view: the
/// driver's tables, its decoded bodies and the backend's layout facts.
pub trait InlineEnv: ProgramEnv {
    /// The run's type pool.
    fn pool(&self) -> &InternPool;
    /// The run's [`Memo`] in a release build; `None` in a dev build,
    /// which copies only std callees and remembers no copy.
    fn memo(&self) -> Option<&Memo>;
    /// The path of the module that declares `def`.
    fn module_text(&self, def: DefId) -> Cow<'_, str>;
    /// The declaration's inlining choice (`annot.inline.*`).
    fn inline_hint(&self, def: DefId) -> InlineHint;
    /// The declared parameter types of `def`, as written, whether it
    /// suspends, and whether it has a requirement row; `None` for a body
    /// without a signature (a module's init).
    fn signature(&self, def: DefId) -> Option<(Vec<Ty>, bool, bool)>;
    /// The checker's body of `def`, decoded once per run, and its hash.
    fn checked_body(&self, def: DefId) -> Option<&(Body, Hash128)>;
    /// A fresh decoding of the checker's body of `def`, as
    /// [`InlineEnv::checked_body`] gives it, for a caller to change.
    fn fresh_body(&self, def: DefId) -> Option<Body>;
    /// Whether a value of type `t`, which names no parameter, is a
    /// reference in the backend's layout.
    fn reference(&self, t: Ty) -> bool;
    /// The item and type arguments a trait call chosen as an impl runs;
    /// `None` for any other call.
    fn impl_target(&self, callee: &Callee) -> Option<(DefId, TyList)>;
    /// The impl of `trait_` at a concrete receiver and trait arguments, as
    /// collection selects it; `None` for an open type or a compiler-supplied
    /// implementation.
    fn select_impl(&self, trait_: DefId, self_ty: Ty, targs: TyList) -> Option<DefId>;
}

/// A release build's inliner state for one run. Only looked up, never
/// iterated, so output stays deterministic.
#[derive(Default)]
pub struct Memo {
    /// Per callee and type arguments, the copy a call of it inlines, its
    /// own calls copied first, and its hash, or `None` when a call of it
    /// stays (`callee_copy`).
    inlined: Mutex<HashMap<(DefId, TyList), Option<Inlined>>>,
    /// The functions the inliner found on a cycle of calls
    /// (`inline_into`): a call of one stays a call.
    recursive: Mutex<HashSet<DefId>>,
}

/// A copy a call inlines, and its hash ([`Memo`]).
type Inlined = (Body, Hash128);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Copies into `body` each call of a small function `callee_copy`
/// allows (`docs/compiler/tir-v2.md`, "The inliner"), before the passes,
/// each parameter bound by its pass mode, and returns the body's hash with
/// each copied callee's TIR hash.
pub fn inline_calls(env: &dyn InlineEnv, body: &mut Body, hash: Hash128) -> Hash128 {
    env.inline_into(body, hash, &mut vec![body.item])
}

impl dyn InlineEnv + '_ {
    /// Whether `t` names a type parameter, an associated type or an opaque
    /// type, which an instance decides.
    fn is_open(&self, t: Ty) -> bool {
        let types = self.pool().types();
        types.has_param(t) || types.has_assoc(t) || types.has_opaque(t)
    }

    /// [`inline_calls`] inside the copies on `stack`, the caller first: a
    /// call of one of them stays a call, so recursion never inlines.
    fn inline_into(&self, body: &mut Body, hash: Hash128, stack: &mut Vec<DefId>) -> Hash128 {
        let pool = self.pool();
        let mut k = StableHasher::new("inline");
        k.hash(hash);
        let (mut sites, mut hashes) = (Vec::new(), Vec::new());
        for c in (0..body.len()).rev() {
            let Some((def, targs)) =
                hd_tir::inline::known_call(body, c).and_then(|callee| match callee {
                    Callee::Item { def, targs } => Some((def, targs)),
                    Callee::TraitMethod { .. } => self.impl_target(&callee),
                })
            else {
                continue;
            };
            if self.inline_hint(def) == InlineHint::Default
                && hd_tir::inline::plain_call(body, c).is_none()
            {
                continue;
            }
            if let Some(at) = stack.iter().position(|&d| d == def) {
                self.found_cycle(&stack[at..]);
                continue;
            }
            let Some((mut copy, callee_hash)) = self.callee_copy(def, targs, stack) else {
                continue;
            };
            // A copy made for another caller may call one on the way down.
            if let Some(at) = stack.iter().position(|&d| self.calls(&copy, d)) {
                self.found_cycle(&stack[at..]);
                self.found_cycle(&[def]);
                continue;
            }
            if !self.fits_call(body, c, &mut copy) {
                continue;
            }
            sites.push(Site { call: c, copy });
            hashes.push(callee_hash);
        }
        // All of the body's copies in one rebuild.
        let copied = hd_tir::inline::inline_calls(body, sites).unwrap_or_default();
        for (_, &h) in copied.iter().zip(&hashes).filter(|(c, _)| **c) {
            k.hash(h);
        }
        if copied.contains(&true) {
            // A copy's types are its callee's with every opaque result
            // opened, as an instance has them; the caller's still name the
            // calls' opaque results. The caller opens its own too, so a
            // value crossing between the two (an argument a parameter's
            // `Store` binds, a result the root's `Break` carries, a value
            // the caller stores) has one type on both sides. One that does
            // not open stays as it is: no copy names it (`fits_call`).
            let ty = |t: Ty| hd_mono::reveal(pool, self, t).unwrap_or(t);
            let list = |l| {
                pool.list(
                    &pool
                        .list_items(l)
                        .iter()
                        .map(|&t| ty(t))
                        .collect::<Vec<_>>(),
                )
            };
            hd_tir::ir::map_body_types(body, &ty, &list);
            k.finish()
        } else {
            hash
        }
    }

    /// Records where `members`, a run of calls that reaches its first
    /// member again, must break in a release build. An all-`.Always`
    /// cycle is rejected by the package check and none is copied. A mixed
    /// cycle breaks at every function not marked `.Always`, so `.Always`
    /// calls on either side can still be copied.
    fn found_cycle(&self, members: &[DefId]) {
        if let Some(memo) = self.memo() {
            let breaks: Vec<_> = members
                .iter()
                .copied()
                .filter(|def| self.inline_hint(*def) != InlineHint::Always)
                .collect();
            lock(&memo.recursive).extend(if breaks.is_empty() {
                members.to_vec()
            } else {
                breaks
            });
        }
    }

    /// Whether `body` calls `def`, by name or through a trait method's
    /// impl.
    fn calls(&self, body: &Body, def: DefId) -> bool {
        (0..body.len()).any(|c| {
            hd_tir::inline::known_call(body, c)
                .and_then(|callee| match callee {
                    Callee::Item { def, .. } => Some(def),
                    Callee::TraitMethod { .. } => self.impl_target(&callee).map(|(d, _)| d),
                })
                .is_some_and(|d| d == def)
        })
    }

    /// The copy a call of `def` at `targs` inlines, at the call's types,
    /// and its hash (`inline_callee`). A memoized build inlines bottom-up:
    /// the instance's own calls are copied first, as far as their callees
    /// allow, and the limit applies to the result, so a function that only
    /// wraps a call of a leaf is a leaf itself once that call is a copy. A
    /// copy that outgrows the limit that way is the instance as written,
    /// when that fits. A function on a cycle of calls the walk finds stays
    /// a call. Each instance is made once per run. Dev builds memoize only
    /// explicitly decorated user functions; release builds also consider
    /// ordinary user functions.
    fn callee_copy(
        &self,
        def: DefId,
        targs: TyList,
        stack: &mut Vec<DefId>,
    ) -> Option<(Body, Hash128)> {
        let Some(memo) = self.memo() else {
            return (self.instance(def, targs))
                .and_then(|(copy, hash, fits)| fits.then_some((copy, hash)));
        };
        if let Some(made) = lock(&memo.inlined).get(&(def, targs)) {
            return made.clone();
        }
        if lock(&memo.recursive).contains(&def) {
            return None;
        }
        let made = self.instance(def, targs).and_then(|(copy, hash, fits)| {
            let nested = (!hd_tir::inline::leaf(&copy)).then(|| {
                let mut nested = copy.clone();
                stack.push(def);
                let h = self.inline_into(&mut nested, hash, stack);
                stack.pop();
                (nested, h)
            });
            if lock(&memo.recursive).contains(&def) {
                return None;
            }
            match nested {
                Some((nested, h)) if self.fits(def, targs, &nested) => Some((nested, h)),
                _ => fits.then_some((copy, hash)),
            }
        });
        lock(&memo.inlined).insert((def, targs), made.clone());
        made
    }

    /// The callee's body at the call's types, with its hash, and whether
    /// it inlines as written (`fits`); `None` when no copy of it can
    /// (`inline_callee`), or its types do not settle.
    fn instance(&self, def: DefId, targs: TyList) -> Option<(Body, Hash128, bool)> {
        let pool = self.pool();
        let (mut copy, hash) = self.inline_callee(def)?;
        let ty = |t: Ty| hd_mono::subst(pool, self, def, targs, t);
        let list = |l| {
            pool.list(
                &pool
                    .list_items(l)
                    .iter()
                    .map(|&t| ty(t))
                    .collect::<Vec<_>>(),
            )
        };
        hd_tir::ir::map_body_types(&mut copy, &ty, &list);
        // A projection left open names a type the caller's bounds may
        // fix, which the call's type already shows; such a copy waits.
        // An opaque result `subst` could not open (a trait method's at a
        // receiver no impl is selected for) left poison: the copy would
        // not know its own types, so the call stays.
        if copy
            .ty
            .iter()
            .chain(&copy.local_ty)
            .any(|&t| pool.has_assoc(t))
            || copy
                .ty
                .iter()
                .chain(&copy.local_ty)
                .chain(copy.consts.iter().map(|c| &c.0))
                .any(|&t| t == Ty::POISON)
        {
            return None;
        }
        hd_tir::inline::rechoose(&mut copy, &|t, s, a| self.select_impl(t, s, a));
        let fits = self.fits(def, targs, &copy);
        Some((copy, hash, fits))
    }

    /// Whether `copy`, the body of `def` at `targs`, may inline:
    /// The rule selected by `def`'s [`InlineHint`]: ordinary calls use
    /// [`INLINE_LIMIT`] and refuse reference parameters, `.Auto` uses
    /// [`AUTO_INLINE_LIMIT`] and accepts them, and `.Always` has neither
    /// limit. The checker has already rejected uncopyable `.Always`
    /// bodies.
    fn fits(&self, def: DefId, targs: TyList, copy: &Body) -> bool {
        match self.inline_hint(def) {
            InlineHint::Default => {
                hd_tir::inline::inlinable(copy, INLINE_LIMIT) && !self.takes_reference(def, targs)
            }
            InlineHint::Auto => hd_tir::inline::inlinable(copy, AUTO_INLINE_LIMIT),
            InlineHint::Always => hd_tir::inline::copyable(copy),
            InlineHint::Never => false,
        }
    }

    /// A fresh decoding of the checker's body of `def`, for the caller to
    /// change, and its hash, when a call of it may inline: a function
    /// outside `std.rt`, with a body of its own, not marked
    /// `@inline(.Never)` or `@cold` (`annot.inline.never`), and that does
    /// not suspend or call itself. Dev builds consider std functions and
    /// explicitly decorated user functions; release builds consider all
    /// user functions. `fits` applies the selected hint's size and
    /// reference policy.
    fn inline_callee(&self, def: DefId) -> Option<(Body, Hash128)> {
        let module = self.module_text(def);
        let std = module.starts_with("std.");
        let rt = module == "std.rt" || module.starts_with("std.rt.");
        let release = self.memo().is_some();
        let hint = self.inline_hint(def);
        let decorated = matches!(hint, InlineHint::Auto | InlineHint::Always);
        if (hint == InlineHint::Default && INLINE_LIMIT == 0)
            || rt
            || !(std || release || decorated)
            || self.intrinsic(def).is_some()
            || hint == InlineHint::Never
        {
            return None;
        }
        let (_, suspends, has_row) = self.signature(def)?;
        if suspends || has_row {
            return None;
        }
        let (body, hash) = self.checked_body(def)?;
        let body_fits = match hint {
            InlineHint::Default => hd_tir::inline::inlinable(body, INLINE_LIMIT),
            InlineHint::Auto => hd_tir::inline::inlinable(body, AUTO_INLINE_LIMIT),
            InlineHint::Always => hd_tir::inline::copyable(body),
            InlineHint::Never => false,
        };
        if !body_fits || self.calls(body, def) {
            return None;
        }
        Some((self.fresh_body(def)?, *hash))
    }

    /// Whether a parameter of `def` at `targs` is, or may be in an
    /// instance, a reference. A copy binds a parameter in a local, and a
    /// reference held in a local is counted, and makes a local it points
    /// at a counted cell, where a call lends both; so a callee that takes
    /// one stays a call (#856: copying `StringBuilder.push` into a loop
    /// made `string_build` 40% slower).
    fn takes_reference(&self, def: DefId, targs: TyList) -> bool {
        let Some((params, _, _)) = self.signature(def) else {
            return true;
        };
        let pool = self.pool();
        params.iter().any(|&t| {
            // As the backend reads a declared type: each reference type
            // lowered to `std.core.Ref[T]` or `std.core.Counted[T]`.
            let mut t = pool.lower_refs(self.ref_cells(), t);
            if self.is_open(t) {
                t = hd_mono::subst(pool, self, def, targs, t);
            }
            while let TyData::Mut(inner) = pool.get(t) {
                t = inner;
            }
            t == Ty::POISON
                || self.is_open(t)
                || matches!(pool.get(t), TyData::Ref { .. })
                || matches!(pool.get(t), TyData::Adt { def, .. } if self.ref_cells().holds(def))
                || self.reference(t)
        })
    }

    /// Whether `copy`, a callee's body at the types of call `c` of
    /// `caller`, takes the call's arguments and gives its result at the
    /// types they have once the caller's opaque results are opened
    /// (`inline_into`): each parameter's `Store` and the root's `Break`
    /// are then well typed. A `mut self` parameter of a type with no
    /// changeable copy of its own (a primitive, an enum, a collection) has
    /// type `mut T` while the call passes a `T`, a marker with `T`'s
    /// layout; its local takes the argument's type, as the call binds it.
    /// Any other difference keeps the call.
    fn fits_call(&self, caller: &Body, c: usize, copy: &mut Body) -> bool {
        let pool = self.pool();
        let open = |t: Ty| hd_mono::reveal(pool, self, t);
        let fits = |got: Option<Ty>, want: Ty| {
            got.is_some_and(|got| got == want || got == Ty::NEVER || want == Ty::NEVER)
        };
        let r = caller.record(caller.data[c][1]);
        let args = &r[..r.len().saturating_sub(3)];
        let params: Vec<usize> = (0..copy.local_ty.len())
            .filter(|&l| copy.local_flags[l] & hd_tir::ir::local_flags::PARAM != 0)
            .collect();
        let mut n = 0;
        for (l, &a) in params.into_iter().zip(args) {
            n += 1;
            // A `()` argument has no value; its `Store` stores none.
            if a == hd_tir::ir::NONE {
                continue;
            }
            let arg = if a & hd_tir::ir::Ref::CONST_BIT != 0 {
                caller
                    .consts
                    .get((a & !hd_tir::ir::Ref::CONST_BIT) as usize)
                    .map(|k| k.0)
            } else {
                caller.ty.get(a as usize).copied()
            };
            let Some(arg) = arg.and_then(open) else {
                return false;
            };
            let want = copy.local_ty[l];
            if matches!(pool.get(want), TyData::Mut(inner) if inner == arg)
                || lends_by_kind(pool, self.ref_cells(), arg, want)
            {
                copy.local_ty[l] = arg;
                keep_parameter_carrier(pool, self.ref_cells(), copy, l);
            } else if !fits(Some(arg), want) {
                return false;
            }
        }
        let root = copy.sub_root.first().map(|&r| copy.ty[r as usize]);
        n == args.len() && root.is_some_and(|root| fits(open(caller.ty[c]), root))
    }
}

/// Carries a rebound parameter's reference kind through copies already
/// inlined into `copy`. Bottom-up inlining may have bound a nested
/// `&mut self` to a read of the outer parameter before this caller made
/// that parameter counted. Its whole reads and the binding locals they
/// fill must follow the argument's carrier too; otherwise ownership sees
/// a `Ref` read of a `Counted` local and inserts a page-table count.
fn keep_parameter_carrier(pool: &InternPool, cells: RefCells, copy: &mut Body, param: usize) {
    let mut locals = vec![false; copy.local_ty.len()];
    locals[param] = true;
    let mut values = vec![false; copy.len()];
    let mut stores = vec![0u32; copy.local_ty.len()];
    for (data, &tag) in copy.data.iter().zip(&copy.tags) {
        let word = data[0];
        if tag == Tag::Store && word & place::BARE_LOCAL != 0 {
            stores[(word & !place::BARE_LOCAL) as usize] += 1;
        }
    }
    for i in 0..copy.len() {
        if let Some(local) = copy.local_read(i)
            && locals[local.idx()]
        {
            let carrier = copy.local_ty[local.idx()];
            if copy.ty[i] == carrier || lends_by_kind(pool, cells, carrier, copy.ty[i]) {
                copy.ty[i] = carrier;
                values[i] = true;
            }
        }
        if copy.tags[i] == Tag::Store {
            let [word, value] = copy.data[i];
            let local =
                (word & place::BARE_LOCAL != 0).then_some((word & !place::BARE_LOCAL) as usize);
            let source = Ref(value).as_inst().map(|v| v.0 as usize);
            if let (Some(local), Some(source)) = (local, source)
                && values.get(source) == Some(&true)
                && stores[local] == 1
            {
                let carrier = copy.ty[source];
                if copy.local_ty[local] == carrier
                    || lends_by_kind(pool, cells, carrier, copy.local_ty[local])
                {
                    copy.local_ty[local] = carrier;
                    locals[local] = true;
                }
            }
        }
    }
}

/// Whether argument type `arg` differs from parameter type `want` only by
/// reference kinds: a counted argument lends itself to a local reference
/// parameter (`ref.convert.lend`), so a copy's parameter local takes the
/// argument's carrier.
pub(crate) fn lends_by_kind(pool: &InternPool, cells: RefCells, arg: Ty, want: Ty) -> bool {
    let erase = |t| pool.lower_refs(cells.local_only(), pool.types().raise_refs(cells, t));
    arg != want && erase(arg) == erase(want)
}

/// Visits the call graph from `root` bottom-up with an iterative Tarjan
/// walk, so a deep call chain cannot overflow the stack: `scc` gets each
/// strongly connected component once every component its members call
/// has had its turn, with whether it is recursive (two members, or one
/// that calls itself). `done` marks the nodes an earlier walk finished,
/// which this one skips; `callees` gives a node's edges, once per node.
pub fn bottom_up<N: Copy + Eq + std::hash::Hash>(
    root: N,
    done: &dyn Fn(N) -> bool,
    callees: &mut dyn FnMut(N) -> Vec<N>,
    scc: &mut dyn FnMut(&mut Vec<N>, bool),
) {
    if done(root) {
        return;
    }
    // Per node: its index, its low link, and whether it is on the stack.
    let mut index: HashMap<N, (u32, u32, bool)> = HashMap::default();
    let (mut stack, mut next) = (Vec::new(), 0);
    let mut work: Vec<(N, Option<Vec<N>>, usize)> = vec![(root, None, 0)];
    while let Some((n, edges, at)) = work.last_mut() {
        let n = *n;
        let edges = edges.get_or_insert_with(|| {
            index.insert(n, (next, next, true));
            next += 1;
            stack.push(n);
            callees(n)
        });
        if let Some(&c) = edges.get(*at) {
            *at += 1;
            match index.get(&c) {
                None if !done(c) => work.push((c, None, 0)),
                Some(&(ci, _, true)) => {
                    let low = &mut index.get_mut(&n).expect("visited").1;
                    *low = (*low).min(ci);
                }
                _ => {}
            }
            continue;
        }
        let calls_itself = edges.contains(&n);
        work.pop();
        let (i, low, _) = index[&n];
        if let Some(&(parent, ..)) = work.last() {
            let up = &mut index.get_mut(&parent).expect("visited").1;
            *up = (*up).min(low);
        }
        if i == low {
            let from = stack.iter().rposition(|&m| m == n).expect("on the stack");
            let mut members: Vec<N> = stack.drain(from..).collect();
            for m in &members {
                index.get_mut(m).expect("visited").2 = false;
            }
            let recursive = members.len() > 1 || calls_itself;
            scc(&mut members, recursive);
        }
    }
}

#[cfg(test)]
mod tests {
    use hd_base::SliceSort as _;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use hd_base::DefId;
    use hd_tir::ir::{Body, BodyKind, ReadMode, Tag, local_flags, place};
    use hd_types::{InternPool, RefCells, Ty, TyData};

    use super::{bottom_up, keep_parameter_carrier};

    /// A counted parameter flows through the unique binding local of an
    /// already-inlined method. A local with another write stays local:
    /// changing it would make that unrelated write ill typed.
    #[test]
    fn parameter_carrier_follows_only_unique_binding_stores() {
        let pool = InternPool::new();
        let cells = RefCells {
            local: DefId::from_raw(70),
            counted: DefId::from_raw(71),
        };
        let local = pool.lower_refs(
            cells,
            pool.intern_ty(&TyData::Ref {
                inner: Ty::I32,
                mutable: true,
                counted: false,
            }),
        );
        let counted = pool.lower_refs(
            cells,
            pool.intern_ty(&TyData::Ref {
                inner: Ty::I32,
                mutable: true,
                counted: true,
            }),
        );
        let mut body = Body::new(DefId::from_raw(1), BodyKind::Fn);
        body.local_ty = vec![counted, local, local, local];
        body.local_flags = vec![local_flags::PARAM, 0, 0, 0];
        body.tags = vec![
            Tag::Read,
            Tag::Store,
            Tag::Read,
            Tag::Store,
            Tag::Read,
            Tag::Store,
        ];
        body.data = vec![
            [place::BARE_LOCAL, ReadMode::Use as u32],
            [place::BARE_LOCAL | 1, 0],
            [place::BARE_LOCAL, ReadMode::Use as u32],
            [place::BARE_LOCAL | 2, 2],
            [place::BARE_LOCAL | 3, ReadMode::Use as u32],
            [place::BARE_LOCAL | 2, 4],
        ];
        body.ty = vec![local, Ty::VOID, local, Ty::VOID, local, Ty::VOID];

        keep_parameter_carrier(&pool, cells, &mut body, 0);

        assert_eq!(body.ty[0], counted);
        assert_eq!(body.local_ty[1], counted);
        assert_eq!(body.ty[2], counted);
        assert_eq!(
            body.local_ty[2], local,
            "two writes keep the declared carrier"
        );
        assert_eq!(body.ty[4], local, "the unrelated read stays local");
    }

    /// Walks `edges` from each of `roots` in turn, as collection demands
    /// bodies, and gives each node a summary that reads its callees'
    /// outside its component, as the release passes will: per node, the
    /// summary and its component's sorted members and recursion flag.
    fn walk(edges: &[(u32, u32)], roots: &[u32]) -> BTreeMap<u32, (u64, Vec<u32>, bool)> {
        let out = RefCell::new(BTreeMap::<u32, (u64, Vec<u32>, bool)>::new());
        let callees =
            |n: u32| -> Vec<u32> { edges.iter().filter(|e| e.0 == n).map(|e| e.1).collect() };
        for &root in roots {
            let done = |n: u32| out.borrow().contains_key(&n);
            let mut scc = |members: &mut Vec<u32>, recursive: bool| {
                members.sort_stable();
                let mut out = out.borrow_mut();
                for &m in members.iter() {
                    let mut s = u64::from(m) + 1;
                    for c in callees(m).into_iter().filter(|c| !members.contains(c)) {
                        // Callees first: every outside callee is summarized.
                        s = s * 31 + out.get(&c).expect("callee first").0;
                    }
                    out.insert(m, (s, members.clone(), recursive));
                }
            };
            bottom_up(root, &done, &mut |n| callees(n), &mut scc);
        }
        out.into_inner()
    }

    /// The summaries do not depend on the order collection demands bodies
    /// in: a cycle, a self call, shared callees and an unreached node, from
    /// roots in three orders.
    #[test]
    fn summaries_do_not_depend_on_the_order_of_the_roots() {
        let edges = [
            (0, 1),
            (1, 2),
            (2, 1),
            (2, 3),
            (3, 3),
            (4, 0),
            (4, 3),
            (5, 2),
        ];
        let first = walk(&edges, &[0, 4, 5]);
        assert_eq!(first[&1].1, vec![1, 2]);
        assert!(first[&3].2 && !first[&0].2, "{first:?}");
        assert!(!first.contains_key(&6));
        for roots in [&[5, 4, 0][..], &[3, 5, 1, 4, 0], &[2, 0, 5, 4]] {
            assert_eq!(first, walk(&edges, roots), "roots {roots:?}");
        }
    }

    /// The walk is iterative: a chain of 200,000 calls does not overflow
    /// the stack, and is owned from its far end.
    #[test]
    fn a_deep_call_chain_does_not_overflow() {
        let edges: Vec<(u32, u32)> = (0..200_000).map(|n| (n, n + 1)).collect();
        let order = RefCell::new(Vec::new());
        let mut scc = |members: &mut Vec<u32>, _: bool| order.borrow_mut().extend(&*members);
        let callees = |n: u32| edges.get(n as usize).map(|e| vec![e.1]).unwrap_or_default();
        bottom_up(0, &|_| false, &mut |n| callees(n), &mut scc);
        let order = order.into_inner();
        assert_eq!(
            (order.len(), order[0], order[200_000]),
            (200_001, 200_000, 0)
        );
    }
}
