//! What `tests/unit` reaches inside the checker: the feature `testing`,
//! which only the crate's own dev-dependency turns on, so a normal build
//! neither compiles nor exports it.
//!
//! The unit tests live outside `src` so that `cargo test` builds this
//! library once, not again in test mode (`[lib] test = false`; matklad,
//! "Delete Cargo Integration Tests", 2021). They see three kinds of item:
//! - [`Checker`]: a body checker over an empty program, with the queries
//!   the tests drive, so its fields and methods stay crate-private;
//! - functions that run one internal constructor or query and hand back
//!   its parts, for internal-error and work-count tests;
//! - re-exports of the self-contained algorithms the tests exercise as a
//!   whole (region solving, line-up keys, join logs, candidate indexes).
//!
//! What needs a module's private fields stays in that module behind the
//! feature: `Results`' signature bookkeeping, `Solution`'s set sizes and
//! `pat::decide_unsettled`.
//!
//! Cost: nothing in a normal build; with the feature, these functions and
//! one thread-local counter bump per pair `same_but_projections` visits.

use std::cell::{Cell, OnceCell, RefCell};
use std::sync::Arc;

use hd_base::{DefId, FileId, Fuel, HashMap, InferVar, LocalId, NotImplemented, StageResult};
use hd_diag::DiagBuf;
use hd_intern::{PathTable, ShardedInterner};
use hd_resolve::error_type::ErrorShape;
use hd_resolve::{
    Field, FnSig, Item, KnownItems, Locals, Lookup, ModuleScope, Names, Src, Variant,
};
use hd_syntax::NodeRef;
use hd_tir::ir::{IntrinsicOp, NONE, Ref, TirBuilder};
use hd_types::lookup::{UnitSlots, Unseen};
use hd_types::solver::{
    Answer, Candidate, Evidence, GlobalMemo, ImplUniverseId, ImplView, OwnerMap, ParamEnv,
    TableSolver, UniverseImpls,
};
use hd_types::{FnDeps, InferTable, InternPool, LocalPool, RowId, Ty, TyData, TyList, Types};

use crate::body::{BodyCx, Ck, ProjectSite, new_ck};

pub use crate::collide::{can_coincide, mentions_param};
pub use crate::dep_results::{Key, Layouts, Var, line_up, line_up_with};
pub use crate::expr::interpolation_buffer_field;
pub use crate::facts::facts_items;
pub use crate::fnref::{Member, MemberFnType, Owner};
pub use crate::frames::Frames;
pub use crate::join::{DeferredTry, Joins};
pub use crate::local::{LocalGeneric, named_closure_binding};
pub use crate::local_refs::{EscapeMemo, escape_in, hidden, holds_local_ref, lends};
pub use crate::pat::variant_list;
pub use crate::regions::{Category, Constraints, Edge, Loan, Region, RegionTable, Solution};
pub use crate::rows::{KeyCandidates, key_is};
pub use crate::trial::candidate_table;
pub use crate::value::{ProjectionCycles, inline_kids};

/// The deepest row key [`row_key_depth`] counts to.
pub const MAX_KEY_DEPTH: u32 = crate::results::MAX_KEY_DEPTH;

