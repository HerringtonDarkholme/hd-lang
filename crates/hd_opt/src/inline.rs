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
use hd_tir::ir::{Body, Callee, CaptureMode, Ref, Tag, local_flags, place, ref_word};
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
    /// The declared result type of `def`, as written; `None` for a body
    /// without a signature.
    fn result(&self, def: DefId) -> Option<Ty>;
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
    /// The referent of a reference of type `t` that an inlined copy may
    /// take by value (`hd_wasm_linear::lent_referent`, counted parts
    /// allowed), in every build (#1151, #1167); `None` when the
    /// environment binds no copy by value.
    fn lent_referent(&self, _t: Ty) -> Option<Ty> {
        None
    }
    /// The referent of a reference of type `t` that holds a value with a
    /// `std.rt.Drop` (`hd_wasm_linear::rooted_referent`): an inlined copy
    /// never takes it by value; it keeps the reference in release, and in
    /// dev the call stays a call.
    fn rooted_referent(&self, _t: Ty) -> Option<Ty> {
        None
    }
    /// Whether the referent of a reference of type `t`, one
    /// [`InlineEnv::lent_referent`] gives, holds a counted part.
    fn counted_referent(&self, _t: Ty) -> bool {
        false
    }
    /// Whether a call of `callee` may write memory its caller reaches, from
    /// the declarations alone: the compiler's code of an intrinsic that
    /// only reads its arguments does not (`OwnCx::may_write` without the
    /// release summaries, which the inliner may run before).
    fn call_may_write(&self, _callee: &Callee) -> bool {
        true
    }
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
        let mut hash = hash;
        // A call through a function reference the body knows is a call
        // of its item, which may copy in turn; a copy may bring more.
        let release = self.memo().is_some();
        if release {
            self.direct_item_calls(body);
        }
        for _ in 0..4 {
            let (h, copied) = self.inline_round(body, hash, stack);
            hash = h;
            if !(copied && release && self.direct_item_calls(body)) {
                break;
            }
        }
        hash
    }

    /// Rewrites each `CallValue` whose callee is a function reference to
    /// an item (`ItemRef`), directly or through a local stored once with
    /// one, into a `Call` of that item (#1385): the value's code is the
    /// item's adapter, which only calls it. Not for a trait method's
    /// reference, an item that suspends or has a requirement row, whose
    /// calls pass more than the arguments. Whether any changed.
    fn direct_item_calls(&self, body: &mut Body) -> bool {
        if !body.tags.contains(&Tag::ItemRef) {
            return false;
        }
        let none = u32::MAX;
        // Per local: its stores, and the value of the last whole one.
        let mut sets = vec![(0u32, none); body.local_ty.len()];
        for i in 0..body.len() {
            if let Some((l, whole)) = body.stored_local(i) {
                let s = &mut sets[l.idx()];
                *s = (s.0 + 1, if whole { body.data[i][1] } else { none });
            }
            if body.tags[i] == Tag::Ref
                && let Some(l) = body.local_in_place(body.data[i][0])
            {
                sets[l.idx()].0 = 2;
            }
        }
        for (l, &f) in body.local_flags.iter().enumerate() {
            if f & local_flags::PARAM != 0 {
                sets[l].0 = 2;
            }
        }
        let mut changed = false;
        for i in 0..body.len() {
            if body.tags[i] != Tag::CallValue {
                continue;
            }
            let v = body.data[i][0];
            let made = match body.tags.get(v as usize) {
                Some(Tag::ItemRef) => v,
                Some(Tag::Read) => match body.view(body.data[v as usize][0]) {
                    p if p.whole() => match p.local().map(|l| sets[l as usize]) {
                        Some((1, x)) if body.tags.get(x as usize) == Some(&Tag::ItemRef) => x,
                        _ => continue,
                    },
                    _ => continue,
                },
                _ => continue,
            };
            let [a, w] = body.data[made as usize];
            let (Some(&d), Some(&targs)) = (body.record(a).first(), body.record(w).first()) else {
                continue;
            };
            let def = DefId::from_raw(d);
            let method_of_trait = self
                .parent(def)
                .is_some_and(|(owner, _)| self.impl_head(owner).is_none());
            if method_of_trait
                || self.suspends(def)
                || !matches!(self.signature(def), Some((_, false, false)))
            {
                continue;
            }
            let mut args = body.record(body.data[i][1]).to_vec();
            args.extend([0, 0, 0]);
            let callee = push_record(body, &[0, d, targs]);
            let args = push_record(body, &args);
            body.tags[i] = Tag::Call;
            body.data[i] = [callee, args];
            changed = true;
        }
        changed
    }

    /// One round of [`Self::inline_into`]: the body's hash with each copied
    /// callee's, and whether it copied any.
    fn inline_round(
        &self,
        body: &mut Body,
        hash: Hash128,
        stack: &mut Vec<DefId>,
    ) -> (Hash128, bool) {
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
            // Prefer taking a reader's referent by value when each
            // argument names a place the copy may read once. A small
            // release reader can otherwise keep the reference itself.
            let mut by_ref = 0;
            let by_value = if self.inline_hint(def) == InlineHint::Default
                && self.takes_reference(def, targs)
            {
                let may_write = |c: &Callee| self.call_may_write(c);
                let promote = self.by_value(def, targs, &copy);
                let (mask, counted, rooted) = promote.unwrap_or((0, 0, 0));
                // A referent that holds a `std.rt.Drop` keeps its
                // reference, as a small release reader may; in dev, where
                // none does, the call stays a call. A by-value binding of
                // it borrows only where the passes find nothing that
                // writes or moves around it; elsewhere they copy it, a
                // second owner, which they drop at the copy's end even
                // where a value read from it still lives
                // (`UniqList.swap`'s `self.buffer.at(left)` at `string`).
                let kept = mask & rooted;
                let mask = mask & !kept;
                let writes = hd_tir::own::writes_in(&copy, 0..copy.len(), &may_write);
                let mut args =
                    hd_tir::own::promote::inline_args(body, c, counted, writes, &may_write);
                let (references, bindings) =
                    if self.memo().is_some() && self.read_only_refs(def, targs, &copy) {
                        args |= captured_read_args(body, c);
                        (reference_read_args(body, c), weakened_read_args(body, c))
                    } else {
                        (0, 0)
                    };
                if promote.is_none() {
                    if self.memo().is_none() || !self.read_only_refs(def, targs, &copy) {
                        continue;
                    }
                    by_ref = self
                        .reference_params(def, targs, true)
                        .unwrap_or_default()
                        .iter()
                        .fold(0, |mask, (k, _)| mask | 1 << k);
                    0
                } else if args & mask != mask || kept != 0 {
                    // A Drop owner keeps its address; other promotable
                    // referents retain the existing call-site checks.
                    if mask & !(args | references | bindings) != 0
                        || self.memo().is_none()
                        || !self.read_only_refs(def, targs, &copy)
                    {
                        continue;
                    }
                    by_ref = kept | (mask & !args & !bindings);
                    mask & args
                } else {
                    mask
                }
            } else {
                0
            };
            if by_value != 0 {
                let referent = |t: Ty| {
                    self.lent_referent(t)
                        .or_else(|| self.rooted_referent(t))
                        .unwrap_or(t)
                };
                copy = hd_tir::own::promote::promote(&copy, by_value, referent);
            }
            sites.push(Site {
                call: c,
                copy,
                by_value,
                by_ref,
            });
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
            (k.finish(), true)
        } else {
            (hash, false)
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
    /// and its hash (`inline_callee`). A copy is built bottom-up:
    /// the instance's own calls are copied first, as far as their callees
    /// allow. The automatic limit applies to the result, so a function that
    /// only wraps a call of a leaf is a leaf itself once that call is a copy.
    /// An explicit hint applies its limit to the declaration as written,
    /// then honors explicit hints inside its copy even when they grow it.
    /// A function on a cycle of calls the walk finds stays a call. Each
    /// instance is made once per release run; a dev build
    /// makes one per explicit call site. Dev builds consider only explicitly
    /// decorated user functions; release builds also consider ordinary user
    /// functions.
    fn callee_copy(
        &self,
        def: DefId,
        targs: TyList,
        stack: &mut Vec<DefId>,
    ) -> Option<(Body, Hash128)> {
        let memo = self.memo();
        if let Some(memo) = memo {
            if let Some(made) = lock(&memo.inlined).get(&(def, targs)) {
                return made.clone();
            }
            if lock(&memo.recursive).contains(&def) {
                return None;
            }
        }
        let made = self.instance(def, targs).and_then(|(copy, hash, fits)| {
            let nested = (!hd_tir::inline::leaf(&copy)).then(|| {
                let mut nested = copy.clone();
                stack.push(def);
                let h = self.inline_into(&mut nested, hash, stack);
                stack.pop();
                (nested, h)
            });
            if memo.is_some_and(|memo| lock(&memo.recursive).contains(&def)) {
                return None;
            }
            match nested {
                Some((nested, h)) if fits && self.inline_hint(def) != InlineHint::Default => {
                    Some((nested, h))
                }
                Some((nested, h)) if self.fits(def, targs, &nested) => Some((nested, h)),
                _ => fits.then_some((copy, hash)),
            }
        });
        if let Some(memo) = memo {
            lock(&memo.inlined).insert((def, targs), made.clone());
        }
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
    /// [`INLINE_LIMIT`] and accept a getter's reference parameters
    /// ([`Self::takes_reference`]), promoted ones ([`Self::by_value`]),
    /// and small release readers that keep them ([`Self::read_only_refs`]); `.Auto` uses
    /// [`AUTO_INLINE_LIMIT`] and accepts them, and `.Always` has neither
    /// limit. The checker has already rejected uncopyable `.Always`
    /// bodies.
    fn fits(&self, def: DefId, targs: TyList, copy: &Body) -> bool {
        match self.inline_hint(def) {
            InlineHint::Default => {
                hd_tir::inline::inlinable(copy, INLINE_LIMIT)
                    && (!self.takes_reference(def, targs)
                        || self.by_value(def, targs, copy).is_some()
                        || (self.memo().is_some() && self.read_only_refs(def, targs, copy)))
            }
            InlineHint::Auto => hd_tir::inline::inlinable(copy, AUTO_INLINE_LIMIT),
            InlineHint::Always => hd_tir::inline::copyable(copy),
            InlineHint::Never => false,
        }
    }

    /// A fresh decoding of the checker's body of `def`, for the caller to
    /// change, and its hash, when a call of it may inline: a function
    /// with a body of its own, not marked
    /// `@inline(.Never)` or `@cold` (`annot.inline.never`), and that does
    /// not suspend or call itself. Dev builds consider std functions and
    /// explicitly decorated user functions; release builds consider all
    /// user functions. `fits` applies the selected hint's size and
    /// reference policy.
    fn inline_callee(&self, def: DefId) -> Option<(Body, Hash128)> {
        let module = self.module_text(def);
        let std = module.starts_with("std.");
        let release = self.memo().is_some();
        let hint = self.inline_hint(def);
        let decorated = matches!(hint, InlineHint::Auto | InlineHint::Always);
        if (hint == InlineHint::Default && INLINE_LIMIT == 0)
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
    /// instance, a reference, unless `def` is a reference getter. A copy
    /// binds a parameter in a local, and a reference held in a local was
    /// counted, and made a local it points at a counted cell, where a call
    /// lends both; so a callee that takes one stays a call (#856: copying
    /// `StringBuilder.push` into a loop made `string_build` 40% slower).
    /// A getter's result holds a local reference, which borrows from its
    /// one reference parameter (`ref.dep.elide`): the call already keeps
    /// that local alive over the result's uses, so the copy's binding
    /// points into a frame slot with no count
    /// (`crates/hd_wasm_linear/DESIGN.md` §23.38).
    fn takes_reference(&self, def: DefId, targs: TyList) -> bool {
        let Some(refs) = self.reference_params(def, targs, false) else {
            return true;
        };
        let getter = self
            .result(def)
            .is_some_and(|r| holds_local(self.pool(), self.ref_cells(), r, 0));
        refs.len() > 1 || (refs.len() == 1 && !getter)
    }

    /// The parameters a copy of `def` at `targs` takes by value (#1151;
    /// bit k: parameter k), in dev and release builds alike, when each of its reference parameters
    /// has a referent a copy may take by value (`lent_referent`) and
    /// `copy` only reads through it
    /// (`hd_tir::own::promote::read_only_params`). The copy then binds the
    /// referent's value, not the address, and no cell follows
    /// (`crates/hd_wasm_linear/DESIGN.md` §23.40). The binding is an
    /// ordinary read of the caller's place, before the passes, so a
    /// counted referent's count is theirs to decide: a temporary's moves
    /// into the copy, and a local that stays unchanged lends it. So a dev
    /// build copies std's `&self` readers as it copies its `self` ones
    /// (#1167): `part.len()` binds `part`'s value, which borrows, where a
    /// call would address the local and count it.
    ///
    /// With the mask, the parameters among them whose referents hold
    /// counted parts (`InlineEnv::counted_referent`), and those whose
    /// referents hold a `std.rt.Drop` value (`InlineEnv::rooted_referent`).
    fn by_value(&self, def: DefId, targs: TyList, copy: &Body) -> Option<(u64, u64, u64)> {
        let (mut mask, mut counted, mut rooted) = (0, 0, 0);
        for (k, t) in self.reference_params(def, targs, false)? {
            if k >= 64 {
                return None;
            }
            if self.lent_referent(t).is_some() {
                if self.counted_referent(t) {
                    counted |= 1 << k;
                }
            } else {
                // Kept a reference, or the call stays a call
                // ([`InlineEnv::rooted_referent`]).
                self.rooted_referent(t)?;
                rooted |= 1 << k;
                counted |= 1 << k;
            }
            mask |= 1 << k;
        }
        (mask != 0 && hd_tir::own::promote::read_only_params(copy, mask) == mask)
            .then_some((mask, counted, rooted))
    }

    /// A small release copy may keep its reference parameters when it
    /// only reads through them. Unlike promotion, this preserves each
    /// address and its lifetime, including an open or uncopyable referent.
    fn read_only_refs(&self, def: DefId, targs: TyList, copy: &Body) -> bool {
        let may_write = |callee: &Callee| self.call_may_write(callee);
        if hd_tir::own::writes_in(copy, 0..copy.len(), &may_write) {
            return false;
        }
        let Some(refs) = self.reference_params(def, targs, true) else {
            return false;
        };
        let shared = |ty| match self.pool().get(ty) {
            TyData::Ref { mutable, .. } => !mutable,
            TyData::Adt { def, args } if self.ref_cells().holds(def) => self
                .pool()
                .list_items(args)
                .first()
                .is_some_and(|&inner| !matches!(self.pool().get(inner), TyData::Mut(_))),
            _ => false,
        };
        // A mutable-reference parameter establishes its call loan even
        // if this particular body only reads it. Keep that boundary.
        if refs.iter().any(|(k, ty)| *k >= 64 || !shared(*ty)) {
            return false;
        }
        let mask = refs.iter().fold(0, |mask, (k, _)| mask | 1 << k);
        let params = hd_tir::own::params(copy);
        let parameter = |local| params.iter().position(|&p| p == local).filter(|&k| k < 64);
        let value_parameter = |value| {
            copy.local_read(value as usize)
                .and_then(|local| parameter(local.raw()))
        };
        let mut read = 0;
        for i in 0..copy.len() {
            let param = if copy.tags[i] == Tag::Read {
                let place = copy.view(copy.data[i][0]);
                if place.steps(copy).next() == Some(hd_tir::ir::Proj::Deref) {
                    match place.root {
                        hd_tir::ir::Root::Local(local) => parameter(local),
                        hd_tir::ir::Root::Value(value) => value_parameter(value),
                        _ => None,
                    }
                } else {
                    None
                }
            } else if copy.tags[i] == Tag::Match {
                value_parameter(copy.data[i][0])
            } else {
                None
            };
            if let Some(param) = param {
                read |= 1 << param;
            }
        }
        // An unused receiver still has its argument's loan/view cleanup
        // contract. This fallback requires an actual read through each
        // reference, rather than treating a constant body as a reader.
        mask != 0
            && read & mask == mask
            && hd_tir::own::promote::read_only_params(copy, mask) == mask
    }

    /// Each parameter of `def` at `targs` that is, or may be in an
    /// instance, a reference, with its type as the backend reads it;
    /// `None` when a parameter's type does not settle, except a declared
    /// reference may keep an open referent when it need not be promoted.
    fn reference_params(
        &self,
        def: DefId,
        targs: TyList,
        open_referents: bool,
    ) -> Option<Vec<(usize, Ty)>> {
        let (params, _, _) = self.signature(def)?;
        let pool = self.pool();
        let mut refs = Vec::new();
        for (k, &t) in params.iter().enumerate() {
            // As the backend reads a declared type: each reference type
            // lowered to `std.core.Ref[T]` or `std.core.Counted[T]`.
            let mut t = pool.lower_refs(self.ref_cells(), t);
            if self.is_open(t) {
                t = hd_mono::subst(pool, self, def, targs, t);
            }
            while let TyData::Mut(inner) = pool.get(t) {
                t = inner;
            }
            let reference = matches!(pool.get(t), TyData::Ref { .. })
                || matches!(pool.get(t), TyData::Adt { def, .. } if self.ref_cells().holds(def));
            if t == Ty::POISON || (self.is_open(t) && !(open_referents && reference)) {
                return None;
            }
            if reference || self.reference(t) {
                refs.push((k, t));
            }
        }
        Some(refs)
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
        let (mut n, mut rebound) = (0, false);
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
                rebound = true;
            } else if !fits(Some(arg), want) {
                return false;
            }
        }
        let root = copy.sub_root.first().map(|&r| copy.ty[r as usize]);
        n == args.len()
            && root.is_some_and(|root| fits(open(caller.ty[c]), root))
            && (!rebound || carriers_agree(copy))
    }
}

