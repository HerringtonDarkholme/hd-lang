# New Compiler: Type Checking

Status: design record; may lag the code. The code and the spec win.

Frontend lane, 2026-10-07. Revised the same
day for the Codex review of 8bb6860d, the changes that
[crates/hd_check/DESIGN.md (trait solver) §16.4](#164-changes-needed-in-type-checkingmd-and-the-other-design-files)
asks for, and [docs/compiler/data-structures.md](../../docs/compiler/data-structures.md) §3.4, §3.9 and
§3.18 to §3.19. Revised again for the Codex re-review through 54f249b7
(frontend re-review response (`git show f1c55020:future-work/compiler/codex-rereview-response-frontend.md`)) and
for the removal of GADTs.

This document details the body checker of the new compiler. It refines
these sections of [COMPILER_DESIGN.md](../../docs/compiler/design-overview.md) (part D1):

- [§3 Core Data Structures](../../docs/compiler/data-structures.md#3-core-data-structures):
  IDs, interners, types, arenas, the poison type, diagnostics;
- [§4.9 Name Resolution](../hd_resolve/DESIGN.md#49-name-resolution) and
  [§4.10 Folder Interface Construction](../hd_resolve/DESIGN.md#410-folder-interface-construction):
  what the checker receives;
- [§4.12 Traits, Impls And Coherence](../hd_resolve/DESIGN.md#412-traits-impls-and-coherence):
  what the checker asks the trait solver;
- [§4.13 Body Checking](../hd_tir/DESIGN.md#413-body-checking): the three
  per-module phases M1, M2 and M3, which this document replaces in detail;
- [§4.14 Diagnostics](../hd_tir/DESIGN.md#414-diagnostics) and
  [§4.15 Limits](../hd_tir/DESIGN.md#415-limits);
- [§6 Scheduler](../hd_sched/DESIGN.md#6-scheduler-and-task-graph) and
  [§8 Determinism And Soundness Tests](../../docs/compiler/testing-the-compiler.md#8-determinism-and-soundness-tests).

The trait solver has its own design, [crates/hd_check/DESIGN.md (trait solver)](#new-compiler-the-trait-solver).
Section 1.6 states the interface between the two.

## How To Read This

Conventions:

- **Mine** marks an idea of this document: not shipped practice, and not
  in the research or in D1.
- **Rule TC-n** marks a design rule that a reviewer can check against the
  code. The prototype failure each rule prevents is in section 15.
- Rust code sketches shape and ownership. It is not final code.
- "First release" is the first shipped compiler. "Later" is after it.
- The spec decides semantics. Where this design reads the spec in a way
  that needs confirming, section 16 says so.

**Two inputs from the backend lane (D2, commit 440964bf).** They are not
choices of this document.

1. **One typed IR per body: TIR.** THIR and MIR are merged into one typed
   IR, as in Zig (Sema to AIR) and Carbon (SemIR). The checker emits it
   while it checks. TIR holds types, desugaring, explicit coercions, match
   decision trees, cleanup scopes for `defer` and cancellation,
   suspension and hook points, and structured control flow. No
   monomorphized IR is ever built: emission instantiates as it writes
   Wasm. D2 owns the instruction catalog, the builder API and the
   invariants
   ([§4.13.11](../hd_tir/DESIGN.md#41311-the-typed-ir-tir)). This
   document says what the checker needs from the builder and what it
   promises in return (section 1.5).
2. **Data-oriented storage.** Types live in an InternPool as a tag, a
   data word and `extra` words; TIR is columns; rollback is truncation of
   append-only columns plus the inference trail
   ([§3.9](../../docs/compiler/data-structures.md#39-data-oriented-encoding),
   [§3.10](../../docs/compiler/data-structures.md#310-ir-abstraction-contracts)). This
   design reads types through an abstract accessor API (section 1.4),
   which the pool's generated views satisfy, and uses D2's one rollback
   protocol (section 3.5).

## 1. Scope And Interfaces

### 1.1 Who Owns What

| Component | Owns | Does not own |
| --- | --- | --- |
| Resolution (`hd_resolve`, before) | uses, module scope, paths in headers, folder interfaces, impl tables, derived heads, header checks | anything in a body except path lookup through the frozen scope |
| **Type checker (`hd_check`, this document)** | name lookup of locals, every expression, statement and pattern in a body; inference variables; coercions; rows used by bodies; suspension checks; exhaustiveness and decision trees; mutability; definite initialization; omitted results and rows; body diagnostics; calls to the TIR builder | trait impl search, impl matching, coherence, TIR encoding, instantiation |
| Trait solver (`hd_types::solve`, beside) | goals of the form "type implements trait", projections, the instantiations of a generic trait, method candidates, its memo | inference variables (it reads them, never writes them), diagnostic text |
| TIR builder (D2's `hd_tir`, called by the checker) | the instruction encoding, scratch checkpoints, the final type sweep, the TIR verifier | types, names, any decision the checker makes |
| Collection and emission (D2, after) | instances, layout, Wasm | re-deriving anything TIR states |

**Rule TC-1. The checker decides; the builder records.** Every semantic
choice is made in the checker: which callee, which impl or bound, which
coercion, which literal width. TIR states it.
D2 never consults names, scopes or the solver to reconstruct a choice.

### 1.2 Inputs

All inputs are frozen before a body task starts. A body task reads them
through shared references and never locks
([§3.5](../../docs/compiler/data-structures.md#35-arenas-and-lifetimes)).

| Input | Producer | Used for |
| --- | --- | --- |
| `ModuleScope` | `ModulePrep` (M1) | module names, uses, prelude, local impl table |
| `FolderIface` of every used folder | `FolderIface` tasks | signatures, fields, variants, bounds, rows, templates |
| `PrivateSigs` of the module | M1 | private headers lowered to types; inferred results (M1) |
| impl tables | resolution | through the solver only |
| body syntax | parser | the green tree and its typed views |
| `CheckConfig` | driver | fuel per body, limits, emit mode, tests overlay on or off |

**Header projection completion (M4a gap 2).** Header lowering deliberately
leaves the trait-argument list empty on projections written `Self::Out`
and on projections inherited from a seeded bound, such as `P::Error` for
`P < Walker[Self]`. When the checker instantiates that header, it fills a
`Self` projection from the enclosing trait call's declared arguments. For
a bound projection it uses the matching parameter-environment clause; if
the concrete impl must determine arguments that were implicit in the
bound, it creates the missing variables before solving. Only then does it
normalize the projection. Header lowering therefore does not invent or
duplicate the call-site substitution.

### 1.3 Outputs

```rust
/// What one body check produces. Owned by the body task until ModuleFinish.
pub struct BodyResult {
    pub item: BodyKey,                    // (module, item index) or init body or derive instance
    pub diags: Vec<Diagnostic>,           // unsorted; ModuleFinish sorts (§12)
    pub result_ty: Option<Ty>,            // M1 bodies only: the inferred result
    pub row_facts: RowFacts,              // M3 input (§5.5)
    pub pending_calls: Vec<PendingCall>,  // calls whose providers M3 fills (§5.5)
    pub init_facts: InitFacts,            // init summary input (§8.4)
    pub fuel_used: u64,                   // reported in test mode (§14)
    pub tir: Option<TirBody>,             // None only in no-emit mode (§1.5)
    pub poisoned: bool,                   // any error in this body
}
```

`ModuleFinish` (M3) combines the module's `BodyResult`s into D1's
`ModuleResult`: sorted diagnostics, solved rows, the init summary, fact
records, and the TIR bodies, whose pending providers it fills before it
writes the `tir` entry
([Lifetime And The `tir` Entry](../hd_tir/DESIGN.md#lifetime-and-the-tir-entry)).

### 1.4 The Type Accessor API

The checker reads and builds types only through this API. D1 §3.4
stores types in the InternPool and decodes them through generated views
([§3.9.6](../../docs/compiler/data-structures.md#396-one-schema-generated-accessors));
that view layer implements this API, and so would any other encoding.

```rust
/// A type handle: 32 bits. One bit marks a body-local type (holds inference
/// variables). Copy, Eq, Hash; no Ord, no Display (TC-2).
pub struct Ty(u32);
pub struct TyList(u32);          // interned slice of Ty
pub struct RowRef(u32);          // interned row; may hold a pending private row (§5.5)

pub enum TyView<'a> {            // a typed view; borrowed, never stored
    Prim(Prim), Never, Poison, Void,
    Adt { def: DefId, args: &'a [Ty] },
    Tuple { elems: &'a [Ty], rest: Option<Ty> },
    Option(Ty),                  // T? is Option[T]; kept apart for speed only
    Fn { params: &'a [Ty], result: Ty, row: RowRef, suspends: bool },
    TraitValue { def: DefId, args: &'a [Ty], bindings: &'a [(Symbol, Ty)] },
    Param(ParamRef),             // declared, rigid; never inferred
    Assoc { assoc: DefId, tref: TraitRef },  // a projection; crates/hd_check/DESIGN.md (trait solver) §2.1
    Mut(Ty),                     // the mutable view `mut T`; `T` alone is readonly
    Infer(InferVar),             // body-local only
}

pub trait TyRead {
    fn view(&self, t: Ty) -> TyView<'_>;
    fn flags(&self, t: Ty) -> TyFlags;    // HAS_INFER, HAS_POISON, HAS_PARAM, HAS_ROWVAR, HAS_ASSOC
    fn size(&self, t: Ty) -> u32;          // node count, cached at intern time (§11)
}
pub trait TyBuild: TyRead {
    fn mk(&mut self, v: TyNew<'_>) -> Ty; // global if no HAS_INFER, else body-local
}
```

- **`Mut`, not `Readonly`.** In the spec `T` is the readonly view and
  `mut T` is the marked form
  ([Views](../../spec/lang/04-type-system.md#views)).
  [docs/compiler/data-structures.md §3.4](../../docs/compiler/data-structures.md#34-types) now uses `Mut`
  too.
- **Projections name their item.** `Assoc` holds the associated type's
  own `DefId` and the instantiated trait reference, so
  `<S as Add[i32]>::Out` and `<S as Add[i64]>::Out` are different types
  ([crates/hd_check/DESIGN.md (trait solver) §2.1](#21-trait-references)).
  docs/compiler/data-structures.md §3.4 still has `{ base, trait_, name }`; the backend
  lane changes it (crates/hd_check/DESIGN.md (trait solver) change 15).
- **One inference-variable form.** D1 has `Infer` and `IntLit`. Here a
  literal is an ordinary `Infer` variable whose kind (general, integer
  literal, float literal) lives in the inference table, because two
  variables merge their kinds when unified (section 3.6).
- **Global types are shared, local types are not.** `mk` interns a
  variable-free type in the global pool, and a type with variables in
  the body-local pool. A body never writes another body's local types.
- **Flags are computed at intern time**, so "does this type hold a
  variable or poison" is one load.

**Rule TC-2. Types are handles, never text.** No checker code compares,
hashes or parses type spellings. Printing goes through the diagnostic
renderer, which prints stable paths at the end of the run. `Ty` has no
`Display` impl, so the old pattern cannot compile.

### 1.5 What The Checker Needs From The TIR Builder

D2 defines TIR's instructions, its builder API and its invariants
([§4.13.11](../hd_tir/DESIGN.md#41311-the-typed-ir-tir)). The checker
is the builder's only caller. This section lists what the checker relies
on, and what it promises in return.

**What the checker uses.**

| Builder operation | Checker use |
| --- | --- |
| `konst` | a literal or other constant: a constant `Ref` into the global pool, with no instruction ([§3.18](../../docs/compiler/data-structures.md#318-tir)) |
| `local`, `get`, `set`, `prim`, `emit` | every expression, in evaluation order |
| `reserve`, `fill` | a slot in the open block's list, filled later, once (the four cases below) |
| `call` with a `Callee` record and `Providers` | every resolved call; `(key, provider)` pairs, or `Pending(index)` naming a pending call record for a private callee whose row M3 solves (section 5.5) |
| `await_` | bang calls, `s!()`, `all!`, `race!`; the builder records the enclosing scope chain |
| `coerce` | each coercion found by `coerce` (section 4.2) |
| `open_block`/`close_block`, `open_scope`/`defer`/`close_scope`, `open_loop`, `sub_body` | every block, cleanup scope, loop and closure, opened and closed in source nesting |
| `checkpoint`, `rollback` | every trial (section 3.5) |
| `finish(solution)` | once per body, at its end: the final type sweep and, in debug builds, the verifier |

**When the checker reserves a slot.** A block's list is evaluation
order, but the checker does not always decide in that order
([§3.9.5](../../docs/compiler/data-structures.md#395-building-scratch-buffer-checkpoints-truncation)).
It reserves in exactly four cases:

1. **A postponed argument** (section 2.4 step 4): its slot is reserved at
   its source position and filled when the argument is checked.
2. **A left operand checked second** (section 2.5, operators): when the
   left operand is built only of unsuffixed literals and is not a bare
   constant, its slot is reserved before the right operand is checked.
3. **An arm or branch tail** of an `if`, `match` or value loop with no
   expected type: the coercion to the joined type is known only at the
   join (section 4.3), so each tail reserves a slot that the join fills.
4. **A stalled method call** (section 2.5): its callee record is filled
   when the receiver's type is known, at the latest at the end of the
   statement.

A slot is a real TIR instruction with tag `Slot`, which lives as long as
the body, and `fill` writes it once
([§3.9.5](../../docs/compiler/data-structures.md#395-building-scratch-buffer-checkpoints-truncation),
Codex re-review N-I1). A slot reserved inside a trial is truncated with
the instructions. A slot reserved before a trial is never filled during
it: the checker appends the fill to its `deferred_fills` column, which
rollback truncates, and applies the column once no trial is open
(section 3.5). A slot still empty at the end of the body is a checker bug,
which `finish` reports as an internal error.

**Constants have no span in TIR, and the checker needs none.** A literal
is a constant `Ref` with no instruction and no `syn` entry. The checker
reports literal diagnostics (`integer-literal-range`, the `+N` hint) from
its own literal table, which keeps each literal's syntax node (section
3.6), and promise 6 below means it never reads TIR back. One gap is the
backend's: a literal whose width is still open when it is emitted has no
global constant form yet (review finding I7). The checker needs `konst`
to take such a literal; it gets its width from the `Solution` at
`finish`.

**What the checker promises.** These are the invariants of §4.13.11
seen from the caller's side.

1. **Calls are resolved.** A `Callee` is an item with its type
   arguments, a trait method with its choice (the impl's `DefId` or the
   bound's index), or stored evidence. Operators, interpolation,
   indexing, iteration, `?`, pipes, compound assignment, comprehensions
   and default arguments arrive desugared, as D2's desugaring table says.
   A default is a call of the default's own body with the earlier
   arguments (section 2.4 step 3).
2. **Every implicit conversion is a `coerce`.** So every operand's type
   equals the type its position expects (invariant 4).
3. **Scopes are explicit.** Every cleanup scope of
   [Cleanup Scopes](../../spec/lang/06-control-flow.md#cleanup-scopes) is
   an `open_scope`/`close_scope` pair, and every exit names its target.
   D2 derives cleanup and cancellation paths from that nesting.
4. **Types resolve once.** Instructions may carry types with inference
   variables. At the end of the body the checker hands `finish` its
   `Solution`, the resolved inference table. Literal widths and use-site
   type arguments are therefore never patched elsewhere.
5. **Decision trees come last in a match.** The checker emits each arm's
   block while it checks the arm. After usefulness it builds the
   decision tree and emits it as switch instructions (section 7.2).
6. **The checker never reads TIR back.** Every check the checker makes
   reads its own tables, never instructions (sections 8.2 to 8.4). So
   checking does not depend on what was emitted.

**Rule TC-3. Checking does not depend on emission (mine).** Promise 6
means the builder can be swapped for a discarding one with no change in
diagnostics. The determinism matrix runs every case both ways and
compares the output byte for byte (section 14).

**Where a check drops TIR.** D2 has `hd check` emit TIR and write it to
the module's `tir` entry at `ModuleFinish`, then free it, so a later
build reuses the entry instead of rechecking
([§3.10.2](../../docs/compiler/data-structures.md#3102-lifetimes-sizes-and-peak-memory)).
This design keeps that default and adds one mode:

| Mode | Used by | TIR |
| --- | --- | --- |
| emit (default) | `hd check`, `hd build`, `hd run`, `hd test` | built per body in the worker's columns; moved to the module result; serialized and freed at `ModuleFinish` |
| emit and verify | debug builds of `hd`, `--verify`, CI on std | as above, plus the verifier at each `finish` |
| no-emit (mine) | the playground's check-as-you-type, `hd fix` rounds, `hd doc` | never built; the checker is generic over the builder (`Checker<B: TirSink>`), and the discarding sink's methods are no-ops that return dummy handles |

The no-emit mode saves about 200 bytes per source line of transient
memory and the emission time where no build can follow. Its `check`
entry is written without a `tir` entry, so a later build rechecks that
module. The mode is accepted for the playground and `hd fix`
(orchestrator, 2026-10-07).

### 1.6 The Trait Solver Interface

The checker asks the solver four kinds of goals. The solver never
writes the checker's state, never reports a diagnostic, and never sees
spans. [crates/hd_check/DESIGN.md (trait solver)](#new-compiler-the-trait-solver) designs the solver; this is the
interface as the checker sees it, with that design's changes 1 to 5.

```rust
pub struct TraitRef { pub trait_: DefId, pub self_ty: Ty, pub args: TyList }

pub enum Goal {
    /// `self_ty: Trait[args, bindings]`; `mut_` for a `T < mut Tr` bound.
    Implements { tref: TraitRef, bindings: AssocList, mut_: bool },
    /// Normalize `<self_ty as Trait[args]>::assoc`; `assoc` is the item's own DefId.
    Project { assoc: DefId, tref: TraitRef },
    /// Every instantiation `Trait[A..]` that `self_ty` implements.
    /// For parameters named only in bounds and for choosing among instantiations.
    Instantiations { self_ty: Ty, trait_: DefId, mut_: bool },
    /// The method candidates named `name` for a receiver type,
    /// inherent first, then available traits.
    Methods { receiver: Ty, name: Symbol },
}

pub enum Answer {
    Holds { evidence: Evidence, learned: SmallVec<[(InferVar, Ty); 2]> },
    Normalized { ty: Ty, evidence: Evidence, learned: SmallVec<[(InferVar, Ty); 2]> }, // Project only
    Many { candidates: SmallVec<[Candidate; 4]>, unavailable: SmallVec<[DefId; 2]> }, // Instantiations, Methods
    Fails(FailInfo),                         // closest impl, failed subgoal, for the message
    Stalled { on: SmallVec<[InferVar; 2]> }, // needs more inference first
    Overflow,                                // trait-resolution-depth
    OutOfFuel,
}

pub enum Evidence {
    Impl { row: ImplRef, args: TyList },     // written, derived, delegated, generated, numeric family, tuple template
    Bound { param: ParamRef, index: u16 },   // a clause index in the elaborated environment
    TraitValue { trait_: DefId },            // the self type is a trait value of this trait or a subtrait
    Builtin(BuiltinImpl),                    // Any, AnyVal, AnyRef, Inspectable, Tuple, Num, Integer, Float, Suspend
    Poison,
}

pub struct SolveCx<'a> {
    pub env: &'a ParamEnv,          // the item's elaborated environment
    pub infer: &'a dyn InferRead,   // shallow resolution of inference variables
    pub avail: AvailKey,            // which traits are available in this module
    pub universe: ImplUniverseId,   // the argument-owned impls this context sees (crates/hd_check/DESIGN.md (trait solver) §3.2);
                                    // a body's after M1, a HeaderCheck(F)'s from closure(F) at task start
    pub local_vis: LocalVis,        // the local impls visible at this point
    pub memo: &'a mut BodyMemo,     // per-body memo and `met` set; owned by BodyCx
}

pub trait Solver: Sync {
    fn solve(&self, cx: &mut SolveCx<'_>, goal: Goal, fuel: &mut Fuel) -> Answer;
    fn elaborate(&self, bounds: &[DeclaredBound], out: &mut ParamEnvBuilder) -> EnvKey;
    fn select(&self, tref: ConcreteTraitRef) -> Selection;          // codegen only
    fn normalize_concrete(&self, assoc: DefId, tref: ConcreteTraitRef) -> Ty; // codegen only
}
```

- **No `Coinductive` evidence.** A derive instance's member goals reach
  the derived head as an ordinary `Impl`, so the search never meets a
  cycle it must assume
  ([crates/hd_check/DESIGN.md (trait solver) §3.10](#310-derives-delegation-and-error)).
- **Tuples are templates.** Tuple `Eq`, `Ord`, `Hash` and `Debug` are
  `Impl` evidence of std's tuple templates, not `Builtin` (section 6.2).
- **`elaborate`** runs once per item signature, when its body task
  starts. The resulting `EnvKey` is part of every memo key that names a
  parameter.

**How the checker uses answers.**

- `Holds` or `Normalized` with `learned` bindings: the checker applies
  each binding through its own unifier, on its own trail (section 3.5).
  For `Normalized`, it then unifies `ty` with the other side. The solver
  stays pure, so its answers can be memoized.
- `Stalled`: the checker keeps the goal as an **obligation** and watches
  the listed variables. It asks again when one of them is bound, and at
  the latest at the end of the statement or the body (section 2.7). A
  `Project` that stalls on a variable inside its own base becomes a
  **projection-equality obligation** `?0 == <?0 as Tr>::Item`; the
  checker never binds a variable to a projection over itself
  ([crates/hd_check/DESIGN.md (trait solver) §4.3](#43-normalization-lazy-at-three-points)).
- `Fails`: the checker reports the error, once per root cause, with
  `FailInfo` for the message. One missing impl gives one error per body
  and leaf goal, with an "and N more uses" note (owner, 2026-10-07).
- `Overflow` and `OutOfFuel`: the checker reports the limit diagnostic
  and poisons the expression.
- `Many.unavailable` lists traits that are not available but would
  match. It feeds the `use` fix-it of `unknown-method` only.

**What the checker sees of caching and budgets.**

- **Memo.** The solver canonicalizes a goal: variables resolved, arm
  equalities substituted, concrete projections normalized, remaining
  variables numbered by first occurrence. The memo key holds the
  canonical goal plus every input that can change the answer: the
  `EnvKey` when the goal names a parameter, the `LocalVis` when it can
  match a local impl, the `AvailKey` for `Methods`, and the context's
  `ImplUniverseId` for `Instantiations` and `Methods`. A goal with no
  variable and no local type goes to the run's global
  memo; any other goes to the body memo. A goal that met the depth cut
  or ran out of fuel is never published globally
  ([crates/hd_check/DESIGN.md (trait solver) §7.1](#71-memo-keys-and-eligibility)).
  The checker never reads or writes memo entries itself.
- **Fuel.** The checker passes its body's `Fuel`. The first time a body
  meets a canonical goal, the solver walks the goal's proof DAG and
  charges 1 plus the heads matched for each node the body has not met
  yet. Each later ask of the same goal in that body costs 1. So a shared
  subgoal is paid for once per body, and the fuel a goal
  costs is the same on every run, thread count and cache state (rule
  TS-5; lesson 3 of
  Lessons For hd (`git show f1c55020:future-work/compiler/prior-art-issues.md`)).
- **Depth.** The solver tracks depth itself, by stored heights, and
  answers `Overflow` at `trait-resolution-depth`. One level is a bound-plan
  step, a tuple template's element obligation, an `Inspectable` step
  through a type argument, or a projection step (owner, 2026-10-07).
- **Stalled goals are not memoized globally**, since their answer depends
  on the body. The body memo lives in the checker's `BodyCx` and is lent
  to the solver for each call. Its keys and answers live in its own
  arena, over canonical placeholders, so a rollback that truncates the
  body-local pool leaves no entry dangling.

### 1.7 Body Tasks And The Exactly-Once Rule

D1's phases stay. This design adds one rule.

| Phase | Task | Bodies checked here |
| --- | --- | --- |
| M1 | `ModulePrep(m)`, serial | the module's top-level statements; every non-public function and inherent method whose result type is omitted; depth first (section 9) |
| M2 | `Body(m)`: the module's bodies as one batched parallel iterator (crates/hd_sched/DESIGN.md granularity rule) | every other body: functions and methods with written results, impl members, derive instances, test bodies, default bodies, fact and shared-enum-data expressions |
| M3 | `ModuleFinish(m)`, serial | no body: the row solve, deferred row checks, init summary, diagnostic sort |

**Rule TC-4. Each body is checked exactly once per run.** A body checked
in M1 for its result type is not checked again in M2: its `BodyResult`
is final. Rows never cause a recheck (section 5.5). Literal widths never
cause a statement recheck (section 3.6). The only repeated work is a
speculative trial (section 3.5), which is bounded and rolled back.

**A body result is a function of the body and the module's frozen
inputs.** An M2 `BodyResult` reads only its own syntax, the module scope,
the private signatures (M1 results included) and the frozen interfaces
(rule TC-9). This is what a later per-body cache would need: after a
private body edit that leaves every private signature unchanged, only
that body's result differs. The first release still rechecks the whole
module ([crates/hd_cache/DESIGN.md](../hd_cache/DESIGN.md)); the review's edit-latency finding (P2) is
the backend lane's.

**Template target premise (M4a gap 4).** While checking a method body
from a `by Structure` template, the parameter environment includes the
compiler-supplied premise `target < Structure`. The premise belongs only
to that instantiated template body; it is not written into the target
type's interface and does not make `Structure` hold in unrelated code.

**Default and fact bodies (review finding 4).** They are checked once,
where they are declared, and never at the call:

| Body | Checked as | Evaluated |
| --- | --- | --- |
| a parameter default | a function of the earlier parameters and the declaration's generics, with the parameter's type as its result | at each call that omits it, after every explicit argument, in declaration order ([`fn.default.eval`](../../spec/lang/07-functions.md#r-fn.default.eval)) |
| a data field default | a function of the declaration's generics only; it sees the declaration's lexical scope, not other fields | at each construction that omits it, in field order ([`data.default.eval`](../../spec/lang/08-data-and-enums.md#r-data.default.eval)) |
| a fact or metadata expression | a body with no parameters | once, at run time, on first read ([`annot.fact.eval.lazy`](../../spec/lang/14-annotations.md#r-annot.fact.eval.lazy)) |
| a variant's shared-data constructor and its defaults | a body with no parameters | once, at run time, during its declaring module's initialization ([`data.shared.module-init`](../../spec/lang/08-data-and-enums.md#r-data.shared.module-init)) |

All of them are restricted contexts (section 5.8) and must be
requirement-free. The checker states which body a call's default is and
passes the earlier argument values (section 2.4 step 3). It never
decides how a default or a fact is evaluated: a per-call default
instruction and a fact's lazily initialized global are the backend lane's
(review finding 4; section 17). A dependent's check reads only a fact's
type, never its value.

## 2. The Algorithm

### 2.1 Modes

The checker is bidirectional and local to one body.

```rust
enum Expect { None, Ty(Ty), Fn { params: SmallVec<[Option<Ty>; 4]>, result: Option<Ty>, row: Option<RowRef> } }

fn infer(&mut self, e: ExprId) -> Typed;                 // synthesize a type
fn check(&mut self, e: ExprId, want: Ty) -> Typed;       // check against a type, coercing (§4)
fn check_exact(&mut self, e: ExprId, want: Ty) -> Typed; // no coercion: patterns, `is`, receivers
```

`Typed` is `(Ref, Ty)`: the TIR value and its type. `check` infers when the form has no use for
the expected type, then calls `coerce(found, want)` (section 4.1).
`Expect::Fn` carries a partial function type for closures whose
parameters come from a generic callee.

### 2.2 Expression Forms

Where the expected type flows, form by form. "Check" means the form
uses `want`. "Infer" means it synthesizes and the caller coerces.

| Form | Mode | Expected type flows to | Notes |
| --- | --- | --- | --- |
| integer or float literal | check | the literal: takes `want` if numeric of its kind | no `want`: a fresh literal variable (section 3.6) |
| `-lit`, `+lit` | check | the literal, as a unit for range | signed literal ([`types.literal.local.signed`](../../spec/lang/04-type-system.md#r-types.literal.local.signed)) |
| suffixed literal, prefixed string | infer | the literal gets the suffix function's parameter type | a call to the literal function |
| string, char, bool | infer | none | |
| name of a local, global or function | infer | none; a generic function value instantiates with fresh variables | |
| `.Variant(args)` | check only | the enum comes from `want` | no `want`: `missing-contextual-enum-type` with the fix in its hint (section 10.5) |
| tuple `(a, b)` | check | each element gets its component of `want` | |
| list `[a, b]` | check | each element gets `want`'s element type; else LCT (section 4.3) | `[]` with no `want`: an element variable |
| map `{k: v}` | check | keys and values; else LCT | |
| data literal `T { f: e }` | infer head, check fields | each field gets its declared type after substitution | freshness gives `mut T` (section 8.1) |
| copy-update `T { ...e, f: x }` | as data | | |
| call `f(args)` | infer callee, check args | parameter types after instantiation; `want` unifies the result first | section 2.4 |
| method call `r.m(args)` | infer receiver | as call | section 2.5 |
| operator `a op b` | see section 2.5 | the non-literal operand first, then the other | desugared to a call |
| `a?` | infer | none | section 4.4 |
| `if c: a else: b` | check | each branch gets `want`; else LCT | condition checked against `bool` |
| `match s: arms` | check | each arm gets `want`; else LCT | scrutinee inferred; section 7 |
| `for`, `while` | check if value-producing | `break` values and `else` get `want`; else LCT | |
| closure `fn(x): e` | check | parameters and result from `Expect::Fn` | section 2.6 |
| block of 2+ statements | check | the final expression | its statements are separate literal scopes |
| pipe `a \|> f` | as call | | |
| `$.use(K)` | infer | none | key must be in `available` (section 5.2) |
| `$.with K=p: body` | check | `body` gets `want` | each provider checked against `K` |
| `f!(args)` | as call | | suspension checks (section 5.7) |
| `x is y` | infer both | none | identity rules; see below |
| `_` | check | records `want` for the hole diagnostic | `placeholder-outside-pipe` (section 10.4) |

**Interpolation ownership (M4b gap 9).** An interpolation contributes
only its top-level expression result to the enclosing string's parts.
Tokens and literals nested inside that expression remain operands of the
expression. The M4 checker violates this for `${ages["cy"]}` by also
appending the index key literal `"cy"`; the token/child walk must consume
each interpolation subtree exactly once.

**Identity `is`** ([Allocation Identity](../../spec/lang/05-expressions.md#allocation-identity),
as the S1c spec pass applied the owner's answers of 2026-10-07).
After inferring both operands, the checker:

1. Strips `mut` at every level of both types.
2. Rejects any operand type that does not implement `AnyRef`:
   `identity-requires-references`. That covers primitives, strings,
   tuples, every enum (optionals and `Result` included) and function
   types ([`expr.is.value-operand`](../../spec/lang/05-expressions.md#r-expr.is.value-operand)).
   Only data, `List`, `Map`, trait values, `Any`, suspensions, handles and
   `T < AnyRef` parameters pass. A function never reaches `T < AnyRef`,
   because the bound check rejects it (`unsatisfied-trait-bound`).
3. Requires compatible types: equal, or one a trait value or `Any` that
   the other converts to. Otherwise `incompatible-identity-operands`.
4. Emits `Is` with two operands of reference types; no desugaring for
   optionals or results remains.

When an `Any` or trait value holds a value type at run time, the result
is unspecified. That is codegen's concern, not the checker's.

### 2.3 Statements And Blocks

1. Statements are checked in source order. Each statement is a **literal
   scope**: open literal variables still unbound at its end take their
   default ([`types.literal.local.statement`](../../spec/lang/04-type-system.md#r-types.literal.local.statement)).
   The one exception is a literal held by an open join: a `return`
   operand of a body with an omitted result, or a `break` value of a
   loop. It waits for that join (section 3.6).
2. `let x = e`: infer `e`, then apply the binding rule for the view
   (section 8.1). `let x: T = e`: check `e` against `T`.
3. An expression statement whose value is discarded is checked for
   must-use types after its literal scope closes
   ([`flow.must-use.discard`](../../spec/lang/06-control-flow.md#r-flow.must-use.discard)).
4. The checker keeps a **divergence flag**: after a statement of type
   `never`, the next statement gets `unreachable-code` once per run of
   unreachable statements, and is still checked
   ([`flow.unreachable.checked`](../../spec/lang/06-control-flow.md#r-flow.unreachable.checked)).

### 2.4 Calls And Use-Site Type Arguments

For a call `f(args)` whose callee resolves to a generic declaration:

1. **Instantiate.** Each generic parameter gets a fresh variable. An
   explicit list `f::[A, _]` binds its written slots at once; `_` and
   omitted slots stay variables
   ([Generic Arguments](../../spec/lang/04-type-system.md#generic-arguments)).
2. **Expected result first.** If the call has a `want`, unify the
   callee's result type with it, tentatively: a failure here is not an
   error yet, it only means the expectation gives no information. The
   attempt is a speculation (section 3.5) and is rolled back on failure.
3. **Match arguments to parameters.** Positional, then named, then
   spreads and varargs ([Calls](../../spec/lang/05-expressions.md#calls)).
   A missing argument with a default is filled by a **default call**:
   after every explicit argument, in parameter declaration order, the
   checker emits a call of that parameter's default body, with the
   callee's type arguments and the values of the earlier parameters,
   explicit or defaulted. Each call evaluates the default again, so a
   default that allocates or mutates does so once per call. A data
   literal fills an omitted field the same way, with no earlier fields.
4. **Check arguments in source order.** Each argument is checked against
   its parameter type, which may hold variables. An argument that needs
   an expected type to check (a closure with unannotated parameters, a
   contextual variant, a collection holding one) is **postponed** when
   its parameter type is still an unbound variable at the head. The
   checker checks it after the other arguments, so `map(xs, fn(x): x + 1)`
   works in either argument order. This is the only reordering. TIR keeps
   source evaluation order: each argument's instructions are emitted in
   source order, and the call lists them in parameter order.
5. **Join several arguments.** Arguments that solve one parameter may
   differ only in outer permission
   ([`types.generic.infer.join`](../../spec/lang/04-type-system.md#r-types.generic.infer.join)).
   Section 3.4 gives the mechanism.
6. **Bounds.** Each instantiated bound becomes an `Implements` goal.
   `Holds` may teach a binding; `Stalled` becomes an obligation.
7. **Parameters named only in bounds.** After the other parameters, each such
   parameter asks `Instantiations` of its bounded parameter's type, and
   solves when exactly one instantiation fits. The step repeats until
   nothing changes
   ([`types.generic.infer.bound.fixed-point`](../../spec/lang/04-type-system.md#r-types.generic.infer.bound.fixed-point)).
   The repeat count is at most the number of parameters named only in bounds.
8. **Defaults.** A parameter still unbound takes its declared default,
   in declaration order, with earlier arguments substituted. Then its
   bounds are checked
   ([`types.generic.default.fill`](../../spec/lang/04-type-system.md#r-types.generic.default.fill)).
9. **Leftovers** wait as obligations until the end of the statement.
   Then a parameter with no solution is `cannot-infer-type`, and one with
   several is `ambiguous-type`
   ([`types.infer.ambiguous.code`](../../spec/lang/04-type-system.md#r-types.infer.ambiguous.code)).

There is no overloading and no disjunction. One name gives one callee, so
no step searches alternatives (the Swift lesson). The only choice among
candidates is section 2.5's instantiation choice.

### 2.5 Methods And Operators

**Typed structure facts (M4a gap 4).** The resolved inherent call
`h.fact::[D]()` on `std.structure.Field` is a typed fact lookup: the
explicit argument `D` is checked against the handled member's type. It
does not add the ordinary `D < Inspectable` method bound. The checker
recognizes this by the resolved `Field.fact` identity, not by source
spelling alone.

**Method lookup.** Infer the receiver, resolve it shallowly, and ask
`Methods { receiver, name }`. The solver returns inherent candidates
first, then candidates from available traits
([Method Resolution](../../spec/lang/09-traits.md#method-resolution)).

| Answer | Checker action |
| --- | --- |
| one inherent method | call it |
| methods of two different traits | `ambiguous-method` |
| several instantiations of one generic trait | the instantiation choice below |
| none | `unknown-method`, with a `use` fix-it when an unavailable trait would match, and a did-you-mean on the name |
| receiver is an unbound variable | `Stalled`: postpone to the end of the statement; if still unbound, `cannot-infer-type` naming the receiver |

**The instantiation choice** ([Instantiations Of One Generic Trait](../../spec/lang/09-traits.md#instantiations-of-one-generic-trait)).
The solver returns the candidates in content order. The checker then:

1. **Prefilters (change 7).** It drops each candidate whose parameter's
   head constructor cannot match an argument whose type is already
   known. No trial runs for those.
2. **Tries each survivor.** Inside a trial (section 3.5) it instantiates
   the candidate's scheme: a fresh variable for each impl parameter the
   receiver did not fix, and the scheme's residual bounds as obligations
   (crates/hd_check/DESIGN.md (trait solver) §6.5). It checks the arguments against the
   candidate's parameters. When the call has an
   expected type, the trial then runs `coerce(result, want)`, still
   inside the trial (change 9, review T4). Then it retries the residual
   obligations once. A candidate **fits** if the trial has no error: a
   residual that `Fails` is an error, and one that still stalls does not
   rule the candidate out. The chosen candidate's stalled residuals stay
   as ordinary obligations of the real check, so a residual bound that
   fails later is still reported. So `let n: i32 = money.pick()` picks `Pick[i32]`
   when `Money` implements `Pick[i32]` and `Pick[string]`, as
   [`trait.resolve.fits.expected`](../../spec/lang/09-traits.md#r-trait.resolve.applies.expected)
   requires. Methods of two different traits are still `ambiguous-method`
   whatever the expected type.
3. **Chooses:**
   - one fit: check the call again for real against it;
   - several fits, and they differ only in the width of an open literal
     argument: default the literal and choose again
     ([`trait.resolve.literal-arg`](../../spec/lang/09-traits.md#r-trait.resolve.literal-arg));
   - several fits otherwise: `ambiguous-method`;
   - none: `type-mismatch` listing the instantiations.

**Trial cost.** Arguments that check the same way for every candidate
(they have no expected-type dependence) are inferred once before the
trials, and a trial only unifies their types. Only postponed arguments
(step 4 above) are checked once per candidate. Every trial charges fuel.

**The per-site trial memo (change 8, mine).** Nested calls multiply
trials: the real check of an outer call re-checks its inner calls, and
each outer candidate's trial checks them too. So each call site keeps its
trial outcomes in the body's trial memo, keyed by:

- the call's syntax node and the candidate's index;
- the canonical expected type and the canonical types of the arguments
  inferred before the trials, numbered as the solver numbers variables
  (crates/hd_check/DESIGN.md (trait solver) §2.2).

An entry records only "fits" or "does not fit". It is valid only if the
trial read no other inference variable from outside itself. The trial
tracks this: a `find` on a variable older than the trial's checkpoint
that is not in the key marks the outcome **tainted**, and a tainted
outcome is not stored. A closure body that reads an outer local whose
type is still open is the usual cause. A hit costs 1 fuel. The
memo is per body and never rolled back: its keys name no variable, and
an untainted outcome depends on nothing else.

**What the memo bounds (Codex re-review N-T7).** The number of trials is
at most the number of distinct `(site, candidate, canonical context)`
keys, plus the tainted trials, which are never cached. When the outer
candidates give an inner call the same expected type, as in the usual
`From` chain, the keys repeat and nested trials cost sites × candidates.
When each outer candidate wraps the expected type differently, the
distinct contexts can double at each level, and the memo gives no
linear bound. Fuel then ends the check with `item-too-complex`. The
`pathological` suite has both cases, and a tainted-closure case, and
checks for the limit diagnostic rather than a speed claim
([crates/hd_check/DESIGN.md (trait solver) §7.6](#76-bounded-is-not-linear)).

**Unavailable traits.** A `Methods` answer also lists traits that are
not available but would match (change 10). The checker uses the list
only for the `use` fix-it of `unknown-method` (section 10.5).

**Operators.** `a op b` follows
[Operator Traits](../../spec/lang/05-expressions.md#operator-traits):

1. If one operand is built only of unsuffixed literals, check the other
   operand first and give its type to the literal side
   ([`types.num.binary.literal-join`](../../spec/lang/04-type-system.md#r-types.num.binary.literal-join)).
2. If both operand types are primitive after that, the built-in rule
   types the operator. Two different numeric types are `type-mismatch`,
   or `mixed-signedness` across signedness.
3. Otherwise the left operand's type selects the impl: the call
   `Op::[R]::m(a, b)`, through the instantiation choice above.
4. The emitted TIR is the trait `Call` (or a `Prim`), with the
   evaluation order left, right, call.

**Trait-method conformance (P2-1b `53563951`,
`hd_check/src/conform.rs`).** Each impl method's signature is checked
against its trait's; a mismatch is `trait-method-signature`.

### 2.6 Closures

1. With `Expect::Fn`, unannotated parameters take the expected parameter
   types, and the result is checked against the expected result.
2. Without it, every parameter needs an annotation:
   `closure-parameter-needs-annotation`.
3. An omitted result is the LCT of the final value and every `return`
   operand (section 4.3). Literal `return` operands stay open until that
   join, even across statements
   ([`types.literal.local.form.closure-return.statements`](../../spec/lang/04-type-system.md#r-types.literal.local.form.closure-return.statements),
   section 3.6).
4. A closure is monomorphic. Its body is checked inside the enclosing
   body, with the same inference table, and is part of the same body
   task.
5. Its row is inferred from the keys its body uses (section 5.4).
6. A recursive closure bound by `let f = fn ...` needs its result written
   ([`fn.closure.recursive-result`](../../spec/lang/07-functions.md#r-fn.closure.recursive-result)).
   The binding is entered in scope with the written function type before
   the closure body is checked.
7. Captures are recorded per local (section 8.3).

A local `fn` declaration with an omitted result is checked where it is
declared, before the statements after it. It sees only earlier local
functions and itself, so the only cycle is self-recursion, which is
`recursive-function-needs-result-type`.

### 2.7 When Variables Are Resolved

| Variable | Resolved | If still open |
| --- | --- | --- |
| literal (integer or float) | as soon as it meets a concrete numeric type; else at the end of its statement, by default; a class held by a join, at the join (section 3.6) | never open past its statement or its join |
| use-site type argument | when unified; else at the end of the call's statement, after defaults | `cannot-infer-type` or `ambiguous-type` |
| local binding's type | from its initializer, in its statement | an empty collection literal can leave its element open to the end of the body (section 2.8) |
| closure parameter | from `Expect::Fn` or the annotation | `closure-parameter-needs-annotation` |
| omitted private result | at the end of its body in M1 | never open |
| private row variable | in M3, then the row sweep (section 5.5) | never open |

**Obligations.** A stalled goal goes on the body's obligation list with
the variables it waits on. Each variable it waits on gets one **watch edge**, a row
`(variable, obligation, next)` in an append-only table; the variable's
`watch_head` points at its newest edge (section 13). One obligation that
waits on three variables has three edges, one in each list.

- **Waking.** Binding a variable, or uniting it under another root,
  wakes the obligations on its edges. A woken obligation is queued once
  per wake round: its state moves from `Pending` to `Woken`, and a second
  wake in the same round finds it `Woken` and does nothing (change 11).
- **Retrying.** Woken obligations are retried at the next statement
  boundary, and before any step that needs their answer, such as method
  lookup on a stalled receiver. They are retried in creation order, so
  the result does not depend on which binding woke them. A retry is
  charged the goal's size, and one that stalls again adds edges on the
  variables it now waits on.
- **Projections.** A stalled `Project` whose base holds the variable it
  would bind is a projection-equality obligation, retried like any other
  (section 1.6).

### 2.8 Empty Collections

`let names = []` leaves the element type open. The spec's literal rule
does not apply: this is not a literal width. The binding's type holds a
variable that later statements may solve, as in `names.push("a")`
([Type Inference Boundaries](../../spec/lang/04-type-system.md#type-inference-boundaries)).
At the end of the body, a still-open element type is `cannot-infer-type`.
The diagnostic is built then, so it knows whether the binding was
mutated, and its fix-it writes the `mut` form when it was:
`let names: mut List[string] = []` (wishlist item 1, section 10.5).

## 3. The Unifier

### 3.1 Variables

```rust
pub struct InferVar(u32);       // numbered per body from 0 (D1 §6.5 rule 2)

pub enum VarKind : u8 {
    General,                    // a use-site type argument, element type, closure parameter
    IntLit { signed: bool },    // an open integer literal class
    FloatLit,                   // an open float literal class
}
```

A variable is created for a use-site type argument, an open literal, an
empty collection's element, a closure parameter or result without an
expected type, and a placeholder slot `_` in an explicit list.

### 3.2 Union-Find

- Union-find over `InferVar` with **path halving** and **union by rank**.
  The root holds the binding: a `Ty` or nothing.
- Each root also holds a **blame span**: the span of the first
  unification that bound it. A mismatch message names it ("expected
  `i64` because of this argument").
- A union wakes the watchers of the root that stops being a root, so
  their obligations re-stall on the new root (section 2.7).
- `unify(a, b)` resolves both sides shallowly, then:
  - two unbound roots: union; merge kinds (section 3.6);
  - a root and a type: occurs check, kind check, bind;
  - two constructors: same head and arity, then unify children;
  - `Poison` on either side: succeed silently (section 10.1);
  - `Never` against anything: handled by coercion, not here.
- Structural unification walks children with an explicit work stack, not
  native recursion, so a deep type cannot overflow the stack. Each pair
  visited costs one fuel step.

### 3.3 The Occurs Check

Binding `?a := T` first checks that `?a` does not occur in `T`. The walk
visits only subterms whose `HAS_INFER` flag is set, so a variable-free
type costs one load. A failure is `type-mismatch` with the message
"`?a` would be infinite", and both sides become poison so it reports
once.

### 3.4 No Levels, No Generalization

hd has no let-generalization. A local binding is monomorphic, a closure
"declares no type parameters and always has a monomorphic function type"
([`fn.closure.monomorphic`](../../spec/lang/07-functions.md#r-fn.closure.monomorphic)),
and a declaration's generics are always written
([`types.infer.named-fn`](../../spec/lang/04-type-system.md#r-types.infer.named-fn)).
So the unifier has **no levels**, no generalization step and no
let-polymorphism. A generic function used as a value is instantiated
with fresh variables at the use, as a call is.

**Permission join.** Several arguments solving one parameter may differ
only in outer `mut` ([`types.generic.infer.join`](../../spec/lang/04-type-system.md#r-types.generic.infer.join)).
This is not unification, so it is handled at the call:

1. While checking a call's arguments, a parameter type that is a bare
   variable `?T` is solved by a **join slot** instead of a plain bind.
2. The first argument binds the slot to its type `S`.
3. A later argument `S'` that equals `S` up to the outermost `Mut`
   lowers the slot to the readonly view. Anything else unifies as usual,
   and a conflict is `type-mismatch`.
4. At the end of argument checking, `?T` is bound to the slot's value,
   and each argument whose type was `mut X` gets a weakening coercion.
5. An expected type for the result was unified first (section 2.4 step
   2). If it fixed `?T` as `mut X`, a readonly argument is
   `mutable-upgrade`
   ([`types.mut.no-upgrade.inference`](../../spec/lang/04-type-system.md#r-types.mut.no-upgrade.inference)).

### 3.5 The Trail And The One Rollback Contract

The audit found two rollback mechanisms with different promises
(CA-05).
D2 fixed one protocol for every IR builder: a checkpoint is a tuple of
column lengths, and rollback truncates them all, plus the inference trail
([§3.9.5](../../docs/compiler/data-structures.md#395-building-scratch-buffer-checkpoints-truncation)).
The checker uses exactly that protocol.

**Rule TC-5. One rollback contract.** Every piece of body state that a
check can change is in exactly one of these four classes. The review
(T2) found fields that the first version left out; the table lists every
field of section 13.

| Class | Rollback | Fields |
| --- | --- | --- |
| **append-only column** | truncated to its checkpoint length | through the builder: TIR instructions, `extra`, labels, locals (one table with the checker's columns), sub-bodies, captures, side tables, reserved slots, the body-local pool and its `resolved` column. In the checker: buffered diagnostics, obligations, watch edges, the wake queue, row facts, pending call records, init facts, scope bindings, literal members, the statement's open literal classes, join items, the "reported once" list |
| **trailed slot** | the trail restores the old value | per variable: parent, rank, binding, kind, blame, `watch_head`, literal-class data (signed, first span, held by a join, defaulted); per obligation: state; per local: flags and the definite-assignment bits; per scope: its newest binding; per open `fns` or `loops` frame: the head of its join-item list; per pool row below the checkpoint: a `resolved` entry |
| **balanced stack** | equal depth at the trial's end, asserted | `scopes`, `loops`, `fns`, `avail`, `restricted`, the divergence flag. A trial checks whole expressions, which push and pop in pairs. A frame's fields other than its join-item head never change after the push |
| **kept on purpose** | never rolled back | fuel (rule TC-8); the solver's body memo and its `met` set (crates/hd_check/DESIGN.md (trait solver) rule TS-5); the trial memo (section 2.5); an M1 body checked during a trial, which runs in its own `BodyCx` and is final (TC-4) |

**Contents, not only containers (Codex re-review N10).** Equal stack
depth does not show that a frame that existed before the trial is
unchanged. Two writes reach such state, and each now has a class:

- **Join items.** A trial may check an argument that holds an early
  `return` or `break`. That appends a join operand, and maybe a held
  literal class (section 3.6), to an enclosing `fns` or `loops` frame
  that is older than the trial. These items no longer live in the
  frame. They are rows of one append-only `join_items` column, each
  `(item, next)`, and the frame keeps only the trailed head of its list,
  as watch edges do. Rollback truncates the rows and restores the head.
- **Fills of older slots.** A trial may bind a variable that wakes an
  obligation created before the trial, such as a stalled method call
  (section 1.5, case 4). Retrying it may teach bindings, which are
  trailed, and fill the call's slot. A slot reserved before the
  checkpoint is not filled during the trial: the checker appends the
  fill to its `deferred_fills` column, whose length is in its
  `Checkpoint`; rollback truncates the column, and the column is applied
  in order once no trial is open
  ([§3.9.5](../../docs/compiler/data-structures.md#395-building-scratch-buffer-checkpoints-truncation),
  Codex re-review N10 and N-I1). So a discarded candidate's callee is
  never installed, and the builder keeps no fill log.
- **The statement's open literal classes** were a stack. A trial adds
  classes to them, so they are now an append-only column, truncated
  like the others.

Three fields needed a new layout to fit a class:

- **Watch lists** were a `next` column on each obligation, which cannot
  put one obligation on several variables' lists. They are now an
  append-only edge table, with each variable's head trailed (section
  2.7).
- **The `resolved` memo** of the local pool maps a local type to its
  global resolution. A trial that binds `?T := i32` and resolves a
  pre-existing `List[?T]` must not leave `List[i32]` cached after
  rollback. A write to `resolved` for a row older than the checkpoint
  goes on the trail; a newer row is truncated anyway.
- **The error count** that decides whether a trial failed is the number
  of error diagnostics in the buffer, so truncation restores it.

A checkpoint records each column's length and the trail's length. The
builder's lengths, the local pool's included, are in its `TirCheckpoint`
([§3.9.5](../../docs/compiler/data-structures.md#395-building-scratch-buffer-checkpoints-truncation)),
so the checker's part does not repeat them. Rollback pops the trail,
restoring each old value, and truncates each column. No checker state is
ever cloned.

```rust
#[derive(Copy, Clone)]
pub struct Checkpoint {                  // the checker's part; `tir` holds the local pool's length
    trail: u32, diags: u32, errors: u32, obligations: u32, watch_edges: u32,
    wake_queue: u32, row_facts: u32, pending_calls: u32, init_facts: u32,
    scope_binds: u32, lit_members: u32, lit_open: u32, join_items: u32,
    reported: u32,
    stacks: StackDepths,                 // asserted equal at rollback, not restored
    tir: tir::TirCheckpoint,
}

enum Undo {                              // 8 bytes: tag + index; old value in `trail_old`
    Parent(InferVar), Rank(InferVar), Bind(InferVar), Kind(InferVar),
    Blame(InferVar), WatchHead(InferVar), LitClass(InferVar),
    ObState(ObId), LocalFlag(LocalId), Assigned(LocalId), ScopeHead(ScopeId),
    JoinHead(FrameId), Resolved(PoolRow),
}

impl<B: TirSink> Checker<'_, B> {
    fn trial<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> (R, bool /* had error */) {
        let c = self.checkpoint();
        let r = f(self);
        let failed = self.diags.errors() > c.errors;
        self.rollback(c);
        (r, failed)
    }
}
```

- **Path halving writes too.** `find` shortens paths, which writes
  parents. Inside a trial those writes go on the trail like any other.
  When no trial is open, the trail is cleared at each statement
  boundary, since nothing can roll back past that point, so halving costs
  nothing extra there.
- **The solver's answers own their storage.** Body-memo keys and answers
  live in the memo's own arena, over canonical placeholders, never in the
  local pool. So truncating the pool cannot leave an entry dangling
  ([crates/hd_check/DESIGN.md (trait solver) §7.1](#71-memo-keys-and-eligibility)).
- **Users of `trial`:** the expected-result attempt (section 2.4), the
  instantiation choice (section 2.5), LCT joins (section 4.3), key
  collision checks (section 5.2), and hole candidates (section 10.4). No
  other code rolls back.
- **Trials nest, within fuel.** Every trial charges a fixed cost plus the
  steps it takes, so fuel bounds the product of nested trials.
- **Debug check (mine).** In debug builds, `rollback` compares a content
  hash with the one taken at the checkpoint. The hash covers every
  trailed slot, every column length, the stack depths, the fields of
  every open frame, and the fill state of every reserved slot older than
  the checkpoint. It also checks the indices that slots point at: no
  `watch_head`, join head, obligation or scope head may point past its
  column's length. A mismatch is an internal error naming the trial's
  span. This is the verifier for TC-5.

### 3.6 Literal Widths

This follows [Open Literal Width](../../spec/lang/04-type-system.md#open-literal-width)
and the plan in [Literal Inference](../../docs/compiler/goals.md#literal-inference).

**Variables.** A literal with an expected numeric type of its kind takes
it on the spot and range-checks (`integer-literal-range`,
`float-literal-range`). A literal with no such type gets a fresh
variable of kind `IntLit { signed }` or `FloatLit`. An integer literal
never takes a float type, and the reverse
([`types.literal.int-not-float`](../../spec/lang/04-type-system.md#r-types.literal.int-not-float)).

**Classes.** Unifying two literal variables merges them. The merged
root's kind is `IntLit { signed: a.signed || b.signed }`. So all literals
that meet each other form one **class**, which is the spec's literal
group as far as the literals are connected. Unifying a class with a
concrete type binds the whole class: an integer type for `IntLit`, a
float type for `FloatLit`, else `type-mismatch` at the blame span. Every
literal of the class is range-checked against the width when the class
binds, which is linear in the class.

**Defaulting.** At the end of each statement, every class still open
takes its default: `i32` if the class holds a signed literal, `usize`
otherwise, `f64` for floats
([`types.literal.local.default`](../../spec/lang/04-type-system.md#r-types.literal.local.default)).
The statement's open classes are a per-statement list, so defaulting
walks only them.

**Joins that cross statements (review T5).** The return paths of one
closure, and the `break` and `else` values of one loop, join although
they sit in different statements
([`types.literal.local.form.closure-return.statements`](../../spec/lang/04-type-system.md#r-types.literal.local.form.closure-return.statements)).
So `return 0` early in a closure whose last value is an `i64` must give
`0` the type `i64`, not `usize`. The mechanism:

1. Each open join (a body with an omitted result, or a value loop with no
   expected type) keeps a list of the values it will join, and a list of
   **held classes**. Both are rows of the `join_items` column, linked
   from the join's frame (section 3.5).
2. When a `return` or `break` operand is checked, every open literal
   class reachable through its type is marked held (a trailed flag on
   its root) and added to the join's list. That covers a bare literal,
   and also one nested in a list, tuple, optional or any other type, as
   in `return [0]` (Codex re-review N-T5;
   [`types.literal.local.form.join-open`](../../spec/lang/04-type-system.md#r-types.literal.local.form.join-open)).
   The walk visits only subterms with the `HAS_INFER` flag. Defaulting at
   the end of a statement skips held classes, with one exception: a held
   class that a `let` binding of the same statement also reaches takes
   its default there, because the binding's type is fixed at the end of
   its statement. The join then sees the defaulted type.
3. At the join (the end of the body, or the end of the loop), the LCT
   fold of section 4.3 runs over the collected values. A held class meets
   the typed values there and binds. A held class still open after the
   fold takes its default then.
4. A held class that meets a type earlier, inside its own statement,
   binds at once as usual; holding only delays defaulting.

A literal that leaves its statement some other way, for example in a
`let` binding, takes no part: the binding's type is fixed at the end of
its statement ([`types.literal.local.statement`](../../spec/lang/04-type-system.md#r-types.literal.local.statement)).
The spec names closures only. This design treats a private function or
local `fn` with an omitted result the same way, since its result is the
same kind of LCT site
([`types.lct.sites`](../../spec/lang/04-type-system.md#r-types.lct.sites));
section 16.1 reading 1 asks to confirm.

**Where a literal meets a type without unifying.** Three forms need care:

| Form | Rule | Mechanism |
| --- | --- | --- |
| converted to `Any` or a trait value, `let x: Any = 42` | the group default ([`types.literal.local.erased`](../../spec/lang/04-type-system.md#r-types.literal.local.erased)) | the coercion defaults the class first, then wraps |
| several instantiations fit only by width, `price.add(5)` | the default ([`types.literal.local.instantiation`](../../spec/lang/04-type-system.md#r-types.literal.local.instantiation)) | the instantiation choice defaults and re-chooses (section 2.5) |
| a method on an open literal receiver, `5.max(n)` or `10.halve()` | a type only an impl names is not met | the **family rule** below |

**The family rule (mine).** For a method call whose receiver is an open
literal class, the checker asks `Methods` once per width of the class's
kind (at most 10, memoized per name and kind). If every width has the
method from the same trait, or from one inherent family that std
declares per width, the receiver stays open, and the call's other parts
solve it: `5.max(n)` with `n: i64` gives `i64`. Otherwise the class takes
its default before lookup, so `10.halve()` with only `impl Halve for i64`
is an error against `usize`, as the spec's note requires.

**No statement retry (owner, 2026-10-07).** A literal's width is decided
once per connected literal class, by union-find, not by re-checking a
failed statement. The prototype re-checked, with a weaker rollback and
with widths scraped from diagnostic text. With classes, a literal binds
to the width it meets the moment it meets it, so the default is used
only when nothing decides the class. In `(pick(1, big), pick(2, small))`
with `big: i64` and `small: i32`, `1` is `i64` and `2` is `i32`. The spec
pass rewords
[`types.literal.local.join`](../../spec/lang/04-type-system.md#r-types.literal.local.class.meet)
to say so. A test-only **oracle mode** implements the old retry rule
literally, over the same trail, and the differential fuzzer compares the
two (section 14). After the rewording, a difference that only separate
classes explain is expected; any other is a bug.

**The `+N` style.** When a class defaulted to `usize` later meets a
signed type or a negative operation, the error points at the literal and
suggests the fix there
([`types.literal.local.hint`](../../spec/lang/04-type-system.md#r-types.literal.local.hint)):
`+0` for `i32`, and an annotation such as `let total: i64 = 0` for other
widths. The class root keeps the span of its first literal and whether
it was defaulted, so the hint costs nothing until it is needed.

## 4. Coercions And Joins

### 4.1 Coercion Sites And Order

`coerce(found, want)` runs where the spec allows an implicit conversion:
assignments, bindings with a written type, arguments, return values,
field initializers, collection elements with an expected element type,
and branch values with an expected type
([`types.option.wrap.sites`](../../spec/lang/04-type-system.md#r-types.option.wrap.sites),
[`req.row.subsume.sites`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume.sites)).
It tries the rules of
[Assignability And Coercion](../../spec/lang/04-type-system.md#assignability-and-coercion)
in a fixed order, and at most one applies:

1. `found` or `want` is poison: succeed, no node.
2. `found` is `never`: succeed with a `Never` coercion
   ([`types.assign.never`](../../spec/lang/04-type-system.md#r-types.assign.never)).
3. Identical after alias expansion, or unifiable (variables bind): no
   node.
4. `mut T` to `T`: `Weaken`.
5. A declared variance conversion on a readonly outer type: `Variance`.
6. `T` to `T?`, one layer: `WrapSome`
   ([`types.assign.optional`](../../spec/lang/04-type-system.md#r-types.assign.optional)).
7. A function type whose row the target row entails, the rest unifying:
   `RowSubsume` (section 5.3).
8. `S` to a trait value `Tr` when `S: Tr` holds: `ToTraitValue` with the
   evidence; `mut S` to `mut Tr` likewise
   ([`types.assign.trait-value.mut`](../../spec/lang/04-type-system.md#r-types.assign.trait-value.mut)).
9. A child trait value to a supertrait value: `Supertrait`.
10. A suspending function type to its constructor form: `SuspendFnToCtor`
    ([`req.suspend.type.conversion`](../../spec/lang/11-requirements-and-suspension.md#r-req.suspend.type.conversion)).
11. Otherwise an error. The code depends on the pair: a readonly value
    where `mut` is wanted is `mutable-upgrade` or
    `readonly-argument-to-mutable-parameter`; a narrower or wider numeric
    type is `implicit-narrowing` or `type-mismatch` with a cast fix-it;
    otherwise `type-mismatch`.

Rule 3 comes before the others, so a variable never binds through a
conversion. In `let x: i32? = id(5)`, `id`'s `T` is solved from the
expected type `i32?` first (section 2.4 step 2). Only if that fails does
the result wrap.

### 4.2 Coercion Instructions

**Rule TC-6. Every implicit conversion is an explicit TIR `Coerce`.** D2
never re-derives one, and TIR's invariant 4 (every operand has the type
its position expects) holds only because of this rule.

| Checker coercion | From → to | TIR `Coerce` kind | Run-time meaning for D2 |
| --- | --- | --- | --- |
| `Never` | `never` → any | `never` to any | unreachable |
| `Weaken` | `mut T` → `T` | readonly view | none: a static view change |
| `Variance` | `C[A]` → `C[B]` by declared variance | variance (crates/hd_tir/DESIGN.md catalog) | none, by [Representation-Preserving Variance](../../spec/lang/04-type-system.md#representation-preserving-variance) |
| `WrapSome` | `T` → `T?` | option wrap | build `.Some` |
| `RowSubsume` | `fn ... $ R1` → `fn ... $ R2` | row subsumption | none: every function value takes a context and looks its keys up by id, so a context holding `R2`'s keys serves `R1` ([crates/hd_mono/DESIGN.md §12.4](../hd_mono/DESIGN.md#124-rows-and-providers), Codex re-review N-I2) |
| `ToTraitValue` | `S` → `Tr`, `mut S` → `mut Tr`; `Any` included | to trait value, to `Any` | box with its dispatch table; the impl choice is in the record |
| `Supertrait` | child trait value → parent trait value | supertrait (crates/hd_tir/DESIGN.md catalog) | re-table: load the parent's vtable from the child's |
| `SuspendFnToCtor` | `fn!` type → constructor type | suspending function to constructor | none, or a thin adapter |

**Declared variance (P2-1b `53563951`, `hd_resolve/src/variance.rs`).**
Variance markers are declared, not inferred, and the `Variance` step
above applies them; anything else is `invalid-variance`.

The error conversion of `?` is not a coercion: it is a `Call` of the
resolved `From` method on the failing path (section 4.4). Literal widths
and numeric casts are not coercions either. A literal's width is its
type, and a cast is a call written in source.

### 4.3 Least Common Type

The sites are list and map literals, value-producing `if`, `match` and
loops, and omitted closure and private results
([`types.lct.sites`](../../spec/lang/04-type-system.md#r-types.lct.sites)).
With an expected type, the site does not join: each value is checked
against the expected type. Without one:

1. **Drop `never`.** If every value is `never`, the type is `never`.
2. **Contextual variants refuse.** A `.None` or other contextual variant
   has no type to offer: `missing-contextual-enum-type`
   ([`types.lct.contextual.error`](../../spec/lang/04-type-system.md#r-types.lct.contextual.error)).
3. **Rows first.** If the values are function types, widen each row to
   their union
   ([`types.lct.row-union-every-site`](../../spec/lang/04-type-system.md#r-types.lct.row-union-every-site)).
4. **Fold.** Join the values left to right with a binary `join(a, b)`.
   It tries, in order: unify with no conversion; weaken the outer `mut`
   of either side; inject one side into an optional by one layer;
   declared variance on a readonly outer type. It never combines
   weakening with a variance step for one candidate
   ([`types.lct.no-combine`](../../spec/lang/04-type-system.md#r-types.lct.no-combine)).
   It never uses `Any`, a trait value or a supertrait.
5. **Uniqueness.** If two steps give incomparable candidates, the error
   is `no-least-common-type`. If no step applies, it is `no-common-type`
   ([`types.lct.no-least`](../../spec/lang/04-type-system.md#r-types.lct.no-least)).
6. **Coerce.** Each value then gets the coercion from its type to the
   joined type, as a `Coerce` instruction. A branch or arm tail was
   emitted before the join was known, so it reserved a slot, which the
   join fills now (section 1.5).

Each `join` is a trial over two types and costs fuel per step. The fold
is linear in the number of values, so a list literal of 100,000 elements
joins in linear time. Literal elements are classes: `[1, 2, -3]` makes
one signed class, which defaults to `i32`.

### 4.4 Propagation

For `e?` on `Result[T, E]` in a function returning `Result[U, F]`:

1. `E` reaches `F` by one coercion of section 4.1, or else by one
   `From[E]` impl of `F`, never both and never chained
   ([`types.result.propagate.one-step`](../../spec/lang/04-type-system.md#r-types.result.propagate.one-step)).
   The second is an `Implements` goal, and its evidence becomes the
   choice of a `From` call.
2. The checker emits D2's desugared form: a `SwitchTag` on the result;
   the failing leaf converts the error (a `Coerce` or the `From` call)
   and `Return`s; the other leaf yields the success value.
3. On an optional, the enclosing result must be optional too, and the
   exit returns `.None`.
4. In a test body, `?` follows
   [Propagation In Test Blocks](../../spec/lang/05-expressions.md#propagation-in-test-blocks).

## 5. Rows And Suspension

### 5.1 Representation

A row in a body is a small structure over interned parts:

```rust
pub struct BodyRow {
    keys: RowId,                          // interned sorted key list (D1 §4.13.4)
    params: SmallVec<[RowParamRef; 1]>,   // declared row parameters listed in the row
    pending: SmallVec<[PendingRow; 1]>,   // private callees' rows, solved in M3
}
/// RowVar(g) as seen from one call: g's solved row, with g's generics
/// replaced by the call's arguments, minus the keys that `$.with` blocks
/// around the call provide.
pub struct PendingRow { var: RowVar, subst: SubstRef, minus: RowId }
pub struct RowVar(u32);   // the private callable's item index in its module: stable, not body-local
```

- Keys are sorted by `Ty` value for in-run merges. They are re-sorted by
  stable content for printing and hashing (D1 §4.13.4).
- Entailment is membership after alias expansion
  ([Entailment](../../spec/lang/11-requirements-and-suspension.md#entailment)).
- `subst` maps the callee's type parameters and row parameters to the
  call's arguments, so one generic callee called at `T = i32` and at
  `T = string` gives two pending rows (review finding 7).
- `minus` exists because a call inside `$.with K=...` needs only
  `RowVar(g)` without `K` from the caller.

### 5.2 Available Keys, Providers And `$.use`

Each body keeps an **available stack**. The body's declared row is at
the bottom, and each enclosing `$.with` block pushes an entry on entry
and pops it on exit. `available` is their union
([`req.row.entail.available`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.entail.available)).

- `$.with K=p: body`: check `p` against `K`. For a mutable requirement
  trait, `p` must be `mut`, else `mutable-upgrade`. Check generic key
  collisions among the new keys and the visible ones
  (`generic-requirement-key-collision`): unify the two keys inside a
  trial, with rigid parameters replaced by fresh variables. Then push.
- `$.use(K)`: `K` must be in `available`, else `missing-requirement`.
  The result is `mut K` for a mutable requirement trait, else `K`.
- A private function with an inferred row records each key it uses
  outside its own `$.with` blocks as a row fact. A closure with an
  inferred row adds such keys to its own row instead (section 5.4).

### 5.3 Call Checking

For a call to a callee whose substituted row is `R`:

| `R` holds | Check |
| --- | --- |
| concrete keys | each key in `available`, else `missing-requirement` naming the key; when the caller is a function whose header the user wrote, a fix-it adds the key to that header |
| a row parameter `$R` of the callee | `R` was solved from an argument's function type (least solution, section 5.6), then as above |
| the caller's own row parameter | entailed only if the caller's row lists it |
| `RowVar(g)` of a private callee | a deferred check, recorded as a row fact (section 5.5) |
| pending parts, in the row of a function value being called | each part as the call of that private callee: a deferred check (section 5.4) |

A function value checked against an expected function type uses row
subsumption: the expected row must entail every key of the value's row
([`req.row.subsume`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume)).
When the value's row holds a `RowVar`, that check is deferred too.

### 5.4 Closure Rows

A closure without a `$` clause gets the least row that contains every
key its body uses outside its own `$.with` blocks. A `$.with` around the
closure never satisfies its keys
([`req.row.omitted.outer-scope`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.outer-scope)).
The closure body is part of the same body task, so its row is known when
its last statement is checked. If it calls a private callee with a row
variable, its row holds that variable. With an expected row parameter,
the inferred row unifies with it
([`req.row.omitted.expected-parameter`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.expected-parameter)).

**A closure owns its row (Codex re-review N8).** Creating a closure value
runs none of its body, so the closure's requirements belong to its
function type, never to the body that creates it.

- The closure's row is a `BodyRow` (section 5.1): the keys it uses, plus
  one pending part for each private callee it calls. No row fact names
  the creator.
- A call inside the closure records its `PendingCall` with
  `caller: Closure(sub_body)`. Its providers come from the closure's own
  provider bundle, which the closure's caller passes at each invocation.
- The creator takes part only where it **invokes** a function value. A
  call of a value whose row holds pending parts is checked as a call of
  each of those private callees (section 5.3), so the invoking body
  records `Includes` or `Entails` facts for them. Returning, storing or
  discarding the value adds nothing.
- So a private factory that returns a closure calling private `tick`,
  whose row is `{Clock}`, stays requirement-free. Its result type holds
  the pending part `RowVar(tick)`, and the row sweep (section 5.5 step 6)
  turns it into `{Clock}`.
- A cold suspension is different. A plain call of a suspending callee
  captures the caller's providers when the value is made (section 5.7),
  so that call records its facts under the caller, as any call does.

### 5.5 Private Rows And The M3 Fixpoint

> As built (#194): rows are solved before the module's items are written, with no pending rows in types or TIR; see crates/hd_tir/DESIGN.md §4.13.1 "M3: inferred rows, as built". The text below is the original plan.

A non-public function, method or local `fn` without a `$` clause has an
inferred row
([`req.row.omitted.inferred-private`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.inferred-private)).
Rows never order checking (D1 §4.13.1): bodies record facts, and M3
solves them. The review (finding 7) found the first version's solve and
its output incomplete; this section replaces it.

**What a body records.**

```rust
pub enum RowFact {
    Uses     { f: RowVar, keys: RowId },                   // f's body uses keys outside its own `$.with` blocks
    Includes { f: RowVar, g: PendingRow, at: Span },       // f's row must contain g's row, substituted, minus
    Entails  { avail: RowId, params: ParamSetRef, g: PendingRow, at: Span }, // a written row must contain it
}

/// One call whose providers M3 fills. Appended in emission order.
pub struct PendingCall {
    inst: Inst,                           // the call; its provider word holds `Pending(index of this record)`
    g: PendingRow,                        // what the callee needs, as seen from this call
    with: Range32,                        // (key, provider Ref) pairs of the `$.with` blocks around the call
    caller: CallerRow,                    // Written(row) | Inferred(RowVar) | Closure(sub_body): where the other keys come from
}
```

The `with` pairs are the lexical provider environment at the call, so
M3 can pick each provider without looking at the body again. A call
inside a closure names the closure as its caller, never the enclosing
callable (section 5.4).

**The solve, in M3, per module:**

1. **One set per row variable.** Each `RowVar` starts from its own `Uses`
   keys, not from a union over its SCC. Then the solve is a worklist over
   `Includes` edges: when `row(g)` grows, each edge `f ← g` adds
   `subst(row(g)) − minus` to `row(f)`, and `f` is queued if it grew.
   The transfer is monotone, since `minus` is a constant per edge, so the
   result is the least solution
   ([`req.row.omitted.cycle`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.cycle)).
   In the review's example, `f` uses `Clock` and calls `g`, and `g` calls
   `f` inside a `$.with Clock=...` block. The solve gives `row(f) =
   {Clock}` and `row(g) = {}`; `g` never over-requires `Clock`.
2. **Order.** The worklist starts with the row variables in source
   order and pops in queue order, so the solve is deterministic. SCCs
   are not needed for correctness; Tarjan's order is kept as the initial
   queue order, so most rows finish in one pass.
3. **Substitution and growth.** `subst` can make a key larger, as when a
   generic `g[T]` calls itself at `List[T]` and needs `Repo[T]`. That is
   polymorphic recursion in rows. A key whose type-argument nesting
   passes the instantiation depth limit stops the solve for that row
   with `instantiation-too-deep` at the call that added it, the code the
   owner chose for polymorphic recursion (2026-10-07). With that cut,
   every row is finite. Each addition costs one step of the module's M3
   fuel.
4. **Entailment checks.** Each `Entails` fact is checked against the
   solved rows. A missing key is `missing-requirement` at the call. A
   note names the callee chain that brought the key in: each row keeps,
   per key, the first edge that added it.
5. **Providers.** For each `PendingCall`, in order, and each key `k` of
   `subst(row(g))` in canonical key order: if `k` is in `minus`, the
   provider is the matching `with` pair's `Ref`; otherwise it is the
   caller's own provider for `k`, which its written or solved row now
   holds. A closure's solved row is its own keys plus its solved pending
   parts; it is a plain function of the rows of step 1, since no row
   variable names a closure, so it needs no fixpoint. **A closure
   caller's provider** for `k` is read from the closure's context
   bundle, through the context parameter that every closure takes
   (Codex re-review N-I2): the `Ref` is the closure's context parameter
   with key `k`, never a provider of the enclosing callable. M3 appends the `(key, provider)` pairs to the body's `extra` and
   patches the call's provider word to point at them. The count of pairs
   was unknown in M2, so the patch is one fixed word pointing at an
   appended range, not words written in place.
6. **The row sweep.** A type can hold a pending row: a closure's function
   type, a local bound to it, a row argument of a generic call. The
   body-end sweep at `finish` leaves such types alone, because their
   pending parts name module-scoped `RowVar`s, not body-local variables.
   After step 5, M3 rewrites every type with the `HAS_ROWVAR` flag in the
   module's bodies (the TIR `ty` column, locals, callee records and
   coercion records) by replacing each pending part with its solved row.
   The flag makes the scan one load per entry. After the sweep, no type
   and no call is pending (TIR invariant 8).

No body is checked again (TC-4). The prototype rechecked every relevant
body in each row round.

Steps 5 and 6 need two things from the backend lane: a pending row that
names a `RowVar` must be internable outside the body-local pool for the
length of `ModuleFinish`, and M3 needs a second write to the `ty` column
and to the type words of records. Section 17 lists them.

### 5.6 Row Patterns And Least Solutions

Matching a parameter's row pattern `$ R + K` against an argument's row
`S` solves `R` as the least row: `S` without the pattern's concrete keys
([Least Row Solutions](../../spec/lang/11-requirements-and-suspension.md#least-row-solutions)).
Patterns are solved one at a time, in the spec's order. A pattern with
two unknown row parameters is a header error that resolution already
reported (`ambiguous-row-pattern`).

When `S` holds a `PendingRow`, the least solution is still expressible:
`R := S` without `K` keeps the pending part with a larger `minus`. But a
pattern whose keys mention a **type parameter**, such as `Repo[A]`,
cannot be matched against a pending row in M2: its keys are not known
yet. That case is `cannot-infer-type`, with a fix-it that writes the
private callee's `$` clause (orchestrator's call, 2026-10-07). The case is
rare and the fix is one line; inferring such rows in M1 instead would
move most private bodies into the serial phase.

### 5.7 Suspension

- `!` is part of a name, so suspension needs no inference
  ([`fn.decl.suspension-not-inferred`](../../spec/lang/07-functions.md#r-fn.decl.suspension-not-inferred)).
- The body context records whether the body is a **driver context**: a
  suspending function or closure body, or a test body. A bang call
  elsewhere is `bang-call-outside-suspension`.
- A bang call whose callee is neither suspending nor a `Suspend[T]`
  value is `not-suspending`.
- A plain call of a suspending callee has type `mut Suspend[T]`, a
  must-use type.
- `all!` and `race!` are intrinsics typed by
  [Typing `all!`](../../spec/lang/11-requirements-and-suspension.md#typing-all).
  Each argument must be a cold `mut Suspend[T]`. An argument that is a
  bang call gets the hint "pass the cold value: write `slow()`"
  (wishlist item 3).
- Each bang call, `s!()`, `all!` and `race!` is emitted with `await_`, a
  suspension point whose side record lists its enclosing scopes. Frames,
  polling and cancellation paths are D2's.

### 5.8 The Direct `block_on` And `println` Ban

By owner answer 13 the ban is **direct-only**. The body context keeps a
**restricted-context depth**. It is non-zero inside:

- a `defer` suite;
- a default expression;
- a fact or metadata expression;
- non-entry module initialization: the init body of a module that is not
  an entry module.

There, a call whose resolved callee is `std.task.block_on` or `println`
is `suspension-forbidden-context`, and so is a bang call
([`flow.defer.suspend`](../../spec/lang/06-control-flow.md#r-flow.defer.suspend)).
The check reads the resolved callee at the call. It never looks into
another body, so it needs no summary and no interface fact. An indirect
call reaches the run-time check that D2 emits. For a `defer` suite, the
diagnostic says to collect the work and do it after the scope (wishlist
item 6). Section 16.2 lists the spec text that still says "transitive".

## 6. GADT Refinement And Tuples

### 6.1 Arm-Local Equalities

**Removed with GADTs** (owner, 2026-10-07, in
[docs/compiler/goals.md](../../docs/compiler/goals.md#summary); the spec removal is pending as S1e). The
language has no pattern refinement and no existential variant
parameters, so the checker has none of the machinery an earlier version
of this section designed: arm equalities, `Rigid` existential
placeholders, birth marks, the open-arms stack, the arm-environment
table, rules TC-10 and TC-11, the `Refine` coercion and the `Evidence`
callee for calls through an existential. A `match` arm is checked like
any other block. Until S1e lands, chapter 13's fixtures are known
failures of the new compiler.

### 6.2 Tuples, Varargs And Spreads

There are no variadic generics
([chapter 12](../../spec/lang/12-variadic-generics.md)).

- A vararg `args...: Args` with `Args < Tuple` takes the call's remaining
  arguments as one tuple. `Args` unifies with the tuple of their types.
- A spread `f(args...)` unifies the spread tuple with the remaining
  parameters. A tuple whose arity is still a variable at that point is a
  stalled obligation, resolved like any other.
- A tuple with a rest element, `(A, B, List[T]...)`, uses the `rest`
  field of the tuple view. A spread pattern binds the rest as `List[T]`.
- `Tuple` is compiler-supplied at every arity: `Evidence::Builtin`.
- Tuple `Eq`, `Ord`, `Hash` and `Debug` come from std's tuple templates,
  so the solver answers them with `Evidence::Impl` of the template at the
  tuple type, as for any impl (crates/hd_check/DESIGN.md (trait solver) change 6).
- `Fn[Args, O, $ R]` unifies `Args` with a function type's parameter
  tuple.

## 7. Patterns And Exhaustiveness

### 7.1 Pattern Typing

Patterns are checked against the scrutinee's type with no coercion
(`check_exact`):

| Pattern | Rule |
| --- | --- |
| `_`, a bare name | catch-all; a name binds the subject's type; a name that resolves to a variant of the subject enum is `bare-variant-pattern` |
| `.V(p..)`, `E.V(p..)` | the subject's enum must be known (`missing-contextual-enum-type`); payload arity (`pattern-arity`); named payloads |
| literal | checked with the subject type as its expected type, so `integer-literal-range` applies |
| range `a..=b` | the subject must be an integer type; each bound is checked against it |
| tuple `(p, q)`, spread `(p, xs...)` | arity and rest rules of [Spread Patterns](../../spec/lang/06-control-flow.md#spread-patterns) |
| data `T { f, g: p }` | nominal; fields exist, are distinct and visible |
| unit `()` | the subject is `void` |

Bindings take access types by the path rules. A direct `mut U` field of
a readonly subject binds as `U`. A generic field keeps its substituted
type, `mut` included
([Payload Bindings](../../spec/lang/06-control-flow.md#payload-bindings)).

### 7.2 Usefulness

Exhaustiveness and unreachable arms use Maranget's usefulness algorithm
over the pattern matrix, as rustc and OCaml do.

1. Rows are the arms in source order. A guarded arm adds no coverage
   ([`flow.match.guard.coverage`](../../spec/lang/06-control-flow.md#r-flow.match.guard.coverage)),
   but its own usefulness is still checked.
2. The constructors of a type: the enum's variants; `true` and `false`;
   one tuple or data constructor; `()`; and for integers, **disjoint ranges** split from
   the literals and ranges present, as rustc splits them. Strings and
   chars are infinite, so only a catch-all covers them.
3. An arm that is not useful is `unreachable-match-arm`. That is an
   error ([`flow.match.unreachable`](../../spec/lang/06-control-flow.md#r-flow.match.unreachable)).
   The spec has no redundancy warning, so neither does the checker.
4. If the wildcard row is still useful after every arm, the match is
   `nonexhaustive-match`. The witness, such as `.Err(_)` or
   `(.None, _)`, becomes the diagnostic's example, and later a fix-it
   that adds the missing arm.

**Fast path (mine, after rustc).** When every arm is a distinct literal
or variant in one column with no nesting, the checker uses a hash set in
linear time and builds no matrix. That covers the common 1,000-arm
literal match.

**Limit.** Every matrix cell visited costs one fuel step. When fuel runs
out during a usefulness check, the limit diagnostic is
`match-too-complex` on the `match` instead of `item-too-complex`, and the
body stops as section 11.2 says.

**Decision trees.** The same specialization code compiles the matrix into
a decision tree, emitted after the arms as TIR's `SwitchTag`, `SwitchInt`,
`SwitchChar`, `SwitchStr`, `Guard` and `ToArm` instructions. A shared arm
appears once. In no-emit mode no tree is built.

### 7.3 `let` Patterns And `let-else`

- `let p = e` with a refutable `p` and no `else` is
  `refutable-let-pattern`.
- An `else` after an irrefutable pattern is `unreachable-match-arm`.
- The `else` block must diverge: its type must be `never`, by the
  divergence flag of section 2.3. Otherwise it is
  `let-else-falls-through`
  ([`flow.let.else.diverge`](../../spec/lang/06-control-flow.md#r-flow.let.else.diverge)).
- Refutability is one usefulness question: is the wildcard useful after
  the single row `p`?

## 8. Mutability, Access And Initialization

### 8.1 Access Types And Bindings

Every expression's type carries its access, `mut T` or `T`. Access is
computed one path step at a time
([Mutable Paths](../../spec/lang/04-type-system.md#mutable-paths)):

| Step | Access of the result |
| --- | --- |
| binding, parameter, `self` | its declared or inferred type |
| call | the declared result type |
| field read `e.f` | by the field's kind and `e`'s access; a direct `mut U` field through a readonly `e` gives `U` |
| index, iteration, lookup on a built-in collection | the declared element type, whatever the collection's access |
| unwrapping, tuple element, generic payload | the declared contents |

Data, copy-update, stored variant, list and map expressions are fresh:
they produce `mut T` for the new outer object. The binding forms then
apply ([Binding Forms](../../spec/lang/04-type-system.md#binding-forms)):

| Form | Type of the name |
| --- | --- |
| `x := e` | the readonly view of `e`'s type |
| `let x = e` | the readonly view; reassignable |
| `let mut x = e` | `e`'s type, which must be `mut T`, else `mutable-upgrade`; `mut-on-primitive` and `mut-on-tuple` take precedence |
| `let x: mut T = e` | `mut T`; `e` is checked against it |
| `let mut x: mut T = e` | as above, plus `redundant-let-mut` with its fix-it |
| `let mut x: T = e` | `let-mut-readonly-type` with its fix-it |

### 8.2 Mutation Checks

A mutation needs mutable access on exactly one expression, the one it
acts on ([Mutation Checks](../../spec/lang/04-type-system.md#mutation-checks)).
The checker checks it at the mutation, from the access type it already
computed:

| Mutation | Error when the target is readonly |
| --- | --- |
| `mut self` method call | `mutable-receiver-required` |
| store through a readonly edge `field: U` | `readonly-edge` |
| store through any other readonly root | `readonly-root` |
| argument to a `mut T` parameter | `readonly-argument-to-mutable-parameter` |
| reassigning a `:=` binding | `non-reassignable-binding` |

When the target is a local, `mutable-receiver-required` and
`readonly-root` carry the `let mut` fix-it. It rewrites the local's
declaration, `x := e` or `let x = e`, to `let mut x = e`. When the
initializer is readonly, which `let mut` would reject, the fix-it is a
`Suggestion` that names the fresh-copy form instead (section 10.5).

**Access implementation (P2-1b `53563951`, `hd_check/src/access.rs`).**
Fresh data, list and map literals are `mut T`, except when a readonly
value sits in a direct `mut U` field; a newtype carries its base value's
permission. Inference binds `T = mut U` and joins several arguments at
the readonly view; `T < mut Trait` bounds give mutable access and reject
readonly arguments, and mutable requirement traits need `mut` providers.
Field reads follow the edge kind; `mut self` calls, loops over an
iterator, and driving a suspension need mutable receivers. Nested
permissions convert only by declared variance, else `type-mismatch`.

### 8.3 Local Flags

Each local has a flags byte, written through the trail: `READ`,
`ASSIGNED`, `MUTATED`, `CAPTURED`, `CAPTURED_ASSIGNED`. It is the
`local_flags` column of the one local table that the TIR builder owns
([§3.19](../../docs/compiler/data-structures.md#319-per-body-checker-scratch)); the checker
keeps no second copy. With no extra
pass they give:

- `unused-local-binding`, and the error for an unread must-use binding,
  at the end of the body;
- the `mut` fix-it of `cannot-infer-type` (section 2.8);
- capture modes for TIR's `Closure` instruction: `Copy` when no side
  writes the local after the capture, `Move` when only the closure uses
  it afterward, `Shared` when both may
  ([`fn.capture.storage.shared`](../../spec/lang/07-functions.md#r-fn.capture.storage.shared)).
  The mode depends on statements after the closure, so the checker
  emits a provisional mode and supplies the final one in its `Solution`
  at `finish` (section 17). Storage representation is D2's; the
  prototype's capture-cell pass, which the emitter imported, has no
  counterpart.

### 8.4 Definite Initialization

**Locals: during checking, not over TIR (mine).** A `:=` binding can sit
inside an expression and binds into the enclosing scope, so a name can
be in scope on a path that skipped its initializer
([Definite Initialization](../../spec/lang/03-names-and-scopes.md#definite-initialization)).
hd's control flow is structured, so the checker computes definite
assignment while it checks, syntax-directed as Java's rules are. It
keeps a bit set of initialized locals, and for a `bool` condition two
sets, "initialized when true" and "when false":

- `a && b`: `b` starts from `a`'s when-true set; the result is
  initialized when true after `b` is true, and when false only where both
  false paths agree. `||` is the mirror, and `!` swaps the two sets.
- `if`: the then-branch starts from the condition's when-true set, the
  else-branch from its when-false set. After the `if`: the intersection
  over the branches that complete normally, since a diverging branch is
  not an incoming path
  ([`names.definite.diverging`](../../spec/lang/03-names-and-scopes.md#r-names.definite.diverging)).
- `while`: the body starts from the when-true set; after the loop, the
  when-false set intersected with the set at each `break`.
- `match`: each arm starts from the set after the scrutinee; after it,
  the intersection over arms that complete normally.
- A read of a local whose bit is clear is
  `possibly-uninitialized-binding`.

So `if flag && (name := load()) != "": greet(name)` is accepted, and a
use of `name` after the `if` is rejected, as the spec's examples need.
The bits are trail slots (section 3.5). The pass reads no TIR, which
keeps rule TC-3 true.

**Top-level bindings.** Within one module, M3 decides
[`module.init.definite`](../../spec/lang/10-modules.md#r-module.init.definite)
from `InitFacts`. For each function and top-level statement they list
the top-level bindings it reads directly, the functions of the module and
folder it references (calls and values), and the trait methods it
dispatches through a bound or a trait value. The checker records them
from resolved references while it checks. M3 computes transitive read
sets per statement in source order and reports
`top-level-read-before-initialization`. An **init group** across modules
of one folder uses the same facts through D1's `InitOrder(F)` task
([§4.13.10](../hd_tir/DESIGN.md#41310-module-initialization)).

## 9. Omitted Result Types (M1)

### 9.1 Who Can Omit

A non-public function, a non-public inherent method and a local `fn` may
omit `-> T`
([`fn.decl.result-omitted-private`](../../spec/lang/07-functions.md#r-fn.decl.result-omitted-private)).
Public functions, trait methods and impl methods cannot
(`missing-result-type`, a header error).

**Hidden private items a template names (review finding 6).** A derive template may
call private helpers of its trait's module, which the interface exports
as hidden items (owner, 2026-10-07). A dependent checks the instantiated
template, so it needs each helper's signature. To keep "interfaces come
from syntax alone", this design proposes that a private item named by a
template body follows the public signature rules
([`module.package.annotated`](../../spec/lang/10-modules.md#r-module.package.annotated)):

- a function writes its result type, else `missing-result-type`;
- a function without a `$` clause has the empty row, as a public one
  does ([`req.row.omitted.empty-pub`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.empty-pub)),
  so its body must provide any key it uses itself;
- a top-level binding is not a helper: a template may not name one.

Resolution applies these rules at interface time from the template's
resolution table (crates/hd_resolve/DESIGN.md §4.10 step 5), so they
need no body. With them, an omitted result or row never reaches a folder
interface, and inference never enters a dependent's cache key. The rule
is new language text; section 16 question 1 asks the owner.

### 9.2 The M1 Walk

```text
M1(module m):
    state[f] = Todo  for each private callable f with an omitted result
    check the top-level statements in source order       (the init body)
    for each f still Todo, in source order:  visit(f)

visit(f):
    state[f] = InProgress; push a BodyCx for f on the M1 stack
    check f's body: a full body check, final by rule TC-4
    result[f] = the LCT of f's final value and return operands, resolved
    state[f] = Done; pop

on a reference to g inside any M1 body:
    Done       -> use result[g]
    Todo       -> visit(g) first, then continue
    InProgress -> a cycle: recursive-function-needs-result-type, once,
                  on the cycle member first in source order;
                  result[each member] = Poison
```

- The walk is serial within one module. Different modules run their M1
  in parallel.
- The M1 stack is an explicit vector of open `BodyCx`s, not native
  recursion. A chain of 10,000 private functions cannot overflow the
  stack. Its depth is bounded by the module's item count.
- A reference is a call or a use as a value, since both need the type.
- The top-level statements go first, because unannotated top-level
  bindings get their types there and other bodies read them. A function
  that reads a binding whose statement is not checked yet triggers that
  statement first, the same depth-first way. A cycle through a binding's
  type, as in `let a = fn(): b()` and `let b = fn(): a()` at top level,
  is `recursive-function-needs-result-type` on the first binding in
  source order, with the hint "annotate one binding" (owner, 2026-10-07).
- Cycle reporting follows
  [`fn.decl.omitted-cycle.report`](../../spec/lang/07-functions.md#r-fn.decl.omitted-cycle.report):
  the member first in source order, whichever member the walk entered
  first. Adding a result type to any member removes the cycle.

### 9.3 Cost

M1 does each such body's work once, as M2 would have. The cost is lost
parallelism inside one module: a module made only of private functions
with omitted results checks serially. Other modules' tasks run
meanwhile, so the pool stays busy in a multi-module package. The
`parallel-speedup` metric will show whether a real package suffers. The
remedy would be a lint that suggests result types, not a design change.

## 10. Error Recovery And Diagnostics

### 10.1 Errors Are Values

**Rule TC-7. No unwinding.** A failed check returns a poisoned result
and checking goes on. No checker function throws, panics or returns
early to a boundary for a user error. The prototype's `fail()` threw a
`CheckFailure` and stopped the function at its first fatal error
(CA-07).
A Rust panic in the checker is a compiler bug. The scheduler catches it
at the task boundary and reports an internal error naming the item
([§6.4](../hd_sched/DESIGN.md#64-budgets-cancellation-and-the-memory-cap)).

### 10.2 Poison

The poison type follows [§3.6](../../docs/compiler/data-structures.md#36-the-poison-type):

| Source of poison | Reported where |
| --- | --- |
| unknown name | once per name per body; later uses are silent |
| broken header (own module or a dependency) | once, at the header, in its module |
| syntax error node | by the parser |
| a failed check (mismatch, missing method) | at that check; the expression's type becomes poison |
| a limit (fuel, type size, match) | once per body |
| a cycle in M1 | once per cycle |

Propagation rules:

1. `unify` with poison on either side succeeds and binds nothing.
2. `coerce` from or to poison succeeds with no node.
3. A goal whose type holds poison answers `Holds { evidence: Poison }`
   without searching.
4. Method lookup on a poison receiver gives a poison callee, and its
   arguments are still checked, against poison, so errors inside them
   still report.
5. A variable bound to poison is poison. At the end of the body, an
   unsolved variable whose obligations involve poison is not reported.
6. TIR gets a `Poison` instruction, or `Hole` for `_`. Both occur only
   in a module with errors, which D2 does not lower in the first release.

### 10.3 One Diagnostic Per Root Cause

Every diagnostic carries a `RootKey` (D1 §4.14): the body plus the cause.
The cause is one of: a name (unknown name, unknown method on a type), a
local (all errors about one binding's type), an inference variable's
root (all mismatches against one bound variable after the first), a
header item, or a span. Rules:

1. The first diagnostic per root key in content order is kept;
   `ModuleFinish` drops the rest (D1 §4.14).
2. **Mismatch against a blamed variable.** When unifying with a bound
   variable fails, the root key is the variable's root, and its blame
   span is the secondary label. Ten uses of a mistyped `x` give one
   error.
3. **Cascade cut.** After a failed check, the expression's type is
   poison. So a parent expression cannot report about it.
4. **Arguments of a failed call** are still checked, against the
   parameter types if the callee is known, else against poison.

### 10.4 Typed Holes

`_` as an expression outside a pipe step is
`placeholder-outside-pipe`
([`expr.pipe.placeholder-outside`](../../spec/lang/05-expressions.md#r-expr.pipe.placeholder-outside)).
The checker enriches that error. It checks the hole in check mode, so it
knows the expected type at the end of the statement, after literal
defaulting and obligations. The diagnostic's note shows:

- the expected type, rendered with stable paths;
- up to five locals, parameters and module functions in scope whose type
  fits, by a plain unification trial against each candidate in scope
  order, each trial charged to fuel.

The candidates also go into TIR's side table for the `Hole`
instruction, where tools can read them. The hole's type is poison
afterwards, so it causes nothing else. Typed holes are `_` only in the
first release (owner, 2026-10-07); there is no `todo()`.

### 10.5 Fix-Its For Common Mistakes

The writing log (`audit/hd-writing-log.md`) and the
diagnostics wishlist (deleted 2026-10-15; `git show
44890c30:audit/diagnostics-wishlist-2026-10-06.md`)
give the checker's most frequent codes. In the log: `type-mismatch` (24
rows), `mixed-signedness` (15), `unsatisfied-trait-bound` (13),
`unknown-method` (13), `unknown-name` (11), `missing-requirement` (11),
`missing-contextual-enum-type` (10), `mutable-receiver-required` (9),
`cannot-infer-type` (8), `mutable-upgrade` (6), `mut-on-primitive` (5).
Each gets this treatment:

| Code | What the checker knows | Message and fix-it |
| --- | --- | --- |
| `type-mismatch` | both types; the blame span of the deciding unification | "expected `i64` (from this argument), found `i32`". A numeric pair gets the cast fix-it `i64(x)` ([`types.num.no-implicit.fix`](../../spec/lang/04-type-system.md#r-types.num.no-implicit.fix)) |
| `mixed-signedness` | each operand's type, and whether it came from a defaulted literal class | when one side is a defaulted literal, point at the literal: "`index` is `usize` because `0` had no type; write `+0` for `i32`" |
| `unsatisfied-trait-bound` | the goal, `FailInfo` from the solver | names the bound and where it comes from; for `Display` on an optional, "match on it, or use `debug(...)`" |
| `unknown-method` | the receiver, available and unavailable traits | did-you-mean by edit distance over the receiver's methods; a `use` fix-it when an unavailable trait has it |
| `unknown-name` | the scope, the exports of std folders | did-you-mean; a `use std.x.Name` fix-it when a std folder exports the name; for `Ok`, `Some`, `None` the dotted form |
| `missing-requirement` | the key, the caller's header span | a fix-it adding `$ Console` to the caller's header, or `+ Console` to its row; for a trait method impl, the note that impl methods carry no row |
| `missing-contextual-enum-type` | the site | "give it an expected type, as `let x: i32? = .None`"; for a tuple scrutinee of results, "match each value in its own `match`" (wishlist item 2) |
| `mutable-receiver-required` | the local's declaration | the `let mut` fix-it (section 8.2) |
| `cannot-infer-type` | the open variable; local flags at body end | an annotation fix-it in the `mut` form when the binding was mutated: `let names: mut List[string] = []` (wishlist item 1) |
| `mutable-upgrade` | the readonly source | "a readonly value cannot become `mut`; make a fresh copy: `User { ...user }`" |
| `mut-on-primitive` | the binding | fix-it removing `mut`; "a plain `let` is already reassignable" |
| `integer-literal-range` | the literal, the expected type | names the range and a wider type of the same family |

**Suggestions search only keyed inputs.** A did-you-mean or `use`
fix-it may search only what the module's `check` key covers: its own
syntax, the folders in its dependency closure, std and the prelude
([crates/hd_cache/DESIGN.md §5.3](../hd_cache/DESIGN.md#53-key-composition), review A7). So the
`unknown-method` fix-it lists unavailable traits from the dependency
closure only, the `unknown-name` fix-it searches std's exports and the
closure's folders, and a cached diagnostic never names an item whose
change would not invalidate it.

**Message quality rules** (from wishlist item 4 and group 3):

1. **No internal names.** Types render through stable paths with the
   shortest unambiguous name. There are no `__std_` names, so none can
   print. `mut List[char]`, never `mut:List[char]`.
2. **The right file.** A problem in std code is reported in the std
   file, or not at all, never at the user's `use` line.
3. **One hint line.** The compact form keeps at most one `hint:` line
   (D1 §4.14), which is the fix-it's title when there is one.
4. **Fix-its are exact edits.** Each fix-it passes the re-parse check
   before it is marked `Exact` (D1 §4.14).

## 11. Limits And Budgets

### 11.1 What Counts

Every count is a language-level unit, never time or allocation (lesson
3). The body's `Fuel` counts:

| Step | Cost |
| --- | --- |
| an expression or pattern node checked | 1 |
| a unification pair visited | 1 |
| a goal the body meets for the first time | 1 plus heads matched for each node of its proof DAG that the body has not met yet (charged by the solver, rule TS-5) |
| a goal the body has met before | 1 |
| a trial started | 8, plus its steps |
| a trial-memo hit (section 2.5) | 1 |
| a usefulness matrix cell visited | 1 |
| an LCT join step | 1 |
| an obligation retried | 1 |

The default is 2,000,000 steps per body (D1 §4.15). Fuel is per body, so
one item's complexity never fails another item.

**Rule TC-8. Fuel used is a pure function of the body and its frozen
inputs.** A goal's charge depends only on the goal and on whether this
body met it before, never on which body filled the memo. Trials are
charged whether or not they commit. So a body passes or fails its budget the
same way on any thread count, cache state or check order. The test mode
prints `fuel_used` per body, and the determinism matrix compares it
(section 14).

### 11.2 Running Out

When fuel runs out inside a body:

1. Report `item-too-complex` on the item, once, naming it; or
   `match-too-complex` on the `match` if the usefulness check was
   running.
2. Stop checking the body. Its `BodyResult` is poisoned and its result
   type (M1) is poison. Its TIR is rolled back to the body's start and
   replaced by a single `Poison`.
3. No other diagnostic of that body is kept after the limit diagnostic.
   Earlier ones, reported before fuel ran out, stay: they are in content
   order and the same on every run.

Other limits each have their own diagnostic (D1 §4.15):

| Limit | Default | Checked at | Diagnostic |
| --- | --- | --- | --- |
| trait resolution depth | 64 nested subgoals | solver | `trait-resolution-depth` |
| type size | 10,000 nodes | `mk`, by the cached size | `type-too-large` (proposed) |
| nesting depth | 256 | parser; the checker relies on it | `nesting-too-deep` (proposed) |
| M1 depth | module item count | M1 stack | none needed: bounded by construction |
| match complexity | shares body fuel | usefulness | `match-too-complex` (proposed) |

### 11.3 Pathological Cases

| Case | Danger | Design answer |
| --- | --- | --- |
| a method chain of 10,000 calls, `a.b().c()...` | the syntax tree is 10,000 deep on its left spine, though nesting is 1 | **spine iteration (mine)**: the checker walks a postfix chain into a scratch stack and checks it bottom-up in a loop. The same holds for left-deep binary operator chains |
| deep nesting of brackets, blocks and closures | stack overflow | the nesting limit of 256 bounds native recursion; the browser build's stack is sized for it |
| a list literal of 100,000 elements | quadratic LCT or literal grouping | the LCT fold is linear; literal classes are union-find, O(n α) |
| a 10,000-arm literal match | quadratic usefulness | the one-column fast path is linear |
| wide tuple patterns with nested enums | exponential usefulness | fuel per cell; `match-too-complex` |
| nested instantiation choices, `a.add(b.add(c.add(...)))` | trials multiply | the head prefilter; arguments inferred once outside trials; the per-site trial memo, so cost is sites × candidates when contexts repeat; fuel |
| nested choices whose outer candidates give distinct expected types; a tainted closure argument | contexts double per level; tainted trials are never cached | fuel only: `item-too-complex`, which the case expects (section 2.5) |
| a type that grows on each step, `List[List[...]]` built by inference | memory | type size limit at `mk` |
| many obligations waiting on one variable | quadratic waking | watch edges; each obligation is queued at most once per wake round |
| a long chain of private functions in M1 | native recursion | the explicit M1 stack |
| huge rows | quadratic set operations | sorted merges, linear |
| an ill-typed version of each of the above | Swift's slowest cases are errors | poison stops cascades, and the pathological suite runs every case ill-typed too (section 14) |

## 12. Determinism And Parallelism

**Rule TC-9. No shared mutable checker state.** A body task owns one
`BodyCx`. It reads frozen inputs through `&` and writes only:

- its own `BodyCx` and arena;
- the append-only global interners (types, rows, strings), whose IDs
  never reach output (D1 §3.1);
- the solver's global memo, whose entries are pure functions of their
  keys (crates/hd_check/DESIGN.md (trait solver) rule TS-1), written first-writer-wins, and
  published only for completed, context-free goals (rule TS-4).

There are no statics, thread-locals, global counters or caches keyed by
span. The prototype's module-global literal map and map-key facts have
no counterpart.

**Content order.**

- Inference variables, locals and holes are numbered per body from zero.
- Diagnostics carry spans and codes. `ModuleFinish` sorts them by D1's
  total key (file, offsets, code, rendered message).
- The solver returns candidates in content order (stable path, then
  source position), never in table order. The instantiation choice and
  method lookup iterate in that order.
- M1's walk and M3's SCC order follow source order.
- Obligations are retried in creation order.

**Per-body arenas.** Each worker has a bump arena for `BodyCx` and local
types, reset after each body. M1 keeps one arena per open body on its
stack. A body's output (`BodyResult`) is moved out before the reset.

**Parallel shape.** M2 bodies of all modules run in parallel. M1 and M3
are serial within a module and parallel across modules. Nothing in a
body waits on another body, except M1's own depth-first walk, which runs
inside one task.

## 13. Checker Data Structures

All per-body state lives in `BodyCx`, struct-of-arrays, indexed by
`u32` newtypes, with no `Box` per node. Sizes are per entry.

```rust
pub struct BodyCx<'f, B: TirSink> {
    frozen: &'f Frozen,                 // module scope, ifaces, private sigs, solver, config
    tir: B,                             // the TIR builder, or the discarding sink; owns the local table
    fuel: Fuel,
    env: EnvKey,                        // the item's elaborated environment (§1.6)
    // inference table, SoA, indexed by InferVar; every column trailed
    parent: Vec<u32>,                   // 4 B; union-find parent (self = root)
    rank: Vec<u8>,                      // 1 B
    kind: Vec<VarKind>,                 // 1 B; General | IntLit{signed} | FloatLit
    value: Vec<Ty>,                     // 4 B; Ty::NONE when unbound
    blame: Vec<SpanIdx>,                // 4 B; first deciding span
    watch_head: Vec<u32>,               // 4 B; newest watch edge of this var
    lit_flags: Vec<u8>,                 // 1 B; held by a join, defaulted (§3.6)
    // trail
    trail: Vec<Undo>,                   // 8 B each
    trail_old: Vec<u32>,                // 4 B; old values
    // obligations, SoA; append-only except `ob_state`
    ob_goal: Vec<Goal>,                 // 16 B
    ob_span: Vec<SpanIdx>,              // 4 B
    ob_state: Vec<u8>,                  // 1 B; Pending | Woken | Done; trailed
    watch_edges: Vec<(InferVar, ObId, u32)>, // 12 B; (variable, obligation, next edge)
    wake_queue: Vec<ObId>,              // this round's woken obligations
    // locals: the builder's one table (local_ty, local_name, local_syn, local_flags);
    // the checker reads it and writes flags through the trail
    // scopes and contexts
    scopes: Vec<Scope>,                 // balanced stack; each scope's newest binding is trailed
    scope_binds: Vec<(Symbol, LocalId, u32)>, // append-only: name, local, previous binding
    loops: Vec<LoopCx>,                 // break type slot, label, join-item head (trailed), ~16 B
    fns: Vec<FnCx>,                     // result slot, row, driver flag, join-item head (trailed), ~28 B
    join_items: Vec<(JoinItem, u32)>,   // append-only: a join operand or held class, next item (§3.5)
    avail: Vec<RowId>,                  // the available stack (§5.2)
    restricted: u16,                    // §5.8 depth
    lit_open: Vec<InferVar>,            // append-only: open literal classes of the current statement
    lit_members: Vec<(InferVar, NodeIdx)>, // every literal and its node: range checks, hints
    assigned: BitStack,                 // definite assignment sets (§8.4)
    // outputs, append-only
    diags: Vec<Diagnostic>,
    reported: Vec<RootKey>,             // "once per name per body" keys
    row_facts: Vec<RowFact>,
    pending_calls: Vec<PendingCall>,    // §5.5
    init_facts: Vec<InitFact>,
    // reusable scratch, cleared per use, never shrunk within a body
    scratch_tys: Vec<Ty>, scratch_refs: Vec<Ref>, scratch_args: Vec<ArgSlot>,
    memo: BodyMemo,                     // lent to the solver per goal (§1.6); never rolled back
    trials: TrialMemo,                  // §2.5; never rolled back
    spine: Vec<ExprId>,                 // §11.3 spine iteration
}
```

- **Inference table:** 19 bytes per variable. A typical body has tens to
  hundreds of variables, so the table fits in a few cache lines. The
  count comes from the column widths. Whether columns or a small struct
  per variable is faster is measured in slice 3
  ([§3.9.4](../../docs/compiler/data-structures.md#394-tables-as-struct-of-arrays)).
- **Trail:** 8 bytes per entry, plus 4 for an old value. Entries exist
  only while a trial is open, or since the last statement boundary; the
  trail is cleared at each statement boundary outside trials, since
  nothing can roll back past a finished statement.
- **One local table.** The builder owns the locals' columns, and the
  checker reads them (docs/compiler/data-structures.md §3.19). Locals are truncated
  with the builder's checkpoint.
- **No expectation stack.** The expected type is an argument of `check`,
  passed by value. The context stacks (`scopes`, `loops`, `fns`,
  `avail`) are the only stacks, and they are vectors.
- **Scratch buffers** are owned by `BodyCx` and reused, so steady-state
  checking allocates only for outputs.
- **Body-local types** live in the body-local pool
  ([§3.9.2](../../docs/compiler/data-structures.md#392-the-internpool)). At body end the
  checker resolves every type in its outputs, and TIR's `finish` sweeps
  the `ty` column; both intern the variable-free results globally.
- **Budget check.** The inference table and obligations add a few dozen
  bytes per variable and per goal, beside TIR's 26 bytes per instruction
  ([§3.9.7](../../docs/compiler/data-structures.md#397-byte-budgets-and-linear-passes)).

## 14. Testing

### 14.1 Spec Traceability

Each spec section maps to one checker component. A CI script reads the
`r[...]` rule IDs of these chapters and the conformance index, and lists
rules with no fixture, per component.

| Spec area | Component (module in `hd_check`) |
| --- | --- |
| [Literal Types](../../spec/lang/04-type-system.md#literal-types), [Numeric Conversions](../../spec/lang/04-type-system.md#numeric-conversions) | `lit`: literal classes, defaulting, range checks |
| [Assignability And Coercion](../../spec/lang/04-type-system.md#assignability-and-coercion), [Optional Types](../../spec/lang/04-type-system.md#optional-types) | `coerce` |
| [Least Common Type](../../spec/lang/04-type-system.md#least-common-type) | `lct` |
| [Composite Values And Access Permission](../../spec/lang/04-type-system.md#composite-values-and-access-permission) | `access`, `bind` |
| [Generics](../../spec/lang/04-type-system.md#generics), [Type Inference Boundaries](../../spec/lang/04-type-system.md#type-inference-boundaries) | `call`, `unify` |
| [Expressions](../../spec/lang/05-expressions.md) | `expr`, `op`, `call`, `method` |
| [Control Flow](../../spec/lang/06-control-flow.md) | `stmt`, `flow`, `pat`, `usefulness` |
| [Definite Initialization](../../spec/lang/03-names-and-scopes.md#definite-initialization) | `flow` (definite assignment sets) |
| [Functions](../../spec/lang/07-functions.md) | `m1`, `closure`, `call` |
| [Data And Enums](../../spec/lang/08-data-and-enums.md) | `expr` (literals, copy-update), `access` (embedding) |
| [Traits](../../spec/lang/09-traits.md) (body side) | `method`, the solver interface |
| [Modules](../../spec/lang/10-modules.md) (initialization) | `init` |
| [Requirements And Suspension](../../spec/lang/11-requirements-and-suspension.md) | `row`, `m3`, `suspend` |
| [Annotations](../../spec/lang/14-annotations.md) (templates, facts, `@error`) | `derive`, `fact` |

### 14.2 Test Kinds

| Test | What it checks |
| --- | --- |
| conformance typing fixtures (`spec/conformance`) | accept or reject and the `code:` of each diagnostic, through the portable command contract; failures listed in the new compiler's `KNOWN_FAILURES.tsv` |
| differential against the frozen prototype | on the suite and on generated programs: accept or reject, and the set of codes per file; each disagreement triaged as a prototype bug, a new-compiler bug or a spec gap |
| literal oracle | the test-only oracle mode (section 3.6) against the normal mode on every fixture and on fuzzed statements; any difference is a bug or a spec question |
| determinism matrix (D1 §8.1) plus two dimensions | emit versus no-emit (TC-3); `fuel_used` per body equal across threads, shuffled orders, cold and warm memo (TC-8) |
| rollback verifier | debug builds hash body state at each mark and compare at rollback (TC-5) |
| TIR verifier | D2's verifier ([§3.10.3](../../docs/compiler/data-structures.md#3103-invariants-and-verifiers)) on every body in debug builds and CI; a failure is a checker bug, since the checker is the builder's only caller |
| exactly-once counter | a debug counter per body; the run fails if any body is checked twice (TC-4) |
| pathological suite | each case of section 11.3, well-typed and ill-typed, within its fuel and wall-time budget, with its limit diagnostic |
| one-root-cause fuzzer | mutate one token of a well-typed program; count error diagnostics; more than two flags a cascade for review |
| crash fuzzer | `cargo fuzz` over generated and mutated programs: no panic, no internal error, no poison type in a rendered message |

## 15. Prototype Failures And The Rules That Prevent Them

From the checker audit,
the findings and
the architecture directions (deleted 2026-10-15; `git show
44890c30:audit/compiler/NAME` for `checker-2026-10-04.md`,
`findings-2026-10-04.md` and `architecture-directions-2026-10-05.md`):

| Prototype failure | Evidence | Rule here |
| --- | --- | --- |
| types are canonical strings, parsed and compared as text | CA-02 | TC-2: types are handles; printing only in the renderer; `Ty` has no `Display` |
| 18 checker classes share one mutable `CheckerContext` with 21 constructor parameters | CA-03 | TC-9: one owned `BodyCx` per body; frozen inputs by `&`; components are functions over explicit contexts |
| `LazySignatures.get` runs another checker inside a lookup | checker audit, Signature Inference | M1 is an explicit serial phase (section 9); lookups in M2 are pure reads |
| one body checked during lazy result inference, again per row round, again for final HIR | CA-03 | TC-4: each body exactly once; rows by M3 facts; the exactly-once counter |
| two rollback mechanisms with different promises | CA-05 | TC-5: one trail plus append-only truncation; the rollback verifier |
| literal retry scrapes widths from diagnostic text and re-checks statements | `literal-retry.ts` (`namedWidths`) | literal classes in union-find; no statement retry; the oracle mode for checking it |
| literal joining mutates HIR in place; a global span-keyed map retains literal spans across compilations | checker audit; CS-01 | literal widths are variables; IR types resolve once in `finish`; no global literal state (TC-9) |
| `fail()` throws `CheckFailure`; a function stops at its first fatal error | CA-07 | TC-7: errors are values; poison; checking continues; panics are internal errors at the task boundary |
| no verifier for checker output | CA-07; directions §2 | the TIR verifier, the rollback verifier, the interface validator (D1 §4.10) |
| captured-cell conversion on HIR, imported by the emitter | CA-04 | TC-1: the checker states capture facts; representation is D2's |
| unused-local warnings re-walk the finished HIR | checker audit | local flags during checking (section 8.3) |
| the winning candidate is re-checked outside the transaction | checker audit, Candidate Transactions | kept, but only for the instantiation choice, with arguments inferred once outside trials |
| speculation snapshots large state; exponential trait search (F-626) | perf F1, F-626 | no state copies; fuel with memo-hit charging (TC-8) |
| internal names (`__std_...`, `mut:List[char]`) and std errors at the user's line 1 | wishlist group 3 | stable-path rendering; std spans stay in std files (section 10.5) |

## 16. Open Questions For The Owner

**Answered (owner and orchestrator, 2026-10-07).** Used above, not asked
again:

| Question | Answer | Where |
| --- | --- | --- |
| literal width per class or per statement | per connected class; the spec rewords `types.literal.local.join`; the oracle stays for differential tests | section 3.6 |
| a row pattern against an unsolved private row | `cannot-infer-type` with a fix-it writing the callee's `$` clause (orchestrator) | section 5.6 |
| type cycles through top-level bindings | `recursive-function-needs-result-type` on the first binding | section 9.2 |
| typed holes | `_` only in the first release | section 10.4 |
| a no-emit mode | accepted for the playground and `hd fix` (orchestrator) | section 1.5 |
| polymorphic recursion | `instantiation-too-deep`; the row solve uses the same code | section 5.5 |
| identity of enums, functions and boxes | none (S1c): `is` on any enum, optional or result, on a function value or on another value type is `identity-requires-references` | section 2.2 |
| trait-solver questions 1, 3, 4 | dependency-closure impls; depth counted as listed; one error per body and leaf goal | section 1.6 |

**Open.**

1. **Signatures of hidden private items a template names (review finding 6).** A
   template may call private helpers of its trait's module (owner,
   2026-10-07), so a dependent needs their signatures. If a helper may
   omit its result or row, the interface depends on a body, and a helper
   body edit changes what a dependent's check reads. **Recommendation:**
   a private item that a template body names follows the public
   signature rules: a written result type, else `missing-result-type`;
   no `$` clause means the empty row; a template may not name a
   top-level binding. Add one rule beside
   [`module.package.annotated`](../../spec/lang/10-modules.md#r-module.package.annotated).
   It uses existing codes and keeps "interfaces come from syntax alone"
   (section 9.1). The alternative, checking helper bodies before the
   interface is usable, would put bodies on the interface's critical path
   and break the one-module recheck after a private edit.

### 16.1 Readings Of The Spec To Confirm

These follow from the spec as this design reads it. No answer is needed
unless the owner disagrees.

1. **Literal joins in functions with omitted results.**
   [`types.literal.local.form.closure-return.statements`](../../spec/lang/04-type-system.md#r-types.literal.local.form.closure-return.statements)
   names closures. This design applies it to every body with an omitted
   result, private functions and local `fn`s included, because they are
   the same LCT site (section 3.6). Without it, `return 0` before a final
   `i64` value would default to `usize` and fail in a private function
   but not in a closure.
2. **GADT arms and outer inference variables.** Removed with GADTs
   (section 6.1).
3. **`is` on results.** Settled by S1c: every enum, optionals and
   results included, is a value type, so `is` on one is
   `identity-requires-references` (section 2.2).

### 16.2 Inconsistencies Found

1. **The `block_on` ban is still transitive in the spec.**
   [`req.drive.block-on.transitive`](../../spec/lang/11-requirements-and-suspension.md#r-req.drive.block-on.indirect),
   `req.drive.block-on.unprovable`,
   [`flow.defer.block-on`](../../spec/lang/06-control-flow.md#r-flow.defer.block-on.direct)
   ("direct or transitive") and
   [`annot.fact.no-block-on`](../../spec/lang/14-annotations.md#r-annot.fact.block-on.direct)
   with its `unprovable` rule all predate answer 13: direct-only, with a
   run-time panic for an indirect call. The spec pass should rewrite
   them.
2. **`println` in the ban.** Answer 13 and D1 §4.13.5 ban a direct
   `println` as well as `block_on`, but no spec rule names `println`.
   Section 5.8 follows the answer.
3. **Redundancy warnings.** This design's brief asks for redundancy
   warnings, but the spec makes an unreachable arm an error
   (`unreachable-match-arm`). The design follows the spec.
4. **Checks over TIR.** D1 §4.13.3 runs mutability checks "on TIR
   places" and definite initialization and `let-else` as "a forward
   dataflow pass over the body's TIR", and §4.13.10 computes the init
   summary from TIR. Here all of them run during checking, on the
   checker's own tables (section 8). Only the checker's way keeps
   checking independent of emission (TC-3), and it reports with the
   syntax still at hand.
5. **Coercion kinds.** Resolved: crates/hd_tir/DESIGN.md's catalog now has
   kinds for declared variance and supertrait widening (section 4.2). The
   GADT `Refine` kind is removed with GADTs.
6. **Ambiguity codes.** D1 §4.13.2 says an unsolved variable is "an
   ambiguity error". The spec separates `cannot-infer-type` (no solution)
   from `ambiguous-type` (several).

Two earlier items are resolved: docs/compiler/data-structures.md §3.4 now has `Mut`
instead of `Readonly`, and one `Infer` form with the literal kind in the
inference table.

## 17. Changes Needed In COMPILER_DESIGN.md

COMPILER_DESIGN.md is now split into the files of the
[README](../../docs/compiler/ARCHITECTURE.md). These changes are for the backend lane to make in
its files. This document edits none of them. Items 15 to 21 of the
Codex review are in
[crates/hd_check/DESIGN.md (trait solver) §16.4](#164-changes-needed-in-type-checkingmd-and-the-other-design-files).

1. **docs/compiler/data-structures.md §3.4 and §3.9.2:** the projection type is
   `Assoc { assoc: DefId, tref }` (crates/hd_check/DESIGN.md (trait solver) change 15). A `Row`
   item whose pending part names a module-scoped `RowVar`, with a
   substitution, may live outside the body-local pool until
   `ModuleFinish` ends (section 5.5 step 6).
2. **docs/compiler/data-structures.md §3.9.5:** two more writes after the fact, both by
   M3: the row sweep rewrites `HAS_ROWVAR` types in the `ty` column and in
   type words of records; and each pending call's provider word is
   patched to point at an appended `(key, provider)` range (section 5.5).
3. **crates/hd_tir/DESIGN.md §4.13.1:** add rule TC-4 (each body checked
   exactly once). Add that M1 checks the module's top-level statements
   first, because unannotated top-level bindings get their types there.
4. **crates/hd_tir/DESIGN.md §4.13.2:** replace "ambiguity error" with the two
   codes. Add the permission join (section 3.4), the family rule, the
   no-retry literal plan and the cross-statement joins (section 3.6), and
   the instantiation choice with its prefilter, expected-type filter and
   trial memo (section 2.5).
5. **crates/hd_tir/DESIGN.md §4.13.3 and §4.13.10:** move mutability, definite
   initialization, `let-else` and init summaries from "over TIR" to
   "during checking", with a pointer to section 8.
6. **crates/hd_tir/DESIGN.md §4.13.4:** replace the pending row and the
   `Pending(row variable)` provider with section 5.5's `PendingRow` (with
   its substitution), `PendingCall` record, per-variable worklist solve
   and row sweep. Keep the M2 limit of section 5.6.
7. **crates/hd_tir/DESIGN.md §4.13.6:** removed with GADTs: drop GADT
   refinement, arm environments and the scoped pop (section 6.1).
8. **crates/hd_tir/DESIGN.md §4.13.11, catalog:** replace `Default` with a
   default call: the default body's `DefId`, the callee's type arguments
   and the earlier argument values, emitted per call after the explicit
   arguments (section 1.7; review finding 4). Add `Coerce` kinds for
   declared variance and supertrait widening; no `Refine` kind, which
   was removed with GADTs. Facts and metadata need first-read runtime
   bodies; shared enum data needs a requirement-free module-initializer
   body. That design is the backend lane's (crates/hd_mono/DESIGN.md §12.3).
9. **crates/hd_tir/DESIGN.md §4.13.11, builder:** `konst` must accept a
   literal whose width is still an inference variable; the width comes
   from the `Solution` at `finish` (review I7, section 1.5).
10. **crates/hd_tir/DESIGN.md §4.13.11, `Closure`:** a capture's mode
    (`Copy`, `Move`, `Shared`) depends on statements after the closure;
    `finish` fills the capture-mode column from the checker's `Solution`
    (already in docs/compiler/data-structures.md §3.9.5).
11. **crates/hd_tir/DESIGN.md §4.15:** add the fuel cost table of section
    11.1 and the out-of-fuel policy of section 11.2.
12. **docs/compiler/testing-the-compiler.md §8.1:** add the emit and no-emit dimension
    and the `fuel_used` comparison to the determinism matrix.
13. **`f1c55020:future-work/compiler/open-questions.md` §10.1:** add inconsistencies 1, 2 and 3 of
    section 16.2.

# New Compiler: The Trait Solver

Status: design record; may lag the code. The code and the spec win.

Frontend lane, 2026-10-07. Revised the same
day with the owner's answers to section 16.1 and the Codex review of
8bb6860d.
Revised again for the Codex re-review through 54f249b7
(`f1c55020:future-work/compiler/codex-rereview-response-frontend.md`),
the removal of GADTs and the `dyn` decisions.

This document details the trait solver of the new compiler. It implements
the interface that [crates/hd_check/DESIGN.md (type checking) §1.6](#16-the-trait-solver-interface)
fixes, and it refines these sections of the D1 and D2 design:

- [§4.12 Traits, Impls And Coherence](../hd_resolve/DESIGN.md#412-traits-impls-and-coherence):
  owner-module impl tables, solving and coherence;
- [§4.10 Folder Interface Construction](../hd_resolve/DESIGN.md#410-folder-interface-construction):
  what the interface records about traits and impls;
- [§4.13.11 The Typed IR](../hd_tir/DESIGN.md#41311-the-typed-ir-tir):
  how a choice is recorded;
- [§13.2 Collection](../hd_mono/DESIGN.md#132-collection) and
  [§13.5 Dictionaries](../hd_mono/DESIGN.md#135-dictionaries-trait-values-and-gadt-evidence):
  how codegen consumes the choice.

Where this design needs a change in another file, section 16.4 lists it.
It does not edit those files.

## How To Read This

Conventions, as in [crates/hd_check/DESIGN.md (type checking)](DESIGN.md):

- **Mine** marks an idea of this document: not shipped practice, and not
  in the research or in D1 and D2.
- **Rule TS-n** marks a design rule that a reviewer can check against the
  code. Section 15 maps prototype failures to these rules.
- Rust code sketches shape and ownership. It is not final code.
- "First release" is the first shipped compiler. "Later" is after it.
- The spec decides semantics. Where this design reads the spec in a way
  that needs confirming, section 16 says so.

**The short version.** hd's trait rules are small on purpose. Overlap is
decided from heads alone
([`trait.overlap.heads-only`](../../spec/lang/09-traits.md#r-trait.overlap.heads-only)),
there are no blanket impls
([`trait.target.no-blanket`](../../spec/lang/09-traits.md#r-trait.target.no-blanket)),
no specialization and no negative impls, and an impl may live only in one
of a few modules. Three consequences carry the whole design:

1. **At most one impl head matches a goal whose types are known.** So the
   solver commits to that impl and never backtracks (rule TS-2). The proof
   is an AND tree, not an AND-OR search.
2. **No search fixpoint.** The only coinduction the spec asks for is a
   derived impl assuming itself while its members are checked. Its head is
   already in the impl table, and uses never re-check members, so the
   search never meets that cycle. Every cycle it does meet is an overflow
   (rule TS-7).

Section 16.5 answers the Codex review of the earlier design files, finding
by finding.
3. **Codegen never searches.** At a concrete instance, the one matching
   head is the answer (rule TS-6).

What remains is real work: associated types and their normalization,
inference variables, the instantiation choice of generic traits, budgets
that do not depend on the schedule, and messages that name the root cause.

## Prior Art In One Table

Each row was checked against its source; the design keeps the column on the
right.

| System | What it does | What hd takes |
| --- | --- | --- |
| rustc's next solver ([dev guide: caching](https://rustc-dev-guide.rust-lang.org/solve/caching.html), [coinduction](https://rustc-dev-guide.rust-lang.org/solve/coinduction.html)) | canonical goals as cache keys; a search graph with cycle heads; provisional results and fixpoint iteration for coinductive cycles (auto traits, `Sized`, well-formedness); inductive cycles answer overflow; global cache entries store the depth they reached, and overflow is cached per remaining depth | canonical goals; depth stored with each entry (as an intrinsic height, section 7.3); inductive cycles answer overflow. Not taken: the provisional cache and fixpoints, which hd's rules make unnecessary |
| chalk | a separate logic-programming solver (SLG tabling) shared by rust-analyzer; it drifted from rustc and was abandoned for the next solver ([Rust blog, 2026-08-21](https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/)) | one solver, used by the checker, the impl checks and codegen alike; no general logic engine |
| Swift's Requirement Machine ([forums](https://forums.swift.org/t/the-requirement-machine-a-new-generics-implementation-based-on-term-rewriting/55601)) | generic signatures as a rewrite system, completed by Knuth-Bendix, after the old builder went exponential | not needed: hd has no same-type requirements between parameters except associated-type bindings, which are oriented (projection to type). Elaboration is a linear walk (section 4.2) |
| Lean 4 tabled resolution ([paper](https://arxiv.org/pdf/2001.04301)) | tabling removes the exponential cost of diamond hierarchies and handles cycles | the lesson that memoization must remove diamonds; hd gets it from the memo alone, since there is no OR search |
| Haskell (GHC) | an instance is selected by its head alone; the context is checked after the commit, and there is no backtracking | the commit rule TS-2. hd's head-only overlap makes it sound without any overlapping-instance pragmas |
| Scala 3 givens | search with priorities and a divergence check | nothing: hd has no priorities and no implicit search |
| Go | interface satisfaction by method sets; constraint satisfaction is memoized ("T satisfies B", Go #66699) | memoize "type satisfies bound" globally |
| MoonBit ([methods and traits](https://docs.moonbitlang.com/en/stable/language/methods.html)) | explicit `impl Trait for Type`; an impl lives in the package of the trait or of the type; the stable docs show no trait parameters and no associated types; monomorphization after linking. Its checker is known to be fast | the fast path of section 3.3: a goal on a non-generic trait with a known self type is one table probe. hd pays for more only where its rules are richer than MoonBit's: generic traits, associated types and impls owned through a trait argument |

## 1. Scope

### 1.1 Who Owns What

The solver lives in `hd_types`; other crates call its interface rather
than owning a second solver
(reconciliation, item 12 (`git show f1c55020:future-work/compiler/reconciliation.md`)).

| Component | Owns | Does not own |
| --- | --- | --- |
| Resolution (`hd_resolve`) | impl heads, bound plans and derived heads in the interface; every header check that needs no solver: orphan, module ownership, targets, unconstrained parameters, sealed traits, per-member `dyn` availability, template placement | goals |
| **Trait solver (`hd_types::solve`, this document)** | the four goals; parameter-environment elaboration; normalization of projections; canonical goals and the memo; impl selection for codegen; `FailInfo` | inference variables (it reads them, never writes them), diagnostics text, spans |
| Impl checks (`HeaderCheck(F)`, one task per folder, before bodies need it) | each impl's supertrait and binding checks, newtype bases, delegation targets: goals under the impl's environment, asked through the solver ([crates/hd_resolve/DESIGN.md §4.10.1](../hd_resolve/DESIGN.md#4101-header-validation-stages)) | the answers |
| Derive instances (body tasks in M2) | derive member obligations, checked with the instance body | the answers |
| Type checker (`hd_check`) | when to ask, applying learned bindings, obligations, the instantiation choice by trial, every diagnostic | impl search |
| Coherence (`Coherence(trait)` task) | overlap across the program graph, per trait, from heads | goals |
| Codegen collection (D2) | instances and vtables | anything but a head match at concrete types (rule TS-6) |

**Rule TS-1. The solver is a pure function of its memo key.** An answer
depends only on the canonical goal, the parameter environment, the local
impls visible at the asking point, the trait list (for `Methods`),
the impl universe of the asking context (for `Instantiations`, `Methods`,
and any goal with an open argument, section 3.2), and the run's frozen interfaces. Section 7.1 makes each of these part of
the key. The solver holds no mutable state except memo tables. It never
reports a diagnostic, never sees a span, and never writes an inference
variable.

### 1.2 The Four Goals

The interface is the one in
[crates/hd_check/DESIGN.md (type checking) §1.6](#16-the-trait-solver-interface),
with a `mut` flag and a `Normalized` answer added (section 16.4,
changes 1 and 4).

| Goal | Asked for | hd example | Typical answer |
| --- | --- | --- | --- |
| `Implements` | each bound of an instantiated callee; `==`, `<`, `?` conversions; trait-value conversions; impl checks | `fn show[T < Display](v: T)` called with a `User` asks `User: Display` | `Holds { Impl(impl Display for User) }` |
| `Project` | a projection meets another type in unification; an operator's `Out`; `T::Item` in a signature | `price + price` with `impl Add for Money { type Out = Money }` asks `<Money as Add[Money]>::Out` | `Normalized { ty: Money, evidence }` |
| `Instantiations` | parameters named only in bounds; the instantiation choice of a method or operator; `X::from(v)` | `AppError::from(code)` asks `Instantiations { AppError, From }` | `Many([From[i32], From[string]])` in content order |
| `Methods` | `value.name(...)`, `Type::name(...)` | `w.show()` on a `Wrapper` with an inherent `show` and an available `Show` | `Many([Inherent(show), Trait(Show::show)])`; the checker takes the inherent one |

The answer shapes:

| Answer | Meaning | Checker action |
| --- | --- | --- |
| `Holds { evidence, learned }` | proven; `learned` binds inference variables that the proof forced (section 3.8) | apply `learned` on its own trail; record `evidence` in TIR |
| `Normalized { ty, evidence, learned }` | `Project` only: the projection's normal form, over the caller's variables | unify `ty` with the other side |
| `Many(candidates)` | one or more candidates, in content order (`Instantiations`, `Methods` only); an `Instantiations` candidate is a scheme with residual bounds (section 6.5) | the instantiation choice or method rules |
| `Fails(FailInfo)` | no proof exists, whatever the open variables become | report, once per root cause (section 10) |
| `Stalled { on }` | the answer depends on these variables | keep an obligation watching them |
| `Overflow` | the proof needs a bound deeper than 64, or a cycle | `trait-resolution-depth` |
| `OutOfFuel` | the body's fuel ran out inside the solver | `item-too-complex` |

### 1.3 Entry Points

```rust
pub trait Solver: Sync {
    /// The checker's entry: one goal in one body.
    fn solve(&self, cx: &mut SolveCx<'_>, goal: Goal, fuel: &mut Fuel) -> Answer;

    /// Builds the elaborated environment of an item from its declared bounds
    /// (section 4.2). Called once per item signature by the checker or an impl check.
    fn elaborate(&self, bounds: &[DeclaredBound], out: &mut ParamEnvBuilder) -> EnvKey;

    /// Codegen's entry: the impl whose head matches a concrete trait reference.
    /// A head match, then the plan's `Bind` steps to fix every impl argument;
    /// no proof, no depth limit, no fuel (rule TS-6, section 8.3).
    fn select(&self, tref: ConcreteTraitRef) -> Selection;

    /// Codegen's entry for associated types at an instance.
    fn normalize_concrete(&self, base: Ty, trait_: DefId, args: TyList, name: Symbol) -> Ty;
}
```

`elaborate`, `select` and `normalize_concrete` are additions to the §1.6
sketch. They need no new goal kind. Not in scope: implied bounds
([Declared Bounds Are Not Implied](../../spec/lang/09-traits.md#declared-bounds-are-not-implied)),
structural conformance, and any goal at run time.

## 2. Goal Representation

### 2.1 Trait References

A trait reference is a trait, a self type and the trait's arguments. Its
associated-type bindings travel beside it.

```rust
pub struct TraitRef { pub trait_: DefId, pub self_ty: Ty, pub args: TyList }

pub enum Goal {
    Implements { tref: TraitRef, bindings: AssocList, mut_: bool },  // mut_: a `T < mut Tr` bound
    Project { assoc: DefId, tref: TraitRef },  // assoc: the associated type's own item
    Instantiations { self_ty: Ty, trait_: DefId, mut_: bool },
    Methods { receiver: Ty, name: Symbol },
}

/// A projection type in the pool (replaces D1's `Assoc { base, trait_, name }`).
/// `tref` is the declaring trait's instantiated reference, so `<S as Add[i32]>::Out`
/// and `<S as Add[i64]>::Out` are different types.
Assoc { assoc: DefId, tref: TraitRef }
```

- **Projections name the declaring item.** `I::Item` under
  `I < NamedSupplier` names `Supplier`'s `Item` item and the instantiated
  `Supplier` reference that elaboration found
  ([`trait.binding.name-reach`](../../spec/lang/09-traits.md#r-trait.binding.name-reach)).
  The name alone would lose the supertrait path and the trait arguments.
- **An `AssocList` binding** is keyed by that item's `DefId`, not by name.

- `self_ty` keeps its outer `Mut` only for the `mut_` check. Impl matching
  strips it, since one impl serves both views
  ([`trait.target.both-views`](../../spec/lang/09-traits.md#r-trait.target.both-views)).
- An inner `mut` stays: `Store[mut User]` and `Store[User]` are different
  trait references
  ([`trait.target.inner-mut`](../../spec/lang/09-traits.md#r-trait.target.inner-mut)).
- Types reach the solver with aliases expanded. Newtypes stay nominal.
- Trait argument defaults are filled before the goal is formed, so
  `impl Add for Money` and the goal `Money: Add[Money]` agree
  ([`expr.op.trait.rhs-self`](../../spec/lang/05-expressions.md#r-expr.op.trait.rhs-self)).
  Header lowering fills every written trait reference once, so a
  projection written under a bound takes the bound's filled arguments:
  `T::Out` under `T < Add` is `<T as Add[T]>::Out`. `Self::X` inside a
  trait keeps no filled arguments; each use fills its own.

### 2.2 Canonical Goals

A goal is canonicalized before any memo lookup:

1. **Resolve shallowly.** Each inference variable is replaced by its
   binding, recursively, through the checker's `InferRead`.
2. **Normalize concrete projections.** A projection whose base is known is
   replaced by its normal form (section 4.3). Projections on parameters
   stay as rigid types.
3. **Number the variables.** The remaining inference variables are
   replaced by canonical placeholders `Canon(0)`, `Canon(1)`, ... in order
   of first occurrence in a fixed pre-order walk: self type first, then
   trait arguments left to right, then bindings in name order. Each
   placeholder keeps its variable's kind: general, integer literal or float
   literal.
4. **Pick the key's scope.** See the table below. (An earlier step that
   applied GADT arm equalities is removed with GADTs.)

```rust
pub struct CanonGoal {
    pub kind: GoalKind,     // u8
    pub mut_: bool,
    pub n_vars: u8,          // canonical placeholders; more than 255 is a Stalled answer
    pub trait_: DefId,       // or NONE for Methods
    pub self_ty: Ty,         // global pool when the scope is Global or Env
    pub args: TyList,
    pub extra: u32,          // AssocList for Implements, assoc DefId for Project, Symbol for Methods
}
pub struct CanonVars { pub map: SmallVec<[InferVar; 4]>, pub kinds: SmallVec<[VarKind; 4]> }
```

`Canon(i)` is a global pool tag, so a canonical goal holds no body-local
type. Two bodies asking `List[?3]: Eq` and `List[?9]: Eq` produce the same
key. `CanonVars` maps placeholders back to the asking body's variables
when the answer has learned bindings or stalls.

| Scope | When | Memo | Key (section 7.1) |
| --- | --- | --- | --- |
| `Global` | no placeholder, no parameter, no local type | the run's global memo, shared by every body and by codegen | the canonical goal |
| `Env` (mine) | parameters, but no placeholder, no local type | the run's global memo | the canonical goal and the item's `EnvKey` |
| `Body` | any placeholder or local type | the body's memo | the canonical goal and the visible local impls |

An `Instantiations` goal adds the context's `ImplUniverseId` in every
scope (section 3.2), as does a `Methods` goal. A `Methods` goal is never
memoized: it carries the trait list from the method index instead of an
availability key, and no availability key exists yet.

**Why parameters may go global.** With the `EnvKey` in the key, a goal
that mentions `T` is still a pure function of its key, and the members of
a generic impl, which share the impl's bounds, share their goals.
crates/hd_check/DESIGN.md (type checking) §1.6 keeps such goals per body (change 2).

**Why the body memo has its own key arena.** A trial truncates the
body-local pool on rollback
([§3.9.5](../../docs/compiler/data-structures.md#395-building-scratch-buffer-checkpoints-truncation)),
so a key stored there would dangle. Canonical keys hold only global types
and placeholders, in an arena no rollback truncates, and an entry stays
valid after a rollback because it names no particular variable.

### 2.3 The Parameter Environment

```rust
pub struct ParamEnv {            // SoA, built by `elaborate`, frozen per item
    pub key: EnvKey,             // interned: equal environments get equal keys
    clause_self: Box<[Ty]>,      // a Param
    clause_trait: Box<[DefId]>,
    clause_args: Box<[TyList]>,
    clause_bindings: Box<[AssocList]>,
    clause_mut: BitBox,          // the bound was written `mut`
    clause_origin: Box<[u16]>,   // declared bound index, or the supertrait path's first step
    by_self: Box<[(Ty, u32, u32)]>,  // sorted index: self type -> clause range
}
```

A clause's index is its position in the elaborated list. TIR's
`Bound { param, index }` choice uses it (section 8.1). The order is
deterministic: declared bounds in source order, each followed by its
supertraits, depth first, in declared order, each trait reference once.

## 3. Impl Selection

### 3.1 Where Candidates Come From

For a goal `S: Tr[A1..An]`, candidates come from these sources, tried in
this order:

| Order | Source | When it applies |
| --- | --- | --- |
| 1 | the parameter environment (section 4.2) | `S` is a parameter |
| 2 | a trait value as self (section 9.3) | `S` is a trait value type of `Tr` or of a subtrait |
| 3 | compiler-supplied impls (section 3.9) | `Tr` is sealed and compiler-supplied: `Any`, `AnyVal`, `AnyRef`, `Inspectable`, `Tuple`, `Suspend`, `Structure` (std writes `Num`, `Integer` and `Float`) |
| 4 | impl tables of the owner folders (section 3.2) | written impls, derived impls, delegations, numeric families, tuple templates |
| 5 | the body's local impls | the goal names a local trait or a local type |

**Rule TS-2. Commit on the head; never backtrack among impls (mine, after
GHC).** Overlap is decided from heads, and no two heads of one trait
unify. So when the goal's types are known, at most one impl head matches.
The solver commits to it. If one of its bounds then fails, the goal fails:
no other impl could have answered it. Sources 1 to 3 come first, and the
first source that holds answers. A parameter can match only
parameter-headed impls (numeric families), which are std's and agree with
the environment.

Order between sources matters only for the evidence recorded, never for
whether a goal holds. A goal `T: Add[T]` under `T < Num` holds through the
environment (the supertrait binding of `Num`) and through std's
`impl[N < Num] Add for N`. The environment wins, so the evidence is
`Bound`. Codegen re-selects at the concrete type anyway (rule TS-6).

### 3.2 Owner Folders

An impl `impl Tr[Args] for Target` lives in a folder that declares `Tr`,
the outer constructor of `Target`, or the outer constructor of one of
`Args`
([Implementation Modules](../../spec/lang/09-traits.md#implementation-modules)).
Tables are per folder, not per module: each folder's interface holds one
`ImplTable`, and lookup reads the tables of the owner folders
([§4.12.1](../hd_resolve/DESIGN.md#4121-owner-module-impl-tables-mine)).

```rust
fn owner_folders(g: &TraitRef) -> Folders {
    // the folders declaring g.trait_, the head constructor of the self
    // type, and the head constructor of each known trait argument
}
```

**Unowned rows.** std's impls for built-in targets (primitives, `List`,
`Map`, tuples, `Fn`, `Option`, `Result`) live in the synthetic
`STD_BUILTIN_TABLE`, which is an "unowned" table: it also holds the impls
the ownership check rejected as `nonlocal-impl` or `orphan-impl`.
Rejected impls are already one error each; they stay in the table so the
rest of the pipeline sees a stable row, but coherence skips them and the
overlap check never reports them twice.

**The gap: impls owned through a trait argument.** When an argument is
still an inference variable, its owner folder is unknown. `Money:
Add[?0]` may be answered by `impl Add[Cents] for Money` in the folder that
declares `Cents`
([`trait.own.argument`](../../spec/lang/09-traits.md#r-trait.own.argument)).
`Instantiations` and the trait part of `Methods` are the same case with
every argument open. The `2 + len(Args)` bound covers only goals whose
arguments are known. The Codex review found the same gap (blocker 3).

The fix is a **candidate directory**, after the review's proposal:

1. Resolution flags each impl that its folder owns only through a trait
   argument. The interface lists those heads in an `arg_impls` section,
   sorted by `(trait, target head key)`. There is no section hash:
   nothing persists the directory, so nothing hashes it.
2. Once per impl universe, the driver merges the `arg_impls` sections of
   that universe's folders into a **directory**: per trait, rows of
   `(target head key, folder, impl row)`, in content order. Merging takes
   no barrier, and crates/hd_sched/DESIGN.md §6.1's "universes add no edge" holds:
   every folder in a context's closure is already a dependency of its
   task through the `FolderIface` chain.
3. A goal with an open argument reads the trait's directory rows for its
   target head key. Lookup itself takes no memo: `candidates` is a pure
   function of the tables, the directory and the goal.
4. **Cache key (incremental soundness).** An argument-owned impl's trait
   and target are nameable outside its folder, so its head is in its
   folder's `api_hash` and therefore in the deep hash
   ([crates/hd_resolve/DESIGN.md §4.10](../hd_resolve/DESIGN.md#410-folder-interface-construction)).
   A folder's `check` key already holds the deep hash of every folder in
   its closure, so adding `impl Pick[Product] for Receiver` in a third
   folder rechecks every folder whose closure holds it. No separate
   `arg_impls` hash is needed (Codex re-review N-A1; the backend lane
   removes `argc` from crates/hd_cache/DESIGN.md).

**The impl universe (Codex re-review N1).** The directory makes an
answer depend on which folders the asking context sees. Two folders may
both ask `Instantiations { Receiver, Pick }` while only one's universe
holds `impl Pick[Product] for Receiver`. The goal has no placeholder, so
without more it would be one global memo entry, and whichever folder
asked first would decide the other's answer. So:

- Every solving context carries an **`ImplUniverseId`** in `SolveCx`: the
  interned, sorted list of the folders in its closure that have
  `arg_impls` or unowned rows. A folder with neither joins no universe.
  The driver computes it once per context. For a folder's bodies and its
  test overlay, that is after M1 builds the closure bit set.
- The contexts are a folder's bodies, the folder's test overlay (its
  closure includes test-only dependencies), a folder's header checks
  (the folder's closure universe, with no own table), a program build
  (every folder, section 8.3), and a derive instance (its folder's).
  Contexts with equal lists share one id, so most folders of a package
  share one.
- **The id is part of the key of every goal with an open argument**, in
  every scope: `Instantiations` and `Methods` always, and an
  `Implements` or `Project` goal whose argument is open (`Infer`,
  `Canon` or `Poison`). The checker phrases instantiation and
  method-trait questions with fresh variables, so they land here.
- An `Implements` or `Project` goal in `Global` or `Env` scope with all
  arguments known never reads the directory: its owner folders are
  fixed, and the folder that declares a known type lies in the closure
  of every folder that can name that type. Its bound-plan steps are
  `Implements` and `Project` goals with known arguments too — except a
  `Bind` step whose bound leaves trait arguments implicit
  (`n < arity`): that step reads the directory after all.
- **No `HeaderCheck` goal kind exists.** Header checks are not solver
  goals and carry no universe in any key. They run a separate
  `TableSolver` from `hd_check::header`, under the item's environment
  with the folder's closure universe and no own table (section 3.7).
- **Debug check.** Each frame keeps a "read the directory" bit and a
  "read unowned rows" bit (section 7.1). A frame whose key has no
  universe and sets the directory bit is an internal error.

The memo-invariance test asks the two-folder pair in both orders
and on several threads, with equal trait availability and different
universes.

Why the closure, not the whole graph: a library's check result must not
depend on which downstream packages a program adds. A downstream package
may write `impl Pick[Product] for Receiver` for a library's `Receiver`,
and the library, checked alone or inside that program, must see the same
instantiations.

### 3.3 The Head Index

```rust
pub struct ImplTable {                     // per folder, SoA, frozen with its interface
    // D1's columns
    pub trait_: Box<[DefId]>, pub def: Box<[DefId]>, pub head_key: Box<[HeadKey]>,
    // added by this design
    pub arg_key: Box<[[HeadKey; 2]]>,      // head keys of the first two trait arguments; ANY when generic
    pub n_params: Box<[u8]>,
    pub head: Box<[HeadRef]>,              // target and trait arguments, as interface type records
    pub plan: Box<[PlanRange]>,            // the bound plan (section 3.6)
    pub assoc: Box<[AssocRange]>,          // associated-type bindings, by associated item DefId
    pub origin: Box<[ImplOrigin]>,         // Written | Derived { template } | Delegated { field } | Error | NumericFamily | TupleTemplate
    pub rank: Box<[u64]>,                  // content rank: (module path rank, item index in source order); the sort key
    pub by_trait: HashMap<DefId, (u32, u32)>,  // an index into the sorted rows
}
pub enum HeadKey { Ctor(DefId), Prim(Prim), Tuple(u16), TupleAny, Fn, SuspendFn, Param, Any }
```

Rows are sorted by `(trait rank, head key, rank)`. The head index is a
per-trait permutation over that order: it sorts each trait's row range
once, and a probe walks the bucket in row order. An impl that appears in
two tables is one head (rule TS-2): lookup dedups rows by impl `DefId`
before matching. A probe for `S: Tr[A..]` in one folder is:

1. `by_trait[Tr]` gives the trait's rows.
2. Binary search for `head_key(S)`; add the `Param` rows (numeric
   families) and, for a tuple, the `TupleAny` rows (tuple templates).
3. Match the survivors' heads (section 3.4).

The `arg_key` fast reject is written but not read: resolution fills the
first two argument keys per row, but nothing skips on them — rustc's
`DeepRejectCtxt` cut to two keys is not done.

**Fast path (mine, after MoonBit).** A non-generic trait (`Display`, `Eq`,
`Hash`, `Debug`) with a known self constructor usually has one row per
head key in all owner folders together. It may have more: `Tr for
Box[i32]` and `Tr for Box[string]` share the head key `Box`. So the probe
is one binary search per owner folder, and then a match of every row in
the bucket (Codex re-review N-T6). The global memo makes the second probe
free. In
practice most goals of a body take this path.

### 3.4 Matching A Head

Matching is one-way unification of the impl head against the goal. Impl
parameters are pattern variables. Goal types are rigid, except canonical
placeholders, which the solver may not bind.

```rust
enum MatchResult { Yes(Subst), No, Maybe(SmallVec<[CanonVar; 2]>) }
```

| Goal position holds | Head position holds | Result |
| --- | --- | --- |
| a type | an impl parameter | bind it; a second occurrence must be equal |
| a constructor | the same constructor | match the arguments pairwise |
| a constructor | another constructor | `No` |
| a placeholder | a constructor or a bound parameter | `Maybe(placeholder)` |
| a parameter of the asking item | a constructor | `No` |
| a row | a row parameter | bind it; a row parameter matches every row |
| a row | a concrete row | equal key sets, or `No` |
| a rigid projection | anything but an impl parameter | `No`: a projection on a parameter is opaque |

- **Rows.** A row argument in a head is a row parameter or a concrete row
  ([`trait.target.row-argument`](../../spec/lang/09-traits.md#r-trait.target.row-argument)),
  so matching never needs row unification with a tail.
- **Tuples.** A tuple head matches a tuple of the same arity and the same
  rest shape. `TupleAny` matches every tuple.
- **Several `Maybe`s.** The goal stalls on the union of their
  placeholders, in canonical order.
- **Numeric families match by trait list.** A `Param`-keyed head matches
  a primitive only when the impl bound's family trait lists that type
  (`family_excludes`): `impl[N < Num] Add for N` does not match `string`.
  A parameter matches through its environment instead (section 3.9).
- **Placeholders never learn, and carry no literal kind.** A placeholder
  against a constructor or a bound parameter is `Maybe` even when exactly
  one head matches: no unique head ever binds a placeholder (rule TS-3).
  The `Maybe` carries no `VarKind` split and there is no `Canon` arm, so
  literal-kind-specific matching is not done.
- **Cost.** Linear in the head's size. Heads are small, so matching is a
  few dozen steps at most.

### 3.5 Committing

After the probe:

| Matches | Answer |
| --- | --- |
| no `Yes`, no `Maybe` | `Fails`: no impl |
| exactly one `Yes`, no `Maybe` | commit: solve its bound plan |
| any `Maybe` | `Stalled` on the placeholders |
| two `Yes` | only possible with an overlap error elsewhere: take the first in content order and continue. Coherence reports the overlap (section 5.4); the solver reports nothing |

- **A failing bound fails with its own failure.** Under a committed head
  there is no second route, so a bound that fails answers with that
  bound's `FailInfo`, with this impl and step prepended to its `chain`
  (section 10.1).
- **Exact exhaustion (M1 finding 4).** When every candidate head is exact
  and none matches, the answer is `Fails(FailInfo { reason: NoImpl,
  ... })`; it does not stall and is not an internal error.

### 3.6 Bounds As Subgoals

An impl's bounds become subgoals after its head matched. Resolution stores
each impl's **bound plan**, an ordered list of steps computed once at
interface time (mine):

```rust
pub enum PlanStep {
    Bound { param: u8, trait_: DefId, args: TyListTemplate, mut_: bool },
    Bind  { param: u8, trait_: DefId, args: TyListTemplate, name: Symbol, target: u8 }, // solve target from a projection
}
```

- **Order.** A parameter that only a binding constrains, as `T` in
  `impl[T < Display, I < Store[Item = T]] Summary for Feed[I]`
  ([`trait.overlap.constrained-binding`](../../spec/lang/09-traits.md#r-trait.overlap.constrained-binding)),
  is solved by its `Bind` step before any step that reads it. Resolution
  rejects an impl with no such order (`unconstrained-impl-parameter`), so
  a plan always exists. It is a topological sort of the binding graph,
  with source order breaking ties.
- **Execution.** Steps run in plan order, each at depth `d + 1` where `d`
  is the goal's depth
  ([`trait.bound.depth`](../../spec/lang/09-traits.md#r-trait.bound.depth)).
  The first step that fails decides the answer, so a plan's answer does
  not depend on anything but its order.
- **A `Bind` step can read the directory.** Most steps are `Implements`
  goals with known arguments, but a `Bind` whose bound leaves trait
  arguments implicit (`n < arity`) reads the candidate directory like an
  open-argument goal (section 3.2).
- **Self types shrink.** A step's self type is an impl parameter, and every
  parameter is bound to a part of the goal or fixed by a binding. So a
  subgoal's self type is a subterm of the goal's types or a projection's
  normal form. Only trait arguments and projections can make subgoals
  larger. This is why the depth limit, the type size limit and fuel
  together suffice (section 13).

### 3.7 Supertraits

Supertraits act in two directions.

| Direction | What happens | Where |
| --- | --- | --- |
| using a bound | `T < Child` also proves `T: Parent`, with supertrait bindings substituted (`Summable < Add[Out = Self]` gives `T: Add[T, Out = T]`) | elaboration (section 4.2), once per item |
| proving a bound | `X: Child` holds through `impl Child for X`; `X: Parent` is not re-proved | nowhere at use sites |

The second row is sound because the impl check proves every supertrait
under the impl's own bounds
([`trait.super.impl-bounds`](../../spec/lang/09-traits.md#r-trait.super.impl-bounds)).
That check runs in the folder's header stage, from headers and
impl tables only, so a dependency's impls are checked even when its bodies
are not (review T7). It lives in `hd_check::header`: each item asks its
supertrait `Implements` goals through `TableSolver` under the item's
environment, with the folder's closure universe and no own table, and
resolution keeps only the overlap check. The findings are cached in one
`graph` entry per folder keyed by `hdr_key` (#95). Not done: supertrait
bindings are not asked as `Project` (the solver has no `Project` path;
the checker normalizes); there is no fuel diagnostic per item (§4.10.1).

### 3.8 What A Goal May Teach The Checker

**Rule TS-3. `Implements` learns only through associated-type bindings.**
`I: Supplier[Item = ?0]` holds with `learned = [?0 := string]` when `I`'s
impl binds `Item = string`
([`trait.binding.inference`](../../spec/lang/09-traits.md#r-trait.binding.inference)).
A unique head match never teaches anything. `Money: Add[?0]` with one
impl `Add[Money] for Money` stalls; it does not learn `?0 := Money`.
Choosing among instantiations is the job of `Instantiations`, which the
checker asks exactly where the spec says: parameters named only in
bounds
([Inference Through A Bound](../../spec/lang/04-type-system.md#inference-through-a-bound))
and the instantiation choice
([Instantiations Of One Generic Trait](../../spec/lang/09-traits.md#instantiations-of-one-generic-trait)).
So adding an impl changes inference only where the spec's own rules let
it. This is a reading of the spec; section 16 notes it.

### 3.9 Compiler-Supplied Impls

Each sealed trait has one function that answers it from the type's form.
These functions are table rows in a `match`, not special cases scattered
through matching.

| Trait | Holds for | Subgoals | Evidence |
| --- | --- | --- | --- |
| `Any` | every value type, `never`, optionals | none | `Builtin(Any)` |
| `AnyVal` | `bool`, `char`, integers, floats, `string`, tuples, enums (optionals included), function types; a newtype over one of them ([`types.sealed.anyval-values`](../../spec/lang/04-type-system.md#r-types.sealed.anyval-values)) | the newtype's base | `Builtin(AnyVal)` |
| `AnyRef` | data, `List`, `Map`, trait values, `Any`, suspensions, handles; a newtype over one | the newtype's base | `Builtin(AnyRef)` |
| `Inspectable` | [inspectable types](../../spec/lang/09-traits.md#inspectable-types): primitives, `string`, module-level declarations with inspectable arguments, `List`, `Map`, tuples, `Inspectable` trait values; trait values and `Any` as arguments only | one per type argument, each one level deeper | `Builtin(Inspectable)`; codegen derives the `TypeId` from the type |
| `Tuple` | every tuple type | none | `Builtin(Tuple)` |
| `Num`, `Integer`, `Float` | the fixed primitive lists of [Numeric Traits](../../spec/lang/09-traits.md#numeric-traits): std writes one impl per type (`lib/std/num.hd`, with the bodies of `zero`, `one`, `from_i64`), so the impl tables answer them | the impl's | `Impl` |
| `Suspend[T]` | compiler frames and the `std.task` types | none | `Builtin(Suspend)`; no row while std declares no such trait |
| `Structure` | the target of a template instance, through that instance's environment; and every concrete tuple type (#126), since the compiler generates a tuple's `Structure` while it instantiates a tuple template (`annot.tuple.structure`) | none | `Bound` in a template; `Builtin(Structure)` at a tuple, lowered per tuple type (crates/hd_mono/DESIGN.md §13.6) |

For a parameter or a rigid projection, every row above but `Any`
answers through the environment only
([`types.sealed.type-parameter`](../../spec/lang/04-type-system.md#r-types.sealed.type-parameter));
every type parameter is `Any`. `never` implements neither `AnyVal` nor
`AnyRef`, and is not inspectable. A row on an inference variable stalls,
except `Any`, which holds.

**Where the rows live (#121).** `hd_types::sealed::row` is the table: one
`match` over the sealed trait and the type's form. `Search::implements`
asks it as source 3, after the environment and the trait-value rows of
section 9.3 (source 2), so body checks, header checks and codegen's
`select` all get its answer; the checker has no answerer of its own
(`builtin_holds` is gone, and `is` asks `AnyRef`). A row's subgoals are
`Implements` goals on the same trait one level deeper (a newtype's base;
an inspectable type's arguments, where a trait value counts without a
subgoal), asked and memoized like a committed impl's bound plan. The
rows read no impl table, so their memo entries have no probes and need
no universe; they read declarations through `SolveCx::decls` (a
`Declarations`: the sealed traits' ids, a type's declaration kind, a
trait's supertraits), which are the same in every context of a run.
Codegen's `select` returns `Selection::Builtin` for them.

Not done: no `Suspend` row (std declares no such trait); block-local
declarations, which `trait.inspectable.not.local` excludes, do not exist
in the compiler yet; `Inspectable`'s compiler-supplied `runtime_type`
body is a panicking vtable slot (crates/hd_mono/DESIGN.md §13.6).

**Written impls that act like built-ins.** Two kinds live in std's tables
as ordinary rows with special head keys:

- **Numeric families.** `impl[N < Num] Add for N` has head key `Param`.
  It matches a primitive that the family lists, or a parameter whose
  environment proves the family trait. Its bound `N < Num` is then an
  ordinary subgoal. Overlap expands it to one head per member type
  ([`trait.overlap.numeric-family`](../../spec/lang/09-traits.md#r-trait.overlap.numeric-family)).
- **Tuple templates.** `impl[T < Tuple] Eq for T by Structure` has head key
  `TupleAny`. Its bound plan is the walker obligation of
  [`annot.walker.obligation`](../../spec/lang/14-annotations.md#r-annot.walker.obligation):
  one subgoal per element, and for a rest element `List[T]...` either on
  `T` or on `List[T]`, by whether the walker implements `rest`. The
  interface does not carry template bodies yet
  ([`annot.limit.interfaces`](../../spec/lang/14-annotations.md#r-annot.limit.interfaces)),
  so the driver computes the plan, a list of `MemberStep`s, from the
  template body at first use: it checks the template's methods in their
  own module and reads the `member` and `rest` bounds of each walker,
  describer or source the body passes. The result is cached per template
  `DefId` for the run, and a template whose body has an error has no plan.
  The solver reads it through `ImplView::plans` after the row's bound
  plan; a context without one (the plan's own check) asks no member goals.
  The evidence is `Impl { def: template, args: [tuple] }`: the
  spec calls each tuple instance "an ordinary implementation"
  ([`annot.template.tuple.instance`](../../spec/lang/14-annotations.md#r-annot.template.tuple.instance)).
  A tuple whose elements fail the obligation does not implement the trait;
  that is a `Fails`, not an error, until a use needs it.

Function types are not special: `Fn` and `SuspendFn` are std constructors
with head keys of their own, and their impls are ordinary rows.

### 3.10 Derives, Delegation And `@error`

The solver never reads a template body. It sees only heads.

| Source | Head in the target module's table | Bounds | Body checked by |
| --- | --- | --- | --- |
| `@derive(X)` on `data Box[T]` | `impl[T < X] X for Box[T]`, origin `Derived { template }` | `T < X` per walked parameter ([Derived Bounds](../../spec/lang/14-annotations.md#derived-bounds)) | the derive-instance body task ([§4.13.9](../hd_tir/DESIGN.md#4139-derive-instances-templates-facts-and-test-overlays)) |
| a derivation block with a header | the header as written | the header's bounds | same |
| `@derive(X)` on a newtype | `impl X for Mile` | none at the head; the base type's impl is an impl-check goal | the impl check |
| `impl Tr for C by E` | as written | as written | generated forwarders, checked as written ([Generated Methods](../../spec/lang/09-traits.md#generated-methods)) |
| `@error`, `@from`, `@source` | `Display`, `Error`, `From[P]` heads | the generated bounds of [Generated Error Bounds](../../spec/lang/14-annotations.md#generated-error-bounds) | the generated bodies |

- **Delegated associated types.** Each associated type of `impl Tr for C
  by E` is bound to the projection `<E's type as Tr>::Name`, with `C`'s
  arguments substituted. Normalizing it is an ordinary `Project`.
- **Hidden helpers.** A template body may call private helpers of the
  trait's module, which the interface exports as hidden items
  (owner, 2026-10-07). Calls to them resolve through the template's
  resolution table. Each helper's signature is written in full, so a
  dependent reads it from the interface, never from a body
  ([crates/hd_check/DESIGN.md (type checking) §9.1](#91-who-can-omit), an open
  owner question). Trait goals inside the instance run under the
  instance's environment like any other body; the solver needs nothing
  more.
- **Member obligations and coinduction.** The derive-instance task checks
  each walked member against the walker's bound. Recursion is coinductive
  ([`annot.bound.recursive`](../../spec/lang/14-annotations.md#r-annot.bound.recursive)):
  while checking `Tree[T]`'s members, `Tree[T]: Eq` is assumed. No
  mechanism is needed for that (mine). The derived head
  `impl[T < Eq] Eq for Tree[T]` is already in the target module's table,
  and the solver never re-checks an impl's members at a use. So the member
  `children: List[Tree[T]]` asks `List[Tree[T]]: Eq`, which commits to
  `List`'s impl, then to the derived head, then proves `T: Eq` from the
  environment. The search meets no cycle and holds no assumption, so no
  memo entry can depend on one (rule TS-7). This is how rustc checks an
  impl's well-formedness with the impl itself in scope.
- **`missing-derived-bound`.** A use whose failing subgoal is a bound that
  a derived head added reports `missing-derived-bound`, not
  `unsatisfied-trait-bound`
  ([`trait.derive.bound-unmet`](../../spec/lang/09-traits.md#r-trait.derive.bound-unmet)).
  `FailInfo` carries the impl's origin, so the checker can tell (section
  10.2).

### 3.11 The Synthetic Impl Inventory

Every impl that no one wrote as `impl ... :` has a stable descriptor, a
place where its head lives, a dependency that invalidates it, an overlap
rule and an evidence form. The Codex review (A5) asked for this table.

| Family | Descriptor (stable) | Head lives in | Invalidated by | Overlap | Evidence |
| --- | --- | --- | --- | --- | --- |
| `@derive(X)`, derivation block | target path, trait path, `Derived` | the target folder's `heads` section | the target folder's interface; the template body via the trait folder's deep hash | ordinary heads; beside a written impl, `overlapping-impl` at folder time or in `Coherence` | `Impl` |
| tuple template of any trait | trait path, `TupleTemplate` | the trait folder's `heads` section, head key `TupleAny` | the trait folder's interface; for the member steps, the template module's source (std's through the toolchain key) | `TupleAny` joins every tuple head: a written tuple impl of the trait is `overlapping-impl` ([`annot.template.tuple.overlap`](../../spec/lang/14-annotations.md#r-annot.template.tuple.overlap)) | `Impl` of the template at the tuple type |
| numeric family | impl path, `NumericFamily` | std's `heads` section, head key `Param` | std's interface | expanded per member type | `Impl` |
| `@error`, `@from`, `@source` | target path, trait path, member index | the target folder's `heads` section | the target folder's interface | ordinary heads; two `@from` of one type at folder time | `Impl` |
| delegation `by E` | impl path, `Delegated` | written head | the folder's interface | ordinary head | `Impl` |
| `Any`, `AnyVal`, `AnyRef`, `Tuple`, `Num`, `Integer`, `Float`, `Suspend` | the trait | the compiler (section 3.9) | the toolchain key | sealed: no user impl can exist | `Builtin` |
| `Inspectable`, `TypeId::of` | the trait | the compiler | the toolchain key; a type's inspectability depends only on its declaration's place (module level or block) | sealed | `Builtin` |
| `Structure` | the template instance | the instance's environment | the instance | never an impl | `Bound` |
| function types `Fn`, `SuspendFn` | not synthetic: std constructors | ordinary heads | ordinary | ordinary | `Impl` |

A user library's tuple template is no different from std's: the module
that uses `==` or `.encode()` on a tuple names the trait, so the trait
folder is in its key. The member steps come from the template's body, so
the source hash of every user tuple-template module in `closure(F)` joins
the key of each module's `check` entry and of `HeaderCheck(F)`, whether or
not the text opts in (`template_sources`). A walker's bounds are in the
interfaces already in those keys. An edit to a template module redoes the
header check of each folder that reaches it, and the cached result of an
unrelated edit stays. `Collect` needs nothing more: its key holds the
modules' TIR, and a check that passed under a changed plan picks the same
impls, since the plan only decides provability.

## 4. Associated Types And Normalization

### 4.1 Where Projections Come From

A projection `<S as Tr[A]>::Name` is the pool's `Assoc { assoc, tref }`
type (section 2.1): the associated item and the instantiated reference of
the trait that declares it. Projections come from:

- signatures: `T::Item`, `Self::Item`, `I::Item` in a result type;
- instantiated method results: `Self::Out` of `add` becomes
  `<Money as Add[Money]>::Out`;
- impl bindings that name another projection, as a delegated impl's do;
- bounds with bindings: `I < Supplier[Item = T]` relates `I::Item` and `T`.

### 4.2 Elaboration Of The Environment

`elaborate` turns an item's declared bounds into its `ParamEnv` once:

1. Each declared bound `P < Tr[A, N = U]` is a clause, with its binding.
2. For each clause, walk `Tr`'s supertraits depth first, in declared
   order, substituting `P` for `Self` and the clause's arguments for the
   trait's parameters. A supertrait binding (`Num < Add[Out = Self]`)
   becomes a binding on the new clause (`P: Add[P, Out = P]`). A binding
   written on the bound for a supertrait's associated type
   ([`trait.binding.name-reach.meaning`](../../spec/lang/09-traits.md#r-trait.binding.name-reach.meaning))
   lands on that supertrait's clause.
3. **One clause per trait reference, with merged bindings (Codex
   re-review N-T3).** Clauses are keyed by their `TraitRef`; bindings sit
   beside it, so a reference met again may bring bindings the first
   visit lacked. A diamond may reach `Supplier` once unbound and once
   with `Item = i32`. On a second visit the walk merges binding by
   binding: a binding not yet present is added, an equal one is a no-op,
   and a different one is a conflict. The walk does not descend again:
   the reference's supertraits were walked on the first visit, and a
   supertrait binding that names this reference's projection is stored
   as that projection, which normalization resolves through the merged
   binding later (section 4.3). The result does not depend on the order
   in which paths are met. The supertrait graph is acyclic
   (`supertrait-cycle` is a header error), so the walk ends.
4. **A conflict is a header error.** Two different bound types for one
   projection make the bounds unsatisfiable, and inside the body the two
   types would be equal. Resolution makes the same walk before any body
   and reports `duplicate-associated-binding` on the parameter's bound
   list (needs owner: the spec's
   [`trait.binding.once`](../../spec/lang/09-traits.md#r-trait.binding.once)
   covers only written bindings).
5. Intern the clause list; its content hash is the `EnvKey`.

Cost: linear in the size of the supertrait closure of the declared bounds.
`Num`'s closure is about fifteen clauses, `Integer`'s about twenty-five.
Elaboration runs once per item, not per goal.

Header checks that need the environment, such as
`ambiguous-associated-type` on `I::Item` with two bounds declaring `Item`,
are made by resolution with the same walk, before any body.

### 4.3 Normalization: Lazy, At Three Points

The solver normalizes a projection only when someone needs its value, as
rustc's lazy normalization does. The checker asks `Project`:

1. **When unification meets a projection** against a type that is not the
   same projection. `Assoc` against `Assoc` with equal parts unifies
   structurally first.
2. **Before shallow resolution for method lookup or field access** on a
   value whose type is a projection.
3. **Before canonicalizing a goal** whose types contain a projection with
   a known base (section 2.2 step 3).

`Project` computes, for `<S as Tr[A]>::Name`:

| `S` is | Answer |
| --- | --- |
| an inference variable, or holds one at a position the head needs | `Stalled` |
| a parameter with an environment binding for `Name` | the bound type: `I::Item` is `T` under `I < Supplier[Item = T]` |
| a parameter with a bound on `Tr` but no binding | the projection itself, as a rigid type. It equals only itself |
| a trait value `Tr[A, Name = U]` or a subtrait's value that binds it | `U` ([`trait.dyn.bound.projection`](../../spec/lang/09-traits.md#r-trait.dyn.bound.projection)) |
| known | prove `Implements { tref }` first, as its own memoized goal at the same depth. On `Holds` with `Impl` evidence, read that impl's binding for `Name`, substitute the impl's arguments, and normalize the result once more |
| known, but `Implements` fails | `Fails`, with the `Implements` goal's `FailInfo`: `Box[NoDisplay]::Item` is no type when the impl for `Box[T]` needs `T < Display` |
| known, but `Implements` stalls or overflows | the same answer |

**Normalization proves applicability (Codex re-review N-T1).** A head
match alone does not make an impl apply: its bounds must hold too. So a
source-level `Project` on a known base always rests on a proof of the
`Implements` goal. That goal is memoized, and most projections meet a
goal that a use has already asked, so the extra cost is one memo hit.
Rigid projections under a declared bound are unchanged. Codegen's
`normalize_concrete` is a separate mode: at an instance the checker has
already proven the goal, so it reads the binding after a head match and
keeps its answers apart from the proof memo (section 8.3).

- **One answer, no outer projection.** Each `Project` returns a type with
  no outer projection, by normalizing its result. A chain
  `<<T as A>::X as B>::Y` normalizes inside out. Each step is one goal
  level, so a chain counts against the depth limit and is memoized step by
  step.
- **Bindings in `Implements`.** `S: Tr[A, Name = U]` holds when `S: Tr[A]`
  holds and the normal form of `<S as Tr[A]>::Name` unifies with `U`. If
  `U` holds placeholders, the unification gives the learned bindings
  (rule TS-3). If they disagree, the answer is `Fails` with the binding
  as the reason (`unsatisfied-trait-bound`,
  [`trait.binding.mismatch`](../../spec/lang/09-traits.md#r-trait.binding.mismatch)).
- **Rigid projections and equality.** Under `T < Supplier`, `T::Item` and
  `string` never unify. Under `I < Supplier[Item = T]`, `I::Item` and `T`
  are the same type, because normalization replaces the projection before
  any comparison
  ([`trait.binding.interchangeable`](../../spec/lang/09-traits.md#r-trait.binding.interchangeable)).
- **The result.** `Project` answers `Normalized { ty, evidence, learned }`.
  `ty` is stored over placeholders in the memo and mapped back to the
  caller's variables. `evidence` is the impl, clause or trait value that
  gave the binding.
- **Projections over variables and the occurs check.** Unifying `?0` with
  `<?0 as Tr>::Item` must not bind `?0`. The checker asks `Project` first;
  if it stalls, it keeps a projection-equality obligation `?0 ==
  <?0 as Tr>::Item` instead of binding, and retries it when `?0` is bound.
  A rigid projection whose base holds `?0` counts as an occurrence of `?0`
  only after normalization fails.
- **Normalization cycles.** An impl binding that names its own projection,
  directly or through other impls, is a cycle on the stack: `Overflow`
  (section 6.3).
- **Aliases** are expanded when headers are lowered, so a projection never
  meets an alias.

### 4.4 Projections In Impl Heads

The spec does not say whether an impl head may contain a projection of an
impl parameter, as in `impl[I < Store] Summary for Feed[I::Item]`. Such a
head is not injective: two different `I` may give one `Feed` type, so
matching cannot solve `I`, and overlap cannot be decided from heads.
The owner decided (2026-10-07) to forbid it: resolution reports the head
as `unconstrained-impl-parameter`, since `I` appears only under a
projection. A parameter that appears only inside a projection is not
constrained. No new code.

### 4.5 Header Bounds In Bodies And Calls

The 26 `unsatisfied-trait-bound` failures behind #155 are gone (a fresh
per-case listing finds 5, all with other causes — see R28 note below),
so this records the working shape, not a fix. A declared bound enters
solving in exactly two places:

- **Body environment.** `elaborate` runs once per item signature and
  the resulting `ParamEnv` travels with the body (`EnvKey` in the
  solve context): every clause — the bound itself, its supertrait
  expansion, and its bindings — is available to goals the body asks,
  so a call on `T` with `T < Display` solves through the environment
  clause, never by searching impls.
- **Call sites.** The caller's arguments must satisfy the callee's
  bounds (`trait.bound.unsatisfied`), checked as obligations under
  the call; the callee body then solves from its own environment.
  `T::f(args)` resolves through the bounds for the same reason
  (`trait.assoc-call.parameter`).

A binding constrains a projection by orientation: `I::Item` under a
`Store[Item = T]` bound normalizes to `T` through the merged binding
(§4.2 step 3, §4.3), so projections never need a search of their own.
`Bind` plan steps (§3.6) solve an impl's open trait arguments from
its bindings the same way.

R28 note: the brief's 26 programs all pass today (fixed by the #155
/ #48 / #57 work); the 5 remaining `unsatisfied-trait-bound`
accepts are `FromIterator` for `mut List` (×4, missing impl, not
bounds) and `Result[void, FsError]` vs `Eq` (×1, void value). No
header-bound case remains to shrink.

## 5. Coherence

### 5.1 What Runs When

| Check | Needs | Runs at | Error |
| --- | --- | --- | --- |
| orphan rule (package level) | the impl head and package ownership | folder interface, step 5 | `orphan-impl` |
| module ownership | the head and the declaring modules | folder interface | `nonlocal-impl` |
| targets: bare parameter, trait value, outer `mut`, invalid inherent target | the head | folder interface | `bare-parameter-impl-target`, `trait-value-impl-target`, `mutable-impl-target`, `invalid-impl-target` |
| unconstrained parameters and the bound plan | the head and bounds | folder interface | `unconstrained-impl-parameter` |
| sealed traits | the trait | folder interface | `sealed-trait-implementation` |
| a second template; `@derive` beside a written impl or a block for the same trait | the module's heads | folder interface | `overlapping-impl` |
| law partners all derived or all written | the module's heads | folder interface | `mixed-derived-law` |
| supertraits and supertrait bindings of an impl | the solver, under the impl's environment | `HeaderCheck(F)` | `missing-supertrait-implementation` |
| overlap between impls | heads of one trait across the program graph | `Coherence(trait)` task | `overlapping-impl` |
| duplicate inherent members | inherent heads of one type, in its module | folder interface | `duplicate-inherent-member` |
| local impls | the body's local impls | the body | the same codes |

Only the impl checks need goals. Everything else is a function of heads,
so it runs before any body and caches with the interface.

### 5.2 The Overlap Check

One `Coherence(trait)` task runs per trait with more than one impl in the
program's graph, as D1 has it
([§4.12.3](../hd_resolve/DESIGN.md#4123-coherence)). D1's pairwise
unification within buckets is quadratic: the Codex review (T6) counts
50 million pair checks for 10,000 heads under one constructor. This design
replaces it with a near-linear algorithm (mine):

1. **Collect** the trait's heads from the `heads` sections of every folder
   in the graph, in content order. Derived, generated and delegated heads
   and tuple templates are among them (section 3.11).
2. **Expand** numeric-family heads to one head per member type.
3. **Phase one, ground heads** (no impl parameter), every one before any
   generic head: encode each head canonically and insert it into a hash
   set, where a second equal head is an overlap, and into a **ground
   trie** over the head's pre-order walk.
4. **Phase two, generic heads**, in content order: before inserting a
   head into the **generic trie** (a discrimination tree in which an impl
   parameter is a wildcard), query the generic trie for stored heads that
   may unify with it, and walk its wildcard positions through the ground
   trie. Every ground head is already in the ground trie, so a generic
   head meets every ground head and every earlier generic head, whichever
   came first in the source (Codex re-review N-T6). A row parameter
   matches every row; `Args < Tuple` matches every tuple; `TupleAny`
   matches every tuple head.
5. **Confirm** each candidate pair with full unification after renaming
   apart. The tries treat each parameter occurrence as its own wildcard,
   so a head with a repeated parameter, such as `Pair[T, T]`, yields
   candidates that only unification can reject. Report a pair once, on
   the later impl in content order (package, module path, item index).
   The message names the earlier impl and a **witness**: the unifier's
   solution applied to the head, such as "both apply to `Box[Plain]`".
6. **Budget.** The task counts trie nodes visited and stops after the
   first overlap reported per impl, so an all-overlapping bucket costs one
   report per impl, not one per pair.

Cost: linear in the total head size plus the candidate pairs the tries
return. For heads that differ at a constructor, which is the normal
case, the tries return no false candidates. Repeated parameters can add
candidates that unification rejects; the trie-node budget of step 6
bounds them, and the `pathological` suite measures them. This is not a
proof of linearity. 300 `From`
impls for one error type differ at the first trait argument, so the trie
separates them at its second level.

**Known M3 simplification (M3 gap 6).** `hd_resolve::Universe::overlaps`
currently sorts each trait's heads, then unifies every later head with
every earlier head. Its answer and content-order blame match this section,
but its work is quadratic. M4 replaces that loop with the ground and
generic tries above; the pairwise loop is not another supported strategy.

### 5.3 Why No Global Index

- **Selection needs no global view.** Ownership confines the impls a goal
  can use to at most `2 + len(Args)` modules, plus the candidate directory
  for open arguments (section 3.2), which is per trait and holds only the
  rare argument-owned impls.
- **Coherence needs a per-trait view, not a global one.** Each trait is
  one task keyed by its sorted head hashes, so an edit reruns only the
  traits whose heads changed. A **head hash** is `H(head, rank)`: the
  head's canonical encoding (its parameters, target and trait
  arguments) and its content rank (package, module path, item index).
  The rank decides which impl of a pair is reported, so it must be an
  input: swapping two overlapping impls changes both ranks, and the task
  reruns instead of reusing a report on the wrong impl (Codex re-review
  N-D1).
- **A global index would be a shared mutable structure** built as folders
  finish, and its state at a given moment would depend on the schedule.
  The owner tables are frozen with their folder, so a body can solve
  while unrelated folders are still being resolved.

### 5.4 What The Solver Assumes

Bodies are checked before or while coherence runs; they do not wait for
it. The solver assumes no overlap. If two heads match a goal, which only an
overlap error allows, the solver takes the first in content order and
reports nothing (section 3.5). The program fails on `overlapping-impl`, so
the choice never reaches a build.

`orphan-impl` names the concrete modules that may hold the impl (the
trait's, the type's, or a trait argument's); `nonlocal-impl` adds a move
fix-it when exactly one module of the package qualifies.

## 6. Search

### 6.1 Depth First, On An Explicit Stack

```rust
struct Frame {                    // 24 bytes
    goal: CanonGoalRef,           // u32 into the memo's key arena
    impl_row: u32,                // the committed impl, or NONE
    step: u16,                    // next bound-plan step
    depth: u8,                    // depth from the use (section 7.3)
    height: u8,                   // deepest relative level reached below
    heads: u32,                   // heads matched in the probe (section 7.4)
    subst: u32,                   // range in the scratch buffer: the impl's parameters
    seen: u32,                    // range in the scratch buffer: distinct children so far
}
```

**Rule TS-9. No native recursion in the solver.** Goals are frames on a
vector. A subgoal pushes a frame; an answer pops it and resumes the
parent. The deepest stack is bounded by the depth limit (section
7.3), so it never overflows the native stack, including in the browser.

### 6.2 The Search Graph

With rule TS-2, a goal has one way to be proven. The graph of goals is a
graph of AND nodes:

- a node is a canonical goal;
- its children are the steps of its committed impl's bound plan, or the
  inner goals of a compiler-supplied row, or one `Project`.

So a goal's answer is a function of its children's answers, combined in
plan order: the first `Fails`, `Overflow` or `Stalled` child decides,
otherwise `Holds`. There is no OR node except the choice among sources in
section 3.1, which is ordered.

### 6.3 Cycles

A goal that meets itself on the stack is a cycle.

- **Coinductive cycles.** The spec has one: a derived impl's member check
  ([`annot.bound.recursive`](../../spec/lang/14-annotations.md#r-annot.bound.recursive)).
  It never reaches the search: the derived head is in the table, and the
  solver never re-checks members at a use (section 3.10).
- **Inductive cycles.** Every other cycle has no finite proof. Its depth
  would exceed 64, so the spec's answer is `trait-resolution-depth`
  ([`trait.bound.depth.limit`](../../spec/lang/09-traits.md#r-trait.bound.depth.limit)).
  The solver answers `Overflow` at once, without going 64 levels deep.

**Rule TS-7. No provisional results, no fixpoint (mine).** A frame on the
stack is exploring its current plan step, and every earlier step of it has
completed with `Holds`. Those completed answers are context-free (section
7.1), so a fresh evaluation of any frame retraces the same path. Hence:

- **The SCC rule.** When goal `G` meets itself, the frames from `G` to the
  top of the stack form one strongly connected set: each reaches the
  others along the stack path. Every one of them answers `Overflow`,
  whichever was asked first.
- **Completion before publication.** A frame's entry is published only
  when the frame pops, with its final answer. Members of the set pop as
  `Overflow`. A frame that completed earlier with `Holds` did not reach
  `G`, so it is not in the set, and its entry stands.
- **No provisional answer ever leaves the stack.** So every entry, in a
  cycle or not, can be memoized at once.

This removes the part of rustc's search graph that took the most work:
provisional caches, cycle heads, fixpoint iteration, and the rule that
only cycle roots enter the global cache. It holds only because no node has
a second route (rule TS-2) and no cycle is coinductive.

The "bounded monotone fixpoint" of the prior-art lessons is therefore not
needed in the solver. The checker keeps the one fixpoint the spec defines,
the bound-inference loop
([`types.generic.infer.bound.fixed-point`](../../spec/lang/04-type-system.md#r-types.generic.infer.bound.fixed-point)),
which is bounded by the number of parameters named only in bounds
([crates/hd_check/DESIGN.md (type checking) §2.4](#24-calls-and-use-site-type-arguments)).

### 6.4 Stalling

A goal stalls when its answer depends on a placeholder:

- the self type is a placeholder: `?0: Display` stalls on `?0`;
- a head match reports `Maybe`: `Money: Add[?0]` with two `Add` impls
  stalls on `?0`;
- a child stalls: `List[?0]: Eq` commits to `impl[T < Eq] Eq for List[T]`,
  and its child `?0: Eq` stalls, so the goal stalls on `?0`.

The stalled answer lists the placeholders in canonical order, mapped back
to the body's variables. A stall is never memoized globally, since its
scope is `Body`. The body memo keeps it, because asking the same canonical
goal again gives the same stall.

**Literal placeholders.** An integer-literal placeholder stalls like any
other. A head that is not an integer type rules it out (`No` instead of
`Maybe`). So `?int: Display` stalls, and `?int: Pairing` with no integer
impl fails at once. When several candidates fit only while literals stay
open, the solver retries with the defaults (`i32`, `usize`, `f64`)
before stalling. Literal defaulting at the end of the statement then
wakes the obligation.

### 6.5 Ambiguity

`Many` belongs to `Instantiations` and `Methods` only.

**`Instantiations { S, Tr }`** collects every impl head of `Tr` that
matches `S` with all trait arguments open: the owner tables of `Tr` and
`S`, and the candidate directory (section 3.2). For a parameter, it
collects the environment clauses on `S` for `Tr`; for a trait value, the
one instantiation the value names.

**Candidates are schemes with residual bounds (Codex re-review N-T4).**
Matching `S` fixes only the impl parameters that occur in the target. A
parameter that occurs only in the trait arguments, as `U` in
`impl[U < Display] Pick[U] for Family`, stays open, and a bound on it
cannot be decided yet. So each candidate is a **scheme**:

```rust
pub struct Candidate {
    pub row: ImplRef,          // or the clause or trait value it came from
    pub n_fresh: u8,           // impl parameters the target did not fix
    pub impl_args: TyList,     // each impl parameter: the fixed type, or the parameter itself when fresh
    pub args: TyList,          // the trait arguments, over the fixed types and the fresh parameters
    pub residual: Vec<u16>,    // the plan steps the solver did not decide, in plan order
}
```

- The solver runs every plan step whose parameters the target fixed. A
  candidate whose step `Fails` is dropped. A step that overflows makes
  the whole answer `Overflow`, since depth exhaustion is never a
  failure.
- The steps that read a fresh parameter are the candidate's
  **residual obligations**. So is a fixed plan step that stalls on a
  variable of `S`. A residual `Bind` step is never an obligation: only
  `Bound` steps become goals.
- The solver does not run them.
- The checker instantiates a scheme inside each trial: a fresh variable
  per fresh parameter, unified with the call's argument types, then the
  residual steps as obligations under that trial
  ([crates/hd_check/DESIGN.md (type checking) §2.5](#25-methods-and-operators)).

The outcomes, which the checker handles in one way:

| Outcome | Answer |
| --- | --- |
| `S` holds a placeholder at a position the heads need | `Stalled` |
| no candidate survives | `Fails` |
| one or more survive | `Many`, in content order, even for one: the checker always instantiates the scheme and its residual obligations |
| fuel runs out | `OutOfFuel` |

A scheme is stored over the impl's parameters and global types, so it
is memoizable as before.

**`Methods { receiver, name }`** returns, in this order:

1. inherent candidates of the receiver's nominal type whose impl target
   matches it
   ([`trait.inherent.target-match`](../../spec/lang/09-traits.md#r-trait.inherent.target-match));
2. for a parameter, the methods named `name` of its elaborated clauses;
   for a trait value, the methods of its trait and supertraits;
3. otherwise, each available trait
   ([Trait Availability](../../spec/lang/09-traits.md#trait-availability))
   that declares `name` or reaches it through a supertrait, and that the
   receiver implements, by one `Instantiations` goal each.

A per-folder method index maps a name to the available traits that
declare it, so step 3 asks one goal per trait with that name, not one per
available trait. The `Methods` goal carries that trait list from the
index; it carries no `AvailKey`. The answer also lists, for diagnostics only, traits that
are not available but would match: the `use` fix-it.

**Checked once vs once per trial.** The checker infers each call
argument once before the trials unless its check needs the expected
type. Those go per trial, one check per surviving candidate: closures,
`.V` contextual variants (including through a call head), `if`, `match`
and block expressions, empty collections, and any list, map, tuple or
parenthesized argument holding one of those
([crates/hd_check/DESIGN.md (type checking) §2.4](#24-calls-and-use-site-type-arguments)).

### 6.6 Choosing Among Several Traits

A method name provided by more than one available trait resolves in
four passes, all driven by one `Methods { receiver, name }` goal:

1. **Gather.** The per-folder method index maps the name to the
   available traits that declare it (or reach it through a
   supertrait); the goal carries that trait list and asks one
   `Instantiations` goal per trait. Unavailable traits never enter
   the goal — they are listed in the answer for diagnostics only
   (the `use` fix-it).
2. **Narrow by shape.** Before any trial, candidates whose
   parameter head constructors cannot match an argument whose type
   is already known are dropped.
3. **Narrow by bounds and fit.** Each surviving scheme instantiates
   inside a checker trial (fresh variables, residual obligations as
   goals); a candidate applies when the arguments check against its
   method type with the instantiation substituted, plus the expected
   result type when the call has one
   (`trait.resolve.applies`, `trait.resolve.applies.expected`).
   Failing candidates drop.
4. **Decide.** Exactly one applicable candidate is selected
   (`trait.resolve.one-applies`); instantiations of one generic
   trait choose the same way, with literal-default resolution
   breaking width ties (`trait.resolve.literal-arg`). Two or more
   applicable candidates from different traits — or beside a
   promoted candidate — are `ambiguous-method`, whatever the
   signatures (`names.method-lookup.ambiguous`); a single candidate
   with the wrong signature is still selected and the call is
   checked against it. None applicable is `type-mismatch` (listing
   the instantiations), none at all `unknown-method`.
5. **Record for Emit.** The call keeps a callee record: `Item` for
   inherent and direct callees, `TraitMethod` with the solver's
   choice otherwise — `Impl` (direct call of the impl method),
   `Bound` (`select` at the instance's types), `TraitValue`
   (`CallDyn`, plus a witness for generic methods), `Builtin` (the
   generated body). Collection pushes per §13.2 steps 3–4.
6. **Memoize by availability.** Multi-trait answers depend on which
   traits are available, so `Methods` goals key by `AvailKey` (with
   `EnvKey`, `LocalVis` and the universe where applicable); without
   it one module's answer would leak into another's scope.

## 7. Memoization And Budgets

### 7.1 Memo Keys And Eligibility

An answer may be reused only where every input that decided it is equal.
The Codex review (blocker 8) listed the inputs that D1's key missed. Each
is now part of the key, or makes the goal ineligible for the global memo:

| Input that can change an answer | How the key covers it |
| --- | --- |
| the goal's types, with variables | the canonical goal; placeholders keep their kinds (section 2.2) |
| declared bounds and supertrait bindings | `EnvKey`, the interned elaborated environment; `EMPTY` when the goal names no parameter |
| local impls, visible from their declaration point ([`trait.impl.local.lookup`](../../spec/lang/09-traits.md#r-trait.impl.local.lookup)) | `LocalVis`: the interned sorted list of visible local impls, in the `Body` key; a goal that names no local type or local trait cannot match a local impl and keys with `EMPTY` |
| trait availability (`Methods` only) | not keyed: `Methods` goals are not memoized, so there is no availability key yet (section 6.5) |
| which argument-owned impls the asking context sees (`Instantiations`, `Methods`, open-argument `Implements`/`Project`) | the context's `ImplUniverseId` (section 3.2), in every scope |
| what the proof read | each entry records the `(trait, head key)` probes of its subtree; a global entry serves a folder only when that folder's own table has no row for any of them (the own-table rule) |
| unowned rows read | an answer whose proof read unowned rows is scoped: it is published under its key with the universe filled in, and the universe counts folders with unowned rows (section 3.2) |
| coinductive assumptions | none exist (section 3.10) |
| proof depth | the stored height, checked at each use (section 7.3) |
| remaining fuel | never stored: `OutOfFuel` makes the frame and every ancestor in the same `solve` call ineligible |
| the interfaces of the run | the global memo lives for one run: one frozen set of interfaces. A long-lived process (the playground, the REPL, a watch loop) starts a new memo per run, so no entry survives a source revision |

**Rule TS-4. Only completed, context-free answers are published
globally.** An entry goes to the global memo only when its scope is
`Global` or `Env`, it completed (it popped with a final answer, rule
TS-7), and it never met the depth cut or ran out of fuel (section 7.3).
Every other entry goes to the body memo or nowhere.

**Remapping.** Answers are stored over placeholders and global types.
`learned`, `Normalized.ty` and `Stalled.on` are mapped back through the
caller's `CanonVars`. No entry holds a body-local `Ty` or `InferVar`, so a
rollback cannot leave a dangling entry.

```rust
pub struct GlobalMemo {                         // one per run; shared by checking and codegen
    shards: [Mutex<RawTable<u32>>; 64],         // hash(scope key) -> entry index
    entries: PerThreadAppend<MemoEntry>,        // as the InternPool's columns
    keys: PerThreadAppend<u32>,                 // canonical goals, encoded
}
pub struct BodyMemo {                           // per body; owned by BodyCx, lent per call
    table: RawTable<u32>,
    entries: Vec<MemoEntry>,
    keys: Vec<u32>,                             // its own arena; never truncated by a rollback
    met: BitVec,                                // goals this body has been charged for (7.4)
}
#[repr(C)]
pub struct MemoEntry {                          // 12 bytes
    answer: CanonAnswerRef,                     // u32: evidence and learned bindings over placeholders
    children: u32,                              // range in the children arena of entry indices (not goals): the proof DAG (7.4)
    height: u8,                                 // intrinsic height (7.3); 255: a cycle
    kind: u8,                                   // Holds | Fails | Stalled | Overflow | AtLeast
    heads: u16,                                 // heads matched in the probe (7.4); saturating
}
```

- **First writer wins** in the global memo. Two threads that compute one
  key compute the same entry (rule TS-1), so either write is correct.
- **Hits take no lock (systems review, finding 9).** A memo hit used to
  lock a shard, and rule TS-5's walk did one locked lookup per node of
  the proof DAG. So:
  1. **Children are entry indices.** `MemoEntry.children` is a range of
     `u32` entry indices into the append-only `entries` arena, not
     canonical goals to look up again. A global entry's children are
     global entries, since a goal that names no parameter or local type
     asks only such goals. A body memo entry marks each child as body or
     global with the high bit. The TS-5 walk then follows indices and
     reads published, immutable entries with no lock and no hashing.
  2. **A per-worker read-through table** of 4,096 direct-mapped slots,
     `(hash, entry index)`, sits in front of the shards, as for the
     interners ([docs/compiler/data-structures.md §3.3](../../docs/compiler/data-structures.md#33-interners)).
     A published entry never changes, so a slot is never stale within a
     run. The table is cleared when a new memo starts (a new run).
  3. **The selection table** that codegen shares across programs
     ([crates/hd_mono/DESIGN.md §13.2](../hd_mono/DESIGN.md#132-collection)) gets the same
     per-worker table.

  | Operation (cold 10k-line check, review's estimates) | Count | Before | After |
  | --- | --- | --- | --- |
  | memo lookups | about 50,000 | a shard lock each: 20 to 150 ns | an estimated 80% per-worker hits at 5 to 10 ns; the rest locked |
  | TS-5 walk nodes | about 150,000 | a locked lookup each | an index load each: 2 to 5 ns |
  | total, with interning (docs/compiler/data-structures.md §3.3) | about 400,000 locked operations | 10 to 60 ms of CPU | about 2 to 10 ms of CPU |

  Slice 3 measures the hit cost at 1 and 8 threads; the 100 ns hit
  target (§13) is gated at 1 thread and reported at 8.
- **Not persisted** in the first release. A persisted memo would need its
  own key (the hashes of every impl table it read), and a warm check
  rechecks only edited modules, whose goals take microseconds.

### 7.2 Depth Is Charged From The Use

The spec counts depth from the use: a bound required directly by a use has
depth 1, and a bound of the impl that proves a depth-`n` bound has depth
`n + 1`. More than 64 is `trait-resolution-depth`, "whether or not a
deeper proof would succeed"
([`trait.bound.depth.limit`](../../spec/lang/09-traits.md#r-trait.bound.depth.limit)).

### 7.3 Heights And Lower Bounds

A memoized answer must not depend on the depth at which it was first
computed. rustc stores the depth an entry reached and caches overflow per
remaining depth. This design does the same in two entry kinds:

1. **Exploration is cut at the absolute limit.** A goal computed at depth
   `d` may explore `r = 65 - d` levels, itself included.
2. **A goal that completes without meeting the cut** stores its answer and
   its **height** `h`: the deepest level it used, counted from itself. The
   answer is context-free, because more depth would not change it. A use
   at depth `d'` gets `Overflow` when `d' + h - 1 > 64`, and the stored
   answer otherwise.
3. **A goal that meets the cut** stores `AtLeast(r + 1)`: "with `r` levels
   or fewer, this is `Overflow`". A later use with at most `r` levels left
   reuses it as `Overflow`. A use with more levels left computes the goal
   again, and may store a better entry. Such an entry is never global and
   never a `Fails`, so depth exhaustion is never cached as a semantic
   failure (review blocker 8).
4. **A cycle** stores `Overflow` with height 255: no depth suffices.

So whether a goal overflows depends only on the goal and on where it is
used, never on which body computed it first. The stack never holds more
than 64 goal frames plus their `Project` steps, which count as levels.

**What counts as a level** (owner, 2026-10-07): a bound-plan
step of an impl, an element obligation of a tuple template, one
`Inspectable` step through a type argument, and one `Project` step. An
environment clause, a trait-value source and a sealed membership with no
subgoal are leaves at no extra level.

### 7.4 Fuel

crates/hd_check/DESIGN.md (type checking) fixes the principle: fuel used is a pure function of the
body and its frozen inputs
([crates/hd_check/DESIGN.md (type checking) §11.1](#111-what-counts)).

**Proof nodes, charged once per body (Codex re-review N-T2).** Each memo
entry stores its **children**: the canonical goals its committed plan
asked, in plan order, with repeats among its own children removed (as in
`Pair[X, X]`, where both bound steps ask `X: Eq`). It also stores how
many heads its probe matched (rows that survived the fast reject). The
entries and their children form the goal's **proof DAG**. The children
are a function of the goal (rule TS-1), so the DAG is the same whether
an entry was computed now or found in a memo.

An earlier version charged a stored recursive cost, `1 + heads + sum of
the children's costs`. That sum counts a shared descendant once per path
to it, so a DAG with two goals per level, each needing the same two
goals at the next level, cost `2^n` for about `2n` goals. It is
replaced.

**Rule TS-5. Charge each proof node once per body (mine).** A body's
`met` set records the canonical goals it has paid for. When a body asks a
goal, the solver walks the goal's proof DAG depth first, in plan order.
Each node not yet in `met` costs `1 + heads matched` and joins `met`
before its children are walked. A node already in `met` costs nothing,
and the walk does not enter it; the asked goal itself costs 1 when it is
already met. A goal computed now is charged by the same walk, as its
frames pop. So a body pays once for each distinct goal in the union of
the proof DAGs it used, whatever the memo held. The sequence of goals a
body asks is fixed by its source and its frozen inputs, so the charge,
and the point where fuel runs out, are deterministic. A rollback refunds
nothing and does not clear the `met` set. A cycle ends the walk, since a
node joins `met` before its children.

Charging the full DAG on every hit would bill the 1,600 `Cents::from(N)`
lines of the `pathological` metric 1,600 times for one goal. Charging only
what was computed would make a warm memo cheaper than a cold one, so a
body could pass its budget on one run and fail on the next (lesson 3 of
Lessons For hd (`git show f1c55020:future-work/compiler/prior-art-issues.md`)).

### 7.5 Limits And Their Diagnostics

| Limit | Value | Counted by | Diagnostic |
| --- | --- | --- | --- |
| proof depth | 64, fixed by the spec | heights (7.3) | `trait-resolution-depth` at the use, showing the first three goals of the chain and the last |
| body fuel | 2,000,000 steps per body | proof nodes, once per body (7.4) | `item-too-complex`, by the checker |
| type size | 10,000 nodes | `mk` | `type-too-large` (proposed), if a subgoal's argument grows |
| placeholders per goal | 255 | canonicalization | none: the goal stalls, which ends as `cannot-infer-type` |
| candidates in one `Many` | none | | none needed: bounded by the impls that exist |

**F-626-style blowups are impossible**, for four reasons:

1. No checker state is copied. The solver reads inference variables and
   writes nothing (rule TS-1), and the checker's trials truncate
   ([crates/hd_check/DESIGN.md (type checking) §3.5](#35-the-trail-and-the-one-rollback-contract)).
2. One `Instantiations` goal per distinct receiver and trait is computed
   once per run and found in the memo after that.
3. No goal has more than one committed impl (rule TS-2), so no search
   explores alternatives.
4. Shared subgoals are computed once and charged once per body as proof
   nodes, so a diamond costs the size of its DAG.

### 7.6 Bounded Is Not Linear

Fuel and depth make every case end. They do not make it fast. The Codex
review (T6) asks for the algorithms that make the common cases
near-linear; these are they, each with the input it is linear in:

| Work | Algorithm | Linear in |
| --- | --- | --- |
| repeated goals | the memo, keyed canonically; repeats cost one lookup | distinct goals |
| shared subgoals | memoized children; each proof node charged once per body (rule TS-5) | distinct goals in the proof DAG |
| impl lookup | head key, then first two argument keys; the fast path for non-generic traits | candidates that share both keys, usually one |
| open trait arguments | the per-trait candidate directory, filtered by the closure bit set | argument-owned impls of that trait |
| overlap | ground heads hashed; generic heads in a discrimination tree (section 5.2) | total head size, plus reported overlaps |
| elaboration | once per item, one clause per trait reference, bindings merged | the supertrait closure |
| canonicalization | one walk of the goal, charged one step per node | goal size |
| normalization | one memoized `Project` per projection | distinct projections |
| instantiation choice | prefilter candidates by the head of each known argument type before any trial (change 7); one trial per surviving candidate | surviving candidates |
| nested trial chains | a per-call-site trial memo keyed by the canonical expected type and the canonical argument types (mine; change 8). It removes repeats only: when outer candidates give an inner site the same context, trials cost sites × candidates; when they give distinct contexts, the count can grow exponentially, and fuel ends it with `item-too-complex` (Codex re-review N-T7) | distinct trial contexts, plus tainted trials |
| obligations | each obligation is queued once per wake round, deduplicated by index; a retry is charged its goal's size | obligations × bindings of their variables |

The `pathological` suite checks the scaling claims, not only the limits:
each case runs at sizes `n` and `2n`, and the run reports the observed
exponent. Above 1.2 fails the case (section 14.2).

## 8. Evidence

### 8.1 What A `Holds` Answer Carries

```rust
pub enum Evidence {
    Impl { row: ImplRef, args: TyList },     // written, derived, delegated, generated, numeric family, tuple template
    Bound { param: ParamRef, index: u16 },   // clause index in the elaborated environment
    TraitValue { trait_: DefId },            // the self type is a trait value of this trait or a subtrait
    Builtin(BuiltinImpl),                    // Any, AnyVal, AnyRef, Inspectable, Tuple, Num, Integer, Float, Suspend
    Poison,
}
// crates/hd_check/DESIGN.md (type checking)'s `Coinductive` is not needed: a derive instance's member
// goals reach the derived head as an ordinary `Impl` (section 3.10).
```

Only the **top-level choice** is evidence. The nested proof (which impls
proved the bounds) is not stored in TIR, because codegen monomorphizes: at
each instance the nested bounds are concrete and are selected again by
head (rule TS-6). The solver keeps the nested proof only in the memo, for
`FailInfo` and for `hd debug solve`.

### 8.2 What TIR Records

| Use | TIR | Evidence recorded |
| --- | --- | --- |
| a trait method call with a known self type or a bound | `Call` with a `TraitMethod` callee | the choice: `Impl`, `Bound`, `TraitValue` or `Builtin` |
| a conversion to a trait value | `Coerce` of kind to-trait-value | the concrete type, the trait reference and the `Impl` choice |
| a call through a trait value | `CallDyn` | none: its method-level bounds are proven at the call and reach the erased body through the type witness (section 9.2) |

The `TraitMethod` choice in D2's catalog is "the impl's `DefId` or the
index of the bound in scope". This design needs two more choices,
`TraitValue` and `Builtin` (section 16.4, changes 4 and 16). A tuple template
instance is an `Impl` choice and needs none.

### 8.3 What Codegen Does With It

**Rule TS-6. Codegen selects through owner lookup (mine).** At an
instance, every type is concrete. A program build's impl universe is
every folder, and selection shares the run's global memo with checking.
`select` solves the concrete trait reference as an `Implements` goal and
takes the `Impl` evidence, or a sealed trait's `Builtin` evidence
(`Selection::Builtin`, section 3.9): the designed head-only `select` with its own
table is not built. It has no depth limit of its own and charges no
fuel. The returned `Selection` holds the impl row with the args the
proof found; `Selection.args` returning every impl argument verbatim
from the solver is not done.

**Selection, then reconstruction (Codex re-review N7).** A head match
fixes only the impl parameters that occur in the head. In
`impl[T < Display, I < Store[Item = T]] Summary for Feed[I]`, matching
`Feed[ConcreteStore]` fixes `I` but not `T`, and the selected method's
body needs `T`. So after the head match, `select` runs the plan's `Bind`
steps (section 3.6), in plan order, with `normalize_concrete`: each one
reads a binding at concrete types and fixes its target parameter. It
skips the `Bound` steps, which the checker proved. This is deterministic
normalization, not search: the plan order already exists, and every
parameter is fixed by the head or by a `Bind` step, or resolution has
rejected the impl as `unconstrained-impl-parameter`. The returned
`Selection` holds every impl argument.

`select` solves through the run's shared global memo. Mono also keeps
a per-build selection table of picks keyed by concrete trait reference
(crates/hd_mono/DESIGN.md §13.2); that table caches which impl was picked, while the
proofs behind the picks live in the shared memo.

- **No depth limit at instances.** The spec's 64 counts from a use in
  source. An instance of generic code may need a deep concrete proof that
  no source use asked for. The representation of generic code is free
  ([`trait.bound.representation`](../../spec/lang/09-traits.md#r-trait.bound.representation)),
  so this is no language error. Polymorphic recursion is stopped by the
  instantiation depth limit instead
  ([§13.4](../hd_mono/DESIGN.md#134-the-instantiation-depth-limit)).
- **A failed select is an internal error**, never a user diagnostic: the
  checker proved the goal generically. CI asserts it
  ([§12.7](../hd_mono/DESIGN.md#127-emission-time-checks)).
- **Default methods.** A `TraitMethod` call resolved to an impl that does
  not write the method instantiates the trait's default body, with `Self`
  set to the impl's target.
- **Vtables.** For each `(type, trait reference)` that a coercion or an
  evidence choice needs, codegen selects the impl of
  the trait and of each supertrait, and fills the vtable shape (section
  9.2).

## 9. Trait Values And Dynamic Safety

### 9.1 Which Traits Can Be Values

**Every trait can be a `dyn` type** (owner, 2026-10-07, in
[docs/compiler/goals.md](../../docs/compiler/goals.md#summary); applied in S1d as
[`trait.dyn.any-trait`](../../spec/lang/09-traits.md#r-trait.dyn.any-trait)). The
per-trait dynamic-safety gate of
[Dynamic Safety](../../spec/lang/09-traits.md#dynamic-safety) is
dropped, Swift 5.7 style. A trait value type is written `dyn Tr`
([crates/hd_syntax/DESIGN.md §4.4](../hd_syntax/DESIGN.md#44-parser-and-green-tree)). What cannot work dynamically is decided **per member**,
and the error is at the call, not at the type.

Resolution computes, per trait member at interface time, from headers:

| Member | On a `dyn` value |
| --- | --- |
| an associated function | unavailable |
| a method with `Self` outside the receiver | unavailable |
| a method with method-level type parameters, whatever their bounds | available, through the erased slot (section 9.2; [`trait.dyn.safe.method-type-param`](../../spec/lang/09-traits.md#r-trait.dyn.safe.method-type-param), S1c) |
| a method whose signature mentions an associated type | available when the `dyn` type binds that type; otherwise see below |

The trait record stores, per member, an "unavailable" flag with its
reason and a small set of the associated items its signature mentions.
The checker reads both at a call through a `dyn` value: a cheap test, no
solver goal.

**An unbound associated type (Codex re-review, authors' question 2).**
Decided (owner, 2026-10-07): the type itself stays an error, as
`trait.dyn.binding.complete` says; the per-member recommendation below
was not taken.
Today [`trait.dyn.binding.complete`](../../spec/lang/09-traits.md#r-trait.dyn.binding.complete)
requires a `dyn` type to bind every associated type. With the gate gone,
this design recommends, as an owner question, the per-member rule
instead of existentials:

- A `dyn Tr` that leaves `Item` unbound is a valid type. Every member
  whose signature mentions `Item` is unavailable on it, with the error at
  the call and a fix-it that adds `Item = ...` to the type. Members that
  do not mention it work.
- Such a type satisfies no bound on `Tr`, or on a trait that reaches
  `Item` (Swift's "an existential does not conform to its protocol").
  Generic code under `T < Tr` may name `T::Item`, and
  `<dyn Tr as Tr>::Item` has no normal form, so `Project` on it `Fails`.
- No call produces an existential value, and no member is opened.

The alternative keeps `trait.dyn.binding.complete` unchanged, as Rust
does: the type itself is an error. It needs no language change, but it
is the one remaining per-type gate. Either way the solver's part is the
same table; only where the error lands differs.

Requirement keys
([`req.key.any-trait`](../../spec/lang/11-requirements-and-suspension.md#r-req.key.any-trait))
follow the same per-member rule.

### 9.2 Vtable Shapes

A **vtable shape** is computed once per trait at interface time and stored
in the trait record (mine):

```rust
pub struct VtableShape {
    slots: Box<[SlotRef]>,          // the trait's own methods, in declaration order
    supers: Box<[DefId]>,           // direct supertraits, in declared order; each a pointer to its vtable
    type_id: bool,                  // the trait is Inspectable or extends it
}
```

- **Supertraits by pointer, not by copying their slots.** A diamond such as
  `Error < Display & Inspectable` shares one vtable per
  supertrait. Widening to a supertrait value reads one pointer
  ([`trait.dyn.widen`](../../spec/lang/09-traits.md#r-trait.dyn.widen)).
- **Method-level type parameters.** A method available on `dyn` may have
  a parameter `T < Display`, called with any type argument
  ([`types.trait.safe.method-type-arg`](../../spec/lang/04-type-system.md#r-types.trait.safe.method-type-arg)).
  The checker proves each such bound at the call's type argument, as an
  ordinary `Implements` goal, and records nothing more: `CallDyn` has no
  evidence operands. The slot's body is the impl method compiled once,
  erased, and it receives one type witness per method type parameter
  (owner, 2026-10-07, in [docs/compiler/goals.md](../../docs/compiler/goals.md#summary)). The witness
  carries the layout operations and, as outlined thunks, the bound
  methods at the caller's concrete type, each selected by head there
  (rule TS-6). That ABI is codegen's
  ([crates/hd_mono/DESIGN.md §13.5](../hd_mono/DESIGN.md#135-dictionaries-trait-values-and-gadt-evidence),
  Codex re-review N5). Statically dispatched calls of the same method
  stay monomorphized.
- **Row parameters** pass their providers as one bundle and need no slot.

### 9.3 Trait Values As Self Types

A trait value type satisfies a bound on its own trait and on each
supertrait ([`trait.dyn.bound`](../../spec/lang/09-traits.md#r-trait.dyn.bound)).
These are the rows of source 2 in section 3.1:

| Goal | Holds when |
| --- | --- |
| `Tr[A]: Tr[B]` | `A` equals `B` ([`trait.dyn.bound.instantiation`](../../spec/lang/09-traits.md#r-trait.dyn.bound.instantiation)) |
| `Child[A]: Parent[B]` | the elaborated supertraits of `Child[A]` contain `Parent[B]` |
| with a binding `Name = U` | the value binds `Name` to `U` ([`trait.dyn.bound.binding`](../../spec/lang/09-traits.md#r-trait.dyn.bound.binding)) |
| with `mut_` set | the value is `mut Tr` ([`trait.dyn.bound.mut`](../../spec/lang/09-traits.md#r-trait.dyn.bound.mut)) |
| any other trait | never: the value satisfies no other bound and is no impl target ([`trait.dyn.bound.no-impl`](../../spec/lang/09-traits.md#r-trait.dyn.bound.no-impl)) |

The first row is the prototype's F-618: a trait value of a generic trait
did not satisfy the instantiated bound.

### 9.4 Conversion To A Trait Value

`S` converts to `Tr[A, N = U]` when `Implements { S: Tr[A], bindings:
[N = U], mut_ }` holds, with `mut_` set for `mut Tr`
([`trait.dyn.binding.convert`](../../spec/lang/09-traits.md#r-trait.dyn.binding.convert),
[`trait.dyn.mut`](../../spec/lang/09-traits.md#r-trait.dyn.mut)). `Any`
always converts. `Inspectable` converts through its compiler-supplied row,
which checks inspectability. A trait extending `Inspectable` still needs
its own impl
([`trait.erase.child-impl`](../../spec/lang/09-traits.md#r-trait.erase.child-impl)).
A failure is `type-mismatch`, not `unsatisfied-trait-bound`: the checker
picks the code from the site, and `FailInfo` gives the reason.

## 10. Diagnostics

The solver writes no text. It returns `FailInfo`, and the checker renders
it (rule TS-1).

### 10.1 `FailInfo`

```rust
pub struct FailInfo {
    pub leaf: CanonGoalRef,                  // the deepest failing goal on the committed path: the root cause
    pub chain: SmallVec<[ChainStep; 4]>,     // the committed impls from the asked goal down to the leaf
    pub reason: FailReason,
    pub near: SmallVec<[NearMiss; 3]>,       // at most three, in content order
}
pub struct ChainStep { pub impl_row: ImplRef, pub step: u16, pub origin: ImplOrigin }
pub enum FailReason {
    NoImpl,                                  // no head matches the leaf
    Binding { name: Symbol, found: Ty },     // the impl binds another type
    NotMutable,                              // a `mut` bound on a readonly type
    NotInspectable { arg: Ty },              // the first type argument that is not inspectable
    WrongCategory,                           // AnyVal for a reference, AnyRef for a value
    Sealed,                                  // a sealed trait the type is not listed for
}
pub enum NearMiss {
    OtherInstantiation(ImplRef),             // `Store[mut User]` exists; `Store[User]` was asked
    NotAvailable(DefId),                     // a trait with the method exists but is not in scope
    Derivable { trait_: DefId, target: DefId }, // the leaf could be derived
}
```

Because of rule TS-2, the failing path is unique: one chain from the
asked goal to one leaf. There is no "which of these alternatives did you
mean" tree to summarize.

### 10.2 Which Code

The checker picks the code from the site and the chain:

| Site or chain | Code |
| --- | --- |
| a bound of a call or a type, failing at the leaf | `unsatisfied-trait-bound` |
| the chain's last step is a bound that a derived head added | `missing-derived-bound` ([`trait.derive.bound-unmet`](../../spec/lang/09-traits.md#r-trait.derive.bound-unmet)) |
| a member obligation in a derive instance of `Eq`, `PartialOrd`, `Ord` or `Hash` | `derive-field-missing-trait`, at the field |
| a member obligation of any other template | `member-not-derivable`, at the opt-in, naming the member |
| an impl check of a supertrait or supertrait binding | `missing-supertrait-implementation` |
| a conversion to a trait value | `type-mismatch` |
| an operator with no fitting impl | `type-mismatch` naming the missing trait ([`expr.op.no-impl`](../../spec/lang/05-expressions.md#r-expr.op.no-impl)) |
| `Overflow` | `trait-resolution-depth` |

### 10.3 Messages And Fix-Its

**Name the root cause, then the path.** One line, then one hint:

```text
app.hd:12:9: unsatisfied-trait-bound: `Box[Handle]` does not implement `Eq`, because `Handle` does not
hint: add `@derive(Eq)` to `data Handle` (app.hd:3)
```

With `--verbose`, a note lists the chain: "`impl[T < Eq] Eq for Box[T]`
(derived, app.hd:5) requires `T < Eq`". The compact form stays within the
60-token target of the `mistakes` metric.

**Missing-derive fix-it.** When the leaf is `X: Tr`, `X` is a data type,
enum or newtype of the user's own package, and `Tr` has a
template, the fix-it inserts `@derive(Tr)` or extends an existing list. It
adds missing law partners, since `@derive(Hash)` alone is
`mixed-derived-law` ([Law Partners](../../spec/lang/09-traits.md#related-traits)),
and gives none when `X` has a hand-written partner impl.

| Near miss | Message |
| --- | --- |
| `OtherInstantiation` | "`Shelf` implements `Store[mut User]`, not `Store[User]`" ([`trait.target.inner-mut.distinct`](../../spec/lang/09-traits.md#r-trait.target.inner-mut.distinct)) |
| `NotMutable` | "`clear_value` needs a mutable `T`", with the `let mut` fix-it |
| `NotAvailable` | the `use` fix-it of [`trait.avail.suggest`](../../spec/lang/09-traits.md#r-trait.avail.suggest) |
| `Binding` | "`Money`'s `Add` gives `Out = i64`, but `Summable` requires `Out = Money`" |

**Overflow messages** show the first three goals of the chain and the
last, and name the impl that grows the goal when the chain repeats one.

### 10.4 One Diagnostic Per Root Cause

A failure's root key is the body plus the leaf goal's canonical key (mine).
Ten `==` uses on a `Point` without `Eq` in one body give one error at the
first use, with a note "and 9 more uses in this body". Different leaves
are different causes: `Box[Handle]: Eq` and `Handle: Hash` give two.
The owner chose this over one error per use site (2026-10-07).

A stalled goal still open at the end of the body becomes crates/hd_check/DESIGN.md (type checking)'s
`cannot-infer-type`, naming the goal's trait: "cannot infer `U`: `Both`
implements `Source[i32]` and `Source[string]`".

## 11. Determinism And Parallelism

**Rule TS-8. Every order the solver shows comes from content.**

- Candidates, `Many` lists, `near` lists and coherence reports are ordered
  by each impl's content `rank`: its module's rank in stable-path order,
  then its item index in source order. An item index, unlike a byte
  offset, does not move when a body above the impl is edited, so the
  interface that stores it stays valid. `rank` is computed when the interface is
  built, so sorting never compares paths at solve time, and never uses a
  `DefId`.
- Elaborated clauses are in declaration order, supertraits depth first in
  declared order. A `Bound` index is therefore the same on every run.
- Canonical placeholders are numbered by first occurrence in a fixed walk.
- The bound plan runs in its stored order, so the first failing step is
  fixed.

**Purity and sharing.** Answers are functions of their keys (rule TS-1),
and fuel is charged per distinct proof node (rule TS-5), so a warm memo, a
shuffled check order or another thread count changes no answer and no
`fuel_used`. The only shared mutable state is the global memo, written
first-writer-wins, and the interners. Impl tables, the candidate directory
and environments are frozen and read through `&`. The global memo's 64
shards keep contention low; two threads that miss on one key both compute
it, which is rare and bounded by the thread count.

## 12. Data Structures

All tables are struct-of-arrays with `u32` indexes, in the style of
[§3.9.4](../../docs/compiler/data-structures.md#394-tables-as-struct-of-arrays). Sizes are per
entry.

| Structure | Where | Size | Lifetime |
| --- | --- | --- | --- |
| `ImplTable` row | interface blob, per module | about 40 B: trait 4, def 4, head key 4, arg keys 8, head ref 4, plan 4, assoc 4, origin 2, params 1, rank 8 | the folder interface |
| bound-plan step | interface blob | 12 B plus its argument template | the folder interface |
| `arg_impls` row | interface blob, per folder | 16 B: trait, head key, module, row | the folder interface |
| `VtableShape` | trait record | 4 B per slot and per direct supertrait | the folder interface |
| `ParamEnv` clause | per item, interned | 16 B: self 4, trait 4, args 4, bindings 4; plus 1 bit `mut` and 2 B origin | process (interned by `EnvKey`) |
| canonical goal key | memo key arena | 20 B: kind and flags 4, trait 4, self 4, args 4, extra 4 | the memo |
| `MemoEntry` | memo | 12 B, plus 4 B per child in the children arena | global: process; body: the body |
| canonical answer | memo answer arena | 4 B tag plus evidence words plus 8 B per learned binding | the memo |
| `Frame` | solver stack | 24 B; at most about 64 goal frames plus their projection steps, a few KiB | one `solve` call |
| scratch (substitutions, seen children) | the worker's scratch buffer | 4 B per word | truncated at each pop |

A `solve` call allocates nothing per goal: frames go on a reused vector and
substitutions on the worker's scratch buffer
([§3.9.5](../../docs/compiler/data-structures.md#395-building-scratch-buffer-checkpoints-truncation)).
A large body asks a few thousand distinct goals, under 100 KB of body
memo; the global memo for std and a 50,000-line package is a few
megabytes. The key and answer encodings come from one schema,
`hd_types/solve.ir`, like the pool's accessors.

## 13. Performance Targets And Pathological Cases

**Targets.**

| Measure | Target |
| --- | --- |
| memo hit, global or body | under 100 ns |
| a computed goal on the fast path (section 3.3) | under 1 µs |
| a computed goal with three bound steps | under 5 µs |
| solver share of check time on std and the generated packages | under 10% |
| `select` per instance call in codegen | under 200 ns, memoized |

**Pathological cases.** Each is in the `pathological` suite, well-typed
and ill-typed (section 14.2).

| Case | Danger | What bounds it | Expected cost |
| --- | --- | --- | --- |
| 1,600 `Cents::from(N)` over two `From` impls (F-626) | a search per call | one `Instantiations` entry, then hits; the checker's trials truncate | 1 computed goal, 1,599 hits; fuel about 5,000 steps; well under 100 ms in all |
| 500 `From[X]` impls for one error type | `Instantiations` returns 500 candidates | sub-buckets by argument key make `Implements` with a known argument a binary search; `Many` is computed once per body | the checker's trial count, not the solver, dominates; section 16.4 change 7 lets the checker prefilter by argument head |
| a supertrait chain 200 deep | elaboration per goal | elaboration once per item; clauses indexed by self type | 200 clauses per item; each goal one index probe |
| diamond supertraits (`Error`'s three supertraits, `Num`'s eight) | duplicate clauses | elaboration keeps one clause per trait reference and merges its bindings | linear in distinct supertraits |
| an associated-type chain 60 deep | normalization blowup | one `Project` per level, memoized; depth counts | 60 goals; at 65, `trait-resolution-depth` |
| a recursive impl through a projection | a cycle | cycle detection on the stack, `Overflow` at the first repeat (rule TS-7) | the cycle's length |
| a goal whose trait arguments grow on each step | unbounded search | depth 64 first, type size limit second | at most 64 levels |
| `==` on a 1,000-element tuple | per-element search | the tuple template's plan: one step per element; distinct element types are memoized | linear in elements |
| `List` nested 64 deep, `Eq` | deep proof | depth limit | 64 goals; 65 deep is `trait-resolution-depth` |
| many bodies asking the same goals | repeated work | the global memo | each distinct goal computed about once per run |
| a method name declared by 50 available traits | 50 goals per call | the per-module method index; one `Instantiations` per trait | 50 memoized goals once per receiver type |

## 14. Testing

### 14.1 Spec Traceability

Each spec area maps to one solver component, a module of `hd_types::solve`
or a resolution check. A CI script reads the `trait.*` rule IDs and the
conformance index, and lists rules with no fixture, per component.

| Spec area | Component |
| --- | --- |
| [Supertraits](../../spec/lang/09-traits.md#supertraits), [Supertrait Bindings](../../spec/lang/09-traits.md#supertrait-bindings) | `env` (elaboration); impl checks |
| [Associated Types](../../spec/lang/09-traits.md#associated-types), [Associated Type Bindings](../../spec/lang/09-traits.md#associated-type-bindings) | `project` |
| [Implementation Targets](../../spec/lang/09-traits.md#implementation-targets), [Implementation Ownership](../../spec/lang/09-traits.md#implementation-ownership) | resolution header checks; `owners` |
| [Overlap](../../spec/lang/09-traits.md#overlap) | `Coherence(trait)`; `plan` (constrained parameters) |
| [Derived Implementations](../../spec/lang/09-traits.md#derived-implementations), [Templates](../../spec/lang/14-annotations.md#templates), [Derived Bounds](../../spec/lang/14-annotations.md#derived-bounds) | derived heads; derive-instance environment |
| [Method Resolution](../../spec/lang/09-traits.md#method-resolution), [Trait Availability](../../spec/lang/09-traits.md#trait-availability) | `methods` |
| [Instantiations Of One Generic Trait](../../spec/lang/09-traits.md#instantiations-of-one-generic-trait), [Inference Through A Bound](../../spec/lang/04-type-system.md#inference-through-a-bound) | `inst` |
| [Generic Bounds And Static Dispatch](../../spec/lang/09-traits.md#generic-bounds-and-static-dispatch), [Mutable Bounds](../../spec/lang/04-type-system.md#mutable-bounds) | `head`, `search`, `memo` (depth) |
| [Dynamic Trait Values](../../spec/lang/09-traits.md#dynamic-trait-values) | resolution (safety); `dynself`; vtable shapes |
| [Sealed Traits](../../spec/lang/09-traits.md#sealed-traits), [Runtime Type Identity](../../spec/lang/09-traits.md#runtime-type-identity), [`AnyVal` And `AnyRef`](../../spec/lang/04-type-system.md#anyval-and-anyref) | `builtin` |
| [Trait Delegation](../../spec/lang/09-traits.md#trait-delegation) | delegated heads; impl checks |
| [Error Derivation](../../spec/lang/14-annotations.md#error-derivation) | generated heads |
| [Tuple Templates](../../spec/lang/14-annotations.md#tuple-templates) | `builtin` (tuple-template plans) |

### 14.2 Test Kinds

| Test | What it checks |
| --- | --- |
| conformance fixtures (`spec/conformance`) | accept or reject and the code of each diagnostic, through the portable command contract; failures in the new compiler's `KNOWN_FAILURES.tsv` |
| differential against the frozen prototype | accept or reject and the set of codes per file, on the suite and on generated programs; known prototype bugs (F-617 to F-620, F-624) are expected disagreements, each a row in a triage list |
| solver unit snapshots | `hd debug solve` over small impl sets: goal in, answer, evidence, height and cost out. These are internal tests of the Rust crate, not conformance fixtures, so the fixture format stays implementation-neutral |
| impl-set fuzzer (mine) | random traits (zero to two parameters, zero or one associated type), random data types, random impl heads that obey ownership, random goals. The oracle is a naive solver: recursive, no memo, no index, no fast path, over every impl in the program. Answers and top-level evidence must agree on every goal that the oracle finishes within the depth limit |
| coherence fuzzer | random heads; the oracle enumerates ground types up to nesting 3 and reports two heads that both match one. Every oracle overlap must be reported, with a witness that matches both heads |
| memo invariance | the same goal sequence with a cold memo, a warm memo, a shuffled body order and 1, 4 and 16 threads: identical answers, identical `fuel_used` per body (rules TS-1, TS-5). It includes two modules that ask one `Instantiations` goal with different closures, in both orders (section 3.2) |
| canonicalization property | alpha-renamed goals get one key; goals that differ only in a variable's kind get different keys |
| select soundness | in CI builds, every `select` at an instance re-solves the full goal and asserts `Holds` with the same impl (rule TS-6) |
| pathological suite | each case of section 13, well-typed and ill-typed, within its fuel and wall-time budget, with its limit diagnostic; each also at sizes `n` and `2n`, failing when the observed exponent exceeds 1.2 (section 7.6) |
| one-root-cause fuzzer | mutate one impl or bound of a well-typed program; more than two error diagnostics flags a cascade for review |
| adversarial solver fixtures (Codex re-review N-R1, gate 4; frontend lane) | built before the full checker, run with shuffled scheduling before threads: two modules with different candidate universes in both orders (section 3.2); binding-constrained impl parameters through `select` (section 8.3); supertrait diamonds that bind on one path only, and with conflicting bindings (section 4.2); shared-subgoal DAGs with a fixed fuel charge (section 7.4); residual bounds that stall, then fit or fail (section 6.5); projections through impls whose bounds fail (section 4.3); trials that hold early returns or wake older stalled calls (crates/hd_check/DESIGN.md (type checking) §3.5). Accept or reject goes into implementation-neutral conformance fixtures; costs and answers go into solver snapshots |

## 15. Prototype Failures And The Rules That Prevent Them

From `src/KNOWN_ISSUES.md` (deleted with the prototype), its history, the
checker audit and the
findings (both deleted 2026-10-15; `git show
44890c30:audit/compiler/checker-2026-10-04.md` and `findings-2026-10-04.md`):

| Prototype failure | Evidence | Rule here |
| --- | --- | --- |
| a call with several instantiations copies checker state per candidate: 1,600 `From` calls take 96 s | F-626 | TS-1 (the solver writes nothing); one memoized `Instantiations` per receiver; TS-5; the checker's trials truncate |
| `Trait::f()` for an associated function rejected instead of inferring `Self` | F-617 (fixed in the prototype) | an `Implements` goal with a placeholder self stalls, and the obligation resolves when `Self` is known; open at the end, `cannot-infer-type` ([`trait.assoc-call.trait.undetermined`](../../spec/lang/09-traits.md#r-trait.assoc-call.trait.undetermined)) |
| a trait value of a generic trait does not satisfy the instantiated bound; only argument-free bounds were forwarded | F-618 | the trait-value source compares the full trait reference (section 9.3) |
| `T < mut Any` drops the `Any` bound and its `mut` | F-619 | `mut_` on every goal; `Any` is an ordinary sealed row, never dropped |
| `TypeId` spelled by bare name; two same-named types of different modules could share an identity | F-620 | `Inspectable` evidence names the type, and codegen derives identity from the canonical type over stable paths ([§13.3](../hd_mono/DESIGN.md#133-instance-keys)) |
| `Trait::method(item)` with a parameter receiver searched impls only | F-624 | one goal for every source, the environment first (section 3.1) |
| impl matching on canonical type strings | CA-02 | heads in the pool; the head index; TC-2 |
| dictionary plans in HIR, an erased-dictionary ABI shared with the emitter | CA-04 | TIR records only the top choice; codegen selects by head (TS-6); dictionaries only for trait values; `dyn` generic methods get type witnesses (crates/hd_mono/DESIGN.md §13.5.1); GADT evidence is removed with GADTs |
| derivation diagnostics in process-global registries keyed by span | CS-02 | `FailInfo` carries the impl's origin; diagnostics are values in the body result |
| derives through generated source that is parsed again | checker audit, Derivation And Std Integration | the solver sees derived heads only; template bodies are token text with a resolution table, checked as derive instances |
| bound inference repeats until no solution changes | checker audit, `bound-inference.ts` | the spec's bounded loop stays in the checker; the solver has no fixpoint (TS-7) |
| associated calls and member calls use separate selection paths | checker audit | one `Methods` goal for both; one `Instantiations` goal for the choice |

## 16. Open Questions, Readings And Changes

### 16.1 Questions For The Owner

All four are answered (owner, 2026-10-07) and used above:

| Question | Answer | Where |
| --- | --- | --- |
| 1. Which impls an instantiation set counts | only impls in the asking module's dependency closure; the spec pass adds a sentence to [Instantiations Of One Generic Trait](../../spec/lang/09-traits.md#instantiations-of-one-generic-trait) and [Inference Through A Bound](../../spec/lang/04-type-system.md#inference-through-a-bound) | section 3.2 |
| 2. Projections of impl parameters in impl heads | forbidden, as `unconstrained-impl-parameter`; the spec pass says so in [`trait.overlap.constrained-head`](../../spec/lang/09-traits.md#r-trait.overlap.constrained-head) | section 4.4 |
| 3. One level of proof depth | a bound-plan step, a tuple template's element obligation, an `Inspectable` step and a projection step; environment clauses, trait-value sources and sealed memberships are no level; the spec pass adds the rule | section 7.3 |
| 4. One error per missing impl per body, or per use | one per body and leaf goal, with "and N more uses" | section 10.4 |

### 16.2 Readings Of The Spec To Confirm

These follow from the spec as written; no answer is needed unless the
owner disagrees.

1. **`Implements` never learns from a unique head match** (rule TS-3). Only
   associated-type bindings teach a variable, and only `Instantiations`
   chooses among instantiations, where the spec says so.
2. **A cycle reports `trait-resolution-depth` at once.** An inductive
   cycle has no finite proof, so the spec's depth rule already rejects it.
3. **No depth limit at instances.** Codegen's concrete proofs are not
   source uses (section 8.3).
4. **`Structure` holds through a template instance's environment**,
   never as a written impl
   ([`annot.structure.generated`](../../spec/lang/14-annotations.md#r-annot.structure.generated)).
   The one compiler-wide answer is a concrete tuple type (#126): its row is
   shared by checking and codegen because row memo entries carry no mode.
   Checking cannot reach it from user code, since naming `Structure`
   outside a template is `structure-outside-template` and a template's `T`
   is a parameter its environment answers. A template body that names
   `Structure` at a concrete tuple would get the tuple answer; nothing
   writes that today.

### 16.3 Inconsistencies Found

1. **Tuple impls: compiler-derived or templates?** Chapter 12's table says
   tuple equality, ordering and hashing are "derived by the compiler for
   every arity". Chapter 9 and chapter 14 say every tuple trait comes from
   a std tuple template. crates/hd_check/DESIGN.md (type checking) §6.2 answers them with
   `Evidence::Builtin`, and codegen §13.6 generates them per arity. This
   design follows chapters 9 and 14: they are `Impl` evidence of the
   template. Chapter 12's table should link to tuple templates.
2. **Which std module owns a primitive?** Trait impls must live in a module
   that declares the trait, the target or a trait argument
   ([`trait.own.module.trait`](../../spec/lang/09-traits.md#r-trait.own.module.trait)).
   For `impl Display for i32` in std, no std module "declares" `i32`. D1
   maps built-in owners to "fixed std modules". This design reads it as:
   any std module may hold impls whose target is a std-owned built-in
   constructor, and std's interface gathers them into one synthetic owner
   table. The spec should say so, as
   [`trait.own.module.inherent.std`](../../spec/lang/09-traits.md#r-trait.own.module.inherent.std)
   does for inherent impls.
3. **D1 §4.12.1's "at most `2 + len(Args)` modules"** holds only when the
   trait arguments are known. Open arguments need the candidate directory
   (section 3.2).
4. **D1 §4.12.2's cycle rule** ("a goal already on the stack succeeds only
   inside a derived impl's member check") and crates/hd_check/DESIGN.md (type checking)'s
   `Coinductive` evidence describe a mechanism that is not needed: the
   derived head is in the table, so the member check never meets a cycle
   (section 3.10). Every cycle that the search meets is `Overflow`.
5. **Smaller gaps**, each fixed by a change in section 16.4: D2's
   "dictionaries only for trait values and GADT evidence" (both GADT
   evidence and `CallDyn` bound evidence are now gone; change 19); crates/hd_check/DESIGN.md (type checking)'s goals have no `mut`, the
   prototype's F-619 (change 1); D2's collection re-solves where a head
   match suffices (change 18).

### 16.4 Changes Needed In type-checking.md And The Other Design Files

For the owners of those files to make. This document edits none of them.
Status, 2026-10-07: changes 1 to 11 are applied in crates/hd_check/DESIGN.md (type checking) and
changes 12 to 14 in crates/hd_resolve/DESIGN.md. Changes 15 to 21 are
the backend lane's, applied in crates/hd_mono/DESIGN.md, crates/hd_tir/DESIGN.md, crates/hd_cache/DESIGN.md
and docs/compiler/data-structures.md.

**crates/hd_check/DESIGN.md (type checking)**

1. **§1.6 `Goal`:** add `mut_: bool` to `Implements` and `Instantiations`;
   wrap the self type and arguments in a `TraitRef`; `Project` names the
   associated item's `DefId` (section 2.1).
2. **§1.6 memo:** the keys and eligibility of section 7.1: the `Env`
   scope, `LocalVis`, `AvailKey`, a global memo per run, and no global
   entry for a goal
   that met the depth cut or ran out of fuel.
3. **§1.6 fuel and §11.1:** each proof node is charged once per body, and
   a repeated ask costs 1 (rule TS-5).
4. **§1.6 `Answer` and `Evidence`:** add `Normalized { ty, evidence,
   learned }` for `Project`; add `TraitValue { trait_ }`; drop
   `Coinductive`; a `Bound` index is a clause index in the elaborated
   environment.
5. **§1.6 `Solver`:** add `elaborate`, `select` and `normalize_concrete`
   (section 1.3). The solver tracks depth itself, by heights.
6. **§6.2:** tuple `Eq`, `Ord`, `Hash` and `Debug` are `Impl` evidence of
   std's tuple templates, not `Builtin`.
7. **§2.5 instantiation choice:** before trials, drop candidates whose
   parameter's head constructor cannot match an argument whose type is
   already known.
8. **§2.5 trial memo (mine):** memoize each call site's trial outcomes by
   the canonical expected type and argument types, so nested chains of
   expected-type-dependent calls cost sites × candidates (section 7.6).
9. **§2.5 fits (review T4):** a candidate fits only if, when the call has
   an expected type, the method's result is assignable to it
   ([`trait.resolve.fits.expected`](../../spec/lang/09-traits.md#r-trait.resolve.applies.expected)).
   Each trial runs `coerce(result, want)` inside its rollback before the
   fits are counted, so `let n: i32 = money.pick()` picks `Pick[i32]`.
10. **§2.5 `Methods`:** the answer also carries the unavailable traits
    that would match, for the `use` fix-it.
11. **§2.7 obligations:** keep a projection-equality obligation instead of
    binding a variable to a projection over itself (section 4.3); queue
    each obligation once per wake round.

**crates/hd_resolve/DESIGN.md**

12. **§4.12.1:** add the `ImplTable` columns of section 3.3, the
    `arg_impls` interface section, the per-run candidate directory, and
    std's synthetic table for built-in targets, inherent impls included.
13. **§4.10 step 5:** compute each impl's bound plan; reject projections
    of impl parameters in heads (question 2); store each trait's
    per-member `dyn` availability and vtable shape; list tuple templates in the
    `heads` section with head key `TupleAny`.
14. **§4.12.3:** replace pairwise bucket unification with the hash set and
    discrimination tree of section 5.2, and report a witness type.

**docs/compiler/data-structures.md**

15. **§3.4 and §3.9.2:** the projection type is `Assoc { assoc: DefId,
    tref }`, with the trait's arguments, not `{ base, trait_, name }`.

**crates/hd_tir/DESIGN.md**

16. **§4.13.11 catalog:** the `TraitMethod` choice gains `TraitValue` and
    `Builtin`. `CallDyn` has no evidence operands: the erased body gets
    its bound methods from the type witness (Codex re-review N5).
    (`NewVariant` evidence choices are removed with GADTs.)
17. **§4.13.9:** a derive instance needs no coinductive assumption
    (section 3.10). Supertraits, supertrait bindings, delegation parts
    and newtype bases are checked by the folder's `HeaderCheck(F)` task
    (crates/hd_resolve/DESIGN.md §4.10.1), not by an M2 body task, and
    crates/hd_sched/DESIGN.md gains that task.

**crates/hd_mono/DESIGN.md**

18. **§13.2 step 4:** select by head only, with no depth limit and no
    fuel; a failure is an internal error (rule TS-6).
19. **§13.5:** vtables follow the trait record's shape, with supertrait
    vtables by pointer; a slot for a method with method-level parameters
    is the erased instance with one type witness per parameter (section
    9.2; crates/hd_mono/DESIGN.md §13.5.1).
20. **§13.6:** tuple `Eq`, `Ord`, `Hash` and `Debug` are tuple-template
    instances, instantiated per tuple type like any impl.

**crates/hd_cache/DESIGN.md**

21. **The `check` key** covers argument-owned impls through the deep
    hashes of the module's dependency closure; the `argc` hash that this
    change first asked for is redundant and goes (Codex re-review N-A1,
    section 3.2). The solver memo is never persisted and never part of a
    key.

### 16.5 Codex Review

The Codex review of commit 8bb6860d (`codex_review.md`, not committed)
predates this document. Each finding below was checked against the spec
before it was accepted.

| Finding | Verdict | Where |
| --- | --- | --- |
| Blocker 3: owner lookup cannot enumerate unknown trait arguments; std's inherent exception | **Accepted, fixed.** Verified: [`trait.own.argument`](../../spec/lang/09-traits.md#r-trait.own.argument) allows argument-owned impls, and [`trait.own.module.inherent.std`](../../spec/lang/09-traits.md#r-trait.own.module.inherent.std) allows any std module. Taken: the per-run, per-trait candidate directory and a coarse hash in the check key. Changed: the directory is filtered by the asking module's dependency closure, so a downstream package cannot change a library's result (question 1) | sections 3.2, 5.3; changes 12, 21 |
| Blocker 5: dynamically safe generic methods lack a codegen strategy | **Accepted, fixed** for the solver's part. Verified against [`types.trait.safe.method-type-arg`](../../spec/lang/04-type-system.md#r-types.trait.safe.method-type-arg): an erased slot instance with one vtable parameter per bound | section 9.2; changes 16, 19 |
| Blocker 8: memo entries are not functions of their keys | **Accepted, fixed.** Keys now hold the environment, visible local impls, availability, and a per-run memo; depth is a stored height with lower-bound entries; fuel and depth exhaustion are never cached as failures; completion before publication is the SCC rule. Not needed: a key for coinductive assumptions, since none exist | sections 6.3, 7.1, 7.3; change 2 |
| T1: projections cannot represent their inputs or return outputs | **Accepted, fixed.** Verified: D1's `Assoc` drops the trait arguments, and §1.6's answers had no normalized type. Projections now name the associated item and the instantiated trait reference; `Project` answers `Normalized`; normalization cycles, the occurs check and aliases are specified | sections 2.1, 4.3; changes 1, 4, 11, 15 |
| T4: candidate trials omit expected-result filtering | **Accepted.** Verified: [`trait.resolve.fits.expected`](../../spec/lang/09-traits.md#r-trait.resolve.applies.expected). The trial is the checker's; the change is stated for crates/hd_check/DESIGN.md (type checking) | change 9 |
| T6: fuel bounds work but does not make it near-linear | **Accepted, fixed** for the solver and coherence: the algorithms of section 7.6, the discrimination-tree overlap check, and the doubling test. The trial multiplication and obligation waking belong to the checker; changes 8 and 11 state them | sections 5.2, 7.6, 14.2; changes 7, 8, 11, 14 |
| A5: synthetic impls need dependency and coherence rules | **Accepted, fixed.** Verified: any trait may have a tuple template ([`annot.template.tuple.form`](../../spec/lang/14-annotations.md#r-annot.template.tuple.form)), and a written tuple impl beside it is an overlap ([`annot.template.tuple.overlap`](../../spec/lang/14-annotations.md#r-annot.template.tuple.overlap)) | section 3.11; change 13 |