/// Runs `run` on a checker of an empty function `app.main.test`.
pub fn with_checker(run: impl FnOnce(&mut Checker<'_, '_, '_>)) {
    with_checker_items(|_| Vec::new(), run);
}

/// [`with_checker`] over a program whose items `items` declares.
pub fn with_checker_items(
    items: impl FnOnce(Names<'_>) -> Vec<Item>,
    run: impl FnOnce(&mut Checker<'_, '_, '_>),
) {
    let pool = InternPool::new();
    let local = LocalPool::new();
    let paths = PathTable::new();
    let syms = ShardedInterner::default();
    let known = KnownItems::new(&paths);
    let names = Names {
        pool: &pool,
        paths: &paths,
        syms: &syms,
        known: &known,
    };
    let item = names.item("app.main", "test");
    let parsed = hd_syntax::parse(b"");
    let index = hd_resolve::iface::IfaceIndex::default();
    let items = items(names);
    let lookup = Lookup::new(&items, std::iter::empty(), &index);
    let owners = OwnerMap::default();
    let extra = UniverseImpls::default();
    let impls = ImplView {
        paths: &paths,
        owners: &owners,
        own: None,
        units: UnitSlots::default(),
        universe: ImplUniverseId(0),
        extra: &extra,
        arity: &|_| 0,
        hidden: Unseen::default(),
        plans: None,
    };
    let global = GlobalMemo::default();
    let scope = ModuleScope::default();
    let locals = Locals::new(vec![]);
    let cx = BodyCx {
        names,
        src: Src {
            parse: &parsed,
            text: "",
            file: FileId::from_raw(0),
        },
        scope: &scope,
        lookup: &lookup,
        impls: &impls,
        global: &global,
        solver: &TableSolver,
        methods: Arc::default(),
        init: RefCell::default(),
        results: RefCell::default(),
        dbg_refs: RefCell::default(),
        source_facts: None,
        locals: &locals,
        enclosing: OnceCell::default(),
        instantiations: RefCell::default(),
        traits_in_scope: Arc::default(),
        file_name: "test.hd",
        tests: None,
        session: &[],
        fuel: Fuel::BODY_DEFAULT,
        verify_tir: false,
    };
    let mut diags = DiagBuf::default();
    let mut ck = new_ck(
        &cx,
        &local,
        item,
        item,
        hd_tir::BodyKind::Fn,
        (Ty::VOID, RowId::EMPTY),
        &mut diags,
    );
    run(&mut Checker { ck: &mut ck });
}

/// A body checker: its context, the state the tests read, and the
/// queries they drive.
pub struct Checker<'r, 'a, 'c> {
    ck: &'r mut Ck<'a, 'c>,
}

impl<'a, 'c> Checker<'_, 'a, 'c> {
    #[must_use]
    pub fn cx(&self) -> &'c BodyCx<'a> {
        self.ck.cx
    }

    #[must_use]
    pub fn pool(&self) -> Types<'c> {
        self.ck.pool()
    }

    /// The root of the (empty) source file, for a query's syntax position.
    #[must_use]
    pub fn root(&self) -> NodeRef<'a> {
        self.ck.cx.src.parse.tree.root()
    }

    pub fn b(&mut self) -> &mut TirBuilder {
        &mut self.ck.b
    }

    pub fn infer(&mut self) -> &mut InferTable {
        &mut self.ck.infer
    }

    pub fn env(&mut self) -> &mut ParamEnv {
        &mut self.ck.env
    }

    pub fn strict_rets(&mut self) -> &mut Vec<Ty> {
        &mut self.ck.strict_rets
    }

    pub fn local_generics(&mut self) -> &mut HashMap<LocalId, LocalGeneric> {
        &mut self.ck.local_generics
    }

    /// Open lexical scopes, open loops and pending goals.
    #[must_use]
    pub fn depths(&self) -> (usize, usize, usize) {
        (
            self.ck.scopes.depth(),
            self.ck.loops.len(),
            self.ck.pending.len(),
        )
    }

    #[must_use]
    pub fn optional(&self, t: Ty) -> Ty {
        self.ck.optional(t)
    }

    pub fn repr_change(&mut self, got: Ty, want: Ty, depth: u32) -> bool {
        self.ck.repr_change(got, want, depth)
    }

    pub fn normalize_deep(&mut self, t: Ty) -> StageResult<Ty> {
        self.ck.normalize_deep(t)
    }

    pub fn normalize_deep_in(&mut self, t: Ty, memo: &mut HashMap<Ty, Ty>) -> StageResult<Ty> {
        self.ck.normalize_deep_in(t, memo)
    }

    /// Opens the projections of a call's result type `t` at the root.
    pub fn open_result_projections(&mut self, t: Ty) -> Ty {
        let root = self.root();
        self.ck.open_projections(t, ProjectSite::Result, root)
    }

    /// Adds the bounds of `t`'s projections to the checker's own
    /// environment; whether any was added.
    pub fn add_projection_bounds(&mut self, t: Ty) -> bool {
        let cx = self.ck.cx;
        crate::body::add_projection_bounds(
            cx.lookup,
            self.ck.pool(),
            &mut self.ck.env,
            t,
            cx.names.known.copy,
        )
    }

    pub fn unify_join(&mut self, a: Ty, b: Ty) -> bool {
        self.ck.unify_join(a, b)
    }

    #[must_use]
    pub fn lct_meet(&self, a: Ty, b: Ty, top: bool) -> Option<Ty> {
        self.ck.lct_meet(a, b, top)
    }

    #[must_use]
    pub fn lct_converts(&self, a: Ty, b: Ty, top: bool) -> bool {
        self.ck.lct_converts(a, b, top)
    }

    #[must_use]
    pub fn strict_opaque_return(&self, t: Ty) -> bool {
        self.ck.strict_opaque_return(t)
    }

    pub fn constrain_opaque_return(&mut self, got: Ty, want: Ty) -> bool {
        let root = self.root();
        self.ck.constrain_opaque_return(got, want, root)
    }

    pub fn solve_nested_rows(&mut self, got: Ty, want: Ty, depth: u32) {
        self.ck.solve_nested_rows(got, want, depth);
    }

    pub fn proj_lifetimes(&self, ls: &Layouts<'_>, proj: Ty) -> Vec<u32> {
        self.ck.proj_lifetimes(ls, proj)
    }

    pub fn bound_ids_in(
        &self,
        ls: &Layouts<'_>,
        self_ty: Ty,
        trait_: DefId,
        memo: &mut HashMap<(Ty, DefId), Vec<Vec<u32>>>,
    ) -> Vec<Vec<u32>> {
        self.ck.bound_ids_in(ls, self_ty, trait_, memo)
    }

    /// Self-reference questions about the data type `data`, with their
    /// own memo.
    #[must_use]
    pub fn self_refs(&self, data: DefId) -> SelfRefs<'c, 'a> {
        SelfRefs(crate::structure::SelfRefs::new(self.ck.cx, data))
    }

    /// Whether `from` converts to `to` through the newtype `newtype` over
    /// `base`, and the plan that does it.
    pub fn newtype_plan(&self, (base, newtype): (Ty, Ty), from: Ty, to: Ty) -> Option<NewtypePlan> {
        crate::newtype::Conversion { newtype, base }
            .can(self.ck, from, to)
            .map(NewtypePlan)
    }

    /// Emits `plan` on the value `input`.
    pub fn apply_newtype_plan(&mut self, input: Ref, plan: NewtypePlan) -> Ref {
        let root = self.root();
        crate::newtype::Conversion::apply(self.ck, input, plan.0, root)
    }

    pub fn map_loop_entry(&self, item: Ty) -> StageResult<IntrinsicOp> {
        self.ck.map_loop_entry(item)
    }

    /// A map loop over items of type `item` with a poisoned source,
    /// calling `body` for its body.
    pub fn map_loop(
        &mut self,
        item: Ty,
        body: &mut dyn FnMut() -> StageResult<()>,
    ) -> StageResult<(Ref, Ty)> {
        let root = self.root();
        self.ck.collection_loop(
            root,
            (Ref(NONE), Ty::POISON, item, true),
            (None, None),
            root,
            &mut |_| body(),
        )
    }

    pub fn range_comparison(&mut self, lo: Option<Ref>, hi: Option<Ref>) -> StageResult<Ref> {
        self.ck.range_comparison(lo, hi, hd_base::NodeIdx::NONE)
    }

    /// A one-row decision whose only cell is the unsettled pattern node
    /// `pattern`, over an `i32` value; with whether its decision stack
    /// stayed empty.
    pub fn decide_unsettled(&mut self, pattern: NodeRef<'_>) -> (StageResult<()>, bool) {
        crate::pat::decide_unsettled(self.ck, pattern)
    }

    pub fn new_interpolation_builder(&mut self) -> StageResult<()> {
        let root = self.root();
        self.ck.new_interpolation_builder(root).map(drop)
    }

    pub fn string_builder_helper(&mut self, name: &str) -> StageResult<DefId> {
        self.ck.string_builder_helper(name)
    }

    pub fn builder_call(&mut self, name: &str, args: &[Ref]) -> StageResult<(Ref, Ty)> {
        let root = self.root();
        self.ck.builder_call(name, args, root)
    }

    pub fn interpolation_inherent(
        &mut self,
        receiver: Ty,
        name: &str,
    ) -> StageResult<(DefId, DefId, Vec<Ty>)> {
        self.ck.interpolation_inherent(receiver, name)
    }

    /// The function type of `member`, its first `skip` parameters dropped.
    pub fn member_fn_ty(&mut self, member: &Member, skip: usize) -> StageResult<MemberFnType> {
        self.ck.member_fn_ty(member, skip)
    }

    #[must_use]
    pub fn sig_fn_deps(&self, sig: &FnSig, skip: usize) -> FnDeps {
        self.ck.sig_fn_deps(sig, skip)
    }

    /// A dynamic reference to `member` on `receiver`.
    pub fn dynamic_member_ref(
        &mut self,
        member: Member,
        receiver: (Ty, Ty, bool),
    ) -> StageResult<(Ref, Ty)> {
        let root = self.root();
        self.ck
            .dynamic_member_ref(member, receiver, None, None, root)
    }

    pub fn local_generic_entry(&mut self, local: LocalId) -> StageResult<&mut LocalGeneric> {
        self.ck.local_generic_entry(local)
    }

    /// A call of `method` of `trait_` on `self_ty` through `candidate`,
    /// with no receiver or arguments.
    pub fn call_candidate(
        &mut self,
        callee: (DefId, DefId, Ty),
        candidate: &Candidate,
    ) -> StageResult<(Ref, Ty)> {
        let root = self.root();
        let args = crate::call::Args::empty();
        self.ck
            .call_candidate(callee, candidate, None, &args, (root, false), None)
    }

    /// The payload of an `Error.source` result type `ret`.
    pub fn error_source_payload(&self, ret: Ty) -> StageResult<Ty> {
        crate::error::source_payload(self.ck.cx, ret)
    }

    /// The `source` arm `@error` emits for variant `variant` of the data
    /// type `ty`, whose variants' members are `fields`, on the bound value
    /// `source`.
    pub fn error_cause_arm(
        &mut self,
        (ty, shape, fields): (DefId, &ErrorShape<'_>, Vec<Vec<Field>>),
        source: (LocalId, Ty),
        variant: usize,
        ret: Ty,
    ) -> StageResult<Ref> {
        crate::error::Target {
            ty,
            shape,
            fields,
            variants: Vec::new(),
            shared: Vec::new(),
        }
        .cause_arm(self.ck, source, variant, ret, &|t| t)
    }
}

