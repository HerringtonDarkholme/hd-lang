//! The ownership passes' place in the driver (`docs/compiler/tir-v2.md`,
//! "Where and when the passes run", "What the passes read"): every body
//! the program reaches goes through `hd_tir::own` once, after decoding
//! (`Run::body_of`) or after collection generates it (a supplied or a
//! `dbg` body), and checked and debug builds, and any build that asks
//! (`Packages::verify_own`), verify the result
//! (`hd_tir::ir::verify_owned`). [`Env`] answers the passes' questions
//! from signatures and layouts, never from another body, and remembers
//! each answer for the run.

use hd_base::SliceSort as _;
use std::cell::RefCell;
use std::fmt::Write as _;

use hd_base::HashMap;
use hd_base::{DefId, Hash128, NotImplemented, StableHasher, Stage, StageResult};
use hd_mono::ProgramEnv;
use hd_tir::inline::Site;
use hd_tir::ir::{Body, Callee, ChoiceKind};
use hd_tir::own::{OwnCx, OwnScratch, Pass, Summary, Tri};
use hd_types::solver::{
    BodyMemo, ConcreteTraitRef, ParamEnv, Selection, SolveCx, Solver, TableSolver, TraitRef,
};
use hd_types::{Ty, TyData, TyList};

use super::{Env, LazyBody, ProgramTables, Run, lock};

/// The run's release summaries (`docs/future-work/RELEASE_INTERPROC.md`,
/// "Data structures"): per body the passes ran over, what its code says of
/// its parameters. Only looked up, never iterated, so output stays
/// deterministic. Type checking never reads it.
#[derive(Default)]
pub(super) struct Summaries {
    by_item: std::sync::Mutex<HashMap<DefId, Summary>>,
    /// The members of recursive strongly connected components, each with
    /// its component's first member in path order: a call between two
    /// members reads the declared convention.
    cycle: std::sync::Mutex<HashMap<DefId, DefId>>,
    /// Whether each walked body may write memory its caller can reach
    /// (`hd_tir::own::writes`), known before the passes run over its
    /// component: `OwnCx::may_write` reads it for a call of the body.
    writes: std::sync::Mutex<HashMap<DefId, bool>>,
    /// Per summarized item outside a recursive component, the value
    /// parameters its summary lends ([`Env::lent_mask`]), once recorded.
    lent: std::sync::Mutex<HashMap<DefId, u64>>,
    /// Per item and parameter, the answer the passes first read
    /// ([`Env::summary_lends`]), which stays: a body and its emission read
    /// the same one, and a bit read as kept before the record stays kept.
    read: std::sync::Mutex<HashMap<(DefId, u32), bool>>,
    /// Per callee and type arguments, the copy a call of it inlines, its
    /// own calls copied first, and its hash, or `None` when a call of it
    /// stays ([`Env::callee_copy`]).
    inlined: std::sync::Mutex<HashMap<(DefId, TyList), Option<Inlined>>>,
    /// The functions the inliner found on a cycle of calls
    /// ([`Env::inline_into`]): a call of one stays a call.
    recursive: std::sync::Mutex<hd_base::HashSet<DefId>>,
}

/// A copy a call inlines, and its hash ([`Summaries`]).
type Inlined = (Body, Hash128);

/// The `own` view's line for `body`'s summary in `all`: its kept
/// parameters, by local, the one every result is, and whether it is
/// recursive; `summary none` for a function with none (one that suspends),
/// nothing for any other body.
pub(super) fn summary_line(body: &Body, all: &Summaries) -> String {
    use hd_tir::ir::BodyKind;
    let Some(s) = all.of(body.item) else {
        let function = matches!(body.kind, BodyKind::Fn | BodyKind::DeriveInstance);
        return if function { "summary none\n" } else { "" }.to_owned();
    };
    let params = hd_tir::own::params(body);
    let kept: Vec<String> = (params.iter().enumerate())
        .filter(|&(k, _)| k >= 64 || s.kept & (1 << k) != 0)
        .map(|(_, l)| format!("${l}"))
        .collect();
    let mut line = format!("summary kept [{}]", kept.join(" "));
    if let Some(&l) = s.returns.and_then(|k| params.get(usize::from(k))) {
        let _ = write!(line, " returns ${l}");
    }
    if s.writes {
        line.push_str(" writes");
    }
    if lock(&all.cycle).contains_key(&body.item) {
        line.push_str(" recursive");
    }
    line.push('\n');
    line
}

impl Summaries {
    /// The summary of `item`'s body, once the passes have run over it.
    pub(super) fn of(&self, item: DefId) -> Option<Summary> {
        lock(&self.by_item).get(&item).copied()
    }
}

