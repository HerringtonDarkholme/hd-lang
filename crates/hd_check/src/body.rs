//! Body checking (`crates/hd_check/DESIGN.md` (type checking) §1.4 to §1.6; `crates/hd_tir/DESIGN.md`
//! §4.13): one function body over the full parser's tree, typed with an
//! `InferTable` whose variable types live in the body's `LocalPool`, trait goals through the
//! `Solver` with fuel, TIR written through `TirBuilder`. User errors go to
//! the `DiagBuf`; a construct the checker does not carry yet is a
//! structured "not implemented", which stops a build.
//!
//! Expressions are in `expr`, calls and member lookup in `call`, patterns
//! and `match` in `pat`, annotations in `ty`.

use hd_base::SliceSort as _;
use hd_base::{HashMap, HashSet};
use std::fmt::Write as _;

use hd_base::{DefId, Fuel, LocalId, ModuleId, NotImplemented, Span, Stage, StageResult, Symbol};
use hd_diag::{Code, DiagBuf};
use hd_resolve::{ItemData, Lookup, LookupDecls, ModuleScope, Names, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::Body;
use hd_tir::ir::{
    BodyKind, Callee, Coercion, IntrinsicOp, LoopMark, NONE, Ref, SubMark, Tag, TirBuilder,
    TirSink, VerifyError, local_flags,
};
use hd_types::solver::{
    Answer, BodyMemo, Evidence, GlobalMemo, Goal, ImplTable, ImplView, Impls, ParamEnv, SolveCx,
    Solver, TraitRef,
};
use hd_types::{InferTable, ParamRef, RowId, Ty, TyData, TyList, VarKind};

use crate::expr::Indexed;

/// What a body sees: the run's tables, its module's scope, the items of
/// its closure and its view of the impls (owner tables and its impl
/// universe, `crates/hd_check/DESIGN.md` (trait solver) §3.2).
pub struct BodyCx<'a> {
    pub names: Names<'a>,
    pub src: Src<'a>,
    pub scope: &'a ModuleScope,
    pub lookup: &'a Lookup<'a>,
    pub impls: &'a ImplView<'a>,
    pub global: &'a GlobalMemo,
    pub solver: &'a dyn Solver,
    /// The methods of the closure by name, built on first use and shared
    /// with the module's forked contexts ([`crate::fork`]).
    pub methods: std::cell::OnceCell<std::sync::Arc<crate::MethodIndex>>,
    /// The module's top-level bindings and its bodies' init facts.
    pub init: std::cell::RefCell<crate::init::ModuleInit>,
    /// The module's private callables with omitted result types (M1).
    pub results: std::cell::RefCell<crate::results::Results>,
    /// Every reference to `dbg` the module's bodies make, which a release
    /// build rejects in the user's own code (`module.dbg.release`).
    pub dbg_refs: std::cell::RefCell<Vec<crate::dbg::DbgRef>>,
    /// Optional source facts requested by tooling; absent on ordinary builds.
    pub source_facts: Option<std::cell::RefCell<crate::comparison::SourceFacts>>,
    /// The module's block-local declarations, lifted to hidden items
    /// (`crates/hd_tir/DESIGN.md` "Local items lift to hidden module items").
    pub locals: &'a [hd_resolve::LocalItem],
    /// The module's file as diagnostics name it, which a `dbg` line's
    /// location starts with (`module.dbg.location`).
    pub file_name: &'a str,
    /// When the module's test code is checked, its `tests:` block.
    pub tests: Option<TestsView<'a>>,
    /// For a REPL input: the modules of its session, the input itself,
    /// `src/lib.hd` and the earlier inputs. A session is one scope, as a
    /// module is (`cli.repl.package.lib`), so a member one of them declares
    /// is visible to the input's bodies as their own module's. Empty for
    /// any other module.
    pub session: &'a [hd_base::PathId],
    /// The fuel of each body this context checks: `Fuel::BODY_DEFAULT`,
    /// or a smaller budget a test sets to reach `item-too-complex` fast.
    pub fuel: u64,
}

/// A module's `tests:` block, when its items are checked: its extent (a
/// second block is an error, `grammar.tests.once`, but sees the same), and
/// the scope inside it, which only code in the block sees (spec 03 "Tests
/// Blocks").
#[derive(Clone, Copy)]
pub struct TestsView<'a> {
    pub blocks: &'a [hd_base::Span],
    pub scope: &'a ModuleScope,
}

impl<'a> BodyCx<'a> {
    /// The module-level names seen at byte `at`: the `tests:` block's
    /// scope inside the block (`names.tests.inside-only`), else the
    /// module's.
    #[must_use]
    pub fn scope_at(&self, at: u32) -> &'a ModuleScope {
        match self.tests {
            Some(t) if t.blocks.iter().any(|b| b.lo <= at && at < b.hi) => t.scope,
            _ => self.scope,
        }
    }
}

impl BodyCx<'_> {
    /// The declarations the solver's compiler-supplied rows read
    /// (`crates/hd_check/DESIGN.md` (trait solver) §3.9).
    pub(crate) fn decls(&self) -> LookupDecls<'_> {
        LookupDecls {
            lookup: self.lookup,
            pool: self.names.pool,
            sealed: self.names.known.sealed(),
            paths: self.names.paths,
            syms: self.names.syms,
        }
    }

    /// The table an impl row of a solver answer lives in.
    #[must_use]
    pub fn impl_table(&self, m: ModuleId) -> Option<&ImplTable> {
        Impls::Owned(self.impls).table(m)
    }
}

pub(crate) fn unsupported<T>(what: impl Into<String>) -> StageResult<T> {
    Err(NotImplemented::new(Stage::Body, what))
}

/// What a check stops with when the body's fuel runs out, in its own
/// steps or the solver's: the body's entry turns it into
/// `item-too-complex` (`too_complex`).
const OUT_OF_FUEL: &str = "the body's fuel ran out";

pub(crate) fn out_of_fuel<T>() -> StageResult<T> {
    unsupported(OUT_OF_FUEL)
}

/// Whether a check stopped because its body's fuel ran out.
pub(crate) fn is_out_of_fuel(e: &NotImplemented) -> bool {
    e.stage == Stage::Body && e.what == OUT_OF_FUEL
}

/// The message of `item-too-complex`, in a body and in a header.
pub(crate) const TOO_COMPLEX: &str =
    "checking this item needs more steps than the compiler allows; split it into smaller items";

/// `fn.limit.complexity.error` (`crates/hd_check/DESIGN.md` (type
/// checking) §11.2): an item whose check ran out of fuel gets one
/// `item-too-complex` at `at`. The diagnostics reported before stay, and
/// its TIR is a single poison value.
pub(crate) fn too_complex(
    cx: &BodyCx<'_>,
    (item, kind): (DefId, BodyKind),
    at: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Body> {
    diags.error(Code::ItemTooComplex, cx.src.span(at), TOO_COMPLEX);
    let mut b = TirBuilder::new(item, kind);
    let blk = b.open_block();
    let p = b.emit(Tag::Poison, NONE, NONE, Ty::POISON, at.index());
    let root = b.close_block(blk, Some(p), Ty::POISON, at.index());
    b.finish(root, &[])
        .map_err(|e| VerifyError::bug(Stage::Body, "TIR verifier", &e))
}

/// An open closure: its builder mark, the scope depth at its start, its
/// result type.
/// One level of the available-keys stack (`crates/hd_check/DESIGN.md` (type checking) §5.2).
#[derive(Clone)]
pub(crate) enum RowFrame {
    /// A function's declared row: the bottom of a body's stack.
    Declared(RowId),
    /// The entry module's top level: every key is available.
    Any,
    /// A closure with a written row, or (`written: None`) the keys its
    /// body uses so far, which become its inferred row
    /// (`req.row.omitted.closure-row`).
    Closure {
        written: Option<RowId>,
        used: hd_types::RowData,
    },
    /// A private function's omitted row (`req.row.omitted.inferred-private`):
    /// the keys its body uses so far outside its own `$.with` blocks, and
    /// its calls of rows still open (`crates/hd_tir/DESIGN.md` "M3: inferred
    /// rows").
    Inferred {
        used: hd_types::RowData,
        edges: Vec<crate::results::RowEdge>,
    },
    /// The keys a `$.with` block binds.
    With(RowId),
    /// A test case's body: the keys the runner binds for it, and those of
    /// them the body uses so far, which become the case's row
    /// (`module.testing.unit-row.test-runner`,
    /// `module.testing.integration-row`).
    Profile { row: RowId, used: hd_types::RowData },
}

/// The checked primitive or writable-annotation category of a binding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LetMutKind {
    /// Its type is primitive (types.bind.let-mut-primitive-local).
    Primitive,
    /// Its `mut Suspend[T]` annotation already grants `mut`.
    Annotated,
}

pub(crate) struct OpenSub {
    pub mark: SubMark,
    pub depth: usize,
}

/// Which mismatch errors an `expect` reports: a binary operator operand
/// or range bound is checked symmetrically, so it reports mixed integer
/// signedness instead of a plain mismatch, and never implicit narrowing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ExpectPosition {
    /// A binary operator operand or range bound.
    Operand,
    /// Any other position.
    General,
}

pub(crate) struct Ck<'a, 'c> {
    pub cx: &'c BodyCx<'a>,
    /// Body-local append-only facts: failed trials truncate these before publication.
    pub comparisons: Vec<crate::comparison::Comparison>,
    pub statements: Vec<crate::comparison::Statement>,
    pub discarded: Vec<crate::comparison::DiscardedExpression>,
    /// The body's local pool: its types that hold inference variables.
    pub local: &'c hd_types::LocalPool,
    pub b: TirBuilder,
    pub infer: InferTable,
    pub scopes: Vec<HashMap<Symbol, LocalId>>,
    pub env: ParamEnv,
    /// Open loops: label and the type of their `break` values.
    pub loops: Vec<(LoopMark, Option<Ty>)>,
    pub diags: &'c mut DiagBuf,
    pub fuel: Fuel,
    pub memo: BodyMemo,
    /// The result type of the body or closure being checked.
    pub rets: Vec<Ty>,
    /// Opaque result inference is invariant; it never joins or inserts coercions.
    pub strict_rets: Vec<Ty>,
    pub opaque_rets: Vec<(Ty, Ty)>,
    pub opaque_annotation: Option<u32>,
    pub recursive_returns: Vec<Ty>,
    /// Type parameters by name: the impl's, then the function's.
    pub gens: Vec<(Symbol, Ty)>,
    /// The `using` binding being checked (`flow.using.access.v2`).
    pub using_bind: bool,
    /// The locals bound with `using`, which are not reassignable
    /// (`flow.using.binding.v2`).
    pub using_locals: Vec<LocalId>,
    /// The locals bound with `let mut`: a struct value in one changes in
    /// place (`types.value.writable-forms`).
    pub mut_locals: Vec<LocalId>,
    /// The locals whose collection a change reaches in place (a push, an
    /// element or map store), which a closure capturing them shares
    /// (`fn.capture.cell.writes`).
    pub changed_locals: Vec<LocalId>,
    /// The `mut` parameters (and `mut self`) whose value is the callee's own
    /// copy.
    pub mut_params: Vec<LocalId>,
    /// Checked mutations of those copies, before direct-statement grouping.
    pub param_writes: Vec<(LocalId, hd_base::NodeIdx)>,
    /// Neutral facts published to optional post-check analysis.
    pub parameter_writes: Vec<crate::comparison::ParameterWrite>,
    /// Source-only mutable-address and written binding facts requested by tooling.
    pub ref_taken: Vec<LocalId>,
    pub mut_bindings: Vec<(LocalId, hd_base::NodeIdx, Option<LetMutKind>)>,
    pub variant_patterns: Vec<crate::comparison::VariantPattern>,
    pub self_ty: Option<Ty>,
    /// The available keys: the body's row, then closures and `$.with`
    /// blocks, innermost last.
    pub rows: Vec<RowFrame>,
    /// Row parameters by name: the impl's, then the function's.
    pub row_gens: Vec<(Symbol, hd_types::RowParamRef)>,
    pub subs: Vec<OpenSub>,
    /// Goals on types not yet known at their use: bounds are solved at
    /// the end of the body, calls and projections also at the end of
    /// their statement.
    pub pending: Vec<PendingGoal>,
    /// Type parameters an argument of type `never` was passed for, with no
    /// earlier error to have left that `never`: one that nothing else
    /// solves is `never` at the end of the body.
    pub never_args: Vec<Ty>,
    /// The literal classes that took the `usize` default, by class
    /// (`InferTable::class_of`): the types `flow.panic.report.fallback`
    /// reports.
    pub fell_back: HashSet<u32>,
    /// A member call's explicit method type arguments (`x.f::[T]()`),
    /// taken by the method's instantiation.
    pub method_targs: Vec<Ty>,
    /// The receiver of the method call being checked is a trait value read
    /// through `&dyn` (`false`) or `&mut dyn` (`true`), written at the node
    /// (`trait.dyn.ref.member`).
    pub dyn_recv: Option<(bool, hd_base::NodeIdx)>,
    /// Inside an explicit type-argument list of an expression, where a
    /// named type may omit arguments the expression infers
    /// (`h.fact::[MaxLen]()`); a written type elsewhere infers nothing.
    pub in_explicit_targs: bool,
    /// The next call's default bodies: callee, type arguments and the
    /// values before the first argument (a receiver).
    pub default_owner: Option<(DefId, TyList, Vec<Ref>)>,
    /// The next call's defaults when the callee is a local function.
    pub local_defaults: Option<crate::local::LocalDefaults>,
    /// The parameter names and default closures of the local functions
    /// declared so far, by their name's local.
    pub local_forms: HashMap<LocalId, crate::local::LocalForm>,
    /// Whether the body or closure being checked suspends (a driver
    /// context for bang calls).
    pub suspends: Vec<bool>,
    /// Inside a `defer` suite: the loop depth at its start.
    pub defer_base: Option<usize>,
    /// The open closures at the start of that `defer` suite: a call in a
    /// closure declared inside it is not written directly in it.
    pub defer_subs: usize,
    /// The context a call written directly in this body is forbidden to
    /// suspend in: a default or fact expression, or a non-entry module's
    /// initialization (`req.drive.block-on.forbidden-contexts`).
    pub forbidden: Option<&'static str>,
    /// The `suspends` depth of the default expression being checked: a bang
    /// call there is `suspension-forbidden-context`, not a call outside any
    /// suspension (`fn.default.suspends`).
    pub default_depth: usize,
    /// While a default expression is checked against an empty row: whether
    /// its `requirement-in-default` is reported yet.
    pub default_row: Option<bool>,
    /// The `:=` bindings that a path to this point may have skipped
    /// (`names.definite.skipped`).
    pub uninit: hd_base::HashSet<LocalId>,
    /// The `:=` bindings declared so far, with their scope's depth.
    pub bound: Vec<(LocalId, usize)>,
    /// The statement-form `:=` binding being checked: the one `ExprStmt`
    /// child that may grant its closure a self-name
    /// (`fn.recursion.closure.binding`). A chain member is not it.
    pub stmt_binding: Option<hd_base::NodeIdx>,
    /// The statement being checked, innermost: where a fix-it that needs
    /// a statement of its own inserts it.
    pub cur_stmt: Option<hd_base::NodeIdx>,
    /// What the condition just checked leaves on its true and false paths.
    pub cond_flow: Option<crate::init::CondFlow>,
    /// The node of each literal constant, for range diagnostics, and
    /// whether a `-` negated it.
    pub lit_nodes: Vec<(Ref, hd_base::NodeIdx, bool)>,
    /// The constants of decimal literals larger than `u64::MAX`, clamped
    /// to the maximum where parsed: never in range
    /// (`types.literal.int-range`).
    pub overflow_lits: Vec<Ref>,
    /// Each `-` applied to a number whose type was still open, with its
    /// operand type: checked once the type is final
    /// (`expr.arith.unary-minus`).
    pub neg_nodes: Vec<(Ty, hd_base::NodeIdx)>,
    /// The literal node being checked as the argument of its literal
    /// function (`expr.literal-fn.ordinary-call`).
    pub lit_arg: Option<hd_base::NodeIdx>,
    /// Checking a module's top-level statements: the module path.
    pub module_init: Option<String>,
    /// The top-level statement being checked.
    pub init_stmt: usize,
    /// The simple bindings the module's top-level statements declare, by
    /// the first statement that does: a read of one from an earlier
    /// statement is a read before initialization.
    pub later_names: hd_base::HashMap<hd_base::Symbol, usize>,
    /// A REPL input that is one expression: that statement, then its
    /// value and type once checked (`cli.repl.value`).
    pub shown: Option<(hd_base::NodeIdx, Option<(Ref, Ty)>)>,
    /// What this body reads and calls (module initialization).
    pub facts: crate::init::InitFacts,
    /// The undo log of the body state a trial changes in place: local
    /// flags, init facts, the possibly-uninitialized set and the row
    /// frames (rule TC-5). Written only while a checkpoint is open.
    pub undo: Vec<crate::trial::BodyUndo>,
    /// Element references (`ref.elem.call`, `elem_refs.rs`): the
    /// expressions where one may stand, by what uses it there; the
    /// arguments that are one; and the loop bindings that are one
    /// (`flow.for.ref.binding`).
    pub elem_ok: HashMap<hd_base::NodeIdx, crate::elem_refs::ElemUse>,
    pub elem_args: HashSet<hd_base::NodeIdx>,
    pub elem_locals: HashSet<LocalId>,
    /// Local references (`local_refs.rs`): the `&` each binding inferred
    /// from another local takes its type from, the locals whose capture
    /// reported, and the types the body and its signature write.
    pub ref_origins: HashMap<LocalId, hd_base::TokenIdx>,
    pub ref_captured: HashSet<LocalId>,
    /// The initializer of each unannotated binding, where a `box(...)`
    /// fix-it boxes the value at its declaration (`req.ref.install-value.fix`).
    pub local_inits: HashMap<LocalId, hd_base::NodeIdx>,
    /// The values that reported `element-reference-outlives-call`, which
    /// report nothing more (`ref.elem.kept`).
    pub elem_kept: HashSet<hd_base::NodeIdx>,
    pub declared_tys: HashSet<Ty>,
    /// The exclusivity stack of the open calls and element reference
    /// loops, with each frame's start, and the conflicting accesses found
    /// so far by the collection's binding (`ref.elem.conflict`,
    /// `elem_refs.rs`), reported at the body's end.
    pub excl: Vec<crate::elem_refs::Excl>,
    pub excl_frames: Vec<usize>,
    pub conflicts: Vec<(LocalId, hd_base::NodeIdx)>,
    /// The checkpoints taken and not yet rolled back or committed.
    pub open_trials: u32,
    /// A pipe's value while its step is checked (`_`).
    pub placeholder: Option<(Ref, Ty)>,
    /// A bare pipe step's piped value, which stands for the argument node
    /// at this index (`expr.pipe.bare.call`).
    pub pipe_arg: Option<(hd_base::NodeIdx, (Ref, Ty))>,
    /// Checking the call a bare pipe step stands for.
    pub bare_step: bool,
    /// The next `for` loop's `else` suite and the loop's result type.
    pub loop_else: Option<(NodeRefIdx, Ty)>,
    /// The next `for` loop's comprehension result, a local and its type,
    /// which reserves room once before the loop when the source's length
    /// is known then.
    pub reserve_into: Option<(hd_base::LocalId, Ty)>,
    /// The function body block, for `missing-return-value`.
    pub fn_body: Option<NodeRefIdx>,
    /// Whether the function omits its result type: its result is
    /// inferred, never `void` by declaration, so a final value is not a
    /// `void-final` error even when checking runs with a `void` want.
    pub result_omitted: bool,
    /// The tail expression of the function body block, when it is an `if`
    /// without `else`: that `if` is `missing-return-value`, not a value
    /// `if` (`flow.if.value.else` reports every other value position).
    pub fn_body_tail: Option<hd_base::NodeIdx>,
    /// The function header's written requirement row, which a
    /// `missing-requirement` shows with its expansion
    /// (`req.row.alias.diagnostics.expanded`).
    pub written_row: Option<NodeRefIdx>,
    /// Declaring a `let` pattern's names: whether it has an annotation.
    pub let_view: Option<bool>,
    /// Set when `declare_pattern` rejected a pattern; its arm then takes
    /// no part in the reachability and exhaustiveness checks.
    pub pattern_rejected: bool,
    /// Source bindings for unread-suspension checks and optional tooling facts.
    pub user_locals: Vec<LocalId>,
    /// Whether the body read a name that a failed `use` poisoned, or a
    /// local whose declared type is poison (a header parameter's type in
    /// error): its poison types come from that error, not from a failed
    /// inference.
    pub read_poison_name: bool,
    /// The source spans of expressions checked against a poisoned expected
    /// type: the error that poisoned it is already reported, so the end of
    /// the body does not report their unsolved types again.
    pub absorbed: Vec<hd_base::Span>,
    /// Nested projection normalizations, against binding cycles.
    pub norm_depth: u32,
    /// Arguments of an instantiation choice inferred once, before its
    /// trials (`crates/hd_check/DESIGN.md` (type checking) §2.5 "Trial cost"): node, value, type.
    pub pre_args: Vec<(NodeRefIdx, Ref, Ty)>,
    /// Call parameters named only in bounds that call inference left
    /// unsolved (§2.4 step 9): variable, call, and whether a bound allows
    /// several instantiations. Decided at the end of the statement.
    pub open_params: Vec<(Ty, NodeRefIdx, bool)>,
    /// The type variables of function references (spec 07, Generic
    /// Function Values): variable, its default, and the reference.
    /// Decided at the end of the statement.
    pub ref_params: Vec<(Ty, Option<Ty>, NodeRefIdx)>,
    /// A call's type variables, closed at the end of the body, when the
    /// impls and loops that wait on them are settled.
    pub call_params: Vec<(Ty, NodeRefIdx)>,
    /// A trait-qualified associated call's `Self`, closed at the end of
    /// the body: inference from the arguments and the expected type may
    /// still determine it (`trait.assoc-call.trait.undetermined`).
    pub assoc_self: Vec<(Ty, NodeRefIdx)>,
    /// Checking a template's method at a derivation opt-in: the opt-in
    /// (`annot.template.checked`).
    pub opt_in: Option<&'c crate::derive::OptIn>,
    /// The structure protocol calls of an opt-in's template body: the
    /// `Structure` method and its walker, describer or source type.
    pub structure_calls: Vec<(DefId, Ty)>,
    /// Checking a tuple template's method for its plan: where its member
    /// steps go (`crates/hd_check/DESIGN.md` (trait solver) §3.9).
    pub plan_sink: Option<&'c std::cell::RefCell<Vec<hd_types::solver::MemberStep>>>,
    /// The least-common-type sites being joined (`types.lct.sites`), by
    /// join variable, with the values each has joined.
    pub joins: crate::join::Joins,
    /// The byte of the statement being checked: the local declarations
    /// whose extent holds it are in scope (`names.local-type.name`).
    pub at: u32,
    /// The local impls not in scope at `at` (`trait.impl.local.lookup`).
    pub hidden: Vec<DefId>,
    /// The module the body is written in: a member is visible from it
    /// when declared there or `pub` (`names.visible.field-method`).
    pub module: hd_base::PathId,
    /// Checking a test registration call in test position: the next call
    /// of a registration function registers test cases instead of being
    /// misplaced (`module.testing.direct-call`).
    pub registering: bool,
    /// Calls of open rows under a written row: each edge and that row,
    /// checked once the open rows are solved.
    pub row_waits: Vec<(crate::results::RowEdge, RowId)>,
    /// The generic local functions declared so far, by their name's local
    /// (`sub_flags::GENERIC`): a read of one instantiates it.
    pub local_generics: HashMap<LocalId, crate::local::LocalGeneric>,
    /// The owner of the body's type parameters, whose parameter numbers a
    /// generic local function's own parameters continue
    /// (`hd_mono::subst`).
    pub gen_owner: DefId,
    /// The next free parameter number of `gen_owner`.
    pub next_gen: u16,
    /// The generic local functions used from a closure inside their own
    /// body before their captures were known: each body is checked again
    /// with them.
    pub generic_recheck: Vec<LocalId>,
    /// Whether the body is a `by Structure` template's, which names no
    /// top-level binding (`module.package.template-private.no-binding`).
    pub in_template: Option<DefId>,
}

