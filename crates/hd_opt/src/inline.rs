//! The inliner (`docs/compiler/tir-v2.md`, "The inliner"): before the
//! ownership passes, each call of a small function is copied into its
//! caller, the callee's own calls copied first in a release build. It
//! rewrites TIR and reads the program only through [`InlineEnv`], which
//! the driver implements; [`bottom_up`] is the call-graph order a release
//! build owns bodies in, callees first.

use std::borrow::Cow;
use std::sync::{Mutex, MutexGuard, PoisonError};

use hd_base::{DefId, Hash128, HashMap, HashSet, SliceSort, StableHasher};
use hd_mono::ProgramEnv;
use hd_tir::inline::Site;
use hd_tir::ir::{Body, Callee, Proj, Ref, Tag, local_flags, place, ref_word};
use hd_types::{InternPool, Ty, TyData, TyList};

/// The default callee size budget, in std and outside it alike.
/// `hd_tir::inline::inlinable` accepts the raw-row bound or a lowering
/// estimate that charges labelled blocks and call-boundary bindings.
/// The numeric budget started as 16 raw rows (#856); 0 disables it.
pub const INLINE_LIMIT: usize = 16;

/// The size budget for a function marked `@inline` or
/// `@inline(.Auto)` may have and still be copied.
pub const AUTO_INLINE_LIMIT: usize = 64;

/// A bounded consumer copy and the inputs whose function identity is only
/// called or immutably aliased. Cached independently of ordinary eligibility.
struct LiteralConsumer {
    body: Body,
    hash: Hash128,
    parameters: Vec<bool>,
    loop_body: bool,
}

impl LiteralConsumer {
    fn for_call(&self, inputs: &[usize], in_loop: bool) -> Option<(Body, Hash128)> {
        // The contextual bonus must not fuse a callee loop into an active
        // caller loop: V8 cannot tier-up that activation without OSR.
        // Keep this policy outside the per-callee cached preparation.
        if (in_loop && self.loop_body)
            || !inputs
                .iter()
                .any(|&k| self.parameters.get(k) == Some(&true))
        {
            return None;
        }
        Some((self.body.clone(), self.hash))
    }
}

/// The most times one body's calls are copied in a release build: what a
/// round's copies call is copied in the next.
const MAX_ROUNDS: usize = 4;

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
    /// take by value (`hd_wasm::lent_referent`, counted parts
    /// allowed), in every build (#1151, #1167). The environment also
    /// proves that the referent may be copied: its small layout alone
    /// does not prove that. `None` keeps a reader's reference instead.
    fn lent_referent(&self, t: Ty) -> Option<Ty>;
    /// The referent of a reference of type `t` that holds a value with a
    /// `std.rt.Drop` (`hd_wasm::rooted_referent`): an inlined copy
    /// never takes it by value; it keeps the reference in release, and in
    /// dev the call stays a call.
    fn rooted_referent(&self, t: Ty) -> Option<Ty>;
    /// Whether the referent of a reference of type `t`, one
    /// [`InlineEnv::lent_referent`] gives, holds a counted part.
    fn counted_referent(&self, t: Ty) -> bool;
    /// Whether a call of `callee` may write memory its caller reaches, from
    /// the declarations alone: the compiler's code of an intrinsic that
    /// only reads its arguments does not (`OwnCx::may_write` without the
    /// release summaries, which the inliner may run before).
    fn call_may_write(&self, callee: &Callee) -> bool;
    /// Whether the pipeline holds `const_fold` (#1518): a copy whose only
    /// effect is one known result is copied as that result
    /// (`hd_mono::defaults::collapse`), whatever its size.
    fn const_fold(&self) -> bool;
    /// The release folds an instance's own body takes before the inliner
    /// (generated structure traversals, declaration facts), applied to a
    /// copy; returns its hash, the same one when nothing changed.
    fn prepare_copy(&self, body: &mut Body, hash: Hash128) -> Hash128;
    /// Whether `Call` or `Drop` `i` of `body` has no effect a caller sees
    /// (`hd_mono::defaults::collapse_quiet`).
    fn quiet(&self, body: &Body, i: usize) -> bool;
}

/// A release build's inliner state for one run. Only looked up, never
/// iterated, so output stays deterministic.
#[derive(Default)]
pub struct Memo {
    /// Per callee and type arguments, the copy a call of it inlines, its
    /// own calls copied first, and its hash, or `None` when a call of it
    /// stays (`callee_copy`).
    inlined: Mutex<HashMap<(DefId, TyList), Option<Inlined>>>,
    /// Contextual consumer allowance, separate from ordinary eligibility.
    /// At most one bounded copy per concrete callee (once per racing worker).
    literal_consumers: Mutex<HashMap<(DefId, TyList), Option<LiteralConsumer>>>,
    /// The instances the inliner found on a cycle of calls
    /// (`inline_into`): a call of one stays a call.
    recursive: Mutex<HashSet<(DefId, TyList)>>,
    /// Per flag method instance (`annot.bound.recursive.flag`), its
    /// solved value and the constant its call copies as, or `None` where
    /// the call stays a call (`flag_site`). Insert-once: the first value
    /// written stays; a thread that solved the same component meanwhile
    /// found the same values, and drops them.
    flags: Mutex<HashMap<(DefId, TyList), Solved>>,
}

/// A flag's solved value and its copy ([`Memo`]), both `None` where the
/// call stays a call.
type Solved = (Option<bool>, Option<Inlined>);

/// What a call of a flag method copies (`flag_site`).
enum FlagSite {
    /// Its solved value.
    Folded(Box<Inlined>),
    /// Nothing: the call stays a call, a node of the solve in progress.
    Kept,
    /// Whatever the ordinary rules copy: not a flag, or one the solve
    /// leaves to its runtime flag.
    Plain,
}

/// A copy on the inliner's stack: its function and type arguments, or no
/// type arguments for the body being inlined into, whose instance the
/// inliner is not told. A frame with none stands for every instance of
/// its function.
type Frame = (DefId, Option<TyList>);

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
    env.inline_into(body, hash, &mut Stack::new((body.item, None), false))
}

/// The inliner's stack of copies ([`Frame`]), the caller first, and
/// whether they are copied for a flag solve (`solve_flags_from`), passed
/// down explicitly as rustc's query stack is: inside a solve, every flag
/// call not yet solved is one of the solve's members (its scan adds those
/// the copy keeps), so it stays a call (`FlagSite::Kept`).
struct Stack {
    frames: Vec<Frame>,
    solving: bool,
}

impl Memo {
    /// One query per site, one bounded preparation per concrete consumer.
    /// Locks never cover signature/type queries; a racing worker may prepare
    /// one additional result, as with the ordinary inliner cache.
    fn consumer_copy(
        &self,
        key: (DefId, TyList),
        prepare: impl FnOnce() -> Option<LiteralConsumer>,
        select: impl FnOnce(&LiteralConsumer) -> Option<(Body, Hash128)>,
    ) -> Option<(Body, Hash128)> {
        if let Some(made) = lock(&self.literal_consumers).get(&key) {
            return made.as_ref().and_then(select);
        }
        let made = prepare();
        let result = made.as_ref().and_then(select);
        // Cache the body facts, never this caller's selection result.
        lock(&self.literal_consumers).insert(key, made);
        result
    }
}

impl Stack {
    fn new(first: Frame, solving: bool) -> Self {
        Self {
            frames: vec![first],
            solving,
        }
    }
}

impl std::ops::Deref for Stack {
    type Target = Vec<Frame>;
    fn deref(&self) -> &Vec<Frame> {
        &self.frames
    }
}