thread_local! {
    /// The passes' tables, reused across every body a thread runs them on.
    static SCRATCH: RefCell<OwnScratch> = RefCell::new(OwnScratch::default());
    /// The rooting step's tables, for the bodies collection generates.
    static ROOTS: RefCell<hd_tir::ir::RootTables> = RefCell::new(hd_tir::ir::RootTables::default());
}

/// The most instructions a callee's body may have for a call of it to
/// inline, in std and outside it alike (`docs/compiler/tir-v2.md`, "The
/// inliner", tuned at 16 over 8, 12, 16 and 20 in #856: 12 leaves
/// `spectral_norm`'s `a(i, j)` (15 instructions) a call, which V8's
/// baseline tier runs twice as slowly, and 20 only grows code); 0 turns
/// the inliner off.
const INLINE_LIMIT: usize = 16;

/// The intrinsics that read only their arguments and write nothing
/// (`OwnCx::may_write`): the scalar square root and the byte reads of a
/// string, which return a scalar and never change memory.
const PURE_INTRINSICS: &[&str] = &[
    "sqrt",
    "bytes_len",
    "bytes_at",
    "bytes_index",
    "bytes_index_json",
    "bytes_index_pair",
    "bytes_mismatch",
    "utf8_valid_len",
];

/// The intrinsics that own their value arguments, as a function's value
/// parameters do: `string_from_bytes` takes over the list's buffer when its
/// argument is the buffer's one holder (`crates/hd_wasm_linear/DESIGN.md`
/// §23.17), so the call consumes the list, moved in at its last use.
const OWNING_INTRINSICS: &[&str] = &["string_from_bytes"];

impl OwnCx for Env<'_> {
    fn counted(&self, ty: Ty) -> Tri {
        if self.is_open(ty) {
            return Tri::Maybe;
        }
        if self.layout_facts(ty).0 {
            Tri::Yes
        } else {
            Tri::No
        }
    }

    fn pass(&self, callee: &Callee, k: usize, n: usize) -> Pass {
        // Release: a parameter the callee's summary lends, unless the
        // compiler supplies the code of the call (an intrinsic), whose
        // declared body is no summary of it.
        if self.run.summaries.is_some()
            && let Some(d) = match callee {
                Callee::Item { def, .. } => Some(*def),
                c @ Callee::TraitMethod { .. } => self.impl_target(c).map(|(d, _)| d),
            }
            && self.intrinsic(d).is_none()
            && self.summary_lends(d, k)
        {
            return Pass::Lent;
        }
        // The signature's parameters, the receiver first.
        let def = match callee {
            Callee::Item { def, .. } => *def,
            Callee::TraitMethod { method, .. } => *method,
        };
        let key = (
            def,
            u32::try_from(k).expect("arguments"),
            u32::try_from(n).expect("arguments"),
        );
        if let Some(&p) = lock(&self.run.own_params).get(&key) {
            return p;
        }
        let p = self.declared_pass(def, k, n);
        lock(&self.run.own_params).insert(key, p);
        p
    }

    fn pass_value(&self, fn_ty: Ty, k: usize, n: usize) -> Pass {
        let pool = &self.run.pool;
        let TyData::Fn {
            params, suspends, ..
        } = pool.get(hd_wasm_linear::strip_type(pool, self, fn_ty))
        else {
            return Pass::Lent;
        };
        let params = pool.list_items(params);
        if suspends || params.len() != n {
            return Pass::Lent;
        }
        self.value_parameter(params[k])
    }

    fn may_write(&self, callee: &Callee) -> bool {
        // Release: the walked body's own answer, unless the compiler
        // supplies the code of the call (an intrinsic).
        if let Some(all) = &self.run.summaries
            && let Some(def) = match callee {
                Callee::Item { def, .. } => Some(*def),
                c @ Callee::TraitMethod { .. } => self.impl_target(c).map(|(d, _)| d),
            }
            && self.intrinsic(def).is_none()
            && let Some(&w) = lock(&all.writes).get(&def)
        {
            return w;
        }
        // The compiler's scalar square root reaches nothing but its
        // argument (`list.rs`'s loop proofs allow it the same way), and
        // the byte reads of a string only read it ([`PURE_INTRINSICS`]).
        match callee {
            // `@intrinsic("sqrt")`, or `f64.sqrt` and `f32.sqrt`, body-less
            // methods of a float's inherent impl (`trait.impl.intrinsic`).
            Callee::Item { def, .. } => {
                // Whether it has a body, from the body table: decoding
                // one would read another body.
                let bodyless = !self
                    .p
                    .bodies
                    .all
                    .contains_key(&self.run.paths.hash(hd_base::PathId::from_raw(def.raw())));
                let method = bodyless
                    && self.run.names().path(*def).rsplit(['.', '/']).next() == Some("sqrt")
                    && self
                        .parent(*def)
                        .and_then(|(owner, _)| self.impl_head(owner))
                        .is_some_and(|(head, _, _)| {
                            matches!(self.run.pool.get(head), TyData::Prim(p) if p.is_float())
                        });
                !method
                    && !self
                        .intrinsic(*def)
                        .is_some_and(|key| PURE_INTRINSICS.contains(&key.as_str()))
            }
            Callee::TraitMethod {
                method,
                self_ty,
                choice: (ChoiceKind::Impl | ChoiceKind::Builtin, _),
                ..
            } => {
                let float = matches!(self.run.pool.get(*self_ty), TyData::Prim(p) if p.is_float());
                !(float && self.run.names().path(*method).rsplit(['.', '/']).next() == Some("sqrt"))
            }
            Callee::TraitMethod { .. } => true,
        }
    }

    fn received(&self, item: DefId, k: usize, n: usize) -> Pass {
        let Some(sig) = self.sig(item) else {
            return Pass::Owned;
        };
        if sig.suspends || sig.params.len() != n {
            return Pass::Owned;
        }
        self.value_parameter(self.low(sig.params[k].1))
    }

    fn suspending(&self, item: DefId) -> bool {
        ProgramEnv::suspends(self, item)
    }

    fn parts(&self, ty: Ty, variant: Option<u32>, out: &mut Vec<Ty>) -> bool {
        if self.is_open(ty) {
            out.clear();
            return false;
        }
        hd_wasm_linear::in_place_parts(&self.run.pool, self, ty, variant, out)
    }

    fn summarizes(&self) -> bool {
        self.run.summaries.is_some()
    }

    fn holds_coll(&self, ty: Ty) -> bool {
        let pool = &self.run.pool;
        self.is_open(ty)
            || hd_mono::holds_coll(pool, self, hd_wasm_linear::strip_type(pool, self, ty))
    }
}