/// [`Checker::self_refs`].
pub struct SelfRefs<'a, 'c>(crate::structure::SelfRefs<'a, 'c>);

impl SelfRefs<'_, '_> {
    /// Whether a member of type `t` needs `SelfRef`.
    pub fn needs(&self, t: Ty) -> bool {
        self.0.needs(t, &mut Vec::new())
    }

    pub fn refers(&self, t: Ty) -> bool {
        self.0.refers(t)
    }
}

/// [`Checker::newtype_plan`].
pub struct NewtypePlan(crate::newtype::ConversionPlan);

/// The parts a selected source function borrows: its header, signature
/// and block.
pub fn source_function<'i, 't>(
    item: Option<&'i Item>,
    node: NodeRef<'t>,
) -> StageResult<(&'i Item, &'i FnSig, NodeRef<'t>)> {
    crate::body::SourceFunction::new(item, node).map(|f| (f.item, f.sig, f.body))
}

/// The header and signature a derive instantiation selects.
pub fn resolved_callable(item: Option<&Item>) -> StageResult<(&Item, &FnSig)> {
    crate::derive::ResolvedCallable::new(item).map(|c| (c.item, c.sig))
}

/// A selected `Structure` declaration's item, its fields (a struct's) or
/// shared fields (an enum's), and an enum's variants.
pub type StructureParts<'a> = (&'a Item, &'a [Field], Option<&'a [Variant]>);