impl std::ops::DerefMut for Stack {
    fn deref_mut(&mut self) -> &mut Vec<Frame> {
        &mut self.frames
    }
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
    fn inline_into(&self, body: &mut Body, hash: Hash128, stack: &mut Stack) -> Hash128 {
        let mut hash = hash;
        // A call through a function reference the body knows is a call
        // of its item, which may copy in turn. A copy may bring more: a
        // default's enclosing call turns plain, and a copy taken as written
        // (`callee_copy`) keeps calls of its own. Those in a loop are judged
        // as the caller's in the next round. Outside a loop one saves little,
        // and a larger function may lose V8's own inlining of it
        // (binary_trees' allocator, 290 to 317 ms).
        let release = self.memo().is_some();
        if release {
            self.direct_item_calls(body);
        }
        let mut loops_only = false;
        for _ in 0..MAX_ROUNDS {
            let literals = release
                && hd_tir::inline::inline_local_closures(
                    body,
                    INLINE_LIMIT,
                    &|caller, call, copy| {
                        let references: Vec<_> = hd_tir::own::params(copy)
                            .iter()
                            .enumerate()
                            .filter_map(|(k, &local)| {
                                self.reference(copy.local_ty[local as usize])
                                    .then_some((k, copy.local_ty[local as usize]))
                            })
                            .collect();
                        if !references.is_empty()
                            && !self.read_only_reference_inputs(&references, copy)
                        {
                            return false;
                        }
                        self.fits_call(caller, call, copy)
                    },
                )
                .unwrap_or(false);
            let defaults = body.tags.contains(&Tag::DefaultCall);
            let (h, copied, writer) = self.inline_round(body, hash, stack, loops_only);
            let copied = copied || literals;
            if copied {
                let pool = self.pool();
                // An item's or literal's copy opens its opaque result types,
                // as an instance does; the caller's still name the
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
            }
            hash = h;
            if !(copied && release) {
                break;
            }
            let direct = self.direct_item_calls(body);
            // A default exposes its enclosing call; a scalar writer may
            // expose the reader/constructor around its local state. Both
            // use the existing four-round budget.
            loops_only = !(defaults || direct || writer || literals);
        }
        hash
    }