/// A read of an unchanged reference parameter retains a visible origin
/// after substitution: the ownership walk follows the read back to that
/// parameter. References held in fields or call results lack this proof.
/// Appends a record of `words` to `body`'s `extra`; its offset.
fn push_record(body: &mut Body, words: &[u32]) -> u32 {
    let at = u32::try_from(body.extra.len()).expect("extra");
    body.extra.push(u32::try_from(words.len()).expect("record"));
    body.extra.extend_from_slice(words);
    at
}

fn reference_read_args(body: &Body, call: usize) -> u64 {
    let words = body.record(body.data[call][1]);
    words[..words.len().saturating_sub(3)]
        .iter()
        .enumerate()
        .take(64)
        .filter(|&(_, &arg)| {
            let Some(i) = Ref(arg).as_inst().map(|v| v.0 as usize) else {
                return false;
            };
            if body.tags[i] != Tag::Read {
                return false;
            }
            let Some(local) = body.local_read(i) else {
                return false;
            };
            let flags = body.local_flags[local.idx()];
            flags & local_flags::PARAM != 0
                && flags & (local_flags::ASSIGNED | local_flags::CAPTURED) == 0
        })
        .fold(0, |mask, (k, _)| mask | 1 << k)
}

/// Weakening an address of an uncaptured local keeps its evaluated
/// reference. An ordinary inline binding retains that origin; directly
/// substituting the coercion would hide it behind a value-rooted place.
fn weakened_read_args(body: &Body, call: usize) -> u64 {
    if body.tags.iter().any(|t| {
        matches!(
            t,
            Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace | Tag::Hook
        )
    }) {
        return 0;
    }
    let words = body.record(body.data[call][1]);
    words[..words.len().saturating_sub(3)]
        .iter()
        .enumerate()
        .take(64)
        .filter(|&(_, &arg)| {
            let Some(i) = Ref(arg).as_inst().map(|v| v.0 as usize) else {
                return false;
            };
            body.tags[i] == Tag::Coerce
                && body.record(body.data[i][1]).first()
                    == Some(&(hd_tir::ir::Coercion::Weaken as u32))
                && Ref(body.data[i][0]).as_inst().is_some_and(|source| {
                    hd_tir::own::promote::promotable_ref(body, source.0 as usize, i)
                })
        })
        .fold(0, |mask, (k, _)| mask | 1 << k)
}

