//! The ownership passes' place in the driver (`docs/compiler/tir-v2.md`,
//! "Where and when the passes run", "What the passes read"): every body
//! the program reaches goes through `hd_tir::own` once, after decoding
//! (`Run::body_of`) or after collection generates it (a supplied or a
//! `dbg` body); a std body in a dev build is owned once per toolchain
//! and settings and stored (`Run::owned_std_key`). Checked and debug
//! builds, and any build that asks
//! (`Packages::verify_own`), verify the result
//! (`hd_tir::ir::verify_owned`). [`Env`] answers the passes' own
//! questions from signatures and layouts, and remembers each answer for
//! the run. Three steps read other bodies, so a body's owned form depends
//! on its callees' bodies too: the folds and the inliner that run before
//! the passes ([`Env::fold_and_inline`]) copy callees' checked bodies,
//! argument promotion ([`Env::promoted_copy`]) reads a callee's owned
//! body, and a default body's result type is its checked body's root.

use hd_base::SliceSort as _;
use std::cell::RefCell;
use std::fmt::Write as _;

use hd_base::HashMap;
use hd_base::{DefId, Hash128, NotImplemented, StableHasher, Stage, StageResult};
use hd_mono::ProgramEnv;
use hd_mono::layout::LayoutEnv as _;
use hd_opt::fact_specialize::FactEnv;
use hd_opt::inline::{InlineEnv, InlineHint};
use hd_opt::structure_specialize::{GeneratedHandle, StructureEnv};
use hd_tir::ir::{Body, Callee, ChoiceKind, Tag};
use hd_tir::own::{CallMemory, OwnCx, OwnScratch, Pass, Summary, Tri, Uncopied};
use hd_types::solver::{
    BodyMemo, ConcreteTraitRef, ParamEnv, Selection, SolveCx, Solver, TableSolver, TraitRef,
};
use hd_types::{Ty, TyData, TyList};

use super::{Env, LazyBody, ProgramTables, Run, lock};

mod effects;
mod program;