impl Env<'_> {
    /// Whether `t` names a type parameter, an associated type or an opaque
    /// type, which an instance decides.
    fn is_open(&self, t: Ty) -> bool {
        let types = self.run.pool.types();
        types.has_param(t) || types.has_assoc(t) || types.has_opaque(t)
    }

    /// [`OwnCx::pass`] of parameter `k` of `def` called with `n`
    /// arguments, from its declaration. A body without a signature (a
    /// module's init) is never called.
    fn declared_pass(&self, def: DefId, k: usize, n: usize) -> Pass {
        let Some(sig) = self.sig(def) else {
            return Pass::Lent;
        };
        // A suspending callee's cold constructor counts what its frame
        // keeps; its callers lend every argument. So does a call of an
        // `@intrinsic` function, which the backend lowers in place and
        // which counts whatever it keeps itself (`Em::intrinsic`), unless
        // it takes over its argument ([`OWNING_INTRINSICS`]).
        if sig.suspends || sig.params.len() != n {
            return Pass::Lent;
        }
        if let Some(key) = self.intrinsic(def)
            && !OWNING_INTRINSICS.contains(&key.as_str())
        {
            return Pass::Lent;
        }
        // The declared type decides, unsubstituted: substituting would read
        // the callee's generics, which a default body's owner keeps in its
        // body, and the passes read no other body.
        self.value_parameter(self.low(sig.params[k].1))
    }

    /// How a parameter of declared type `t` takes its argument: owned when
    /// it is a value, or may be one in an instance (a type parameter, an
    /// associated type, a newtype over a type parameter); lent when it is
    /// a reference or holds nothing counted. An instance that makes an owned
    /// parameter a reference borrows the argument, and the backend releases
    /// the value the call consumed after it (`Em::release_untaken`); one
    /// that makes it hold nothing counted counts nothing.
    fn value_parameter(&self, t: Ty) -> Pass {
        let pool = &self.run.pool;
        let mut t = t;
        while let TyData::Mut(inner) = pool.get(t) {
            t = inner;
        }
        match pool.get(t) {
            TyData::Ref { .. } => Pass::Lent,
            TyData::Adt { def, .. } if def == self.run.known.ref_cell => Pass::Lent,
            _ if self.is_open(t) => Pass::Owned,
            _ => match self.layout_facts(t) {
                (true, false) => Pass::Owned,
                _ => Pass::Lent,
            },
        }
    }

    /// Whether a value of type `t`, which names no parameter, holds a
    /// counted leaf, and whether it is a reference: the backend's layout
    /// facts, once per run (a layout walk allocates as it goes).
    fn layout_facts(&self, t: Ty) -> (bool, bool) {
        if let Some(&facts) = lock(&self.run.own_types).get(&t) {
            return facts;
        }
        let pool = &self.run.pool;
        let facts = (
            hd_wasm_linear::counted_type(pool, self, t),
            hd_wasm_linear::reference_type(pool, self, t),
        );
        lock(&self.run.own_types).insert(t, facts);
        facts
    }

    /// Runs the ownership passes over `body`, then, in checked and debug
    /// builds, verifies their output; with `summarize`, also returns the
    /// body's summary (`hd_tir::own::summary`).
    pub(super) fn own_passes(
        &self,
        mut body: Body,
        summarize: bool,
    ) -> StageResult<(Body, Option<Summary>)> {
        // A body's passes never run another's; the tables are taken out
        // anyway, so a nested run would only start from empty ones.
        let mut scratch = SCRATCH.with(RefCell::take);
        // Each instruction names itself until the passes renumber the
        // body, the inliner's copies their call: the line table's anchors
        // stay the `check` entry's.
        if body.anchors.is_empty() {
            body.anchors = (0..u32::try_from(body.len()).expect("insts")).collect();
        }
        let returns = summarize
            .then(|| hd_tir::own::returned_param(&body))
            .flatten();
        // Each type's layout once per body, not once per query of it.
        let (refused, summary) = hd_wasm_linear::with_layout_cache(|| {
            let refused = hd_tir::own::run(&mut body, self, &mut scratch);
            let summary = (summarize && refused.is_ok())
                .then(|| hd_tir::own::summary(&body, self, &scratch, returns))
                .flatten();
            (refused, summary)
        });
        SCRATCH.with(|s| *s.borrow_mut() = scratch);
        let path = || self.run.names().path(body.item);
        if let Err(what) = refused {
            return Err(NotImplemented::bug(
                Stage::Collect,
                format!("the ownership passes refused `{}`: {what}", path()),
            ));
        }
        if cfg!(debug_assertions) || self.run.checked || self.run.verify_own {
            let errs = hd_tir::ir::verify_owned(&body, self);
            if let Some(e) = errs.first() {
                return Err(NotImplemented::bug(
                    Stage::Collect,
                    format!(
                        "the ownership passes' output for `{}` fails invariant {} at %{}: {} ({} findings)",
                        path(),
                        e.invariant,
                        e.inst,
                        e.what,
                        errs.len()
                    ),
                ));
            }
        }
        Ok((body, summary))
    }

    /// Copies into `body` each call of a small function
    /// [`Env::callee_copy`] allows (`docs/compiler/tir-v2.md`, "The
    /// inliner"), before the passes, each parameter bound by its pass mode,
    /// and returns the body's hash with each copied callee's TIR hash.
    fn inline_calls(&self, body: &mut Body, hash: Hash128) -> Hash128 {
        self.inline_into(body, hash, &mut vec![body.item])
    }

    /// [`Env::inline_calls`] inside the copies on `stack`, the caller
    /// first: a call of one of them stays a call, so recursion never
    /// inlines.
    fn inline_into(&self, body: &mut Body, hash: Hash128, stack: &mut Vec<DefId>) -> Hash128 {
        let pool = &self.run.pool;
        let mut k = StableHasher::new("inline");
        k.hash(hash);
        let (mut sites, mut hashes) = (Vec::new(), Vec::new());
        for c in (0..body.len()).rev() {
            let Some((def, targs)) =
                hd_tir::inline::plain_call(body, c).and_then(|callee| match callee {
                    Callee::Item { def, targs } => Some((def, targs)),
                    Callee::TraitMethod { .. } => self.impl_target(&callee),
                })
            else {
                continue;
            };
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
            // not open stays as it is: no copy names it ([`Env::fits_call`]).
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

    /// Records `members`, a run of calls that reaches its first member
    /// again, as recursive, in a release build: none of them is copied.
    fn found_cycle(&self, members: &[DefId]) {
        if let Some(all) = &self.run.summaries {
            lock(&all.recursive).extend(members);
        }
    }

    /// Whether `body` calls `def`, by name or through a trait method's
    /// impl.
    fn calls(&self, body: &Body, def: DefId) -> bool {
        (0..body.len()).any(|c| {
            hd_tir::inline::plain_call(body, c)
                .and_then(|callee| match callee {
                    Callee::Item { def, .. } => Some(def),
                    Callee::TraitMethod { .. } => self.impl_target(&callee).map(|(d, _)| d),
                })
                .is_some_and(|d| d == def)
        })
    }

    /// The copy a call of `def` at `targs` inlines, at the call's types,
    /// and its hash ([`Env::inline_callee`]). A release build inlines
    /// bottom-up: the instance's own calls are copied first, as far as
    /// their callees allow, and the limit applies to the result, so a
    /// function that only wraps a call of a leaf is a leaf itself once that
    /// call is a copy. A copy that outgrows the limit that way is the
    /// instance as written, when that fits. A function on a cycle of
    /// calls the walk finds stays a call. Each instance is made once per
    /// run.
    fn callee_copy(
        &self,
        def: DefId,
        targs: TyList,
        stack: &mut Vec<DefId>,
    ) -> Option<(Body, Hash128)> {
        let Some(all) = &self.run.summaries else {
            return (self.instance(def, targs))
                .and_then(|(copy, hash, fits)| fits.then_some((copy, hash)));
        };
        if let Some(made) = lock(&all.inlined).get(&(def, targs)) {
            return made.clone();
        }
        if lock(&all.recursive).contains(&def) {
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
            if lock(&all.recursive).contains(&def) {
                return None;
            }
            match nested {
                Some((nested, h)) if self.fits(def, targs, &nested) => Some((nested, h)),
                _ => fits.then_some((copy, hash)),
            }
        });
        lock(&all.inlined).insert((def, targs), made.clone());
        made
    }

    /// The callee's body at the call's types, with its hash, and whether
    /// it inlines as written ([`Env::fits`]); `None` when no copy of it
    /// can ([`Env::inline_callee`]), or its types do not settle.
    fn instance(&self, def: DefId, targs: TyList) -> Option<(Body, Hash128, bool)> {
        let pool = &self.run.pool;
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
    /// [`hd_tir::inline::inlinable`] at [`INLINE_LIMIT`], taking no
    /// reference ([`Env::takes_reference`]). The rule is one for std and
    /// user callees; only which callees a build considers differs
    /// ([`Env::inline_callee`]).
    fn fits(&self, def: DefId, targs: TyList, copy: &Body) -> bool {
        hd_tir::inline::inlinable(copy, INLINE_LIMIT) && !self.takes_reference(def, targs)
    }

    /// A fresh decoding of the checker's body of `def`, for the caller to
    /// change, and its hash, when a call of it may inline: a function
    /// outside `std.rt`, with a body of its own, that does not suspend
    /// or call itself, in std or in a release build (a dev build leaves user code as
    /// written, for stepping, stack traces and `dbg`), and that
    /// [`hd_tir::inline::inlinable`] allows at [`INLINE_LIMIT`]
    /// ([`Env::fits`]), which copying its calls only grows.
    fn inline_callee(&self, def: DefId) -> Option<(Body, Hash128)> {
        let module = self.run.item_module_text(def);
        let std = module.starts_with("std.");
        let rt = module == "std.rt" || module.starts_with("std.rt.");
        let release = self.run.summaries.is_some();
        if INLINE_LIMIT == 0 || rt || !(std || release) || self.intrinsic(def).is_some() {
            return None;
        }
        if self.sig(def)?.suspends {
            return None;
        }
        let path = self.run.paths.hash(hd_base::PathId::from_raw(def.raw()));
        let lazy = self.p.bodies.all.get(&path)?;
        let (body, hash) = self.run.decode(lazy)?;
        if !hd_tir::inline::inlinable(body, INLINE_LIMIT) || self.calls(body, def) {
            return None;
        }
        let (m, entry, at) = lazy.wire.as_ref()?;
        let run = self.run;
        let mut fresh =
            hd_tir::wire::read_body(entry.get(at.clone())?, &run.pool, &run.paths, &run.syms)?;
        // A fetched package's `dbg` prints nothing (`module.dbg.dependency`):
        // the copy keeps the suppression `decode` gave its body.
        if run.fetched[usize::from(run.table.modules[*m].package)] {
            crate::dbg::suppress(&mut fresh, &run.names());
        }
        Some((fresh, *hash))
    }

    /// Whether a parameter of `def` at `targs` is, or may be in an
    /// instance, a reference. A copy binds a parameter in a local, and a
    /// reference held in a local is counted, and makes a local it points
    /// at a counted cell, where a call lends both; so a callee that takes
    /// one stays a call (#856: copying `StringBuilder.push` into a loop
    /// made `string_build` 40% slower).
    fn takes_reference(&self, def: DefId, targs: TyList) -> bool {
        let Some(sig) = self.sig(def) else {
            return true;
        };
        let pool = &self.run.pool;
        sig.params.iter().any(|&(_, t)| {
            let mut t = self.low(t);
            if self.is_open(t) {
                t = hd_mono::subst(pool, self, def, targs, t);
            }
            while let TyData::Mut(inner) = pool.get(t) {
                t = inner;
            }
            t == Ty::POISON
                || self.is_open(t)
                || matches!(pool.get(t), TyData::Ref { .. })
                || matches!(pool.get(t), TyData::Adt { def, .. } if def == self.run.known.ref_cell)
                || self.layout_facts(t).1
        })
    }

    /// Whether `copy`, a callee's body at the types of call `c` of
    /// `caller`, takes the call's arguments and gives its result at the
    /// types they have once the caller's opaque results are opened
    /// ([`Env::inline_calls`]): each parameter's `Store` and the root's
    /// `Break` are then well typed. A `mut self` parameter of a type with
    /// no changeable copy of its own (a primitive, an enum, a collection)
    /// has type `mut T` while the call passes a `T`, a marker with `T`'s
    /// layout; its local takes the argument's type, as the call binds it.
    /// Any other difference keeps the call.
    fn fits_call(&self, caller: &Body, c: usize, copy: &mut Body) -> bool {
        let pool = &self.run.pool;
        let open = |t: Ty| hd_mono::reveal(pool, self, t);
        let fits = |got: Option<Ty>, want: Ty| {
            got.is_some_and(|got| got == want || got == Ty::NEVER || want == Ty::NEVER)
        };
        let r = caller.record(caller.data[c][1]);
        let args = &r[..r.len().saturating_sub(3)];
        let params = (0..copy.local_ty.len())
            .filter(|&l| copy.local_flags[l] & hd_tir::ir::local_flags::PARAM != 0);
        let mut n = 0;
        for (l, &a) in params.zip(args) {
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
            if matches!(pool.get(want), TyData::Mut(inner) if inner == arg) {
                copy.local_ty[l] = arg;
            } else if !fits(Some(arg), want) {
                return false;
            }
        }
        let root = copy.sub_root.first().map(|&r| copy.ty[r as usize]);
        n == args.len() && root.is_some_and(|root| fits(open(caller.ty[c]), root))
    }

    /// The item and type arguments a trait call chosen as an impl runs:
    /// the impl's method at the impl's arguments, matched from the receiver
    /// and the trait's arguments as collection selects, then the method's
    /// own; or the trait's default body at the receiver and the call's.
    fn impl_target(&self, callee: &Callee) -> Option<(DefId, TyList)> {
        let &Callee::TraitMethod {
            trait_,
            method,
            self_ty,
            targs,
            choice: (ChoiceKind::Impl, d),
        } = callee
        else {
            return None;
        };
        let (pool, impl_) = (&self.run.pool, DefId::from_raw(d));
        // A trait value receiver dispatches through its table, and a
        // derived newtype impl forwards to its base's, whatever the choice.
        let value = match pool.get(self_ty) {
            TyData::Adt { def, args } if def == self.run.known.ref_cell => pool.list_items(args)[0],
            _ => self_ty,
        };
        if hd_mono::is_trait_value(pool, value)
            || self.intrinsic(method).is_some()
            || self.is_derived_newtype_impl(impl_)
        {
            return None;
        }
        let items = pool.list_items(targs);
        let Some(m) = self.impl_method(impl_, method) else {
            return Some((method, pool.list(&[&[self_ty], items].concat())));
        };
        let (trait_args, own) = items.split_at(self.trait_arity(trait_).min(items.len()));
        let (head, heads, n) = self.impl_head(impl_)?;
        let mut out = vec![None; n];
        hd_mono::unify(pool, impl_, head, self_ty, &mut out);
        for (&x, &y) in pool.list_items(heads).iter().zip(trait_args) {
            hd_mono::unify(pool, impl_, x, y, &mut out);
        }
        let mut all = out.into_iter().collect::<Option<Vec<_>>>()?;
        all.extend_from_slice(own);
        Some((m, pool.list(&all)))
    }

    /// The impl of `trait_` at a concrete receiver and trait arguments, as
    /// collection selects it; `None` for an open type or a compiler-supplied
    /// implementation.
    fn select_impl(&self, trait_: DefId, self_ty: Ty, targs: TyList) -> Option<DefId> {
        let types = self.run.pool.types();
        let items = types.list_items(targs);
        let args = &items[..self.trait_arity(trait_).min(items.len())];
        if std::iter::once(&self_ty)
            .chain(args)
            .any(|&t| self.is_open(t))
        {
            return None;
        }
        let cell = self.run.known.ref_cell;
        let raised: Vec<Ty> = args.iter().map(|&a| types.raise_refs(cell, a)).collect();
        let tref = TraitRef {
            trait_,
            self_ty: types.raise_refs(cell, self_ty),
            args: types.list(&raised),
        };
        let (universe, global) = self.solving();
        let mut cx = SolveCx {
            pool: types,
            env: &ParamEnv::default(),
            universe,
            impls: self.impls,
            decls: self,
            body_memo: &mut BodyMemo::default(),
            global,
        };
        match TableSolver.select(&mut cx, ConcreteTraitRef(tref)).ok()? {
            Selection::Impl { impl_row, .. } => {
                Some(self.impls.table(impl_row.module)?.def[impl_row.row as usize])
            }
            Selection::Builtin(_) => None,
        }
    }

    /// A body collection generates (a supplied method's, a `dbg` body),
    /// after the ownership passes. Its places are rooted at their locals
    /// first, as the checker roots its bodies' (`hd_tir::ir::root_places`).
    pub(super) fn owned_generated(&self, mut body: Body) -> StageResult<Body> {
        ROOTS.with(|r| {
            let (types, item) = (self.run.pool.types(), body.item);
            let open = |t| hd_tir::ir::erasable(types, item, t);
            hd_tir::ir::root_places(&mut body, &open, &mut r.borrow_mut());
        });
        Ok(self.own_passes(body, false)?.0)
    }
}

impl Env<'_> {
    /// Inlined body `body`, with hash `hash` ([`Env::inline_calls`]), after
    /// the ownership passes, and its hash, which changes with them; in
    /// release, its summary is recorded.
    fn own_inlined(&self, body: Body, hash: Hash128) -> StageResult<(Body, Hash128)> {
        let (body, summary) = self.own_passes(body, self.run.summaries.is_some())?;
        if let (Some(all), Some(mut s)) = (&self.run.summaries, summary) {
            s.writes = lock(&all.writes).get(&body.item) != Some(&false);
            // The passes add no write the walk before them missed.
            if !s.writes && (self.run.checked || cfg!(debug_assertions)) {
                let path = self.run.names().path(body.item);
                assert!(
                    !hd_tir::own::writes(&body, self),
                    "{path}: writes after the passes"
                );
            }
            lock(&all.by_item).insert(body.item, s);
            // What it lends, but nothing of a recursive component's member
            // (its callers inside read the declared convention), nothing
            // emitted glue calls with owned arguments (the runtime and a
            // host boundary codec's items), and no bit the passes already
            // read as kept.
            let item = body.item;
            let glue = self.run.item_module_text(item).starts_with("std.rt")
                || self.json_items().is_some_and(|j| {
                    [
                        j.package_encode,
                        j.package_decode,
                        j.encode,
                        j.decode,
                        j.result_is_ok,
                        j.result_expect,
                        j.host_provider,
                    ]
                    .contains(&item)
                });
            let n = hd_tir::own::params(&body).len().min(64);
            let read = lock(&all.read);
            let mut lent = !s.kept
                & u64::MAX
                    .checked_shr(64 - u32::try_from(n).expect("params"))
                    .unwrap_or(0);
            if glue || lock(&all.cycle).contains_key(&item) {
                lent = 0;
            }
            for k in 0..u32::try_from(n).expect("params") {
                if read.get(&(item, k)) == Some(&false) {
                    lent &= !(1 << k);
                }
            }
            if lent != 0 {
                lock(&all.lent).insert(item, lent);
            }
        }
        let mut k = StableHasher::new("own-passes");
        k.hash(hash);
        Ok((body, k.finish()))
    }

    /// Release (`docs/future-work/RELEASE_INTERPROC.md`, "What each caller
    /// reads"): whether every caller passes parameter `k` of `def` with no
    /// count, as its summary lends it. The first answer stays, so a caller's
    /// passes and its emission agree; one read before the record is no.
    fn summary_lends(&self, def: DefId, k: usize) -> bool {
        let Some(all) = &self.run.summaries else {
            return false;
        };
        let key = (def, u32::try_from(k).expect("arguments"));
        let mut read = lock(&all.read);
        *read.entry(key).or_insert_with(|| {
            k < 64 && lock(&all.lent).get(&def).is_some_and(|m| m & (1 << k) != 0)
        })
    }

    /// The value parameters, the receiver first, that every instance of
    /// `def` receives lent ([`Env::summary_lends`]): emission takes no
    /// count of them on either side of a call.
    pub(super) fn lent_mask(&self, def: DefId) -> u64 {
        (self.run.summaries.as_ref())
            .and_then(|all| lock(&all.lent).get(&def).copied())
            .unwrap_or(0)
    }

    /// The callee scan: the items whose summaries inlined body `body`'s
    /// calls read, once each in call order: a direct call's callee, and a
    /// trait call's impl method where the call is chosen as an impl. Only
    /// items with a body of their own.
    fn summarized_callees(&self, body: &Body) -> Vec<DefId> {
        let (mut out, mut seen) = (Vec::new(), hd_base::HashSet::default());
        for i in (0..body.len()).filter(|&i| body.tags[i] == hd_tir::ir::Tag::Call) {
            let def = match Callee::from_words(body.record(body.data[i][0])) {
                Some(Callee::Item { def, .. }) => Some(def),
                Some(c) => self.impl_target(&c).map(|(d, _)| d),
                None => None,
            };
            if let Some(d) = def
                && self.run.lazy_body(self.p, d).is_some()
                && seen.insert(d)
            {
                out.push(d);
            }
        }
        out
    }
}