/// A single-use shared reference into an immutable value capture may
/// read that capture once instead. It is the closure's own snapshot,
/// rather than a cell another invocation can change.
fn captured_read_args(body: &Body, call: usize) -> u64 {
    if body.tags.iter().any(|&t| {
        matches!(
            t,
            Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace | Tag::Hook
        )
    }) {
        return 0;
    }
    let words = body.record(body.data[call][1]);
    words[..words.len().saturating_sub(3)]
        .iter()
        .enumerate()
        .take(64)
        .filter(|&(_, &arg)| {
            let Some(i) = Ref(arg).as_inst().map(|v| v.0 as usize) else {
                return false;
            };
            if body.tags[i] != Tag::Ref || body.data[i][1] & ref_word::MUT != 0 {
                return false;
            }
            let p = body.view(body.data[i][0]);
            let Some(local) = p.local().filter(|_| p.steps(body).next().is_none()) else {
                return false;
            };
            let mut captured = false;
            for (&l, &mode) in body.cap_local.iter().zip(&body.cap_mode) {
                if l.raw() == local {
                    if mode == CaptureMode::Shared {
                        return false;
                    }
                    captured = true;
                }
            }
            if !captured {
                return false;
            }
            let mut ops = Vec::new();
            let mut seen = 0;
            for j in 0..body.len() {
                hd_tir::own::operands(body, j, &mut ops);
                let n = ops.iter().filter(|o| o.value == arg).count();
                if j == call {
                    seen += n;
                } else if n != 0 && body.tags[j] != Tag::Drop {
                    return false;
                }
            }
            seen == 1
        })
        .fold(0, |mask, (k, _)| mask | 1 << k)
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

/// Whether each value a block of `copy` takes, as its tail or by a
/// `Break`, still has the block's type once parameters took their
/// arguments' carriers: a getter returning `self` would give the counted
/// carrier where its result is the local one.
fn carriers_agree(copy: &Body) -> bool {
    let has = |v: u32, want: Ty| {
        Ref(v).as_inst().is_none_or(|v| {
            let t = copy.ty[v.0 as usize];
            t == want || t == Ty::NEVER
        })
    };
    (0..copy.len()).all(|i| {
        let [a, v] = copy.data[i];
        match copy.tags[i] {
            Tag::Block => v == hd_tir::ir::NONE || has(v, copy.ty[i]),
            Tag::Break => copy.label_inst.get(a as usize).is_none_or(|&t| {
                t as usize >= copy.len() || v == hd_tir::ir::NONE || has(v, copy.ty[t as usize])
            }),
            _ => true,
        }
    })
}

/// Whether a value of type `t` holds a local reference, as declared
/// (`&T`) or lowered (`Ref[T]`), outside function types.
fn holds_local(pool: &InternPool, cells: RefCells, t: Ty, depth: u32) -> bool {
    if depth > 32 {
        return false;
    }
    let any = |l| {
        pool.list_items(l)
            .iter()
            .any(|&a| holds_local(pool, cells, a, depth + 1))
    };
    match pool.get(t) {
        TyData::Ref { counted, inner, .. } => {
            !counted || holds_local(pool, cells, inner, depth + 1)
        }
        TyData::Adt { def, args } => def == cells.local || any(args),
        TyData::Tuple { elems, rest } => {
            any(elems) || rest.is_some_and(|r| holds_local(pool, cells, r, depth + 1))
        }
        TyData::Option(i) | TyData::Mut(i) => holds_local(pool, cells, i, depth + 1),
        _ => false,
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
