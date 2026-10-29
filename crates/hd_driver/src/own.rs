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
use hd_opt::inline::{InlineEnv, InlineHint};
use hd_opt::structure_specialize::{GeneratedHandle, StructureEnv};
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
    /// The inliner's copies and the functions it found recursive.
    inline: hd_opt::inline::Memo,
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

/// The intrinsics that read only their arguments and write nothing
/// (`OwnCx::may_write`): the scalar square root, a trait value's downcasts
/// and type identity, which compare its type and return it or nothing,
/// the reads and conversions of a string, a character or a number, which
/// return a scalar or a new value, and the panic, which never returns.
/// One that writes through a `&mut` argument is not here: a caller's
/// passes read that write from `may_write` alone.
const PURE_INTRINSICS: &[&str] = &[
    "sqrt",
    "downcast_ref",
    "downcast_mut",
    "type_id_of",
    "bytes_len",
    "bytes_at",
    "bytes_index",
    "bytes_index_json",
    "bytes_index_pair",
    "bytes_mismatch",
    "utf8_valid_len",
    "bytes_slice",
    "bytes_concat",
    "concat_all",
    "string_to_bytes",
    "string_from_bytes",
    "string_lower",
    "string_upper",
    "char_from_scalar",
    "char_scalar",
    "format_int",
    "format_f64",
    "format_f32",
    "format_f64_fixed",
    "panic",
];

/// The intrinsics that own their value arguments, as a function's value
/// parameters do: `string_from_bytes` takes over the list's buffer when its
/// argument is the buffer's one holder (`crates/hd_wasm_linear/DESIGN.md`
/// §23.17), so the call consumes the list, moved in at its last use.
const OWNING_INTRINSICS: &[&str] = &["string_from_bytes", "weak_weaken"];

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
            TyData::Adt { def, .. } if self.run.known.ref_cells().holds(def) => Pass::Lent,
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

/// The inliner's view of the program (`hd_opt::inline`): the driver's
/// tables and decoded bodies, and the backend's layout facts.
impl InlineEnv for Env<'_> {
    fn pool(&self) -> &hd_types::InternPool {
        &self.run.pool
    }

    fn memo(&self) -> Option<&hd_opt::inline::Memo> {
        self.run.summaries.as_ref().map(|all| &all.inline)
    }

    fn module_text(&self, def: DefId) -> std::borrow::Cow<'_, str> {
        self.run.item_module_text(def)
    }

    fn inline_hint(&self, def: DefId) -> InlineHint {
        self.p
            .items
            .get(def)
            .map_or(InlineHint::Default, |i| match i.inline {
                hd_resolve::iface::INLINE_AUTO => InlineHint::Auto,
                hd_resolve::iface::INLINE_ALWAYS => InlineHint::Always,
                hd_resolve::iface::INLINE_NEVER | hd_resolve::iface::COLD => InlineHint::Never,
                _ => InlineHint::Default,
            })
    }

    fn signature(&self, def: DefId) -> Option<(Vec<Ty>, bool, bool)> {
        let sig = self.sig(def)?;
        Some((
            sig.params.iter().map(|&(_, t)| t).collect(),
            sig.suspends,
            sig.row != hd_types::RowId::EMPTY,
        ))
    }

    fn checked_body(&self, def: DefId) -> Option<&(Body, Hash128)> {
        self.run.decode(self.run.lazy_body(self.p, def)?)
    }

    fn fresh_body(&self, def: DefId) -> Option<Body> {
        let run = self.run;
        let (m, entry, at) = run.lazy_body(self.p, def)?.wire.as_ref()?;
        let mut fresh =
            hd_tir::wire::read_body(entry.get(at.clone())?, &run.pool, &run.paths, &run.syms)?;
        // A fetched package's `dbg` prints nothing (`module.dbg.dependency`):
        // the copy keeps the suppression `decode` gave its body.
        if run.fetched[usize::from(run.table.modules[*m].package)] {
            crate::dbg::suppress(&mut fresh, &run.names());
        }
        Some(fresh)
    }

    fn reference(&self, t: Ty) -> bool {
        self.layout_facts(t).1
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
            TyData::Adt { def, args } if self.run.known.ref_cells().holds(def) => {
                pool.list_items(args)[0]
            }
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
        let cell = self.run.known.ref_cells();
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
}

impl StructureEnv for Env<'_> {
    fn generated_structure_walk(&self, def: DefId) -> bool {
        hd_structure::is_generated_walk(&self.run.names(), def)
    }

    fn walker_trait(&self) -> DefId {
        self.run.known.walker
    }

    fn walker_data_variant(&self) -> DefId {
        self.run.known.walker_data_variant
    }

    fn walker_data_member(&self) -> DefId {
        self.run.known.walker_data_member
    }

    fn data_format_boundaries(&self) -> [DefId; 2] {
        [
            self.run.known.serializer_begin_data,
            self.run.known.serializer_end_data,
        ]
    }

    fn generated_structure_handle(&self, def: DefId) -> Option<GeneratedHandle> {
        match hd_structure::generated_body(&self.run.names(), def)? {
            hd_structure::GeneratedBody::Handle(hd_structure::GeneratedHandle::Variant {
                variant,
            }) => Some(GeneratedHandle::Variant { variant }),
            hd_structure::GeneratedBody::Handle(hd_structure::GeneratedHandle::Field {
                variant,
                field,
                ..
            }) => Some(GeneratedHandle::Field { variant, field }),
            _ => None,
        }
    }

    fn field_get(&self) -> DefId {
        self.run.known.field_get
    }

    fn variant_holds(&self) -> DefId {
        self.run.known.variant_holds
    }
}

impl Env<'_> {
    /// Inlined body `body`, with hash `hash` (`hd_opt::inline::inline_calls`), after
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
            let hash = hd_opt::structure_specialize::specialize_calls(env, &mut body, hash);
            let hash = hd_opt::inline::inline_calls(env, &mut body, hash);
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
                let hash = hd_opt::structure_specialize::specialize_calls(env, &mut body, *hash);
                let hash = hd_opt::inline::inline_calls(env, &mut body, hash);
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
            hd_opt::inline::bottom_up(root, &done, &mut callees, &mut own);
        });
    }
}
