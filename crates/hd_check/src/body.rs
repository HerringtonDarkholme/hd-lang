//! Body checking (type-checking.md §1.4 to §1.6; checking-and-tir.md
//! §4.13): one function body over the full parser's tree, typed with an
//! `InferTable` whose variable types live in the body's `LocalPool`, trait goals through the
//! `Solver` with fuel, TIR written through `TirBuilder`. User errors go to
//! the `DiagBuf`; a construct the checker does not carry yet is a
//! structured "not implemented", which stops a build.
//!
//! Expressions are in `expr`, calls and member lookup in `call`, patterns
//! and `match` in `pat`, annotations in `ty`.

use std::collections::HashMap;

use hd_base::{DefId, Fuel, LocalId, ModuleId, NotImplemented, Stage, StageResult, Symbol};
use hd_diag::{Code, DiagBuf};
use hd_resolve::{ItemData, Lookup, LookupDecls, ModuleScope, Names, Src};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::Body;
use hd_tir::ir::{
    BodyKind, Callee, Coercion, IntrinsicOp, LoopMark, NONE, Ref, SubMark, Tag, TirBuilder,
    TirSink, local_flags,
};
use hd_types::solver::{
    Answer, BodyMemo, Evidence, GlobalMemo, Goal, ImplTable, ImplView, Impls, ParamEnv, SolveCx,
    Solver, TraitRef,
};
use hd_types::{InferTable, ParamRef, RowId, Ty, TyData, TyList, VarKind};

use crate::access::FieldKind;