thread_local! {
    /// What a checked count report names the program being built by: a
    /// test harness sets it before each build ([`copy_report_label`]).
    static COPY_LABEL: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Names the builds this thread runs next in a checked count report: a
/// conformance case's path.
pub fn copy_report_label(label: &str) {
    COPY_LABEL.with(|l| label.clone_into(&mut l.borrow_mut()));
}

/// The harness's source label also resolves a virtual `main.hd` in a
/// checked count report; it never participates in ownership decisions.
#[cfg(feature = "checked-report")]
pub(super) fn count_report_label() -> String {
    COPY_LABEL.with(|l| l.borrow().clone())
}

/// The run's release summaries (`docs/future-work/RELEASE_INTERPROC.md`,
/// "Data structures"): per instance the passes ran over, what its code
/// says of its parameters. Only looked up, never iterated, so output
/// stays deterministic. Type checking never reads it.
#[derive(Default)]
pub(super) struct Summaries {
    by_item: std::sync::Mutex<HashMap<Node, Summary>>,
    /// The members of recursive strongly connected components, each with
    /// its component's first member in path order: a call between two
    /// members reads the component's fixed point
    /// ([`Env::component_borrows`]).
    cycle: std::sync::Mutex<HashMap<Node, Node>>,
    /// Complete external/parameter write effects, known before the passes
    /// run over the component. Calls map the parameter mask through their
    /// actual reference roots; unknown external effects remain conservative.
    memory: std::sync::Mutex<HashMap<Node, hd_tir::own::memory_effects::Writes>>,
    effect_indices:
        std::sync::Mutex<HashMap<(DefId, Ty), std::sync::Arc<hd_tir::own::EffectIndex>>>,
    effect_depths: std::sync::Mutex<HashMap<Ty, u32>>,
    /// Per summarized instance, the value parameters its summary lends
    /// ([`Env::lent_mask`]), once recorded; a recursive component's
    /// member's are its fixed point. After collection, what each receives
    /// lent ([`Env::own_program`]).
    lent: std::sync::Mutex<HashMap<Node, u64>>,
    /// Per instance and parameter, the answer the passes first read
    /// ([`Env::summary_lends`]), which stays: a body and its emission read
    /// the same one, and a bit read as kept before the record stays kept.
    read: std::sync::Mutex<HashMap<(Node, u32), bool>>,
    /// While a recursive component is owned, each member's borrows, which
    /// [`Env::summary_lends`] answers without recording: assumed while
    /// its fixed point runs ([`Env::component_borrows`]), then the fixed
    /// point while the passes run over the members.
    assume: std::sync::Mutex<HashMap<Node, u64>>,
    /// The inliner's copies and the functions it found recursive.
    inline: hd_opt::inline::Memo,
    /// Per instance whose passes would borrow through a `&T` or `&mut T`
    /// parameter across a call that may write (#1118): the parameters
    /// that relies on, and the body after the passes in exclusive mode
    /// (`sub_flags::EXCLUSIVE`). Collection gives it to the calls whose
    /// arguments stage 7 keeps exclusive (`hd_tir::own::exclusive_call`).
    exclusive: std::sync::Mutex<HashMap<Node, Exclusive>>,
    /// Per item, type arguments and whether of the exclusive copy, its
    /// promoted copy (#1151, [`Env::promoted_copy`]), once asked.
    promoted: std::sync::Mutex<HashMap<PromotedKey, Option<Exclusive>>>,
}

/// The most parts by value an instance's type arguments may hold for the
/// instance to get a body of its own ([`Env::inlined_instance`]); past it
/// the item's generic body serves. The passes'
/// layout queries walk every part of a concrete type, and a type that
/// doubles at each level (`Pair[Pair[A, A], Pair[A, A]]`) has
/// exponentially many: owning its instances would not end before
/// collection reaches the depth limit (`instantiation-too-deep`).
const MAX_INSTANCE_LEAVES: u64 = 1 << 12;

/// How many parts the types of `args` hold: each type, and each of its
/// type arguments and members once per occurrence, as a value holds them.
/// Each distinct type is computed once, so this costs the number of
/// distinct types, not of parts.
fn instance_leaves(pool: &hd_types::InternPool, args: TyList) -> u64 {
    fn leaves(pool: &hd_types::InternPool, t: Ty, memo: &mut HashMap<Ty, u64>) -> u64 {
        if let Some(&n) = memo.get(&t) {
            return n;
        }
        let sum = |ts: &[Ty], memo: &mut HashMap<Ty, u64>| {
            ts.iter()
                .fold(1u64, |n, &u| n.saturating_add(leaves(pool, u, memo)))
        };
        let n = match pool.get(t) {
            TyData::Tuple { elems, rest } => {
                let mut ts = pool.list_items(elems).to_vec();
                ts.extend(rest);
                sum(&ts, memo)
            }
            TyData::Adt { args, .. } => sum(pool.list_items(args), memo),
            TyData::Union(ms) => sum(pool.list_items(ms), memo),
            TyData::Ref { inner, .. } => sum(&[inner], memo),
            _ => 1,
        };
        memo.insert(t, n);
        n
    }
    let mut memo = HashMap::default();
    pool.list_items(args)
        .iter()
        .fold(0u64, |n, &t| n.saturating_add(leaves(pool, t, &mut memo)))
}

/// An instance the summaries describe: an item and its type arguments as
/// collection keys them (`hd_mono::rowless`). A non-generic item's, and
/// a generic item's own body's, has none.
pub(super) type Node = (DefId, TyList);

#[derive(Clone, Copy)]
pub(super) struct ColdSignature {
    item: DefId,
    params: TyList,
    dynamic_receiver: bool,
}

/// A promoted copy's item, type arguments, whether of the exclusive copy,
/// and the parameters its call may pass by value and lend with no count
/// ([`Env::promoted_copy`]).
type PromotedKey = (DefId, TyList, bool, u64, u64);

/// An exclusive copy: the parameters it relies on, and the body after the
/// passes ([`Summaries::exclusive`]).
pub(super) type Exclusive = (u64, std::sync::Arc<Body>);

/// The `own` view's line for `body`'s summary in `all`: its kept
/// parameters, by local, those it borrows, the one every result is, and
/// whether it is
/// recursive; `summary none` for a function with none (one that suspends),
/// nothing for any other body.
pub(super) fn summary_line(body: &Body, all: &Summaries) -> String {
    use hd_tir::ir::BodyKind;
    let node = (body.item, TyList::EMPTY);
    let Some(s) = all.of(node) else {
        let function = matches!(body.kind, BodyKind::Fn | BodyKind::DeriveInstance);
        return if function { "summary none\n" } else { "" }.to_owned();
    };
    let params = hd_tir::own::params(body);
    let kept: Vec<String> = (params.iter().enumerate())
        .filter(|&(k, _)| k >= 64 || s.kept & (1 << k) != 0)
        .map(|(_, l)| format!("${l}"))
        .collect();
    let mut line = format!("summary kept [{}]", kept.join(" "));
    let borrows: Vec<String> = (params.iter().enumerate())
        .filter(|&(k, _)| k < 64 && s.borrows & (1 << k) != 0)
        .map(|(_, l)| format!("${l}"))
        .collect();
    if !borrows.is_empty() {
        let _ = write!(line, " borrows [{}]", borrows.join(" "));
    }
    if let Some(&l) = s.returns.and_then(|k| params.get(usize::from(k))) {
        let _ = write!(line, " returns ${l}");
    }
    if s.writes {
        line.push_str(" writes");
    }
    if let Some((relies, _)) = all.exclusive(node) {
        let on: Vec<String> = (params.iter().enumerate())
            .filter(|&(k, _)| k < 64 && relies & (1 << k) != 0)
            .map(|(_, l)| format!("${l}"))
            .collect();
        let _ = write!(line, " relies [{}]", on.join(" "));
    }
    if lock(&all.cycle).contains_key(&node) {
        line.push_str(" recursive");
    }
    line.push('\n');
    line
}

impl Summaries {
    /// The summary of instance `node`'s body, once the passes have run
    /// over it.
    pub(super) fn of(&self, node: Node) -> Option<Summary> {
        lock(&self.by_item).get(&node).copied()
    }

    /// Instance `node`'s exclusive copy and the parameters it relies on,
    /// if its passes would borrow differently in exclusive mode.
    pub(super) fn exclusive(&self, node: Node) -> Option<Exclusive> {
        lock(&self.exclusive).get(&node).cloned()
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
    "ptr_offset",
    "mem_string_bytes",
    "ptr_size_of",
    "ptr_ref",
    "ptr_mut",
    "downcast_ref",
    "downcast_mut",
    "type_id_of",
    "bytes_len",
    "bytes_at",
    "bytes_index",
    "bytes_index_set",
    "bytes_index_pair",
    "bytes_mismatch",
    "utf8_valid_len",
    "bytes_slice",
    "string_to_bytes",
    "string_from_bytes",
    "string_lower",
    "string_upper",
    "char_from_scalar",
    "char_scalar",
    "format_f64",
    "format_f32",
    "format_f64_fixed",
    "panic",
    "panic_message",
];

/// The intrinsics that own their value arguments, as a function's value
/// parameters do: `string_from_bytes` takes over the list's buffer when its
/// argument is the buffer's one holder (`crates/hd_wasm/DESIGN.md`
/// §23.17), so the call consumes the list, moved in at its last use.
const OWNING_INTRINSICS: &[&str] = &["string_from_bytes"];

impl Env<'_> {
    /// The compiler-known raw-memory relation of an intrinsic. This names
    /// intrinsic semantics, never an ordinary hd function: a body such as
    /// `concat_all` earns its summary from the write walk over its code.
    fn declared_call_memory(&self, callee: &Callee) -> CallMemory {
        let Callee::Item { def, .. } = callee else {
            return CallMemory::None;
        };
        match self.intrinsic(*def).as_deref() {
            Some("mem_new_string") => CallMemory::Allocate,
            Some("mem_string_bytes" | "ptr_offset") => CallMemory::Derive(0),
            Some(
                "mem_store_u8" | "mem_store_u16" | "mem_store_u32" | "mem_store_u64" | "mem_copy"
                | "mem_fill",
            ) => CallMemory::Write(0),
            Some("format_int") => CallMemory::Write(1),
            _ => CallMemory::None,
        }
    }

    /// [`OwnCx::may_write`] without a release summary: declarations and
    /// the generator's cached proof of a whole factory family. No query
    /// recursively owns another body.
    fn declared_may_write(&self, callee: &Callee) -> bool {
        if let Callee::Item { def, targs } = *callee
            && matches!(
                hd_structure::generated_body(&self.run.names(), def),
                Some(hd_structure::GeneratedBody::Handle(_))
            )
            && let Some(&target) = self.run.pool.list_items(targs).first()
            && !self.is_open(target)
            && let Ok(Some(family)) = self.raw_supplied_family(def, target)
            && family.index.contains_key(&def)
            && family.factories_read_only
        {
            return false;
        }
        self.intrinsic_may_write(callee)
    }

    /// The base declaration policy, also used while proving generated
    /// families. It does not enter another generated-family query.
    fn intrinsic_may_write(&self, callee: &Callee) -> bool {
        // The compiler's scalar square root reaches nothing but its
        // argument (`list.rs`'s loop proofs allow it the same way), and
        // the byte reads of a string only read it ([`PURE_INTRINSICS`]).
        match callee {
            // `@intrinsic("sqrt")`, or `f64.sqrt` and `f32.sqrt`, body-less
            // methods of a float's inherent impl (`trait.impl.intrinsic`).
            Callee::Item { def, .. } => {
                // Whether it has a body, from the body table: decoding
                // one would read another body.
                let bodyless = self.run.lazy_body(self.p, *def).is_none();
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
                // The compiler's own code for a scalar's comparison and a
                // float's square root reads its arguments and nothing else.
                let names = self.run.names();
                let name = names.path(*method);
                let name = name.rsplit(['.', '/']).next();
                let TyData::Prim(p) = self.run.pool.get(*self_ty) else {
                    return true;
                };
                !(matches!(name, Some("eq" | "cmp" | "partial_cmp"))
                    || (p.is_float() && name == Some("sqrt")))
            }
            Callee::TraitMethod { .. } => true,
        }
    }
}

impl OwnCx for Env<'_> {
    fn temporary_owner_name(&self) -> hd_base::Symbol {
        self.run.syms.intern("$temporary_owner")
    }

    fn parameter_count(&self, item: DefId) -> Option<usize> {
        self.sig(item).map(|sig| self.arity(item, sig))
    }

    fn holds_drop(&self, ty: Ty) -> Tri {
        if self.is_open(ty) {
            Tri::Maybe
        } else if hd_wasm::holds_drop_type(&self.run.pool, self, ty) {
            Tri::Yes
        } else {
            Tri::No
        }
    }

    fn function_suspends(&self, ty: Ty) -> bool {
        matches!(self.run.pool.get(ty), TyData::Fn { suspends: true, .. })
    }

    fn function_spreads_arguments(&self, ty: Ty, args: usize) -> bool {
        matches!(self.run.pool.get(ty), TyData::Fn { params, inputs, .. }
            if inputs == hd_types::Inputs::Tuple || self.run.pool.list_items(params).len() != args)
    }

    fn function_has_requirement_row(&self, ty: Ty) -> bool {
        let TyData::Fn { row, .. } = self.run.pool.get(ty) else {
            return false;
        };
        let row = self.run.pool.row_data(row);
        !row.keys.is_empty() || !row.params.is_empty()
    }

    fn dynamic_receiver_may_load(&self, callee: &Callee, receiver: Ty) -> bool {
        let Callee::TraitMethod {
            trait_,
            method,
            self_ty,
            ..
        } = *callee
        else {
            return false;
        };
        let pool = &self.run.pool;
        let TyData::Ref { inner, .. } = pool.get(receiver) else {
            return false;
        };
        if self.ref_target_or(inner) == inner && !self.is_open(inner) {
            return false;
        }
        let value = self.ref_target_or(self_ty);
        (hd_mono::is_trait_value(pool, value) || self.is_open(value))
            && self.trait_method_dispatches(trait_, method)
            && self.sig(method).is_none_or(|sig| sig.generics.is_empty())
    }

    fn call_has_requirement_context(&self, callee: &Callee) -> bool {
        let chosen = self.chosen(callee);
        let instance = match chosen {
            Callee::Item { def, targs } => Some((def, targs)),
            Callee::TraitMethod { .. } => self.impl_target(&chosen),
        };
        if let Some((def, args)) = instance {
            if self.intrinsic(def).is_some() {
                return false;
            }
            // Read the call's row arguments before node() makes them open.
            // An unresolved argument may acquire keys in its instance.
            let args = self.run.pool.list_items(args);
            return self
                .generic_rows(self.generics_owner(def))
                .into_iter()
                .enumerate()
                .any(|(j, row)| {
                    row && args.get(j).is_none_or(|&t| {
                        let TyData::Row(r) = self.run.pool.get(t) else {
                            return true;
                        };
                        let r = self.run.pool.row_data(r);
                        !r.keys.is_empty() || !r.params.is_empty()
                    })
                });
        }
        // A dynamic slot packages its row's keys at the trait value's
        // arguments too. An unresolved bound or argument keeps the same
        // conditional contract until its instance selects a concrete
        // signature.
        let Callee::TraitMethod { method, targs, .. } = chosen else {
            return false;
        };
        if self.intrinsic(method).is_some() {
            return false;
        }
        let Some(sig) = self.sig(method) else {
            return self.has_requirement_row(method);
        };
        // The method's row at the trait's arguments, `Self` left out.
        let pool = &self.run.pool;
        let mut all = vec![hd_mono::class_ref(pool)];
        all.extend(pool.list_items(targs).iter().copied());
        let row = pool.intern_ty(&TyData::Row(sig.row));
        let row = match pool.get(hd_mono::subst(pool, self, method, pool.list(&all), row)) {
            TyData::Row(r) => pool.row_data(r),
            _ => pool.row_data(sig.row),
        };
        !row.keys.is_empty() || !row.params.is_empty() || hd_mono::has_row_params(self, method)
    }

    fn has_requirement_row(&self, item: DefId) -> bool {
        self.sig(item).map_or_else(
            || !self.init_keys(item).is_empty(),
            |sig| {
                !self.run.pool.row_data(sig.row).keys.is_empty()
                    || hd_mono::has_row_params(self, item)
            },
        )
    }

    fn retains_result(&self, callee: &Callee) -> bool {
        matches!(callee, Callee::Item { def, .. } if self.intrinsic(*def).as_deref() == Some("bytes_slice"))
    }

    fn copies_map_snapshot(&self, callee: &Callee) -> bool {
        matches!(callee, Callee::Item { def, .. } if self.intrinsic(*def).as_deref() == Some("map_steps"))
    }

    fn is_map_probe(&self, callee: &Callee) -> bool {
        matches!(callee, Callee::Item { def, .. } if self.intrinsic(*def).as_deref() == Some("map_probe"))
    }

    fn counted(&self, ty: Ty) -> bool {
        // A generic body's open type may count in an instance.
        self.is_open(ty) || self.layout_facts(ty).0
    }

    fn may_frame_referent(&self, ty: Ty) -> bool {
        self.is_open(ty) || hd_wasm::frame_referent_type(&self.run.pool, self, ty)
    }

    fn single_counted_address(&self, ty: Ty) -> bool {
        !self.is_open(ty) && hd_wasm::single_counted_address_type(&self.run.pool, self, ty)
    }

    fn thin_counted_reference(&self, ty: Ty) -> bool {
        !self.is_open(ty) && hd_wasm::thin_counted_reference_type(&self.run.pool, self, ty)
    }

    fn reference_target(&self, ty: Ty) -> Option<Ty> {
        (!self.is_open(ty))
            .then(|| hd_wasm::reference_target_type(&self.run.pool, self, ty))
            .flatten()
    }

    fn trait_reference_widens(&self, from: Ty, to: Ty) -> bool {
        !self.is_open(from)
            && !self.is_open(to)
            && hd_mono::trait_reference_widens(&self.run.pool, self, from, to)
    }

    fn plain_reference(&self, ty: Ty) -> bool {
        !self.is_open(ty) && hd_wasm::plain_reference_type(&self.run.pool, self, ty)
    }

    fn plain_result(&self, callee: &Callee) -> bool {
        let Callee::Item { def, .. } = callee else {
            return false;
        };
        matches!(self.intrinsic(*def).as_deref(), Some("ptr_ref" | "ptr_mut"))
    }

    fn pass(&self, callee: &Callee, k: usize, n: usize) -> Pass {
        if let Some((def, ty)) = self.cold_parameter(callee, k, n) {
            return if self.cold_transfers_declared(def, ty) || self.cold_moves(ty) {
                self.value_parameter(ty)
            } else {
                Pass::Lent
            };
        }
        let chosen = self.chosen(callee);
        let callee = &chosen;
        // Release: a parameter the callee's summary lends, unless the
        // compiler supplies the code of the call (an intrinsic), whose
        // declared body is no summary of it.
        if self.run.summaries.is_some()
            && let Some(node) = self.callee_node(callee)
            && self.intrinsic(node.0).is_none()
            && self.summary_lends(node, k)
        {
            return Pass::Lent;
        }
        // A call through a trait value lends its receiver to its slot, which
        // takes any count its concrete target owns (the backend's
        // `Em::borrowed_arg`). It lends an argument too, unless the call takes
        // it (`Em::push_dyn`): an open value (`Em::dyn_takes`), whose type is
        // open in a generic method's slot or at the caller's instance, or a
        // value holding a `Drop` hook, which moves.
        // A cold method's explicit arguments use the selected mode above.
        // Its erased receiver still lends its payload through the adapter.
        // An intrinsic supplies its own ownership behavior.
        if let Callee::TraitMethod {
            method,
            self_ty,
            targs,
            choice: (ChoiceKind::TraitValue, _),
            ..
        } = callee
            && self.intrinsic(*method).is_none()
            && self.sig(*method).is_some_and(|sig| {
                sig.params.len() == n
                    && (k == 0 || !sig.suspends && self.slot_lends(*method, *self_ty, *targs, k))
            })
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
        let cached = lock(&self.run.own_params).get(&key).copied();
        let p = cached.unwrap_or_else(|| {
            let p = self.declared_pass(def, k, n);
            lock(&self.run.own_params).insert(key, p);
            p
        });
        if p != Pass::Owned {
            return p;
        }
        // A call names its instance, directly or through the impl chosen:
        // the instance's parameter type decides, as the backend's
        // `Em::borrowed_arg` reads it (`id(&u.name)` borrows).
        let instance = match *callee {
            Callee::Item { def, targs } => Some((def, targs)),
            Callee::TraitMethod { .. } => self.impl_target(callee),
        };
        if let Some((def, targs)) = instance {
            return self.instance_pass(def, targs, k, n).unwrap_or(p);
        }
        p
    }

    fn cold_argument_transfers(&self, callee: &Callee, k: usize, n: usize) -> bool {
        self.cold_parameter(callee, k, n)
            .is_some_and(|(def, ty)| self.cold_transfers_declared(def, ty))
    }

    fn call_argument_may_own(&self, callee: &Callee, k: usize, n: usize) -> bool {
        let chosen = self.chosen(callee);
        let target = match chosen {
            Callee::Item { def, targs } => Some((def, targs)),
            Callee::TraitMethod { .. } => self.impl_target(&chosen),
        };
        let Some((def, args)) = target else {
            return false;
        };
        if self.intrinsic(def).is_some()
            || ProgramEnv::suspends(self, def)
            || !ProgramEnv::has_body(self, def)
        {
            return false;
        }
        let Some(sig) = self
            .sig(def)
            .filter(|s| !s.suspends && self.arity(def, s) == n && k < n)
        else {
            return false;
        };
        let ty = hd_mono::subst(&self.run.pool, self, def, args, self.param_ty(sig, k));
        ty != Ty::POISON && self.value_parameter(ty) == Pass::Owned
    }

    fn pass_value(&self, fn_ty: Ty, k: usize, n: usize) -> Pass {
        let pool = &self.run.pool;
        let TyData::Fn {
            params, suspends, ..
        } = pool.get(hd_wasm::strip_type(pool, self, fn_ty))
        else {
            return Pass::Lent;
        };
        let params = pool.list_items(params);
        if params.len() != n {
            // A spread transfers its tuple's native components; its count
            // carrier is retained independently by the argument packet.
            return Pass::Owned;
        }
        let ty = params[k];
        if self.copies(ty) != Tri::Yes && self.value_parameter(ty) == Pass::Owned {
            return Pass::Owned;
        }
        if suspends
            && !ProgramEnv::function_value_transfers(self, params[k])
            && !self.cold_moves(params[k])
        {
            return Pass::Lent;
        }
        self.function_parameter(ty)
    }

    fn function_lends_counts(&self, fn_ty: Ty, k: usize, n: usize) -> bool {
        let pool = &self.run.pool;
        let TyData::Fn { params, .. } = pool.get(hd_wasm::strip_type(pool, self, fn_ty)) else {
            return false;
        };
        let params = pool.list_items(params);
        params.len() != n
            || params
                .get(k)
                .is_some_and(|&ty| self.is_open(ty) || self.function_parameter(ty) == Pass::Lent)
    }

    fn keeps(&self, callee: &Callee, k: usize, _: usize) -> bool {
        // Release: a parameter the callee's summary lends, which it never
        // stores, captures or returns, unless the compiler supplies the
        // call's code (an intrinsic) or the callee suspends, whose frame
        // keeps every parameter. Read-only: no summary read is recorded.
        if self.run.summaries.is_some()
            && let Some(node) = self.callee_node(callee)
            && self.intrinsic(node.0).is_none()
            && !ProgramEnv::suspends(self, node.0)
            && k < 64
            && self.lent_mask(node) & (1 << k) != 0
        {
            return false;
        }
        true
    }

    fn stores_arg(&self, callee: &Callee, into: usize, from: usize) -> bool {
        let Callee::Item { def, .. } = *callee else {
            return true;
        };
        let Some(sig) = self.sig(def).filter(|s| !s.suspends) else {
            return true;
        };
        if self.intrinsic(def).is_some() {
            return true;
        }
        let pool = self.run.pool.types();
        let (Some(&(_, dest)), Some(&(_, source))) = (sig.params.get(into), sig.params.get(from))
        else {
            return true;
        };
        // Generic slots and hidden captures have dependency variables
        // beyond the written local-reference slots. Keep their existing
        // conservative relation until that full map is available here.
        if [dest, source]
            .into_iter()
            .any(|t| pool.has_param(t) || pool.has_fn(t) || pool.has_opaque(t) || pool.has_assoc(t))
        {
            return true;
        }
        let TyData::Ref {
            counted: false,
            mutable: true,
            ..
        } = pool.get(dest)
        else {
            return true;
        };
        let slots = |p: usize| {
            let start: usize = sig.params[..p]
                .iter()
                .map(|&(_, t)| pool.ref_slots(t))
                .sum();
            start..start + pool.ref_slots(sig.params[p].1)
        };
        // The outer destination reference lends the destination's own
        // storage; only its referent's slots can keep another argument.
        // Elided input slots are distinct anonymous lifetimes, not one
        // shared lifetime numbered zero (ref.dep.slot).
        slots(into).skip(1).any(|d| {
            sig.lifetimes.get(d).is_some_and(|&life| {
                life != 0 && slots(from).any(|s| sig.lifetimes.get(s) == Some(&life))
            })
        })
    }

    fn may_write(&self, callee: &Callee) -> bool {
        self.call_writes(callee).any()
    }

    fn call_writes(&self, callee: &Callee) -> hd_tir::own::memory_effects::Writes {
        use hd_tir::own::memory_effects::Writes;
        let node = self.callee_node(callee);
        // A cold constructor and its later poll/frame cleanup do not have
        // the same effects. Do not discharge deferred parameter writes at
        // the construction call using an ordinary body's summary.
        if node.is_some_and(|node| ProgramEnv::suspends(self, node.0)) {
            return Writes::EXTERNAL;
        }
        if let Some(all) = &self.run.summaries
            && let Some(node) = node
            && self.intrinsic(node.0).is_none()
            && let Some(&effects) = lock(&all.memory).get(&node)
        {
            return effects;
        }
        if self.declared_may_write(callee) {
            Writes::EXTERNAL
        } else {
            Writes::default()
        }
    }

    fn call_memory(&self, callee: &Callee) -> CallMemory {
        self.declared_call_memory(callee)
    }

    fn received(&self, item: DefId, k: usize, n: usize) -> Pass {
        let Some(sig) = self.sig(item) else {
            return Pass::Owned;
        };
        if sig.suspends || self.arity(item, sig) != n {
            return Pass::Owned;
        }
        self.value_parameter(self.param_ty(sig, k))
    }

    fn suspending(&self, item: DefId) -> bool {
        ProgramEnv::suspends(self, item)
    }

    fn entry_root(&self, item: DefId) -> bool {
        self.run
            .entry_roots
            .get()
            .is_some_and(|roots| roots.contains(&item))
    }

    fn local_ref_param(&self, item: DefId, k: usize, n: usize) -> bool {
        let Some(sig) = self.sig(item).filter(|s| self.arity(item, s) == n) else {
            return false;
        };
        let pool = &self.run.pool;
        let t = self.param_ty(sig, k);
        matches!(pool.get(t), TyData::Ref { counted: false, .. })
    }

    fn in_place_struct(&self, ty: Ty) -> bool {
        let pool = &self.run.pool;
        let t = ty;
        matches!(pool.get(t), TyData::Adt { def, .. }
            if self.std_kind(def) == hd_mono::layout::StdKind::Other && self.is_struct(def))
    }

    fn struct_fields(&self, ty: Ty, out: &mut Vec<Ty>) -> bool {
        out.clear();
        if !self.in_place_struct(ty) {
            return false;
        }
        let pool = &self.run.pool;
        let TyData::Adt { def, args } = pool.get(ty) else {
            return false;
        };
        let Some(fields) = self.data_fields(def) else {
            return false;
        };
        out.extend(
            fields
                .into_iter()
                .map(|f| hd_mono::subst(pool, self, def, args, f)),
        );
        true
    }

    fn trait_value(&self, ty: Ty) -> bool {
        let pool = &self.run.pool;
        let t = ty;
        matches!(pool.get(t), TyData::TraitValue { .. })
    }

    fn parts(&self, ty: Ty, variant: Option<u32>, out: &mut Vec<Ty>) -> bool {
        if self.is_open(ty) {
            out.clear();
            return false;
        }
        // An enum's own runtime Drop hook still runs for an empty
        // variant; its fields alone do not describe its ownership.
        if variant.is_some()
            && let TyData::Adt { def, .. } =
                self.run
                    .pool
                    .get(hd_wasm::strip_type(&self.run.pool, self, ty))
            && self.has_drop(def)
        {
            out.clear();
            return false;
        }
        hd_wasm::in_place_parts(&self.run.pool, self, ty, variant, out)
    }

    fn summarizes(&self) -> bool {
        self.run.summaries.is_some()
    }

    fn holds_coll(&self, ty: Ty) -> bool {
        let pool = &self.run.pool;
        self.is_open(ty) || hd_mono::holds_coll(pool, self, hd_wasm::strip_type(pool, self, ty))
    }

    fn holds_ref(&self, ty: Ty) -> bool {
        holds_ref(&self.run.pool, self.run.known.ptr, ty, 0)
    }

    fn function_value(&self, ty: Ty) -> bool {
        matches!(self.run.pool.get(ty), TyData::Fn { .. })
    }

    fn lends(&self, from: Ty, to: Ty) -> bool {
        !self.is_open(from)
            && !self.is_open(to)
            && hd_wasm::lending_types(&self.run.pool, self, from, to)
    }

    fn counted_refs_only(&self, ty: Ty) -> bool {
        // A type of counted references: what `holds_ref` finds with each
        // counted `Ref` taken out holds none.
        let pool = &self.run.pool;
        let mut counted = false;
        let only = refs_kinds(pool, self.run.known.ptr, ty, 0, &mut counted);
        only && counted
    }

    fn copies(&self, ty: Ty) -> Tri {
        if let Some(&c) = lock(&self.run.copy_types).get(&ty) {
            return c;
        }
        let c = self.copy_parts(ty, &mut Vec::new());
        lock(&self.run.copy_types).insert(ty, c);
        c
    }
}