/// A node index kept for a later diagnostic.
pub(crate) type NodeRefIdx = hd_base::NodeIdx;

/// Which literals of a waiting goal take their defaults before it is
/// solved again at the end of the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Defaults {
    /// None: the statement's literal classes have their types already.
    None,
    /// The receiver's.
    Receiver,
    /// The trait arguments' too.
    All,
}

/// A goal that waited for inference, with the node it arose at.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PendingGoal {
    pub tref: TraitRef,
    pub at: NodeRefIdx,
    pub kind: Waiting,
}

/// What a waiting goal decides once its types are known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Waiting {
    /// A bound: whether it holds, retaining its diagnostic when deferred.
    Bound { code: Code },
    /// The impl of a call of the trait method `method`: what it learns
    /// fixes the trait's arguments, and the call's choice is settled from
    /// it.
    Call { method: DefId },
    /// The projection `assoc` of the goal's trait, which `var` stands
    /// for until then. A failure is the call's bound's to report.
    Project { assoc: DefId, var: Ty },
}

/// The checker of one body: the item whose parameters and bounds are in
/// scope (`env`), the TIR item and kind, its result and row.
pub(crate) fn new_ck<'a, 'c>(
    cx: &'c BodyCx<'a>,
    local: &'c hd_types::LocalPool,
    env: DefId,
    item: DefId,
    kind: BodyKind,
    (ret, row): (Ty, RowId),
    diags: &'c mut DiagBuf,
) -> Ck<'a, 'c> {
    let pool = cx.names.pool;
    let mut ck = Ck {
        cx,
        local,
        comparisons: Vec::new(),
        statements: Vec::new(),
        discarded: Vec::new(),
        b: TirBuilder::new(item, kind),
        infer: InferTable::default(),
        scopes: vec![HashMap::default()],
        env: ParamEnv::default(),
        loops: Vec::new(),
        diags,
        fuel: Fuel::new(cx.fuel),
        memo: BodyMemo::default(),
        rets: vec![ret],
        strict_rets: vec![],
        opaque_rets: vec![],
        opaque_annotation: None,
        recursive_returns: vec![],
        gens: Vec::new(),
        using_bind: false,
        using_locals: Vec::new(),
        mut_locals: Vec::new(),
        changed_locals: Vec::new(),
        mut_params: Vec::new(),
        param_writes: Vec::new(),
        parameter_writes: Vec::new(),
        ref_taken: Vec::new(),
        mut_bindings: Vec::new(),
        variant_patterns: Vec::new(),
        self_ty: None,
        rows: vec![
            if kind == BodyKind::Init && !pool.row_data(row).params.is_empty() {
                RowFrame::Any
            } else {
                RowFrame::Declared(row)
            },
        ],
        row_gens: Vec::new(),
        subs: Vec::new(),
        pending: Vec::new(),
        never_args: Vec::new(),
        fell_back: HashSet::default(),
        method_targs: Vec::new(),
        dyn_recv: None,
        in_explicit_targs: false,
        default_owner: None,
        local_defaults: None,
        local_forms: HashMap::default(),
        suspends: vec![false],
        defer_base: None,
        defer_subs: 0,
        uninit: hd_base::HashSet::default(),
        bound: Vec::new(),
        stmt_binding: None,
        cur_stmt: None,
        cond_flow: None,
        forbidden: (kind == BodyKind::Default).then_some("a default or fact expression"),
        default_depth: 1,
        default_row: None,
        lit_nodes: Vec::new(),
        overflow_lits: Vec::new(),
        neg_nodes: Vec::new(),
        read_poison_name: false,
        absorbed: Vec::new(),
        lit_arg: None,
        module_init: None,
        init_stmt: 0,
        later_names: hd_base::HashMap::default(),
        shown: None,
        facts: crate::init::InitFacts::default(),
        undo: Vec::new(),
        elem_ok: HashMap::default(),
        elem_args: HashSet::default(),
        elem_locals: HashSet::default(),
        ref_origins: HashMap::default(),
        ref_captured: HashSet::default(),
        local_inits: HashMap::default(),
        elem_kept: HashSet::default(),
        declared_tys: HashSet::default(),
        excl: Vec::new(),
        excl_frames: Vec::new(),
        conflicts: Vec::new(),
        open_trials: 0,
        placeholder: None,
        pipe_arg: None,
        bare_step: false,
        loop_else: None,
        reserve_into: None,
        fn_body: None,
        result_omitted: false,
        fn_body_tail: None,
        written_row: None,
        let_view: None,
        pattern_rejected: false,
        user_locals: Vec::new(),
        norm_depth: 0,
        pre_args: Vec::new(),
        open_params: Vec::new(),
        ref_params: Vec::new(),
        call_params: Vec::new(),
        assoc_self: Vec::new(),
        opt_in: None,
        structure_calls: Vec::new(),
        plan_sink: None,
        joins: crate::join::Joins::default(),
        at: 0,
        hidden: Vec::new(),
        module: cx.names.module_node(item),
        registering: false,
        row_waits: Vec::new(),
        local_generics: HashMap::default(),
        gen_owner: env,
        next_gen: 0,
        generic_recheck: Vec::new(),
        in_template: None,
    };
    // No local declaration holds byte 0: every local impl starts hidden.
    ck.move_to(0);
    let Some(it) = cx.lookup.item(env) else {
        return ck;
    };
    // The owner's parameters and bounds: an impl's, or a trait's `Self`.
    if let ItemData::Method { owner, .. } = &it.data
        && let Some(o) = cx.lookup.item(*owner)
    {
        match &o.data {
            ItemData::Impl {
                self_ty,
                kind,
                trait_,
                trait_args,
                ..
            } => {
                ck.self_ty = Some(*self_ty);
                // A `by Structure` template's target has the structure
                // protocol by construction, and implements the derived
                // trait through the instance (annotations, "Templates",
                // `annot.template.qualified-self`).
                if matches!(
                    kind,
                    hd_resolve::ImplKind::Template | hd_resolve::ImplKind::TupleTemplate
                ) {
                    ck.in_template = Some(*owner);
                    let st = cx.names.known.structure;
                    for (def, args) in [(st, TyList::EMPTY), (*trait_, *trait_args)] {
                        let tv = pool.intern_ty(&TyData::TraitValue {
                            def,
                            args,
                            bindings: vec![],
                        });
                        ck.add_bound(*self_ty, tv, 0);
                    }
                }
                ck.add_generics(*owner, 0, &o.generics);
            }
            ItemData::Trait(_) => {
                let p = pool.intern_ty(&TyData::Param(ParamRef {
                    owner: *owner,
                    index: 0,
                }));
                ck.self_ty = Some(p);
                // `Self` is bound by the trait over its own parameters, so
                // `Self::Assoc` of a generic trait is the projection a
                // call of a sibling method returns.
                let own: Vec<Ty> = o
                    .generics
                    .iter()
                    .enumerate()
                    .map(|(i, g)| {
                        let index = u16::try_from(i + 1).unwrap_or(u16::MAX);
                        if g.row {
                            pool.intern_ty(&TyData::Row(pool.row(&hd_types::RowData {
                                keys: Vec::new(),
                                params: vec![hd_types::RowParamRef {
                                    owner: *owner,
                                    index,
                                }],
                            })))
                        } else {
                            pool.intern_ty(&TyData::Param(ParamRef {
                                owner: *owner,
                                index,
                            }))
                        }
                    })
                    .collect();
                let tv = pool.intern_ty(&TyData::TraitValue {
                    def: *owner,
                    args: pool.list(&own),
                    bindings: vec![],
                });
                ck.add_bound(p, tv, 0);
                // The trait's own parameters, by name, with their bounds.
                ck.add_generics(*owner, 1, &o.generics);
            }
            _ => {}
        }
    }
    let gs = match it.sig() {
        Some(sig) => sig.generics.clone(),
        None => it.generics.clone(),
    };
    ck.add_generics(env, 0, &gs);
    ck.next_gen = u16::try_from(gs.len()).unwrap_or(u16::MAX);
    // The environment is frozen from here: its memo key, once per body (a
    // generic local function's bounds renew it).
    ck.env.key = Some(cx.global.env_key(&ck.env));
    ck
}

/// Whether trait `a` has `b` among its supertraits.
pub(crate) fn trait_extends(
    lookup: &Lookup<'_>,
    pool: hd_types::Types<'_>,
    a: DefId,
    b: DefId,
    depth: u32,
) -> bool {
    if depth > 16 {
        return false;
    }
    let Some(ItemData::Trait(t)) = lookup.item(a).map(|i| &i.data) else {
        return false;
    };
    t.supers.iter().any(|s| match pool.get(*s) {
        TyData::TraitValue { def, .. } => {
            def == b || trait_extends(lookup, pool, def, b, depth + 1)
        }
        _ => false,
    })
}

/// The arguments `from[from_args]` inherits for transitive supertrait
/// `to`: each step's supertrait parameters filled from the child's
/// arguments, as `fill_trait_args` fills them, but with no `Self` to
/// fill (`trait.dyn.widen`). `None` when `to` is no supertrait of `from`.
pub(crate) fn supertrait_args(
    lookup: &Lookup<'_>,
    pool: hd_types::Types<'_>,
    from: DefId,
    from_args: TyList,
    to: DefId,
    depth: u32,
) -> Option<TyList> {
    if from == to {
        return Some(from_args);
    }
    if depth > 16 {
        return None;
    }
    let Some(ItemData::Trait(t)) = lookup.item(from).map(|i| &i.data) else {
        return None;
    };
    let known = pool.list_items(from_args);
    for s in &t.supers {
        let TyData::TraitValue { def, args, .. } = pool.get(*s) else {
            continue;
        };
        let filled = pool.list(
            &pool
                .list_items(args)
                .iter()
                .map(|a| {
                    pool.subst(*a, &|p: ParamRef| {
                        if p.owner != from || p.index == 0 {
                            return None;
                        }
                        known.get(usize::from(p.index) - 1).copied()
                    })
                })
                .collect::<Vec<_>>(),
        );
        if def == to {
            return Some(filled);
        }
        if let Some(found) = supertrait_args(lookup, pool, def, filled, to, depth + 1) {
            return Some(found);
        }
    }
    None
}

/// Whether `key` may not be a requirement key: `Inspectable` itself, or a
/// trait with it among its supertraits (`req.key.inspectable`).
pub(crate) fn is_inspectable_key(
    lookup: &Lookup<'_>,
    pool: hd_types::Types<'_>,
    inspectable: DefId,
    key: Ty,
) -> bool {
    let TyData::TraitValue { def, .. } = pool.get(key) else {
        return false;
    };
    inspectable != DefId::NONE
        && (def == inspectable || trait_extends(lookup, pool, def, inspectable, 0))
}

/// Elaborates one declared bound into an environment (`crates/hd_check/DESIGN.md` (trait solver)
/// §2.3, §4.2): the clause, a defaulted trait argument left out filled
/// in, then its supertraits, depth first in declared order, each trait
/// reference once. Bodies and header checks build their environments
/// with it.
pub(crate) fn add_bound(
    lookup: &Lookup<'_>,
    global: &hd_types::InternPool,
    pool: hd_types::Types<'_>,
    env: &mut ParamEnv,
    (self_ty, bound): (Ty, Ty),
    depth: u32,
) {
    // A bound that names no trait, already reported, is a clause on no
    // trait: the parameter satisfies every trait (`implements`).
    if bound == Ty::POISON {
        if !env
            .clause_self
            .iter()
            .zip(&env.clause_trait)
            .any(|(s, t)| *s == self_ty && *t == DefId::NONE)
        {
            env.clause_self.push(self_ty);
            env.clause_trait.push(DefId::NONE);
            env.clause_args.push(hd_types::TyList::EMPTY);
            env.clause_bindings.push(Vec::new());
            env.clause_mut.push(false);
            env.clause_origin
                .push(u16::try_from(env.clause_origin.len()).unwrap_or(u16::MAX));
        }
        return;
    }
    let TyData::TraitValue {
        def,
        args,
        bindings,
    } = pool.get(bound)
    else {
        return;
    };
    // Headers carry their defaults filled; a bound the checker forms
    // itself is filled the same way, so the clause matches its goals.
    let args = match lookup.item(def) {
        Some(it) => hd_resolve::fill_trait_args(global, def, &it.generics, args, Some(self_ty)),
        None => args,
    };
    if depth > 16
        || (0..env.clause_self.len()).any(|i| {
            env.clause_self[i] == self_ty
                && env.clause_trait[i] == def
                && env.clause_args[i] == args
        })
    {
        return;
    }
    env.clause_self.push(self_ty);
    env.clause_trait.push(def);
    env.clause_args.push(args);
    env.clause_bindings.push(bindings);
    env.clause_mut.push(false);
    env.clause_origin
        .push(u16::try_from(env.clause_origin.len()).unwrap_or(u16::MAX));
    if let Some(ItemData::Trait(t)) = lookup.item(def).map(|i| &i.data) {
        let known = pool.list_items(args);
        for s in &t.supers {
            let s = pool.subst(*s, &|p: ParamRef| {
                if p.owner != def {
                    return None;
                }
                if p.index == 0 {
                    Some(self_ty)
                } else {
                    known.get(p.index as usize - 1).copied()
                }
            });
            add_bound(lookup, global, pool, env, (self_ty, s), depth + 1);
        }
        // Each associated type's declared bounds hold for its projection
        // (`trait.assoc.bound-use`): under `I < Iterable[T]`, the clause
        // `I::IntoIter < Iterator[T]`.
        for &(_, a) in &t.assoc {
            let Some(ItemData::AssocType { bounds, .. }) = lookup.item(a).map(|i| &i.data) else {
                continue;
            };
            if bounds.is_empty() {
                continue;
            }
            let proj = pool.intern_ty(&TyData::Assoc {
                assoc: a,
                trait_: def,
                self_ty,
                args,
            });
            for b in bounds {
                let b = pool.subst(*b, &|p: ParamRef| {
                    if p.owner != def {
                        return None;
                    }
                    if p.index == 0 {
                        Some(self_ty)
                    } else {
                        known.get(p.index as usize - 1).copied()
                    }
                });
                add_bound(lookup, global, pool, env, (proj, b), depth + 1);
            }
        }
    }
}

/// Checks one function body and returns its TIR.
pub fn check_fn(
    cx: &BodyCx<'_>,
    def: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Body> {
    check_fn_in(cx, def, node, diags, None)
}

/// Checks another configured body against the result type and row inferred
/// for its pair's canonical body. Written parts pass `None` and continue to
/// come from the shared interface signature.
pub fn check_fn_with_inferred(
    cx: &BodyCx<'_>,
    def: DefId,
    node: NodeRef<'_>,
    inferred_ret: Option<Ty>,
    inferred_row: Option<RowId>,
    diags: &mut DiagBuf,
) -> StageResult<Body> {
    check_fn_body_as(
        cx,
        def,
        node,
        diags,
        (None, None),
        Infer::default(),
        Some((inferred_ret, inferred_row)),
    )
    .map(|(b, ..)| b)
}

/// Checks one function body, a template's method at a derivation opt-in
/// when `opt_in` is given: the opt-in's parameters and their bounds join
/// the environment, and the members' obligations are checked at the end.
pub(crate) fn check_fn_in(
    cx: &BodyCx<'_>,
    def: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
    opt_in: Option<&crate::derive::OptIn>,
) -> StageResult<Body> {
    check_fn_body(cx, def, node, diags, (opt_in, None), Infer::default()).map(|(b, ..)| b)
}

/// Checks a tuple template's method for the template's plan: the member
/// steps of its `walk`, `describe` and `build` calls go to `sink`
/// (`crates/hd_check/DESIGN.md` (trait solver) §3.9). Diagnostics go to `diags`.
pub fn check_tuple_template_method(
    cx: &BodyCx<'_>,
    def: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
    sink: &std::cell::RefCell<Vec<hd_types::solver::MemberStep>>,
) -> StageResult<()> {
    check_fn_body(cx, def, node, diags, (None, Some(sink)), Infer::default()).map(|_| ())
}

/// What a body checks for its declaration: an omitted result type, an
/// omitted row.
#[derive(Clone, Copy, Default)]
pub(crate) struct Infer {
    pub ret: bool,
    pub row: bool,
}

/// Checks one function body and returns its TIR, its result type and its
/// row facts. With `infer.ret`, the written result is omitted: the result
/// is a join variable, so the final value and every `return` operand take
/// their least common type (`fn.decl.result-inferred.common`). With
/// `infer.row`, the row is omitted: the body's bottom frame takes every
/// key it uses (`req.row.omitted.inferred-private`). A body that runs out
/// of fuel is poisoned, its result too (`too_complex`).
pub(crate) fn check_fn_body(
    cx: &BodyCx<'_>,
    def: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
    hooks: (
        Option<&crate::derive::OptIn>,
        Option<&std::cell::RefCell<Vec<hd_types::solver::MemberStep>>>,
    ),
    infer: Infer,
) -> StageResult<(Body, Ty, crate::results::RowFacts)> {
    check_fn_body_as(cx, def, node, diags, hooks, infer, None)
}

fn check_fn_body_as(
    cx: &BodyCx<'_>,
    def: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
    hooks: (
        Option<&crate::derive::OptIn>,
        Option<&std::cell::RefCell<Vec<hd_types::solver::MemberStep>>>,
    ),
    infer: Infer,
    inferred: Option<(Option<Ty>, Option<RowId>)>,
) -> StageResult<(Body, Ty, crate::results::RowFacts)> {
    match fn_body_steps(cx, def, node, diags, hooks, infer, inferred) {
        Err(e) if is_out_of_fuel(&e) => {
            let body = too_complex(cx, (def, BodyKind::Fn), node, diags)?;
            Ok((body, Ty::POISON, crate::results::RowFacts::default()))
        }
        r => r,
    }
}

fn fn_body_steps(
    cx: &BodyCx<'_>,
    def: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
    (opt_in, plan_sink): (
        Option<&crate::derive::OptIn>,
        Option<&std::cell::RefCell<Vec<hd_types::solver::MemberStep>>>,
    ),
    infer: Infer,
    inferred: Option<(Option<Ty>, Option<RowId>)>,
) -> StageResult<(Body, Ty, crate::results::RowFacts)> {
    let Some(item) = cx.lookup.item(def) else {
        return unsupported(format!("body of {} has no header", cx.names.path(def)));
    };
    let Some(mut sig) = item.sig().cloned() else {
        return unsupported("a body of a non-function item");
    };
    if let Some((ret, row)) = inferred {
        if let Some(ret) = ret {
            sig.ret = ret;
        }
        if let Some(row) = row {
            sig.row = row;
        }
    }
    let local = hd_types::LocalPool::new();
    let mut ck = new_ck(
        cx,
        &local,
        def,
        def,
        BodyKind::Fn,
        (sig.ret, sig.row),
        diags,
    );
    if let Some(o) = opt_in {
        ck.enter_opt_in(o);
    }
    ck.plan_sink = plan_sink;
    if infer.row {
        ck.rows[0] = RowFrame::Inferred {
            used: hd_types::RowData::default(),
            edges: Vec::new(),
        };
    }
    // A local impl's method, or a local trait's default, sees what is in
    // scope where it is written (`trait.impl.local.lookup`).
    ck.move_to(cx.src.span(node).lo);
    ck.check_impl_method(def, node)?;
    ck.check_literal_marker(def, node);
    ck.check_row_patterns(&sig, node);
    ck.suspends = vec![sig.suspends];
    let blk = ck.b.open_block();
    // Projections in the signature normalize under the bounds, so
    // `I::Item` and `T` are one type under `I < Supplier[Item = T]`
    // (`trait.binding.interchangeable`).
    ck.declared_tys.insert(sig.ret);
    for (name, ty) in sig.params.clone() {
        let ty = ck.norm_ty(ty);
        ck.declared_tys.insert(ty);
        let l = ck.b.local(ty, name, local_flags::PARAM, node.index());
        ck.scopes[0].insert(name, l);
    }
    ck.mark_mut_params(node);
    let Some(body) = Src::child(node, SyntaxKind::Block) else {
        return unsupported("a function without a body");
    };
    ck.fn_body = Some(body.index());
    ck.result_omitted = Src::type_child(node).is_none();
    ck.written_row = Src::child(node, SyntaxKind::RequirementRow).map(NodeRef::index);
    // `types.lct.sites`: an omitted result type joins the final value and
    // the `return` operands, and widens in place when an optional one
    // follows plain ones (`join_site`).
    let (tail, ret) = if ck.pool().has_opaque(sig.ret) {
        let before = ck.opaque_rets.len();
        let ret = ck.open_opaque_return(sig.ret);
        let occurrences = before..ck.opaque_rets.len();
        ck.rets[0] = ret;
        let (tail, _) = ck.block_value(body, Some(ret))?;
        ck.check_opaque_occurrences(occurrences, node)?;
        (tail, ret)
    } else {
        let want = (!infer.ret).then(|| ck.norm_ty(sig.ret));
        ck.join_site(want, |ck, ret| {
            ck.rets[0] = ret;
            let (tail, _) = ck.block_value(body, Some(ret))?;
            Ok((tail, ret))
        })?
    };
    let root = ck.b.close_block(blk, tail, ret, body.index());
    ck.finish_parameter_writes(body);
    ck.finish_with(root, ret, Some(node))
}

/// The `DefId` of a default body: the declaration's path plus the field or
/// parameter name (`crates/hd_tir/DESIGN.md` "Default calls").
#[must_use]
pub fn default_body_def(names: &Names<'_>, owner: DefId, name: &str) -> DefId {
    names.member(owner, hd_intern::PathKind::Hidden, name)
}

/// Checks one default expression as a body of kind `Default`
/// (`crates/hd_check/DESIGN.md` (type checking) §1.7): a data field's (no parameters) or a function
/// parameter's (the earlier parameters are its parameters). A default
/// that runs out of fuel is poisoned (`too_complex`).
pub fn check_default(
    cx: &BodyCx<'_>,
    owner: DefId,
    name: &str,
    expr: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Body> {
    match default_steps(cx, owner, name, expr, diags) {
        Err(e) if is_out_of_fuel(&e) => {
            let def = default_body_def(&cx.names, owner, name);
            too_complex(cx, (def, BodyKind::Default), expr, diags)
        }
        r => r,
    }
}

fn default_steps(
    cx: &BodyCx<'_>,
    owner: DefId,
    name: &str,
    expr: NodeRef<'_>,
    diags: &mut DiagBuf,
) -> StageResult<Body> {
    let Some(item) = cx.lookup.item(owner) else {
        return unsupported("a default of an item outside the module");
    };
    let sym = cx.names.syms.intern(name);
    let (ty, params) = match (&item.data, item.sig()) {
        (ItemData::Data(fields), _) => match fields.iter().find(|f| f.name == sym) {
            Some(f) => (f.ty, vec![]),
            None => return unsupported("a default of an unknown field"),
        },
        (_, Some(sig)) => match sig.params.iter().position(|p| p.0 == sym) {
            Some(i) => (sig.params[i].1, sig.params[..i].to_vec()),
            None => return unsupported("a default of an unknown parameter"),
        },
        _ => return unsupported("a default of this item kind"),
    };
    let def = default_body_def(&cx.names, owner, name);
    let local = hd_types::LocalPool::new();
    let mut ck = new_ck(
        cx,
        &local,
        owner,
        def,
        BodyKind::Default,
        (ty, RowId::EMPTY),
        diags,
    );
    // A default is looked up where it is written (`trait.impl.local.lookup`).
    ck.move_to(cx.src.span(expr).lo);
    ck.default_row = Some(false);
    let blk = ck.b.open_block();
    for (n, t) in params {
        let l = ck.b.local(t, n, local_flags::PARAM, expr.index());
        ck.scopes[0].insert(n, l);
    }
    // The parameter itself and the later ones are named but not visible
    // (`fn.default.later-parameter`).
    if let (_, Some(sig)) = (&item.data, item.sig())
        && let Some(i) = sig.params.iter().position(|p| p.0 == sym)
    {
        for &(n, t) in &sig.params[i..] {
            let l = ck.b.local(t, n, local_flags::NOT_YET, expr.index());
            ck.scopes[0].insert(n, l);
        }
    }
    let (r, t) = ck.expr(expr, Some(ty))?;
    let r = ck.coerce(r, t, ty, expr, "default", ExpectPosition::General);
    let root = ck.b.close_block(blk, Some(r), ty, expr.index());
    ck.finish(root)
}

impl<'c> Ck<'_, 'c> {
    /// The types this body sees: the run's pool and the body's local pool.
    pub(crate) fn pool(&self) -> hd_types::Types<'c> {
        hd_types::Types::with_local(self.cx.names.pool, self.local)
    }
}