/// Visits the call graph from `root` bottom-up with an iterative Tarjan
/// walk, so a deep call chain cannot overflow the stack: `scc` gets each
/// strongly connected component once every component its members call
/// has had its turn, with whether it is recursive (two members, or one
/// that calls itself). `done` marks the nodes an earlier walk finished,
/// which this one skips; `callees` gives a node's edges, once per node.
pub(super) fn bottom_up<N: Copy + Eq + std::hash::Hash>(
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

impl Run<'_> {
    /// The body table's entry for `def`.
    pub(super) fn lazy_body<'p>(&self, p: &'p ProgramTables, def: DefId) -> Option<&'p LazyBody> {
        p.bodies
            .all
            .get(&self.paths.hash(hd_base::PathId::from_raw(def.raw())))
    }

    /// Runs `f` with the passes' view of the program.
    fn with_env<T>(&self, p: &ProgramTables, f: impl FnOnce(&Env<'_>) -> T) -> T {
        // The program's impls: a layout normalizes associated types.
        let folders = self.program_folders();
        let arity = p.arity();
        let view = self.program_view(p, &folders, &arity);
        f(&Env {
            run: self,
            p,
            impls: hd_types::solver::Impls::Owned(&view),
            sealed: self.known.sealed(),
        })
    }

    /// Body `body`, with TIR hash `hash`, after the ownership passes, and
    /// its hash, which changes with them; `None`, with the error noted for
    /// `Collect`, when the passes refuse it or their output fails its
    /// verification.
    pub(super) fn own_body(
        &self,
        p: &ProgramTables,
        body: Body,
        hash: Hash128,
    ) -> Option<(Body, Hash128)> {
        let owned = self.with_env(p, |env| {
            let mut body = body;
            let hash = env.inline_calls(&mut body, hash);
            env.own_inlined(body, hash)
        });
        match owned {
            Ok(owned) => Some(owned),
            Err(e) => {
                lock(&p.bodies.own_error).get_or_insert_with(|| e.to_string());
                None
            }
        }
    }

    /// Release (`docs/future-work/RELEASE_INTERPROC.md`, "Data flow"): owns
    /// the bodies `root` reaches through the callee scan, the callees'
    /// strongly connected components first, so every summary a body's
    /// passes may read is recorded before they run. Each component's
    /// members are owned in path order and marked when it is recursive. A
    /// body the passes refuse stays unowned, for `Run::body_of` to report.
    pub(super) fn owned_bottom_up(&self, p: &ProgramTables, root: DefId) {
        let Some(all) = &self.summaries else {
            return;
        };
        let done = |d: DefId| self.lazy_body(p, d).is_none_or(|b| b.owned.get().is_some());
        self.with_env(p, |env| {
            let inlined = RefCell::new(HashMap::default());
            let mut callees = |d: DefId| {
                let Some((raw, hash)) = self.lazy_body(p, d).and_then(|b| self.decode(b)) else {
                    return Vec::new();
                };
                let mut body = raw.clone();
                let hash = env.inline_calls(&mut body, *hash);
                let out = env.summarized_callees(&body);
                inlined.borrow_mut().insert(d, (body, hash));
                out
            };
            let mut own = |members: &mut Vec<DefId>, recursive: bool| {
                members.sort_stable_by_cached_key(|&m| self.names().path(m));
                // Writes, before the passes read them: the least fixed
                // point over the component, from every member writing
                // nothing (a member with no inlined body writes).
                let bodies = inlined.borrow();
                let mut writes = lock(&all.writes);
                for m in members.iter() {
                    writes.insert(*m, !bodies.contains_key(m));
                }
                drop(writes);
                let mut grew = true;
                while grew {
                    grew = false;
                    for m in members.iter() {
                        let known = lock(&all.writes)[m];
                        if !known && hd_tir::own::writes(&bodies[m].0, env) {
                            lock(&all.writes).insert(*m, true);
                            grew = true;
                        }
                    }
                }
                drop(bodies);
                if recursive {
                    for &m in members.iter() {
                        lock(&all.cycle).insert(m, members[0]);
                    }
                }
                for &m in members.iter() {
                    let body = inlined.borrow_mut().remove(&m);
                    if let (Some(lazy), Some((body, hash))) = (self.lazy_body(p, m), body)
                        && let Ok(owned) = env.own_inlined(body, hash)
                    {
                        let _ = lazy.owned.set(Some(owned));
                    }
                }
            };
            bottom_up(root, &done, &mut callees, &mut own);
        });
    }
}

#[cfg(test)]
mod tests {
    use hd_base::SliceSort as _;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::bottom_up;

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
