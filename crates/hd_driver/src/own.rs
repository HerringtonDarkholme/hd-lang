//! The ownership passes' place in the driver (`docs/compiler/tir-v2.md`,
//! "Where and when the passes run", "What the passes read"): every body
//! the program reaches goes through `hd_tir::own` once, after decoding
//! (`Run::body_of`) or after collection generates it (a supplied or a
//! `dbg` body), and checked and debug builds verify the result
//! (`hd_tir::ir::verify_owned`). [`Env`] answers the passes' questions
//! from signatures and layouts, never from another body, and remembers
//! each answer for the run.

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
}

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
/// inline (`docs/compiler/tir-v2.md`, "The inliner", tuned at 8 in
/// commit 4: the smallest code of 8, 12, 16 and 20); 0 turns the inliner
/// off.
const INLINE_LIMIT: usize = 8;

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
        // which counts whatever it keeps itself (`Em::intrinsic`).
        if sig.suspends || sig.params.len() != n || self.intrinsic(def).is_some() {
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
        if cfg!(debug_assertions) || self.run.checked {
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

    /// Copies into `body` each call of a small std function outside
    /// `std.rt` (`docs/compiler/tir-v2.md`, "The inliner"), before the
    /// passes, each parameter bound by its pass mode, and returns the
    /// body's hash with each copied callee's TIR hash.
    fn inline_calls(&self, body: &mut Body, hash: Hash128) -> Hash128 {
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
            let Some((mut copy, callee_hash)) = self.inline_callee(def, body.item) else {
                continue;
            };
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
            if copy
                .ty
                .iter()
                .chain(&copy.local_ty)
                .any(|&t| pool.has_assoc(t))
            {
                continue;
            }
            hd_tir::inline::rechoose(&mut copy, &|t, s, a| self.select_impl(t, s, a));
            sites.push(Site { call: c, copy });
            hashes.push(callee_hash);
        }
        // All of the body's copies in one rebuild.
        let copied = hd_tir::inline::inline_calls(body, sites).unwrap_or_default();
        for (_, &h) in copied.iter().zip(&hashes).filter(|(c, _)| **c) {
            k.hash(h);
        }
        if copied.contains(&true) {
            k.finish()
        } else {
            hash
        }
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

    /// A fresh decoding of the checker's body of `def`, for the caller to
    /// change, and its hash, when a call of it inlines: a std function
    /// outside `std.rt`, not the caller, with a body of its own, that
    /// does not suspend, and that [`hd_tir::inline::inlinable`] allows at
    /// [`INLINE_LIMIT`].
    fn inline_callee(&self, def: DefId, caller: DefId) -> Option<(Body, Hash128)> {
        let module = self.run.item_module_text(def);
        let rt = module == "std.rt" || module.starts_with("std.rt.");
        if INLINE_LIMIT == 0
            || def == caller
            || !module.starts_with("std.")
            || rt
            || self.intrinsic(def).is_some()
        {
            return None;
        }
        if self.sig(def)?.suspends {
            return None;
        }
        let path = self.run.paths.hash(hd_base::PathId::from_raw(def.raw()));
        let lazy = self.p.bodies.all.get(&path)?;
        let (body, hash) = self.run.decode(lazy)?;
        if !hd_tir::inline::inlinable(body, INLINE_LIMIT) {
            return None;
        }
        let (_, entry, at) = lazy.wire.as_ref()?;
        let run = self.run;
        let fresh =
            hd_tir::wire::read_body(entry.get(at.clone())?, &run.pool, &run.paths, &run.syms)?;
        Some((fresh, *hash))
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
        if let (Some(all), Some(s)) = (&self.run.summaries, summary) {
            lock(&all.by_item).insert(body.item, s);
        }
        let mut k = StableHasher::new("own-passes");
        k.hash(hash);
        Ok((body, k.finish()))
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
                members.sort_by_cached_key(|&m| self.names().path(m));
                for &m in members.iter() {
                    if recursive {
                        lock(&all.cycle).insert(m, members[0]);
                    }
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
                members.sort_unstable();
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