    /// Rewrites each `CallValue` whose callee is a function reference to
    /// an item (`ItemRef`), directly or through a local stored once with
    /// one, into a `Call` of that item (#1385): the value's code is the
    /// item's adapter, which only calls it. Not for a trait method's
    /// reference or an item that suspends. A row-bearing reference uses the
    /// current call-site providers, just as a direct call with the empty
    /// provider trailer does; copying later keeps lexical `With` scopes.
    /// Whether any changed.
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
                || !matches!(self.signature(def), Some((_, false, _)))
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
        stack: &mut Stack,
        loops_only: bool,
    ) -> (Hash128, bool, bool) {
        let mut k = StableHasher::new("inline");
        k.hash(hash);
        let (mut sites, mut hashes) = (Vec::new(), Vec::new());
        let loops = hd_tir::ir::Topology::new(body).loop_of;
        let literals = if self.memo().is_some() {
            hd_tir::inline::literal_arguments(body)
        } else {
            Vec::new()
        };
        let writer_origins = if self.memo().is_some() {
            writer_origins(body)
        } else {
            Vec::new()
        };
        let writer_uses = if self.memo().is_some() {
            writer_uses(body)
        } else {
            Vec::new()
        };
        let mut caller_effects = None;
        for c in (0..body.len()).rev() {
            let in_loop = loops[c] != hd_tir::ir::NONE;
            let Some((def, targs)) =
                hd_tir::inline::known_call(body, c).and_then(|callee| match callee {
                    Callee::Item { def, targs } => Some((def, targs)),
                    Callee::TraitMethod { .. } => self.impl_target(&callee),
                })
            else {
                continue;
            };
            if loops_only && !in_loop && !self.declares_local_reference(def) {
                continue;
            }
            if body.tags[c] == Tag::DefaultCall
                && self
                    .checked_body(def)
                    .is_some_and(|(body, _)| hd_mono::defaults::constant(body).is_some())
            {
                // Collection specializes the callee on scalar defaults.
                // Keep that marker until it has folded those decisions.
                continue;
            }
            if self.inline_hint(def) == InlineHint::Default
                && hd_tir::inline::plain_call(body, c).is_none()
            {
                continue;
            }
            // A flag method's call copies its solved value, even its own
            // call inside its body.
            let folded = match self.flag_site(def, targs, stack.solving) {
                FlagSite::Folded(copy) => Some(*copy),
                FlagSite::Kept => continue,
                FlagSite::Plain => None,
            };
            // A cycle is a call of an instance already on the stack. A
            // function at other type arguments is another instance (a
            // derived flag asking a member's), up to `MAX_DEPTH` copies of
            // one function, collection's own bound on instance nesting,
            // which stops a polymorphic recursion's ever-larger types.
            let same = |&(d, a): &Frame| d == def && a.is_none_or(|a| a == targs);
            let again = stack.iter().filter(|f| f.0 == def).count();
            if folded.is_none()
                && let Some(at) = stack.iter().position(same).or_else(|| {
                    (again >= usize::from(hd_mono::layout::MAX_DEPTH))
                        .then(|| stack.iter().position(|f| f.0 == def))
                        .flatten()
                })
            {
                self.found_cycle(&stack[at..]);
                continue;
            }
            let ordinary = folded.or_else(|| self.callee_copy(def, targs, stack));
            let consumer = ordinary.is_none();
            let Some((mut copy, callee_hash)) = ordinary.or_else(|| {
                self.literal_consumer(
                    def,
                    targs,
                    literals.get(c).map_or(&[], Vec::as_slice),
                    in_loop,
                )
            }) else {
                continue;
            };
            // A copy made for another caller may call one on the way down.
            if let Some(at) = stack.iter().position(|&f| self.calls_frame(&copy, f)) {
                self.found_cycle(&stack[at..]);
                self.found_cycle(&[(def, Some(targs))]);
                continue;
            }
            if !self.fits_call(body, c, &mut copy) {
                continue;
            }
            let writer_refs = self
                .scalar_writer_refs(&copy)
                .filter(|&mask| writer_arguments(body, c, mask, &writer_origins));
            if loops_only && !in_loop && writer_refs.is_none() {
                continue;
            }
            // Over the ordinary budget only by `fits`'s allowance for a callee
            // that takes local references and runs no call of its own.
            if self.inline_hint(def) == InlineHint::Default
                && !self.fits_size(def, targs, &copy, INLINE_LIMIT)
                && !in_loop
                && writer_refs.is_none()
                && !consumer
            {
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
                let mut args = if promote.is_some() {
                    let effects =
                        caller_effects.get_or_insert_with(|| hd_tir::own::WriteEffects::new(body));
                    hd_tir::own::promote::inline_args(effects, c, counted, writes, &may_write)
                } else if writer_refs.is_some() {
                    fresh_writer_args(body, c, &writer_uses)
                } else {
                    0
                };
                let (references, bindings) =
                    if self.memo().is_some() && self.read_only_refs(def, targs, &copy) {
                        args |= captured_read_args(body, c);
                        (reference_read_args(body, c), weakened_read_args(body, c))
                    } else {
                        (0, 0)
                    };
                if promote.is_none() {
                    if self.memo().is_none() {
                        continue;
                    }
                    if self.read_only_refs(def, targs, &copy) {
                        by_ref = self
                            .reference_params(def, targs, true)
                            .unwrap_or_default()
                            .iter()
                            .fold(0, |mask, (k, _)| mask | 1 << k);
                        0
                    } else if let Some(mask) = writer_refs {
                        let values =
                            self.writer_values(&copy, body, c, mask, &writer_origins) & args;
                        by_ref = mask & !values;
                        // A writer does not prevent an independent scalar
                        // reader from keeping the call's by-value ABI.
                        values
                    } else if !in_loop {
                        continue;
                    } else {
                        0
                    }
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
            hashes.push((callee_hash, writer_refs.is_some()));
        }
        // All of the body's copies in one rebuild.
        let copied = hd_tir::inline::inline_calls(body, sites).unwrap_or_default();
        let mut writer = false;
        for (_, &(h, scalar_writer)) in copied.iter().zip(&hashes).filter(|(c, _)| **c) {
            k.hash(h);
            writer |= scalar_writer;
        }
        if copied.contains(&true) {
            (k.finish(), true, writer)
        } else {
            (hash, false, false)
        }
    }

    /// Records where `members`, a run of calls that reaches its first
    /// member again, must break in a release build. An all-`.Always`
    /// cycle is rejected by the package check and none is copied. A mixed
    /// cycle breaks at every function not marked `.Always`, so `.Always`
    /// calls on either side can still be copied.
    /// A frame with no type arguments is the body being inlined into,
    /// which no copy names; its own calls stay calls without a mark.
    fn found_cycle(&self, members: &[Frame]) {
        if let Some(memo) = self.memo() {
            let known = |&(d, a): &Frame| a.map(|a| (d, a));
            let breaks: Vec<Frame> = members
                .iter()
                .copied()
                .filter(|f| self.inline_hint(f.0) != InlineHint::Always)
                .collect();
            let breaks = if breaks.is_empty() { members } else { &breaks };
            lock(&memo.recursive).extend(breaks.iter().filter_map(known));
        }
    }

    /// Whether `body` calls an instance of `frame`, by name or through a
    /// trait method's impl.
    fn calls_frame(&self, body: &Body, (def, args): Frame) -> bool {
        (0..body.len()).any(|c| {
            hd_tir::inline::known_call(body, c)
                .and_then(|callee| match callee {
                    Callee::Item { def, targs } => Some((def, targs)),
                    Callee::TraitMethod { .. } => self.impl_target(&callee),
                })
                .is_some_and(|(d, t)| d == def && args.is_none_or(|a| a == t))
        })
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
    fn callee_copy(&self, def: DefId, targs: TyList, stack: &mut Stack) -> Option<(Body, Hash128)> {
        let memo = self.memo();
        if let Some(memo) = memo {
            if let Some(made) = lock(&memo.inlined).get(&(def, targs)) {
                return made.clone();
            }
            if lock(&memo.recursive).contains(&(def, targs)) {
                return None;
            }
        }
        if self.const_fold()
            && let Some(c) = self
                .quiet_constant(def, targs, stack)
                .filter(|(body, _)| self.copy_metadata_fits(def, body))
        {
            if let Some(memo) = memo {
                lock(&memo.inlined).insert((def, targs), Some(c.clone()));
            }
            return Some(c);
        }
        let made = self.instance(def, targs).and_then(|(copy, hash, fits)| {
            let nested = (!hd_tir::inline::leaf(&copy)).then(|| {
                let mut nested = copy.clone();
                stack.push((def, Some(targs)));
                let h = self.inline_into(&mut nested, hash, stack);
                stack.pop();
                // The decisions the copied callees' constants settle fold
                // before the copy's size is judged, as its instance's own
                // body folds them (#1445).
                if self.const_fold()
                    && let Some(folded) = hd_mono::defaults::specialize(&nested, &[])
                {
                    nested = folded;
                }
                (nested, h)
            });
            if memo.is_some_and(|memo| lock(&memo.recursive).contains(&(def, targs))) {
                return None;
            }
            // A copy that only makes a known value is that value, at any
            // size: a call of it folds, and so do the decisions it makes.
            if self.const_fold()
                && let Some(c) =
                    hd_mono::defaults::collapse(nested.as_ref().map_or(&copy, |n| &n.0))
            {
                return Some((c, nested.as_ref().map_or(hash, |n| n.1)));
            }
            match nested {
                Some((nested, h)) if fits && self.inline_hint(def) != InlineHint::Default => {
                    Some((nested, h))
                }
                Some((nested, h)) if self.fits(def, targs, &nested) => Some((nested, h)),
                // Charge a tiny one-call forwarding body as well as its
                // scalar writer, rather than losing the known target at
                // each forwarding layer. Memoized copies remain bounded
                // by the sum of those two existing budgets.
                Some((nested, h))
                    if nested.len() <= AUTO_INLINE_LIMIT + INLINE_LIMIT
                        && hd_tir::inline::inlinable(&copy, INLINE_LIMIT)
                        && copy.tags.iter().filter(|&&tag| tag == Tag::Call).count() == 1
                        && self.scalar_writer_refs(&nested).is_some() =>
                {
                    Some((nested, h))
                }
                _ => fits.then_some((copy, hash)),
            }
        });
        let made = made.filter(|(body, _)| self.copy_metadata_fits(def, body));
        if let Some(memo) = memo {
            lock(&memo.inlined).insert((def, targs), made.clone());
        }
        made
    }

    /// The copy of `def` as the one value it makes, where the folds its
    /// own instance body takes (`InlineEnv::prepare_copy`) expose a body
    /// whose effects all stay in its frame (a derived flag's describer
    /// walk, #1445). `None` when those folds change nothing, so every
    /// other callee keeps the ordinary path, or no constant results.
    fn quiet_constant(
        &self,
        def: DefId,
        targs: TyList,
        stack: &mut Stack,
    ) -> Option<(Body, Hash128)> {
        let (mut scratch, hash, prepared) = self.prepared(def, targs)?;
        if prepared == hash {
            return None;
        }
        stack.push((def, Some(targs)));
        let h = self.inline_into(&mut scratch, prepared, stack);
        stack.pop();
        let quiet = |b: &Body, i: usize| self.quiet(b, i);
        let mut c = hd_mono::defaults::collapse_quiet(&scratch, &quiet)?;
        // A derived instance's constant is copied as any function's.
        c.kind = hd_tir::ir::BodyKind::Fn;
        Some((c, h))
    }

    /// The body of `def`, a function with no parameters, at `targs`,
    /// with its types settled, its trait calls chosen, and the folds its
    /// own instance body takes (`InlineEnv::prepare_copy`) applied; with
    /// its checked hash and the hash after those folds. Only a function
    /// with no parameters can make one value at every call; its body need
    /// not be one the inliner copies as written.
    fn prepared(&self, def: DefId, targs: TyList) -> Option<(Body, Hash128, Hash128)> {
        let (params, suspends, has_row) = self.signature(def)?;
        if !params.is_empty() || suspends || has_row || self.intrinsic(def).is_some() {
            return None;
        }
        let hash = self.checked_body(def)?.1;
        let mut scratch = self.fresh_body(def)?;
        let pool = self.pool();
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
        hd_tir::ir::map_body_types(&mut scratch, &ty, &list);
        if scratch.ty.iter().chain(&scratch.local_ty).any(|&t| {
            t == Ty::POISON || pool.has_param(t) || pool.has_assoc(t) || pool.has_opaque(t)
        }) {
            return None;
        }
        hd_tir::inline::rechoose(&mut scratch, &|t, s, a| self.select_impl(t, s, a));
        let prepared = self.prepare_copy(&mut scratch, hash);
        Some((scratch, hash, prepared))
    }

    /// What a call of `def` at `targs` copies when it is a flag method
    /// (`annot.bound.recursive.flag`) in a release build that folds
    /// constants: the value `hd_opt::whole::solve_flags` gives it, solved
    /// with every flag it reaches on first ask. Dev keeps every call, and
    /// its runtime flag answers a re-entry.
    fn flag_site(&self, def: DefId, targs: TyList, solving: bool) -> FlagSite {
        let Some(memo) = self.memo() else {
            return FlagSite::Plain;
        };
        if !self.const_fold() || !crate::whole::is_flag_method(self, def, targs) {
            return FlagSite::Plain;
        }
        let solved = || {
            lock(&memo.flags).get(&(def, targs)).map(|s| {
                s.1.clone()
                    .map_or(FlagSite::Plain, |c| FlagSite::Folded(Box::new(c)))
            })
        };
        if let Some(site) = solved() {
            return site;
        }
        if solving {
            return FlagSite::Kept;
        }
        self.solve_flags_from((def, targs), memo);
        solved().unwrap_or(FlagSite::Plain)
    }

    /// Solves the flag methods `root` reaches and records each one's copy
    /// in `memo` (`crate::whole::solve_flags`). Each flag's body is its
    /// prepared body with its calls copied, every flag call kept as a
    /// call (`FlagSite::Kept`), then folded; the flags those calls name
    /// are the graph's nodes, found breadth-first. A flag solved before
    /// is no node: its call was copied as its value. Nothing is locked
    /// while solving: each component is solved once per thread that
    /// reaches it before the first insert, so at most the thread count
    /// (`HD_JOBS`, at most 8) times one solve, and once in the common
    /// case. One solve costs one copy and one scan per flag, then
    /// [`crate::whole::solve_flags`]'s bound.
    fn solve_flags_from(&self, root: (DefId, TyList), memo: &Memo) {
        let mut index: HashMap<(DefId, TyList), u32> = HashMap::default();
        index.insert(root, 0);
        let mut nodes = vec![root];
        let mut bodies: Vec<Option<Body>> = Vec::new();
        // Per node: its flag calls, by instruction, sorted.
        let mut sites: Vec<Vec<(u32, u32)>> = Vec::new();
        let (mut from, mut callees) = (vec![0u32], Vec::new());
        while let Some(&(def, targs)) = nodes.get(bodies.len()) {
            let body = self.prepared(def, targs).map(|(mut b, _, prepared)| {
                self.inline_into(&mut b, prepared, &mut Stack::new((def, Some(targs)), true));
                hd_mono::defaults::specialize(&b, &[]).unwrap_or(b)
            });
            let mut mine = Vec::new();
            for c in 0..body.as_ref().map_or(0, Body::len) {
                let Some(b) = &body else { break };
                let Some((d, t)) = (b.tags[c] == Tag::Call)
                    .then(|| hd_tir::inline::known_call(b, c))
                    .flatten()
                    .and_then(|callee| match callee {
                        Callee::Item { def, targs } => Some((def, targs)),
                        Callee::TraitMethod { .. } => self.impl_target(&callee),
                    })
                else {
                    continue;
                };
                if !crate::whole::is_flag_method(self, d, t)
                    || lock(&memo.flags).contains_key(&(d, t))
                {
                    continue;
                }
                let j = *index.entry((d, t)).or_insert_with(|| {
                    nodes.push((d, t));
                    index_word(nodes.len() - 1)
                });
                mine.push((index_word(c), j));
            }
            let mut out: Vec<u32> = mine.iter().map(|s| s.1).collect();
            out.sort_stable();
            out.dedup();
            callees.extend(out);
            from.push(index_word(callees.len()));
            bodies.push(body);
            sites.push(mine);
        }
        let site = |i: usize, c: usize| {
            sites[i]
                .binary_search_by_key(&index_word(c), |s| s.0)
                .ok()
                .map(|k| sites[i][k].1)
        };
        let local: Vec<bool> = (0..nodes.len())
            .map(|i| {
                bodies[i].as_ref().is_some_and(|b| {
                    let quiet = |b: &Body, c: usize| site(i, c).is_some() || self.quiet(b, c);
                    hd_mono::defaults::local_only(b, &quiet)
                })
            })
            .collect();
        let mut eval = |i: u32, seed: &dyn Fn(u32) -> Option<bool>| {
            let i = i as usize;
            let calls = |c: usize| {
                site(i, c)
                    .and_then(seed)
                    .map(|v| hd_mono::defaults::Value::Scalar(u64::from(v)))
            };
            let value = hd_mono::defaults::value_given(bodies[i].as_ref()?, &calls)?;
            // ruled out: whole::is_flag_method admits only a `bool` result,
            // and value_given makes a variant only of a `NewVariant`.
            debug_assert!(
                matches!(value, hd_mono::defaults::Value::Scalar(_)),
                "a flag method's value is a variant"
            );
            Some(value != hd_mono::defaults::Value::Scalar(0))
        };
        let values = crate::whole::solve_flags(&from, &callees, &local, &mut eval, &mut 0);
        let mut flags = lock(&memo.flags);
        for (i, &(def, targs)) in nodes.iter().enumerate() {
            let made = values[i].and_then(|v| {
                let value = hd_mono::defaults::Value::Scalar(u64::from(v));
                let mut copy = hd_mono::defaults::constant_copy(bodies[i].as_ref()?, value)?;
                copy.kind = hd_tir::ir::BodyKind::Fn;

                let mut h = StableHasher::new("flag-value");
                h.hash(self.checked_body(def)?.1);
                h.u8(u8::from(v));
                Some((copy, h.finish()))
            });
            match flags.entry((def, targs)) {
                std::collections::hash_map::Entry::Occupied(first) => {
                    debug_assert_eq!(
                        first.get().0,
                        values[i],
                        "flag values differ between solves"
                    );
                }
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert((values[i], made));
                }
            }
        }
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

    /// A larger return scope has not proved the call's loan-end
    /// contract for a reference-carrying result. Keep that boundary;
    /// existing raw-budget getters and explicit Always copies retain
    /// their established eligibility.
    fn fits_size(&self, def: DefId, targs: TyList, copy: &Body, limit: usize) -> bool {
        if copy.len() > limit && self.result_holds_local(def, targs, copy) {
            return false;
        }
        hd_tir::inline::inlinable(copy, limit)
    }

    fn result_holds_local(&self, def: DefId, targs: TyList, copy: &Body) -> bool {
        let declared = self.result(def).map(|t| {
            let t = self.pool().erase_fn_deps(t);
            hd_mono::subst(self.pool(), self, def, targs, t)
        });
        let root = copy
            .sub_root
            .first()
            .and_then(|&r| copy.ty.get(r as usize))
            .copied();
        // A body whose every path explicitly returns has a never root;
        // its instantiated declaration still carries the loan.
        declared
            .into_iter()
            .chain(root)
            .any(|t| holds_local(self.pool(), t, 0))
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
                let release = self.memo().is_some();
                (self.fits_size(def, targs, copy, INLINE_LIMIT)
                    && (!self.takes_reference(def, targs)
                        || self.by_value(def, targs, copy).is_some()
                        || (release && self.read_only_refs(def, targs, copy))))
                    || (release
                        && !self.takes_reference(def, targs)
                        && self.plain_memory(copy)
                        && self.fits_size(def, targs, copy, AUTO_INLINE_LIMIT))
                    || (release
                        && self.takes_reference(def, targs)
                        && self.only_local_references(def, targs)
                        && self.plain_memory(copy)
                        && copy
                            .sub_root
                            .first()
                            .is_some_and(|&r| copy.ty[r as usize] == Ty::VOID)
                        && self.fits_size(def, targs, copy, AUTO_INLINE_LIMIT))
            }
            // A by-value parameter of a type still open waits for the instance:
            // an erased body cannot bind one in a local.
            InlineHint::Auto => {
                self.fits_size(def, targs, copy, AUTO_INLINE_LIMIT)
                    && self.reference_params(def, targs, true).is_some()
            }
            InlineHint::Always => hd_tir::inline::copyable(copy),
            InlineHint::Never => false,
        }
    }

    /// A known literal makes a small higher-order consumer profitable even
    /// when the loop around its callback exceeds the ordinary size budget.
    /// Keep the ordinary global limit unchanged. The raw consumer uses the
    /// existing Auto budget; its calls are handled by later bounded rounds.
    /// Cost O(K) per admitted site, K <=16*`AUTO_INLINE_LIMIT` budget units;
    /// each instruction/local unit covers a fixed number of column words.
    /// no recursive preparation or failed-memo policy is changed.
    fn literal_consumer(
        &self,
        def: DefId,
        targs: TyList,
        inputs: &[usize],
        in_loop: bool,
    ) -> Option<(Body, Hash128)> {
        let memo = self.memo()?;
        if inputs.is_empty()
            || lock(&memo.recursive).contains(&(def, targs))
            || self.inline_hint(def) != InlineHint::Default
        {
            return None;
        }
        memo.consumer_copy(
            (def, targs),
            || self.make_literal_consumer(def, targs),
            |made| made.for_call(inputs, in_loop),
        )
    }

    fn make_literal_consumer(&self, def: DefId, targs: TyList) -> Option<LiteralConsumer> {
        // Reject oversized metadata before cloning/substitution as well as
        // after: the failed attempt must not scan a large callee per site.
        let (raw, _) = self.checked_body(def)?;
        if !copy_metadata_fits(raw) {
            return None;
        }
        let (copy, hash, _) = self.instance(def, targs)?;
        if !copy_metadata_fits(&copy)
            || !self.fits_size(def, targs, &copy, AUTO_INLINE_LIMIT)
            // The contextual size bonus does not enlarge dependent-result
            // scopes beyond the existing ordinary raw-body boundary.
            || copy.len() > INLINE_LIMIT && self.result_holds_local(def, targs, &copy)
            || self.takes_reference(def, targs)
                && self.by_value(def, targs, &copy).is_none()
                && !self.read_only_refs(def, targs, &copy)
        {
            return None;
        }
        let parameters = hd_tir::inline::called_parameters(&copy);
        let loop_body = copy.tags.contains(&Tag::Loop);
        parameters.contains(&true).then_some(LiteralConsumer {
            loop_body,
            body: copy,
            hash,
            parameters,
        })
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
        let (_, suspends, _) = self.signature(def)?;
        if suspends {
            return None;
        }
        let (body, hash) = self.checked_body(def)?;
        // A With can hold many keys in very few instructions. The same
        // metadata bound as literal consumers prevents record width from
        // multiplying by automatic call sites, before a fresh decoding.
        // Explicit hints retain their specified output-driven copy cost.
        if !self.copy_metadata_fits(def, body) {
            return None;
        }
        // Defaults that may call other code keep their runtime guard.
        // A fresh whole-local reference can move into the caller: inline_calls
        // relocates its local before frame planning, which still marks any
        // reference escaping the caller as a heap owner.
        if body.kind == hd_tir::ir::BodyKind::Default
            && (hd_mono::default_needs_bracket(body)
                || (self
                    .result(def)
                    .is_some_and(|t| holds_local(self.pool(), t, 0))
                    && !hd_mono::default_returns_new_object(body)))
        {
            return None;
        }
        let body_fits = match hint {
            InlineHint::Default => {
                hd_tir::inline::inlinable(body, INLINE_LIMIT)
                    || (release
                        && (self.declares_local_reference(def) || body.len() <= AUTO_INLINE_LIMIT)
                        && hd_tir::inline::inlinable(body, AUTO_INLINE_LIMIT))
            }
            InlineHint::Auto => hd_tir::inline::inlinable(body, AUTO_INLINE_LIMIT),
            InlineHint::Always => hd_tir::inline::copyable(body),
            InlineHint::Never => false,
        };
        if !body_fits || self.calls(body, def) {
            return None;
        }
        Some((self.fresh_body(def)?, *hash))
    }

    fn copy_metadata_fits(&self, def: DefId, body: &Body) -> bool {
        self.inline_hint(def) != InlineHint::Default || copy_metadata_fits(body)
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
    /// (`crates/hd_wasm/DESIGN.md` §23.38).
    fn takes_reference(&self, def: DefId, targs: TyList) -> bool {
        let Some(refs) = self.reference_params(def, targs, false) else {
            return true;
        };
        let getter = self
            .result(def)
            .is_some_and(|r| holds_local(self.pool(), r, 0));
        refs.len() > 1 || (refs.len() == 1 && !getter)
    }

    /// The parameters a copy of `def` at `targs` takes by value (#1151;
    /// bit k: parameter k), in dev and release builds alike, when each of its reference parameters
    /// has a referent a copy may take by value (`lent_referent`) and
    /// `copy` only reads through it
    /// (`hd_tir::own::promote::read_only_params`). The copy then binds the
    /// referent's value, not the address, and no cell follows
    /// (`crates/hd_wasm/DESIGN.md` §23.40). The binding is an
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
        let Some(refs) = self.reference_params(def, targs, true) else {
            return false;
        };
        self.read_only_reference_inputs(&refs, copy)
    }

    fn read_only_reference_inputs(&self, refs: &[(usize, Ty)], copy: &Body) -> bool {
        let may_write = |callee: &Callee| self.call_may_write(callee);
        if hd_tir::own::writes_in(copy, 0..copy.len(), &may_write) {
            return false;
        }
        let shared = |ty| matches!(self.pool().get(ty), TyData::Ref { mutable: false, .. });
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

    /// Promote an independent scalar reader even when another parameter
    /// writes. Private local roots are distinct from other private roots
    /// and from an enclosing reference parameter's external storage. Two
    /// external parameters, or two parts of one local, keep their address.
    fn writer_values(
        &self,
        copy: &Body,
        caller: &Body,
        call: usize,
        mask: u64,
        origins: &[Option<WriterOrigin>],
    ) -> u64 {
        let readonly = hd_tir::own::promote::read_only_params(copy, mask);
        let mut candidates = 0;
        let mut writable = mask & !readonly;
        for (k, ty) in copy
            .local_ty
            .iter()
            .zip(&copy.local_flags)
            .filter_map(|(&ty, &flags)| (flags & local_flags::PARAM != 0).then_some(ty))
            .take(64)
            .enumerate()
        {
            if matches!(self.pool().get(ty), TyData::Ref { mutable: true, .. }) {
                // An unused mutable argument still has its call loan.
                writable |= 1 << k;
            } else if readonly & (1 << k) != 0
                && self.lent_referent(ty).is_some()
                && !self.counted_referent(ty)
            {
                candidates |= 1 << k;
            }
        }
        independent_writer_values(
            hd_tir::inline::arguments(caller, call),
            candidates,
            writable,
            origins,
        )
    }

    /// The unchanged reference bindings of a scalar void writer. Direct
    /// substitution preserves writes through them without an owning copy.
    fn scalar_writer_refs(&self, copy: &Body) -> Option<u64> {
        if self.memo().is_none()
            || copy
                .sub_root
                .first()
                .is_none_or(|&r| copy.ty[r as usize] != Ty::VOID)
            || !self.plain_memory(copy)
        {
            return None;
        }
        let mut mask = 0;
        for (param, (&ty, &flags)) in copy
            .local_ty
            .iter()
            .zip(&copy.local_flags)
            .filter(|(_, flags)| **flags & local_flags::PARAM != 0)
            .enumerate()
        {
            if self.local_reference_type(ty) {
                if param >= 64 || flags & (local_flags::ASSIGNED | local_flags::CAPTURED) != 0 {
                    return None;
                }
                mask |= 1 << param;
            }
        }
        (mask != 0).then_some(mask)
    }

    /// Whether each reference parameter of `def` at `targs` is a declared
    /// local reference, `&T` or `&mut T`, whatever the body does through it.
    fn only_local_references(&self, def: DefId, targs: TyList) -> bool {
        self.reference_params(def, targs, true)
            .is_some_and(|refs| refs.iter().all(|&(_, t)| self.local_reference_type(t)))
    }

    /// Whether `t` is a local reference type.
    fn local_reference_type(&self, t: Ty) -> bool {
        matches!(self.pool().get(t), TyData::Ref { counted: false, .. })
    }

    /// Whether a declared parameter of `def` is a local reference.
    fn declares_local_reference(&self, def: DefId) -> bool {
        self.signature(def)
            .is_some_and(|(params, _, _)| params.iter().any(|&t| self.local_reference_type(t)))
    }

    /// The existing cost filter: local references and value-layout types,
    /// no element places or lowered collection intrinsics, and no continuing
    /// calls except compiler intrinsics. Value-layout aggregates may still
    /// hold counted fields. Counts are placed after copying; a writer's
    /// call site separately proves its storage origin and loan boundary.
    fn plain_memory(&self, copy: &Body) -> bool {
        let scalar = |t: Ty| {
            !self.is_open(t)
                && (self.local_reference_type(t) || t == Ty::NEVER || !self.reference(t))
        };
        copy.ty.iter().chain(&copy.local_ty).all(|&t| scalar(t))
            && (0..copy.len()).all(|i| {
                let (oa, ob) = copy.tags[i].operands();
                let elem = [oa, ob].into_iter().enumerate().any(|(x, op)| {
                    op == hd_tir::ir::Op::Place
                        && (copy.view(copy.data[i][x]).steps(copy))
                            .any(|s| matches!(s, Proj::Elem(_)))
                });
                let call = matches!(
                    copy.tags[i],
                    Tag::Call | Tag::CallValue | Tag::CallDyn | Tag::CallHost | Tag::DefaultCall
                );
                !elem
                    && copy.tags[i] != Tag::Intrinsic
                    && (!call
                        || copy.ty[i] == Ty::NEVER
                        || matches!(
                            hd_tir::inline::known_call(copy, i),
                            Some(Callee::Item { def, .. }) if self.intrinsic(def).is_some()
                        ))
            })
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
            // As the backend reads a declared type: with no dependency map.
            let mut t = pool.erase_fn_deps(t);
            if self.is_open(t) {
                t = hd_mono::subst(pool, self, def, targs, t);
            }
            let reference = matches!(pool.get(t), TyData::Ref { .. });
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
    /// are then well typed. A parameter the call lends by kind takes the
    /// argument's type, as the call binds it. Any other difference keeps
    /// the call.
    fn fits_call(&self, caller: &Body, c: usize, copy: &mut Body) -> bool {
        let pool = self.pool();
        let open = |t: Ty| hd_mono::reveal(pool, self, t);
        let fits = |got: Option<Ty>, want: Ty| {
            got.is_some_and(|got| got == want || got == Ty::NEVER || want == Ty::NEVER)
        };
        let args = hd_tir::inline::arguments(caller, c);
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
            if lends_by_kind(pool, arg, want) {
                copy.local_ty[l] = arg;
                keep_parameter_kind(pool, copy, l);
                rebound = true;
            } else if !fits(Some(arg), want) {
                return false;
            }
        }
        let root = copy.sub_root.first().map(|&r| copy.ty[r as usize]);
        n == args.len()
            && root.is_some_and(|root| fits(open(caller.ty[c]), root))
            && (!rebound || kinds_agree(copy))
    }
}

/// Constant-time vector-length checks and at most K string rows, before any
/// candidate clone. Large literals and side tables cannot multiply by sites.
fn copy_metadata_fits(body: &Body) -> bool {
    copy_metadata_fits_with(body, || {})
}

fn copy_metadata_fits_with(body: &Body, mut visit: impl FnMut()) -> bool {
    let mut remaining = 16 * AUTO_INLINE_LIMIT;
    for len in [
        body.len(),
        body.extra.len(),
        body.local_ty.len(),
        body.consts.len(),
        body.sub_root.len(),
        body.cap_local.len(),
        body.label_inst.len(),
        body.fallback.len(),
        body.held_args.len(),
        body.inlined.len(),
        body.inlined_origins.len(),
        body.inlined_shared.len(),
        body.inlined_capture_origins.len().saturating_mul(3),
    ] {
        visit();
        let Some(next) = remaining.checked_sub(len) else {
            return false;
        };
        remaining = next;
    }
    for text in &body.strings {
        visit();
        let Some(next) = remaining.checked_sub(text.len().div_ceil(4).max(1)) else {
            return false;
        };
        remaining = next;
    }
    true
}

/// Appends a record of `words` to `body`'s `extra`; its offset.
fn push_record(body: &mut Body, words: &[u32]) -> u32 {
    let at = u32::try_from(body.extra.len()).expect("extra");
    body.extra.push(u32::try_from(words.len()).expect("record"));
    body.extra.extend_from_slice(words);
    at
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WriterOrigin {
    Local(u32),
    Parameter(u32),
}

/// Plain local storage has no dynamic alias loan. An unchanged reference
/// parameter already has the enclosing call's loan. Keep other arguments'
/// call boundaries, including counted-pointer paths and shared variables.
fn writer_origins(body: &Body) -> Vec<Option<WriterOrigin>> {
    writer_origins_counted(body, || {})
}

/// Count each row and each distinct place's projection once. The call-site
/// walk never follows a projection chain anew for each reference to it.
fn writer_origins_counted(body: &Body, mut visit: impl FnMut()) -> Vec<Option<WriterOrigin>> {
    let mut shared = vec![false; body.local_ty.len()];
    for (&local, &mode) in body.cap_local.iter().zip(&body.cap_mode) {
        visit();
        shared[local.idx()] |= mode.shares();
    }
    let mut places = HashMap::default();
    let mut origin = vec![None; body.len()];
    for i in 0..body.len() {
        visit();
        origin[i] = match body.tags[i] {
            Tag::Ref => *places.entry(body.data[i][0]).or_insert_with(|| {
                let place = body.view(body.data[i][0]);
                place
                    .local()
                    .filter(|&local| {
                        !shared[local as usize]
                            && place.steps(body).all(|step| {
                                visit();
                                matches!(step, Proj::Field(_) | Proj::Variant(_))
                            })
                    })
                    .map(WriterOrigin::Local)
            }),
            Tag::Read => body
                .local_read(i)
                .filter(|local| {
                    let flags = body.local_flags[local.idx()];
                    flags & local_flags::PARAM != 0
                        && flags & (local_flags::ASSIGNED | local_flags::CAPTURED) == 0
                })
                .map(|local| WriterOrigin::Parameter(local.raw())),
            Tag::Coerce
                if body.record(body.data[i][1]).first()
                    == Some(&(hd_tir::ir::Coercion::Identity as u32)) =>
            {
                Ref(body.data[i][0])
                    .as_inst()
                    .and_then(|value| origin[value.0 as usize])
            }
            _ => None,
        };
    }
    origin
}

/// One index of the call's at most 64 reference arguments, then one
/// reader walk. No reader rescans the writers or follows a place chain.
fn independent_writer_values(
    args: &[u32],
    candidates: u64,
    writable: u64,
    origins: &[Option<WriterOrigin>],
) -> u64 {
    let origin = |arg: u32| {
        Ref(arg)
            .as_inst()
            .and_then(|i| origins.get(i.0 as usize))
            .copied()
            .flatten()
    };
    let mut locals = HashSet::default();
    let mut external = false;
    for (k, &arg) in args.iter().enumerate().take(64) {
        if writable & (1 << k) == 0 {
            continue;
        }
        match origin(arg) {
            Some(WriterOrigin::Local(local)) => {
                locals.insert(local);
            }
            Some(WriterOrigin::Parameter(_)) => external = true,
            None => return 0,
        }
    }
    args.iter()
        .enumerate()
        .take(64)
        .filter(|&(k, &arg)| {
            candidates & (1 << k) != 0
                && origin(arg).is_some_and(|source| match source {
                    WriterOrigin::Local(local) => !locals.contains(&local),
                    WriterOrigin::Parameter(_) => !external,
                })
        })
        .fold(0, |mask, (k, _)| mask | 1 << k)
}

/// Count uses once for the caller, rather than walking it separately for
/// each writer's argument. Cleanup-only uses do not expose an address.
fn writer_uses(body: &Body) -> Vec<u32> {
    writer_uses_counted(body, || {})
}

fn writer_uses_counted(body: &Body, mut visit: impl FnMut()) -> Vec<u32> {
    let mut uses = vec![0u32; body.len()];
    for i in 0..body.len() {
        visit();
        if matches!(body.tags[i], Tag::Drop | Tag::LoanEnd) {
            continue;
        }
        hd_tir::ir::slots(body, i, |_, value, word| {
            if matches!(
                word,
                hd_tir::ir::Word::Value(_) | hd_tir::ir::Word::Provider
            ) {
                visit();
                if let Some(inst) = Ref(value).as_inst()
                    && let Some(count) = uses.get_mut(inst.0 as usize)
                {
                    *count = count.saturating_add(1);
                }
            }
        });
    }
    uses
}

/// Only the call reads this evaluated shared address. Replacing it by a
/// scalar read cannot change another use of the same reference value.
fn fresh_writer_args(body: &Body, call: usize, uses: &[u32]) -> u64 {
    hd_tir::inline::arguments(body, call)
        .iter()
        .enumerate()
        .take(64)
        .filter(|&(_, &arg)| {
            Ref(arg).as_inst().is_some_and(|inst| {
                let i = inst.0 as usize;
                body.tags[i] == Tag::Ref && body.data[i][1] & ref_word::MUT == 0 && uses[i] == 1
            })
        })
        .fold(0, |mask, (k, _)| mask | 1 << k)
}

fn writer_arguments(body: &Body, call: usize, mask: u64, origins: &[Option<WriterOrigin>]) -> bool {
    hd_tir::inline::arguments(body, call)
        .iter()
        .enumerate()
        .take(64)
        .filter(|&(k, _)| mask & (1 << k) != 0)
        .all(|(_, &arg)| {
            let Some(i) = Ref(arg).as_inst().map(|v| v.0 as usize) else {
                return false;
            };
            origins[i].is_some()
        })
}

/// A read of an unchanged reference parameter retains a visible origin
/// after substitution: the ownership walk follows the read back to that
/// parameter. References held in fields or call results lack this proof.
fn reference_read_args(body: &Body, call: usize) -> u64 {
    hd_tir::inline::arguments(body, call)
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
    hd_tir::inline::arguments(body, call)
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
    hd_tir::inline::arguments(body, call)
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
                    if mode.shares() {
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
                } else if n != 0 && !matches!(body.tags[j], Tag::Drop | Tag::LoanEnd) {
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
/// fill must follow the argument's kind too; otherwise ownership sees a
/// `&mut T` read of a `*mut T` local and inserts a page-table count.
fn keep_parameter_kind(pool: &InternPool, copy: &mut Body, param: usize) {
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
            let kind = copy.local_ty[local.idx()];
            if copy.ty[i] == kind || lends_by_kind(pool, kind, copy.ty[i]) {
                copy.ty[i] = kind;
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
                let kind = copy.ty[source];
                if copy.local_ty[local] == kind || lends_by_kind(pool, kind, copy.local_ty[local]) {
                    copy.local_ty[local] = kind;
                    locals[local] = true;
                }
            }
        }
    }
}

/// Whether each value a block of `copy` takes, as its tail or by a
/// `Break`, still has the block's type once parameters took their
/// arguments' kinds: a getter returning `self` would give the counted
/// reference where its result is the local one.
fn kinds_agree(copy: &Body) -> bool {
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

/// Whether a value of type `t` holds a local reference (`&T`), outside
/// function types.
fn holds_local(pool: &InternPool, t: Ty, depth: u32) -> bool {
    if depth > 32 {
        return false;
    }
    let any = |l| {
        pool.list_items(l)
            .iter()
            .any(|&a| holds_local(pool, a, depth + 1))
    };
    match pool.get(t) {
        TyData::Ref { counted, inner, .. } => !counted || holds_local(pool, inner, depth + 1),
        TyData::Adt { args, .. } => any(args),
        TyData::Tuple { elems, rest } => {
            any(elems) || rest.is_some_and(|r| holds_local(pool, r, depth + 1))
        }
        _ => false,
    }
}

/// Whether argument type `arg` differs from parameter type `want` only by
/// reference kinds: a counted argument lends itself to a local reference
/// parameter (`ref.convert.lend`), so a copy's parameter local takes the
/// argument's kind.
pub(crate) fn lends_by_kind(pool: &InternPool, arg: Ty, want: Ty) -> bool {
    let types = pool.types();
    arg != want && types.erase_ref_kinds(arg) == types.erase_ref_kinds(want)
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

/// `i`, a body position or a count of flags, as a `u32` word.
fn index_word(i: usize) -> u32 {
    // ruled out: TIR positions are u32 words, and a solve's flags are
    // instances, whose ids are u32 words too.
    u32::try_from(i).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use hd_base::SliceSort as _;
    use hd_tir::ir::TirSink as _;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use hd_base::DefId;
    use hd_tir::ir::{Body, BodyKind, ReadMode, Tag, local_flags, place};
    use hd_types::{InternPool, Ty, TyData};

    use super::{bottom_up, keep_parameter_kind};

    #[test]
    fn consumer_query_prepares_once_across_site_width_and_instance_depth() {
        for (width, depth) in [(1usize, 1u32), (1000, 1), (1, 20), (1000, 20)] {
            let memo = super::Memo::default();
            let mut preparations = 0;
            let mut copied_rows = 0;
            for _ in 0..width {
                for level in 0..depth {
                    let key = (hd_base::DefId::from_raw(1), hd_types::TyList(level));
                    let result = memo
                        .consumer_copy(
                            key,
                            || {
                                preparations += 1;
                                let n = hd_base::NodeIdx::from_raw(1);
                                let mut builder =
                                    hd_tir::ir::TirBuilder::new(key.0, hd_tir::ir::BodyKind::Fn);
                                let block = builder.open_block();
                                let value = builder.const_value(hd_types::Ty::I32, 0);
                                for _ in 1..super::AUTO_INLINE_LIMIT {
                                    builder.prim(
                                        hd_tir::ir::PrimOp::Add as u32,
                                        &[value, value],
                                        hd_types::Ty::I32,
                                        n,
                                    );
                                }
                                let root = builder.close_block(block, None, hd_types::Ty::VOID, n);
                                let body = builder
                                    .finish(root, &[])
                                    .expect("generated bounded consumer");
                                Some(super::LiteralConsumer {
                                    body,
                                    hash: hd_base::Hash128::default(),
                                    parameters: vec![true],
                                    loop_body: false,
                                })
                            },
                            |made| made.for_call(&[0], false),
                        )
                        .expect("admitted consumer");
                    copied_rows += result.0.len();
                    let nested = memo
                        .consumer_copy(
                            key,
                            || panic!("cached query"),
                            |made| made.for_call(&[0], true),
                        )
                        .expect("non-loop nested consumer");
                    copied_rows += nested.0.len();
                    assert!(
                        memo.consumer_copy(
                            key,
                            || panic!("cached query"),
                            |made| made.for_call(&[1], true)
                        )
                        .is_none()
                    );
                }
            }
            assert_eq!(preparations, depth);
            assert_eq!(
                copied_rows,
                2 * width * depth as usize * super::AUTO_INLINE_LIMIT
            );
            let mut failed = 0;
            for _ in 0..width {
                assert!(
                    memo.consumer_copy(
                        (hd_base::DefId::from_raw(2), hd_types::TyList::EMPTY),
                        || {
                            failed += 1;
                            None
                        },
                        |made| made.for_call(&[0], false)
                    )
                    .is_none()
                );
            }
            assert_eq!(failed, 1);
        }
    }

    #[test]
    fn nested_site_rejection_never_caches_a_root_site_rejection() {
        for size in [17, 31, 48] {
            for result in [Ty::VOID, Ty::I32, Ty::STRING] {
                let n = hd_base::NodeIdx::from_raw(1);
                let mut b = hd_tir::ir::TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
                let root = b.open_block();
                let loop_mark = b.open_loop();
                let inner = b.open_block();
                let value = b.const_value(Ty::I32, 1);
                for _ in 0..size {
                    b.prim(hd_tir::ir::PrimOp::Add as u32, &[value, value], Ty::I32, n);
                }
                let inner = b.close_block(inner, None, Ty::VOID, n);
                b.close_loop(loop_mark, inner, Ty::VOID, n);
                let value = match result {
                    Ty::VOID => None,
                    Ty::STRING => Some(b.const_str("result")),
                    _ => Some(value),
                };
                let root = b.close_block(root, value, result, n);
                let body = b.finish(root, &[]).expect("generated loop consumer");
                assert!(hd_tir::ir::verify_checked(&body).is_empty());
                let memo = super::Memo::default();
                let key = (body.item, hd_types::TyList::EMPTY);
                assert!(
                    memo.consumer_copy(
                        key,
                        || Some(super::LiteralConsumer {
                            loop_body: body.tags.contains(&Tag::Loop),
                            body,
                            hash: hd_base::Hash128::default(),
                            parameters: vec![true],
                        }),
                        |made| made.for_call(&[0], true)
                    )
                    .is_none()
                );
                assert!(
                    memo.consumer_copy(
                        key,
                        || panic!("context veto poisoned the cache"),
                        |made| made.for_call(&[0], false)
                    )
                    .is_some()
                );
                assert!(
                    memo.consumer_copy(
                        key,
                        || panic!("cached query"),
                        |made| made.for_call(&[0], true)
                    )
                    .is_none()
                );
            }
        }
    }

    #[test]
    fn consumer_metadata_rejection_work_does_not_grow_with_literal_table_width() {
        let mut previous = None;
        for width in [1000, 10000] {
            let mut builder =
                hd_tir::ir::TirBuilder::new(hd_base::DefId::from_raw(1), hd_tir::ir::BodyKind::Fn);
            let block = builder.open_block();
            for _ in 0..width {
                builder.const_str("x");
            }
            let root = builder.close_block(
                block,
                None,
                hd_types::Ty::VOID,
                hd_base::NodeIdx::from_raw(1),
            );
            let body = builder.finish(root, &[]).expect("generated literal table");
            let mut work = 0;
            assert!(!super::copy_metadata_fits_with(&body, || work += 1));
            assert!(work <= 16 * super::AUTO_INLINE_LIMIT + 15);
            if let Some(old) = previous {
                assert!(work <= old);
            }
            previous = Some(work);
        }
    }

    #[test]
    fn provider_metadata_width_and_depth_reject_in_constant_work() {
        use hd_base::DefId;
        use hd_tir::ir::{Body, BodyKind, Tag};
        use hd_types::Ty;
        let mut previous = None;
        for (width, depth) in [(1000, 20), (10000, 20), (1000, 200)] {
            let mut body = Body::new(DefId::from_raw(1), BodyKind::Fn);
            for _ in 0..depth {
                let at = super::index_word(body.extra.len());
                body.extra.push(super::index_word(2 * width));
                for _ in 0..width {
                    body.extra.extend([Ty::I32.0, 0]);
                }
                body.tags.push(Tag::With);
                body.data.push([at, 0]);
                body.ty.push(Ty::VOID);
            }
            let mut work = 0;
            assert!(!super::copy_metadata_fits_with(&body, || work += 1));
            assert!(work <= 14);
            if let Some(old) = previous {
                assert_eq!(work, old);
            }
            previous = Some(work);
        }
    }

    #[test]
    fn consumer_capture_origin_budget_rejects_before_scanning_rows() {
        use hd_base::{DefId, NodeIdx, SubId, Symbol};
        use hd_tir::ir::{BodyKind, TirBuilder, TirSink};
        use hd_types::Ty;
        for width in [350, 3_500] {
            let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let block = builder.open_block();
            let creator = builder.local(Ty::VOID, Symbol::from_raw(0), 0, NodeIdx::NONE);
            let snapshots: Vec<_> = (0..width)
                .map(|_| builder.local(Ty::VOID, Symbol::from_raw(0), 0, NodeIdx::NONE))
                .collect();
            let root = builder.close_block(block, None, Ty::VOID, NodeIdx::NONE);
            let mut body = builder
                .finish(root, &[])
                .expect("generated capture origins");
            body.inlined_capture_origins = snapshots
                .into_iter()
                .map(|snapshot| (SubId::from_raw(0), snapshot, creator))
                .collect();
            let mut visits = 0;
            assert!(!super::copy_metadata_fits_with(&body, || visits += 1));
            assert!(
                visits <= 14,
                "vector lengths reject without walking metadata"
            );
        }
    }

    #[test]
    fn scalar_writer_values_require_independent_storage() {
        use super::WriterOrigin::{Local, Parameter};
        let origins = [
            Some(Local(0)),
            Some(Local(1)),
            Some(Parameter(2)),
            Some(Parameter(3)),
            None,
        ];
        for (args, expected) in [
            ([0, 1], 1), // Different private locals.
            ([0, 0], 0), // Two parts of one local may overlap.
            ([0, 2], 1), // An external parameter cannot point at a private local.
            ([2, 0], 1),
            ([2, 3], 0), // Different parameters may point at the same storage.
            ([0, 4], 0), // Unknown writer origin keeps every reader's address.
        ] {
            assert_eq!(
                super::independent_writer_values(&args, 1, 2, &origins),
                expected
            );
        }
    }

    /// Width does not multiply projection depth: references to one place
    /// share its classification, including across identity SSA chains.
    #[test]
    fn scalar_writer_origins_share_deep_places_in_bounded_rounds() {
        for (width, depth) in [(1000usize, 20usize), (10000, 20), (20, 1000)] {
            let pool = InternPool::new();
            let mut nested = Ty::I32;
            for _ in 0..depth {
                nested = pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&[nested]),
                    rest: None,
                });
            }
            let reference = pool.intern_ty(&TyData::Ref {
                inner: Ty::I32,
                mutable: true,
                counted: false,
            });
            let mut body = Body::new(DefId::from_raw(1), BodyKind::Fn);
            body.local_ty = vec![nested];
            body.local_flags = vec![0];
            let mut words = vec![place::ROOT_LOCAL, 0];
            words.extend(std::iter::repeat_n(place::PROJ_FIELD, depth));
            body.extra.push(super::index_word(words.len()));
            body.extra.extend(words);
            let identity = super::push_record(&mut body, &[hd_tir::ir::Coercion::Identity as u32]);
            for i in 0..width {
                body.tags.push(Tag::Ref);
                body.data.push([0, hd_tir::ir::ref_word::MUT]);
                body.ty.push(reference);
                body.tags.push(Tag::Coerce);
                body.data.push([super::index_word(i * 2), identity]);
                body.ty.push(reference);
            }
            let mut visits = 0;
            for _ in 0..super::MAX_ROUNDS {
                let origins = super::writer_origins_counted(&body, || visits += 1);
                let uses = super::writer_uses_counted(&body, || visits += 1);
                assert_eq!(uses.iter().sum::<u32>(), super::index_word(width));
                assert!(
                    origins
                        .iter()
                        .all(|&private| private == Some(super::WriterOrigin::Local(0)))
                );
            }
            assert_eq!(visits, super::MAX_ROUNDS * (width * 5 + depth));
        }
    }

    /// A counted parameter flows through the unique binding local of an
    /// already-inlined method. A local with another write stays local:
    /// changing it would make that unrelated write ill typed.
    #[test]
    fn parameter_kind_follows_only_unique_binding_stores() {
        let pool = InternPool::new();
        let local = pool.intern_ty(&TyData::Ref {
            inner: Ty::I32,
            mutable: true,
            counted: false,
        });
        let counted = pool.intern_ty(&TyData::Ref {
            inner: Ty::I32,
            mutable: true,
            counted: true,
        });
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

        keep_parameter_kind(&pool, &mut body, 0);

        assert_eq!(body.ty[0], counted);
        assert_eq!(body.local_ty[1], counted);
        assert_eq!(body.ty[2], counted);
        assert_eq!(body.local_ty[2], local, "two writes keep the declared kind");
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