/// What a body sees: the run's tables, its module's scope, the items of
/// its closure and its view of the impls (owner tables and its impl
/// universe, trait-solver.md §3.2).
pub struct BodyCx<'a> {
    pub names: Names<'a>,
    pub src: Src<'a>,
    pub scope: &'a ModuleScope,
    pub lookup: &'a Lookup<'a>,
    pub impls: &'a ImplView<'a>,
    pub global: &'a GlobalMemo,
    pub solver: &'a dyn Solver,
    /// The methods of the closure by name, built on first use.
    pub methods: std::cell::OnceCell<crate::MethodIndex>,
    /// The module's top-level bindings and its bodies' init facts.
    pub init: std::cell::RefCell<crate::init::ModuleInit>,
    /// The module's private callables with omitted result types (M1).
    pub results: std::cell::RefCell<crate::results::Results>,
    /// Every reference to `dbg` the module's bodies make, which a release
    /// build rejects in the user's own code (`module.dbg.release`).
    pub dbg_refs: std::cell::RefCell<Vec<crate::dbg::DbgRef>>,
    /// The module's block-local declarations, lifted to hidden items
    /// (checking-and-tir.md "Local items lift to hidden module items").
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
    /// (trait-solver.md §3.9).
    pub(crate) fn decls(&self) -> LookupDecls<'_> {
        LookupDecls {
            lookup: self.lookup,
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

/// An open closure: its builder mark, the scope depth at its start, its
/// result type.
/// One level of the available-keys stack (type-checking.md §5.2).
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
    /// its calls of rows still open (checking-and-tir.md "M3: inferred
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

pub(crate) struct OpenSub {
    pub mark: SubMark,
    pub depth: usize,
}

pub(crate) struct Ck<'a, 'c> {
    pub cx: &'c BodyCx<'a>,
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
    /// Type parameters by name: the impl's, then the function's.
    pub gens: Vec<(Symbol, Ty)>,
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
    /// A member call's explicit method type arguments (`x.f::[T]()`),
    /// taken by the method's instantiation.
    pub method_targs: Vec<Ty>,
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
    pub uninit: std::collections::HashSet<LocalId>,
    /// The `:=` bindings declared so far, with their scope's depth.
    pub bound: Vec<(LocalId, usize)>,
    /// The statement-form `:=` binding being checked: the one `ExprStmt`
    /// child that may grant its closure a self-name
    /// (`fn.recursion.closure.binding`). A chain member is not it.
    pub stmt_binding: Option<hd_base::NodeIdx>,
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
    pub later_names: std::collections::HashMap<hd_base::Symbol, usize>,
    /// A REPL input that is one expression: that statement, then its
    /// value and type once checked (`cli.repl.value`).
    pub shown: Option<(hd_base::NodeIdx, Option<(Ref, Ty)>)>,
    /// What this body reads and calls (module initialization).
    pub facts: crate::init::InitFacts,
    /// A pipe's value while its step is checked (`_`).
    pub placeholder: Option<(Ref, Ty)>,
    /// A bare pipe step's piped value, which stands for the argument node
    /// at this index (`expr.pipe.bare.call`).
    pub pipe_arg: Option<(hd_base::NodeIdx, (Ref, Ty))>,
    /// Checking the call a bare pipe step stands for.
    pub bare_step: bool,
    /// The next `for` loop's `else` suite and the loop's result type.
    pub loop_else: Option<(NodeRefIdx, Ty)>,
    /// The function body block, for `missing-return-value`.
    pub fn_body: Option<NodeRefIdx>,
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
    /// The locals the source binds by name, for `unused-local-binding`.
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
    /// trials (type-checking.md §2.5 "Trial cost"): node, value, type.
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
    /// Checking a template's method at a derivation opt-in: the opt-in
    /// (`annot.template.checked`).
    pub opt_in: Option<&'c crate::derive::OptIn>,
    /// The structure protocol calls of an opt-in's template body: the
    /// `Structure` method and its walker, describer or source type.
    pub structure_calls: Vec<(DefId, Ty)>,
    /// Checking a tuple template's method for its plan: where its member
    /// steps go (trait-solver.md §3.9).
    pub plan_sink: Option<&'c std::cell::RefCell<Vec<hd_types::solver::MemberStep>>>,
    /// The least-common-type sites being joined (`types.lct.sites`), by
    /// join variable, with the values each has joined.
    pub joins: crate::join::Joins,
    /// The join variables of the sites now checking their branches that
    /// can restart with a wider type (`join_site`).
    pub restarts: Vec<Ty>,
    /// A join variable met an optional value of its solved inner type: the
    /// site must start over with the optional as its type (`types.lct.optional`).
    pub widen: Vec<(Ty, Ty)>,
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
    /// A bound: only whether it holds, checked at the end of the body.
    Bound,
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
        b: TirBuilder::new(item, kind),
        infer: InferTable::default(),
        scopes: vec![HashMap::new()],
        env: ParamEnv::default(),
        loops: Vec::new(),
        diags,
        fuel: Fuel::new(Fuel::BODY_DEFAULT),
        memo: BodyMemo::default(),
        rets: vec![ret],
        gens: Vec::new(),
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
        method_targs: Vec::new(),
        in_explicit_targs: false,
        default_owner: None,
        local_defaults: None,
        local_forms: HashMap::new(),
        suspends: vec![false],
        defer_base: None,
        defer_subs: 0,
        uninit: std::collections::HashSet::new(),
        bound: Vec::new(),
        stmt_binding: None,
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
        later_names: std::collections::HashMap::new(),
        shown: None,
        facts: crate::init::InitFacts::default(),
        placeholder: None,
        pipe_arg: None,
        bare_step: false,
        loop_else: None,
        fn_body: None,
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
        opt_in: None,
        structure_calls: Vec::new(),
        plan_sink: None,
        joins: crate::join::Joins::default(),
        restarts: Vec::new(),
        widen: Vec::new(),
        at: 0,
        hidden: Vec::new(),
        module: cx.names.module_node(item),
        registering: false,
        row_waits: Vec::new(),
        local_generics: HashMap::new(),
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
                let own: Vec<Ty> = (1..=o.generics.len())
                    .map(|i| {
                        pool.intern_ty(&TyData::Param(ParamRef {
                            owner: *owner,
                            index: u16::try_from(i).unwrap_or(u16::MAX),
                        }))
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

/// Elaborates one declared bound into an environment (trait-solver.md
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
/// (trait-solver.md §3.9). Diagnostics go to `diags`.
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
/// key it uses (`req.row.omitted.inferred-private`).
pub(crate) fn check_fn_body(
    cx: &BodyCx<'_>,
    def: DefId,
    node: NodeRef<'_>,
    diags: &mut DiagBuf,
    (opt_in, plan_sink): (
        Option<&crate::derive::OptIn>,
        Option<&std::cell::RefCell<Vec<hd_types::solver::MemberStep>>>,
    ),
    infer: Infer,
) -> StageResult<(Body, Ty, crate::results::RowFacts)> {
    let Some(item) = cx.lookup.item(def) else {
        return unsupported(format!("body of {} has no header", cx.names.path(def)));
    };
    let Some(sig) = item.sig().cloned() else {
        return unsupported("a body of a non-function item");
    };
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
    ck.check_impl_method(def, node);
    ck.check_literal_marker(def, node);
    ck.check_row_patterns(&sig, node);
    ck.suspends = vec![sig.suspends];
    let blk = ck.b.open_block();
    // Projections in the signature normalize under the bounds, so
    // `I::Item` and `T` are one type under `I < Supplier[Item = T]`
    // (`trait.binding.interchangeable`).
    for (name, ty) in sig.params.clone() {
        let ty = ck.norm_ty(ty);
        let l = ck.b.local(ty, name, local_flags::PARAM, node.index());
        ck.scopes[0].insert(name, l);
    }
    let Some(body) = Src::child(node, SyntaxKind::Block) else {
        return unsupported("a function without a body");
    };
    ck.fn_body = Some(body.index());
    ck.written_row = Src::child(node, SyntaxKind::RequirementRow).map(NodeRef::index);
    // `types.lct.sites`: an omitted result type joins the final value and
    // the `return` operands, and starts over when an optional one widens
    // it (`join_site`).
    let want = (!infer.ret).then(|| ck.norm_ty(sig.ret));
    let (tail, ret) = ck.join_site(want, |ck, ret| {
        ck.rets[0] = ret;
        let (tail, _) = ck.block_value(body, Some(ret))?;
        Ok((tail, ret))
    })?;
    let root = ck.b.close_block(blk, tail, ret, body.index());
    ck.finish_with(root, ret)
}

/// The `DefId` of a default body: the declaration's path plus the field or
/// parameter name (checking-and-tir.md "Default calls").
#[must_use]
pub fn default_body_def(names: &Names<'_>, owner: DefId, name: &str) -> DefId {
    names.member(owner, hd_intern::PathKind::Hidden, name)
}

/// Checks one default expression as a body of kind `Default`
/// (type-checking.md §1.7): a data field's (no parameters) or a function
/// parameter's (the earlier parameters are its parameters).
pub fn check_default(
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
    let (r, t) = ck.expr(expr, Some(ty))?;
    let r = ck.coerce(r, t, ty, expr, "default");
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
            unsupported("the body's fuel ran out (limit diagnostic)")
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
        if w != g {
            return (w, g);
        }
        let pool = self.pool();
        let rows =
            |t: Ty| hd_resolve::show_ty_rows_in(&self.cx.names, pool, self.infer.resolve(pool, t));
        (rows(want), rows(got))
    }

    /// `unused-local-binding` (flow.unused.warning): a local the source
    /// binds and never reads; a name beginning with `_` is exempt.
    fn unused_locals(&mut self) {
        // One warning per line: a `let` pattern's unread names share it.
        let mut lines = std::collections::HashSet::new();
        for l in std::mem::take(&mut self.user_locals) {
            let body = self.b.body_mut();
            let (flags, name, at) = (
                body.local_flags[l.idx()],
                body.local_name[l.idx()],
                body.local_syn[l.idx()],
            );
            let text = self.cx.names.text(name);
            if flags & local_flags::READ != 0 {
                continue;
            }
            // flow.unused.must-use: an unread suspension binding is an
            // error, whatever its name; `Result` and `T?` stay warnings.
            let lt = self.b.local_ty(l);
            if self.must_use_in(lt, true) == Some(true) {
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
                continue;
            }
            if text.starts_with('_') {
                continue;
            }
            let msg = format!("`{text}` is never read; name it `_{text}` to keep it");
            let span = self.cx.src.span(self.cx.src.parse.tree.node(at));
            let source = self.cx.src.text;
            let lo = usize::try_from(span.lo).unwrap_or(0).min(source.len());
            if !lines.insert(source[..lo].rfind('\n')) {
                continue;
            }
            self.diags.push(
                Code::UnusedLocalBinding,
                hd_diag::Severity::Warning,
                span,
                &msg,
                None,
            );
        }
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

    /// `integer-literal-range`: an integer constant must fit its type.
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
                        f32::is_finite,
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
            let first = self.env.clause_self.len();
            for b in &g.bounds {
                self.add_bound(p, *b, 0);
            }
            // `T < mut Trait`: values of `T` have mutable access.
            if g.mut_bound {
                for m in &mut self.env.clause_mut[first..] {
                    *m = true;
                }
            }
        }
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
        // `names.definite.reject`: reported once per binding.
        if self.uninit.remove(&l) {
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
        let flags = &mut self.b.body_mut().local_flags[l.idx()];
        *flags |= local_flags::READ;
        if captured {
            *flags |= local_flags::CAPTURED;
        }
        let t = self.b.local_ty(l);
        if self.pool().has_poison(t) {
            self.read_poison_name = true;
        }
        (self.b.get(l, t, n.index()), t)
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
        let flags = &mut self.b.body_mut().local_flags[l.idx()];
        if captured {
            *flags |= local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED;
        } else if *flags & local_flags::ASSIGNED != 0 {
            *flags |= local_flags::MUTATED;
        }
        *flags |= local_flags::ASSIGNED;
    }

    pub(crate) fn sym_of(&self, n: NodeRef<'_>) -> Symbol {
        self.cx
            .names
            .syms
            .intern(self.cx.src.text(self.cx.src.first(n)))
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
        let in_tests = self.in_tests_block();
        let bound = (hd_resolve::is_prelude_name(text)
            && self.cx.scope_at(self.at).bound_from_outside(name))
            || (in_tests && hd_resolve::is_test_prelude_name(text));
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
    fn injects_into(&self, got: Ty, want: Ty) -> bool {
        self.option_layers(got) + 1 == self.option_layers(want)
    }

    /// Unifies `got` with `want`, inserting the coercions of
    /// type-checking.md §4.2 (`never`, `.Some` wrapping, trait values).
    pub(crate) fn coerce(&mut self, r: Ref, got: Ty, want: Ty, n: NodeRef<'_>, what: &str) -> Ref {
        let pool = self.pool();
        let target = want;
        let (got, want) = (self.norm_ty(got), self.norm_ty(want));
        let g = self.infer.shallow(pool, got);
        let w = self.infer.shallow(pool, want);
        if g == Ty::NEVER || w == Ty::NEVER || g == w {
            return r;
        }
        // A least-common-type site's variable is not a declared type: the
        // value joins the others instead (types.lct).
        if self.joins.is_site(target) {
            return self.join_value(r, got, target, n, what);
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
            if !self.check_upgrade(got, inner, n, what) {
                self.expect(got, inner, n, what);
            }
            return self.b.coerce(Coercion::WrapSome, NONE, r, want, n.index());
        }
        if let (TyData::TraitValue { def: to, .. }, TyData::TraitValue { def: from, .. }) =
            (pool.get(ws), pool.get(gs))
            && to != from
            && self.trait_extends(from, to, 0)
        {
            return self
                .b
                .coerce(Coercion::Supertrait, NONE, r, want, n.index());
        }
        if let TyData::TraitValue { def, args, .. } = pool.get(ws)
            && !open
            && !matches!(pool.get(gs), TyData::TraitValue { .. } | TyData::Poison)
        {
            // `types.literal.local.erased`: a literal converted to a trait
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
            // binding's type does not fit (`trait.dyn.convert`).
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
            return self.coerce(r, lowered, target, n, what);
        }
        let before = self.diags.len();
        self.expect(got, want, n, what);
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
    pub(crate) fn expect(&mut self, got: Ty, want: Ty, n: NodeRef<'_>, what: &str) -> bool {
        let pool = self.pool();
        if got == Ty::NEVER || want == Ty::NEVER {
            return false;
        }
        let (got, want) = (self.norm_ty(got), self.norm_ty(want));
        let snap = self.infer.snapshot();
        if self.infer.unify(pool, got, want).is_err() {
            self.infer.rollback(snap);
            // `types.num.narrowing`: a value of a wider type of the same
            // numeric family where a narrower one is expected.
            if !matches!(what, "operand" | "range bound") && self.narrows(got, want) {
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
            let msg = format!("in {what}: expected {want_s}, found {got_s}");
            // `types.num.binary.no-mix`, `expr.range.bound.signedness`: a
            // signed and an unsigned integer operand do not mix.
            let code =
                if matches!(what, "operand" | "range bound") && self.mixes_signedness(got, want) {
                    Code::MixedSignedness
                } else {
                    Code::TypeMismatch
                };
            self.err(code, n, &msg);
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
        let pool = self.pool();
        let prim = |t: Ty| {
            let t = self.strip_mut(self.infer.resolve(pool, t));
            match pool.get(t) {
                TyData::Prim(p) => Some(p),
                _ => None,
            }
        };
        let (Some(g), Some(w)) = (prim(got), prim(want)) else {
            return false;
        };
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
        (g.is_integer() || g.is_float())
            && (w.is_integer() || w.is_float())
            && family(g) == family(w)
            && width(w) < width(g)
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
        let pair = |this: &mut Self, from: Ty, to: Ty| -> Option<bool> {
            if this.can_unify(from, to) {
                return Some(false);
            }
            if this.elem_converts(from, to) {
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
        match pool.get(to) {
            TyData::TraitValue { def, args, .. }
                if !matches!(pool.get(from), TyData::TraitValue { .. } | TyData::Poison) =>
            {
                let tref = TraitRef {
                    trait_: def,
                    self_ty: from,
                    args,
                };
                matches!(self.solve(tref), Ok(Answer::Holds { .. }))
            }
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

    /// A concrete value of type `got` converts to the trait value type
    /// `want` only when its implementation binds each associated type to
    /// the bound type (trait.dyn.binding.convert): otherwise the vtable's
    /// slots would read another type than the value's methods return.
    fn check_value_bindings(&mut self, got: Ty, want: Ty, n: NodeRef<'_>) {
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

    /// [`Self::require_ref`], reporting a failure with `code`.
    pub(crate) fn require_ref_as(
        &mut self,
        tref: TraitRef,
        at: NodeRef<'_>,
        code: Code,
    ) -> StageResult<Option<hd_types::solver::Evidence>> {
        let pool = self.pool();
        // Projections with a known base normalize before the goal is
        // solved (trait-solver.md §4.3, point 3).
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
                let msg = format!(
                    "{} does not implement {}",
                    self.show(tref.self_ty),
                    self.cx.names.display_name(tref.trait_)
                );
                self.err(code, at, &msg);
                Ok(None)
            }
            Answer::Stalled { .. } => {
                self.pending.push(PendingGoal {
                    tref,
                    at: at.index(),
                    kind: Waiting::Bound,
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
            Answer::OutOfFuel => unsupported("the solver's fuel ran out (limit diagnostic)"),
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
        self.cx.solver.solve(&mut scx, goal, &mut self.fuel)
    }

    /// The evidence of a goal that a compiler-supplied row (trait-solver.md
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
        self.scopes.push(HashMap::new());
        // The suite's local declarations end with it: the rest of the
        // enclosing statement is back where it started.
        let at = self.at;
        // A suite with `defer` is a cleanup scope (`flow.defer.scopes`):
        // a `Scope` over its statements, whose suites are its `Defer`s.
        let r = if block.children().any(|c| c.kind() == SyntaxKind::DeferStmt) {
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
        // is still checked (`flow.unreachable.checked`); only the warning
        // (`flow.unreachable.warning`) is new.
        let mut dead = false;
        for (i, s) in stmts.iter().copied().enumerate() {
            let last = i + 1 == stmts.len();
            self.move_to(self.cx.src.span(s).lo);
            if dead {
                self.diags.push(
                    Code::UnreachableCode,
                    hd_diag::Severity::Warning,
                    self.cx.src.span(s),
                    "this statement is unreachable",
                    None,
                );
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
                    Some(w) => self.coerce(r, t, w, e, "result"),
                    None => r,
                };
                tail = Some(r);
                ty = want.unwrap_or(t);
                continue;
            }
            let t = self.stmt(s)?;
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
    pub(crate) fn stmt(&mut self, s: NodeRef<'_>) -> StageResult<Ty> {
        let span = self.cx.src.span(s);
        let start = self.infer.var_count();
        let open = self.open_params.len();
        let refs = self.ref_params.len();
        let waiting = self.pending.len();
        let r = self.stmt_node(s).map_err(|e| e.at(span));
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

    /// A call whose receiver is known at the end of the body but whose
    /// impl still waits: several instantiations of its trait fit it
    /// (`trait.resolve.many-apply`).
    fn ambiguous_call(&mut self, g: PendingGoal, method: DefId) -> StageResult<()> {
        let pool = self.pool();
        let self_ty = self.infer.resolve(pool, g.tref.self_ty);
        let goal = Goal::Instantiations {
            self_ty,
            trait_: g.tref.trait_,
            mut_: false,
        };
        let shown = match self.solve_goal(&goal)? {
            Answer::Candidates(c) => c
                .iter()
                .map(|c| {
                    self.show(pool.intern_ty(&TyData::TraitValue {
                        def: g.tref.trait_,
                        args: c.args,
                        bindings: vec![],
                    }))
                })
                .collect::<Vec<_>>()
                .join(", "),
            _ => self.cx.names.display_name(g.tref.trait_).to_string(),
        };
        let name = self
            .cx
            .lookup
            .item(method)
            .map_or("", |i| self.cx.names.text(i.name));
        let msg = format!(
            "{}: {shown} fit this call: write `{}::[..]::{name}(..)` to choose one",
            self.show(self_ty),
            self.cx.names.display_name(g.tref.trait_),
        );
        let node = self.cx.src.parse.tree.node(g.at);
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
            Waiting::Bound => false,
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
                Waiting::Bound => {}
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
            .is_some_and(|p| p.iter().any(|g| g.kind != Waiting::Bound))
        {
            return Ok(());
        }
        let here = self.at;
        let goals = self.pending.split_off(waiting);
        for g in goals {
            if g.kind == Waiting::Bound || !self.settle_goal(g, Defaults::None, true)? {
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
            let args = if g.kind == Waiting::Bound {
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
                if g.kind == Waiting::Bound {
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
                if report && !matches!(g.kind, Waiting::Project { .. }) {
                    let msg = format!(
                        "{} does not implement {}",
                        self.show(tref.self_ty),
                        self.cx.names.display_name(tref.trait_)
                    );
                    self.err(Code::UnsatisfiedTraitBound, node, &msg);
                }
                Ok(true)
            }
            Answer::Stalled { .. } => Ok(false),
            Answer::OutOfFuel => unsupported("the solver's fuel ran out (limit diagnostic)"),
            other => unsupported(format!("solver answer {other:?}")),
        }
    }

    /// `types.literal.local.class.open`: a literal class that met no type
    /// by the end of its statement takes its default type. A class that
    /// reached the result join of an open function or closure stays open
    /// until the join (`types.literal.local.form.join-open`): its return
    /// paths join across statements
    /// (`types.literal.local.form.function-return.statements`).
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
            .map(|r| self.infer.shallow(pool, *r))
            .collect();
        for (t, k) in self.infer.open_literals_since(pool, start) {
            let root = self.infer.shallow(pool, t);
            if joins.contains(&root) || held.contains(&root) {
                continue;
            }
            if let Some(d) = k.literal_default() {
                let _ = self.infer.unify(pool, t, d);
            }
        }
    }

    /// `module.dbg.debug-hint`: whether `e` is a call of the prelude
    /// function `debug`, by declaration identity and not by spelling.
    fn is_prelude_debug_call(&self, e: NodeRef<'_>) -> bool {
        let call = match e.kind() {
            SyntaxKind::CallExpr => e,
            SyntaxKind::TrailingCallExpr => {
                match e.children().find(|c| c.kind() == SyntaxKind::CallExpr) {
                    Some(c) => c,
                    None => return false,
                }
            }
            _ => return false,
        };
        let Some(mut callee) = call.children().next() else {
            return false;
        };
        if callee.kind() == SyntaxKind::TypeArgsExpr {
            let Some(inner) = callee.children().next() else {
                return false;
            };
            callee = inner;
        }
        if callee.kind() == SyntaxKind::SuspendExpr {
            let Some(inner) = callee.children().next() else {
                return false;
            };
            callee = inner;
        }
        let debug_def = self.cx.names.item("std.format", "debug");
        let at = self.cx.src.span(callee).lo;
        match callee.kind() {
            SyntaxKind::NameExpr => {
                let sym = self
                    .cx
                    .names
                    .syms
                    .intern(self.cx.src.text(self.cx.src.first(callee)));
                if self.find_local(sym).is_some() {
                    return false;
                }
                if hd_resolve::local_at(self.cx.locals, sym, at).is_some() {
                    return false;
                }
                let scope = self.cx.scope_at(at);
                match scope.lookup(sym) {
                    Some(b) if b.kind == hd_resolve::BindingKind::Item => {
                        DefId::from_raw(b.value) == debug_def
                    }
                    _ => false,
                }
            }
            SyntaxKind::FieldExpr | SyntaxKind::PathExpr => {
                let segs = self.segments_expr(callee);
                match self.resolve_path(&segs) {
                    Some(def) => def == debug_def,
                    None => false,
                }
            }
            _ => false,
        }
    }

    fn stmt_node(&mut self, s: NodeRef<'_>) -> StageResult<Ty> {
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
                let (_, t) = self.expr(*e, Some(Ty::VOID))?;
                let t = self.infer.shallow(self.pool(), t);
                if self.must_use(t) {
                    let msg = format!(
                        "a {} value is discarded; handle it or write `_ := ...`",
                        self.show(t)
                    );
                    self.err(Code::DiscardedMustUseValue, *e, &msg);
                }
                // `module.dbg.debug-hint`: an expression statement that
                // calls the prelude function `debug` discards its text.
                if self.is_prelude_debug_call(*e) {
                    let span = self.cx.src.span(*e);
                    self.diags.push(
                        Code::UnusedDebugText,
                        hd_diag::Severity::Warning,
                        span,
                        "a `debug(...)` statement discards its text; use `dbg(...)` to print it",
                        None,
                    );
                }
                return Ok(if t == Ty::NEVER { Ty::NEVER } else { Ty::VOID });
            }
            SyntaxKind::LetStmt => self.let_stmt(s, &kids)?,
            SyntaxKind::DiscardStmt => {
                if let Some(e) = kids.last() {
                    self.expr(*e, None)?;
                }
            }
            SyntaxKind::AssignmentStmt => self.assign(s, &kids)?,
            SyntaxKind::ReturnStmt => {
                if self.defer_base.is_some() {
                    self.err(Code::DeferControlFlow, s, "a `defer` suite cannot `return`");
                }
                let ret = *self.rets.last().expect("ret");
                let r = if let Some(e) = kids.first() {
                    let (r, t) = self.expr(*e, Some(ret))?;
                    self.coerce(r, t, ret, *e, "return")
                } else {
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
                        self.coerce(r, t, want, *e, "break value").0
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

    /// `x = e`, `x op= e`, `a.f = e`, `a[i] = e`, `v() = e`.
    fn assign(&mut self, s: NodeRef<'_>, kids: &[NodeRef<'_>]) -> StageResult<()> {
        let [lhs, rhs] = kids else {
            return unsupported("this assignment form");
        };
        let op_tok = hd_base::TokenIdx::from_raw(self.cx.src.last(*lhs).raw() + 1);
        let op = self.cx.src.tkind(op_tok);
        let copy = op == Some(TokenKind::EllipsisEq);
        let compound = match op {
            Some(TokenKind::Eq | TokenKind::EllipsisEq) => None,
            Some(k) => Some(k),
            None => return unsupported("an assignment operator"),
        };
        // `grammar.stmt.copy-assign`, `data.part.marker-required`: `...=`
        // stores into an embedded field and into no other place.
        if copy
            && matches!(
                lhs.kind(),
                SyntaxKind::NameExpr | SyntaxKind::IndexExpr | SyntaxKind::CallExpr
            )
        {
            self.err(
                Code::CopyIntoOrdinaryField,
                *lhs,
                "`...=` stores into an embedded field; use `=` for this place",
            );
            return Ok(());
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
                    && let Some(crate::init::InputBinding { def, ty, short }) = global
                {
                    // A mutable top-level binding (`GlobalSet`).
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
                    let v = self.coerce(r, t, ty, *rhs, "assignment");
                    let a = self.b.refs_record(&[Ref(def.raw())]);
                    self.b.emit(Tag::GlobalSet, a, v.0, Ty::VOID, s.index());
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
                if flags & local_flags::SHORT != 0 {
                    let msg = format!(
                        "`{}` is bound with `:=`; use `let` to reassign it",
                        self.cx.names.text(name)
                    );
                    self.err(Code::NonReassignableBinding, *lhs, &msg);
                } else if flags & local_flags::PARAM != 0 {
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
                        self.coerce(r, t, lt, *rhs, "assignment")
                    }
                    Some(k) => {
                        let (cur, _) = self.read_local(l, depth, *lhs);
                        self.compound(k, cur, lt, *rhs, s)?
                    }
                };
                self.note_write(l, depth);
                self.b.set(l, v, s.index());
            }
            SyntaxKind::FieldExpr => {
                let Some(base) = lhs.children().next() else {
                    return unsupported("a field without a base");
                };
                let (br, bt) = self.expr(base, None)?;
                let fname = self.cx.src.text(self.cx.src.last(*lhs)).to_owned();
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
                    self.err(Code::UnknownDataField, *lhs, &msg);
                    return Ok(());
                };
                self.check_field_visible(pt, &fname, *lhs);
                let embedded = self.field_kind(pt, &fname) == Some(FieldKind::Embedded);
                // `data.part.marker-required`: a part is stored with `...=`
                // and no other place takes it.
                if copy && !embedded {
                    let msg = format!(
                        "`{fname}` is not an embedded field, so `...=` cannot store into it"
                    );
                    self.err(Code::CopyIntoOrdinaryField, *lhs, &msg);
                    return Ok(());
                }
                if !copy && compound.is_none() && embedded {
                    let msg = format!("`{fname}` is an embedded field; store it with `...=`");
                    self.err(Code::EmbeddedCopyRequired, *lhs, &msg);
                    return Ok(());
                }
                self.check_store_target(base, (br, bt), *lhs);
                let br = pr;
                let v = match compound {
                    None if copy => {
                        // `data.part.construct`, `types.path.store-embedded`:
                        // stores the copy-update `T { ...value }`, which must
                        // have the mutable access of the part.
                        let (r, t) = self.expr(*rhs, Some(ft))?;
                        let (cr, ct) = self.copy_value((r, t), *rhs);
                        let want = self.field_access(pt, &fname, ft);
                        self.check_upgrade(ct, want, *rhs, "assignment");
                        self.coerce(cr, ct, ft, *rhs, "assignment")
                    }
                    None => {
                        let (r, t) = self.expr(*rhs, Some(ft))?;
                        self.coerce(r, t, ft, *rhs, "assignment")
                    }
                    Some(k) => {
                        let cur = self.b.emit(Tag::Field, br.0, idx, ft, lhs.index());
                        self.compound(k, cur, ft, *rhs, s)?
                    }
                };
                let rec = self.b.refs_record(&[Ref(idx), v]);
                self.b.emit(Tag::FieldSet, br.0, rec, Ty::VOID, s.index());
            }
            SyntaxKind::IndexExpr => {
                let ik: Vec<NodeRef<'_>> = lhs.children().collect();
                let [base, key] = ik.as_slice() else {
                    return unsupported("an index target shape");
                };
                let (br, bt) = self.expr(*base, None)?;
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
                let Some((op_get, op_set, kt, vt)) = builtin else {
                    return self.index_set((br, bt), (*base, *key), *rhs, compound, (*lhs, s));
                };
                let (kr, key_ty) = self.index_key_value(*key, builtin)?;
                // `expr.index.slice.no-store`: no range type stores into a
                // list.
                if op_get != IntrinsicOp::MapIndex && self.is_range_ty(key_ty) {
                    self.err(
                        Code::InvalidAssignmentTarget,
                        *lhs,
                        "this index cannot be assigned",
                    );
                    return Ok(());
                }
                self.check_store_target(*base, (br, bt), *lhs);
                let kr = self.index_key((kr, key_ty), *key, kt);
                let v = match compound {
                    None => {
                        let (r, t) = self.expr(*rhs, Some(vt))?;
                        self.coerce(r, t, vt, *rhs, "assignment")
                    }
                    Some(k) => {
                        let rec = self.b.refs_record(&[br, kr]);
                        let cur = self
                            .b
                            .emit(Tag::Intrinsic, op_get as u32, rec, vt, lhs.index());
                        self.compound(k, cur, vt, *rhs, s)?
                    }
                };
                let rec = self.b.refs_record(&[br, kr, v]);
                self.b
                    .emit(Tag::Intrinsic, op_set as u32, rec, Ty::VOID, s.index());
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

    /// `r[k] = v` and `r[k] op= v` on a receiver without built-in
    /// indexing: `IndexSet::[K, V]::index_set(r, k, v)`, after
    /// `Index::[K]::index(r, k)` for a compound one
    /// (`expr.index.trait.write`).
    fn index_set(
        &mut self,
        (br, bt): (Ref, Ty),
        (base, key): (NodeRef<'_>, NodeRef<'_>),
        rhs: NodeRef<'_>,
        compound: Option<TokenKind>,
        (lhs, s): (NodeRef<'_>, NodeRef<'_>),
    ) -> StageResult<()> {
        let (kr, kt) = self.expr(key, None)?;
        let set = self.cx.names.known.index_set;
        if !self.op_fits(set, bt, Some(kt))? {
            self.err(
                Code::InvalidAssignmentTarget,
                lhs,
                &format!("{} does not implement `IndexSet`", self.show(bt)),
            );
            return Ok(());
        }
        self.check_store_target(base, (br, bt), lhs);
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
                let (cur, ct) =
                    self.trait_call_args(index, "index", br, bt, &[(kr, kt, key)], lhs)?;
                let v = self.compound(k, cur, ct, rhs, s)?;
                (v, self.b.ty_of(v))
            }
        };
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
    /// other constraint take their default type (type-checking.md §3.6).
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

    fn finish(self, root: Ref) -> StageResult<Body> {
        let ret = self.rets[0];
        self.finish_with(root, ret).map(|(b, ..)| b)
    }

    /// Finishes the body and returns it with `ret` resolved: a function's
    /// result type, inferred when its declaration omits it. Its row facts
    /// are resolved too.
    fn finish_with(
        mut self,
        root: Ref,
        ret: Ty,
    ) -> StageResult<(Body, Ty, crate::results::RowFacts)> {
        self.unused_locals();
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
        self.opt_in_obligations()?;
        self.tuple_plan_steps()?;
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
            let mut caps = HashMap::new();
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
        // Capture modes (checking-and-tir.md "Data and closures"): `Copy`
        // when no side assigns the local after its declaration, `Shared`
        // otherwise. `Move` needs the liveness pass and is not chosen yet.
        let modes: Vec<hd_tir::ir::CaptureMode> = {
            let body = self.b.body_mut();
            body.cap_local
                .iter()
                .map(|l| {
                    let f = body.local_flags[l.idx()];
                    if f & (local_flags::CAPTURED_ASSIGNED | local_flags::MUTATED) != 0 {
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
        let body = self
            .b
            .finish(root, &modes)
            .map_err(|e| NotImplemented::new(Stage::Body, format!("TIR verifier: {e:?}")))?;
        Ok((body, ret, facts))
    }
}

/// The first number in a literal node's source text, converted straight
/// to `f32` (one rounding, not via `f64`).
fn float_text_f32(text: &str) -> Option<f32> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let number = crate::literals::split_suffix(&text[start..]).0;
    number.replace('_', "").parse::<f32>().ok()
}