impl Env<'_> {
    /// [`OwnCx::copies`] of `t`, as the `Copy` row reads it
    /// (`ref.copy.structure`): `No` when a part held by value is a type
    /// declared `!Copy`, else `Maybe` when one is a type parameter declared
    /// `?Copy`. A reference, a projection and an opaque type copy; so does
    /// a nominal type `open` already walks (a recursive one).
    fn copy_parts(&self, t: Ty, open: &mut Vec<Ty>) -> Tri {
        let pool = &self.run.pool;
        let meet = |a: Tri, b: Tri| match (a, b) {
            (Tri::No, _) | (_, Tri::No) => Tri::No,
            (Tri::Maybe, _) | (_, Tri::Maybe) => Tri::Maybe,
            _ => Tri::Yes,
        };
        match pool.get(t) {
            TyData::Tuple { elems, rest } => {
                let mut c = rest.map_or(Tri::Yes, |r| self.copy_parts(r, open));
                for &e in pool.list_items(elems) {
                    c = meet(c, self.copy_parts(e, open));
                }
                c
            }
            TyData::Union(ms) => pool
                .list_items(ms)
                .iter()
                .fold(Tri::Yes, |c, &m| meet(c, self.copy_parts(m, open))),
            TyData::Param(p) => {
                if self.param_copies(p) {
                    Tri::Yes
                } else {
                    Tri::Maybe
                }
            }
            TyData::Adt { def, args } => {
                if open.contains(&t) || open.len() > 32 {
                    return Tri::Yes;
                }
                let Some(item) = self.p.items.get(def) else {
                    return Tri::Yes;
                };
                match item.copy_decl() {
                    hd_types::CopyDecl::Copies => Tri::Yes,
                    hd_types::CopyDecl::NotCopy => Tri::No,
                    hd_types::CopyDecl::Members(ms) => {
                        let argv = pool.list_items(args);
                        open.push(t);
                        let mut c = Tri::Yes;
                        for m in ms {
                            let m = pool.subst(m, &|p: hd_types::ParamRef| {
                                (p.owner == def)
                                    .then(|| argv.get(usize::from(p.index)).copied())
                                    .flatten()
                            });
                            c = meet(c, self.copy_parts(m, open));
                            if c == Tri::No {
                                break;
                            }
                        }
                        open.pop();
                        c
                    }
                }
            }
            _ => Tri::Yes,
        }
    }

    /// Whether type parameter `p` is `Copy`: it has a bound on a trait
    /// that implies it, as every parameter has unless declared `?Copy`
    /// (`ref.copy.generic.default`); a trait's `Self` is, unless the trait
    /// is declared `< ?Copy` (`ref.copy.trait.default`).
    fn param_copies(&self, p: hd_types::ParamRef) -> bool {
        let Some(item) = self.p.items.get(p.owner) else {
            return true;
        };
        let in_trait = matches!(item.data, hd_resolve::ItemData::Trait(_));
        if let hd_resolve::ItemData::Trait(t) = &item.data
            && p.index == 0
        {
            return !t.relaxed_copy;
        }
        let generics = item.sig().map_or(&item.generics[..], |s| &s.generics[..]);
        let at = usize::from(p.index) - usize::from(in_trait);
        generics.get(at).is_none_or(|g| {
            g.bounds.iter().any(|&b| {
                self.run.pool.trait_value(b).is_some_and(|(d, _)| {
                    self.p
                        .items
                        .get(d)
                        .is_some_and(hd_resolve::Item::requires_copy)
                })
            })
        })
    }
}

/// Internal calls compose as read-only only when every generated factory
/// has no external write. Each helper's TIR is inspected once; no recursive
/// query or fixpoint is needed for this conservative group proof.
fn read_only_factories(
    pool: &hd_types::InternPool,
    target: Ty,
    bodies: &[Body],
    index: &HashMap<DefId, usize>,
    external_writes: &dyn Fn(&Callee) -> bool,
) -> bool {
    bodies
        .iter()
        .filter(|body| body.kind == hd_tir::ir::BodyKind::Handle)
        .all(|body| {
            let internal = |callee: &Callee| {
                matches!(*callee, Callee::Item { def, targs }
                if index.get(&def).is_some_and(|&i| bodies[i].kind == hd_tir::ir::BodyKind::Handle)
                    && pool.list_items(targs).first() == Some(&target))
            };
            let policy = |callee: &Callee| !internal(callee) && external_writes(callee);
            let structural = hd_tir::own::EffectIndex::new(body);
            let effects = hd_tir::own::WriteEffects::indexed(body, &structural);
            let writes = effects.in_range(0..body.len(), &policy);
            !writes
        })
}

#[test]
fn supplied_factory_effects_preserve_external_initializers_and_target_identity()
-> Result<(), String> {
    use hd_tir::ir::{BodyKind, Providers, TirBuilder, TirSink};
    let pool = hd_types::InternPool::new();
    let target = Ty::I32;
    let helper = |def: u32, callee: Option<(u32, Ty)>| {
        let mut b = TirBuilder::new(DefId::from_raw(def), BodyKind::Handle);
        let root = b.open_block();
        if let Some((called, target)) = callee {
            b.call(
                &Callee::Item {
                    def: DefId::from_raw(called),
                    targs: pool.list(&[target]),
                },
                &[],
                Providers::None,
                Ty::VOID,
                hd_base::NodeIdx::NONE,
            );
        }
        let root = b.close_block(root, None, Ty::VOID, hd_base::NodeIdx::NONE);
        b.finish(root, &[]).map_err(|errors| format!("{errors:?}"))
    };
    let index = [(DefId::from_raw(1), 0), (DefId::from_raw(2), 1)]
        .into_iter()
        .collect();
    let static_family = [helper(1, Some((2, target)))?, helper(2, None)?];
    assert!(read_only_factories(
        &pool,
        target,
        &static_family,
        &index,
        &|_| true
    ));
    let effectful = [helper(1, Some((2, target)))?, helper(2, Some((3, target)))?];
    assert!(!read_only_factories(
        &pool,
        target,
        &effectful,
        &index,
        &|_| true
    ));
    assert!(read_only_factories(
        &pool,
        target,
        &effectful,
        &index,
        &|_| false
    ));
    let other_target = [helper(1, Some((2, Ty::BOOL)))?, helper(2, None)?];
    assert!(!read_only_factories(
        &pool,
        target,
        &other_target,
        &index,
        &|_| true
    ));
    Ok(())
}

/// Whether a value of type `t` may hold a reference: a reference type,
/// a trait value, a `Ptr[T]` (`ptr`),
/// whose address `as_ref` makes a reference of, so a `Ptr` read from what
/// a reference reaches lives on what it reaches, or a type whose
/// arguments or parts are one. A type parameter's value may hold a local
/// reference (`ref.generic.local`), but never one into a local of the
/// body that holds it: no expression makes a `T` from that body's storage
/// (`ref.generic.body`), so for frame locals it holds none.
/// Whether every reference `t` may hold is a counted `Ref`, setting
/// `counted` when it holds one: no local `Ref`, trait value, `Ptr`, cell
/// or open type.
fn refs_kinds(
    pool: &hd_types::InternPool,
    ptr: DefId,
    t: Ty,
    depth: u32,
    counted: &mut bool,
) -> bool {
    if depth > 32 {
        return false;
    }
    let all = |l, counted: &mut bool| {
        pool.list_items(l)
            .iter()
            .all(|&a| refs_kinds(pool, ptr, a, depth + 1, counted))
    };
    match pool.get(t) {
        TyData::Ref { counted: true, .. } => {
            *counted = true;
            true
        }
        TyData::Ref { .. } | TyData::TraitValue { .. } | TyData::Fn { .. } => false,
        TyData::Adt { def, args } => def != ptr && all(args, counted),
        TyData::Tuple { elems, rest } => {
            all(elems, counted) && rest.is_none_or(|r| refs_kinds(pool, ptr, r, depth + 1, counted))
        }
        TyData::Union(members) => all(members, counted),
        TyData::Param(..) => false,
        _ => true,
    }
}

#[test]
fn a_function_beside_a_counted_reference_may_hold_a_local_reference() {
    let pool = hd_types::InternPool::new();
    let ptr = DefId::from_raw(1);
    let boxed = pool.intern_ty(&TyData::Ref {
        inner: Ty::I32,
        counted: true,
        mutable: false,
    });
    let callback = pool.intern_ty(&TyData::Fn {
        params: TyList::EMPTY,
        result: Ty::I32,
        row: hd_types::RowId::EMPTY,
        suspends: false,
        inputs: hd_types::Inputs::Fixed,
        deps: hd_types::FnDeps::NONE,
    });
    let pair = |right| {
        pool.intern_ty(&TyData::Tuple {
            elems: pool.list(&[boxed, right]),
            rest: None,
        })
    };
    assert!(refs_kinds(&pool, ptr, pair(Ty::STRING), 0, &mut false));
    assert!(!refs_kinds(&pool, ptr, pair(callback), 0, &mut false));
}