impl Ck<'_, '_> {
    pub(crate) fn charge(&mut self) -> StageResult<()> {
        if self.fuel.charge(1) {
            Ok(())
        } else {
            out_of_fuel()
        }
    }

    /// A construct not carried yet, with where it is (after ` @`).
    pub(crate) fn gap<T>(&self, n: NodeRef<'_>, what: &str) -> StageResult<T> {
        let span = self.cx.src.span(n);
        unsupported(format!("{what} @{}", span.lo))
    }

    pub(crate) fn err(&mut self, code: Code, n: NodeRef<'_>, msg: &str) {
        let span = self.cx.src.span(n);
        self.diags.error(code, span, msg);
    }

    pub(crate) fn show(&self, t: Ty) -> String {
        let t = self.infer.resolve(self.pool(), t);
        match self.pool().get(t) {
            TyData::Infer(_) => match self.infer.kind_of(self.pool(), t) {
                Some(VarKind::IntLit | VarKind::SignedIntLit) => "an integer literal".into(),
                Some(VarKind::FloatLit) => "a floating-point literal".into(),
                _ => "{unknown}".into(),
            },
            _ => hd_resolve::show_ty_in(&self.cx.names, self.pool(), t),
        }
    }

    /// The expected and the found type as a mismatch shows them. Two types
    /// that differ only in a function type's row would read the same, so
    /// they show their rows.
    pub(crate) fn show_pair(&self, want: Ty, got: Ty) -> (String, String) {
        let (w, g) = (self.show(want), self.show(got));
        let pool = self.pool();
        let with_rows = w == g;
        let (w, g) = if with_rows {
            let rows =
                |t| hd_resolve::show_ty_rows_in(&self.cx.names, pool, self.infer.resolve(pool, t));
            (rows(want), rows(got))
        } else {
            (w, g)
        };
        if w.trim_start_matches("mut ") == g.trim_start_matches("mut ") {
            let qualified = |t| {
                hd_resolve::iface::show_ty_qualified_in(
                    &self.cx.names,
                    pool,
                    self.infer.resolve(pool, t),
                    with_rows,
                )
            };
            let (qw, qg) = (qualified(want), qualified(got));
            // A view of the same item still has the same qualified name.
            if qw.trim_start_matches("mut ") != qg.trim_start_matches("mut ") {
                return (qw, qg);
            }
        }
        (w, g)
    }

    /// Unread suspensions are errors even for underscore-prefixed names.
    /// Tooling may request neutral binding/read facts; advice lives outside checking.
    fn finish_bindings(&mut self) -> StageResult<Vec<crate::comparison::Binding>> {
        let mut bindings = Vec::new();
        let mut mutability = std::mem::take(&mut self.mut_bindings);
        mutability.sort_stable_by(|a, b| a.0.raw().cmp(&b.0.raw()));
        for l in std::mem::take(&mut self.user_locals) {
            let body = self.b.body_mut();
            let (item, flags, name, at) = (
                body.item,
                body.local_flags[l.idx()],
                body.local_name[l.idx()],
                body.local_syn[l.idx()],
            );
            let read = flags & local_flags::READ != 0;
            let lt = self.b.local_ty(l);
            // flow.unused.must-use: a readonly binding does not lift the obligation.
            if !read && self.must_use_in(lt, true) == Some(true) {
                let text = self.cx.names.text(name);
                let span = self.cx.src.span(self.cx.src.parse.tree.node(at));
                let msg = format!(
                    "`{text}` binds a {} value that is never read; handle it or write `_ := ...`",
                    self.show(lt)
                );
                self.diags.push(
                    Code::DiscardedMustUseValue,
                    hd_diag::Severity::Error,
                    span,
                    &msg,
                    None,
                );
            }
            if self.cx.source_facts.is_some() {
                let unbound_disposable =
                    !self.using_locals.contains(&l) && self.implements_disposal_protocol(lt)?;
                bindings.push(crate::comparison::Binding {
                    body: item,
                    local: l,
                    name: self.cx.names.text(name).to_owned(),
                    span: self.cx.src.span(self.cx.src.parse.tree.node(at)),
                    read,
                    unbound_disposable,
                    mutability: mutability
                        .binary_search_by_key(&l.raw(), |(local, ..)| local.raw())
                        .ok()
                        .map(|i| {
                            let (_, pattern, kind) = mutability[i];
                            self.binding_mutability(l, pattern, kind)
                        }),
                });
            }
        }
        Ok(bindings)
    }

    /// `expr.arith.unary-minus`: `-` on an operand whose type turned out
    /// unsigned once inference finished is a `type-mismatch`.
    fn check_negated_unsigned(&mut self) {
        for (t, at) in std::mem::take(&mut self.neg_nodes) {
            let t = self.strip_mut(t);
            let pool = self.pool();
            if let TyData::Prim(p) = pool.get(t)
                && p.is_unsigned()
            {
                let node = self.cx.src.parse.tree.node(at);
                let msg = format!("unary `-` does not apply to {}", p.name());
                self.err(Code::TypeMismatch, node, &msg);
            }
        }
    }

    /// Literal constants fit their final type; an expected `f32` rounds
    /// directly from its decimal source (`types.literal.float-expected`).
    fn check_literal_ranges(&mut self) {
        use hd_types::Prim;
        let pool = self.pool();
        for (r, at, negated) in std::mem::take(&mut self.lit_nodes) {
            let Some((t, bits)) = self.b.const_of(r) else {
                continue;
            };
            let TyData::Prim(p) = pool.get(t) else {
                continue;
            };
            let node = self.cx.src.parse.tree.node(at);
            if p.is_float() {
                // `types.literal.float-finite`: the decimal text converts
                // directly to the final width and must stay finite.
                let finite = if p == Prim::F32 {
                    let span = self.cx.src.span(node);
                    let source = self.cx.src.text;
                    let lo = usize::try_from(span.lo).unwrap_or(0).min(source.len());
                    let hi = usize::try_from(span.hi)
                        .unwrap_or(0)
                        .clamp(lo, source.len());
                    float_text_f32(source.get(lo..hi).unwrap_or("")).map_or_else(
                        || f64::from_bits(bits).abs() <= f64::from(f32::MAX),
                        |value| {
                            // Keep the TIR's f64-bit convention, widening
                            // the already rounded f32 exactly. Emission's
                            // demotion then introduces no second rounding.
                            // The bits retain the effective sign through
                            // nested negations and negative zero.
                            let value = f64::from(value).copysign(f64::from_bits(bits));
                            self.b.body_mut().consts[(r.0 & !Ref::CONST_BIT) as usize].1 =
                                value.to_bits();
                            value.is_finite()
                        },
                    )
                } else {
                    f64::from_bits(bits).is_finite()
                };
                if !finite {
                    let msg = format!("floating-point literal is out of range for {}", p.name());
                    self.err(Code::FloatLiteralRange, node, &msg);
                }
                continue;
            }
            // A literal under `-` was negated by the checker: `bits`
            // holds the negated value (`types.literal.negation`).
            let v = bits.cast_signed();
            if negated && p.is_unsigned() {
                let msg = format!("a negated literal does not fit unsigned {}", p.name());
                // A range pattern's bound is outside the subject's type
                // (`flow.match.range.bound-type`).
                let code = if node.kind() == SyntaxKind::RangePattern {
                    Code::IntegerLiteralRange
                } else {
                    Code::TypeMismatch
                };
                self.err(code, node, &msg);
                continue;
            }
            // A decimal integer larger than u64::MAX was clamped to the
            // maximum where parsed, so it is out of range whatever its
            // type resolved to (`types.literal.int-range`).
            if !self.overflow_lits.is_empty() && self.overflow_lits.contains(&r) {
                let msg = format!("integer literal out of range for {}", p.name());
                self.err(Code::IntegerLiteralRange, node, &msg);
                continue;
            }
            let ok = match p {
                Prim::I8 => i8::try_from(v).is_ok(),
                Prim::I16 => i16::try_from(v).is_ok(),
                Prim::I32 => i32::try_from(v).is_ok(),
                // A negated `i64` literal keeps its sign; a plain one
                // above `i64::MAX` wrapped into the negative range.
                Prim::I64 => {
                    if negated {
                        v <= 0
                    } else {
                        v >= 0
                    }
                }
                Prim::U8 => u8::try_from(bits).is_ok(),
                Prim::U16 => u16::try_from(bits).is_ok(),
                Prim::U32 | Prim::Usize => u32::try_from(bits).is_ok(),
                _ => true,
            };
            if !ok {
                let msg = format!("literal {v} does not fit {}", p.name());
                self.err(Code::IntegerLiteralRange, node, &msg);
            }
        }
    }

    /// Declares an item's type parameters with their bounds.
    /// `offset` is the index of the first parameter: a trait's own
    /// parameters follow its `Self`.
    fn add_generics(&mut self, owner: DefId, offset: usize, gs: &[hd_resolve::Generic]) {
        let pool = self.pool();
        for (i, g) in gs.iter().enumerate() {
            let index = u16::try_from(offset + i).unwrap_or(u16::MAX);
            let p = pool.intern_ty(&TyData::Param(ParamRef { owner, index }));
            self.gens.push((g.name, p));
            if g.row {
                self.row_gens
                    .push((g.name, hd_types::RowParamRef { owner, index }));
            }
            for b in &g.bounds {
                self.add_bound(p, *b, 0);
            }
        }
    }

    pub(crate) fn check_opaque_result(
        &mut self,
        ret: Ty,
        bound: Ty,
        n: NodeRef<'_>,
    ) -> StageResult<()> {
        let pool = self.pool();
        if matches!(pool.get(self.infer.resolve(pool, ret)), TyData::Infer(_))
            && self.infer.kind_of(pool, ret) == Some(VarKind::General)
        {
            if self.recursive_returns.contains(&ret) {
                self.err(
                    Code::RecursiveValueType,
                    n,
                    "the recursive hidden result has no concrete type",
                );
                return Ok(());
            }
            let _ = self.infer.unify(pool, ret, Ty::NEVER);
        }
        if let TyData::TraitValue {
            def,
            args,
            bindings,
        } = self.pool().get(bound)
        {
            let args = self.cx.lookup.item(def).map_or(args, |it| {
                hd_resolve::fill_trait_args(self.cx.names.pool, def, &it.generics, args, Some(ret))
            });
            let bound = pool.intern_ty(&TyData::TraitValue {
                def,
                args,
                bindings,
            });
            self.require_ref(
                hd_types::solver::TraitRef {
                    trait_: def,
                    self_ty: ret,
                    args,
                },
                n,
            )?;
            self.check_bindings(ret, bound, n)?;
        }
        Ok(())
    }

    /// Adds a bound and, transitively, its supertraits to the environment.
    pub(crate) fn add_bound(&mut self, self_ty: Ty, bound: Ty, depth: u32) {
        add_bound(
            self.cx.lookup,
            self.cx.names.pool,
            self.pool(),
            &mut self.env,
            (self_ty, bound),
            depth,
        );
    }