/// The parts of the `Structure` declaration `item` selects.
pub fn structure_decl(item: Option<&Item>) -> StageResult<StructureParts<'_>> {
    use crate::structure::StructureMembers;
    let decl = crate::structure::StructureDecl::new(item)?;
    Ok(match decl.members {
        StructureMembers::Struct(fields) => (decl.item, fields, None),
        StructureMembers::Enum { shared, variants } => (decl.item, shared, Some(variants)),
    })
}

/// The arguments of a selected nominal type.
pub fn adt_args(ty: &TyData) -> StageResult<TyList> {
    crate::structure::AdtArgs::new(ty).map(|args| args.0)
}

/// The outcome of an `Implements` proof, as the checker consumes it.
#[derive(Debug)]
pub enum Proof {
    Holds {
        evidence: Evidence,
        learned: Vec<(InferVar, Ty)>,
    },
    Fails,
    Stalled,
    Overflow,
}

pub fn proof_answer(answer: Answer) -> StageResult<Proof> {
    use crate::body::ProofAnswer;
    Ok(match ProofAnswer::try_from(answer)? {
        ProofAnswer::Holds { evidence, learned } => Proof::Holds { evidence, learned },
        ProofAnswer::Fails => Proof::Fails,
        ProofAnswer::Stalled => Proof::Stalled,
        ProofAnswer::Overflow => Proof::Overflow,
    })
}