fn holds_ref(pool: &hd_types::InternPool, ptr: DefId, t: Ty, depth: u32) -> bool {
    if depth > 32 {
        return true;
    }
    let any = |l| {
        pool.list_items(l)
            .iter()
            .any(|&a| holds_ref(pool, ptr, a, depth + 1))
    };
    match pool.get(t) {
        // A function value may hold the references its closure captured.
        TyData::Ref { .. } | TyData::TraitValue { .. } | TyData::Fn { .. } => true,
        TyData::Adt { def, args } => def == ptr || any(args),
        TyData::Tuple { elems, rest } => {
            any(elems) || rest.is_some_and(|r| holds_ref(pool, ptr, r, depth + 1))
        }
        TyData::Union(members) => any(members),
        _ => false,
    }
}

impl Env<'_> {
    /// How many value parameters a body of `def` has: its signature's, and
    /// the hidden call-site record of a `@track_caller` function
    /// (`fn.caller.hidden`), which each call passes after the written
    /// arguments. Cost: one item lookup.
    fn arity(&self, def: DefId, sig: &hd_resolve::FnSig) -> usize {
        let hidden = self
            .p
            .items
            .get(def)
            .and_then(hd_resolve::Item::fn_attrs)
            .is_some_and(|a| a.track_caller);
        sig.params.len() + usize::from(hidden)
    }

    /// The declared type of value parameter `k` of a body with signature
    /// `sig`, as the backend reads it: past the written ones, the hidden
    /// call-site record, a `string`.
    fn param_ty(&self, sig: &hd_resolve::FnSig, k: usize) -> Ty {
        sig.params.get(k).map_or(Ty::STRING, |&(_, t)| self.low(t))
    }

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
        // Cold calls transfer Drop values and parameters with the
        // selected owned convention. An open parameter may become a
        // Drop value. A call of an
        // `@intrinsic` function, which the backend lowers in place and
        // which counts whatever it keeps itself (`Em::intrinsic`), unless
        // it takes over its argument ([`OWNING_INTRINSICS`]).
        if self.arity(def, sig) != n {
            return Pass::Lent;
        }
        if sig.suspends {
            let ty = self.param_ty(sig, k);
            if !self.cold_moves(ty) && !self.cold_transfers_declared(def, ty) {
                return Pass::Lent;
            }
        }
        if let Some(key) = self.intrinsic(def)
            && !OWNING_INTRINSICS.contains(&key.as_str())
        {
            return Pass::Lent;
        }
        // The declared type decides, unsubstituted: substituting would read
        // the callee's generics, which a default body's owner keeps in its
        // body, and the passes read no other body.
        self.value_parameter(self.param_ty(sig, k))
    }

    /// What a reference type `t` refers to, or `t` itself.
    fn ref_target_or(&self, t: Ty) -> Ty {
        match self.run.pool.get(t) {
            TyData::Ref { inner, .. } => inner,
            _ => t,
        }
    }

    /// `callee` with a bound's call at a closed receiver chosen as the
    /// backend's collection chooses it: a trait value dispatches through
    /// its table, and any other receiver runs its impl. A derive's instance
    /// and a generic instance at a trait value keep such calls.
    fn chosen(&self, callee: &Callee) -> Callee {
        let mut out = callee.clone();
        if let Callee::TraitMethod {
            trait_,
            self_ty,
            targs,
            choice: choice @ (ChoiceKind::Bound, _),
            ..
        } = &mut out
        {
            if hd_mono::is_trait_value(&self.run.pool, self.ref_target_or(*self_ty)) {
                *choice = (ChoiceKind::TraitValue, 0);
            } else if let Some(d) = InlineEnv::select_impl(self, *trait_, *self_ty, *targs) {
                *choice = (ChoiceKind::Impl, d.raw());
            }
        }
        // A derived newtype impl forwards to its base's, which runs.
        while let Callee::TraitMethod {
            trait_,
            self_ty,
            targs,
            choice: choice @ (ChoiceKind::Impl, _),
            ..
        } = &mut out
            && self.is_derived_newtype_impl(DefId::from_raw(choice.1))
        {
            let base = hd_mono::layout::strip_newtype(&self.run.pool, self, *self_ty);
            let Some(d) = (base != *self_ty)
                .then(|| InlineEnv::select_impl(self, *trait_, base, *targs))
                .flatten()
            else {
                break;
            };
            *self_ty = base;
            *choice = (ChoiceKind::Impl, d.raw());
        }
        out
    }

    /// [`OwnCx::pass`] of parameter `k` of instance `(def, targs)`: its
    /// declared type substituted as the backend substitutes a callee's
    /// parameters (`hd_mono::subst`, which maps a default body's types to
    /// its owner's generics). `None` while it stays open (the caller's own
    /// generics, in a generic body that serves several instances), for a
    /// suspending callee, whose frame keeps its arguments, and for an
    /// intrinsic.
    fn instance_pass(&self, def: DefId, targs: TyList, k: usize, n: usize) -> Option<Pass> {
        if self.intrinsic(def).is_some() || ProgramEnv::suspends(self, def) {
            return None;
        }
        let sig = self.sig(def)?;
        if sig.suspends || self.arity(def, sig) != n {
            return None;
        }
        let t = hd_mono::subst(&self.run.pool, self, def, targs, self.param_ty(sig, k));
        (t != Ty::POISON && !self.is_open(t)).then(|| self.value_parameter(t))
    }

    /// Whether a call through a trait value lends argument `k > 0` to the
    /// slot of `method`: see [`OwnCx::pass`].
    fn slot_lends(&self, method: DefId, self_ty: Ty, targs: TyList, k: usize) -> bool {
        let pool = &self.run.pool;
        let self_value = self.ref_target_or(self_ty);
        let items = pool.list_items(targs);
        self.sig(method).is_some_and(|sig| {
            let all = pool.list(&[&[self_value], items].concat());
            let t = hd_mono::subst(pool, self, method, all, self.low(sig.params[k].1));
            t != Ty::POISON && !self.is_open(t) && !hd_wasm::holds_drop_type(pool, self, t)
        })
    }

    /// How a parameter of declared type `t` takes its argument: owned when
    /// it is a value, or may be one in an instance (a type parameter, an
    /// associated type, a newtype over a type parameter); lent when it is
    /// a local reference or holds nothing counted. Counted `*` references
    /// are owned values too. Instance substitution preserves borrowing for
    /// local `&` references and counting nothing for uncounted values.
    pub(super) fn value_parameter(&self, t: Ty) -> Pass {
        self.parameter(t, true)
    }

    /// Function values lend Copy inputs and the existing reference-variance
    /// inputs (`hd_wasm::function_input_borrows`). Adapters and closure entry
    /// acquire any count their concrete body owns. Other inputs keep their
    /// ownership transfer, including noncopy resources.
    pub(super) fn function_parameter(&self, t: Ty) -> Pass {
        match self.parameter(t, false) {
            Pass::Owned
                if self.copies(t) == Tri::Yes
                    || !self.is_open(t)
                        && hd_wasm::function_input_borrows(&self.run.pool, self, t) =>
            {
                Pass::Lent
            }
            p => p,
        }
    }

    fn parameter(&self, t: Ty, counted_owned: bool) -> Pass {
        let pool = &self.run.pool;
        match pool.get(t) {
            TyData::Ref { counted: false, .. } => Pass::Lent,
            TyData::Ref { counted: true, .. } => {
                if counted_owned {
                    Pass::Owned
                } else {
                    Pass::Lent
                }
            }
            _ if self.is_open(t) => Pass::Owned,
            _ => match self.layout_facts(t) {
                (true, None) => Pass::Owned,
                (true, Some(true)) if counted_owned => Pass::Owned,
                _ => Pass::Lent,
            },
        }
    }

    /// The selected parameter representation decides the cold entry mode.
    /// Local references retain their borrowed convention. Counted values
    /// arrive owned at the constructor and at a fused fresh entry.
    pub(super) fn cold_transfers_declared(&self, def: DefId, ty: Ty) -> bool {
        if !ProgramEnv::suspends(self, def) || self.intrinsic(def).is_some() {
            return false;
        }
        self.value_parameter(ty) == Pass::Owned
            && !hd_wasm::holds_drop_type(&self.run.pool, self, ty)
    }

    /// The same selected signature used by direct emission and a trait slot.
    /// A dynamic receiver lends its erased payload through the slot bridge.
    fn cold_parameter(&self, callee: &Callee, k: usize, n: usize) -> Option<(DefId, Ty)> {
        let declared = match callee {
            Callee::Item { def, .. } => *def,
            Callee::TraitMethod { method, .. } => *method,
        };
        if !self.sig(declared)?.suspends {
            return None;
        }
        let key = callee.words();
        let cached = lock(&self.run.own_cold).get(&key).copied();
        let signature = if let Some(signature) = cached {
            signature
        } else {
            let signature = (|| {
                let chosen = self.chosen(callee);
                let (def, args, dynamic) = match chosen {
                    Callee::Item { def, targs } => (def, targs, false),
                    Callee::TraitMethod {
                        method,
                        self_ty,
                        targs,
                        choice: (ChoiceKind::TraitValue, _),
                        ..
                    } => {
                        let args = self
                            .run
                            .pool
                            .list(&[&[self_ty], self.run.pool.list_items(targs)].concat());
                        (method, args, true)
                    }
                    Callee::TraitMethod { .. } => {
                        let (def, args) = self.impl_target(&chosen)?;
                        (def, args, false)
                    }
                };
                let sig = self.sig(def).filter(|sig| sig.suspends)?;
                let mut substitutions = HashMap::default();
                let params: Vec<_> = sig
                    .params
                    .iter()
                    .map(|p| {
                        let ty = self.low(p.1);
                        *substitutions
                            .entry(ty)
                            .or_insert_with(|| hd_mono::subst(&self.run.pool, self, def, args, ty))
                    })
                    .collect();
                Some(ColdSignature {
                    item: def,
                    params: self.run.pool.list(&params),
                    dynamic_receiver: dynamic,
                })
            })();
            lock(&self.run.own_cold).insert(key, signature);
            signature
        };
        let signature = signature?;
        let params = self.run.pool.list_items(signature.params);
        if params.len() != n || signature.dynamic_receiver && k == 0 {
            return None;
        }
        params.get(k).copied().map(|ty| (signature.item, ty))
    }

    /// The borrowed cold convention still transfers inline Drop
    /// values. An open value parameter may become one at instantiation.
    /// The walk follows the types `t` holds by value, as the backend lays
    /// them out (`hd_wasm::value_part_types`): tuple and union
    /// members, struct fields, and every value enum's payloads, `Option`'s
    /// as any other's. A self-recursive enum's cell, a list or a map is a
    /// counted object the frame retains by its count.
    ///
    /// Cost: one visit per distinct type held by value, with a worklist.
    fn cold_moves(&self, t: Ty) -> bool {
        let pool = &self.run.pool;
        let mut seen = hd_base::HashSet::default();
        let mut work = vec![t];
        while let Some(t) = work.pop() {
            let t = hd_wasm::strip_type(pool, self, t);
            if !seen.insert(t) {
                continue;
            }
            if hd_wasm::holds_drop_type(pool, self, t) {
                return true;
            }
            match pool.get(t) {
                // Copy-bound arguments may be lent to the constructor.
                // Only a ?Copy parameter needs a possible ownership transfer.
                TyData::Param(p) if !self.param_copies(p) => return true,
                TyData::Assoc { .. } | TyData::Opaque { .. } => return true,
                _ => work.extend(hd_wasm::value_part_types(pool, self, t)),
            }
        }
        false
    }

    /// Whether a value of type `t`, which names no parameter, holds a
    /// counted leaf, and its reference ownership (None for a value, false
    /// for a local loan, true for a counted reference): one normalized kind
    /// query per distinct type, with constant-time warm lookup.
    fn layout_facts(&self, t: Ty) -> (bool, Option<bool>) {
        if let Some(&facts) = lock(&self.run.own_types).get(&t) {
            return facts;
        }
        let pool = &self.run.pool;
        let facts = (
            hd_wasm::counted_type(pool, self, t),
            hd_wasm::reference_ownership(pool, self, t),
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
    ) -> StageResult<(Body, Option<Summary>, u64)> {
        // A body's passes never run another's; the tables are taken out
        // anyway, so a nested run would only start from empty ones.
        let mut scratch = SCRATCH.with(RefCell::take);
        // Each instruction names itself until the passes renumber the
        // body, the inliner's copies their call: the line table's anchors
        // stay the `check` entry's.
        if body.anchors.is_empty() {
            body.anchors = (0..u32::try_from(body.len()).expect("insts")).collect();
        }
        self.root_across_pure_calls(&mut body);
        let returns = summarize
            .then(|| hd_tir::own::returned_param(&body))
            .flatten();
        // Each type's layout once per body, not once per query of it.
        let (refused, summary) = hd_wasm::with_layout_cache(|| {
            let refused = hd_tir::own::run(&mut body, self, &mut scratch);
            let summary = (summarize && refused.is_ok())
                .then(|| hd_tir::own::summary(&body, self, &scratch, returns))
                .flatten();
            (refused, summary)
        });
        let relies = scratch.relies();
        let uncopied = scratch.uncopied().to_vec();
        SCRATCH.with(|s| *s.borrow_mut() = scratch);
        let path = || self.run.names().path(body.item);
        if let Err(what) = refused {
            return Err(NotImplemented::bug(
                Stage::Collect,
                format!("the ownership passes refused `{}`: {what}", path()),
            ));
        }
        if !uncopied.is_empty() {
            self.uncopied(&body, &uncopied)?;
        }
        if self.run.verifies_own() {
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
        Ok((body, summary, relies))
    }

    /// The copy verifier's findings in `body` (#1295): the build fails on
    /// the first with an internal error.
    fn uncopied(&self, body: &Body, sites: &[Uncopied]) -> StageResult<()> {
        let names = self.run.names();
        let path = names.path(body.item);
        let line = |s: &Uncopied| {
            let what = s.capture.map_or_else(
                || {
                    let place = body.decode_place(body.data[s.inst as usize][0]);
                    let place = place.map(|p| p.to_string()).unwrap_or_default();
                    format!("Read copy {place}")
                },
                |k| format!("capture copy ${}", body.cap_local[k as usize].raw()),
            );
            let open = if s.copies == Tri::Maybe {
                " (a `?Copy` parameter)"
            } else {
                ""
            };
            format!(
                "`{path}` %{}: {what} : {}{open}",
                s.inst,
                hd_resolve::show_ty(&names, s.ty)
            )
        };
        Err(NotImplemented::bug(
            Stage::Collect,
            format!(
                "internal: copy of a non-copyable value in {} ({} findings)",
                line(&sites[0]),
                sites.len()
            ),
        ))
    }

    /// Roots `body`'s places again, across the calls that write nothing the
    /// body can reach (`hd_tir::ir::root_places`): the checker roots them
    /// across no call, so `self.items[self.pick()]` reads `self.items`
    /// before `pick` and the list's read must copy it (#1153). Only a body
    /// with such a call can fold more.
    fn root_across_pure_calls(&self, body: &mut Body) {
        let pure = |c: &Callee| !self.may_write(c);
        let any = (0..body.len()).any(|i| {
            body.tags[i] == Tag::Call
                && Callee::from_words(body.record(body.data[i][0])).is_some_and(|c| pure(&c))
        });
        if any {
            Self::root(body, Some(&pure));
        }
    }

    /// Roots `body`'s places at their locals (`hd_tir::ir::root_places`).
    fn root(body: &mut Body, pure: Option<&dyn Fn(&Callee) -> bool>) {
        ROOTS.with(|r| {
            hd_tir::ir::root_places(body, pure, &mut r.borrow_mut());
        });
    }

    /// Generate a traversal and its handle family once per concrete target.
    /// Both specialization and collection reuse this query. The family index
    /// makes direct factory recovery independent of the eliminated call.
    pub(super) fn raw_supplied_family(
        &self,
        method: DefId,
        self_ty: Ty,
    ) -> StageResult<Option<std::sync::Arc<super::SuppliedFamily>>> {
        if let Some(cell) = lock(&self.p.supplied_families)
            .get(&(method, self_ty))
            .cloned()
            && let Some(result) = cell.get()
        {
            return result.clone();
        }
        let names = self.run.names();
        let cx = hd_structure::Cx {
            names: &names,
            items: self.p,
            stage: Stage::Collect,
        };
        let Some(root) = hd_structure::supplied_root(&cx, method) else {
            return Ok(None);
        };
        let cell = lock(&self.p.supplied_families)
            .entry((root, self_ty))
            .or_default()
            .clone();
        if method != root {
            lock(&self.p.supplied_families)
                .entry((method, self_ty))
                .or_insert_with(|| cell.clone());
        }
        cell.get_or_init(|| {
            let Some(target) = hd_structure::dbg_target(&cx, self_ty) else {
                return Ok(None);
            };
            let Some(generated) = hd_structure::supplied(&cx, &target, root)? else {
                return Ok(None);
            };
            {
                let mut rets = lock(&self.p.supplied_rets);
                for item in &generated.helpers.items {
                    if let Some(sig) = item.sig() {
                        let ret = self.low(sig.ret);
                        if let Some(old) = rets.insert(item.def, ret) {
                            // ruled out: supplied factory signatures are templates
                            // over their explicit target and member arguments.
                            debug_assert_eq!(
                                old, ret,
                                "a supplied factory's return template is stable"
                            );
                        }
                    }
                }
            }
            // Hash the traversal once. Serializing the full target type in
            // every individual factory would multiply work for wide tuples.
            let bytes = hd_tir::wire::write_body(
                &generated.body,
                &self.run.pool,
                &self.run.paths,
                &self.run.syms,
            )?;
            let hash = hd_tir::wire::tir_hash(&bytes);
            let inlineable = hd_opt::inline::bounded_inline_metadata(&generated.body).is_some();
            let mut bodies = Vec::with_capacity(1 + generated.helpers.bodies.len());
            let mut index = HashMap::default();
            for body in std::iter::once(generated.body).chain(generated.helpers.bodies) {
                index.insert(body.item, bodies.len());
                bodies.push(body);
            }
            // Prove factories from their actual bodies, including user
            // metadata initializers. A generated name is never a purity fact.
            let factories_read_only =
                read_only_factories(&self.run.pool, self_ty, &bodies, &index, &|callee| {
                    self.intrinsic_may_write(callee)
                });
            Ok(Some(std::sync::Arc::new(super::SuppliedFamily {
                bodies,
                index,
                hash,
                inlineable,
                factories_read_only,
            })))
        })
        .clone()
    }

    /// A body collection generates (a supplied method's, a `dbg` body),
    /// after the ownership passes. Its places are rooted at their locals
    /// first, as the checker roots its bodies' (`hd_tir::ir::root_places`).
    pub(super) fn owned_generated(&self, mut body: Body) -> StageResult<Body> {
        Self::root(&mut body, None);
        Ok(self.own_passes(body, false)?.0)
    }
}

/// The inliner's view of the program (`hd_opt::inline`): the driver's
/// tables and decoded bodies, and the backend's layout facts.
impl InlineEnv for Env<'_> {
    fn pool(&self) -> &hd_types::InternPool {
        &self.run.pool
    }

    fn prepare_copy(&self, body: &mut Body, hash: Hash128) -> Hash128 {
        let structure = hd_opt::structure_specialize::specialize_calls(self, body, hash);
        hd_opt::fact_specialize::specialize_calls(self, body, structure)
    }

    fn quiet(&self, body: &Body, i: usize) -> bool {
        match body.tags[i] {
            // A generated handle or facts factory builds its static record
            // once and only returns it.
            hd_tir::Tag::Call => matches!(
                hd_tir::ir::Callee::from_words(body.record(body.data[i][0])),
                Some(hd_tir::ir::Callee::Item { def, .. })
                    if matches!(
                        hd_structure::generated_body(&self.run.names(), def),
                        Some(hd_structure::GeneratedBody::Handle(_) | hd_structure::GeneratedBody::Facts)
                    )
            ),
            hd_tir::Tag::Drop => body.local_in_place(body.data[i][0]).is_some_and(|l| {
                !hd_wasm::holds_drop_type(&self.run.pool, self, body.local_ty[l.idx()])
            }),
            _ => false,
        }
    }

    fn call_may_write(&self, callee: &Callee) -> bool {
        self.declared_may_write(callee)
    }

    fn memo(&self) -> Option<&hd_opt::inline::Memo> {
        self.run.summaries.as_ref().map(|all| &all.inline)
    }

    fn const_fold(&self) -> bool {
        self.run.const_fold
    }

    fn module_text(&self, def: DefId) -> std::borrow::Cow<'_, str> {
        self.run.item_module_text(def)
    }

    fn inline_hint(&self, def: DefId) -> InlineHint {
        self.p
            .items
            .get(def)
            .map_or(InlineHint::Default, |i| match i.inline() {
                hd_resolve::iface::INLINE_AUTO => InlineHint::Auto,
                hd_resolve::iface::INLINE_ALWAYS => InlineHint::Always,
                hd_resolve::iface::INLINE_NEVER | hd_resolve::iface::COLD => InlineHint::Never,
                _ => InlineHint::Default,
            })
    }

    fn signature(&self, def: DefId) -> Option<(Vec<Ty>, bool, bool)> {
        if let Some(sig) = self.sig(def) {
            return Some((
                sig.params.iter().map(|&(_, t)| t).collect(),
                sig.suspends,
                sig.row != hd_types::RowId::EMPTY,
            ));
        }
        // Defaults have checked parameter locals, but no item signature.
        // Read their declaration without starting ownership recursively.
        let (body, _) = self.checked_body(def)?;
        (body.kind == hd_tir::BodyKind::Default).then(|| {
            let params = hd_tir::own::params(body)
                .iter()
                .map(|&l| body.local_ty[l as usize])
                .collect();
            (params, false, false)
        })
    }

    fn result(&self, def: DefId) -> Option<Ty> {
        if let Some(sig) = self.sig(def) {
            return Some(sig.ret);
        }
        let (body, _) = self.checked_body(def)?;
        (body.kind == hd_tir::BodyKind::Default)
            .then(|| body.sub_root.first().map(|&root| body.ty[root as usize]))
            .flatten()
    }

    fn checked_body(&self, def: DefId) -> Option<&(Body, Hash128)> {
        self.run.decode(self.run.lazy_body(self.p, def)?)
    }

    fn fresh_body(&self, def: DefId) -> Option<Body> {
        let run = self.run;
        let (m, entry, at) = run.lazy_body(self.p, def)?.wire.as_ref()?;
        let mut fresh =
            hd_tir::wire::read_body(entry.get(at.clone())?, &run.pool, &run.paths, &run.syms)?;
        run.checked_room_calls(&mut fresh, Hash128::default());
        // A fetched package's call-site records keep the cleared
        // root-package bit `decode` gave them (`fn.caller.root-package`), so
        // its `dbg` calls print nothing (`module.dbg.dependency`).
        if run.fetched[usize::from(run.table.modules[*m].package)] {
            crate::dbg::foreign_sites(&mut fresh, |d| {
                self.p
                    .items
                    .get(d)
                    .and_then(hd_resolve::Item::fn_attrs)
                    .is_some_and(|a| a.track_caller)
            });
        }
        Some(fresh)
    }

    fn reference(&self, t: Ty) -> bool {
        self.layout_facts(t).1.is_some()
    }

    fn lent_referent(&self, t: Ty) -> Option<Ty> {
        // A layout that fits in locals is not necessarily Copy. A promoted
        // argument reads that value out of the caller's place, and another
        // use of the place can make the ownership passes keep that read as
        // a copy. Keep such a reader's reference instead.
        hd_wasm::lent_referent(&self.run.pool, self, t)
            .filter(|&referent| self.copies(referent) == Tri::Yes)
    }

    /// A value no copy may take: one that holds a `std.rt.Drop`, or a type
    /// that is not `Copy`, whose by-value binding the passes could only
    /// keep by borrowing, which a later copy in the same body may refuse.
    fn rooted_referent(&self, t: Ty) -> Option<Ty> {
        hd_wasm::rooted_referent(&self.run.pool, self, t).or_else(|| {
            hd_wasm::lent_referent(&self.run.pool, self, t).filter(|&r| self.copies(r) == Tri::No)
        })
    }

    fn counted_referent(&self, t: Ty) -> bool {
        hd_wasm::lent_referent(&self.run.pool, self, t).is_some()
            && hd_wasm::promotable_referent(&self.run.pool, self, t).is_none()
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
            TyData::Ref { inner, .. } => inner,
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
        let tref = TraitRef {
            trait_,
            self_ty,
            args: types.list(args),
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
    fn supplied_structure_walk(&self, callee: &Callee) -> Option<(Body, Hash128, TyList)> {
        let (def, args) = self.supplied_walk_node(callee)?;
        let family = self.supplied_walk_family((def, args))?;
        if !family.inlineable {
            return None;
        }
        let body = family.bodies.get(*family.index.get(&def)?)?;
        Some((body.clone(), family.hash, args))
    }

    fn generated_structure_walk(&self, def: DefId) -> bool {
        hd_structure::is_generated_walk(&self.run.names(), def)
    }

    fn generated_structure_build(&self, def: DefId) -> bool {
        hd_structure::generated_body(&self.run.names(), def)
            == Some(hd_structure::GeneratedBody::Build)
    }

    fn generated_structure_describe(&self, def: DefId) -> bool {
        hd_structure::generated_body(&self.run.names(), def)
            == Some(hd_structure::GeneratedBody::Describe)
    }

    fn describer_trait(&self) -> DefId {
        self.run.known.describer
    }

    fn walker_trait(&self) -> DefId {
        self.run.known.walker
    }

    fn source_trait(&self) -> DefId {
        self.run.known.source
    }

    fn walker_data_variant(&self) -> DefId {
        self.run.known.walker_data_variant
    }

    fn walker_data_member(&self) -> DefId {
        self.run.known.walker_data_member
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

impl FactEnv for Env<'_> {
    fn fact_lookup_key(
        &self,
        def: DefId,
        args: TyList,
    ) -> Option<hd_opt::fact_specialize::FactLookup> {
        let find = self.run.known.facts_find;
        if def == find {
            let key = *self.run.pool.list_items(args).first()?;
            if key == Ty::POISON
                || self.run.pool.has_param(key)
                || self.run.pool.has_assoc(key)
                || self.run.pool.has_opaque(key)
            {
                return None;
            }
            return Some(hd_opt::fact_specialize::FactLookup {
                key,
                projection: Vec::new(),
            });
        }
        let body = &self.checked_body(def)?.0;
        if body.tags.iter().any(|tag| {
            !matches!(
                tag,
                hd_tir::Tag::Block | hd_tir::Tag::Read | hd_tir::Tag::Call
            )
        }) {
            return None;
        }
        let mut calls = body
            .tags
            .iter()
            .enumerate()
            .filter_map(|(i, tag)| (*tag == hd_tir::Tag::Call).then_some(i));
        let call = calls.next()?;
        if calls.next().is_some() {
            return None;
        }
        let root = *body.sub_root.first()? as usize;
        if body.tags.get(root) != Some(&hd_tir::Tag::Block)
            || body.data[root][1] != u32::try_from(call).ok()?
        {
            return None;
        }
        let Callee::Item { def: callee, targs } = hd_tir::inline::plain_call(body, call)? else {
            return None;
        };
        if callee != find {
            return None;
        }
        let record = body.record(body.data[call][1]);
        let call_args = &record[..record.len().saturating_sub(3)];
        let receiver = *call_args.first()? as usize;
        if body.tags.get(receiver) != Some(&hd_tir::Tag::Read) {
            return None;
        }
        let receiver = body.decode_place(body.data[receiver][0])?;
        let hd_tir::ir::PlaceRoot::Local(receiver_local) = receiver.root else {
            return None;
        };
        if hd_tir::own::params(body).as_slice() != [receiver_local.raw()] {
            return None;
        }
        let key = *self.run.pool.list_items(targs).first()?;
        let key = hd_mono::subst(&self.run.pool, self, def, args, key);
        if key == Ty::POISON
            || self.run.pool.has_param(key)
            || self.run.pool.has_assoc(key)
            || self.run.pool.has_opaque(key)
        {
            return None;
        }
        Some(hd_opt::fact_specialize::FactLookup {
            key,
            projection: receiver.proj,
        })
    }

    fn list_is_empty(&self, def: DefId) -> bool {
        def == self.run.known.list_is_empty
    }

    fn is_facts(&self, ty: Ty) -> bool {
        matches!(self.run.pool.get(ty), TyData::Adt { def, .. } if def == self.run.known.facts)
    }

    fn known_facts(
        &self,
        def: DefId,
        args: TyList,
        projected: &[hd_tir::ir::Proj],
    ) -> Option<hd_opt::fact_specialize::KnownFacts> {
        if !matches!(projected, [] | [hd_tir::ir::Proj::Field(0)]) {
            return None;
        }
        self.literal_facts(def, args, 0)
    }

    fn fact_handle_body(&self, def: DefId, args: TyList) -> Option<(Body, Hash128)> {
        if !matches!(
            hd_structure::generated_body(&self.run.names(), def),
            Some(hd_structure::GeneratedBody::Handle(_))
        ) {
            return None;
        }
        let (raw, hash) = self.checked_body(def)?;
        let pool = &self.run.pool;
        let mut body = raw.clone();
        let subst = |ty| hd_mono::subst(pool, self, def, args, ty);
        let list = |list| {
            pool.list(
                &pool
                    .list_items(list)
                    .iter()
                    .map(|&ty| subst(ty))
                    .collect::<Vec<_>>(),
            )
        };
        hd_tir::ir::map_body_types(&mut body, &subst, &list);
        Some((body, *hash))
    }

    fn fact_type_hash(&self, ty: Ty) -> Hash128 {
        let names = self.run.names();
        hd_mono::layout::canon(
            &self.run.pool,
            &|def| names.path_hash(def),
            &mut hd_mono::layout::CanonMemo::default(),
            ty,
        )
    }

    fn fact_ref_inner(&self, ty: Ty) -> Option<Ty> {
        match self.run.pool.get(ty) {
            TyData::Ref { inner, .. } => Some(inner),
            _ => None,
        }
    }
}

impl Env<'_> {
    fn literal_facts(
        &self,
        def: DefId,
        args: TyList,
        depth: u8,
    ) -> Option<hd_opt::fact_specialize::KnownFacts> {
        if depth >= 8 {
            return None;
        }
        let generated = hd_structure::generated_body(&self.run.names(), def)
            == Some(hd_structure::GeneratedBody::Facts);
        let (body, body_hash) = self.checked_body(def)?;
        if !generated && body.kind != hd_tir::BodyKind::Fact {
            return None;
        }
        let root = *body.sub_root.first()? as usize;
        if body.tags.get(root) != Some(&hd_tir::Tag::Block) {
            return None;
        }
        let value = body.data[root][1] as usize;
        if body.tags.get(value) == Some(&hd_tir::Tag::DefaultCall) {
            let record = body.record(body.data[value][0]);
            let (&target, &target_args) = (record.first()?, record.get(1)?);
            let target = DefId::from_raw(target);
            let pool = &self.run.pool;
            let concrete: Vec<Ty> = pool
                .list_items(TyList(target_args))
                .iter()
                .map(|&ty| hd_mono::subst(pool, self, def, args, ty))
                .collect();
            let mut known = self.literal_facts(target, pool.list(&concrete), depth + 1)?;
            let mut digest = StableHasher::new("literal-generated-facts-v1");
            digest.hash(*body_hash);
            digest.hash(known.digest);
            known.digest = digest.finish();
            return Some(known);
        }
        if body.tags.get(value) != Some(&hd_tir::Tag::NewStruct) {
            return None;
        }
        let items = *body.record(body.data[value][1]).first()? as usize;
        if body.tags.get(items) != Some(&hd_tir::Tag::NewList)
            || body.data[items][0] != hd_tir::ir::NONE
        {
            return None;
        }
        let pool = &self.run.pool;
        let mut types = Vec::new();
        for &item in body.record(body.data[items][1]) {
            let item = item as usize;
            if body.tags.get(item) != Some(&hd_tir::Tag::RefDyn) {
                return None;
            }
            let address = body.data[item][0] as usize;
            if body.tags.get(address) != Some(&hd_tir::Tag::Block) {
                return None;
            }
            let coercion = body.data[address][1] as usize;
            if body.tags.get(coercion) != Some(&hd_tir::Tag::Coerce)
                || body.record(body.data[coercion][1]).first()
                    != Some(&(hd_tir::ir::Coercion::RefToTraitValue as u32))
            {
                return None;
            }
            let source = body.data[coercion][0] as usize;
            let inner =
                hd_opt::fact_specialize::FactEnv::fact_ref_inner(self, *body.ty.get(source)?)?;
            let concrete = hd_mono::subst(pool, self, def, args, inner);
            if concrete == Ty::POISON
                || pool.has_param(concrete)
                || pool.has_assoc(concrete)
                || pool.has_opaque(concrete)
            {
                return None;
            }
            types.push(concrete);
        }
        let mut digest = StableHasher::new("literal-facts-v1");
        digest.hash(*body_hash);
        let names = self.run.names();
        let mut canons = hd_mono::layout::CanonMemo::default();
        for &ty in &types {
            digest.hash(hd_mono::layout::canon(
                pool,
                &|def| names.path_hash(def),
                &mut canons,
                ty,
            ));
        }
        Some(hd_opt::fact_specialize::KnownFacts {
            types,
            digest: digest.finish(),
        })
    }

    /// The folds and inlining a body takes before the ownership passes,
    /// for an item's own body and for one instance's
    /// (`ProgramEnv::instance_body`) alike: generated structure walks,
    /// then, in release, declaration facts and the constants they decide,
    /// then the inliner, then, where generated structure or facts folded or the pipeline holds
    /// `const_fold`, the decisions the result's own values decide (an
    /// inlined trait method's constant among them, #1518). The result's
    /// hash: the fold is a function of the inlined body and the pipeline,
    /// whose hash every code key carries.
    pub(super) fn fold_and_inline(&self, body: &mut Body, hash: Hash128) -> Hash128 {
        let structure = hd_opt::structure_specialize::specialize_calls(self, body, hash);
        let facts = hd_opt::fact_specialize::specialize_calls(self, body, structure);
        // Generated structure accessors and declaration facts can both
        // expose constant decisions before the general folding pipeline.
        let decided = facts != hash;
        let hash = hd_opt::inline::inline_calls(self, body, facts);
        if (decided || self.run.const_fold)
            && let Some(specialized) = hd_mono::defaults::specialize(body, &[])
        {
            *body = specialized;
        }
        hash
    }

    /// Instance `(def, args)` as collection keys it ([`Node`]).
    pub(super) fn node(&self, def: DefId, args: TyList) -> Node {
        (def, hd_mono::rowless(&self.run.pool, self, def, args))
    }

    fn supplied_walk_node(&self, callee: &Callee) -> Option<Node> {
        let Callee::TraitMethod {
            trait_,
            method,
            self_ty,
            targs,
            choice: (ChoiceKind::Builtin | ChoiceKind::Bound, _),
        } = *callee
        else {
            return None;
        };
        if trait_ != self.run.known.structure || self.is_open(self_ty) {
            return None;
        }
        let names = self.run.names();
        if names.paths.segment(hd_base::PathId::from_raw(method.raw())) != "walk" {
            return None;
        }
        let types = self.run.pool.types();
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
        let tref = TraitRef {
            trait_,
            self_ty,
            args: TyList::EMPTY,
        };
        if !matches!(
            TableSolver.select(&mut cx, ConcreteTraitRef(tref)).ok()?,
            Selection::Builtin(_)
        ) {
            return None;
        }
        let args = self
            .run
            .pool
            .list(&[&[self_ty], self.run.pool.list_items(targs)].concat());
        Some(self.node(method, args))
    }

    fn supplied_walk_family(
        &self,
        (def, args): Node,
    ) -> Option<std::sync::Arc<super::SuppliedFamily>> {
        let names = self.run.names();
        let path = hd_base::PathId::from_raw(def.raw());
        if names.paths.parent(path).raw() != self.run.known.structure.raw()
            || names.paths.segment(path) != "walk"
        {
            return None;
        }
        let items = self.run.pool.list_items(args);
        let &target = items.first()?;
        self.supplied_walk_node(&Callee::TraitMethod {
            trait_: self.run.known.structure,
            method: def,
            self_ty: target,
            targs: self.run.pool.list(&items[1..]),
            choice: (ChoiceKind::Builtin, 0),
        })?;
        let family = self.raw_supplied_family(def, target).ok()??;
        (family.bodies.first()?.item == def).then_some(family)
    }

    /// The instance a call runs, where the call names one: a direct
    /// call's callee at its type arguments, or a trait call's impl method
    /// where the call is chosen as an impl.
    fn callee_node(&self, callee: &Callee) -> Option<Node> {
        let (def, args) = match *callee {
            Callee::Item { def, targs } => (def, targs),
            Callee::TraitMethod { .. } => {
                if let Some(node) = self.supplied_walk_node(callee) {
                    return Some(node);
                }
                self.impl_target(callee)?
            }
        };
        Some(self.node(def, args))
    }

    /// Generic item `def`'s own body folded and inlined
    /// ([`Env::inlined_instance`] at no type arguments), once.
    pub(super) fn generic_inlined(&self, def: DefId) -> Option<&(Body, Hash128)> {
        let lazy = self.run.lazy_body(self.p, def)?;
        let inlined = lazy
            .inlined
            .get_or_init(|| self.inlined_instance((def, TyList::EMPTY)).map(Box::new));
        inlined.as_deref()
    }

    /// Instance `node`'s body before the passes: an item's own body, or a
    /// generic item's at its type arguments, with its types substituted
    /// and its trait calls chosen as impl calls; then folded and inlined
    /// ([`Env::fold_and_inline`]). A non-generic item's own body is its
    /// one instance's. In dev, a generic instance substitutes its item's
    /// body after the folds and the inliner ([`Env::generic_inlined`]):
    /// they run once per generic body, so a trait call the instance
    /// chooses as an impl call stays a call. `None`, so the item's generic
    /// body serves, for type arguments past [`MAX_INSTANCE_LEAVES`].
    pub(super) fn inlined_instance(&self, (def, args): Node) -> Option<(Body, Hash128)> {
        if args != TyList::EMPTY && instance_leaves(&self.run.pool, args) > MAX_INSTANCE_LEAVES {
            return None;
        }
        let dev = self.run.summaries.is_none() && args != TyList::EMPTY;
        let supplied = self.supplied_walk_family((def, args));
        let (mut body, hash) = if let Some(family) = &supplied {
            (family.bodies[0].clone(), family.hash)
        } else {
            let (raw, hash) = if dev {
                self.generic_inlined(def)?
            } else {
                self.run
                    .lazy_body(self.p, def)
                    .and_then(|b| self.run.decode(b))?
            };
            (raw.clone(), *hash)
        };
        if args != TyList::EMPTY {
            let pool = &self.run.pool;
            let ty = |t: Ty| hd_mono::subst(pool, self, def, args, t);
            let list = |l| {
                pool.list(
                    &pool
                        .list_items(l)
                        .iter()
                        .map(|&t| ty(t))
                        .collect::<Vec<_>>(),
                )
            };
            hd_tir::ir::map_body_types(&mut body, &ty, &list);
            hd_tir::inline::rechoose(&mut body, &|trait_, self_ty, targs| {
                InlineEnv::select_impl(self, trait_, self_ty, targs)
            });
        }
        if dev {
            return Some((body, hash));
        }
        let hash = if supplied.is_some() || StructureEnv::generated_structure_walk(self, def) {
            hd_opt::structure_specialize::specialize_shared_walk(self, &mut body, hash)
        } else {
            hash
        };
        let hash = self.fold_and_inline(&mut body, hash);
        Some((body, hash))
    }

    /// Generic instance `node`'s body after the passes
    /// (`ProgramEnv::instance_body`), owned once: in release with its
    /// callees first ([`Run::owned_bottom_up`]), in dev alone, since no
    /// summary is read; `None` when it has no body.
    pub(super) fn owned_instance(&self, node: Node) -> StageResult<Option<std::sync::Arc<Body>>> {
        let instances = &self.run.instances;
        if self.run.summaries.is_some() && !lock(instances).contains_key(&node) {
            self.run.owned_bottom_up(self.p, node);
        }
        if let Some(owned) = lock(instances).get(&node).cloned().flatten() {
            return Ok(Some(owned));
        }
        // The walk keeps no error: the passes' refusal is reported here.
        let Some((body, hash)) = self.inlined_instance(node) else {
            return Ok(None);
        };
        let (owned, _) = self.own_inlined(body, hash, node.1)?;
        let owned = std::sync::Arc::new(owned);
        if self.run.summaries.is_none() {
            lock(instances).insert(node, Some(owned.clone()));
        }
        Ok(Some(owned))
    }

    /// The reference parameters `body` hands whole to a call at a position
    /// the callee's exclusive copy relies on: the caller needs a copy of its
    /// own to pass that exclusivity on (`hd_tir::own::exclusive_call`).
    fn forwarded_relies(&self, body: &Body) -> u64 {
        let Some(all) = &self.run.summaries else {
            return 0;
        };
        if !matches!(
            body.kind,
            hd_tir::BodyKind::Fn | hd_tir::BodyKind::DeriveInstance
        ) {
            return 0;
        }
        let params = hd_tir::own::params(body);
        let mut out = 0;
        for i in 0..body.len() {
            if body.tags[i] != Tag::Call {
                continue;
            }
            let Some(mask) = Callee::from_words(body.record(body.data[i][0]))
                .and_then(|c| self.callee_node(&c))
                .and_then(|node| all.exclusive(node))
                .map(|e| e.0)
            else {
                continue;
            };
            for (k, &arg) in body.record(body.data[i][1]).iter().enumerate().take(64) {
                let Some(r) = hd_tir::ir::Ref(arg).as_inst().map(|r| r.0 as usize) else {
                    continue;
                };
                if mask & (1 << k) == 0 || body.tags[r] != Tag::Read {
                    continue;
                }
                let p = body.view(body.data[r][0]);
                let at = p
                    .local()
                    .filter(|_| p.whole())
                    .and_then(|l| params.iter().position(|&x| x == l));
                if let Some(q) = at.filter(|&q| q < 64)
                    && self.local_ref_param(body.item, q, params.len())
                {
                    out |= 1 << q;
                }
            }
        }
        out
    }

    /// Inlined body `body` of instance `(body.item, args)`, with hash
    /// `hash` (`hd_opt::inline::inline_calls`), after the ownership
    /// passes, and its hash, which changes with them; in release, its
    /// summary is recorded.
    fn own_inlined(&self, body: Body, hash: Hash128, args: TyList) -> StageResult<(Body, Hash128)> {
        let node = (body.item, args);
        // Release: a body with a `&T` or `&mut T` parameter is kept as it
        // is before the passes, for an exclusive copy (#1118).
        let n = hd_tir::own::params(&body).len();
        let plain = (self.run.summaries.is_some()
            && (0..n).any(|k| self.local_ref_param(body.item, k, n)))
        .then(|| body.clone());
        let (body, summary, relies) = self.own_passes(body, self.run.summaries.is_some())?;
        let relies = relies | self.forwarded_relies(&body);
        if let (Some(all), Some(mut plain)) = (&self.run.summaries, plain)
            && relies != 0
        {
            plain.sub_flags[0] |= hd_tir::ir::sub_flags::EXCLUSIVE;
            let (excl, _, relies) = self.own_passes(plain, false)?;
            let relies = relies | self.forwarded_relies(&excl);
            if relies != 0 {
                let copy = std::sync::Arc::new(excl);
                lock(&all.exclusive).insert(node, (relies, copy));
            }
        }
        if let (Some(all), Some(mut s)) = (&self.run.summaries, summary) {
            s.writes = lock(&all.memory)
                .get(&node)
                .is_none_or(|effects| effects.any());
            // The passes add no write the walk before them missed.
            if !s.writes && (self.run.checked() || cfg!(debug_assertions)) {
                let path = self.run.names().path(body.item);
                assert!(
                    // Recheck the same conditional generated-handle effects
                    // after ownership has inserted explicit operations.
                    !effects::writes_after_ownership(self, &body, node),
                    "{path}: writes after the passes"
                );
            }
            lock(&all.by_item).insert(node, s);
            // What it lends: a recursive component's member its fixed
            // point, which its callers inside the component read; nothing
            // emitted glue calls with owned arguments (the runtime and a
            // host boundary codec's items); and no bit the passes already
            // read as kept.
            let item = body.item;
            let glue = self.called_by_glue(item);
            let n = hd_tir::own::params(&body).len().min(64);
            let read = lock(&all.read);
            let mut lent = !s.kept
                & u64::MAX
                    .checked_shr(64 - u32::try_from(n).expect("params"))
                    .unwrap_or(0);
            if glue {
                lent = 0;
            }
            if let Some(&fixed) = lock(&all.assume).get(&node) {
                // The fixed point holds of the passes' last run.
                assert!(
                    fixed & s.kept == 0,
                    "{}: keeps a parameter its component lends",
                    self.run.names().path(item)
                );
                lent = fixed;
            }
            for k in 0..u32::try_from(n).expect("params") {
                if read.get(&(node, k)) == Some(&false) {
                    lent &= !(1 << k);
                }
            }
            if lent != 0 {
                lock(&all.lent).insert(node, lent);
            }
        }
        let mut k = StableHasher::new("own-passes");
        k.hash(hash);
        Ok((body, k.finish()))
    }

    /// Release (`docs/future-work/RELEASE_INTERPROC.md`, "What each caller
    /// reads"): whether every caller passes parameter `k` of instance
    /// `node` with no count, as its summary lends it. The first answer
    /// stays, so a caller's passes and its emission agree; one read before
    /// the record is no.
    fn summary_lends(&self, node: Node, k: usize) -> bool {
        let Some(all) = &self.run.summaries else {
            return false;
        };
        if let Some(&m) = lock(&all.assume).get(&node) {
            return k < 64 && m & (1 << k) != 0;
        }
        let key = (node, u32::try_from(k).expect("arguments"));
        let mut read = lock(&all.read);
        *read.entry(key).or_insert_with(|| {
            k < 64
                && lock(&all.lent)
                    .get(&node)
                    .is_some_and(|m| m & (1 << k) != 0)
        })
    }

    /// Whether emitted glue calls `item` with owned arguments, so it lends
    /// nothing: a runtime function emitted code calls, an entry's status
    /// function, or a host boundary codec.
    fn called_by_glue(&self, item: DefId) -> bool {
        self.run.glue_items().contains(&item)
    }

    /// Release (#1289, #1232): the borrows of each member of recursive component
    /// `members`, whose inlined bodies are `bodies`, by Lean 4's borrow
    /// inference: every member starts borrowing each value parameter that
    /// may hold a count, and the passes run over each member with calls inside
    /// the component lending what is assumed. A parameter the body keeps
    /// is dropped from the assumption, until no member keeps an assumed
    /// one. Dropping a parameter only makes calls pass owned, which keeps
    /// more, so the bits only fall and the loop ends. A member with no
    /// inlined body, one the passes refuse, one that suspends, and one
    /// glue calls ([`Env::called_by_glue`]) borrow nothing, as does a
    /// parameter a caller's passes already read as kept.
    pub(super) fn component_borrows(
        &self,
        members: &[Node],
        bodies: &HashMap<Node, (Body, Hash128)>,
    ) -> HashMap<Node, u64> {
        let mut assume: HashMap<Node, u64> = HashMap::default();
        let Some(all) = &self.run.summaries else {
            return assume;
        };
        for &m in members {
            let mut mask = 0;
            if let Some((b, _)) = bodies.get(&m)
                && !self.called_by_glue(m.0)
                && !ProgramEnv::suspends(self, m.0)
            {
                let read = lock(&all.read);
                let params = hd_tir::own::params(b);
                for (k, &l) in params.iter().enumerate().take(64) {
                    let kept_before = read.get(&(m, u32::try_from(k).expect("params")));
                    if self.counted(b.local_ty[l as usize])
                        && self.received(m.0, k, params.len()) == Pass::Owned
                        && kept_before != Some(&false)
                    {
                        mask |= 1 << k;
                    }
                }
            }
            assume.insert(m, mask);
        }
        lock(&all.assume).clone_from(&assume);
        let mut fell = true;
        while fell {
            fell = false;
            for &m in members {
                let a = assume[&m];
                if a == 0 {
                    continue;
                }
                let kept = bodies
                    .get(&m)
                    .and_then(|(b, _)| self.trial_kept(b.clone()))
                    .unwrap_or(u64::MAX);
                if a & kept != 0 {
                    assume.insert(m, a & !kept);
                    lock(&all.assume).insert(m, a & !kept);
                    fell = true;
                }
            }
        }
        lock(&all.assume).clear();
        assume
    }

    /// The kept bits of `body` after a run of the passes alone, with no
    /// verification and nothing recorded ([`Env::component_borrows`]);
    /// `None` when the passes refuse it or it has no summary.
    fn trial_kept(&self, mut body: Body) -> Option<u64> {
        let mut scratch = SCRATCH.with(RefCell::take);
        if body.anchors.is_empty() {
            body.anchors = (0..u32::try_from(body.len()).expect("insts")).collect();
        }
        self.root_across_pure_calls(&mut body);
        let kept = hd_wasm::with_layout_cache(|| {
            hd_tir::own::run(&mut body, self, &mut scratch).ok()?;
            hd_tir::own::summary(&body, self, &scratch, None).map(|s| s.kept)
        });
        SCRATCH.with(|s| *s.borrow_mut() = scratch);
        kept
    }

    pub(super) fn lent_mask(&self, node: Node) -> u64 {
        (self.run.summaries.as_ref())
            .and_then(|all| lock(&all.lent).get(&node).copied())
            .unwrap_or(0)
    }

    /// Release (#1151): `def`'s promoted copy at `args` and the parameters
    /// it takes by value: each `&T` parameter whose referent at `args`
    /// is small (`hd_wasm::lent_referent`; a counted one is lent
    /// with no count)
    /// and that the owned body only reads through
    /// (`hd_tir::own::promote::read_only_params`). With `exclusive`, the
    /// copy of `def`'s exclusive copy (`sub_flags::EXCLUSIVE`), for a call
    /// that may run both. Only the parameters in `want` (those whose
    /// arguments the call may pass by value) are candidates, and of those
    /// a counted referent's only in `held` (those whose locals the passes
    /// keep alive over the call). None for a suspending item, or when the
    /// pipeline does not promote.
    pub(super) fn promoted_copy(
        &self,
        def: DefId,
        args: TyList,
        exclusive: bool,
        (want, held): (u64, u64),
    ) -> Option<Exclusive> {
        let all = self
            .run
            .summaries
            .as_ref()
            .filter(|_| self.run.promote_args)?;
        let at = (def, args, exclusive, want, held);
        if let Some(hit) = lock(&all.promoted).get(&at) {
            return hit.clone();
        }
        let out = self.promote(def, args, exclusive, (want, held));
        lock(&all.promoted).insert(at, out.clone());
        out
    }

    fn promote(
        &self,
        def: DefId,
        args: TyList,
        exclusive: bool,
        (want, held): (u64, u64),
    ) -> Option<Exclusive> {
        if ProgramEnv::suspends(self, def) {
            return None;
        }
        // A copy of the body the instance runs, its instance body or its
        // exclusive copy, whose summary decides how a call passes the rest.
        let node = self.node(def, args);
        let excl = exclusive.then(|| self.run.summaries.as_ref()?.exclusive(node));
        let instance;
        let b = match &excl {
            Some(e) => &*e.as_ref()?.1,
            None if node.1 == TyList::EMPTY => ProgramEnv::body(self, def)?,
            None => {
                instance = self.owned_instance(node).ok().flatten()?;
                &*instance
            }
        };
        let pool = &self.run.pool;
        let params = hd_tir::own::params(b);
        let mut candidates = 0;
        for (k, &l) in params.iter().enumerate().take(64) {
            let t = hd_mono::subst(pool, self, def, args, b.local_ty[l as usize]);
            // A counted referent is lent with no count, which needs its
            // local alive over the call.
            let counted = hd_wasm::promotable_referent(pool, self, t).is_none();
            if hd_wasm::lent_referent(pool, self, t).is_some() && (!counted || held & (1 << k) != 0)
            {
                candidates |= 1 << k;
            }
        }
        let mask = hd_tir::own::promote::read_only_params(b, candidates & want);
        if mask == 0 {
            return None;
        }
        let referent = |t: Ty| match pool.get(t) {
            TyData::Ref { inner, .. } => inner,
            _ => t,
        };
        let copy = hd_tir::own::promote::promote(b, mask, referent);
        Some((mask, std::sync::Arc::new(copy)))
    }

    /// The callee scan: the instances whose summaries inlined body
    /// `body`'s calls read, once each in call order
    /// ([`Env::callee_node`]). Only items with a body of their own, at
    /// type arguments that name no parameter: a generic body's call of
    /// an open instance reads no summary.
    fn summarized_callees(&self, body: &Body) -> Vec<Node> {
        let (mut out, mut seen) = (Vec::new(), hd_base::HashSet::default());
        for i in (0..body.len()).filter(|&i| {
            matches!(
                body.tags[i],
                hd_tir::ir::Tag::Call | hd_tir::ir::Tag::DefaultCall
            )
        }) {
            // A default argument's record starts with its function and
            // the call's type arguments.
            let node = if body.tags[i] == hd_tir::ir::Tag::DefaultCall {
                let r = body.record(body.data[i][0]);
                r.first()
                    .zip(r.get(1))
                    .map(|(&d, &l)| self.node(DefId::from_raw(d), TyList(l)))
            } else {
                Callee::from_words(body.record(body.data[i][0])).and_then(|c| self.callee_node(&c))
            };
            if let Some(n) = node
                && self.intrinsic(n.0).is_none()
                && (self.run.lazy_body(self.p, n.0).is_some()
                    || self.supplied_walk_family(n).is_some())
                && !self
                    .run
                    .pool
                    .list_items(n.1)
                    .iter()
                    .any(|&t| self.is_open(t))
                && seen.insert(n)
            {
                out.push(n);
            }
        }
        out
    }
}

impl Run<'_> {
    /// The body table's entry for `def`.
    pub(super) fn lazy_body<'p>(&self, p: &'p ProgramTables, def: DefId) -> Option<&'p LazyBody> {
        p.body_entry(self.paths.hash(hd_base::PathId::from_raw(def.raw())), def)
    }

    /// Runs `f` with the passes' view of the program.
    pub(super) fn with_env<T>(&self, p: &ProgramTables, f: impl FnOnce(&Env<'_>) -> T) -> T {
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
            let hash = env.fold_and_inline(&mut body, hash);
            env.own_inlined(body, hash, TyList::EMPTY)
        });
        match owned {
            Ok(owned) => Some(owned),
            Err(e) => {
                lock(&p.bodies.own_error).get_or_insert_with(|| e.to_string());
                None
            }
        }
    }

    /// The store key of std body `b`'s owned form (#1423), item `def`'s,
    /// or `None` where a build may not share it. One owned form serves
    /// every program, because each input of the passes is either in the
    /// key or the same in every program:
    /// - **Keyed**: the checked body (its TIR hash), std's declarations
    ///   and bodies the inliner copies (`toolchain` holds std's hash), the
    ///   pipeline (`const_fold`, `arg_promote` and every pass and knob),
    ///   the build profile, `checked` (`checked_room_calls`, the glue
    ///   items, the runtime) and whether the passes' output is verified.
    /// - **Program-wide queries, the same for a std body.** A std body
    ///   names only std's items and types built from std's constructors
    ///   and type parameters. So the `Env` queries the passes make are
    ///   about std declarations, layouts and impls, except where they scan
    ///   the program: impl selection, associated-type normalization and
    ///   `has_drop`. Implementation ownership (`trait.own.rule`,
    ///   `trait.own.orphan`) lets a package implement a trait only when it
    ///   owns the trait or the outer constructor of the target or of a
    ///   trait argument, and never a bare-parameter target through an
    ///   argument (`trait.own.bare-parameter`). Every one of those is
    ///   std's here, so no user impl can match, and selection at an open
    ///   type answers nothing (`Env::select_impl`).
    /// - **Left out**: a release build, whose passes read the callees'
    ///   summaries; a build whose root package is std, whose std is not
    ///   the toolchain's and may hold test impls; an entry root
    ///   (`OwnCx::entry_root`, the program's `main` or test cases). std is
    ///   never a fetched package (`Run::fetched`) or a host package.
    pub(super) fn owned_std_key(&self, def: DefId, b: &LazyBody) -> Option<Hash128> {
        let (m, entry, at) = b.wire.as_ref()?;
        let std = self.table.packages.len() - 1;
        let entry_root = self
            .entry_roots
            .get()
            .is_some_and(|roots| roots.contains(&def));
        if self.summaries.is_some()
            || self.package == "std"
            || usize::from(self.table.modules[*m].package) != std
            || entry_root
        {
            return None;
        }
        let mut k = StableHasher::new("owned-std-body-v1");
        k.hash(self.toolchain);
        k.hash(self.pipeline);
        k.u8(self.build_profile as u8);
        k.u8(u8::from(self.checked()));
        k.u8(u8::from(self.verifies_own()));
        k.hash(hd_tir::wire::tir_hash(entry.get(at.clone())?));
        Some(k.finish())
    }

    /// The owned body and its hash stored under `key`
    /// ([`Run::owned_std_key`]), decoded into this run's tables.
    pub(super) fn stored_owned(&self, key: Hash128) -> Option<(Body, Hash128)> {
        let sections = self.lookup(hd_cache::EntryKind::Owned, key)?;
        let [hash, wire] = &sections[..] else {
            return None;
        };
        let hash = Hash128(u128::from_le_bytes((**hash).try_into().ok()?));
        let body = hd_tir::wire::read_body(wire, &self.pool, &self.paths, &self.syms)?;
        Some((body, hash))
    }

    /// Stores owned body `owned` and its hash under `key`.
    pub(super) fn store_owned(&self, key: Hash128, (body, hash): &(Body, Hash128)) {
        // Ruled out: a refusal. The wire form maps every tag the passes
        // write, and `every_std_body_owned_form_round_trips` encodes each
        // std body's owned form.
        if let Ok(wire) = hd_tir::wire::write_body(body, &self.pool, &self.paths, &self.syms) {
            self.put(
                hd_cache::EntryKind::Owned,
                key,
                &[&hash.0.to_le_bytes(), &wire],
            );
        }
    }

    /// Release (`docs/future-work/RELEASE_INTERPROC.md`, "Data flow"): owns
    /// the instances `root` reaches through the callee scan, the callees'
    /// strongly connected components first, so every summary a body's
    /// passes may read is recorded before they run. Each instance's body
    /// is its item's own, or a generic item's at its type arguments
    /// ([`Env::inlined_instance`]), and the passes run over it once: an
    /// item's own body is its lazy body's owned entry, a generic
    /// instance's is kept for collection (`Run::instances`). Each
    /// component's members are owned in path order and marked when it is
    /// recursive. A body the passes refuse stays unowned, for
    /// `Run::body_of` or `Env::owned_instance` to report.
    pub(super) fn owned_bottom_up(&self, p: &ProgramTables, root: Node) {
        let Some(all) = &self.summaries else {
            return;
        };
        let done = |n: Node| {
            if n.1 == TyList::EMPTY {
                self.lazy_body(p, n.0)
                    .is_none_or(|b| b.owned.get().is_some())
            } else {
                lock(&self.instances).contains_key(&n)
            }
        };
        self.with_env(p, |env| {
            let inlined = RefCell::new(HashMap::default());
            let mut callees = |n: Node| {
                let Some((body, hash)) = env.inlined_instance(n) else {
                    return env.borrowed_effect_callees(n);
                };
                let out = env.summarized_callees(&body);
                inlined.borrow_mut().insert(n, (body, hash));
                out
            };
            let names = self.names();
            let mut canons = hd_mono::layout::CanonMemo::default();
            // A member's path, then one digest of its type arguments.
            let mut order = |n: &Node| {
                let mut args = StableHasher::new("instance-order-v1");
                for &t in self.pool.list_items(n.1) {
                    args.hash(hd_mono::layout::canon(
                        &self.pool,
                        &|d| names.path_hash(d),
                        &mut canons,
                        t,
                    ));
                }
                (names.path(n.0), args.finish())
            };
            let mut own = |members: &mut Vec<Node>, recursive: bool| {
                use hd_tir::own::memory_effects::{Call, Equation, Writes};
                members.sort_stable_by_cached_key(&mut order);
                // Complete write effects are monotone. A reverse-edge worklist
                // transfers each of 64 parameter bits/external once per call,
                // rather than rechecking every body after every callee change.
                let bodies = inlined.borrow();
                let indices: HashMap<_, _> = members
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(i, n)| (n, i))
                    .collect();
                let equations: Vec<_> = members
                    .iter()
                    .map(|member| {
                        let effects = if let Some((body, _)) = bodies.get(member) {
                            env.conditional_equation(body)
                        } else {
                            env.borrowed_equation(*member).unwrap_or_else(|| {
                                hd_tir::own::memory_effects::BodyEffects {
                                    direct: Writes::EXTERNAL,
                                    calls: Vec::new(),
                                }
                            })
                        };
                        let mut equation = Equation {
                            direct: effects.direct,
                            calls: Vec::new(),
                        };
                        for call in effects.calls {
                            let target = env.callee_node(&call.callee);
                            if target.is_some_and(|node| ProgramEnv::suspends(env, node.0)) {
                                equation.direct.join(call.transfer.apply(Writes::EXTERNAL));
                            } else if let Some(callee) =
                                target.and_then(|n| indices.get(&n).copied())
                            {
                                equation.calls.push(Call {
                                    callee,
                                    transfer: call.transfer,
                                });
                            } else {
                                equation
                                    .direct
                                    .join(call.transfer.apply(env.call_writes(&call.callee)));
                            }
                        }
                        equation
                    })
                    .collect();
                let effects = hd_tir::own::memory_effects::solve(&equations);
                for (&member, effects) in members.iter().zip(effects) {
                    lock(&all.memory).insert(member, effects);
                }
                drop(bodies);
                if recursive {
                    for &m in members.iter() {
                        lock(&all.cycle).insert(m, members[0]);
                    }
                    let borrows = env.component_borrows(members, &inlined.borrow());
                    *lock(&all.assume) = borrows;
                }
                for &m in members.iter() {
                    let body = inlined.borrow_mut().remove(&m);
                    let owned = body.and_then(|(body, hash)| env.own_inlined(body, hash, m.1).ok());
                    if m.1 != TyList::EMPTY {
                        let owned = owned.map(|(body, _)| std::sync::Arc::new(body));
                        lock(&self.instances).insert(m, owned);
                    } else if let (Some(lazy), Some(owned)) = (self.lazy_body(p, m.0), owned) {
                        let _ = lazy.owned.set(Some(owned));
                    }
                }
                lock(&all.assume).clear();
            };
            hd_opt::inline::bottom_up(root, &done, &mut callees, &mut own);
        });
    }
}