    /// A local by name, innermost first, with its scope depth.
    pub(crate) fn find_local(&self, s: Symbol) -> Option<(LocalId, usize)> {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(d, m)| m.get(&s).map(|l| (*l, d)))
    }

    /// Reads a local, capturing it into every open closure it crosses.
    pub(crate) fn read_local(&mut self, l: LocalId, depth: usize, n: NodeRef<'_>) -> (Ref, Ty) {
        // A generic local function is instantiated at each use.
        if self.local_generics.contains_key(&l) {
            return self.local_generic_value(l, &[], n);
        }
        if !self.note_read(l, depth, n) {
            return self.poison_value(n);
        }
        let t = self.b.local_ty(l);
        if self.pool().has_poison(t) {
            self.read_poison_name = true;
        }
        (self.b.get(l, t, n.index()), t)
    }

    /// The checks and marks of a read of local `l`, declared at scope
    /// depth `depth`, without its value: a reference to a place the local
    /// starts reads it this way (`refs.rs`, `make_ref`). The local is
    /// captured into every open closure it crosses. `false` when the read
    /// is an error.
    pub(crate) fn note_read(&mut self, l: LocalId, depth: usize, n: NodeRef<'_>) -> bool {
        // `fn.default.later-parameter`: a default names a parameter at or
        // after its own.
        if self.b.body_mut().local_flags[l.idx()] & local_flags::NOT_YET != 0 {
            let name = self.cx.names.text(self.b.body_mut().local_name[l.idx()]);
            let msg =
                format!("`{name}` is a later parameter, so it is not visible in this default");
            self.err(Code::BindingNotYetVisible, n, &msg);
            return false;
        }
        // `names.definite.reject`: reported once per binding.
        if self.uninit_remove(l) {
            self.err(
                Code::PossiblyUninitializedBinding,
                n,
                "this binding may not have been initialized on every path to here",
            );
        }
        let mut captured = false;
        for s in &self.subs {
            if s.depth > depth {
                self.b.capture(s.mark, l);
                captured = true;
            }
        }
        self.add_flags(l, local_flags::READ);
        if captured {
            self.add_flags(l, local_flags::CAPTURED);
        }
        true
    }

    /// Records an assignment to a local, from inside a closure or not.
    pub(crate) fn note_write(&mut self, l: LocalId, depth: usize) {
        let mut captured = false;
        for s in &self.subs {
            if s.depth > depth {
                self.b.capture(s.mark, l);
                captured = true;
            }
        }
        let flags = self.b.body_mut().local_flags[l.idx()];
        if captured {
            self.add_flags(l, local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED);
        } else if flags & local_flags::ASSIGNED != 0 {
            self.add_flags(l, local_flags::MUTATED);
        }
        self.add_flags(l, local_flags::ASSIGNED);
    }

    /// Records a checked mutation of a function's own mutable parameter copy.
    pub(crate) fn note_param_write(&mut self, l: LocalId, at: hd_base::NodeIdx) {
        if self.cx.source_facts.is_some() && self.mut_params.contains(&l) {
            self.param_writes.push((l, at));
        }
    }

    pub(crate) fn sym_of(&self, n: NodeRef<'_>) -> Symbol {
        self.cx.src.symbol(self.cx.src.first(n), self.cx.names.syms)
    }

    /// `names.type-param.no-redeclare`: a local value must not reuse an
    /// enclosing type parameter's name.
    pub(crate) fn check_local_name(&mut self, name: Symbol, at: NodeRef<'_>) {
        if self.gens.iter().any(|(g, _)| *g == name) {
            let msg = format!(
                "`{}` is a type parameter in scope",
                self.cx.names.text(name)
            );
            self.err(Code::DuplicateBinding, at, &msg);
        }
    }

    /// Whether the code being checked lies in a `tests:` block.
    pub(crate) fn in_tests_block(&self) -> bool {
        self.cx
            .tests
            .as_ref()
            .is_some_and(|t| t.blocks.iter().any(|b| b.lo <= self.at && self.at < b.hi))
    }

    /// Whether the module being checked is an integration test module:
    /// test code under the test root (`module.test.integration`,
    /// `module.testing.kind-decides`).
    pub(crate) fn in_integration_test(&self) -> bool {
        hd_project::role_of(self.cx.file_name) == hd_project::Role::Test
            && matches!(
                hd_project::Root::of(self.cx.file_name).0,
                hd_project::Root::Test
            )
    }

    /// `module.prelude.no-shadow`: a local value, closure parameter or
    /// top-level binding does not bind a prelude name. The binding still
    /// introduces the name afterwards.
    pub(crate) fn check_prelude_shadow(&mut self, name: Symbol, at: NodeRef<'_>) {
        let text = self.cx.names.text(name);
        // Test prelude names are bound in `tests:` blocks and in whole
        // test modules and integration test files alike.
        let in_test_code = self.in_tests_block()
            || hd_project::role_of(self.cx.file_name) == hd_project::Role::Test;
        let bound = (hd_resolve::is_prelude_name(text)
            && self.cx.scope_at(self.at).bound_from_outside(name))
            || (in_test_code && hd_resolve::is_test_prelude_name(text));
        if bound {
            let msg = format!("`{text}` shadows a prelude name");
            self.err(Code::PreludeNameShadow, at, &msg);
        }
    }

    pub(crate) fn bind_local(&mut self, name: Symbol, t: Ty, at: NodeRef<'_>) -> LocalId {
        self.check_local_name(name, at);
        self.check_prelude_shadow(name, at);
        let l = self.b.local(t, name, local_flags::ASSIGNED, at.index());
        self.scopes.last_mut().expect("scope").insert(name, l);
        self.user_locals.push(l);
        l
    }

    /// How many optional layers `t` has: `0` for an `i32`, `2` for an
    /// `i32??`. A layer still an unsolved variable ends the count.
    pub(crate) fn option_layers(&self, mut t: Ty) -> usize {
        let pool = self.pool();
        let mut n = 0;
        while let TyData::Option(inner) = pool.get(self.infer.shallow(pool, t)) {
            n += 1;
            t = inner;
        }
        n
    }

    /// Whether a value of type `got` takes one optional injection into the
    /// optional `want`: `got` has exactly one optional layer fewer, so an
    /// `i32` goes into an `i32?` and an `i32?` into an `i32??`
    /// (`types.assign.optional`, `types.option.nest.accept`). An `i32` is
    /// no injection into an `i32??`, and a value with as many layers as
    /// `want` must match it.
    pub(crate) fn injects_into(&self, got: Ty, want: Ty) -> bool {
        self.option_layers(got) + 1 == self.option_layers(want)
    }

    /// Unifies `got` with `want`, inserting the coercions of
    /// `crates/hd_check/DESIGN.md` (type checking) §4.2 (`never`, `.Some` wrapping, trait values).
    /// `position` flows to `expect`, selecting its error codes.
    /// Stores `v` in local `l`. A `mut T` value stored in a readonly
    /// local of type `T` passes through `Weaken`, so the stored value has
    /// the local's type (`crates/hd_tir/DESIGN.md` §4.13.11, invariant 4).
    pub(crate) fn set_local(&mut self, l: LocalId, v: Ref, syn: hd_base::NodeIdx) {
        let pool = self.pool();
        let lt = self.b.local_ty(l);
        let weakens = v.0 != NONE
            && matches!(
                pool.get(self.infer.resolve(pool, self.b.ty_of(v))),
                TyData::Mut(_)
            )
            && !matches!(
                pool.get(self.infer.resolve(pool, lt)),
                TyData::Mut(_) | TyData::Infer(_) | TyData::Poison
            );
        let v = if weakens {
            self.b.coerce(Coercion::Weaken, NONE, v, lt, syn)
        } else {
            v
        };
        self.b.set(l, v, syn);
    }

    pub(crate) fn coerce(
        &mut self,
        r: Ref,
        got: Ty,
        want: Ty,
        n: NodeRef<'_>,
        what: &str,
        position: ExpectPosition,
    ) -> Ref {
        let pool = self.pool();
        let opaque_return = self.strict_opaque_return(want);
        let got = if opaque_return {
            self.recursive_opaque_return(got)
        } else {
            got
        };
        let target = want;
        let (got, want) = (self.norm_ty(got), self.norm_ty(want));
        let g = self.infer.shallow(pool, got);
        let w = self.infer.shallow(pool, want);
        // `types.never.assignable`: `never` goes into every type, and no
        // ordinary value goes into `never`.
        if g == Ty::NEVER || g == w {
            return r;
        }
        if self.strict_opaque_slot(target) {
            // A call of this same declaration returns its chosen hidden
            // type. It constrains neither which type that is nor its size.
            if self
                .opaque_rets
                .iter()
                .any(|&(opaque, result)| opaque == got && result == target)
            {
                self.recursive_returns.push(target);
                return r;
            }
            let snap = self.infer.snapshot();
            if self.infer.unify(pool, got, want).is_err() {
                self.infer.rollback(snap);
                let (expected, found) = self.show_pair(want, got);
                let msg = format!(
                    "all returns of `impl Trait` need one concrete type: expected {expected}, found {found}"
                );
                self.err(Code::TypeMismatch, n, &msg);
            }
            return r;
        }
        if opaque_return && !self.constrain_opaque_return(got, target, n) {
            return r;
        }
        // `ref.convert.lend`: a counted reference lends itself where a
        // local one is expected, with no conversion at run time. A local
        // one never becomes counted (`ref.local.escape.counted`).
        if self.lends_by_kind(g, w) {
            return r;
        }
        if let Some(out) = self.coerce_ref_kind(r, g, w, n, what, (target, position)) {
            return out;
        }
        // A least-common-type site's variable is not a declared type: the
        // value joins the others instead (types.lct).
        if self.joins.is_site(target) {
            return self.join_value(r, got, target, n, what);
        }
        // Readonly reference views may weaken reference permissions in
        // covariant container positions without changing their layout.
        // Keep unification itself invariant, especially for writable views.
        if matches!(pool.get(g), TyData::Ref { .. })
            && matches!(pool.get(w), TyData::Ref { mutable: false, .. })
        {
            let snapshot = self.infer.snapshot();
            if self.readonly_view_fits(g, w, 0) {
                return self.b.coerce(Coercion::Weaken, NONE, r, want, n.index());
            }
            self.infer.rollback(snapshot);
        }
        // `&mut T` converts to `&T` (`ref.convert.weaken`), also into an
        // optional; `&T` never converts to `&mut T` (`ref.convert.no-upgrade`).
        let shared_target = |t: Ty| match pool.get(t) {
            TyData::Ref { mutable, .. } => Some(mutable),
            TyData::Option(i) => match pool.get(self.infer.shallow(pool, i)) {
                TyData::Ref { mutable, .. } => Some(mutable),
                _ => None,
            },
            _ => None,
        };
        if let TyData::Ref {
            inner,
            mutable,
            counted,
        } = pool.get(g)
        {
            match (mutable, shared_target(w)) {
                (true, Some(false)) => {
                    let shared = pool.intern_ty(&TyData::Ref {
                        inner,
                        mutable: false,
                        counted,
                    });
                    let r = self.b.coerce(Coercion::Weaken, NONE, r, shared, n.index());
                    return self.coerce(r, shared, target, n, what, position);
                }
                (false, Some(true)) => {
                    let (want_s, got_s) = self.show_pair(want, got);
                    let msg = format!(
                        "in {what}: expected {want_s}, found {got_s}; a read-only reference never becomes writable"
                    );
                    self.err(Code::MutableUpgrade, n, &msg);
                    return r;
                }
                _ => {}
            }
        }
        // A conversion to a trait value reference (`trait.dyn.ref.convert`).
        if let Some(to) = self.dyn_ref_of(w)
            && let Some(out) = self.convert_to_dyn_ref(
                r,
                g,
                (to.0, to.1, self.ref_counted(w).unwrap_or(true)),
                n,
                what,
            )
        {
            return out;
        }
        // A variable a generic call solves is not a declared type either:
        // permissions inside it weaken (types.generic.infer.join).
        let joined = matches!(pool.get(want), TyData::Infer(_));
        if !joined && self.check_upgrade(got, want, n, what) {
            return r;
        }
        let strip = |t: Ty| match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        let (gs, ws) = (strip(g), strip(w));
        // An open literal class is never an optional or a trait value, so
        // it wraps or converts as a known type does.
        let lit = self
            .infer
            .kind_of(pool, gs)
            .and_then(VarKind::literal_default);
        let open = matches!(pool.get(gs), TyData::Infer(_)) && lit.is_none();
        if let TyData::Option(inner) = pool.get(ws)
            && !open
            && self.injects_into(gs, ws)
        {
            // A trait value reference converts before the wrap
            // (`trait.dyn.ref.convert`).
            if self.dyn_ref_of(inner).is_some() {
                let r = self.coerce(r, got, inner, n, what, position);
                return self.b.coerce(Coercion::WrapSome, NONE, r, want, n.index());
            }
            if !self.check_upgrade(got, inner, n, what) {
                self.expect(got, inner, n, what, position);
            }
            return self.b.coerce(Coercion::WrapSome, NONE, r, want, n.index());
        }
        // Internal reference getters and identity metadata still use trait
        // descriptors. Written value types cannot reach these conversions.
        if let (
            TyData::TraitValue { def: to, .. },
            TyData::TraitValue {
                def: from,
                args: from_args,
                ..
            },
        ) = (pool.get(ws), pool.get(gs))
            && to != from
            && let Some(widened) = supertrait_args(self.cx.lookup, pool, from, from_args, to, 0)
        {
            // The widening holds only when the inherited supertrait
            // arguments fit the target's (`trait.dyn.widen`); otherwise
            // the value reports below, as a mismatch.
            let widened = pool.intern_ty(&TyData::TraitValue {
                def: to,
                args: widened,
                bindings: vec![],
            });
            if self.can_unify(widened, ws) {
                return self
                    .b
                    .coerce(Coercion::Supertrait, NONE, r, want, n.index());
            }
        }
        if let TyData::TraitValue { def, args, .. } = pool.get(ws)
            && !open
            && !matches!(pool.get(gs), TyData::TraitValue { .. } | TyData::Poison)
        {
            // `types.literal.local.erased.reference-values`: a literal converted to a trait
            // value has its class's default type.
            let gs = match lit {
                Some(d) => {
                    let _ = self.infer.unify(pool, gs, d);
                    d
                }
                None => gs,
            };
            let tref = TraitRef {
                trait_: def,
                self_ty: gs,
                args,
            };
            // A value whose type lacks the impl does not convert: the
            // binding's type does not fit (`trait.dyn.convert.reference-values`).
            let before = self.diags.len();
            let _ = self.require_ref_as(tref, n, Code::TypeMismatch);
            if self.diags.len() == before {
                self.check_value_bindings(gs, ws, n);
            }
            return self
                .b
                .coerce(Coercion::ToTraitValue, NONE, r, want, n.index());
        }
        // A suspending function value weakens to its lowered constructor
        // type, which then meets `want` as any function value does
        // (`fn.type.suspend.weaken`); the reverse is not implicit. A
        // lowered type that cannot fit is reported against the value's
        // own type.
        if let Some(lowered) = self.suspend_ctor(gs, ws)
            && self.can_unify(lowered, ws)
        {
            let r = self
                .b
                .coerce(Coercion::SuspendFnToCtor, NONE, r, lowered, n.index());
            return self.coerce(r, lowered, target, n, what, position);
        }
        // Reference permissions in a function type vary by its input and
        // result positions, rather than by invariant unification (#624).
        // Keep the row fitting below: top-level function rows may widen.
        let snapshot = self.infer.snapshot();
        let variance_view = matches!(
            (pool.get(g), pool.get(w)),
            (TyData::Fn { .. }, TyData::Fn { .. })
                | (TyData::Adt { .. }, TyData::Adt { .. })
                | (TyData::Tuple { .. }, TyData::Tuple { .. })
        ) && self.variance_view_fits(g, w, 0);
        if !variance_view {
            self.infer.rollback(snapshot);
        }
        let before = self.diags.len();
        if !variance_view {
            self.expect(got, want, n, what, position);
        }
        // A row parameter inside a type argument takes its solution from
        // the argument's nested row, exactly (`types.variance.function`).
        if self.diags.len() == before && !joined {
            self.solve_nested_rows(got, want, 0);
        }
        if self.diags.len() == before && !joined && !self.perm_fits(got, want, true, 0) {
            let (want_s, got_s) = self.show_pair(want, got);
            let msg = format!(
                "in {what}: expected {want_s}, found {got_s}; permissions inside a type convert only by declared variance, and a function type inside a type keeps its row"
            );
            self.err(Code::TypeMismatch, n, &msg);
        }
        // A function value fits a wider row (`req.row.subsume`); an open
        // row variable takes the least solution (`req.row.least`).
        if self.diags.len() == before
            && !joined
            && let (TyData::Fn { row: gr, .. }, TyData::Fn { row: wr, .. }) = (
                pool.get(self.infer.resolve(pool, gs)),
                pool.get(self.infer.resolve(pool, ws)),
            )
        {
            self.fit_row(gr, wr, n, what);
        }
        // An accepted conversion whose types still differ is explicit
        // (Rule TC-6): `mut T` to `T`, a wider row, or declared variance.
        if self.diags.len() == before && !joined {
            let (gr, wr) = (
                self.infer.resolve(pool, got),
                self.infer.resolve(pool, want),
            );
            if gr != wr
                && ![gr, wr]
                    .iter()
                    .any(|t| pool.has_infer(*t) || pool.has_poison(*t))
            {
                let kind = match (pool.get(gr), pool.get(wr)) {
                    (TyData::Mut(i), _) if i == wr => Coercion::Weaken,
                    (TyData::Fn { .. }, TyData::Fn { .. }) => Coercion::RowSubsume,
                    _ => Coercion::Variance,
                };
                return self.b.coerce(kind, NONE, r, want, n.index());
            }
        }
        r
    }

    /// The lowered constructor type of the suspending function type `got`,
    /// `fn(A) -> mut Suspend[T] $ R` for `fn!(A) -> T $ R`, when `want` is
    /// an ordinary function type (`fn.type.suspend.weaken`,
    /// `fn.type.suspend.ctor`).
    fn suspend_ctor(&self, got: Ty, want: Ty) -> Option<Ty> {
        let pool = self.pool();
        let TyData::Fn {
            params,
            result,
            row,
            suspends: true,
            inputs,
        } = pool.get(self.infer.shallow(pool, got))
        else {
            return None;
        };
        let TyData::Fn {
            suspends: false, ..
        } = pool.get(self.infer.shallow(pool, want))
        else {
            return None;
        };
        let suspend = pool.intern_ty(&TyData::Adt {
            def: self.cx.names.known.suspend,
            args: pool.list(&[result]),
        });
        Some(pool.intern_ty(&TyData::Fn {
            params,
            result: pool.intern_ty(&TyData::Mut(suspend)),
            row,
            suspends: false,
            inputs,
        }))
    }

    /// Reports a mismatch and returns whether it did.
    ///
    /// `position` selects the error codes: a binary operator operand or
    /// range bound reports mixed integer signedness instead of a plain
    /// mismatch, and never implicit narrowing. It is a separate
    /// parameter so renaming a message label cannot change the code.
    pub(crate) fn expect(
        &mut self,
        got: Ty,
        want: Ty,
        n: NodeRef<'_>,
        what: &str,
        position: ExpectPosition,
    ) -> bool {
        let pool = self.pool();
        if got == Ty::NEVER {
            return false;
        }
        let (got, want) = (self.norm_ty(got), self.norm_ty(want));
        let snap = self.infer.snapshot();
        if self.infer.unify(pool, got, want).is_err() {
            self.infer.rollback(snap);
            // `types.num.narrowing`: a value of a wider type of the same
            // numeric family where a narrower one is expected.
            if position != ExpectPosition::Operand && self.narrows(got, want) {
                let msg = format!(
                    "converting to a narrower type in {what}: expected {}, found {}; write the conversion",
                    self.show(want),
                    self.show(got)
                );
                self.err(Code::ImplicitNarrowing, n, &msg);
                return true;
            }
            let (want_s, got_s) = self.show_pair(want, got);
            // `types.variance.no-insertion`, `fn.type.variance-repr.excluded`:
            // variance would allow the conversion, but an element would
            // change representation.
            if self.repr_change(got, want, 0) {
                let msg = format!(
                    "in {what}: expected {want_s}, found {got_s}; variance never changes an element's representation, so convert the element yourself"
                );
                self.err(Code::VarianceRepresentationChange, n, &msg);
                return true;
            }
            let mut msg = format!("in {what}: expected {want_s}, found {got_s}");
            // `types.num.binary.no-mix`, `expr.range.bound.signedness`: a
            // signed and an unsigned integer operand do not mix.
            let code = if position == ExpectPosition::Operand && self.mixes_signedness(got, want) {
                Code::MixedSignedness
            } else {
                Code::TypeMismatch
            };
            if code == Code::TypeMismatch {
                msg.push_str(&self.cold_call_hint(want, n));
            } else if code == Code::MixedSignedness {
                let span = self.cx.src.span(n);
                let value = &self.cx.src.text[span.lo as usize..span.hi as usize];
                let hint = if got == Ty::prim(hd_types::Prim::Usize)
                    || want == Ty::prim(hd_types::Prim::Usize)
                {
                    format!(
                        "; use `0` for a `usize` counter, or write `{want_s}({value})` for this operand"
                    )
                } else {
                    format!("; convert this operand explicitly with `{want_s}({value})`")
                };
                msg.push_str(&hint);
            }
            let hint = match pool.get(got) {
                TyData::Assoc {
                    trait_,
                    self_ty,
                    assoc,
                    ..
                } if trait_ == self.cx.names.known.add
                    && self.infer.resolve(pool, self_ty) == want
                    && self.cx.names.display_name(assoc) == "Out" =>
                {
                    format!("; constrain `{want_s} < Add[Out = {want_s}]`")
                }
                _ => String::new(),
            };
            self.err(code, n, &format!("{msg}{hint}"));
            // `types.num.no-implicit.fix`: write the conversion around the
            // value, as `i64(small)`.
            if code == Code::TypeMismatch && self.num_widens(got, want) {
                // A labelled field or argument: wrap the value after the
                // label. A shorthand `{ x }` has no value to wrap.
                let value = match n.kind() {
                    SyntaxKind::DataFieldInit | SyntaxKind::NamedArgument => n.children().next(),
                    _ => Some(n),
                };
                let Some(value) = value else {
                    return true;
                };
                let span = self.cx.src.span(value);
                let ty = self.show(want);
                self.diags.add_fix(
                    &format!("convert the value with `{ty}(...)`"),
                    hd_diag::FixSafety::Exact,
                    &[
                        (
                            Span {
                                hi: span.lo,
                                ..span
                            },
                            &format!("{ty}("),
                        ),
                        (
                            Span {
                                lo: span.hi,
                                ..span
                            },
                            ")",
                        ),
                    ],
                );
            }
            return true;
        }
        false
    }

    /// Whether one of two types is a signed and the other an unsigned
    /// integer primitive.
    pub(crate) fn mixes_signedness(&self, a: Ty, b: Ty) -> bool {
        let pool = self.pool();
        let prim = |t: Ty| {
            let t = self.strip_mut(self.infer.resolve(pool, t));
            match pool.get(t) {
                TyData::Prim(p) if p.is_integer() => Some(p),
                _ => None,
            }
        };
        matches!((prim(a), prim(b)), (Some(x), Some(y)) if x.is_unsigned() != y.is_unsigned())
    }

    /// Whether `want` is a narrower numeric type than `got` in the same
    /// family (`types.num.families`); `usize` is 32 bits wide.
    fn narrows(&self, got: Ty, want: Ty) -> bool {
        self.family_widths(got, want).is_some_and(|(g, w)| w < g)
    }

    /// Whether `want` is a wider numeric type than `got` in the same
    /// family (`types.num.no-implicit.wider`).
    fn num_widens(&self, got: Ty, want: Ty) -> bool {
        self.family_widths(got, want).is_some_and(|(g, w)| w > g)
    }

    /// The bit widths of two numeric primitives of one family
    /// (`types.num.families`), `usize` counting as 32 bits.
    fn family_widths(&self, got: Ty, want: Ty) -> Option<(u32, u32)> {
        let pool = self.pool();
        let prim = |t: Ty| {
            let t = self.strip_mut(self.infer.resolve(pool, t));
            match pool.get(t) {
                TyData::Prim(p) if p.is_integer() || p.is_float() => Some(p),
                _ => None,
            }
        };
        let (g, w) = (prim(got)?, prim(want)?);
        let family = |p: hd_types::Prim| (p.is_float(), p.is_unsigned());
        let width = |p: hd_types::Prim| match p {
            hd_types::Prim::I8 | hd_types::Prim::U8 => 8,
            hd_types::Prim::I16 | hd_types::Prim::U16 => 16,
            hd_types::Prim::I32
            | hd_types::Prim::U32
            | hd_types::Prim::Usize
            | hd_types::Prim::F32 => 32,
            _ => 64,
        };
        (family(g) == family(w)).then(|| (width(g), width(w)))
    }

    /// Whether `got` would convert to `want` by variance if one element
    /// did not change representation: a covariant or contravariant type
    /// argument, a function parameter or a function result that converts
    /// only by trait-value construction or optional injection
    /// (`types.variance.no-insertion`).
    fn repr_change(&mut self, got: Ty, want: Ty, depth: u32) -> bool {
        let pool = self.pool();
        if depth > 8 {
            return false;
        }
        let got = self.norm_ty(got);
        let got = self.strip_mut(self.infer.resolve(pool, got));
        let want = self.norm_ty(want);
        let want = self.strip_mut(self.infer.resolve(pool, want));
        // `from` meets `to` in a position: equal (`Some(false)`), or
        // convertible only by a representation change (`Some(true)`).
        // Child-to-supertrait widening rebuilds the trait value, as a
        // trait-value construction does
        // (`types.variance.repr.excluded`).
        let pair = |this: &mut Self, from: Ty, to: Ty| -> Option<bool> {
            if this.can_unify(from, to) {
                return Some(false);
            }
            if this.elem_converts(from, to) {
                return Some(true);
            }
            if this.widens(from, to) {
                return Some(true);
            }
            this.repr_change(from, to, depth + 1).then_some(true)
        };
        match (pool.get(got), pool.get(want)) {
            (TyData::Adt { def, args: xs }, TyData::Adt { def: d2, args: ys }) if def == d2 => {
                let vs = self.declared_variances(def);
                let (xs, ys) = (pool.list_items(xs), pool.list_items(ys));
                let mut changed = false;
                for (j, (x, y)) in xs.iter().zip(ys.iter()).enumerate() {
                    let step = match vs.get(j).copied().unwrap_or(0) {
                        1 => pair(self, *x, *y),
                        -1 => pair(self, *y, *x),
                        _ => self.can_unify(*x, *y).then_some(false),
                    };
                    match step {
                        Some(c) => changed |= c,
                        None => return false,
                    }
                }
                changed
            }
            (
                TyData::Fn {
                    params: xp,
                    result: xr,
                    ..
                },
                TyData::Fn {
                    params: yp,
                    result: yr,
                    ..
                },
            ) => {
                let (xp, yp) = (pool.list_items(xp), pool.list_items(yp));
                if xp.len() != yp.len() {
                    return false;
                }
                let mut changed = false;
                for (x, y) in xp.iter().zip(yp.iter()) {
                    match pair(self, *y, *x) {
                        Some(c) => changed |= c,
                        None => return false,
                    }
                }
                match pair(self, xr, yr) {
                    Some(c) => changed | c,
                    None => false,
                }
            }
            _ => false,
        }
    }

    /// Whether a value of type `from` converts to `to` only by building a
    /// trait value it implements, or by one optional injection.
    fn elem_converts(&mut self, from: Ty, to: Ty) -> bool {
        let pool = self.pool();
        let from = self.infer.resolve(pool, from);
        let from = self.strip_mut(from);
        let to = self.infer.resolve(pool, to);
        let to = self.strip_mut(to);
        if let Some((tv, mutable)) = self.dyn_ref_of(to) {
            return self.ref_of(from).is_some_and(|(inner, writable)| {
                (!mutable || writable) && self.dyn_fits(inner, tv)
            });
        }
        match pool.get(to) {
            TyData::Option(inner) => self.injects_into(from, to) && self.can_unify(from, inner),
            _ => false,
        }
    }

    /// Whether two types can unify, with no lasting effect.
    pub(crate) fn can_unify(&mut self, a: Ty, b: Ty) -> bool {
        let (a, b) = (self.norm_ty(a), self.norm_ty(b));
        let snap = self.infer.snapshot();
        let ok = self.infer.unify(self.pool(), a, b).is_ok();
        self.infer.rollback(snap);
        ok
    }

    /// Whether `from` widens to `to` through a supertrait, rebuilding
    /// the trait value: the same inheritance the coercion applies
    /// (`trait.dyn.widen`), so the error names the representation
    /// change instead of a plain mismatch.
    fn widens(&mut self, from: Ty, to: Ty) -> bool {
        let pool = self.pool();
        let from = self.strip_mut(self.infer.resolve(pool, from));
        let to = self.strip_mut(self.infer.resolve(pool, to));
        let (
            TyData::TraitValue {
                def: from_def,
                args: from_args,
                ..
            },
            TyData::TraitValue { def: to_def, .. },
        ) = (pool.get(from), pool.get(to))
        else {
            return false;
        };
        if from_def == to_def {
            return false;
        }
        let Some(widened) = supertrait_args(self.cx.lookup, pool, from_def, from_args, to_def, 0)
        else {
            return false;
        };
        let widened = pool.intern_ty(&TyData::TraitValue {
            def: to_def,
            args: widened,
            bindings: vec![],
        });
        self.can_unify(widened, to)
    }

    /// A concrete value of type `got` converts to the trait value type
    /// `want` only when its implementation binds each associated type to
    /// the bound type (trait.dyn.binding.reference-convert): otherwise the vtable's
    /// slots would read another type than the value's methods return.
    pub(crate) fn check_value_bindings(&mut self, got: Ty, want: Ty, n: NodeRef<'_>) {
        let pool = self.pool();
        let TyData::TraitValue { bindings, .. } = pool.get(want) else {
            return;
        };
        for (assoc, bound) in bindings {
            let Some(hd_resolve::ItemData::AssocType { owner, .. }) =
                self.cx.lookup.item(assoc).map(|i| &i.data)
            else {
                continue;
            };
            let proj = pool.intern_ty(&TyData::Assoc {
                assoc,
                trait_: *owner,
                self_ty: got,
                args: TyList::EMPTY,
            });
            let Ok(t) = self.normalize(proj) else {
                continue;
            };
            // The bound type may fix what inference left open, as a
            // literal's type in `Pair { key: 1 }`.
            if matches!(pool.get(t), TyData::Assoc { .. })
                || pool.has_poison(t)
                || pool.has_poison(bound)
                || self.infer.unify(pool, t, bound).is_ok()
            {
                continue;
            }
            let t = self.infer.resolve(pool, t);
            let msg = format!(
                "{} binds `{}` to {}, but the trait value type binds it to {}",
                self.show(got),
                self.cx.names.display_name(assoc),
                self.show(t),
                self.show(bound)
            );
            self.err(Code::TypeMismatch, n, &msg);
            return;
        }
    }

    /// Solves `tref` now, learning what the answer fixes; `Ok(None)` when
    /// it waits for inference.
    pub(crate) fn require_ref(
        &mut self,
        tref: TraitRef,
        at: NodeRef<'_>,
    ) -> StageResult<Option<hd_types::solver::Evidence>> {
        self.require_ref_noting(tref, at, Code::UnsatisfiedTraitBound)
    }

    /// [`Self::require_ref_as`] for an obligation whose impl the body takes
    /// to call it now or hand it to a callee: the module's initialization
    /// reaches that impl (`module.init.definite.implicit`). An impl not
    /// found yet is chosen after the statement's facts are taken, so it
    /// stands for every impl of the trait. A failure adds nothing.
    pub(crate) fn require_ref_noting(
        &mut self,
        tref: TraitRef,
        at: NodeRef<'_>,
        code: Code,
    ) -> StageResult<Option<hd_types::solver::Evidence>> {
        let trait_ = tref.trait_;
        let waiting = self.pending.len();
        let ev = self.require_ref_as(tref, at, code)?;
        if ev.is_some() || self.pending.len() > waiting {
            self.note_impl_use(trait_, ev.as_ref());
        }
        Ok(ev)
    }

    /// Retype only the inference classes that actually took the unsigned
    /// literal default. Explicit usize arguments remain untouched.
    fn integer_variant(&self, t: Ty, replacement: Ty, changed: &std::cell::Cell<bool>) -> Ty {
        let pool = self.pool();
        if self
            .infer
            .class_of(pool, t)
            .is_some_and(|c| self.fell_back.contains(&c))
        {
            changed.set(true);
            return replacement;
        }
        let t = self.infer.shallow(pool, t);
        if !pool.has_infer(t) {
            return t;
        }
        let list = |args| {
            pool.list(
                &pool
                    .list_items(args)
                    .iter()
                    .map(|t| self.integer_variant(*t, replacement, changed))
                    .collect::<Vec<_>>(),
            )
        };
        let data = match pool.get(t) {
            TyData::Adt { def, args } => TyData::Adt {
                def,
                args: list(args),
            },
            TyData::Tuple { elems, rest } => TyData::Tuple {
                elems: list(elems),
                rest: rest.map(|t| self.integer_variant(t, replacement, changed)),
            },
            TyData::Mut(t) => TyData::Mut(self.integer_variant(t, replacement, changed)),
            TyData::Option(t) => TyData::Option(self.integer_variant(t, replacement, changed)),
            TyData::Ref {
                inner,
                mutable,
                counted,
            } => TyData::Ref {
                inner: self.integer_variant(inner, replacement, changed),
                mutable,
                counted,
            },
            _ => return self.infer.resolve(pool, t),
        };
        pool.intern_ty(&data)
    }

    fn integer_default_hint_helps(&mut self, origin: TraitRef) -> bool {
        if self.fell_back.is_empty() {
            return false;
        }
        let pool = self.pool();
        for int in [
            hd_types::Prim::I32,
            hd_types::Prim::I64,
            hd_types::Prim::I8,
            hd_types::Prim::I16,
            hd_types::Prim::U8,
            hd_types::Prim::U16,
            hd_types::Prim::U32,
            hd_types::Prim::U64,
        ] {
            let changed = std::cell::Cell::new(false);
            let replacement = Ty::prim(int);
            let self_ty = self.integer_variant(origin.self_ty, replacement, &changed);
            let args = pool.list(
                &pool
                    .list_items(origin.args)
                    .iter()
                    .map(|t| self.integer_variant(*t, replacement, &changed))
                    .collect::<Vec<_>>(),
            );
            if !changed.get() {
                return false;
            }
            if pool.has_poison(self_ty) || pool.list_items(args).iter().any(|t| pool.has_poison(*t))
            {
                return false;
            }
            // Advice must not learn types or consume the body's work budget.
            // Each optional explanation has its own bounded solver query.
            let mark = self.infer.trial_mark();
            let memo = std::mem::take(&mut self.memo);
            let fuel = std::mem::replace(&mut self.fuel, Fuel::new(4096));
            let answer = self.solve(TraitRef {
                trait_: origin.trait_,
                self_ty,
                args,
            });
            self.fuel = fuel;
            self.memo = memo;
            self.infer.rollback_trial(mark);
            if matches!(answer, Ok(Answer::Holds { evidence, .. }) if !matches!(evidence, Evidence::Poison))
            {
                return true;
            }
        }
        false
    }

    fn bound_message(&mut self, tref: TraitRef, origin: TraitRef, at: NodeRef<'_>) -> String {
        let pool = self.pool();
        let trait_ty = pool.intern_ty(&TyData::TraitValue {
            def: tref.trait_,
            args: tref.args,
            bindings: vec![],
        });
        let ty = self.strip_mut(self.infer.resolve(pool, tref.self_ty));
        let mut msg = format!(
            "{} does not implement {}",
            self.show(ty),
            self.show(trait_ty)
        );
        if self.integer_default_hint_helps(origin) {
            msg.push_str("; integer literals defaulted to `usize`");
        }
        let optional = matches!(pool.get(ty), TyData::Option(_));
        let collection = matches!(pool.get(ty), TyData::Adt { def, .. } if def == self.cx.names.known.list || def == self.cx.names.known.map)
            || matches!(pool.get(ty), TyData::Tuple { .. });
        if tref.trait_ == self.cx.names.known.display && (optional || collection) {
            let value = if at.kind() == SyntaxKind::NameExpr {
                Some(at)
            } else if at.kind() == SyntaxKind::CallExpr
                && at.children().next().is_some_and(|callee| {
                    self.resolve_path(&self.segments_expr(callee))
                        == Some(self.cx.names.known.println)
                })
            {
                Src::child(at, SyntaxKind::ArgumentList)
                    .and_then(|args| args.children().next())
                    .and_then(|arg| arg.children().next())
            } else {
                None
            };
            if let Some(value) = value {
                let span = self.cx.src.span(value);
                let value = &self.cx.src.text[span.lo as usize..span.hi as usize];
                let _ = write!(msg, "; use `debug({value})`");
            } else {
                msg.push_str("; use `debug(...)`");
            }
            if optional {
                msg.push_str(" or `unwrap_or(...)`");
            }
        }
        msg
    }

    /// Offer debug formatting only for a directly braced collection name.
    fn collection_interpolation_fix(&mut self, ty: Ty, at: NodeRef<'_>) {
        let syn = at.index();
        if at.kind() != SyntaxKind::NameExpr || syn.raw() == 0 {
            return;
        }
        // Preorder puts the interpolation wrapper immediately before its first child.
        let parent = self
            .cx
            .src
            .parse
            .tree
            .node(hd_base::NodeIdx::from_raw(syn.raw() - 1));
        if parent.kind() != SyntaxKind::Interpolation
            || parent.children().next().map(NodeRef::index) != Some(syn)
        {
            return;
        }
        let pool = self.pool();
        let ty = self.strip_mut(self.infer.resolve(pool, ty));
        if !matches!(pool.get(ty), TyData::Adt { def, .. }
            if def == self.cx.names.known.list || def == self.cx.names.known.map)
            || pool.has_infer(ty)
            || pool.has_poison(ty)
        {
            return;
        }
        let span = self.cx.src.span(at);
        let sym = self.cx.names.syms.intern("debug");
        let init = self.cx.init.borrow();
        if self.find_local(sym).is_some()
            || hd_resolve::local_at(self.cx.locals, sym, span.lo).is_some()
            || init.globals.contains_key(&sym)
            || init.earlier.contains_key(&sym)
        {
            return;
        }
        drop(init);
        let Some(binding) = self.cx.scope_at(span.lo).lookup(sym) else {
            return;
        };
        if binding.kind != hd_resolve::BindingKind::Item
            || DefId::from_raw(binding.value) != self.cx.names.item("std.format", "debug")
        {
            return;
        }
        // A proposed edit must neither learn types nor spend the body's fuel.
        let mark = self.infer.trial_mark();
        let memo = std::mem::take(&mut self.memo);
        let fuel = std::mem::replace(&mut self.fuel, Fuel::new(4096));
        let answer = self.solve(TraitRef {
            trait_: self.cx.names.known.debug,
            self_ty: ty,
            args: pool.list(&[]),
        });
        self.fuel = fuel;
        self.memo = memo;
        self.infer.rollback_trial(mark);
        if !matches!(answer, Ok(Answer::Holds { evidence, .. }) if !matches!(evidence, Evidence::Poison))
        {
            return;
        }
        self.diags.add_fix(
            "format the collection with `debug`",
            hd_diag::FixSafety::Exact,
            &[
                (
                    Span {
                        hi: span.lo,
                        ..span
                    },
                    "debug(",
                ),
                (
                    Span {
                        lo: span.hi,
                        ..span
                    },
                    ")",
                ),
            ],
        );
    }

    fn has_poisoned_impl(&mut self, target: Ty) -> bool {
        let pool = self.pool();
        let target = self.strip_mut(target);
        for item in self.cx.lookup.poisoned_impls() {
            let ItemData::Impl { self_ty, .. } = item.data else {
                continue;
            };
            let mark = self.infer.trial_mark();
            let args: Vec<_> = item
                .generics
                .iter()
                .map(|_| self.infer.fresh(pool, VarKind::General))
                .collect();
            let pattern = pool.subst(self_ty, &|p: ParamRef| {
                (p.owner == item.def)
                    .then(|| args.get(p.index as usize).copied())
                    .flatten()
            });
            let found = self.infer.unify(pool, pattern, target).is_ok();
            self.infer.rollback_trial(mark);
            if found {
                return true;
            }
        }
        false
    }

    /// [`Self::require_ref`], reporting a failure with `code`.
    pub(crate) fn require_ref_as(
        &mut self,
        tref: TraitRef,
        at: NodeRef<'_>,
        code: Code,
    ) -> StageResult<Option<hd_types::solver::Evidence>> {
        let origin = tref;
        let pool = self.pool();
        // Projections with a known base normalize before the goal is
        // solved (`crates/hd_check/DESIGN.md` (trait solver) §4.3, point 3).
        let tref = TraitRef {
            trait_: tref.trait_,
            self_ty: self.norm_ty(self.infer.resolve(pool, tref.self_ty)),
            args: pool.list(
                &pool
                    .list_items(tref.args)
                    .iter()
                    .copied()
                    .map(|a| {
                        let a = self.infer.resolve(pool, a);
                        self.norm_ty(a)
                    })
                    .collect::<Vec<_>>(),
            ),
        };
        // Error recovery leaves a `never` where an earlier error in this
        // body found no type, which adds no diagnostic. Without such an
        // error, `never` is a real type that implements no sealed trait
        // (`types.sealed.never`, `trait.inspectable.not.never`).
        if self.strip_mut(tref.self_ty) == Ty::NEVER && self.diags.has_errors() {
            return Ok(Some(hd_types::solver::Evidence::Poison));
        }
        match self.solve(tref)? {
            Answer::Holds { evidence, learned } => {
                for (v, t) in learned {
                    let var = pool.intern_ty(&TyData::Infer(v));
                    let _ = self.infer.unify(pool, var, t);
                }
                Ok(Some(evidence))
            }
            Answer::Fails(_) => {
                if self.has_poisoned_impl(tref.self_ty) {
                    return Ok(Some(Evidence::Poison));
                }
                let msg = self.bound_message(tref, origin, at);
                self.err(code, at, &msg);
                if code == Code::UnsatisfiedTraitBound && tref.trait_ == self.cx.names.known.display
                {
                    self.collection_interpolation_fix(tref.self_ty, at);
                }
                Ok(None)
            }
            Answer::Stalled { .. } => {
                self.pending.push(PendingGoal {
                    tref,
                    at: at.index(),
                    kind: Waiting::Bound { code },
                });
                Ok(None)
            }
            Answer::Overflow => {
                // `trait.bound.depth.limit`: the proof needs a bound
                // deeper than 64, whether or not a deeper proof would
                // succeed.
                let msg = format!(
                    "the proof that {} implements {} needs more than 64 nested bounds",
                    self.show(tref.self_ty),
                    self.cx.names.display_name(tref.trait_)
                );
                self.err(Code::TraitResolutionDepth, at, &msg);
                Ok(None)
            }
            other => unsupported(format!("solver answer {other:?}")),
        }
    }

    pub(crate) fn solve(&mut self, tref: TraitRef) -> StageResult<Answer> {
        self.solve_goal(&Goal::Implements {
            tref,
            bindings: vec![],
            mut_: false,
        })
    }

    /// Moves the checker to byte `at` of the module: the local declarations
    /// in scope, and the local impls it sees, are those whose extent holds
    /// it (`names.local-type.name`, `trait.impl.local.lookup`). Answers
    /// remembered under other impls are dropped.
    pub(crate) fn move_to(&mut self, at: u32) {
        self.at = at;
        let hidden: Vec<DefId> = self
            .cx
            .locals
            .iter()
            .filter(|l| l.kind == hd_resolve::HeadKind::Impl && !l.covers(at))
            .map(|l| l.def)
            .collect();
        if hidden != self.hidden {
            self.hidden = hidden;
            self.memo = BodyMemo::default();
        }
    }

    /// Asks `goal` of the solver with the body's fuel. A goal that runs
    /// out of fuel stops the check (`out_of_fuel`), so no caller takes
    /// `OutOfFuel` for an answer.
    pub(crate) fn solve_goal(&mut self, goal: &Goal) -> StageResult<Answer> {
        let decls = self.cx.decls();
        let view = self.cx.impls.hiding(&self.hidden);
        let mut scx = SolveCx {
            pool: self.pool(),
            env: &self.env,
            universe: self.cx.impls.universe,
            impls: Impls::Owned(&view),
            decls: &decls,
            body_memo: &mut self.memo,
            global: self.cx.global,
        };
        match self.cx.solver.solve(&mut scx, goal, &mut self.fuel)? {
            Answer::OutOfFuel => out_of_fuel(),
            answer => Ok(answer),
        }
    }

    /// The evidence of a goal that a compiler-supplied row (`crates/hd_check/DESIGN.md` (trait solver)
    /// §3.9) or a trait value's row (§9.3) answers, when one holds; `None`
    /// for a goal the impl tables decide.
    pub(crate) fn supplied_holds(&mut self, tref: TraitRef) -> StageResult<Option<Evidence>> {
        let pool = self.pool();
        let self_ty = self.strip_mut(tref.self_ty);
        if !self.cx.names.known.sealed().supplies(tref.trait_)
            && !matches!(pool.get(self_ty), TyData::TraitValue { .. })
        {
            return Ok(None);
        }
        Ok(match self.solve(tref)? {
            Answer::Holds { evidence, .. } => Some(evidence),
            _ => None,
        })
    }

    // ------------------------------------------------------------ blocks

    /// A block's statements; with a wanted non-void type, the last
    /// expression statement is its value. Returns the tail and its type.
    pub(crate) fn block_value(
        &mut self,
        block: NodeRef<'_>,
        want: Option<Ty>,
    ) -> StageResult<(Option<Ref>, Ty)> {
        self.scopes.push(HashMap::default());
        // The suite's local declarations end with it: the rest of the
        // enclosing statement is back where it started.
        let at = self.at;
        // A suite with `defer` is a cleanup scope (`flow.defer.scopes`):
        // a `Scope` over its statements, whose suites are its `Defer`s.
        let r = if block
            .children()
            .any(|c| matches!(c.kind(), SyntaxKind::DeferStmt | SyntaxKind::UsingStmt))
        {
            let sm = self.b.open_scope();
            let inner = self.b.open_block();
            match self.lines(block, want) {
                Ok((tail, ty)) => {
                    let body = self.b.close_block(inner, tail, ty, block.index());
                    let scope = self.b.close_scope(sm, body, ty, block.index());
                    Ok((if tail.is_some() { Some(scope) } else { None }, ty))
                }
                Err(e) => Err(e),
            }
        } else {
            self.lines(block, want)
        };
        self.scopes.pop();
        self.move_to(at);
        r
    }

    fn lines(&mut self, block: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Option<Ref>, Ty)> {
        let stmts: Vec<NodeRef<'_>> = block
            .children()
            .filter(|s| s.kind() != SyntaxKind::Error)
            .collect();
        let value = want != Some(Ty::VOID);
        let mut ty = Ty::VOID;
        let mut tail = None;
        let mut diverged = false;
        // Whether a listed diverging statement already ran in this suite:
        // every later statement is unreachable
        // (`flow.unreachable.def`, `flow.unreachable.infinite-loop`). It
        // is still checked (`flow.unreachable.checked`). Tools may request
        // these source reachability facts for `flow.unreachable.lint`.
        let mut dead = false;
        for (i, s) in stmts.iter().copied().enumerate() {
            let last = i + 1 == stmts.len();
            self.move_to(self.cx.src.span(s).lo);
            if self.cx.source_facts.is_some() {
                self.statements.push(crate::comparison::Statement {
                    body: self.b.body().item,
                    span: self.cx.src.span(s),
                    reachable: !dead,
                });
            }
            if last && value && s.kind() == SyntaxKind::ExprStmt {
                let Some(e) = s.children().next() else {
                    return unsupported("an empty expression statement");
                };
                // A function-body tail that is an `if` without `else` is
                // `missing-return-value`, so that `if` does not also
                // report `flow.if.value.else`. Any other tail expression
                // leaves the marker as it was.
                let tail_if = (e.kind() == SyntaxKind::IfExpr
                    && Src::child(e, SyntaxKind::ElseClause).is_none()
                    && self.fn_body == Some(block.index()))
                .then(|| e.index());
                let saved_tail = std::mem::replace(&mut self.fn_body_tail, tail_if);
                let checked = self.expr(e, want);
                self.fn_body_tail = saved_tail;
                let (r, t) = checked?;
                // A last expression that cannot complete is a statement.
                if self.infer.shallow(self.pool(), t) == Ty::NEVER {
                    diverged = true;
                    continue;
                }
                // `fn.body.value-less-fallthrough`: a body that ends in a
                // value-less statement such as an `if` without `else`.
                if let Some(w) = want
                    && self.fn_body == Some(block.index())
                    && self.infer.shallow(self.pool(), t) == Ty::VOID
                    && !matches!(
                        self.pool().get(self.infer.shallow(self.pool(), w)),
                        TyData::Infer(_)
                    )
                    && self.infer.shallow(self.pool(), w) != Ty::VOID
                {
                    let msg = format!("this path ends without a {} value", self.show(w));
                    self.err(Code::MissingReturnValue, s, &msg);
                    diverged = true;
                    continue;
                }
                let r = match want {
                    Some(w) => self.coerce(r, t, w, e, "result", ExpectPosition::General),
                    None => r,
                };
                tail = Some(r);
                ty = want.unwrap_or(t);
                continue;
            }
            let t = self.stmt(s, last && !value && self.fn_body == Some(block.index()))?;
            diverged |= t == Ty::NEVER;
            if matches!(
                s.kind(),
                SyntaxKind::ReturnStmt | SyntaxKind::BreakStmt | SyntaxKind::ContinueStmt
            ) || self.infinite_loop(s)
            {
                dead = true;
            }
            if last && diverged {
                ty = Ty::NEVER;
            }
        }
        if value
            && tail.is_none()
            && !diverged
            && let Some(w) = want
        {
            let w = self.infer.shallow(self.pool(), w);
            if w != Ty::VOID && w != Ty::NEVER {
                let mut snap_ok = false;
                if matches!(self.pool().get(w), TyData::Infer(_)) {
                    snap_ok = self.infer.unify(self.pool(), w, Ty::VOID).is_ok();
                }
                if !snap_ok && self.fn_body == Some(block.index()) {
                    // `fn.body.value-less-fallthrough`, at the statement
                    // the body falls through from.
                    let at = stmts.last().copied().unwrap_or(block);
                    let msg = format!("this path ends without a {} value", self.show(w));
                    self.err(Code::MissingReturnValue, at, &msg);
                } else if !snap_ok && !Self::ends_in_return(block) {
                    let msg = format!("in result: expected {}, found void", self.show(w));
                    self.err(Code::TypeMismatch, block, &msg);
                }
            }
        }
        if diverged && tail.is_none() {
            ty = Ty::NEVER;
        }
        Ok((tail, ty))
    }

    /// Whether `s` is an infinite loop no `break` targets
    /// (`flow.unreachable.infinite-loop`): `while true`, or `while
    /// (true)`, whose body has no break. Mirrors the `flow.while.infinite`
    /// typing in `while_expr`.
    fn infinite_loop(&self, s: NodeRef<'_>) -> bool {
        let w = if s.kind() == SyntaxKind::WhileExpr {
            s
        } else if s.kind() == SyntaxKind::ExprStmt {
            match s.children().next() {
                Some(w) if w.kind() == SyntaxKind::WhileExpr => w,
                _ => return false,
            }
        } else {
            return false;
        };
        let Some(cond) = w.children().next() else {
            return false;
        };
        let Some(body) = Src::child(w, SyntaxKind::Block) else {
            return false;
        };
        let mut lit = cond;
        while lit.kind() == SyntaxKind::ParenExpr {
            let Some(inner) = lit.children().next() else {
                return false;
            };
            lit = inner;
        }
        lit.kind() == SyntaxKind::LiteralExpr
            && self.cx.src.tkind(self.cx.src.first(lit)) == Some(TokenKind::KwTrue)
            && !Self::has_break(body)
    }

    /// Whether a block's last statement leaves (`return`, `panic`-like
    /// never calls already typed `never` are handled by `lines`).
    fn ends_in_return(block: NodeRef<'_>) -> bool {
        block.children().last().is_some_and(|s| {
            matches!(
                s.kind(),
                SyntaxKind::ReturnStmt | SyntaxKind::BreakStmt | SyntaxKind::ContinueStmt
            )
        })
    }

    /// `flow.must-use.discard`: `Result[T, E]`, `T?` and `mut Suspend[T]`.
    pub(crate) fn must_use(&self, t: Ty) -> bool {
        self.must_use_in(t, false).is_some()
    }

    /// [`Self::must_use`] with the kind: `Some(true)` for a suspension.
    /// With `any_view` a suspension counts without `mut`, for a binding
    /// (`flow.unused.must-use`: the readonly view of `:=` and `let` does
    /// not lift the obligation).
    fn must_use_in(&self, t: Ty, any_view: bool) -> Option<bool> {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let (inner, is_mut) = match pool.get(t) {
            TyData::Mut(i) => (self.infer.resolve(pool, i), true),
            _ => (t, false),
        };
        match pool.get(inner) {
            TyData::Option(_) => Some(false),
            TyData::Adt { def, .. } if def == self.cx.names.known.result => Some(false),
            TyData::Adt { def, .. }
                if (is_mut || any_view) && def == self.cx.names.known.suspend =>
            {
                Some(true)
            }
            _ => None,
        }
    }

    /// One statement; its type is `never` when it cannot complete.
    pub(crate) fn stmt(&mut self, s: NodeRef<'_>, void_tail: bool) -> StageResult<Ty> {
        let span = self.cx.src.span(s);
        let start = self.infer.var_count();
        let open = self.open_params.len();
        let refs = self.ref_params.len();
        let waiting = self.pending.len();
        let outer = self.cur_stmt.replace(s.index());
        let r = self.stmt_node(s, void_tail).map_err(|e| e.at(span));
        self.cur_stmt = outer;
        // A `break` value joins with the loop's other values, so its class
        // stays open until the join (`types.literal.local.form.join-open`).
        if s.kind() != SyntaxKind::BreakStmt {
            // A class that met the result of a call whose impl waits takes
            // that result's type when the impl is chosen: the other
            // classes take their defaults first, which may choose it.
            let held = self.waiting_results(waiting);
            if !held.is_empty() {
                self.close_literals_but(start, &held);
                if r.is_ok() {
                    self.settle_waiting(waiting).map_err(|e| e.at(span))?;
                }
            }
            self.close_literals(start);
        }
        self.close_open_params(open);
        self.close_ref_params(refs);
        if r.is_ok() {
            self.settle_waiting(waiting).map_err(|e| e.at(span))?;
        }
        r
    }

    /// Concrete trait-qualified calls for an ambiguous written member call.
    pub(crate) fn candidate_advice(
        &self,
        trait_: DefId,
        method: DefId,
        candidates: impl IntoIterator<Item = TyList>,
        node: NodeRef<'_>,
    ) -> String {
        let name = self
            .cx
            .lookup
            .item(method)
            .map_or("", |i| self.cx.names.text(i.name));
        let written = (|| {
            let callee = node.children().next()?;
            let mut field = callee;
            while matches!(
                field.kind(),
                SyntaxKind::SuspendExpr | SyntaxKind::TypeArgsExpr
            ) {
                field = field.children().next()?;
            }
            if field.kind() != SyntaxKind::FieldExpr {
                return None;
            }
            let recv = self.cx.src.span(field.children().next()?);
            let dot = self
                .cx
                .src
                .token_span(field.direct_token(&self.cx.src.parse.tokens, TokenKind::Dot)?);
            let callee = self.cx.src.span(callee);
            let args = self
                .cx
                .src
                .span(Src::child(node, SyntaxKind::ArgumentList)?);
            let text = self.cx.src.text;
            let args = text[args.lo as usize..args.hi as usize]
                .strip_prefix('(')?
                .strip_suffix(')')?;
            let receiver = &text[recv.lo as usize..recv.hi as usize];
            let args = if args.trim().is_empty() {
                receiver.to_owned()
            } else {
                format!("{receiver}, {args}")
            };
            Some((&text[dot.hi as usize..callee.hi as usize], args))
        })();
        let mut choices = Vec::new();
        if let Some((member, args)) = written {
            for types in candidates {
                let types = self
                    .pool()
                    .list_items(types)
                    .iter()
                    .map(|t| self.show(*t))
                    .collect::<Vec<_>>()
                    .join(", ");
                let trait_ = self.cx.names.display_name(trait_);
                let qualifier = if types.is_empty() {
                    trait_.to_owned()
                } else {
                    format!("{trait_}::[{types}]")
                };
                choices.push(format!("`{qualifier}::{member}({args})`"));
            }
        }
        choices.sort_stable();
        choices.dedup();
        if choices.is_empty() {
            format!(
                "write `{}::[..]::{name}(..)` to choose one",
                self.cx.names.display_name(trait_)
            )
        } else {
            format!("choose {}", choices.join(" or "))
        }
    }

    /// A call whose receiver is known at the end of the body but whose
    /// impl still waits: several instantiations of its trait fit it
    /// (`trait.resolve.many-apply`).
    fn ambiguous_call(&mut self, g: PendingGoal, method: DefId) -> StageResult<()> {
        let pool = self.pool();
        let node = self.cx.src.parse.tree.node(g.at);
        let self_ty = self.infer.resolve(pool, g.tref.self_ty);
        let goal = Goal::Instantiations {
            self_ty,
            trait_: g.tref.trait_,
            mut_: false,
        };
        let (shown, advice) = match self.solve_goal(&goal)? {
            Answer::Candidates(c) => {
                let shown = c
                    .iter()
                    .map(|c| {
                        self.show(pool.intern_ty(&TyData::TraitValue {
                            def: g.tref.trait_,
                            args: c.args,
                            bindings: vec![],
                        }))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let advice =
                    self.candidate_advice(g.tref.trait_, method, c.iter().map(|c| c.args), node);
                (shown, advice)
            }
            _ => (
                self.cx.names.display_name(g.tref.trait_).to_owned(),
                self.candidate_advice(g.tref.trait_, method, [], node),
            ),
        };
        let msg = format!("{}: {shown} fit this call: {advice}", self.show(self_ty));
        self.err(Code::AmbiguousMethod, node, &msg);
        Ok(())
    }

    /// A member asked of a value whose type a waiting call or projection
    /// goal fixes: that goal's receiver takes its literal defaults now, as
    /// a literal receiver does, and the goal is solved again for what it
    /// learns. A failure is reported where the goal settles. Whether `t`
    /// is known now.
    pub(crate) fn learn_waiting(&mut self, t: Ty) -> StageResult<bool> {
        let pool = self.pool();
        let root = self.infer.shallow(pool, t);
        let fixes = |me: &Self, g: &PendingGoal| match g.kind {
            Waiting::Bound { .. } => false,
            Waiting::Call { .. } => pool
                .list_items(g.tref.args)
                .iter()
                .any(|a| me.infer.shallow(pool, *a) == root),
            Waiting::Project { var, .. } => me.infer.shallow(pool, var) == root,
        };
        let goals: Vec<PendingGoal> = self
            .pending
            .iter()
            .filter(|g| fixes(self, g))
            .copied()
            .collect();
        if goals.is_empty() {
            return Ok(false);
        }
        let here = self.at;
        for g in goals {
            let _ = self.zonk(g.tref.self_ty);
            self.settle_goal(g, Defaults::None, false)?;
        }
        self.move_to(here);
        Ok(!matches!(
            pool.get(self.infer.shallow(pool, t)),
            TyData::Infer(_)
        ))
    }

    /// The types that the call and projection goals pending since
    /// `waiting` fix: the trait's arguments and the projections.
    fn waiting_results(&self, waiting: usize) -> Vec<Ty> {
        let pool = self.pool();
        let mut out = Vec::new();
        for g in self.pending.get(waiting..).unwrap_or_default() {
            match g.kind {
                Waiting::Bound { .. } => {}
                Waiting::Call { .. } => out.extend(
                    pool.list_items(g.tref.args)
                        .iter()
                        .map(|a| self.infer.shallow(pool, *a)),
                ),
                Waiting::Project { var, .. } => out.push(self.infer.shallow(pool, var)),
            }
        }
        out
    }

    /// The call and projection goals that waited since `waiting`, solved
    /// again now that the statement's literal classes have their types
    /// (`types.literal.local.class.open`); one that still waits is solved
    /// again at the end of the body.
    fn settle_waiting(&mut self, waiting: usize) -> StageResult<()> {
        if !self
            .pending
            .get(waiting..)
            .is_some_and(|p| p.iter().any(|g| !matches!(g.kind, Waiting::Bound { .. })))
        {
            return Ok(());
        }
        let here = self.at;
        let goals = self.pending.split_off(waiting);
        for g in goals {
            if matches!(g.kind, Waiting::Bound { .. })
                || !self.settle_goal(g, Defaults::None, true)?
            {
                self.pending.push(g);
            }
        }
        self.move_to(here);
        Ok(())
    }

    /// Solves a goal that waited for inference, with the local impls known
    /// where it arose; `Ok(false)` when it still waits. At the end of the
    /// body its literals take their defaults first, as `defaults` says. A
    /// call or projection goal that holds learns what it fixes: the
    /// trait's arguments, and the projection's type. A goal that fails is
    /// `unsatisfied-trait-bound`, reported by its call or bound only, and
    /// only when `report`.
    fn settle_goal(
        &mut self,
        g: PendingGoal,
        defaults: Defaults,
        report: bool,
    ) -> StageResult<bool> {
        let pool = self.pool();
        let tref = if defaults == Defaults::None {
            g.tref
        } else {
            let self_ty = self.zonk(g.tref.self_ty);
            if pool.has_poison(self_ty) {
                return Ok(true);
            }
            // A bound's unknown arguments are poison; a call's arguments
            // stay open for what the goal learns.
            let args = if matches!(g.kind, Waiting::Bound { .. }) {
                self.zonk_list(g.tref.args)
            } else {
                if defaults == Defaults::All {
                    self.zonk_list(g.tref.args);
                }
                g.tref.args
            };
            TraitRef {
                trait_: g.tref.trait_,
                self_ty,
                args,
            }
        };
        let resolve = |me: &mut Self, t: Ty| {
            let t = me.infer.resolve(pool, t);
            me.norm_ty(t)
        };
        let self_ty = resolve(self, tref.self_ty);
        let args: Vec<Ty> = pool
            .list_items(tref.args)
            .iter()
            .map(|a| resolve(self, *a))
            .collect();
        let tref = TraitRef {
            trait_: tref.trait_,
            self_ty,
            args: pool.list(&args),
        };
        let node = self.cx.src.parse.tree.node(g.at);
        self.move_to(self.cx.src.span(node).lo);
        match self.solve(tref)? {
            Answer::Holds { learned, .. } => {
                if matches!(g.kind, Waiting::Bound { .. }) {
                    return Ok(true);
                }
                for (v, t) in learned {
                    let var = pool.intern_ty(&TyData::Infer(v));
                    let _ = self.infer.unify(pool, var, t);
                }
                if let Waiting::Project { assoc, var } = g.kind {
                    let self_ty = self.infer.resolve(pool, tref.self_ty);
                    let args: Vec<Ty> = pool
                        .list_items(tref.args)
                        .iter()
                        .map(|a| self.infer.resolve(pool, *a))
                        .collect();
                    let proj = pool.intern_ty(&TyData::Assoc {
                        assoc,
                        trait_: tref.trait_,
                        self_ty,
                        args: pool.list(&args),
                    });
                    let got = self.normalize_deep(proj)?;
                    if matches!(pool.get(got), TyData::Assoc { .. }) && defaults != Defaults::All {
                        return Ok(false);
                    }
                    if self.infer.unify(pool, var, got).is_err() && report {
                        let msg = format!(
                            "in result: expected {}, found {}",
                            self.show(var),
                            self.show(got)
                        );
                        self.err(Code::TypeMismatch, node, &msg);
                    }
                }
                Ok(true)
            }
            Answer::Fails(_) => {
                if self.has_poisoned_impl(tref.self_ty) {
                    return Ok(true);
                }
                if report && !matches!(g.kind, Waiting::Project { .. }) {
                    let msg = self.bound_message(tref, g.tref, node);
                    let code = match g.kind {
                        Waiting::Bound { code } => code,
                        _ => Code::UnsatisfiedTraitBound,
                    };
                    self.err(code, node, &msg);
                }
                Ok(true)
            }
            Answer::Stalled { .. } => Ok(false),
            other => unsupported(format!("solver answer {other:?}")),
        }
    }

    /// `types.literal.local.class.open`: a literal class that met no type
    /// by the end of its statement takes its default type. A class that
    /// reached the result join of an open function or closure, or the
    /// values of an enclosing loop, stays open until the join
    /// (`types.literal.local.form.join-open`): its return paths and
    /// `break` values join across statements
    /// (`types.literal.local.form.function-return.statements`,
    /// `types.literal.local.form.closure-return.statements`).
    pub(crate) fn close_literals(&mut self, start: usize) {
        self.close_literals_but(start, &[]);
    }

    /// [`Self::close_literals`] but for the classes of `held`, which are
    /// [`InferTable::shallow`] types.
    fn close_literals_but(&mut self, start: usize, held: &[Ty]) {
        let pool = self.pool();
        let joins: Vec<Ty> = self
            .rets
            .iter()
            .chain(self.loops.iter().filter_map(|(_, r)| r.as_ref()))
            .map(|r| self.infer.shallow(pool, *r))
            .collect();
        for (t, k) in self.infer.open_literals_since(pool, start) {
            let root = self.infer.shallow(pool, t);
            if joins.contains(&root) || held.contains(&root) {
                continue;
            }
            if let Some(d) = k.literal_default() {
                self.note_fallback(t, k);
                let _ = self.infer.unify(pool, t, d);
            }
        }
    }

    /// Notes that the unbound literal class of `t` is about to take its
    /// default, when that is `usize` (`flow.panic.report.fallback`).
    pub(crate) fn note_fallback(&mut self, t: Ty, kind: VarKind) {
        if kind == VarKind::IntLit {
            let pool = self.pool();
            let root = self.infer.shallow(pool, t);
            if let Some(class) = self.infer.class_of(pool, root) {
                self.fell_back.insert(class);
            }
        }
    }

    /// Declaration identity for the core void-tail exception to debug calls.
    /// Read the selected callee recorded in TIR, never reconstruct it from the
    /// source spelling: a temporary receiver's `.debug` is a method call even
    /// though its last path segment has the prelude function's name.
    fn selected_item_callee(&self, e: NodeRef<'_>, result: Ref) -> Option<DefId> {
        let call = match e.kind() {
            SyntaxKind::CallExpr => e,
            SyntaxKind::TrailingCallExpr => {
                e.children().find(|c| c.kind() == SyntaxKind::CallExpr)?
            }
            _ => return None,
        };
        let inst = result.as_inst()?;
        let i = inst.0 as usize;
        let body = self.b.body();
        if body.syn.get(i) != Some(&call.index())
            || !matches!(body.tags.get(i), Some(Tag::Call | Tag::Await))
        {
            return None;
        }
        let data = body.data.get(i)?;
        let callee = Callee::from_words(body.record(data[0]))?;
        match callee {
            Callee::Item { def, .. } => Some(def),
            Callee::TraitMethod { .. } => None,
        }
    }

    /// The source form and lexical name lookup of a discarded expression.
    /// The consumer decides whether this is advice and whether an edit is safe.
    fn discarded_expression(
        &self,
        expression: NodeRef<'_>,
        selected: Option<DefId>,
        unbound_disposable: bool,
    ) -> crate::comparison::DiscardedExpression {
        use crate::comparison::{DiscardedCall, DiscardedExpression};
        let call = (expression.kind() == SyntaxKind::CallExpr).then(|| {
            let name = expression
                .children()
                .next()
                .filter(|callee| callee.kind() == SyntaxKind::NameExpr)
                .map(|callee| self.cx.src.span(callee));
            let mut positional = 0;
            let mut named = 0;
            let mut spread = false;
            let mut after_spread = 0;
            for argument in Src::child(expression, SyntaxKind::ArgumentList)
                .iter()
                .flat_map(|list| list.children())
            {
                match argument.kind() {
                    SyntaxKind::NamedArgument => named += 1,
                    SyntaxKind::Argument => {
                        if spread {
                            after_spread += 1;
                        } else if argument
                            .children()
                            .next()
                            .is_some_and(|e| e.kind() == SyntaxKind::SpreadExpr)
                        {
                            spread = true;
                        } else {
                            positional += 1;
                        }
                    }
                    _ => {}
                }
            }
            let at = self.cx.src.span(expression).lo;
            DiscardedCall {
                name,
                positional,
                named,
                spread,
                after_spread,
                standard_debug: self.standard_format_name("debug", at),
                standard_dbg: self.standard_format_name("dbg", at),
            }
        });
        DiscardedExpression {
            span: self.cx.src.span(expression),
            selected,
            prelude_debug: selected == Some(self.cx.names.item("std.format", "debug")),
            unbound_disposable,
            call,
        }
    }

    fn standard_format_name(&self, name: &str, at: u32) -> bool {
        let sym = self.cx.names.syms.intern(name);
        let init = self.cx.init.borrow();
        if self.find_local(sym).is_some()
            || hd_resolve::local_at(self.cx.locals, sym, at).is_some()
            || init.globals.contains_key(&sym)
            || init.earlier.contains_key(&sym)
        {
            return false;
        }
        self.cx.scope_at(at).lookup(sym).is_some_and(|binding| {
            binding.kind == hd_resolve::BindingKind::Item
                && DefId::from_raw(binding.value) == self.cx.names.item("std.format", name)
        })
    }

    fn stmt_node(&mut self, s: NodeRef<'_>, void_tail: bool) -> StageResult<Ty> {
        self.charge()?;
        let kids: Vec<NodeRef<'_>> = s.children().collect();
        match s.kind() {
            SyntaxKind::ExprStmt => {
                let Some(e) = kids.first() else {
                    return Ok(Ty::VOID);
                };
                self.stmt_binding = (e.kind() == SyntaxKind::BindingExpr).then(|| e.index());
                // A REPL input's value is shown, not discarded.
                if self.shown.is_some_and(|(at, _)| at == s.index()) {
                    let (r, t) = self.expr(*e, None)?;
                    self.shown = Some((s.index(), Some((r, t))));
                    return Ok(if t == Ty::NEVER { Ty::NEVER } else { Ty::VOID });
                }
                let (result, t) = self.expr(*e, Some(Ty::VOID))?;
                let t = self.infer.shallow(self.pool(), t);
                // `fn.body.void-final`: a `void` function rejects a
                // non-`void` final expression. An omitted result is
                // inferred, never `void` by declaration, and poison
                // already reported. A must-use value and a `debug` call
                // have their own discard rules.
                let void_value = void_tail
                    && !self.result_omitted
                    && t != Ty::VOID
                    && t != Ty::NEVER
                    && !self.pool().has_poison(t)
                    && !self.must_use(t);
                // Ordinary checking needs declaration identity only for this
                // core exception; other discarded expressions need it in lint.
                let selected = (void_value || self.cx.source_facts.is_some())
                    .then(|| self.selected_item_callee(*e, result))
                    .flatten();
                let prelude_debug = selected == Some(self.cx.names.item("std.format", "debug"));
                if void_value && !prelude_debug {
                    let open = matches!(self.pool().get(t), TyData::Infer(_))
                        && !matches!(
                            self.infer.kind_of(self.pool(), t),
                            Some(VarKind::IntLit | VarKind::SignedIntLit | VarKind::FloatLit)
                        );
                    if !open {
                        let msg = format!("in result: expected void, found {}", self.show(t));
                        self.err(Code::TypeMismatch, *e, &msg);
                    }
                }
                if self.must_use(t) {
                    let msg = format!(
                        "a {} value is discarded; handle it or write `_ := ...`",
                        self.show(t)
                    );
                    self.err(Code::DiscardedMustUseValue, *e, &msg);
                }
                if self.cx.source_facts.is_some() {
                    let unbound_disposable = self.implements_disposal_protocol(t)?;
                    let fact = self.discarded_expression(*e, selected, unbound_disposable);
                    self.discarded.push(fact);
                }
                return Ok(if t == Ty::NEVER { Ty::NEVER } else { Ty::VOID });
            }
            SyntaxKind::LetStmt => self.let_stmt(s, &kids)?,
            SyntaxKind::UsingStmt => self.using_stmt(s, &kids)?,
            SyntaxKind::DiscardStmt => {
                if let Some(e) = kids.last() {
                    self.expr(*e, None)?;
                }
            }
            SyntaxKind::AssignmentStmt => self.assign(s, &kids)?,
            SyntaxKind::ReturnStmt => {
                // `flow.return.outside`: the module init body is not a
                // function, at any nesting depth; a closure or local
                // function pushes its own result type.
                if self.module_init.is_some() && self.rets.len() == 1 {
                    self.err(
                        Code::ReturnOutsideFunction,
                        s,
                        "`return` needs a function or closure",
                    );
                    return Ok(Ty::NEVER);
                }
                if self.defer_base.is_some() && self.subs.len() == self.defer_subs {
                    self.err(Code::DeferControlFlow, s, "a `defer` suite cannot `return`");
                }
                let ret = *self.rets.last().expect("ret");
                let r = if let Some(e) = kids.first() {
                    let (r, t) = self.expr(*e, Some(ret))?;
                    self.coerce(r, t, ret, *e, "return", ExpectPosition::General)
                } else {
                    if self.strict_rets.contains(&ret) {
                        self.coerce(
                            Ref(NONE),
                            Ty::VOID,
                            ret,
                            s,
                            "return",
                            ExpectPosition::General,
                        );
                    }
                    Ref(NONE)
                };
                self.b.emit(Tag::Return, r.0, NONE, Ty::NEVER, s.index());
                return Ok(Ty::NEVER);
            }
            SyntaxKind::BreakStmt | SyntaxKind::ContinueStmt => {
                let Some((lp, bt)) = self.loops.last().copied() else {
                    self.err(Code::BreakOutsideLoop, s, "break-outside-loop");
                    return Ok(Ty::NEVER);
                };
                if self.defer_base == Some(self.loops.len()) {
                    self.err(
                        Code::DeferControlFlow,
                        s,
                        "a `defer` suite cannot leave an enclosing loop",
                    );
                }
                if s.kind() == SyntaxKind::ContinueStmt {
                    self.b
                        .emit(Tag::Continue, lp.0.raw(), NONE, Ty::NEVER, s.index());
                    return Ok(Ty::NEVER);
                }
                let v = match (kids.first(), bt) {
                    (Some(e), Some(want)) => {
                        let (r, t) = self.expr(*e, Some(want))?;
                        self.coerce(r, t, want, *e, "break value", ExpectPosition::General)
                            .0
                    }
                    (Some(e), None) => {
                        self.err(
                            Code::BreakValueContext,
                            *e,
                            "only a loop with `else` takes a `break` value",
                        );
                        NONE
                    }
                    (None, Some(_)) => {
                        self.err(
                            Code::BreakValueContext,
                            s,
                            "a loop with `else` needs a `break` value",
                        );
                        NONE
                    }
                    (None, None) => NONE,
                };
                self.b.emit(Tag::Break, lp.0.raw(), v, Ty::NEVER, s.index());
                return Ok(Ty::NEVER);
            }
            SyntaxKind::DeferStmt => {
                let Some(blk) = Src::child(s, SyntaxKind::Block) else {
                    return unsupported("a `defer` without a suite");
                };
                let m = self.b.open_block();
                let saved = self.defer_base.replace(self.loops.len());
                let saved_subs = std::mem::replace(&mut self.defer_subs, self.subs.len());
                self.block_value(blk, Some(Ty::VOID))?;
                self.defer_base = saved;
                self.defer_subs = saved_subs;
                let suite = self.b.close_block(m, None, Ty::VOID, blk.index());
                // Each of these was reported (`defer-control-flow`,
                // `suspension-forbidden-context`); a poison keeps the
                // suite within invariant 6.
                let body = self.b.body_mut();
                for i in hd_tir::ir::defer_suite_violations(body, suite.0) {
                    body.tags[i as usize] = Tag::Poison;
                    body.data[i as usize] = [NONE, NONE];
                }
                self.b.defer(suite, s.index());
            }
            SyntaxKind::DataDecl
            | SyntaxKind::EnumDecl
            | SyntaxKind::TraitDecl
            | SyntaxKind::TypeDecl
            | SyntaxKind::ImplDecl => self.local_decl(s)?,
            SyntaxKind::FnDecl => self.local_fn(s)?,
            other => return unsupported(format!("statement {other:?}")),
        }
        Ok(Ty::VOID)
    }

    /// A whole-name assignment requires `let`, without granting field mutation.
    fn reassignable_binding_fix(&mut self, local: LocalId) {
        let syn = self.b.body_mut().local_syn[local.idx()];
        let tree = &self.cx.src.parse.tree;
        if syn == hd_base::NodeIdx::NONE || syn.raw() < 2 || syn.idx() >= tree.len() {
            return;
        }
        let ty = self.b.local_ty(local);
        if matches!(
            self.pool().get(self.infer.resolve(self.pool(), ty)),
            TyData::Fn { .. }
        ) {
            return;
        }
        let pattern = tree.node(syn);
        // Nodes are in preorder, and both wrappers must own their first child.
        let binding = tree.node(hd_base::NodeIdx::from_raw(syn.raw() - 1));
        let statement = tree.node(hd_base::NodeIdx::from_raw(syn.raw() - 2));
        if pattern.kind() != SyntaxKind::BindingPattern
            || binding.kind() != SyntaxKind::BindingExpr
            || statement.kind() != SyntaxKind::ExprStmt
            || binding.children().next().map(NodeRef::index) != Some(syn)
            || statement.children().next().map(NodeRef::index) != Some(binding.index())
        {
            return;
        }
        let Some(rhs) = binding.children().nth(1) else {
            return;
        };
        if matches!(
            rhs.kind(),
            SyntaxKind::ClosureExpr | SyntaxKind::BindingExpr
        ) {
            return;
        }
        // Bound the work repeated for each failed assignment to this binding.
        for (index, node) in rhs.descendants().enumerate() {
            if index == 64
                || matches!(
                    node.kind(),
                    SyntaxKind::ClosureExpr | SyntaxKind::BindingExpr
                )
            {
                return;
            }
        }
        let Some(operator) = binding.direct_token(&self.cx.src.parse.tokens, TokenKind::ColonEq)
        else {
            return;
        };
        let span = self.cx.src.span(pattern);
        self.diags.add_fix(
            "declare the reassigned name with `let`",
            hd_diag::FixSafety::Exact,
            &[
                (
                    Span {
                        hi: span.lo,
                        ..span
                    },
                    "let ",
                ),
                (self.cx.src.token_span(operator), "="),
            ],
        );
    }

    /// `x = e`, `x op= e`, `a.f = e`, `a[i] = e`, `v() = e`.
    fn assign(&mut self, s: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<()> {
        let [lhs, rhs] = kids else {
            return unsupported("this assignment form");
        };
        let op_tok = hd_base::TokenIdx::from_raw(self.cx.src.last(*lhs).raw() + 1);
        let op = self.cx.src.tkind(op_tok);
        let compound = match op {
            Some(TokenKind::Eq) => None,
            Some(k) => Some(k),
            None => return unsupported("an assignment operator"),
        };
        if lhs.kind() == SyntaxKind::DerefExpr {
            return self.deref_assign(*lhs, *rhs, compound, s);
        }
        match lhs.kind() {
            SyntaxKind::NameExpr => {
                let name = self.sym_of(*lhs);
                // The module's own binding, else an earlier REPL input's
                // (`cli.repl.declarations.bindings`).
                let global = {
                    let init = self.cx.init.borrow();
                    init.globals
                        .get(&name)
                        .map(crate::init::Global::binding)
                        .or_else(|| init.earlier.get(&name).copied())
                };
                if self.find_local(name).is_none()
                    && let Some(crate::init::InputBinding { def, ty, short, .. }) = global
                {
                    // A mutable top-level binding: `Store [@def] v`.
                    if short {
                        let msg = format!(
                            "`{}` is bound with `:=`; use `let` to reassign it",
                            self.cx.names.text(name)
                        );
                        self.err(Code::NonReassignableBinding, *lhs, &msg);
                    }
                    if compound.is_some() {
                        return unsupported("compound assignment to a top-level binding");
                    }
                    let (r, t) = self.expr(*rhs, Some(ty))?;
                    let v = self.coerce(r, t, ty, *rhs, "assignment", ExpectPosition::General);
                    self.store_global(def, ty, v, s.index());
                    return Ok(());
                }
                let Some((l, depth)) = self.find_local(name) else {
                    let text = self.cx.names.text(name).to_owned();
                    if !self.is_poison_name(&text) {
                        let msg = format!("`{text}` is not defined");
                        self.err(Code::UnknownName, *lhs, &msg);
                    }
                    return Ok(());
                };
                let flags = self.b.body_mut().local_flags[l.idx()];
                if self.using_locals.contains(&l) {
                    let msg = format!(
                        "`{}` is bound with `using`, so it cannot be reassigned",
                        self.cx.names.text(name)
                    );
                    self.err(Code::NonReassignableBinding, *lhs, &msg);
                } else if flags & local_flags::SHORT != 0 {
                    let msg = format!(
                        "`{}` is bound with `:=`; use `let` to reassign it",
                        self.cx.names.text(name)
                    );
                    self.err(Code::NonReassignableBinding, *lhs, &msg);
                    self.reassignable_binding_fix(l);
                } else if flags & local_flags::PARAM != 0 && !self.mut_params.contains(&l) {
                    let msg = format!(
                        "parameter `{}` cannot be reassigned",
                        self.cx.names.text(name)
                    );
                    self.err(Code::NonReassignableParameterBinding, *lhs, &msg);
                }
                let lt = self.b.local_ty(l);
                let v = match compound {
                    None => {
                        let (r, t) = self.expr(*rhs, Some(lt))?;
                        self.coerce(r, t, lt, *rhs, "assignment", ExpectPosition::General)
                    }
                    Some(k) => {
                        let (cur, _) = self.read_local(l, depth, *lhs);
                        self.compound(k, cur, lt, *rhs, s)?
                    }
                };
                self.note_write(l, depth);
                self.note_param_write(l, s.index());
                self.set_local(l, v, s.index());
            }
            SyntaxKind::FieldExpr => {
                let Some(base) = lhs.children().next() else {
                    return unsupported("a field without a base");
                };
                let fname = self.cx.src.text(self.cx.src.last(*lhs)).to_owned();
                self.elem_may_stand(base, crate::elem_refs::ElemUse::Through);
                // The place the base names, which nothing reads yet
                // (`docs/compiler/tir-v2.md`, "Field writes").
                let Some(op) = self.place_of(base, None, crate::refs::PlaceUse::Write)? else {
                    return Ok(());
                };
                let (br, bt) = match op {
                    // A struct field changes in a writable place only
                    // (`types.value.writable-forms`).
                    crate::refs::Operand::Place(p) if self.is_struct_ty(p.ty) => {
                        let p = self.promote_place(p, &fname);
                        return self.struct_field_assign(
                            p,
                            (base, *lhs, *rhs, s),
                            &fname,
                            compound,
                        );
                    }
                    crate::refs::Operand::Place(p) => self.read_place(&p, base),
                    crate::refs::Operand::Value(r, t) => (r, t),
                };
                // A field of a referenced value: through `&mut` only
                // (`ref.write.field`).
                let (br, bt) = match self.ref_of(bt) {
                    Some((_, false)) => {
                        let msg = format!(
                            "this writes through a read-only reference {}; take the reference with `&mut` to write through it",
                            self.show(bt)
                        );
                        self.err(Code::NotWritable, *lhs, &msg);
                        return Ok(());
                    }
                    Some((inner, true)) if self.is_struct_ty(self.deref_ty(inner, true)) => {
                        let place = crate::refs::Place {
                            root: crate::refs::Root::Deref(br, bt),
                            path: vec![],
                            ty: self.deref_ty(inner, true),
                        };
                        let p = self.promote_place(place, &fname);
                        return self.struct_field_assign(
                            p,
                            (base, *lhs, *rhs, s),
                            &fname,
                            compound,
                        );
                    }
                    Some((_, true)) => self.emit_deref(br, bt, base.index()),
                    None => (br, bt),
                };
                // A struct value that is no place: its fields cannot change
                // (`types.value.write.other-value`).
                if self.is_struct_ty(bt) {
                    let (_, bt) = self.promote_base(br, bt, &fname, *lhs);
                    if self.field_of(bt, &fname).is_none() {
                        let msg = format!("no field `{fname}` on {}", self.show(bt));
                        self.err(Code::UnknownField, *lhs, &msg);
                        return Ok(());
                    }
                    self.check_field_visible(bt, &fname, *lhs);
                    let msg = "this struct value is not a variable or a field of one, so its fields cannot change; copy it into a `let mut` first";
                    self.err(Code::NotWritable, *lhs, msg);
                    return Ok(());
                }
                let (pr, pt) = self.promote_base(br, bt, &fname, *lhs);
                let Some((idx, ft)) = self.field_of(pt, &fname) else {
                    // Tuple elements and shared enum data are selected but
                    // never stored through (`expr.place.enum-shared-field`).
                    if self.is_read_only_member(bt, &fname) {
                        let msg = format!("`{fname}` of {} cannot be assigned", self.show(bt));
                        self.err(Code::InvalidAssignmentTarget, *lhs, &msg);
                        return Ok(());
                    }
                    let msg = format!("no field `{fname}` on {}", self.show(bt));
                    self.err(Code::UnknownField, *lhs, &msg);
                    return Ok(());
                };
                self.check_field_visible(pt, &fname, *lhs);
                self.check_store_target((br, bt), *lhs);
                let br = pr;
                let v = match compound {
                    None => {
                        let (r, t) = self.expr(*rhs, Some(ft))?;
                        self.coerce(r, t, ft, *rhs, "assignment", ExpectPosition::General)
                    }
                    Some(k) => {
                        let cur = self.field_value(br, idx, ft, lhs.index());
                        self.compound(k, cur, ft, *rhs, s)?
                    }
                };
                // A data value's field: `Store [*%o .k] v`.
                let word = self.b.place(&hd_tir::ir::Place {
                    root: hd_tir::ir::PlaceRoot::Value(br),
                    proj: vec![hd_tir::ir::Proj::Deref, hd_tir::ir::Proj::Field(idx)],
                });
                self.b.emit(Tag::Store, word, v.0, Ty::VOID, s.index());
            }
            SyntaxKind::IndexExpr => {
                let ik: Vec<NodeRef<'_>> = lhs.children().collect();
                let [base, key] = ik.as_slice() else {
                    return unsupported("an index target shape");
                };
                // A place without built-in indexing stores through
                // `IndexSet`'s `&mut self` method, which takes the place's
                // address (`expr.index.trait.mut-ref`).
                if let Some(pt) = self.peek_place_ty(*base)
                    && self.ref_of(pt).is_none()
                    && !self.builtin_index_base(pt)
                {
                    // A type with no `IndexSet` at all is no place to store
                    // into, whatever its binding (`expr.index.trait.write-ref`).
                    let set = self.cx.names.known.index_set;
                    if !self.op_fits(set, pt, None)? {
                        self.expr(*base, None)?;
                        self.no_index_set(pt, *lhs);
                        return Ok(());
                    }
                    if let Some(recv) = self.store_receiver(*base, "index_set")? {
                        self.index_set(recv, (*base, *key), *rhs, compound, (*lhs, s))?;
                    }
                    return Ok(());
                }
                let (br, bt) = self.expr(*base, None)?;
                let (br, bt) = self.deref_index_base(br, bt, base.index());
                // `expr.index.trait.place`: a string is not a place.
                if self.strip_mut(bt) == Ty::STRING {
                    self.err(
                        Code::InvalidAssignmentTarget,
                        *lhs,
                        "this index cannot be assigned",
                    );
                    return Ok(());
                }
                let builtin = self.index_kind(bt);
                let Some((op_get, kt, vt)) = builtin else {
                    // No place: a `&mut` reference stores through what it
                    // refers to.
                    if let Some(recv) = self.stored_through(br, bt, "index_set", *base) {
                        self.index_set(recv, (*base, *key), *rhs, compound, (*lhs, s))?;
                    }
                    return Ok(());
                };
                let (kr, key_ty) = self.index_key_value(*key, builtin)?;
                // `expr.index.slice.no-store`: no range type stores into a
                // list.
                if op_get != Indexed::Map && self.is_range_ty(key_ty) {
                    self.err(
                        Code::InvalidAssignmentTarget,
                        *lhs,
                        "this index cannot be assigned",
                    );
                    return Ok(());
                }
                self.check_store_target((br, bt), *lhs);
                let kr = self.index_key((kr, key_ty), *key, kt);
                let v = match compound {
                    None => {
                        let (r, t) = self.expr(*rhs, Some(vt))?;
                        self.coerce(r, t, vt, *rhs, "assignment", ExpectPosition::General)
                    }
                    Some(k) => {
                        let cur = self.index_read(op_get, (br, kr), vt, lhs.index());
                        self.compound(k, cur, vt, *rhs, s)?
                    }
                };
                // A list element is a place of the list's buffer:
                // `Store [%xs [%i]] v` (`docs/compiler/tir-v2.md`, "Element
                // reads and writes"); a map value is no place.
                if op_get == Indexed::List {
                    let word = self.b.place(&hd_tir::ir::Place {
                        root: hd_tir::ir::PlaceRoot::Value(br),
                        proj: vec![hd_tir::ir::Proj::Elem(kr)],
                    });
                    self.b.emit(Tag::Store, word, v.0, Ty::VOID, s.index());
                } else {
                    let rec = self.b.refs_record(&[br, kr, v]);
                    self.b.emit(
                        Tag::Intrinsic,
                        IntrinsicOp::MapSet as u32,
                        rec,
                        Ty::VOID,
                        s.index(),
                    );
                }
            }
            SyntaxKind::CallExpr => self.call_place(*lhs, *rhs, compound, s)?,
            // `grammar.stmt.assign-place`: any other left side is no place.
            _ => self.err(
                Code::InvalidAssignmentTarget,
                *lhs,
                "this expression is not a place that can be assigned",
            ),
        }
        Ok(())
    }

    /// `expr.index.trait.place`: a store into a type without `IndexSet`.
    fn no_index_set(&mut self, t: Ty, lhs: NodeRef<'_>) {
        let msg = format!("{} does not implement `IndexSet`", self.show(t));
        self.err(Code::InvalidAssignmentTarget, lhs, &msg);
    }

    /// Whether a value of type `t` indexes as a list, map or string, with
    /// no `IndexSet` call (`expr.index.trait.builtin-direct`).
    fn builtin_index_base(&mut self, t: Ty) -> bool {
        let pool = self.pool();
        let t = self.strip_mut(self.infer.resolve(pool, t));
        t == Ty::STRING
            || matches!(
                pool.get(t),
                TyData::Poison | TyData::Never | TyData::Infer(_)
            )
            || self.index_kind(t).is_some()
    }

    /// `r[k] = v` and `r[k] op= v` on a receiver without built-in
    /// indexing: `IndexSet::[K, V]::index_set(&mut r, k, v)`, after
    /// `Index::[K]::index(r, k)` for a compound one
    /// (`expr.index.trait.write-ref`). `br` is the reference to the receiver
    /// (`Ck::store_receiver`), and `bt` the type of the value it refers to.
    fn index_set(
        &mut self,
        (br, bt): (Ref, Ty),
        (_base, key): (NodeRef<'_>, NodeRef<'_>),
        rhs: NodeRef<'_>,
        compound: Option<TokenKind>,
        (lhs, s): (NodeRef<'_>, NodeRef<'_>),
    ) -> StageResult<()> {
        let (kr, kt) = self.expr(key, None)?;
        let set = self.cx.names.known.index_set;
        self.tuple_key_choice(set, bt, kt)?;
        if !self.op_fits(set, bt, Some(kt))? {
            self.no_index_set(bt, lhs);
            return Ok(());
        }
        // `expr.index.trait.choice`: an unsuffixed literal key joins the
        // choice of the implementation, with the value as its second
        // argument.
        if compound.is_none()
            && self
                .literal_arg_choice(set, "index_set", (br, bt), (&[key, rhs], kt), s)?
                .is_some()
        {
            return Ok(());
        }
        let (vr, vt) = match compound {
            None => self.expr(rhs, None)?,
            Some(k) => {
                let index = self.cx.names.known.index;
                if !self.op_fits(index, bt, Some(kt))? {
                    let msg = format!(
                        "{} has no `Index` implementation for this key",
                        self.show(bt)
                    );
                    self.err(Code::TypeMismatch, lhs, &msg);
                    return Ok(());
                }
                // `index` reads the value the reference refers to.
                let (bv, bvt) = self.emit_deref(br, self.b.ty_of(br), lhs.index());
                let chosen =
                    self.literal_arg_choice(index, "index", (bv, bvt), (&[key], kt), lhs)?;
                let (cur, ct) = match chosen {
                    Some(c) => c,
                    None => self.trait_call_args(index, "index", bv, bvt, &[(kr, kt, key)], lhs)?,
                };
                let v = self.compound(k, cur, ct, rhs, s)?;
                (v, self.b.ty_of(v))
            }
        };
        if compound.is_some() {
            // The computed value stands for `rhs` in the trials, so it
            // is not evaluated again.
            let pre = self.pre_args.len();
            self.pre_args.push((rhs.index(), vr, vt));
            let chosen = self.literal_arg_choice(set, "index_set", (br, bt), (&[key, rhs], kt), s);
            self.pre_args.truncate(pre);
            if chosen?.is_some() {
                return Ok(());
            }
        }
        self.trait_call_args(set, "index_set", br, bt, &[(kr, kt, key), (vr, vt, rhs)], s)?;
        Ok(())
    }

    /// `cur op rhs` for a compound assignment.
    pub(crate) fn compound(
        &mut self,
        k: TokenKind,
        cur: Ref,
        t: Ty,
        rhs: NodeRef<'_>,
        at: NodeRef<'_>,
    ) -> StageResult<Ref> {
        let op = match k {
            TokenKind::PlusEq => TokenKind::Plus,
            TokenKind::MinusEq => TokenKind::Minus,
            TokenKind::StarEq => TokenKind::Star,
            TokenKind::SlashEq => TokenKind::Slash,
            TokenKind::PercentEq => TokenKind::Percent,
            TokenKind::AmpEq => TokenKind::Amp,
            TokenKind::PipeEq => TokenKind::Pipe,
            TokenKind::CaretEq => TokenKind::Caret,
            TokenKind::ShlEq => TokenKind::Shl,
            TokenKind::ShrEq => TokenKind::Shr,
            _ => return unsupported("this compound assignment operator"),
        };
        let want = (!matches!(op, TokenKind::Shl | TokenKind::Shr)).then_some(t);
        let (r, rt) = self.expr(rhs, want)?;
        self.binary_values(op, (cur, t), (r, rt), at, rhs)
    }

    /// Resolves every type the body recorded; literal variables with no
    /// other constraint take their default type (`crates/hd_check/DESIGN.md` (type checking) §3.6).
    pub(crate) fn zonk(&mut self, t: Ty) -> Ty {
        let pool = self.pool();
        let r = self.infer.resolve(pool, t);
        if !pool.has_infer(r) {
            return if pool.has_assoc(r) {
                self.normalize_deep(r).unwrap_or(r)
            } else {
                r
            };
        }
        match pool.get(r) {
            TyData::Infer(_) => {
                match self
                    .infer
                    .kind_of(pool, r)
                    .and_then(VarKind::literal_default)
                {
                    Some(d) => {
                        if let Some(k) = self.infer.kind_of(pool, r) {
                            self.note_fallback(r, k);
                        }
                        let _ = self.infer.unify(pool, r, d);
                        d
                    }
                    None => Ty::POISON,
                }
            }
            TyData::Adt { def, args } => {
                let a = self.zonk_list(args);
                pool.intern_ty(&TyData::Adt { def, args: a })
            }
            TyData::Tuple { elems, rest } => {
                let e = self.zonk_list(elems);
                pool.intern_ty(&TyData::Tuple { elems: e, rest })
            }
            TyData::Option(i) => {
                let i = self.zonk(i);
                pool.intern_ty(&TyData::Option(i))
            }
            TyData::Mut(i) => {
                let i = self.zonk(i);
                pool.intern_ty(&TyData::Mut(i))
            }
            TyData::Ref {
                inner,
                mutable,
                counted,
            } => {
                let inner = self.zonk(inner);
                pool.intern_ty(&TyData::Ref {
                    inner,
                    mutable,
                    counted,
                })
            }
            TyData::Fn {
                params,
                result,
                row,
                suspends,
                inputs,
            } => {
                let p = self.zonk_list(params);
                let r = self.zonk(result);
                let row = self.zonk_row(row);
                pool.intern_ty(&TyData::Fn {
                    params: p,
                    result: r,
                    row,
                    suspends,
                    inputs,
                })
            }
            TyData::Row(row) => {
                let row = self.zonk_row(row);
                pool.intern_ty(&TyData::Row(row))
            }
            TyData::Opaque {
                owner,
                site,
                args,
                bound,
            } => {
                let args = self.zonk_list(args);
                let bound = self.zonk(bound);
                pool.intern_ty(&TyData::Opaque {
                    owner,
                    site,
                    args,
                    bound,
                })
            }
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => {
                let a = self.zonk_list(args);
                let bindings = bindings
                    .into_iter()
                    .map(|(d, b)| (d, self.zonk(b)))
                    .collect();
                pool.intern_ty(&TyData::TraitValue {
                    def,
                    args: a,
                    bindings,
                })
            }
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => {
                let s = self.zonk(self_ty);
                let a = self.zonk_list(args);
                let x = pool.intern_ty(&TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty: s,
                    args: a,
                });
                self.normalize(x).unwrap_or(x)
            }
            _ => Ty::POISON,
        }
    }

    /// A row with its variables solved; a row variable nothing constrained
    /// is the empty row, the least solution.
    pub(crate) fn zonk_row(&mut self, row: RowId) -> RowId {
        let pool = self.pool();
        let row = self.infer.resolve_row(pool, row);
        let mut d = pool.row_data(row);
        if !d.keys.iter().any(|k| pool.has_infer(*k)) {
            return row;
        }
        let mut keys = Vec::new();
        for k in d.keys {
            if matches!(pool.get(k), TyData::Infer(_)) {
                let empty = pool.intern_ty(&TyData::Row(RowId::EMPTY));
                let _ = self.infer.unify(pool, k, empty);
            } else {
                keys.push(self.zonk(k));
            }
        }
        d.keys = keys;
        pool.row(&d)
    }

    fn zonk_list(&mut self, l: TyList) -> TyList {
        // Owned: `zonk` needs `&mut self` while the items are walked.
        let mut items: Vec<Ty> = self.pool().list_items(l).to_vec();
        for x in &mut items {
            *x = self.zonk(*x);
        }
        self.pool().list(&items)
    }

    pub(crate) fn finish_body(self, root: Ref) -> StageResult<Body> {
        self.finish(root)
    }

    /// A call's choice once inference is done: a `Builtin` choice made
    /// while the self type was still a literal's variable (`Box { value:
    /// 1 } == ...`, or `(1, "b") < (2, "a")`, whose tuple template waits
    /// on its elements) becomes the impl the solver finds at the defaulted
    /// type. A choice the solver does not answer with an impl stays.
    fn settle_choice(
        &mut self,
        trait_: DefId,
        self_ty: Ty,
        targs: TyList,
        choice: (hd_tir::ir::ChoiceKind, u32),
    ) -> StageResult<(hd_tir::ir::ChoiceKind, u32)> {
        let pool = self.pool();
        let waited = choice == crate::call::PENDING_CHOICE;
        if !waited
            && (choice.0 != hd_tir::ir::ChoiceKind::Builtin
                || !matches!(pool.get(self_ty), TyData::Adt { .. } | TyData::Tuple { .. }))
        {
            return Ok(choice);
        }
        let n = self.cx.lookup.item(trait_).map_or(0, |i| i.generics.len());
        let items = pool.list_items(targs);
        let tref = TraitRef {
            trait_,
            self_ty,
            args: pool.list(&items[..n.min(items.len())]),
        };
        // A call whose impl waited takes whatever evidence there is now;
        // with none, an error was reported or a type stayed unknown.
        Ok(match self.solve(tref)? {
            Answer::Holds {
                evidence: ev @ hd_types::solver::Evidence::Impl { .. },
                ..
            } => self.choice_of(Some(&ev)),
            Answer::Holds { evidence: ev, .. } if waited => self.choice_of(Some(&ev)),
            _ if waited => (hd_tir::ir::ChoiceKind::Builtin, 0),
            _ => choice,
        })
    }

    /// Records the body's overflow-checked `usize` operations whose type
    /// came from a literal's default, with the local such an operation
    /// reads (`flow.panic.report.fallback`). `open` is the body's types
    /// before they were resolved, which still name the classes that took
    /// the default.
    fn fallback_rows(&mut self, open: &[Ty]) {
        if self.fell_back.is_empty() {
            return;
        }
        let pool = self.pool();
        let fell: Vec<bool> = open
            .iter()
            .map(|t| {
                self.infer
                    .class_of(pool, *t)
                    .is_some_and(|c| self.fell_back.contains(&c))
            })
            .collect();
        let checked = [
            hd_tir::ir::PrimOp::Add as u32,
            hd_tir::ir::PrimOp::Sub as u32,
            hd_tir::ir::PrimOp::Mul as u32,
            hd_tir::ir::PrimOp::Neg as u32,
            hd_tir::ir::PrimOp::Pow as u32,
        ];
        let usize_ty = Ty::prim(hd_types::Prim::Usize);
        let body = self.b.body_mut();
        let mut rows = Vec::new();
        for (i, tag) in body.tags.iter().enumerate() {
            if *tag != Tag::Prim || !checked.contains(&body.data[i][0]) || body.ty[i] != usize_ty {
                continue;
            }
            // The operands whose type took the default, among the
            // instructions' values.
            let fell_operands: Vec<usize> = body
                .record(body.data[i][1])
                .iter()
                .filter(|r| **r & Ref::CONST_BIT == 0)
                .map(|r| *r as usize)
                .filter(|r| fell[*r])
                .collect();
            if !fell[i] && fell_operands.is_empty() {
                continue;
            }
            let read = fell_operands
                .iter()
                .find_map(|r| body.local_read(*r))
                .map_or(0, |l| l.raw() + 1);
            rows.push([u32::try_from(i).expect("instructions"), read]);
        }
        body.fallback = rows;
    }

    fn finish(self, root: Ref) -> StageResult<Body> {
        let ret = self.rets[0];
        self.finish_with(root, ret, None).map(|(b, ..)| b)
    }

    /// An impl method's inferred result against its impl parameters'
    /// variances (`types.variance.surface`): a method that omits its
    /// result type still counts toward the markers its parameters derive
    /// from the impl target. Declared parameters, results and rows go
    /// through `impl_variance` in lowering; only the inferred result
    /// needs this, once inference has settled it.
    fn check_inferred_result(&mut self, node: NodeRef<'_>, ret: Ty) {
        // A declared result already checks in lowering, and a poisoned
        // or still-open result already reports elsewhere.
        if Src::type_child(node).is_some() {
            return;
        }
        let pool = self.pool();
        if pool.has_poison(ret) || pool.has_infer(ret) {
            return;
        }
        let item = self.b.body_mut().item;
        let owner = match self.cx.lookup.item(item).map(|i| &i.data) {
            Some(ItemData::Method { owner, .. }) => *owner,
            _ => return,
        };
        let Some(impl_item) = self.cx.lookup.item(owner) else {
            return;
        };
        let hd_resolve::ItemData::Impl { self_ty, .. } = &impl_item.data else {
            return;
        };
        let self_ty = *self_ty;
        let generics = impl_item.generics.clone();
        let TyData::Adt { def: target, args } = pool.get(self_ty) else {
            return;
        };
        // The built-in collections' markers describe their readonly views
        // only, as in lowering.
        let known = self.cx.names.known;
        if target == known.list || target == known.map {
            return;
        }
        let declared = self.declared_variances(target);
        if declared.iter().all(|v| *v == 0) {
            return;
        }
        let global = self.cx.names.pool;
        let var_of = |d: DefId| {
            if d == known.list {
                return vec![1];
            }
            if d == known.map {
                return vec![0, 1];
            }
            self.cx
                .lookup
                .item(d)
                .map(|i| i.generics.iter().map(|g| g.variance).collect())
                .unwrap_or_default()
        };
        let mut seen = vec![hd_resolve::variance::Seen::default(); generics.len()];
        for (a, v) in global.list_items(args).iter().copied().zip(&declared) {
            hd_resolve::variance::walk(global, a, *v, owner, &var_of, &mut seen);
        }
        let derived: Vec<i8> = seen.iter().map(|s| s.derived()).collect();
        let mut found = vec![hd_resolve::variance::Seen::default(); generics.len()];
        hd_resolve::variance::walk(global, ret, 1, owner, &var_of, &mut found);
        for ((g, marker), seen) in generics.iter().zip(&derived).zip(&found) {
            if seen.breaks(*marker) {
                let msg = format!(
                    "`{}` is declared {}, but it occurs in a {} position here",
                    self.cx.names.text(g.name),
                    if *marker > 0 {
                        "covariant (`+`)"
                    } else {
                        "contravariant (`-`)"
                    },
                    if *marker > 0 {
                        "negative or invariant"
                    } else {
                        "positive or invariant"
                    }
                );
                self.err(Code::InvalidVariance, node, &msg);
                return;
            }
        }
    }

    /// Finishes the body and returns it with `ret` resolved: a function's
    /// result type, inferred when its declaration omits it. Its row facts
    /// are resolved too.
    fn finish_with(
        mut self,
        root: Ref,
        ret: Ty,
        fun: Option<NodeRef<'_>>,
    ) -> StageResult<(Body, Ty, crate::results::RowFacts)> {
        let bindings = self.finish_bindings()?;
        self.report_conflicts();
        if self.module_init.is_none() {
            let item = self.b.body_mut().item;
            let facts = std::mem::take(&mut self.facts);
            self.cx.init.borrow_mut().facts.insert(item, facts);
        }
        let pool = self.pool();
        // Literal defaults first, so `x := +0` then `x = y` resolves both.
        let n = self.b.body_mut().ty.len();
        for i in 0..self.b.body_mut().consts.len() {
            let t = self.b.body_mut().consts[i].0;
            let z = self.zonk(t);
            self.b.body_mut().consts[i].0 = z;
        }
        // A tail expression's call parameters are decided here.
        self.close_open_params(0);
        self.close_ref_params(0);
        // A parameter only `never` arguments met is `never`
        // (`types.lct.never.all`); its bounds then fail like any other's.
        for v in std::mem::take(&mut self.never_args) {
            if matches!(pool.get(self.infer.shallow(pool, v)), TyData::Infer(_)) {
                let _ = self.infer.unify(pool, v, Ty::NEVER);
            }
        }
        // Goals that waited for inference, with the defaults in. A bound's
        // evidence is chosen again per instance at collection. What one
        // call goal learns may decide another, so a pass that settled any
        // is followed by another; what still waits then is left to the
        // types that stayed unknown (`cannot-infer-type`).
        let mut defaults = Defaults::Receiver;
        while !self.pending.is_empty() {
            let mut settled = false;
            for g in std::mem::take(&mut self.pending) {
                if self.settle_goal(g, defaults, true)? {
                    settled = true;
                } else {
                    self.pending.push(g);
                }
            }
            if !settled {
                if defaults == Defaults::All {
                    break;
                }
                defaults = Defaults::All;
            }
        }
        for g in std::mem::take(&mut self.pending) {
            if let Waiting::Call { method } = g.kind {
                self.ambiguous_call(g, method)?;
            }
        }
        self.close_call_params(0);
        self.close_assoc_self();
        self.opt_in_obligations()?;
        self.tuple_plan_steps()?;
        let open = self.b.body_mut().ty.clone();
        for i in 0..n {
            let t = self.b.body_mut().ty[i];
            let z = self.zonk(t);
            self.b.body_mut().ty[i] = z;
        }
        for i in 0..self.b.body_mut().local_ty.len() {
            let t = self.b.body_mut().local_ty[i];
            let z = self.zonk(t);
            self.b.body_mut().local_ty[i] = z;
        }
        self.fallback_rows(&open);
        let ret = self.zonk(ret);
        for i in 0..n {
            let tag = self.b.body_mut().tags[i];
            if tag != Tag::Call && tag != Tag::Await {
                continue;
            }
            let at = self.b.body_mut().data[i][0] as usize + 1;
            let words = self
                .b
                .body_mut()
                .record(u32::try_from(at - 1).expect("extra"))
                .to_vec();
            let Some(c) = Callee::from_words(&words) else {
                continue;
            };
            let z = match c {
                Callee::Item { def, targs } => Callee::Item {
                    def,
                    targs: self.zonk_list(targs),
                },
                Callee::TraitMethod {
                    trait_,
                    method,
                    self_ty,
                    targs,
                    choice,
                } => {
                    let self_ty = self.zonk(self_ty);
                    let targs = self.zonk_list(targs);
                    // The impls known at the call (`trait.impl.local.lookup`).
                    let syn = self.b.body_mut().syn[i];
                    if syn != hd_base::NodeIdx::NONE {
                        let node = self.cx.src.parse.tree.node(syn);
                        self.move_to(self.cx.src.span(node).lo);
                    }
                    let choice = self.settle_choice(trait_, self_ty, targs, choice)?;
                    Callee::TraitMethod {
                        trait_,
                        method,
                        self_ty,
                        targs,
                        choice,
                    }
                }
            };
            let nw = z.words();
            self.b.body_mut().extra[at..at + nw.len()].copy_from_slice(&nw);
        }
        // A generic local function's instantiation takes the captures of
        // its defining closure, known once that closure is closed
        // (`sub_flags::GENERIC`).
        {
            let body = self.b.body_mut();
            let mut caps = HashMap::default();
            for i in 0..n {
                if body.tags[i] == Tag::Closure && !body.is_instantiation(i) {
                    let r = body.record(body.data[i][1]);
                    caps.insert(body.data[i][0], (r[0], r[1]));
                }
            }
            for i in 0..n {
                if body.is_instantiation(i)
                    && let Some(&(start, len)) = caps.get(&body.data[i][0])
                {
                    let at = body.data[i][1] as usize;
                    body.extra[at + 1] = start;
                    body.extra[at + 2] = len;
                }
            }
        }
        // Types and type lists kept in records.
        for i in 0..n {
            let tag = self.b.body_mut().tags[i];
            let [a, bw] = self.b.body_mut().data[i];
            let list_at = match tag {
                Tag::ProviderGet => {
                    let at = a as usize + 1;
                    let t = Ty(self.b.body_mut().extra[at]);
                    self.b.body_mut().extra[at] = self.zonk(t).0;
                    continue;
                }
                Tag::With | Tag::ContextNew => {
                    let len = self.b.body_mut().extra[a as usize] as usize;
                    for k in 0..len / 2 {
                        let at = a as usize + 1 + 2 * k;
                        let t = Ty(self.b.body_mut().extra[at]);
                        self.b.body_mut().extra[at] = self.zonk(t).0;
                    }
                    continue;
                }
                Tag::ItemRef => bw as usize + 1,
                Tag::DefaultCall => a as usize + 2,
                Tag::Closure if self.b.body_mut().is_instantiation(i) => bw as usize + 4,
                _ => continue,
            };
            let l = TyList(self.b.body_mut().extra[list_at]);
            self.b.body_mut().extra[list_at] = self.zonk_list(l).0;
        }
        // A global place root's type: an init stores its bindings before
        // their types are settled.
        for at in hd_tir::ir::global_place_types(self.b.body_mut()) {
            let t = Ty(self.b.body_mut().extra[at]);
            self.b.body_mut().extra[at] = self.zonk(t).0;
        }
        if !self.diags.has_errors()
            && !self.cx.results.borrow().has_errors()
            && !self.read_poison_name
        {
            for i in 0..n {
                let t = self.b.body_mut().ty[i];
                let at = self.b.body_mut().syn[i];
                if pool.has_poison(t)
                    && self.b.body_mut().tags[i] != Tag::Poison
                    && at != hd_base::NodeIdx::NONE
                {
                    let node = self.cx.src.parse.tree.node(at);
                    let span = self.cx.src.span(node);
                    if self
                        .absorbed
                        .iter()
                        .any(|a| a.file == span.file && a.lo <= span.lo && span.hi <= a.hi)
                    {
                        continue;
                    }
                    self.err(Code::CannotInferType, node, "annotate this value's type");
                    break;
                }
            }
        }
        self.check_negated_unsigned();
        self.check_literal_ranges();
        self.check_local_ref_types();
        if let Some(node) = fun {
            self.check_inferred_result(node, ret);
        }
        // Capture modes (`crates/hd_tir/DESIGN.md` "Data and closures"): `Copy`
        // when no side assigns the local after its declaration or changes
        // its collection in place, `Shared` otherwise. `Move` needs the
        // liveness pass and is not chosen yet.
        let modes: Vec<hd_tir::ir::CaptureMode> = {
            let changed = std::mem::take(&mut self.changed_locals);
            let body = self.b.body_mut();
            body.cap_local
                .iter()
                .map(|l| {
                    let f = body.local_flags[l.idx()];
                    if f & (local_flags::CAPTURED_ASSIGNED | local_flags::MUTATED) != 0
                        || changed.contains(l)
                    {
                        hd_tir::ir::CaptureMode::Shared
                    } else {
                        hd_tir::ir::CaptureMode::Copy
                    }
                })
                .collect()
        };
        let facts = self.row_facts();
        let waits = self.row_waits();
        if !waits.is_empty() {
            let item = self.b.body_mut().item;
            crate::results::add_waits(self.cx, item, waits);
        }
        // A reported mismatch may leave a value of another type behind
        // (`crates/hd_tir/DESIGN.md` §4.13.11, invariant 4).
        let errors = self.diags.has_errors();
        let tolerated: &[u8] = if errors { &[4] } else { &[] };
        // The backend sees `&T` and `&mut T` as the one data type `Ref[T]`
        // (`refs.rs`).
        self.lower_body_refs();
        // Every place starts at its local (`docs/compiler/tir-v2.md`,
        // stage 2 commit 12a). The walkers above read the base reads, so
        // the places are rooted only now. The rebuild renumbers the body,
        // its root block included.
        let pool = self.pool();
        let body = self.b.body_mut();
        let item = body.item;
        let open = |t: Ty| hd_tir::ir::erasable(pool, item, t);
        body.sub_root[0] = root.0;
        ROOTS.with(|r| hd_tir::ir::root_places(body, &open, &mut r.borrow_mut()));
        let root = Ref(body.sub_root[0]);
        let known = VerifyFacts { cx: self.cx };
        let body = self
            .b
            .finish_in(root, &modes, tolerated, &known)
            .map_err(|e| VerifyError::bug(Stage::Body, "TIR verifier", &e))?;
        if !self.diags.has_errors()
            && let Some(sink) = &self.cx.source_facts
        {
            let mut sink = sink.borrow_mut();
            sink.comparisons.extend(self.comparisons);
            sink.bindings.extend(bindings);
            sink.patterns.extend(self.variant_patterns);
            sink.statements.extend(self.statements);
            sink.parameter_writes.extend(self.parameter_writes);
            sink.discarded.extend(self.discarded);
        }
        Ok((body, ret, facts))
    }
}

thread_local! {
    /// The rooting step's tables, reused across the bodies a thread checks.
    static ROOTS: std::cell::RefCell<hd_tir::ir::RootTables> =
        std::cell::RefCell::new(hd_tir::ir::RootTables::default());
}

/// What the TIR verifier reads from the checker (`hd_tir::ir::VerifyCx`):
/// the items and sealed traits a body names.
struct VerifyFacts<'x, 'a> {
    cx: &'x BodyCx<'a>,
}

impl VerifyFacts<'_, '_> {
    fn sig(&self, def: DefId) -> Option<&hd_resolve::FnSig> {
        match &self.cx.lookup.item(def)?.data {
            ItemData::Fn(sig) | ItemData::Method { sig, .. } => Some(sig),
            _ => None,
        }
    }
}

impl hd_tir::ir::VerifyCx for VerifyFacts<'_, '_> {
    fn variant_count(&self, ty: Ty) -> Option<u32> {
        match self.cx.names.pool.get(ty) {
            TyData::Mut(t) => self.variant_count(t),
            TyData::Option(_) => Some(2),
            TyData::Adt { def, .. } => match &self.cx.lookup.item(def)?.data {
                ItemData::Enum { variants, .. } => u32::try_from(variants.len()).ok(),
                _ => None,
            },
            _ => None,
        }
    }
    fn builtin_trait(&self, index: u32) -> Option<DefId> {
        use hd_types::solver::BuiltinImpl as B;
        let s = self.cx.names.known.sealed();
        [
            (B::Any, s.any),
            (B::AnyVal, s.any_val),
            (B::AnyRef, s.any_ref),
            (B::Inspectable, s.inspectable),
            (B::Tuple, s.tuple),
            (B::Structure, s.structure),
        ]
        .into_iter()
        .find(|(b, _)| *b as u32 == index)
        .map(|(_, t)| t)
    }
    fn method_bounds(&self, method: DefId) -> Option<u32> {
        let n: usize = self
            .sig(method)?
            .generics
            .iter()
            .map(|g| g.bounds.len())
            .sum();
        u32::try_from(n).ok()
    }
    fn row_keys(&self, callee: &hd_tir::ir::Callee) -> Option<u32> {
        let def = match callee {
            hd_tir::ir::Callee::Item { def, .. } => *def,
            hd_tir::ir::Callee::TraitMethod { method, .. } => *method,
        };
        let row = self.cx.names.pool.row_data(self.sig(def)?.row);
        if !row.params.is_empty() {
            return None;
        }
        u32::try_from(row.keys.len()).ok()
    }
}

/// The first number in a literal node's source text, converted straight
/// to `f32` (one rounding, not via `f64`).
fn float_text_f32(text: &str) -> Option<f32> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let number = crate::literals::split_suffix(&text[start..]).0;
    number.replace('_', "").parse::<f32>().ok()
}