#[must_use]
pub fn is_out_of_fuel(e: &NotImplemented) -> bool {
    crate::body::is_out_of_fuel(e)
}

/// `n` units of join work (`crate::work::count_join_work`).
pub fn join_work(n: usize) {
    crate::work::join_work(n);
}

/// The returned origins of a call whose one output is `outs`, after
/// `links`; `work` is called once per unit of work.
pub fn returned_origins(
    outs: &[Var],
    links: &[(Var, Var)],
    work: impl FnMut(),
) -> hd_base::HashSet<Var> {
    crate::dep_results::call_origins(&[], &[outs.to_vec()], &[], links, work).returned
}

/// The nesting depth of a row key's type `t` met at depth `at`.
#[must_use]
pub fn row_key_depth(pool: Types<'_>, t: Ty, at: u32) -> u32 {
    crate::results::depth(pool, t, at)
}

/// Whether solving a row from `before` to `after` changes its signature.
#[must_use]
pub fn row_changed(before: Option<RowId>, after: Option<RowId>) -> bool {
    crate::results::row_changed(before, after)
}

/// Whether the implementation's type `g` is the trait's type `w` up to
/// projections, and the pairs the comparison visited.
pub fn same_but_projections(
    pool: Types<'_>,
    (w, g): (Ty, Ty),
    bound: &dyn Fn(Ty) -> Option<Ty>,
) -> (bool, usize) {
    PROJECTION_PAIRS.set(0);
    let same = crate::conform::same_but_projections(pool, (w, g), bound);
    (same, PROJECTION_PAIRS.get())
}

thread_local! {
    static PROJECTION_PAIRS: Cell<usize> = const { Cell::new(0) };
}

/// One pair `same_but_projections` visits.
pub(crate) fn projection_pair_visited() {
    PROJECTION_PAIRS.set(PROJECTION_PAIRS.get() + 1);
}

/// Generated dependency graphs, including nested declaration scopes. Returns
/// cyclic components, nodes plus edges walked, and work after the group clears.
#[must_use]
pub fn result_cycle_work(width: u32, depth: u32, nested: bool) -> (usize, u64, u64) {
    crate::result_cycles::generated_work(width, depth, nested)
}