#[cfg(test)]
pub(super) mod owned_cache_tests {
    use std::cell::Cell;

    use hd_base::{BuildProfile, Hash128};
    use hd_cache::{CacheStore, EntryKind, MemoryStore};
    use hd_intern::{PathTable, ShardedInterner};
    use hd_project::MemorySources;
    use hd_tir::ir::Body;
    use hd_types::InternPool;

    use super::{ProgramTables, Run};
    use crate::{Executor, Goal, Host, NoClock, Packages, build_packages};

    thread_local! {
        /// While set, how many stored bodies a build verified.
        static VERIFIED: Cell<Option<usize>> = const { Cell::new(None) };
    }

    /// Verifies a std body a build took from the store
    /// (`Run::own_lazy`), while [`every_std_body_owned_form_round_trips`]
    /// asks.
    pub(crate) fn verify_stored(run: &Run<'_>, p: &ProgramTables, body: &Body) {
        let Some(n) = VERIFIED.get() else {
            return;
        };
        let errs = run.with_env(p, |env| hd_tir::ir::verify_owned(body, env));
        if let Some(e) = errs.first() {
            panic!(
                "stored `{}` fails invariant {} at %{}: {}",
                run.names().path(body.item),
                e.invariant,
                e.inst,
                e.what
            );
        }
        VERIFIED.set(Some(n + 1));
    }

    /// Programs over much of std: JSON and serde, maps, sorting, strings,
    /// closures, tasks and trait values.
    const PROGRAMS: [&str; 5] = [
        include_str!("../../../bench/study/json.hd"),
        include_str!("../../../bench/runtime/progs/map-count.hd"),
        include_str!("../../../bench/runtime/progs/task-pipeline.hd"),
        include_str!("../../../bench/runtime/progs/string-builder.hd"),
        include_str!("../../../bench/runtime/progs/plugins.hd"),
    ];

    fn build(store: &MemoryStore, text: &str, profile: BuildProfile) -> crate::Output {
        let mut sources = MemorySources::default();
        sources.insert("main.hd", text);
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store,
            clock: &NoClock,
            executor: Executor::Serial(hd_sched::SerialOrder::Priority),
        };
        let packages = Packages {
            build_profile: profile,
            ..Packages::default()
        };
        let out = build_packages(
            &host,
            "app",
            &packages,
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(!out.diags.has_errors(), "{}", out.render());
        out
    }

    /// Every std body these programs own is stored once per profile, its
    /// wire form decodes into another run's tables and encodes back to the
    /// same bytes, and a later build takes each from the store, whose
    /// owned verifier accepts it.
    #[test]
    fn every_std_body_owned_form_round_trips() {
        let store = MemoryStore::default();
        for profile in [BuildProfile::Checked, BuildProfile::Release] {
            for text in PROGRAMS {
                build(&store, text, profile);
            }
        }
        let owned: Vec<Hash128> = store
            .drain_new()
            .into_iter()
            .filter_map(|(kind, key)| (kind == EntryKind::Owned).then_some(key))
            .collect();
        assert!(owned.len() > 380, "{} owned std bodies", owned.len());
        // A store with only the owned bodies: the builds below check, collect
        // and link again, and take every std body from it.
        let warm = MemoryStore::default();
        for key in &owned {
            let entry = store.get(EntryKind::Owned, key).expect("stored");
            let (_, sections) = hd_cache::decode_entry(&entry).expect("an entry");
            let wire = sections[1].1;
            let (pool, paths, syms) = (
                InternPool::new(),
                PathTable::new(),
                ShardedInterner::default(),
            );
            // Other IDs than the writer's run.
            paths.item("zzz", "a", "b");
            let body = hd_tir::wire::read_body(wire, &pool, &paths, &syms).expect("decodes");
            let again = hd_tir::wire::write_body(&body, &pool, &paths, &syms).expect("encodes");
            assert_eq!(wire, &again[..], "{key:?}");
            warm.put(EntryKind::Owned, key, &entry);
        }
        VERIFIED.set(Some(0));
        let mut hits = 0;
        for profile in [BuildProfile::Checked, BuildProfile::Release] {
            for text in PROGRAMS {
                let out = build(&warm, text, profile);
                assert_eq!(out.counters.misses.get("owned"), None);
                hits += out.counters.hits.get("owned").copied().unwrap_or(0);
            }
        }
        assert_eq!(VERIFIED.take(), Some(hits));
        assert!(hits >= owned.len());
    }
}
