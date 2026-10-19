# New Compiler: Type Checking

Status: design record; may lag the code. The code and the spec win.
Checked against the code at 3209d690: notes marked "Implementation
differs" say where the code does less than this design.

This document details the body checker of the new compiler. It refines
these sections of [COMPILER_DESIGN.md](../../docs/compiler/design-overview.md):

- [§3 Core Data Structures](../../docs/compiler/data-structures.md#3-core-data-structures):
  IDs, interners, types, arenas, the poison type, diagnostics;
- [§4.9 Name Resolution](../hd_resolve/DESIGN.md#49-name-resolution) and
  [§4.10 Folder Interface Construction](../hd_resolve/DESIGN.md#410-folder-interface-construction):
  what the checker receives;
- [§4.12 Traits, Impls And Coherence](../hd_resolve/DESIGN.md#412-traits-impls-and-coherence):
  what the checker asks the trait solver;
- [§4.13 Body Checking](../hd_tir/DESIGN.md#413-body-checking): the
  per-module phases M1, M2 and M3;
- [§4.14 Diagnostics](../hd_tir/DESIGN.md#414-diagnostics) and
  [§4.15 Limits](../hd_tir/DESIGN.md#415-limits);
- [§6 Scheduler](../hd_sched/DESIGN.md#6-scheduler-and-task-graph) and
  [§8 Determinism And Soundness Tests](../../docs/compiler/testing-the-compiler.md#8-determinism-and-soundness-tests).

The trait solver has its own design, [crates/hd_check/DESIGN.md (trait solver)](#new-compiler-the-trait-solver).
Section 1.6 states the interface between the two.

## How To Read This

- **Rule TC-n** marks a design rule that a reviewer can check against the
  code.
- Rust code shows the shape of the real types; field lists are trimmed.
- The spec decides semantics.

**Two premises from the backend design.**

1. **One typed IR per body: TIR.** The checker emits TIR while it checks,
   as Zig (Sema to AIR) and Carbon (SemIR) do. TIR holds types,
   desugaring, explicit coercions, match decisions, cleanup scopes,
   suspension points and structured control flow. No monomorphized IR is
   built: emission instantiates as it writes Wasm. The instruction
   catalog, builder and invariants are
   [crates/hd_tir/DESIGN.md §4.13.11](../hd_tir/DESIGN.md#41311-the-typed-ir-tir).
2. **Data-oriented storage.** Types live in an InternPool; TIR is
   columns; a trial's rollback is truncation of append-only columns plus
   the inference trail
   ([§3.9](../../docs/compiler/data-structures.md#39-data-oriented-encoding)).

## 1. Scope And Interfaces

### 1.1 Who Owns What

| Component | Owns | Does not own |
| --- | --- | --- |
| Resolution (`hd_resolve`, before) | uses, module scope, paths in headers, folder interfaces, impl tables, derived heads, the header checks that need no goal, the overlap check | anything in a body except path lookup through the frozen scope |
| **Type checker (`hd_check`, this document)** | name lookup of locals, every expression, statement and pattern in a body; inference variables; coercions; rows used by bodies; suspension checks; exhaustiveness and match decisions; mutability; definite initialization; omitted results and rows; parameter-environment elaboration (`body::add_bound`); normalization of projections; body diagnostics; the header checks that need goals (`hd_check::header`) | trait impl search, impl matching, TIR encoding, instantiation |
| Trait solver (`hd_types::solver`, beside) | `Implements`, `Instantiations` and the trait part of `Methods`; its memo | inference variables (it reads them, never writes them), diagnostic text |
| TIR builder (`hd_tir::ir::TirBuilder`, called by the checker) | the instruction encoding, checkpoints, the TIR verifier | types, names, any decision the checker makes |
| Collection and emission (`hd_mono`, after) | instances, layout, Wasm | re-deriving anything TIR states |

**Rule TC-1. The checker decides; the builder records.** Every semantic
choice is made in the checker: which callee, which impl or bound, which
coercion, which literal width. TIR states it. Collection never consults
names, scopes or the checker to reconstruct a choice.

### 1.2 Inputs

All inputs are frozen before a module's bodies are checked. The `Body(m)`
task builds one `BodyCx` (`hd_check::body`) and checks every body of the
module through it:

| `BodyCx` field | Used for |
| --- | --- |
| `names`, `src` | interned names, paths, the pool; the module's syntax |
| `scope`, `lookup`, `locals` | the module scope; items of the module and of every interface in its closure; block-local items |
| `impls` (`ImplView`), `solver`, `global` | the solver's view of impls (trait solver §3.2), the solver, the run's global memo |
| `methods` | the method index, built on first use |
| `init`, `results` | module initialization facts; M1's registry of omitted results and rows |
| `tests`, `session` | the `tests:` block scope; REPL session nodes |

**Header projection completion.** Header lowering leaves the trait
arguments empty on projections written `Self::Out` and on projections
inherited from a bound, such as `P::Error` for `P < Walker[Self]`. When
the checker instantiates that header, it fills a `Self` projection from
the enclosing trait call's arguments, and a bound projection from the
matching environment clause; arguments a concrete impl must determine
become fresh variables before solving. Only then is the projection
normalized.

### 1.3 Outputs

`check_fn(cx, def, node, diags) -> StageResult<hd_tir::Body>` checks one
body: diagnostics go to the module's `DiagBuf`, and the TIR body is
returned with every type resolved. A body that stops on a construct the
compiler does not carry yet returns a structured "not implemented".

`Body(m)` gathers the module's bodies, its diagnostics, its derived
methods, the inferred results and rows (section 9), the init summary
(section 8.4) and debug references. `ModuleFinish(m)` writes them into
the module's `check` entry. A module with errors keeps no TIR and an
empty init summary.

### 1.4 The Type Accessor API

The checker reads and builds types through `hd_types::Types`, a copyable
view of the global `InternPool` and, while a body is checked, that body's
`LocalPool`.

```rust
pub struct Ty(pub u32);       // bit 31: a body-local index (holds inference variables)
pub struct TyList(pub u32);
pub struct RowId(pub u32);

pub enum TyData {              // the decoded view; owned
    Prim(Prim), Never, Poison, // void is the empty tuple, `Ty::VOID`
    Adt { def: DefId, args: TyList },
    Tuple { elems: TyList, rest: Option<Ty> },
    Option(Ty),                // T? is Option[T]; kept apart for speed only
    Fn { params: TyList, result: Ty, row: RowId, suspends: bool, inputs: Inputs },
    TraitValue { def: DefId, args: TyList, bindings: Vec<(DefId, Ty)> },
    Param(ParamRef),           // declared, rigid; never inferred
    Assoc { assoc: DefId, trait_: DefId, self_ty: Ty, args: TyList }, // a projection
    Mut(Ty),                   // `mut T`; `T` alone is readonly
    Infer(InferVar),           // body-local only
    Canon(u8),                 // a solver placeholder
    Row(RowId),                // a row in a row parameter's argument slot
    Context(RowId),            // `$.Context[$ Row]`
}
```

- **`Mut`, not `Readonly`.** In the spec `T` is the readonly view and
  `mut T` is the marked form
  ([Views](../../spec/lang/04-type-system.md#views)).
- **Projections name their item.** `Assoc` holds the associated type's
  own `DefId` and the trait reference, so `<S as Add[i32]>::Out` and
  `<S as Add[i64]>::Out` are different types.
- **One inference-variable form.** A literal is an ordinary `Infer`
  variable whose kind lives in the inference table, because two variables
  merge their kinds when unified (section 3.6).
- **Global types are shared, local types are not.** Interning puts a
  variable-free type in the global pool and a type with variables in the
  body-local pool. Reading a local index from another body panics.
- **Flags are computed at intern time** (`HAS_INFER`, `HAS_POISON`,
  `HAS_PARAM`, `HAS_ASSOC`, `HAS_CANON`, and a node count in the meta
  word), so "does this type hold a variable or poison" is one load.

**Rule TC-2. Types are handles, never text.** No checker code compares,
hashes or parses type spellings. `Ty` has no `Display` impl; messages
render through the checker's `show`, and `Types::display` is a debug
printer.

### 1.5 What The Checker Needs From The TIR Builder

The checker is the builder's only caller. It writes through the
`TirSink` trait, which `TirBuilder` implements:

| Builder operation | Checker use |
| --- | --- |
| `konst` | a literal or other constant: a constant `Ref`, with no instruction |
| `local`, `get`, `set`, `prim`, `emit` | every expression, in evaluation order; `emit` also writes `Await`, `AwaitAll` and `AwaitRace` |
| `call` with a `Callee` and `Providers` | every resolved call |
| `coerce` | each implicit conversion (section 4.2) |
| `open_block`/`close_block`, `open_scope`/`defer`/`close_scope`, `open_loop`/`close_loop`, `open_sub`/`capture`/`close_sub` | every block, cleanup scope, loop and closure, in source nesting |
| `checkpoint`, `rollback` | every trial (section 3.5) |
| `TirBuilder::finish(root, capture_modes)` | once per body: the capture modes, then the verifier |

**A call decided later.** A trait method call whose impl waits on
inference is emitted with a pending choice; the checker patches the
callee record when the goal settles, at the end of the statement or of
the body. `reserve` and `fill` exist in the builder, but the checker
does not use them.

**Constants have no span in TIR, and the checker needs none.** The
checker keeps each literal's node itself (`lit_nodes`), so literal
diagnostics never read TIR. A literal's constant carries an open type
until the end of the body, when the checker resolves it.

**What the checker promises.** These are the invariants of §4.13.11
seen from the caller's side.

1. **Calls are resolved.** A `Callee` is an item with its type
   arguments, or a trait method with its choice (`Impl`, `Bound`,
   `TraitValue` or `Builtin`). Operators, interpolation, indexing,
   iteration, `?`, pipes, compound assignment, comprehensions and default
   arguments arrive desugared. A default is a call of the default's own
   body with the earlier arguments (section 2.4 step 3).
2. **Every implicit conversion is a `coerce`.** So every operand's type
   equals the type its position expects.
3. **Scopes are explicit.** Every cleanup scope of
   [Cleanup Scopes](../../spec/lang/06-control-flow.md#cleanup-scopes) is
   an `open_scope`/`close_scope` pair, and every exit names its target.
4. **Types resolve once.** Instructions may carry types with inference
   variables while the body is checked. At the end of the body the
   checker resolves the constants and the `ty` column in one sweep
   (`finish_with`), then calls `finish`.
5. **Matches emit their arms first.** The checker emits each arm's block
   while it checks the arm, then the decision block (section 7.2).
6. **Checks read the checker's own tables.** Diagnostics never come from
   reading instructions back (sections 8.2 to 8.4). The local flags live
   in the builder's local table, which the checker reads and writes.

**Rule TC-3. Checking does not depend on emission.** Promise 6 means the
builder could be swapped for a discarding one with no change in
diagnostics.

**Where a check drops TIR.** `hd check` emits TIR and writes it to the
module's `check` entry at `ModuleFinish`, so a later build reuses it
instead of rechecking. This design adds one mode:

| Mode | Used by | TIR |
| --- | --- | --- |
| emit (default) | `hd check`, `hd build`, `hd run`, `hd test` | built per body; written at `ModuleFinish` |
| no-emit | the playground's check-as-you-type, `hd fix` rounds, `hd doc` | never built; the checker is generic over the builder, and the discarding sink's methods are no-ops |

The no-emit mode saves about 200 bytes per source line of transient
memory and the emission time where no build follows. Its `check` entry
has no TIR, so a later build rechecks that module.

Implementation differs: the checker holds a concrete `TirBuilder`
(`Ck::b`), so there is no no-emit mode; every check builds and verifies
TIR, which costs the playground and `hd fix` memory and time for nothing.
The verifier runs on every body, not only in debug builds; not judged.

### 1.6 The Trait Solver Interface

The checker asks the solver three kinds of goals. The solver never writes
the checker's state, never reports a diagnostic, and never sees spans.

```rust
pub struct TraitRef { pub trait_: DefId, pub self_ty: Ty, pub args: TyList }

pub enum Goal {
    Implements { tref: TraitRef, bindings: Vec<(DefId, Ty)>, mut_: bool },
    Project { assoc: DefId, tref: TraitRef },          // not answered: see below
    Instantiations { self_ty: Ty, trait_: DefId, mut_: bool },
    /// The trait part of method lookup: the available traits that declare
    /// `name`, from the module's method index, in content order.
    Methods { receiver: Ty, name: Symbol, traits: Vec<DefId> },
}

pub enum Answer {
    Holds { evidence: Evidence, learned: Vec<(InferVar, Ty)> },
    Normalized { ty: Ty },                 // reserved for Project
    Candidates(Vec<Candidate>),            // Instantiations, Methods
    Fails(Box<FailInfo>),
    Stalled { on: Vec<InferVar> },
    Overflow,                              // trait-resolution-depth
    OutOfFuel,
}

pub enum Evidence {
    Impl { row: ImplRef, args: TyList },   // written, derived, delegated, generated, numeric family, tuple template
    Bound { param: ParamRef, index: u16 }, // a clause index in the elaborated environment
    TraitValue { trait_: DefId },
    Builtin(BuiltinImpl),                  // compiler-supplied (trait solver §3.9)
    Poison,
}

pub struct SolveCx<'a> {
    pub pool: Types<'a>,
    pub env: &'a ParamEnv,                 // the item's elaborated environment
    pub universe: ImplUniverseId,          // trait solver §3.2
    pub impls: Impls<'a>,                  // own table, owner folders, universe rows, hidden local impls
    pub decls: &'a dyn Declarations,       // what the compiler-supplied rows read
    pub body_memo: &'a mut BodyMemo,
    pub global: &'a GlobalMemo,
}

pub trait Solver: Sync {
    fn solve(&self, cx: &mut SolveCx<'_>, goal: &Goal, fuel: &mut Fuel) -> StageResult<Answer>;
    fn elaborate(&self, bounds: &[DeclaredBound], out: &mut ParamEnvBuilder) -> EnvKey;
    fn select(&self, cx: &mut SolveCx<'_>, tref: ConcreteTraitRef) -> StageResult<Selection>; // codegen
    fn normalize_concrete(&self, pool: Types<'_>, impls: Impls<'_>, t: Ty) -> StageResult<Ty>; // codegen
}
```

- **No `Coinductive` evidence.** A derive instance's member goals reach
  the derived head as an ordinary `Impl` (trait solver §3.10).
- **Tuples are templates.** Tuple `Eq`, `Ord`, `Hash` and `Debug` are
  `Impl` evidence of std's tuple templates (section 6.2).
- **Elaboration is the checker's.** `body::add_bound` builds an item's
  `ParamEnv` once per body: each declared bound, its defaulted arguments
  filled, then its supertraits depth first, each trait reference once.
  `Solver::elaborate` only copies clauses.
- **Inherent methods are the checker's.** It finds them through the
  method index; `Methods` answers only the trait part. The checker also
  lists the unavailable traits that would match, for the `use` hint.
- **Projections are the checker's.** `Project` answers "not implemented".
  The checker normalizes (`Ck::normalize`): a parameter's projection
  through an environment binding, a trait value's through its binding,
  and a known self type by solving `Implements` and reading the
  committed impl's binding, else the trait's default.
  Implementation differs from trait solver §4.3: normalization is not a
  memoized goal, and a failed `Implements` leaves the projection rigid
  instead of failing; not judged.

**How the checker uses answers.**

- `Holds` with `learned` bindings: the checker applies each binding
  through its own unifier, on its own trail (section 3.5).
- `Stalled`: the checker keeps the goal as a `PendingGoal` and asks again
  at the end of the statement, and at the end of the body (section 2.7).
- `Fails`: the checker reports the error with `FailInfo` for the
  message.
- `Overflow`: `trait-resolution-depth`, and the expression is poisoned.
- `OutOfFuel`: see section 11.2.
- Associated-type bindings in an `Implements` goal are checked by the
  checker against the normalized projection (`check_bindings`); the
  solver ignores them, and a goal with bindings skips the memo.

**What the checker sees of caching and budgets.**

- **Memo.** The solver canonicalizes a goal: variables numbered by first
  occurrence. The key holds the canonical goal, the `EnvKey` when the
  goal names a parameter, and the context's `ImplUniverseId` for a goal
  that may read the candidate directory (trait solver §7.1). The
  checker never reads or writes memo entries itself.
- **Fuel.** The checker passes its body's `Fuel`. The first time a body
  meets a canonical goal, the solver charges 1 plus the heads matched for
  each node of its proof DAG the body has not met yet; a later ask of the
  same goal costs 1 (trait solver rule TS-5). So the fuel a goal costs is
  the same on every run, thread count and cache state.
- **Depth.** The solver tracks depth itself, by stored heights, and
  answers `Overflow` past 64 levels.
- **Rollback.** The body memo lives in `Ck` and is lent to the solver for
  each call. Its keys and answers name no body-local type, so a rollback
  that truncates the body-local pool leaves no entry dangling.

### 1.7 Body Tasks And The Exactly-Once Rule

| Phase | Task | Work |
| --- | --- | --- |
| prep | `ModulePrep(m)` | lowers the module's items; on a `check` hit, reads the entry and stops |
| M1 and M2 | `Body(m)`, one task per module | registers the private callables with an omitted result or row; checks the top-level statements; M1's sweep checks the registered bodies (section 9); then every other body in source order: functions and methods, impl members, derive instances, `@error` bodies, test bodies, defaults and facts |
| M3 | `ModuleFinish(m)` | writes the inferred results and rows into the items, sorts and encodes diagnostics, writes TIR and the init summary |

Different modules' `Body` tasks run in parallel; one module's bodies are
checked in order inside one task.

Implementation differs: the design ran a module's M2 bodies as one
batched parallel iterator (crates/hd_sched/DESIGN.md granularity rule);
the code checks them serially in `Body(m)`, so a package with one large
module uses one thread.

**Rule TC-4. Each body is checked exactly once per run.** A body checked
in M1 for its result type is not checked again: `Body(m)` takes its
result (`results::take`). Rows never cause a recheck. Literal widths
never cause a statement recheck (section 3.6). The repeated work is a
speculative trial (section 3.5), which is rolled back, and a generic
local function used from a closure inside its own body before its
captures were known, which is checked again once (`generic_recheck`).

**A body result is a function of the body and the module's frozen
inputs.** A body reads only its own syntax, the module scope, the private
signatures (M1 results included) and the frozen interfaces (rule TC-9).
The first release still rechecks the whole module on any edit
([crates/hd_cache/DESIGN.md](../hd_cache/DESIGN.md)).

**Template target premise.** While checking a method body from a
`by Structure` template, the parameter environment includes the premise
`target < Structure`. It belongs only to that instantiated template body.

**Default and fact bodies.** They are checked once, where they are
declared, and never at the call:

| Body | Checked as | Evaluated |
| --- | --- | --- |
| a parameter default | a function of the earlier parameters and the declaration's generics, with the parameter's type as its result | at each call that omits it, after every explicit argument, in declaration order ([`fn.default.eval`](../../spec/lang/07-functions.md#r-fn.default.eval)) |
| a data field default | a function of the declaration's generics only | at each construction that omits it, in field order ([`data.default.eval`](../../spec/lang/08-data-and-enums.md#r-data.default.eval)) |
| a fact or metadata expression | a body with no parameters | once, on first read ([`annot.fact.eval.lazy`](../../spec/lang/14-annotations.md#r-annot.fact.eval.lazy)) |
| a variant's shared-data constructor and its defaults | a body with no parameters | once, during its declaring module's initialization ([`data.shared.module-init`](../../spec/lang/08-data-and-enums.md#r-data.shared.module-init)) |

All of them are restricted contexts (section 5.8) and must be
requirement-free. A dependent's check reads only a fact's type, never
its value.

## 2. The Algorithm

### 2.1 Modes

The checker is bidirectional and local to one body.

```rust
fn expr(&mut self, e: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)>;
fn coerce(&mut self, r: Ref, got: Ty, want: Ty, n: NodeRef<'_>, what: &str, pos: ExpectPosition) -> Ref;
```

`expr` returns the TIR value and its type. A form that uses `want`
checks against it; one that does not synthesizes its type, and the
caller coerces (section 4.1). A closure gets its parameter types from a
`want` that is a function type.

### 2.2 Expression Forms

Where the expected type flows, form by form. "Check" means the form
uses `want`. "Infer" means it synthesizes and the caller coerces.

| Form | Mode | Expected type flows to | Notes |
| --- | --- | --- | --- |
| integer or float literal | check | the literal: takes `want` if numeric of its kind | no `want`: a fresh literal variable (section 3.6) |
| `-lit`, `+lit` | check | the literal, as a unit for range | signed literal ([`types.literal.local.signed`](../../spec/lang/04-type-system.md#r-types.literal.local.signed)) |
| suffixed literal, prefixed string | infer | the literal gets the literal function's parameter type | a call to the literal function (`literals.rs`) |
| string, char, bool | infer | none | |
| name of a local, global or function | infer | none; a generic function value instantiates with fresh variables | |
| `.Variant(args)` | check only | the enum comes from `want` | no `want`: `missing-contextual-enum-type` |
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
| closure `fn(x): e` | check | parameters and result from a function-type `want` | section 2.6 |
| block of 2+ statements | check | the final expression | its statements are separate literal scopes |
| pipe `a \|> f` | as call | | |
| `$.use(K)` | infer | none | key must be available (section 5.2) |
| `$.with K=p: body` | check | `body` gets `want` | each provider checked against `K` |
| `f!(args)` | as call | | suspension checks (section 5.7) |
| `x is y` | infer both | none | identity rules; see below |
| `_` outside a pipe step | | | `placeholder-outside-pipe` (section 10.4) |

**Interpolation ownership.** An interpolation contributes only its
top-level expression result to the enclosing string's parts. Tokens and
literals nested inside that expression stay operands of the expression.

**Identity `is`** ([Allocation Identity](../../spec/lang/05-expressions.md#allocation-identity)).
After inferring both operands, the checker:

1. Strips `mut` at every level of both types.
2. Asks `AnyRef` of each operand type; one that does not implement it is
   `identity-requires-references`. That covers primitives, strings,
   tuples, every enum (optionals and `Result` included) and function
   types ([`expr.is.value-operand`](../../spec/lang/05-expressions.md#r-expr.is.value-operand)).
3. Requires compatible types: equal, or one a trait value or `Any` that
   the other converts to. Otherwise `incompatible-identity-operands`.
4. Emits `Is` with two operands of reference types.

### 2.3 Statements And Blocks

1. Statements are checked in source order. Each statement is a **literal
   scope**: open literal variables still unbound at its end take their
   default ([`types.literal.local.statement`](../../spec/lang/04-type-system.md#r-types.literal.local.statement)).
   The exceptions are a literal held by an open join (a class equal to an
   open result variable, or a `break` value) and a class that met the
   result of a call whose impl still waits (section 3.6).
2. `let x = e`: infer `e`, then apply the binding rule for the view
   (section 8.1). `let x: T = e`: check `e` against `T`.
3. An expression statement whose value is discarded is checked for
   must-use types after its literal scope closes
   ([`flow.must-use.discard`](../../spec/lang/06-control-flow.md#r-flow.must-use.discard)).
4. After a statement of type `never`, the next statement gets
   `unreachable-code` once per run of unreachable statements, and is
   still checked
   ([`flow.unreachable.checked`](../../spec/lang/06-control-flow.md#r-flow.unreachable.checked)).

### 2.4 Calls And Use-Site Type Arguments

For a call `f(args)` whose callee resolves to a generic declaration:

1. **Instantiate.** Each generic parameter gets a fresh variable. An
   explicit list `f::[A, _]` binds its written slots at once; `_` and
   omitted slots stay variables
   ([Generic Arguments](../../spec/lang/04-type-system.md#generic-arguments)).
2. **Expected result first.** If the call has a `want`, unify the
   callee's result type with it inside a trial: a failure only means the
   expectation gives no information, and is rolled back.
3. **Match arguments to parameters.** Positional, then named, then
   spreads, varargs and a trailing block
   ([Calls](../../spec/lang/05-expressions.md#calls)). A missing argument
   with a default is filled by a **default call**: after every explicit
   argument, in parameter order, the checker emits a call of that
   parameter's default body, with the callee's type arguments and the
   values of the earlier parameters. A data literal fills an omitted
   field the same way, with no earlier fields.
4. **Check arguments.** Each argument is checked against its parameter
   type, which may hold variables. An argument that needs an expected
   type to check (a closure with unannotated parameters, a contextual
   variant, a collection holding one) is **postponed** when its parameter
   type is still an unbound variable at the head, so
   `map(xs, fn(x): x + 1)` works in either argument order. TIR keeps
   source evaluation order.
   Implementation differs: `check_args` postpones only a closure, and
   only its expected type. When a closure argument's parameter type is a
   variable or a function type with a variable input, a trial
   (`solved_by_later`) checks the later arguments, the named ones
   included and closures left out, and is rolled back. The closure is
   then checked against the inputs the trial solved, and every argument
   is checked and emitted in source order, so diagnostics keep their
   order too. A contextual variant or a collection is not postponed.
5. **Join several arguments.** Arguments that solve one bare parameter
   may differ only in outer permission
   ([`types.generic.infer.join`](../../spec/lang/04-type-system.md#r-types.generic.infer.join)).
   Section 3.4 gives the mechanism.
6. **Bounds.** Each instantiated bound becomes an `Implements` goal.
   `Holds` may teach a binding; `Stalled` becomes a pending goal.
7. **Parameters named only in bounds.** Each such parameter is solved
   from its bounded parameter's instantiations of the bound's trait when
   every bound naming it allows exactly one and they agree
   ([`types.generic.infer.bound`](../../spec/lang/04-type-system.md#r-types.generic.infer.bound)),
   repeated to a fixed point; the loop runs at most one more time than
   there are such parameters (`trial::infer_through_bounds`).
8. **Defaults.** A parameter still unbound takes its declared default,
   in declaration order, with earlier arguments substituted
   ([`types.generic.default.fill`](../../spec/lang/04-type-system.md#r-types.generic.default.fill)).
9. **Leftovers** wait until the end of the statement (`open_params`), and
   a call's other variables until the end of the body (`call_params`).
   Then a parameter with no solution is `cannot-infer-type`, and one with
   several is `ambiguous-type`
   ([`types.infer.ambiguous.code`](../../spec/lang/04-type-system.md#r-types.infer.ambiguous.code)).

There is no overloading and no disjunction. One name gives one callee, so
no step searches alternatives (the Swift lesson). The only choice among
candidates is section 2.5's instantiation choice.

### 2.5 Methods And Operators

**Typed structure facts.** The resolved inherent call `h.fact::[D]()` on
`std.structure.Field` is a typed fact lookup: the explicit argument `D`
is checked against the handled member's type, without the ordinary
`D < Inspectable` method bound. The checker recognizes it by the resolved
`Field.fact` identity, not by spelling.

**Method lookup.** Infer the receiver and resolve it shallowly. Inherent
methods come from the method index; the trait part is one `Methods` goal
over the available traits that declare the name
([Method Resolution](../../spec/lang/09-traits.md#method-resolution)).

| Answer | Checker action |
| --- | --- |
| one inherent method | call it |
| methods of two different traits | `ambiguous-method` |
| several instantiations of one generic trait | the instantiation choice below |
| none | `unknown-method`, naming an unavailable trait that would match |
| receiver is an unbound variable | a pending goal settled at the end of the statement; if still unbound, `cannot-infer-type` |

**The instantiation choice** ([Instantiations Of One Generic Trait](../../spec/lang/09-traits.md#instantiations-of-one-generic-trait)).
The solver returns the candidates in content order, each a scheme with
residual bounds (trait solver §6.5). The checker then:

1. **Prefilters.** It drops each candidate whose parameter's head
   constructor cannot match an argument whose type is already known.
2. **Tries each survivor.** Inside a trial (section 3.5) it instantiates
   the scheme: a fresh variable per impl parameter the receiver did not
   fix, and the residual bounds as goals. It checks the arguments against
   the candidate's parameters and, when the call has an expected type,
   runs `coerce(result, want)` inside the trial. A candidate **fits** if
   the trial has no error; a residual that still stalls does not rule it
   out. So `let n: i32 = money.pick()` picks `Pick[i32]` when `Money`
   implements `Pick[i32]` and `Pick[string]`
   ([`trait.resolve.fits.expected`](../../spec/lang/09-traits.md#r-trait.resolve.applies.expected)).
3. **Chooses:**
   - one fit: check the call again for real against it;
   - several fits that differ only in the width of an open literal
     argument: default the literal and choose again
     ([`trait.resolve.literal-arg`](../../spec/lang/09-traits.md#r-trait.resolve.literal-arg));
   - several fits otherwise: `ambiguous-method`;
   - none: `type-mismatch` listing the instantiations.

**Trial cost.** Arguments whose check does not depend on the expected
type are inferred once before the trials (`pre_args`), and a trial only
unifies their types. Arguments that need the expected type (closures,
contextual variants, `if`, `match` and block expressions, empty
collections, and collections holding one) are checked once per
candidate. Every trial charges fuel through the expressions it checks.

**The per-site trial memo.** Nested calls multiply trials, so each call
site would keep its trial outcomes keyed by the call's node, the
candidate's index and the canonical expected and argument types. Not
built yet; see #49. Until then nested instantiation choices whose outer
candidates repeat the inner context cost sites × candidates per level,
and fuel ends the worst cases.

**Operators.** `a op b` follows
[Operator Traits](../../spec/lang/05-expressions.md#operator-traits):

1. If one operand is built only of unsuffixed literals, check the other
   operand first and give its type to the literal side
   ([`types.num.binary.literal-join`](../../spec/lang/04-type-system.md#r-types.num.binary.literal-join)).
2. If both operand types are primitive after that, the built-in rule
   types the operator. Two different numeric types are `type-mismatch`,
   or `mixed-signedness` across signedness.
3. Otherwise the left operand's type selects the impl: the trait call
   `Op::[R]::m(a, b)`. Operators do not use the instantiation choice yet;
   see #40.
4. The emitted TIR is the trait `Call` (or a `Prim`), with the
   evaluation order left, right, call.

**Trait-method conformance** (`conform.rs`). Each impl method's
signature is checked against its trait's; a mismatch is
`trait-method-signature`.

### 2.6 Closures

1. With a function-type `want`, unannotated parameters take the expected
   parameter types, and the result is checked against the expected result.
2. Without it, every parameter needs an annotation:
   `closure-parameter-needs-annotation`.
3. An omitted result is the LCT of the final value and every `return`
   operand (section 4.3). Literal `return` operands stay open until that
   join, even across statements (section 3.6).
4. A closure is monomorphic. Its body is checked inside the enclosing
   body, with the same inference table.
5. Its row is inferred from the keys its body uses (section 5.4).
6. A recursive closure bound by `let f = fn ...` needs its result written
   ([`fn.closure.recursive-result`](../../spec/lang/07-functions.md#r-fn.closure.recursive-result)).
7. Captures are recorded per local (section 8.3).

A local `fn` declaration is a closure bound to its name. One with its
own type parameters is checked once over them, and each use instantiates
it (`sub_flags::GENERIC`).

### 2.7 When Variables Are Resolved

| Variable | Resolved | If still open |
| --- | --- | --- |
| literal (integer or float) | as soon as it meets a concrete numeric type; else at the end of its statement, by default; a held class at its join (section 3.6) | never open past its statement or its join |
| use-site type argument | when unified; else at the end of the statement or body, after defaults | `cannot-infer-type` or `ambiguous-type` |
| local binding's type | from its initializer, in its statement | an empty collection literal can leave its element open to the end of the body (section 2.8) |
| closure parameter | from the expected function type or the annotation | `closure-parameter-needs-annotation` |
| omitted private result | at the end of its body in M1 | never open |
| omitted private row | when its recursive group is solved (section 5.5) | never open |

**Pending goals.** A stalled goal goes on the body's `pending` list with
the node it arose at and what it decides (`Waiting::Bound`, `Call` or
`Project`). The goals a statement created are settled at the end of that
statement (`settle_waiting`); bounds and whatever still waits are settled
at the end of the body, in passes until no pass settles a goal. This
replaces the design's per-variable watch lists: retrying at fixed points
needs no wake queue, and each goal is retried a bounded number of times,
so it is simpler at no extra cost.

### 2.8 Empty Collections

`let names = []` leaves the element type open. The binding's type holds a
variable that later statements may solve, as in `names.push("a")`
([Type Inference Boundaries](../../spec/lang/04-type-system.md#type-inference-boundaries)).
At the end of the body, a still-open element type is `cannot-infer-type`.

## 3. The Unifier

### 3.1 Variables

```rust
pub struct InferVar(u32);       // numbered per body from 0

pub enum VarKind { General, IntLit, SignedIntLit, FloatLit }
```

A variable is created for a use-site type argument, an open literal, an
empty collection's element, a closure parameter or result without an
expected type, a join site, and a placeholder slot `_` in an explicit
list.

### 3.2 Union-Find

- `InferTable` (`hd_types::unify`) is union-find over `InferVar` with
  **union by rank** and no path compression, so a lookup never writes and
  every path is at most log2(n) links long. The root holds the binding.
- `unify(a, b)` resolves both sides shallowly, then:
  - two unbound roots: union; merge kinds (section 3.6);
  - a root and a type: occurs check, kind check, bind; a general
    variable binds to the whole type, `mut` included
    (`types.generic.mut-argument`);
  - `mut` against a plain type: compare the inner types (mutability is
    checked separately);
  - two constructors: same head and arity, then unify children;
  - `Poison` on either side: succeed silently (section 10.1);
  - `Never` against anything: handled by coercion, not here.
- Structural unification recurses natively; the type size bounds it.

### 3.3 The Occurs Check

Binding `?a := T` first checks that `?a` does not occur in `T`. The walk
visits only subterms whose `HAS_INFER` flag is set, so a variable-free
type costs one load. A failure is `UnifyError::Occurs`, reported as
`type-mismatch`.

### 3.4 No Levels, No Generalization

hd has no let-generalization. A local binding is monomorphic, a closure
"declares no type parameters and always has a monomorphic function type"
([`fn.closure.monomorphic`](../../spec/lang/07-functions.md#r-fn.closure.monomorphic)),
and a declaration's generics are always written
([`types.infer.named-fn`](../../spec/lang/04-type-system.md#r-types.infer.named-fn)).
So the unifier has **no levels** and no generalization step. A generic
function used as a value is instantiated with fresh variables at the
use, as a call is.

**Permission join.** Several arguments solving one bare parameter may
differ only in outer `mut`
([`types.generic.infer.join`](../../spec/lang/04-type-system.md#r-types.generic.infer.join)).
This is not unification, so it is handled at the call (`call::join_arg`):
the first argument binds the variable; a later argument that equals it up
to the outermost `mut` lowers the binding to the readonly view
(`InferTable::rebind`, trailed); anything else unifies as usual. An
expected result that fixed the parameter as `mut X` makes a readonly
argument `mutable-upgrade`
([`types.mut.no-upgrade.inference`](../../spec/lang/04-type-system.md#r-types.mut.no-upgrade.inference)).

### 3.5 The Trail And The One Rollback Contract

**Rule TC-5. One rollback contract.** A trial takes a `Checkpoint`
(`trial.rs`) of every piece of body state a check can change, and
`rollback` restores it exactly:

| Class | Rollback | State |
| --- | --- | --- |
| **append-only column** | truncated to its length | the builder's columns (`TirCheckpoint`: instructions, locals, sub-bodies, the local pool); diagnostics; pending goals; literal nodes, overflow literals, negations; user locals; pre-inferred arguments; open, reference and call parameters; `:=` bindings; row waits |
| **trailed** | the inference trail is unwound and the trial's variables dropped (`InferTable::rollback_trial`) | parent, rank, kind, binding |
| **undo log** | `Joins::rollback`; the body undo log (`BodyUndo`), newest first | join sites and their values; the flags of locals older than the trial; init facts; the possibly-uninitialized set; the keys, row parameters and edges added to row frames |
| **balanced stack** | equal depth asserted | scopes, loops, open closures, suspension flags, result types, row frames |
| **kept on purpose** | never rolled back | fuel; the solver's body memo and its `met` set (rule TS-5); an M1 body checked during a trial, which is final (TC-4) |

Users of trials: the expected-result attempt (section 2.4), the
instantiation choice (section 2.5) and generic key collision checks
(section 5.2). Trials nest, within fuel.

No checker state is copied at a checkpoint, so a checkpoint costs the
same in any body. The body undo log is written only while a checkpoint
is open (`commit` closes one that is kept) and emptied when none is.
Replacing the possibly-uninitialized set inside a trial logs the old set,
whose size the `if` that replaces it already pays.

### 3.6 Literal Widths

This follows [Open Literal Width](../../spec/lang/04-type-system.md#open-literal-width).

**Variables.** A literal with an expected numeric type of its kind takes
it on the spot and range-checks (`integer-literal-range`,
`float-literal-range`). A literal with no such type gets a fresh
variable of kind `IntLit`, `SignedIntLit` or `FloatLit`. An integer
literal never takes a float type, and the reverse
([`types.literal.int-not-float`](../../spec/lang/04-type-system.md#r-types.literal.int-not-float)).

**Classes.** Unifying two literal variables merges them; a class with a
signed member is `SignedIntLit`. So all literals that meet each other
form one **class**. Unifying a class with a concrete type binds the whole
class. Range checks run on the literal constants once their types are
resolved, at the end of the body.

**Defaulting.** At the end of each statement, every class made in that
statement and still open takes its default: `i32` for a signed class,
`usize` otherwise, `f64` for floats
([`types.literal.local.default`](../../spec/lang/04-type-system.md#r-types.literal.local.default)).
A class that takes `usize` is remembered (`fell_back`) for
`flow.panic.report.fallback`.

**Joins that cross statements.** The return paths of one closure or
function with an omitted result, and the `break` values of one loop,
join although they sit in different statements
([`types.literal.local.form.closure-return.statements`](../../spec/lang/04-type-system.md#r-types.literal.local.form.closure-return.statements)).
So `return 0` early in a closure whose last value is an `i64` gives `0`
the type `i64`. A class whose root is an open result variable is skipped
by statement defaulting, and a `break` statement closes no literal; the
join binds them, and one still open after the join takes its default.
The design also holds a class nested in the operand's type, as in
`return [0]`
([`types.literal.local.form.join-open`](../../spec/lang/04-type-system.md#r-types.literal.local.form.join-open)).
Implementation differs: only a class that is the result variable itself
is held; not judged whether nested classes reach the join.

**Where a literal meets a type without unifying.**

| Form | Rule | Mechanism |
| --- | --- | --- |
| converted to `Any` or a trait value, `let x: Any = 42` | the group default ([`types.literal.local.erased`](../../spec/lang/04-type-system.md#r-types.literal.local.erased)) | the coercion defaults the class first, then wraps |
| several instantiations fit only by width, `price.add(5)` | the default ([`types.literal.local.instantiation`](../../spec/lang/04-type-system.md#r-types.literal.local.instantiation)) | the instantiation choice defaults and re-chooses (section 2.5) |
| a waiting call's result, `x.pick() + 1` | the class waits for the call's impl | the statement's other classes default first, then the waiting goal settles (`close_literals_but`) |
| a method on an open literal receiver, `5.max(n)` | a type only an impl names is not met | the receiver takes its defaults before the goal is solved again |

**No statement retry.** A literal's width is decided once per connected
literal class, by union-find, not by re-checking a failed statement. In
`(pick(1, big), pick(2, small))` with `big: i64` and `small: i32`, `1` is
`i64` and `2` is `i32`.

## 4. Coercions And Joins

### 4.1 Coercion Sites And Order

`coerce` runs where the spec allows an implicit conversion: assignments,
bindings with a written type, arguments, return values, field
initializers, collection elements with an expected element type, and
branch values with an expected type
([`types.option.wrap.sites`](../../spec/lang/04-type-system.md#r-types.option.wrap.sites),
[`req.row.subsume.sites`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume.sites)).
It tries the rules of
[Assignability And Coercion](../../spec/lang/04-type-system.md#assignability-and-coercion)
in a fixed order, and at most one applies:

1. `found` or `want` is poison: succeed, no node.
2. `found` is `never`: a `Never` coercion
   ([`types.assign.never`](../../spec/lang/04-type-system.md#r-types.assign.never)).
3. Identical, or unifiable (variables bind): no node.
4. `mut T` to `T`: `Weaken`.
5. A declared variance conversion on a readonly outer type: `Variance`.
6. `T` to `T?`, one layer: `WrapSome`
   ([`types.assign.optional`](../../spec/lang/04-type-system.md#r-types.assign.optional)).
7. A function type whose row the target row entails: `RowSubsume`.
8. `S` to a trait value `Tr` when `S: Tr` holds: `ToTraitValue`, or
   `ToAny` for `Any`; `mut S` to `mut Tr` likewise
   ([`types.assign.trait-value.mut`](../../spec/lang/04-type-system.md#r-types.assign.trait-value.mut)).
9. A child trait value to a supertrait value: `Supertrait`. `&S`, or a
   value of `S`, to a trait value reference `&dyn Tr` when `S: Tr` holds:
   a new `Ref[dyn Tr]` whose `get` applies `RefToTraitValue` to the
   source reference (`dynref.rs`,
   [`types.assign.trait-value.ref`](../../spec/lang/04-type-system.md#r-types.assign.trait-value.ref)).
10. A suspending function type to its constructor form: `SuspendFnToCtor`
    ([`req.suspend.type.conversion`](../../spec/lang/11-requirements-and-suspension.md#r-req.suspend.type.conversion)).
11. Otherwise an error: `mutable-upgrade` or
    `readonly-argument-to-mutable-parameter` for a readonly value where
    `mut` is wanted; `implicit-narrowing` for a narrower numeric type of
    the same family; `mixed-signedness` for an operand across
    signedness; `variance-representation-change` when variance would
    change an element's representation; otherwise `type-mismatch`.

Rule 3 comes before the others, so a variable never binds through a
conversion. In `let x: i32? = id(5)`, `id`'s `T` is solved from the
expected type `i32?` first (section 2.4 step 2).

### 4.2 Coercion Instructions

**Rule TC-6. Every implicit conversion is an explicit TIR `Coerce`.**
Collection never re-derives one.

| `Coercion` | From → to | Run-time meaning |
| --- | --- | --- |
| `Never` | `never` → any | unreachable |
| `Weaken` | `mut T` → `T` | none: a static view change |
| `Variance` | `C[A]` → `C[B]` by declared variance | none, by [Representation-Preserving Variance](../../spec/lang/04-type-system.md#representation-preserving-variance) |
| `WrapSome` | `T` → `T?` | build `.Some` |
| `RowSubsume` | `fn ... $ R1` → `fn ... $ R2` | none: a context holding `R2`'s keys serves `R1` ([crates/hd_mono/DESIGN.md §12.4](../hd_mono/DESIGN.md#124-rows-and-providers)) |
| `ToTraitValue`, `ToAny` | `S` → `Tr`, `mut S` → `mut Tr`, `S` → `Any` | box with its dispatch table; the impl choice is in the record |
| `Supertrait` | child trait value → parent trait value | load the parent's vtable from the child's |
| `SuspendFnToCtor` | `fn!` type → constructor type | none, or a thin adapter |
| `Identity` | `T` → `T` | none: a least-common-type slot whose value needed no conversion (section 4.3) |
| `RefToTraitValue` | `&S` → `Tr`, inside the `get` of a trait value reference (`dynref.rs`) | box the reference itself; the vtable reads through it or passes it per slot |

Variance markers are declared, not inferred (`hd_resolve/src/variance.rs`).

The error conversion of `?` is not a coercion: it is a `Call` of the
resolved `From` method on the failing path (section 4.4). Literal widths
and numeric casts are not coercions either.

### 4.3 Least Common Type

The sites are list and map literals, value-producing `if`, `match` and
loops, and omitted closure and private results
([`types.lct.sites`](../../spec/lang/04-type-system.md#r-types.lct.sites)).
With an expected type, the site does not join: each value is checked
against it. Without one, the site gets a join variable (`join.rs`), and
its values are checked in order:

1. **Drop `never`.** A `never` value takes no part
   ([`types.lct.never.dropped`](../../spec/lang/04-type-system.md#r-types.lct.never.dropped)).
2. **Contextual variants refuse.** A `.None` or other contextual variant
   has no type to offer: `missing-contextual-enum-type`.
3. **Rows.** Function values take the union of their rows
   ([`types.lct.row-union-every-site`](../../spec/lang/04-type-system.md#r-types.lct.row-union-every-site)).
4. **Join.** The first value solves the variable; each later value meets
   the type so far by permission weakening, declared readonly variance,
   or optional injection of one layer, and only those
   ([`types.lct.no-combine`](../../spec/lang/04-type-system.md#r-types.lct.no-combine)).
   It never uses `Any`, a trait value or a supertrait.
5. **Errors.** A value that does not join reports `no-common-type`, or
   `no-least-common-type` when a common type exists that these rules
   cannot reach, at that value, once per site.
6. **Coerce.** Each value then gets the coercion to the joined type. A
   slot is reserved after each value, and the site fills it once the type
   is final (`Ck::fill_join`): `WrapSome` for a value one optional layer
   short, else `Weaken`, `RowSubsume` or `Variance` for a value whose type
   still differs, else `Identity`. An optional value after plain ones
   widens the site in place, so each value is checked once, and a
   contextual variant before the widening value stays without a type
   (`types.lct.contextual`). A list spread passes its list unchanged, so
   its elements take only a conversion with no run-time meaning.
7. **Close.** Once its slots are filled, the site closes: its variable
   is an ordinary type from then on, so a later value of that type, such
   as one assigned to a binding the site initialized, takes the
   conversion any other type gives it and reserves no slot.

The site's variable takes a first value whose type is still a variable,
such as a literal's, as an alias (`Unifier::alias`), not a union, so
widening the site does not retype that value.

### 4.4 Propagation

For `e?` on `Result[T, E]` in a function returning `Result[U, F]`:

1. `E` reaches `F` by one coercion of section 4.1, or else by one
   `From[E]` impl of `F`, never both and never chained
   ([`types.result.propagate.one-step`](../../spec/lang/04-type-system.md#r-types.result.propagate.one-step)).
2. The checker emits the desugared form: a switch on the result; the
   failing path converts the error and returns; the other yields the
   success value.
3. On an optional, the enclosing result must be optional too, and the
   exit returns `.None`.
4. In a test body, `?` follows
   [Propagation In Test Blocks](../../spec/lang/05-expressions.md#propagation-in-test-blocks).

## 5. Rows And Suspension

### 5.1 Representation

A row is an interned `RowId` over `RowData { keys: Vec<Ty>, params:
Vec<RowParamRef> }`: sorted concrete keys plus the row parameters it
names. Entailment is membership after alias expansion
([Entailment](../../spec/lang/11-requirements-and-suspension.md#entailment)).

### 5.2 Available Keys, Providers And `$.use`

Each body keeps a stack of `RowFrame`s: the body's declared row at the
bottom (`Declared`, or `Any` for an entry module's top level), then
closures (`Closure`), `$.with` blocks (`With`), an omitted private row
(`Inferred`) or a test case's runner keys (`Profile`). The available keys
are their union
([`req.row.entail.available`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.entail.available)).

- `$.with K=p: body`: check `p` against `K`. For a mutable requirement
  trait, `p` must be `mut`, else `mutable-upgrade`. Check generic key
  collisions among the new keys and the visible ones
  (`generic-requirement-key-collision`) by unifying the two keys inside a
  trial, with rigid parameters replaced by fresh variables. Then push.
- `$.use(K)`: `K` must be available, else `missing-requirement`.
- An omitted private row and a closure without a `$` clause record each
  key used outside their own `$.with` blocks in their frame.

### 5.3 Call Checking

For a call to a callee whose substituted row is `R`:

| `R` holds | Check |
| --- | --- |
| concrete keys | each key available, else `missing-requirement` naming the key |
| a row parameter `$R` of the callee | `R` was solved from an argument's function type (least solution, section 5.6), then as above |
| the caller's own row parameter | entailed only if the caller's row lists it |
| the row of a private callee still open | an edge for the recursive group's solve, or under a written row a deferred check (section 5.5) |

A function value checked against an expected function type uses row
subsumption: the expected row must entail every key of the value's row
([`req.row.subsume`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume)).

### 5.4 Closure Rows

A closure without a `$` clause gets the least row that contains every
key its body uses outside its own `$.with` blocks. A `$.with` around the
closure never satisfies its keys
([`req.row.omitted.outer-scope`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.outer-scope)).
With an expected row parameter, the inferred row unifies with it
([`req.row.omitted.expected-parameter`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.expected-parameter)).

**A closure owns its row.** Creating a closure value runs none of its
body, so the closure's requirements belong to its function type, never to
the body that creates it. The creator takes part only where it invokes a
function value. A closure that calls a private callee needs that
callee's row now, so the callee's body is checked first (`results::row_now`).

### 5.5 Private Rows And The M3 Fixpoint

A non-public function or inherent method without a `$` clause has an
inferred row
([`req.row.omitted.inferred-private`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.inferred-private)).
As built, rows are solved during M1, before the module's items are
written, with no pending rows in types or TIR:
[crates/hd_tir/DESIGN.md §4.13.1 "M3: inferred rows, as built"](../hd_tir/DESIGN.md#4131-task-structure-per-module)
is the reference. In short:

1. The body's `Inferred` frame collects its own keys. A call of a
   callee whose row is open records an edge: the callee, the call's type
   arguments, and the keys of the `$.with` blocks around the call.
2. When no edge reaches an unchecked callable, the ready rows are solved
   together: `row(f) = uses(f) ∪ subst(row(g)) − minus` per edge, until
   nothing grows. That is the least solution
   ([`req.row.omitted.cycle`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.cycle)).
3. A key nested past the instantiation depth limit (32) is
   `instantiation-too-deep` at the call.
4. Deferred entailment checks under written rows run once their callees
   are solved: `missing-requirement` at the call.
5. Codegen picks providers from the callee's signature row, so no TIR
   is patched afterwards.

This is better than the earlier design's pending rows and provider
patching: no type or call is rewritten after its body is checked, and
the solve still never rechecks a body (TC-4). Its one gap: a closure or
function value of a group member still being checked is a structured
"not implemented".

### 5.6 Row Patterns And Least Solutions

Matching a parameter's row pattern `$ R + K` against an argument's row
`S` solves `R` as the least row: `S` without the pattern's concrete keys
([Least Row Solutions](../../spec/lang/11-requirements-and-suspension.md#least-row-solutions)).
Patterns are solved one at a time, in the spec's order. A pattern with
two unknown row parameters is a header error that resolution already
reported (`ambiguous-row-pattern`).

### 5.7 Suspension

- `!` is part of a name, so suspension needs no inference
  ([`fn.decl.suspension-not-inferred`](../../spec/lang/07-functions.md#r-fn.decl.suspension-not-inferred)).
- A bang call needs a driver context: a suspending function or closure
  body, or a test body (the `suspends` stack). Elsewhere it is
  `bang-call-outside-suspension`.
- A bang call whose callee does not suspend is `not-suspending`.
- A plain call of a suspending callee has type `mut Suspend[T]`, a
  must-use type.
- `all!` and `race!` are intrinsics typed by
  [Typing `all!`](../../spec/lang/11-requirements-and-suspension.md#typing-all).
  Each argument must be a cold `mut Suspend[T]`.
- Each bang call, `s!()`, `all!` and `race!` is emitted as `Await`,
  `AwaitValue`, `AwaitAll` or `AwaitRace`. Frames, polling and
  cancellation paths are codegen's.

### 5.8 The Direct `block_on` And `println` Ban

The ban is **direct-only**. A call whose resolved callee is
`std.task.block_on` or `println`, written directly (not in a nested
closure) in one of these contexts, is `suspension-forbidden-context`:

- a `defer` suite;
- a default expression;
- a fact or metadata expression;
- the initialization of a module that is not an entry module, for
  `block_on`; a `println` there is `missing-requirement` instead.

A bang call in a `defer` suite is `suspension-forbidden-context` too
([`flow.defer.suspend`](../../spec/lang/06-control-flow.md#r-flow.defer.suspend)).
The check reads the resolved callee at the call (`check_forbidden_call`).
It never looks into another body, so it needs no summary. An indirect
call reaches the run-time check codegen emits.

## 6. GADT Refinement And Tuples

### 6.1 Arm-Local Equalities

None: hd has no pattern refinement and no existential variant
parameters. A `match` arm is checked like any other block.

### 6.2 Tuples, Varargs And Spreads

There are no variadic generics
([chapter 12](../../spec/lang/12-variadic-generics.md)).

- A vararg `args...: Args` with `Args < Tuple` takes the call's remaining
  arguments as one tuple. `Args` unifies with the tuple of their types.
- A spread `f(args...)` unifies the spread tuple with the remaining
  parameters.
- A tuple with a rest element, `(A, B, List[T]...)`, uses the `rest`
  field of `TyData::Tuple`. A spread pattern binds the rest as `List[T]`.
- `Tuple` is compiler-supplied at every arity: `Evidence::Builtin`.
- Tuple `Eq`, `Ord`, `Hash` and `Debug` come from std's tuple templates,
  so the solver answers them with `Evidence::Impl` of the template at the
  tuple type, as for any impl.
- `Fn[Args, O, $ R]` unifies `Args` with a function type's inputs tuple
  (`Inputs::Tuple`).

## 7. Patterns And Exhaustiveness

### 7.1 Pattern Typing

Patterns are checked against the scrutinee's type with no coercion:

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
over the pattern matrix (`pat.rs`, `useful`), as rustc and OCaml do.

1. Rows are the arms in source order. A guarded arm adds no coverage
   ([`flow.match.guard.coverage`](../../spec/lang/06-control-flow.md#r-flow.match.guard.coverage)),
   but its own usefulness is still checked.
2. The constructors of a type: the enum's variants; `true` and `false`;
   one tuple or data constructor; `()`; and for integers, disjoint
   ranges split from the literals and ranges present. Strings and chars
   are infinite, so only a catch-all covers them.
3. An arm that is not useful is `unreachable-match-arm`, an error
   ([`flow.match.unreachable`](../../spec/lang/06-control-flow.md#r-flow.match.unreachable)).
4. If the wildcard row is still useful after every arm, the match is
   `nonexhaustive-match`, naming the top-level constructors no arm
   covers.

**Decisions.** `decide` compiles the matrix into a decision tree, so a
shared arm appears once and no pattern is tested twice on a path. The
first row's first column with a test decides. A variant column is a
chain of `SwitchTag`s, one per variant the rows name, each over the
rows that variant specializes to; its default holds the rows with no test there,
or is `Unreachable` when every variant is named. A literal or range
column is an `If` on the first row's test, and each side keeps only the
rows that test leaves possible. Tuple and data columns expand into one
column per member that a row reads. A row with no test left is a leaf:
it sets its bindings, then its guard chooses its arm or the rows below
it, in source order (`flow.match.guard.*`). A row with no test in a
switched column repeats under each outcome, as in any decision tree; a
nested-enum match stays linear in TIR size.

Planned, not built: a one-column fast path (a hash set over distinct
literals or variants) for large flat matches; a `match-too-complex` limit
when usefulness runs out of fuel; a witness deeper than the top-level
constructor.

### 7.3 `let` Patterns And `let-else`

- `let p = e` with a refutable `p` and no `else` is
  `refutable-let-pattern`.
- An `else` after an irrefutable pattern is `unreachable-match-arm`.
- The `else` block must diverge: its type must be `never`. Otherwise it
  is `let-else-falls-through`
  ([`flow.let.else.diverge`](../../spec/lang/06-control-flow.md#r-flow.let.else.diverge)).
- Refutability is one usefulness question: is the wildcard useful after
  the single row `p`?

## 8. Mutability, Access And Initialization

### 8.1 Access Types And Bindings

Every expression's type carries its access, `mut T` or `T`. Access is
computed one path step at a time (`access.rs`;
[Mutable Paths](../../spec/lang/04-type-system.md#mutable-paths)):

| Step | Access of the result |
| --- | --- |
| binding, parameter, `self` | its declared or inferred type |
| call | the declared result type |
| field read `e.f` | by the field's kind and `e`'s access; a direct `mut U` field through a readonly `e` gives `U` |
| index, iteration, lookup on a built-in collection | the declared element type, whatever the collection's access |
| unwrapping, tuple element, generic payload | the declared contents |

Data, copy-update, stored variant, list and map expressions are fresh:
they produce `mut T` for the new outer object, except when a readonly
value sits in a direct `mut U` field. A newtype carries its base value's
permission. The binding forms then apply
([Binding Forms](../../spec/lang/04-type-system.md#binding-forms)):

| Form | Type of the name |
| --- | --- |
| `x := e` | the readonly view of `e`'s type |
| `let x = e` | the readonly view; reassignable |
| `let mut x = e` | `e`'s type, which must be `mut T`, else `mutable-upgrade`; `mut-on-primitive` and `mut-on-tuple` take precedence |
| `let x: mut T = e` | `mut T`; `e` is checked against it |
| `let mut x: mut T = e` | as above, plus `redundant-let-mut` |
| `let mut x: T = e` | `let-mut-readonly-type` |

### 8.2 Mutation Checks

A mutation needs mutable access on exactly one expression, the one it
acts on ([Mutation Checks](../../spec/lang/04-type-system.md#mutation-checks)).
The checker checks it at the mutation, from the access type it already
computed:

| Mutation | Error when the target is readonly |
| --- | --- |
| `mut self` method call, a loop over an iterator, driving a suspension | `mutable-receiver-required` |
| store through a readonly edge `field: U` | `readonly-edge` |
| store through any other readonly root | `readonly-root` |
| argument to a `mut T` parameter or a `T < mut Trait` bound | `readonly-argument-to-mutable-parameter` |
| reassigning a `:=` binding | `non-reassignable-binding` |

Nested permissions convert only by declared variance, else
`type-mismatch`.

### 8.3 Local Flags

Each local has a flags byte in the builder's local table
(`hd_tir::ir::local_flags`): `READ`, `ASSIGNED`, `MUTATED`, `CAPTURED`,
`CAPTURED_ASSIGNED`, plus `PARAM`, `SHORT` (a `:=` binding) and
`NOT_YET` (a later parameter seen from a default). With no extra pass
they give:

- `unused-local-binding` at the end of the body;
- `non-reassignable-binding`, and `fn.default.later-parameter`;
- capture modes for TIR's closures: `Copy` when no side writes the local
  after the capture, `Move` when only the closure uses it afterward,
  `Shared` when both may
  ([`fn.capture.cell`](../../spec/lang/07-functions.md#r-fn.capture.cell)).
  The mode depends on statements after the closure, so the checker
  passes the final modes to `TirBuilder::finish`.

### 8.4 Definite Initialization

**Locals: during checking, not over TIR.** A `:=` binding can sit inside
an expression and binds into the enclosing scope, so a name can be in
scope on a path that skipped its initializer
([Definite Initialization](../../spec/lang/03-names-and-scopes.md#definite-initialization)).
hd's control flow is structured, so the checker computes definite
assignment while it checks, syntax-directed as Java's rules are. It keeps
the set of `:=` bindings a path may have skipped (`uninit`), and for a
`bool` condition what it leaves on its true and false paths
(`init::CondFlow`):

- `a && b`: `b` starts from `a`'s true path; `||` is the mirror, and `!`
  swaps the two.
- `if`: the then-branch starts from the condition's true path, the
  else-branch from its false path. After the `if`: the union of what the
  branches that complete normally may skip, since a diverging branch is
  not an incoming path
  ([`names.definite.diverging`](../../spec/lang/03-names-and-scopes.md#r-names.definite.diverging)).
- `while`, `match`: likewise over the loop's exits and the arms.
- A read of a local that may be skipped is
  `possibly-uninitialized-binding`.

So `if flag && (name := load()) != "": greet(name)` is accepted, and a
use of `name` after the `if` is rejected.

**Top-level bindings.** A module's top-level statements are one `Init`
body, checked before its functions. Each body records init facts: the
top-level bindings it reads and the same-module functions it calls.
Definite initialization of top-level bindings combines them over the call
graph and reports `top-level-read-before-initialization`
([`module.init.definite`](../../spec/lang/10-modules.md#r-module.init.definite)).
Across a module cycle, `InitOrder(F)` orders statements from the
summaries (`stages::cycle_order`;
[§4.13.10](../hd_tir/DESIGN.md#41310-module-initialization)).

## 9. Omitted Result Types (M1)

### 9.1 Who Can Omit

A non-public function and a non-public inherent method may omit `-> T`
([`fn.decl.result-omitted-private`](../../spec/lang/07-functions.md#r-fn.decl.result-omitted-private)).
Public functions, trait methods and impl methods cannot
(`missing-result-type`, a header error). A local `fn` is a closure
(section 2.6).

**Private items a template names.** A derive template may call private
helpers of its trait's module, which the interface exports as hidden
items. A dependent needs each helper's signature from the interface, so a
private function that a template body names follows the public signature
rules ([`module.package.annotated`](../../spec/lang/10-modules.md#r-module.package.annotated)):
it writes its result type, else `missing-result-type`
(`results.rs`). With that rule, an omitted result never reaches a folder
interface.

### 9.2 The M1 Walk

`results.rs` keeps a registry of the module's private callables with an
omitted result or row, in source order:

```text
omitted_results: register each such callable as Pending
check the top-level statements                (the Init body)
infer_results:   for each registered callable, in source order: seen(f)

seen(f):
    Done     -> its result and row
    Active   -> a cycle if its result is omitted: report once, result Poison
    Pending  -> if its result is omitted, or the caller forces it:
                Active; check its body (final by TC-4); Done; settle rows
```

- A reference is a call or a use as a value, since both need the type.
- The top-level statements go first, because unannotated top-level
  bindings get their types there and other bodies read them.
- A cycle is `recursive-function-needs-result-type`, reported once at
  the cycle's member that comes first in source order
  ([`fn.decl.omitted-cycle.report`](../../spec/lang/07-functions.md#r-fn.decl.omitted-cycle.report)).
- The walk is serial within one module; different modules run theirs in
  parallel.
- The design keeps an explicit stack of open bodies, so a chain of
  10,000 private functions cannot overflow the native stack.
  Implementation differs: before the sweep, `acyclic_callees_first`
  orders the callables whose bodies reach no cycle callees first, with
  an explicit stack, and checks them in that order, so each call finds
  its callee done. A body's references are its identifier tokens that
  name a registered callable. A callable that reaches a cycle keeps its
  source-order place, and `seen` still reaches its callees through
  `check_fn_body`, so a long chain that ends in a cycle, or one the
  top-level statements enter, still recurses natively.

### 9.3 Cost

M1 does each such body's work once, as M2 would have. The cost is lost
parallelism inside one module: a module made only of private functions
with omitted results checks serially. The remedy would be a lint that
suggests result types, not a design change.

## 10. Error Recovery And Diagnostics

### 10.1 Errors Are Values

**Rule TC-7. No unwinding for user errors.** A failed check reports,
returns a poisoned result, and checking goes on. `StageResult`'s error is
reserved for constructs the compiler does not carry yet ("not
implemented"), which stop the body. A Rust panic in the checker is a
compiler bug, caught at the task boundary
([§6.4](../hd_sched/DESIGN.md#64-budgets-cancellation-and-the-memory-cap)).

### 10.2 Poison

The poison type follows [§3.6](../../docs/compiler/data-structures.md#36-the-poison-type):

| Source of poison | Reported where |
| --- | --- |
| unknown name, failed `use` | at the name or the `use` |
| broken header (own module or a dependency) | once, at the header, in its module |
| syntax error node | by the parser |
| a failed check (mismatch, missing method) | at that check; the expression's type becomes poison |
| a cycle in M1 | once per cycle |

Propagation rules:

1. `unify` with poison on either side succeeds and binds nothing.
2. `coerce` from or to poison succeeds with no node.
3. A goal whose self type holds poison answers `Holds { evidence: Poison }`
   without searching.
4. Method lookup on a poison receiver gives a poison callee, and its
   arguments are still checked, so errors inside them still report.
5. At the end of the body, an unsolved variable is not reported when the
   body read a poisoned name or the expression was checked against a
   poisoned expected type (`read_poison_name`, `absorbed`).
6. TIR gets a poison value or `Hole`. Both occur only in a module with
   errors, whose TIR is not kept.

### 10.3 One Diagnostic Per Root Cause

1. **Cascade cut.** After a failed check, the expression's type is
   poison, so a parent expression cannot report about it.
2. **Arguments of a failed call** are still checked, against the
   parameter types if the callee is known, else against poison.
3. **Once per site.** A join site reports once; a default's
   `requirement-in-default` reports once.

Planned, not built: a root key per diagnostic (the body plus a name, a
local, a variable's root or a header item), so that ten uses of a
mistyped `x` give one error; a blame span on each variable's root, so a
mismatch names the unification that decided it.

### 10.4 Typed Holes

`_` as an expression outside a pipe step is `placeholder-outside-pipe`
([`expr.pipe.placeholder-outside`](../../spec/lang/05-expressions.md#r-expr.pipe.placeholder-outside)),
and the hole's type is poison.

Planned, not built: a note with the expected type and up to five locals,
parameters and functions in scope whose type fits, each found by a
unification trial charged to fuel.

### 10.5 Fix-Its For Common Mistakes

The writing log (`docs/audit/hd-writing-log.md`) gives the checker's most
frequent codes: `type-mismatch`, `mixed-signedness`,
`unsatisfied-trait-bound`, `unknown-method`, `unknown-name`,
`missing-requirement`, `missing-contextual-enum-type`,
`mutable-receiver-required`, `cannot-infer-type`, `mutable-upgrade` and
`mut-on-primitive`.

Built: the checker's one fix-it is `dyn` before a bare trait name in a
type position (`ty.rs`); messages name both types, and `unknown-method`
names an unavailable trait that has the method.

Planned, not built:

| Code | Message and fix-it |
| --- | --- |
| `type-mismatch` | "expected `i64` (from this argument), found `i32`"; a cast fix-it `i64(x)` for a numeric pair ([`types.num.no-implicit.fix`](../../spec/lang/04-type-system.md#r-types.num.no-implicit.fix)) |
| `mixed-signedness` | when one side is a defaulted literal, point at the literal: "`index` is `usize` because `0` had no type; write `+0` for `i32`" ([`types.literal.local.hint`](../../spec/lang/04-type-system.md#r-types.literal.local.hint)) |
| `unsatisfied-trait-bound` | names the bound and where it comes from |
| `unknown-method`, `unknown-name` | did-you-mean by edit distance; a `use` fix-it |
| `missing-requirement` | a fix-it adding the key to the caller's header row |
| `mutable-receiver-required`, `readonly-root` | the `let mut` fix-it on the local's declaration |
| `cannot-infer-type` | an annotation fix-it, in the `mut` form when the binding was mutated: `let names: mut List[string] = []` |
| `mutable-upgrade` | "make a fresh copy: `User { ...user }`" |
| `mut-on-primitive` | a fix-it removing `mut` |

**Suggestions search only keyed inputs.** A did-you-mean or `use`
fix-it may search only what the module's `check` key covers: its own
syntax, the folders in its dependency closure, std and the prelude
([crates/hd_cache/DESIGN.md §5.3](../hd_cache/DESIGN.md#53-key-composition)).
A cached diagnostic never names an item whose change would not
invalidate it.

**Message quality rules.**

1. **No internal names.** Types render through stable paths with the
   shortest unambiguous name.
2. **The right file.** A problem in std code is reported in the std
   file, or not at all, never at the user's `use` line.
3. **One hint line.** The compact form keeps at most one `hint:` line.
4. **Fix-its are exact edits.** Each fix-it passes the re-parse check
   before it is marked `Exact`.

## 11. Limits And Budgets

### 11.1 What Counts

Every count is a language-level unit, never time or allocation. The
body's `Fuel` (`hd_base::Fuel::BODY_DEFAULT`, 2^24 steps) counts:

| Step | Cost |
| --- | --- |
| an expression node checked | 1 |
| a statement checked | 1 |
| a match decision step | 1 |
| a goal the body meets for the first time | 1 plus heads matched for each node of its proof DAG that the body has not met yet (rule TS-5) |
| a goal the body has met before | 1 |

Trials are charged through the nodes they check, whether or not they
commit. Fuel is per body, so one item's complexity never fails another.

**Rule TC-8. Fuel used is a pure function of the body and its frozen
inputs.** A goal's charge depends only on the goal and on whether this
body met it before, never on which body filled the memo. So a body
passes or fails its budget the same way on any thread count, cache state
or check order.

### 11.2 Running Out

When fuel runs out inside a body, the design reports `item-too-complex`
on the item, once, keeps the diagnostics reported before, poisons the
body and its M1 result, and replaces its TIR with a single poison value.

Implementation: running out, in `Ck::charge` or as the solver's
`OutOfFuel`, stops the check with `out_of_fuel`, and each body's entry
turns it into `item-too-complex` (`too_complex`) on its item:

| Body | Entry | Reported on |
| --- | --- | --- |
| a function body, M1 included | `check_fn_body` | the function |
| a default | `check_default` | the default expression |
| a module's `Init` | `check_init` | the module; an unsettled or unreached top-level binding is poison |
| a test case or registration | `check_tests` | its registration call; the error fails the test run |
| a fact list | `check_fact_list`, `check_decl_facts` | the decorated item, once |

A header goal (stage B, `header::ItemCheck::fails`) that runs out of
its item's fuel is one `item-too-complex` on the item's header, and the
item's later goals are not asked; never a proved goal. A local impl's
supertrait check runs out as part of its body.

`BodyCx::fuel` is the budget, and `HeaderCx::fuel` the header's; a driver
test sets a small one for both through `Packages::body_fuel`, which is
part of the check key and the header-check key.
Implementation differs: the generated bodies (`@error`, delegation
forwarders) still stop with a structured "not implemented".

Other limits:

| Limit | Value | Checked at | Diagnostic |
| --- | --- | --- | --- |
| trait resolution depth | 64 | solver | `trait-resolution-depth` |
| nesting depth | 256 | parser; the checker relies on it | `nesting-too-deep` |
| instantiation depth of row keys | 32 | M1 row solve | `instantiation-too-deep` |
| growing generic call cycle | none: any growth on a cycle | after a module's bodies (`inst_cycles`) | `instantiation-too-deep` |

Planned, not built: a type size limit at interning (`type-too-large`)
and `match-too-complex`.

### 11.3 Pathological Cases

| Case | Danger | Design answer |
| --- | --- | --- |
| a method chain of 10,000 calls, `a.b().c()...` | the syntax tree is 10,000 deep on its left spine | planned, not built: walk a postfix chain into a scratch stack and check it bottom-up in a loop; likewise left-deep operator chains |
| deep nesting of brackets, blocks and closures | stack overflow | the nesting limit of 256 bounds native recursion |
| a list literal of 100,000 elements | quadratic LCT or literal grouping | the join compares a value only when the site's type changes, a bounded number of times per site; literal classes are union-find |
| a 10,000-arm literal match | quadratic usefulness | planned one-column fast path (section 7.2) |
| wide tuple patterns with nested enums | exponential usefulness | fuel |
| nested instantiation choices, `a.add(b.add(c.add(...)))` | trials multiply | the head prefilter; arguments inferred once outside trials; the trial memo (#49); fuel |
| many pending goals | quadratic retries | a statement settles only its own goals |
| huge rows | quadratic set operations | sorted key lists |
| an ill-typed version of each of the above | Swift's slowest cases are errors | poison stops cascades |

## 12. Determinism And Parallelism

**Rule TC-9. No shared mutable checker state.** A module's bodies share
one `BodyCx`, owned by the `Body(m)` task. A body writes only:

- its own `Ck` and local pool, and the `BodyCx`'s per-module cells
  (M1 registry, init facts, debug references);
- the append-only global interners, whose IDs never reach output;
- the solver's global memo, whose entries are pure functions of their
  keys, written first-writer-wins, and published only for completed,
  context-free goals (rule TS-4).

There are no statics, thread-locals or caches keyed by span.

**Content order.**

- Inference variables and locals are numbered per body from zero.
- `ModuleFinish` writes diagnostics in a stable order.
- The solver returns candidates in a fixed order (trait solver §11).
- M1's walk follows source order.
- Pending goals are settled in creation order.

## 13. Checker Data Structures

All per-body state lives in `Ck` (`body.rs`), created per body by
`new_ck`. The main parts:

| Field | Holds |
| --- | --- |
| `b: TirBuilder` | the body's TIR, including the local table and its flags |
| `local: &LocalPool` | the body's types that hold variables |
| `infer: InferTable` | `parent`, `rank`, `binding`, `kind` and the trail, one entry per variable |
| `env: ParamEnv`, `memo: BodyMemo`, `fuel` | the elaborated environment, the solver's body memo, the budget |
| `scopes`, `loops`, `subs`, `suspends`, `rets`, `rows` | the context stacks; each balanced across a trial |
| `pending` | stalled goals (section 2.7) |
| `joins` | LCT sites and their reserved slots (section 4.3) |
| `lit_nodes`, `overflow_lits`, `neg_nodes`, `fell_back` | literal bookkeeping (section 3.6) |
| `uninit`, `bound`, `cond_flow`, `facts` | definite initialization and init facts (section 8.4) |
| `open_params`, `ref_params`, `call_params`, `assoc_self` | variables closed at the end of the statement or the body (section 2.4) |

- **No expectation stack.** The expected type is an argument of `expr`.
- **Body-local types** live in the body-local pool. At body end the
  checker resolves every type in its outputs; the results are interned
  globally.

## 14. Testing

### 14.1 Spec Traceability

Each spec section maps to one checker module:

| Spec area | Module in `hd_check` |
| --- | --- |
| [Literal Types](../../spec/lang/04-type-system.md#literal-types), [Numeric Conversions](../../spec/lang/04-type-system.md#numeric-conversions) | `expr`, `body` (literal classes, defaulting, range checks), `literals` (literal functions) |
| [Assignability And Coercion](../../spec/lang/04-type-system.md#assignability-and-coercion), [Optional Types](../../spec/lang/04-type-system.md#optional-types) | `body` (`coerce`) |
| [Least Common Type](../../spec/lang/04-type-system.md#least-common-type) | `join` |
| [Composite Values And Access Permission](../../spec/lang/04-type-system.md#composite-values-and-access-permission) | `access` |
| [Generics](../../spec/lang/04-type-system.md#generics), [Type Inference Boundaries](../../spec/lang/04-type-system.md#type-inference-boundaries) | `call`, `trial`, `hd_types::unify` |
| [Expressions](../../spec/lang/05-expressions.md) | `expr`, `call`, `fnref`, `update` |
| [Control Flow](../../spec/lang/06-control-flow.md) | `body`, `pat`, `local` |
| [Definite Initialization](../../spec/lang/03-names-and-scopes.md#definite-initialization) | `init` |
| [Functions](../../spec/lang/07-functions.md) | `results`, `local`, `call` |
| [Data And Enums](../../spec/lang/08-data-and-enums.md) | `expr`, `update`, `newtype`, `promote` |
| [Traits](../../spec/lang/09-traits.md) (body side) | `call`, `trial`, `conform`, `header`, `delegate` |
| [Modules](../../spec/lang/10-modules.md) (initialization) | `init`, `stages` |
| [Requirements And Suspension](../../spec/lang/11-requirements-and-suspension.md) | `rows`, `results`, `call` |
| [Annotations](../../spec/lang/14-annotations.md) (templates, facts, `@error`) | `derive`, `structure`, `facts`, `error` |

### 14.2 Test Kinds

| Test | What it checks |
| --- | --- |
| conformance typing fixtures (`spec/conformance`) | accept or reject and the `code:` of each diagnostic, through the portable command contract; failures listed in `KNOWN_FAILURES.tsv` |
| driver tests (`crates/hd_driver/tests/all`) | one file per feature: diagnostics, and run results for programs that build |
| unit tests | `hd_types` unifier, pool and solver tests |
| determinism (`hd_testkit`, `generics_determinism`) | equal output across thread counts and orders |
| TIR verifier | `TirBuilder::finish` verifies every body |

Planned, not built: a rollback verifier that hashes body state at each
checkpoint and compares it at rollback (TC-5); an exactly-once counter
per body (TC-4); `fuel_used` per body in test mode, compared across
threads and memo states (TC-8); a pathological suite with each case of
section 11.3, well-typed and ill-typed; a one-root-cause fuzzer; a crash
fuzzer.

# New Compiler: The Trait Solver

Status: design record; may lag the code. The code and the spec win.
Checked against the code at 3209d690: notes marked "Implementation
differs" say where the code does less than this design.

This document details the trait solver of the new compiler. It implements
the interface that [crates/hd_check/DESIGN.md (type checking) §1.6](#16-the-trait-solver-interface)
fixes, and it refines these sections:

- [§4.12 Traits, Impls And Coherence](../hd_resolve/DESIGN.md#412-traits-impls-and-coherence):
  owner-folder impl tables, solving and coherence;
- [§4.10 Folder Interface Construction](../hd_resolve/DESIGN.md#410-folder-interface-construction):
  what the interface records about traits and impls;
- [§4.13.11 The Typed IR](../hd_tir/DESIGN.md#41311-the-typed-ir-tir):
  how a choice is recorded;
- [§13.2 Collection](../hd_mono/DESIGN.md#132-collection) and
  [§13.5 Dictionaries](../hd_mono/DESIGN.md#135-dictionaries-trait-values-and-gadt-evidence):
  how codegen consumes the choice.

## How To Read This

- **Rule TS-n** marks a design rule that a reviewer can check against the
  code.
- Rust code shows the shape of the real types; field lists are trimmed.
- The spec decides semantics.

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
3. **Codegen never searches.** At a concrete instance, the one matching
   head is the answer (rule TS-6).

What remains is real work: associated types, inference variables, the
instantiation choice of generic traits, budgets that do not depend on the
schedule, and messages that name the root cause.

## Prior Art In One Table

| System | What it does | What hd takes |
| --- | --- | --- |
| rustc's next solver ([dev guide: caching](https://rustc-dev-guide.rust-lang.org/solve/caching.html), [coinduction](https://rustc-dev-guide.rust-lang.org/solve/coinduction.html)) | canonical goals as cache keys; a search graph with cycle heads; provisional results and fixpoint iteration for coinductive cycles; inductive cycles answer overflow; global cache entries store the depth they reached | canonical goals; depth stored with each entry (as an intrinsic height, section 7.3); inductive cycles answer overflow. Not taken: the provisional cache and fixpoints, which hd's rules make unnecessary |
| chalk | a separate logic-programming solver (SLG tabling) shared by rust-analyzer; it drifted from rustc and was abandoned for the next solver | one solver, used by the checker, the header checks and codegen alike; no general logic engine |
| Swift's Requirement Machine ([forums](https://forums.swift.org/t/the-requirement-machine-a-new-generics-implementation-based-on-term-rewriting/55601)) | generic signatures as a rewrite system, completed by Knuth-Bendix | not needed: hd has no same-type requirements between parameters except associated-type bindings, which are oriented (projection to type). Elaboration is a linear walk (section 4.2) |
| Lean 4 tabled resolution ([paper](https://arxiv.org/pdf/2001.04301)) | tabling removes the exponential cost of diamond hierarchies and handles cycles | the lesson that memoization must remove diamonds; hd gets it from the memo alone, since there is no OR search |
| Haskell (GHC) | an instance is selected by its head alone; the context is checked after the commit, and there is no backtracking | the commit rule TS-2 |
| Scala 3 givens | search with priorities and a divergence check | nothing: hd has no priorities and no implicit search |
| Go | interface satisfaction by method sets; constraint satisfaction is memoized | memoize "type satisfies bound" globally |
| MoonBit ([methods and traits](https://docs.moonbitlang.com/en/stable/language/methods.html)) | explicit `impl Trait for Type`; an impl lives in the package of the trait or of the type | the fast path of section 3.3: a goal on a non-generic trait with a known self type is one table probe |

## 1. Scope

### 1.1 Who Owns What

The solver lives in `hd_types::solver`; other crates call it rather than
owning a second solver.

| Component | Owns | Does not own |
| --- | --- | --- |
| Resolution (`hd_resolve`) | impl heads, bound plans and derived heads in the interface; every header check that needs no solver: orphan, module ownership, targets, unconstrained parameters, sealed traits, per-member `dyn` availability, template placement, associated-type bindings; the overlap check (`Universe::overlaps`) | goals |
| **Trait solver (`hd_types::solver`, this document)** | `Implements`, `Instantiations` and the trait part of `Methods`; canonical goals and the memo; selection for codegen (`select`, `normalize_concrete`); `FailInfo` | inference variables (it reads them, never writes them), diagnostic text, spans |
| Candidate lookup (`hd_types::lookup`) | owner folders, unowned rows, the candidate directory, impl universes | answers |
| Header checks (`hd_check::header`, the `HeaderCheck(F)` task) | each impl's supertrait checks, newtype bases, delegation targets: goals under the item's environment ([crates/hd_resolve/DESIGN.md §4.10.1](../hd_resolve/DESIGN.md#4101-header-validation-stages)) | the answers |
| Type checker (`hd_check`) | when to ask, environment elaboration, normalization of projections, applying learned bindings, pending goals, the instantiation choice by trial, every diagnostic | impl search |
| Coherence (the `Coherence` task) | overlap across the program graph | goals |
| Codegen collection (`hd_mono`) | instances and vtables | anything but a selection at concrete types (rule TS-6) |

**Rule TS-1. The solver is a pure function of its memo key.** An answer
depends only on the canonical goal, the parameter environment, the impl
universe of the asking context (for a goal that may read the candidate
directory, section 3.2), the asking module's own impl rows, and the run's
frozen interfaces. Section 7.1 shows how each is covered. The solver
holds no mutable state except memo tables. It never reports a diagnostic,
never sees a span, and never writes an inference variable.

### 1.2 The Four Goals

| Goal | Asked for | hd example | Typical answer |
| --- | --- | --- | --- |
| `Implements` | each bound of an instantiated callee; `==`, `<`, `?` conversions; trait-value conversions; header checks | `fn show[T < Display](v: T)` called with a `User` asks `User: Display` | `Holds { Impl(impl Display for User) }` |
| `Project` | reserved; the checker normalizes (type checking §1.6) | | "not implemented" |
| `Instantiations` | parameters named only in bounds; the instantiation choice of a method; `X::from(v)` | `AppError::from(code)` asks `Instantiations { AppError, From }` | `Candidates([From[i32], From[string]])` in content order |
| `Methods` | the trait part of `value.name(...)` | `w.show()` on a `Wrapper` with an available `Show` | `Candidates([Show's candidate])`; inherent methods are the checker's |

The answer shapes:

| Answer | Meaning | Checker action |
| --- | --- | --- |
| `Holds { evidence, learned }` | proven; `learned` binds bare variables among the goal's trait arguments (section 3.8) | apply `learned` on its own trail; record `evidence` in TIR |
| `Candidates(c)` | candidates in content order (`Instantiations`, `Methods` only); each is a scheme with residual bounds (section 6.5) | the instantiation choice or method rules |
| `Fails(FailInfo)` | no proof exists, whatever the open variables become | report |
| `Stalled { on }` | the answer depends on these variables | keep a pending goal |
| `Overflow` | the proof needs a bound deeper than 64, or a cycle | `trait-resolution-depth` |
| `OutOfFuel` | the body's fuel ran out inside the solver | type checking §11.2 |

### 1.3 Entry Points

```rust
pub trait Solver: Sync {
    /// One goal in one context (body, header check or derive instance).
    fn solve(&self, cx: &mut SolveCx<'_>, goal: &Goal, fuel: &mut Fuel) -> StageResult<Answer>;
    /// Copies declared bounds into a `ParamEnv`; the checker's
    /// `body::add_bound` does the supertrait walk (section 4.2).
    fn elaborate(&self, bounds: &[DeclaredBound], out: &mut ParamEnvBuilder) -> EnvKey;
    /// Codegen's entry: the impl of a concrete trait reference (section 8.3).
    fn select(&self, cx: &mut SolveCx<'_>, tref: ConcreteTraitRef) -> StageResult<Selection>;
    /// Codegen's entry for associated types: every projection in `t` with a
    /// concrete base, by a head match.
    fn normalize_concrete(&self, pool: Types<'_>, impls: Impls<'_>, t: Ty) -> StageResult<Ty>;
}
```

`TableSolver` implements it. Not in scope: implied bounds
([Declared Bounds Are Not Implied](../../spec/lang/09-traits.md#declared-bounds-are-not-implied)),
structural conformance, and any goal at run time.

## 2. Goal Representation

### 2.1 Trait References

A trait reference is a trait, a self type and the trait's arguments.
Associated-type bindings travel beside it, keyed by the associated
item's `DefId`.

```rust
pub struct TraitRef { pub trait_: DefId, pub self_ty: Ty, pub args: TyList }
/// The pool's projection: the associated item and its trait reference, so
/// `<S as Add[i32]>::Out` and `<S as Add[i64]>::Out` are different types.
TyData::Assoc { assoc: DefId, trait_: DefId, self_ty: Ty, args: TyList }
```

- **Projections name the declaring item.** `I::Item` under
  `I < NamedSupplier` names `Supplier`'s `Item` item and the `Supplier`
  reference
  ([`trait.binding.name-reach`](../../spec/lang/09-traits.md#r-trait.binding.name-reach)).
- `self_ty` keeps its outer `Mut` only for the `mut_` check. Impl matching
  strips it, since one impl serves both views
  ([`trait.target.both-views`](../../spec/lang/09-traits.md#r-trait.target.both-views)).
- An inner `mut` stays: `Store[mut User]` and `Store[User]` are different
  trait references
  ([`trait.target.inner-mut`](../../spec/lang/09-traits.md#r-trait.target.inner-mut)).
- Types reach the solver with aliases expanded. Newtypes stay nominal.
- Trait argument defaults are filled before the goal is formed
  (`hd_resolve::fill_trait_args`), so `impl Add for Money` and the goal
  `Money: Add[Money]` agree
  ([`expr.op.trait.rhs-self`](../../spec/lang/05-expressions.md#r-expr.op.trait.rhs-self)).
  `Self::X` inside a trait keeps its trait arguments implicit; each use
  fills its own.

### 2.2 Canonical Goals

A goal is canonicalized before any memo lookup (`canonicalize`):

1. **Resolve.** The checker passes types with its known bindings
   applied; the solver reads only the types.
2. **Number the variables.** Inference variables are replaced by
   placeholders `Canon(0)`, `Canon(1)`, ... in order of first occurrence:
   self type first, then trait arguments left to right. `CanonVars` keeps
   each variable and its kind.
3. **Pick the key's scope.**

```rust
pub struct CanonGoal {
    pub kind: GoalKind, pub mut_: bool,
    pub n_vars: u8,          // placeholders; a goal with 255 or more is asked, never memoized
    pub trait_: DefId, pub self_ty: Ty, pub args: TyList,
    pub extra: u32,
}
```

`Canon(i)` is a global pool type, so a canonical goal holds no body-local
type. Two bodies asking `List[?3]: Eq` and `List[?9]: Eq` produce the same
key.

| Scope | When | Memo | Key part |
| --- | --- | --- | --- |
| `Global` | no placeholder, no parameter | the run's global memo | the canonical goal |
| `Env` | parameters, no placeholder | the run's global memo | the canonical goal and the item's `EnvKey` |
| `Body` | any placeholder | the body's memo | the canonical goal |

A goal that may read the candidate directory adds the context's
`ImplUniverseId` (section 3.2). `Methods` is not memoized; its
`Instantiations` goals are.

Placeholders keep their kind only in `CanonVars`, not in the key, so a
literal placeholder and a general one share a key; literal-kind
canonicalization is not built yet; see #40.

**Why parameters may go global.** With the `EnvKey` in the key, a goal
that mentions `T` is still a pure function of its key, and the members of
a generic impl, which share the impl's bounds, share their goals.

**Why the body memo needs no rollback.** A trial truncates the
body-local pool on rollback, so a key stored there would dangle.
Canonical keys hold only global types and placeholders, and an entry
stays valid after a rollback because it names no particular variable.

### 2.3 The Parameter Environment

```rust
pub struct ParamEnv {            // columns, built once per item
    pub key: Option<EnvKey>,     // interned by the run memo: equal environments, equal keys
    pub clause_self: Vec<Ty>,    // a Param
    pub clause_trait: Vec<DefId>,
    pub clause_args: Vec<TyList>,
    pub clause_bindings: Vec<Vec<(DefId, Ty)>>,
    pub clause_mut: Vec<bool>,
    pub clause_origin: Vec<u16>,
}
```

A clause's index is its position in the elaborated list. TIR's
`Bound { param, index }` choice uses it. The order is deterministic:
declared bounds in source order, each followed by its supertraits, depth
first, in declared order, each trait reference once.

## 3. Impl Selection

### 3.1 Where Candidates Come From

For a goal `S: Tr[A1..An]`, `Search::implements` tries these sources in
order:

| Order | Source | When it applies |
| --- | --- | --- |
| 0 | poison | `S` holds poison: `Holds { Poison }` |
| 1 | the parameter environment | `S` is a parameter with a clause on `Tr` |
| 2 | a trait value as self (section 9.3) | `S` is a trait value of `Tr` or of a subtrait |
| 3 | compiler-supplied rows (section 3.9) | `Tr` is a sealed, compiler-supplied trait |
| 4 | impl tables: the asking module's own table, the owner folders, the universe's unowned rows and, for an open argument, the candidate directory (section 3.2) | written, derived, delegated and generated impls, numeric families, tuple templates; block-local impls not visible here are dropped (`ImplView::hidden`) |

**Rule TS-2. Commit on the head; never backtrack among impls (after
GHC).** Overlap is decided from heads, and no two heads of one trait
unify. So when the goal's types are known, at most one impl head matches.
The solver commits to it. If one of its bounds then fails, the goal fails:
no other impl could have answered it. A goal with open variables that
more than one head could still answer stalls; it never tries each head's
bounds.

Order between sources matters only for the evidence recorded, never for
whether a goal holds. A goal `T: Add[T]` under `T < Num` holds through the
environment and through std's `impl[N < Num] Add for N`. The environment
wins, so the evidence is `Bound`. Codegen re-selects at the concrete type
(rule TS-6).

### 3.2 Owner Folders

An impl `impl Tr[Args] for Target` lives in a folder that declares `Tr`,
the outer constructor of `Target`, or the outer constructor of one of
`Args` ([Implementation Modules](../../spec/lang/09-traits.md#implementation-modules)).
Each folder's interface holds one impl table, and lookup reads the tables
of the owner folders
([§4.12.1](../hd_resolve/DESIGN.md#4121-owner-module-impl-tables)).
`Impls::candidates` finds the owners through an `OwnerMap` (module path
to folder) and reads the asking module's own table first.

**Rows no owner reaches.** `FolderImpls::new` splits each folder's rows
once per run:

- **unowned rows**: the folder owns neither the trait, the target nor any
  argument. These are std's impls for built-in targets kept in another
  std folder (the synthetic `STD_BUILTIN_TABLE`) and impls the ownership
  check rejected (`nonlocal-impl`, `orphan-impl`), which stay visible so
  their uses add no second error;
- **`arg_impls`**: the folder owns the impl only through a trait
  argument.

**The gap: impls owned through a trait argument.** When an argument is
still an inference variable, its owner folder is unknown. `Money:
Add[?0]` may be answered by `impl Add[Cents] for Money` in the folder that
declares `Cents`
([`trait.own.argument`](../../spec/lang/09-traits.md#r-trait.own.argument)).
`Instantiations` and the trait part of `Methods` are the same case with
every argument open. The fix is a **candidate directory**: per impl
universe, the `arg_impls` rows of its folders, merged and grouped by
trait and head key (`UniverseImpls`). A goal with an open argument (a
variable, a placeholder or poison) reads the trait's directory rows for
its target head key.

Computing `arg_impls` from the frozen table once per run, rather than
storing a section in the interface, is equal to the design: nothing
persists the directory, and the folder's `api_hash` already covers the
impl's head, so the `check` key changes when an argument-owned impl is
added in a folder of the closure
([crates/hd_resolve/DESIGN.md §4.10](../hd_resolve/DESIGN.md#410-folder-interface-construction)).

**The impl universe.** The directory makes an answer depend on which
folders the asking context sees. Two folders may both ask
`Instantiations { Receiver, Pick }` while only one's closure holds
`impl Pick[Product] for Receiver`. So:

- Every solving context carries an **`ImplUniverseId`** in `SolveCx`: the
  interned, sorted list of the folders in its closure that have
  `arg_impls` or unowned rows (`ImplUniverses::intern`). Contexts with
  equal lists share one id.
- The contexts are a module's bodies, a folder's header checks (the
  folder's closure universe, with no own table), and a program build.
  A module's bodies in the test role use `test_closure(m)`, which adds
  the folders its `tests:` block uses (crates/hd_sched/DESIGN.md §6.1).
- **The id is part of the key of every goal that may read the
  directory** (`MemoKey::universe_for`): `Instantiations` and `Methods`
  always, and an `Implements` or `Project` goal with an open argument.
- **Scoped answers.** A memo record notes whether its subtree read the
  directory or unowned rows (`scoped`). A global entry that did is
  published under its key with the universe filled in.
- **Debug check.** A frame that read the directory under a key with no
  universe is an internal error.

Why the closure, not the whole graph: a library's check result must not
depend on which downstream packages a program adds.

### 3.3 The Head Index

```rust
pub struct ImplTable {                    // per folder (and per module for its own rows); columns
    pub trait_: Vec<DefId>, pub def: Vec<DefId>, pub head_key: Vec<HeadKey>,
    pub arg_key: Vec<[HeadKey; 2]>,       // filled, not read
    pub n_params: Vec<u8>,
    pub head_self: Vec<Ty>, pub head_args: Vec<TyList>,
    pub plan: Vec<Vec<PlanStep>>,         // the bound plan (section 3.6)
    pub assoc: Vec<Vec<(DefId, Ty)>>,     // associated-type bindings
    pub origin: Vec<ImplOrigin>,          // Written | Derived | Delegated | Error | NumericFamily | TupleTemplate
    pub rank: Vec<u64>,                   // content rank
    pub by_trait: BTreeMap<u32, (u32, u32)>, // trait -> row range
    pub by_key: Vec<u32>,                 // within each range, rows by (head key, row)
}
pub enum HeadKey { Ctor(DefId), Prim(Prim), Tuple(u16), TupleAny, Fn, SuspendFn, Param, Any }
```

A probe for `S: Tr[A..]` in one table:

1. `by_trait[Tr]` gives the trait's row range.
2. Binary search `by_key` for `HeadKey::of(S)`; add the `Param` rows
   (numeric families) and, for a tuple, the `TupleAny` rows (tuple
   templates). A goal whose key is `Any` sees every row.
3. Match the survivors' heads (section 3.4).

Rows from several tables merge in table order, own table first, and an
impl seen in two tables is one head (`one_row_per_impl`).

Planned, not built: a fast reject on `arg_key`, the head keys of the
first two trait arguments, after rustc's `DeepRejectCtxt`.

**Fast path (after MoonBit).** A non-generic trait (`Display`, `Eq`,
`Hash`, `Debug`) with a known self constructor usually has one row per
head key in all owner folders together, so the probe is one binary search
per owner folder and a match of the few rows in the bucket. The memo
makes the second probe free.

### 3.4 Matching A Head

Matching is one-way unification of the impl head against the goal
(`match_ty`). Impl parameters are pattern variables. Goal types are
rigid, except placeholders and variables, which the solver may not bind.

| Goal position holds | Head position holds | Result |
| --- | --- | --- |
| a type | an impl parameter | bind it; a second occurrence must be equal |
| a constructor | the same constructor | match the arguments pairwise |
| a constructor | another constructor | `No` |
| a placeholder or variable | a constructor or a bound parameter | `Maybe` |
| a parameter of the asking item | a constructor | `No` |
| a rigid projection | anything but an impl parameter | `No`: a projection on a parameter is opaque |

- **Rows.** A row argument in a head is a row parameter or a concrete row
  ([`trait.target.row-argument`](../../spec/lang/09-traits.md#r-trait.target.row-argument)),
  so matching never needs row unification with a tail.
- **Tuples.** A tuple head matches a tuple of the same arity and rest
  shape. A tuple template's head matches every tuple, and its `T` is the
  tuple.
- **Numeric families match by member list.** A `Param`-keyed head matches
  a primitive only when the impl bound's family trait lists that type
  (`family_excludes`): `impl[N < Num] Add for N` does not match `string`.
- **A bare variable among the goal's trait arguments is skipped** in
  matching; if the head commits, the variable learns the head's argument
  (section 3.8).
- **Cost.** Linear in the head's size.

### 3.5 Committing

After the probe:

| Matches | Answer |
| --- | --- |
| no `Yes`, no `Maybe`, self type known | `Fails`: no impl |
| exactly one `Yes`, no `Maybe` | commit: solve its bound plan |
| any `Maybe`, or several `Yes` on a goal with variables | `Stalled` on the goal's variables, in first-occurrence order |
| several `Yes` on a goal with no variable | only possible beside an overlap error: take the first in content order. Coherence reports the overlap (section 5.4) |

A bound that fails under a committed head fails the goal with that
bound's `FailInfo`, with this impl and step prepended to its `chain`
(section 10.1).

### 3.6 Bounds As Subgoals

An impl's bounds become subgoals after its head matched. Resolution stores
each impl's **bound plan**, an ordered list of steps computed once at
interface time:

```rust
pub enum PlanStep {
    Bound { param: u8, trait_: DefId, args: TyList, mut_: bool },
    /// `P < Tr[A, Name = Q]` with `Q` a bare impl parameter: Q is the normal
    /// form of `<P as Tr[A]>::Name`.
    Bind { param: u8, trait_: DefId, args: TyList, assoc: DefId, target: u8 },
}
```

- **Order.** A parameter that only a binding constrains, as `T` in
  `impl[T < Display, I < Store[Item = T]] Summary for Feed[I]`
  ([`trait.overlap.constrained-binding`](../../spec/lang/09-traits.md#r-trait.overlap.constrained-binding)),
  is solved by its `Bind` step before any step that reads it. Resolution
  rejects an impl with no such order (`unconstrained-impl-parameter`).
- **Execution.** `Bind` steps run while the head's arguments are fixed
  (`apply_binds_in`), by a head match of the projection; a `Bind` whose
  bound leaves trait arguments implicit reads the candidate directory.
  `Bound` steps then run in plan order, each at depth `d + 1`
  ([`trait.bound.depth`](../../spec/lang/09-traits.md#r-trait.bound.depth)).
  The first step that does not hold decides.
- **Self types shrink.** A step's self type is an impl parameter bound to
  a part of the goal or fixed by a binding, so only trait arguments and
  projections can make subgoals larger. The depth limit and fuel bound
  them.

### 3.7 Supertraits

| Direction | What happens | Where |
| --- | --- | --- |
| using a bound | `T < Child` also proves `T: Parent`, with supertrait bindings substituted | elaboration (section 4.2), once per item |
| proving a bound | `X: Child` holds through `impl Child for X`; `X: Parent` is not re-proved | nowhere at use sites |

The second row is sound because the header check proves every supertrait
under the impl's own bounds
([`trait.super.impl-bounds`](../../spec/lang/09-traits.md#r-trait.super.impl-bounds)).
That check runs in the folder's `HeaderCheck(F)` stage, from headers and
impl tables only, so a dependency's impls are checked even when its
bodies are not. `hd_check::header` asks each supertrait `Implements` goal
through `TableSolver` under the item's environment, with the folder's
closure universe and no own table. The findings are cached in one `graph`
entry per folder keyed by `hdr_key`. Supertrait bindings are not asked
as `Project` goals, and an `Overflow` in a header goal reports nothing
yet; see #192.

### 3.8 What A Goal May Teach The Checker

**Rule TS-3. `Implements` learns only through associated-type bindings.**
`I: Supplier[Item = ?0]` holds with `?0 := string` when `I`'s impl binds
`Item = string`
([`trait.binding.inference`](../../spec/lang/09-traits.md#r-trait.binding.inference)).
A unique head match never teaches anything: `Money: Add[?0]` with one impl
`Add[Money] for Money` stalls; only `Instantiations` chooses among
instantiations, where the spec says so
([Inference Through A Bound](../../spec/lang/04-type-system.md#inference-through-a-bound),
[Instantiations Of One Generic Trait](../../spec/lang/09-traits.md#instantiations-of-one-generic-trait)).
So adding an impl changes inference only where the spec's rules let it.

Implementation differs: a bare variable among the goal's trait arguments
is skipped in matching, and when one head matches, the goal commits and
`learned` binds the variable to the head's argument (`learned_from`), so
adding a second impl can change what an unrelated call infers; see #40.

### 3.9 Compiler-Supplied Impls

Each sealed trait is answered from the type's form by one table,
`hd_types::sealed::row`: one `match` over the sealed trait and the type,
not special cases scattered through matching.

| Trait | Holds for | Subgoals | Evidence |
| --- | --- | --- | --- |
| `Any` | every value type, `never`, optionals | none | `Builtin(Any)` |
| `AnyVal` | `bool`, `char`, integers, floats, `string`, tuples, enums (optionals included), function types; a newtype over one of them ([`types.sealed.anyval-members`](../../spec/lang/04-type-system.md#r-types.sealed.anyval-members)) | the newtype's base | `Builtin(AnyVal)` |
| `AnyRef` | data, `List`, `Map`, trait values, `Any`, suspensions, handles; a newtype over one | the newtype's base | `Builtin(AnyRef)` |
| `Inspectable` | [inspectable types](../../spec/lang/09-traits.md#inspectable-types): primitives, `string`, module-level declarations with inspectable arguments, `List`, `Map`, tuples, `Inspectable` trait values; trait values and `Any` as arguments only | one per type argument, one level deeper | `Builtin(Inspectable)` |
| `Tuple` | every tuple type | none | `Builtin(Tuple)` |
| `Num`, `Integer`, `Float` | the fixed primitive lists of [Numeric Traits](../../spec/lang/09-traits.md#numeric-traits): std writes one impl per type (`lib/std/num.hd`), so the impl tables answer them | the impl's | `Impl` |
| `Structure` | the target of a template instance, through that instance's environment; and every concrete tuple type, since the compiler generates a tuple's `Structure` while it instantiates a tuple template (`annot.tuple.structure`) | none | `Bound` in a template; `Builtin(Structure)` at a tuple (crates/hd_mono/DESIGN.md §13.6) |

For a parameter or a rigid projection, every row but `Any` answers
through the environment only
([`types.sealed.type-parameter`](../../spec/lang/04-type-system.md#r-types.sealed.type-parameter)).
`never` implements neither `AnyVal` nor `AnyRef`, and is not inspectable.
A row on an inference variable stalls, except `Any`, which holds.

The rows are source 3 (section 3.1), so body checks, header checks and
codegen's `select` all get the same answer. A row's subgoals are
`Implements` goals on the same trait one level deeper, asked and memoized
like a committed impl's bound plan. The rows read no impl table; they
read declarations through `SolveCx::decls` (the sealed traits' ids, a
type's declaration kind, a trait's supertraits), the same in every
context of a run. `select` returns `Selection::Builtin` for them.

Not built: a `Suspend` row (std declares no such trait); block-local
declarations, which `trait.inspectable.not.local` excludes, do not exist
in the compiler; `Inspectable`'s compiler-supplied `runtime_type` body is
a panicking vtable slot (crates/hd_mono/DESIGN.md §13.6).

**Written impls that act like built-ins.** Two kinds live in std's tables
as ordinary rows with special head keys:

- **Numeric families.** `impl[N < Num] Add for N` has head key `Param`.
  It matches a primitive that the family lists. Its bound `N < Num` is
  then an ordinary subgoal. Overlap expands it to one head per member
  type ([`trait.overlap.numeric-family`](../../spec/lang/09-traits.md#r-trait.overlap.numeric-family)).
- **Tuple templates.** `impl[T < Tuple] Eq for T by Structure` has head key
  `TupleAny`. Its bound plan is the walker obligation of
  [`annot.walker.obligation`](../../spec/lang/14-annotations.md#r-annot.walker.obligation):
  one subgoal per element, and for a rest element `List[T]...` either on
  `T` or on `List[T]`, by whether the walker implements `rest`
  (`MemberStep`, `tuple_goals`). The interface does not carry template
  bodies ([`annot.limit.interfaces`](../../spec/lang/14-annotations.md#r-annot.limit.interfaces)),
  so the driver computes the plan from the template body at first use: it
  checks the template's methods in their own module
  (`check_tuple_template_method`) and reads the `member` and `rest`
  bounds of each walker, describer or source the body passes. The plan
  is cached per template for the run; a template whose body has an error
  has no plan. The solver reads it through `ImplView::plans`. The
  evidence is `Impl` of the template at the tuple type: the spec calls
  each tuple instance "an ordinary implementation"
  ([`annot.template.tuple.instance`](../../spec/lang/14-annotations.md#r-annot.template.tuple.instance)).
  A tuple whose elements fail the obligation does not implement the
  trait: a `Fails`, not an error, until a use needs it.

Function types are not special: `Fn` and `SuspendFn` have head keys of
their own, and their impls are ordinary rows.

### 3.10 Derives, Delegation And `@error`

The solver never reads a template body. It sees only heads.

| Source | Head in the target folder's table | Bounds | Body checked by |
| --- | --- | --- | --- |
| `@derive(X)` on `data Box[T]` | `impl[T < X] X for Box[T]`, origin `Derived { template }` | `T < X` per walked parameter ([Derived Bounds](../../spec/lang/14-annotations.md#derived-bounds)) | the derive instance, checked with the module's bodies ([§4.13.9](../hd_tir/DESIGN.md#4139-derive-instances-templates-facts-and-test-overlays)) |
| a derivation block with a header | the header as written | the header's bounds | same |
| `@derive(X)` on a newtype | `impl X for Mile` | none at the head; the base type's impl is a header-check goal | the header check |
| `impl Tr for C by E` | as written | as written | generated forwarders (`delegate.rs`; [Generated Methods](../../spec/lang/09-traits.md#generated-methods)) |
| `@error`, `@from`, `@source` | `Display`, `Error`, `From[P]` heads | the generated bounds of [Generated Error Bounds](../../spec/lang/14-annotations.md#generated-error-bounds) | the generated bodies (`error.rs`) |

- **Delegated associated types.** Each associated type of `impl Tr for C
  by E` is bound to the projection `<E's type as Tr>::Name`, with `C`'s
  arguments substituted.
- **Hidden helpers.** A template body may call private helpers of the
  trait's module, which the interface exports as hidden items. Each
  helper's result type is written (type checking §9.1), so a dependent
  reads it from the interface.
- **Member obligations and coinduction.** The derive instance checks each
  walked member against the walker's bound. Recursion is coinductive
  ([`annot.bound.recursive`](../../spec/lang/14-annotations.md#r-annot.bound.recursive)):
  while checking `Tree[T]`'s members, `Tree[T]: Eq` is assumed. No
  mechanism is needed for that. The derived head
  `impl[T < Eq] Eq for Tree[T]` is already in the target folder's table,
  and the solver never re-checks an impl's members at a use. So the member
  `children: List[Tree[T]]` asks `List[Tree[T]]: Eq`, which commits to
  `List`'s impl, then to the derived head, then proves `T: Eq` from the
  environment. The search meets no cycle and holds no assumption (rule
  TS-7).
- **`missing-derived-bound`.** A use whose failing subgoal is a bound that
  a derived head added reports `missing-derived-bound`, not
  `unsatisfied-trait-bound`
  ([`trait.derive.bound-unmet`](../../spec/lang/09-traits.md#r-trait.derive.bound-unmet)).
  `FailInfo`'s chain carries the impl's origin.

### 3.11 The Synthetic Impl Inventory

Every impl that no one wrote as `impl ... :` has a stable descriptor, a
place where its head lives, a dependency that invalidates it, an overlap
rule and an evidence form.

| Family | Descriptor (stable) | Head lives in | Invalidated by | Overlap | Evidence |
| --- | --- | --- | --- | --- | --- |
| `@derive(X)`, derivation block | target path, trait path, `Derived` | the target folder's interface | the target folder's interface; the template body via the trait folder's deep hash | ordinary heads; beside a written impl, `overlapping-impl` | `Impl` |
| tuple template of any trait | trait path, `TupleTemplate` | the trait folder's interface, head key `TupleAny` | the trait folder's interface; for the member steps, the template module's template text hash | a written tuple impl of the trait is `overlapping-impl` ([`annot.template.tuple.overlap`](../../spec/lang/14-annotations.md#r-annot.template.tuple.overlap)) | `Impl` of the template at the tuple type |
| numeric family | impl path, `NumericFamily` | std's interface, head key `Param` | std's interface | expanded per member type | `Impl` |
| `@error`, `@from`, `@source` | target path, trait path, member index | the target folder's interface | the target folder's interface | ordinary heads | `Impl` |
| delegation `by E` | impl path, `Delegated` | written head | the folder's interface | ordinary head | `Impl` |
| `Any`, `AnyVal`, `AnyRef`, `Tuple`, `Inspectable` | the trait | the compiler (section 3.9) | the toolchain key | sealed: no user impl can exist | `Builtin` |
| `Structure` | the template instance or the tuple type | the instance's environment; the compiler for tuples | the instance; the toolchain key | never an impl | `Bound`; `Builtin` at a tuple |
| function types `Fn`, `SuspendFn` | not synthetic | ordinary heads | ordinary | ordinary | `Impl` |

A user library's tuple template is no different from std's. The member
steps come from the template's body, so the template text hash of every
tuple-template module in `closure(F)` joins the key of each module's
`check` entry and of `HeaderCheck(F)` (`template_hashes`). That hash
covers the module's API text, the template's method bodies, and the
bodies whose signatures callers infer. An edit to a template body redoes the
header check of each folder that reaches it, and the cached result of an
unrelated edit, in another module or in another body, stays.

## 4. Associated Types And Normalization

### 4.1 Where Projections Come From

A projection `<S as Tr[A]>::Name` is the pool's `Assoc` type (section
2.1). Projections come from:

- signatures: `T::Item`, `Self::Item`, `I::Item` in a result type;
- instantiated method results: `Self::Out` of `add` becomes
  `<Money as Add[Money]>::Out`;
- impl bindings that name another projection, as a delegated impl's do;
- bounds with bindings: `I < Supplier[Item = T]` relates `I::Item` and `T`.

### 4.2 Elaboration Of The Environment

`hd_check::body::add_bound` turns an item's declared bounds into its
`ParamEnv` once per body:

1. Each declared bound `P < Tr[A, N = U]` is a clause, with its
   bindings, its defaulted arguments filled.
2. For each clause, walk `Tr`'s supertraits depth first, in declared
   order, substituting `P` for `Self` and the clause's arguments for the
   trait's parameters. A supertrait binding (`Num < Add[Out = Self]`)
   becomes a binding on the new clause (`P: Add[P, Out = P]`).
3. **One clause per trait reference, with merged bindings.** A diamond
   may reach `Supplier` once unbound and once with `Item = i32`. On a
   second visit the walk should merge binding by binding: add a binding
   not yet present, ignore an equal one, and treat a different one as a
   conflict. The result then does not depend on the order in which paths
   are met.
   Implementation differs: a reference met again is skipped, so only the
   first visit's bindings are kept; not judged whether a real diamond
   loses a binding.
4. **A conflict is a header error.** Resolution reports two different
   bound types for one projection as `duplicate-associated-binding`, and
   two bounds declaring one name as `ambiguous-associated-type`
   (`hd_resolve::assoc`), before any body.
5. The run memo interns the clause list; equal lists get one `EnvKey`.

Cost: linear in the size of the supertrait closure of the declared
bounds, once per item.

### 4.3 Normalization: Lazy, At Three Points

The checker normalizes a projection only when it needs its value, as
rustc's lazy normalization does: when unification meets a projection,
before method lookup or field access on a value whose type is a
projection, and before forming a goal whose types hold a projection with
a known base.

For `<S as Tr[A]>::Name`:

| `S` is | Answer |
| --- | --- |
| a parameter with an environment binding for `Name` | the bound type: `I::Item` is `T` under `I < Supplier[Item = T]` |
| a parameter with a bound on `Tr` but no binding | the projection itself, as a rigid type, with the bound's arguments. It equals only itself |
| a trait value that binds `Name` | the binding ([`trait.dyn.bound.projection`](../../spec/lang/09-traits.md#r-trait.dyn.bound.projection)) |
| known, or open with one fitting impl | prove `Implements` first; on `Holds` with `Impl` evidence, read that impl's binding for `Name`, substitute the impl's arguments, and normalize the result again |
| known, no impl binds it | the trait's default for the associated type, if any |

**Normalization proves applicability.** A head match alone does not make
an impl apply: its bounds must hold too. So a source-level normalization
on a known base rests on a proof of the `Implements` goal, which is
memoized. Codegen's `normalize_concrete` is a separate mode: at an
instance the checker has already proven the goal, so it reads the binding
after a head match.

The design makes this a memoized `Project` goal that answers `Fails`,
with the `Implements` goal's `FailInfo`, when the impl does not apply.
Implementation differs: the checker normalizes directly (`Ck::normalize`)
and leaves the projection rigid when `Implements` does not hold; not
judged.

- **Rigid projections and equality.** Under `T < Supplier`, `T::Item` and
  `string` never unify. Under `I < Supplier[Item = T]`, `I::Item` and `T`
  are the same type, because normalization replaces the projection before
  any comparison
  ([`trait.binding.interchangeable`](../../spec/lang/09-traits.md#r-trait.binding.interchangeable)).
- **Normalization cycles.** Nested normalizations are counted
  (`norm_depth`), so an impl binding that names its own projection ends.
- **Aliases** are expanded when headers are lowered, so a projection never
  meets an alias.

### 4.4 Projections In Impl Heads

An impl head may not contain a projection of an impl parameter, as in
`impl[I < Store] Summary for Feed[I::Item]`: such a head is not
injective, so matching cannot solve `I` and overlap cannot be decided
from heads. Resolution reports it as `unconstrained-impl-parameter`,
since a parameter that appears only inside a projection is not
constrained.

### 4.5 Header Bounds In Bodies And Calls

A declared bound enters solving in exactly two places:

- **Body environment.** The elaborated `ParamEnv` travels with the body
  (`EnvKey` in the memo key): every clause, its supertrait expansion and
  its bindings are available to goals the body asks, so a call on `T`
  with `T < Display` solves through the environment clause, never by
  searching impls.
- **Call sites.** The caller's arguments must satisfy the callee's
  bounds (`trait.bound.unsatisfied`), checked as goals at the call; the
  callee body then solves from its own environment. `T::f(args)` resolves
  through the bounds for the same reason (`trait.assoc-call.parameter`).

A binding constrains a projection by orientation: `I::Item` under a
`Store[Item = T]` bound normalizes to `T`, so projections never need a
search of their own. `Bind` plan steps (section 3.6) solve an impl's
binding-constrained parameters the same way.

## 5. Coherence

### 5.1 What Runs When

| Check | Needs | Runs at | Error |
| --- | --- | --- | --- |
| orphan rule (package level) | the impl head and package ownership | folder interface | `orphan-impl` |
| module ownership | the head and the declaring modules | folder interface | `nonlocal-impl` |
| targets: bare parameter, trait value, outer `mut`, invalid inherent target | the head | folder interface | `bare-parameter-impl-target`, `trait-value-impl-target`, `mutable-impl-target`, `invalid-impl-target` |
| unconstrained parameters and the bound plan | the head and bounds | folder interface | `unconstrained-impl-parameter` |
| sealed traits | the trait | folder interface | `sealed-trait-implementation` |
| a second template; `@derive` beside a written impl or a block for the same trait | the module's heads | folder interface | `overlapping-impl` |
| law partners all derived or all written | the module's heads | folder interface | `mixed-derived-law` |
| supertraits of an impl | the solver, under the impl's environment | `HeaderCheck(F)` | `missing-supertrait-implementation` |
| overlap between impls | heads of one trait across the program graph | `Coherence` task | `overlapping-impl` |
| duplicate inherent members | inherent heads of one type, in its module | folder interface | `duplicate-inherent-member` |
| a written impl missing trait methods or binding an undeclared associated type | the impl and its trait | `Body(m)` | `missing-trait-method`, `extra-trait-member` |

Only the header checks need goals. Everything else is a function of
heads, so it runs before any body and caches with the interface.

### 5.2 The Overlap Check

The design runs one `Coherence(trait)` task per trait with more than one
impl in the program's graph
([§4.12.3](../hd_resolve/DESIGN.md#4123-coherence)), with a near-linear
algorithm:

1. **Collect** the trait's heads from every folder in the graph, in
   content order, derived, generated and delegated heads and tuple
   templates included (section 3.11).
2. **Expand** numeric-family heads to one head per member type.
3. **Ground heads** (no impl parameter) first: encode each head
   canonically and insert it into a hash set, where a second equal head
   is an overlap, and into a **ground trie** over the head's pre-order
   walk.
4. **Generic heads**, in content order: query the **generic trie** (a
   discrimination tree in which an impl parameter is a wildcard) for
   stored heads that may unify, and walk the wildcard positions through
   the ground trie. A row parameter matches every row; `TupleAny`
   matches every tuple head.
5. **Confirm** each candidate pair with full unification after renaming
   apart. Report a pair once, on the later impl in content order, with a
   **witness**: the unifier's solution applied to the head, such as
   "both apply to `Box[Plain]`".
6. **Budget.** Stop after the first overlap reported per impl, so an
   all-overlapping bucket costs one report per impl.

Cost: linear in the total head size plus the candidate pairs the tries
return.

Implementation differs: one `Coherence` task covers every trait
(`hd_driver` `Run::coherence`) and checks again only the traits whose
`coh_key` changed (crates/hd_cache/DESIGN.md §5.3), and
`hd_resolve::Universe::overlaps` unifies each head with
every earlier head of its trait. Its answers, witnesses and content-order
blame match steps 1, 2, 5 and 6, and a written tuple impl beside a tuple
template is reported directly. The pairwise loop is quadratic in a
trait's head count (300 `From` impls of one error type cost about 45,000
unifications).

### 5.3 Why No Global Index

- **Selection needs no global view.** Ownership confines the impls a goal
  can use to at most `2 + len(Args)` folders, plus the universe's unowned
  rows and, for open arguments, the candidate directory (section 3.2).
- **Coherence needs a per-trait view, not a global one.** Each trait is
  one task keyed by its sorted head hashes, so an edit reruns only the
  traits whose heads changed. A head hash is `H(head, rank)`: the rank
  decides which impl of a pair is reported, so swapping two overlapping
  impls reruns the task. As built, the key is coarser (section 5.2).
- **A global index would be a shared mutable structure** built as folders
  finish, and its state at a given moment would depend on the schedule.
  The owner tables are frozen with their folder, so a body can solve
  while unrelated folders are still being resolved.

### 5.4 What The Solver Assumes

Bodies are checked before or while coherence runs; they do not wait for
it. The solver assumes no overlap. If two heads match a goal with no
variable, which only an overlap error allows, the solver takes the first
and reports nothing (section 3.5). The program fails on
`overlapping-impl`, so the choice never reaches a build.

## 6. Search

### 6.1 Depth First, On An Explicit Stack

```rust
pub struct Frame {   // 24 bytes, asserted
    pub goal: u32, pub impl_row: u32, pub step: u16, pub depth: u8,
    pub height: u8, pub heads: u32, pub subst: u32, pub seen: u32,
}
```

**Rule TS-9. No unbounded native recursion in the solver.** The design
keeps goals as frames on a vector, so the deepest stack is bounded by the
depth limit and never overflows the native stack, including in the
browser. As built, `Search::goal` recurses natively through `commit` and
`supplied`, and the stack of open goals (`Search::stack`) holds their
keys for cycle detection; the depth limit of 64 bounds the recursion, so
the native stack holds at most about 64 nested goals. This is equal to
the design in what it bounds, and simpler.

### 6.2 The Search Graph

With rule TS-2, a goal has one way to be proven. The graph of goals is a
graph of AND nodes:

- a node is a canonical goal;
- its children are the steps of its committed impl's bound plan, or the
  subgoals of a compiler-supplied row.

So a goal's answer is a function of its children's answers, combined in
plan order: the first child that does not hold decides, otherwise
`Holds`. There is no OR node except the ordered choice among sources in
section 3.1.

### 6.3 Cycles

A goal that meets itself on the stack is a cycle.

- **Coinductive cycles.** The spec has one: a derived impl's member check
  ([`annot.bound.recursive`](../../spec/lang/14-annotations.md#r-annot.bound.recursive)).
  It never reaches the search (section 3.10).
- **Inductive cycles.** Every other cycle has no finite proof, so the
  spec's answer is `trait-resolution-depth`
  ([`trait.bound.depth.limit`](../../spec/lang/09-traits.md#r-trait.bound.depth.limit)).
  The solver answers `Overflow` at once, with height 255.

**Rule TS-7. No provisional results, no fixpoint.** A frame on the stack
is exploring its current plan step, and every earlier step of it has
completed with `Holds`. Those completed answers are context-free (section
7.1), so a fresh evaluation of any frame retraces the same path. Hence:

- **The SCC rule.** When goal `G` meets itself, the frames from `G` to the
  top of the stack form one strongly connected set. Every one of them
  answers `Overflow`, whichever was asked first.
- **Completion before publication.** A frame's entry is published only
  when the frame pops, with its final answer.
- **No provisional answer ever leaves the stack.** So every entry, in a
  cycle or not, can be memoized at once.

This removes the part of rustc's search graph that took the most work:
provisional caches, cycle heads and fixpoint iteration. It holds only
because no node has a second route (rule TS-2) and no cycle is
coinductive. The checker keeps the one fixpoint the spec defines, the
bound-inference loop
([`types.generic.infer.bound.fixed-point`](../../spec/lang/04-type-system.md#r-types.generic.infer.bound.fixed-point);
[type checking §2.4](#24-calls-and-use-site-type-arguments)).

### 6.4 Stalling

A goal stalls when its answer depends on a variable:

- the self type is a variable: `?0: Display` stalls on `?0`;
- a head match reports `Maybe`, or several heads match a goal with
  variables: `Money: Add[?0]` with two `Add` impls stalls on `?0`;
- a child stalls: `List[?0]: Eq` commits to `impl[T < Eq] Eq for List[T]`,
  and its child `?0: Eq` stalls, so the goal stalls on `?0`.

The stalled answer lists the variables in first-occurrence order. A stall
has a placeholder, so it goes to the body memo only.

Literal placeholders stall like any other, and the checker defaults the
literals of a waiting goal before asking again at the end of the body.
The design's literal-kind matching (a non-integer head rules out an
integer-literal placeholder) is not built yet; see #40.

### 6.5 Ambiguity

`Candidates` belongs to `Instantiations` and `Methods` only.

**`Instantiations { S, Tr }`** collects every impl head of `Tr` that
matches `S` with all trait arguments open: the owner tables of `Tr` and
`S`, the unowned rows and the candidate directory (section 3.2). A
parameter or trait value self type answers "not implemented": the
checker reads the environment clauses or the value's trait itself.

**Candidates are schemes with residual bounds.** Matching `S` fixes only
the impl parameters that occur in the target. A parameter that occurs
only in the trait arguments, as `U` in
`impl[U < Display] Pick[U] for Family`, stays open, and a bound on it
cannot be decided yet. So each candidate is a scheme:

```rust
pub struct Candidate {
    pub row: ImplRef,
    pub n_fresh: u8,        // impl parameters the target did not fix
    pub impl_args: TyList,  // each impl parameter: the fixed type, or the parameter itself when fresh
    pub args: TyList,       // the trait arguments, over the same
    pub residual: Vec<u16>, // the plan steps the solver did not decide, in plan order
}
```

- The solver runs every `Bound` step whose parameters the target fixed. A
  candidate whose step `Fails` is dropped. A step that overflows or runs
  out of fuel ends the whole answer that way.
- The steps that read a fresh parameter, and fixed steps that stall, are
  the candidate's residual obligations. Only `Bound` steps become goals.
- A tuple template at a tuple is a candidate whose residuals are its
  stalled member steps.
- The checker instantiates a scheme inside each trial
  ([type checking §2.5](#25-methods-and-operators)).

| Outcome | Answer |
| --- | --- |
| a head reports `Maybe` | `Stalled` |
| no candidate survives | `Fails` |
| one or more survive | `Candidates`, in content order, even for one |

**`Methods { receiver, traits }`** asks one `Instantiations` goal per
trait in `traits`, in order, and returns every candidate; a trait the
receiver does not implement adds nothing. The trait list comes from the
module's method index (`hd_check::MethodIndex`), which maps a name to the
available traits that declare it, so one goal is asked per trait with
that name, not per available trait. A module's own items reach the index
twice, as its own and again through its folder's interface; each
declaration enters it once, so one trait is never two candidates (#346).
Inherent methods and the unavailable
traits for the `use` hint are the checker's.

### 6.6 Choosing Among Several Traits

A method name provided by more than one available trait resolves in the
checker, driven by one `Methods` goal:

1. **Gather.** The method index gives the available traits that declare
   the name; the goal asks one `Instantiations` goal per trait.
   Unavailable traits never enter the goal.
2. **Narrow by shape.** Before any trial, candidates whose parameter
   head constructors cannot match an argument whose type is already known
   are dropped.
3. **Narrow by fit.** Each surviving scheme instantiates inside a
   checker trial; a candidate applies when the arguments check against
   its method type, plus the expected result type when the call has one
   (`trait.resolve.applies`, `trait.resolve.applies.expected`).
4. **Decide.** Exactly one applicable candidate is selected
   (`trait.resolve.one-applies`); instantiations of one generic trait
   choose the same way, with literal defaults breaking width ties
   (`trait.resolve.literal-arg`). Two or more applicable candidates from
   different traits, or beside a promoted candidate, are
   `ambiguous-method` (`names.method-lookup.ambiguous`); a single
   candidate with the wrong signature is still selected and the call is
   checked against it. None applicable is `type-mismatch`, none at all
   `unknown-method`.
5. **Record.** The call keeps a callee record: `Item` for inherent and
   direct callees, `TraitMethod` with the solver's choice otherwise
   (`Impl`, `Bound`, `TraitValue` or `Builtin`). Collection pushes per
   crates/hd_mono/DESIGN.md §13.2.

The `Methods` goal itself is not memoized, since its answer depends on
the module's trait availability; its `Instantiations` goals are. Keying
`Methods` by an availability key is not built yet; see #40.

## 7. Memoization And Budgets

### 7.1 Memo Keys And Eligibility

An answer may be reused only where every input that decided it is equal:

| Input that can change an answer | How the key covers it |
| --- | --- |
| the goal's types, with variables | the canonical goal (section 2.2) |
| declared bounds and supertrait bindings | `EnvKey`, when the goal names a parameter; `EMPTY` otherwise |
| the asking module's own impl rows, block-local impls included | each record keeps the `(trait, head key)` probes of its subtree; a global entry serves a module only when its own table has no row for any of them (`own_free`), and an answer that read own rows stays in the body memo. This replaces a visible-local-impls key: no key part is needed, and most goals still go global |
| block-local impls not visible at the asking point | a probe that meets a hidden impl drops it, and the own-table rule keeps such answers out of the global memo |
| which argument-owned impls the context sees | the `ImplUniverseId` for goals that may read the directory (section 3.2) |
| unowned rows read | a scoped record is published with the universe filled in; an unscoped entry serves only a universe whose unowned rows add nothing to its probes |
| trait availability (`Methods` only) | not keyed: `Methods` goals are not memoized |
| associated-type bindings in the goal | not keyed: a goal with bindings skips the memo |
| coinductive assumptions | none exist (section 3.10) |
| proof depth | the stored height, checked at each use (section 7.3) |
| remaining fuel | never stored: `OutOfFuel` is never memoized |
| the interfaces of the run | the global memo lives for one run |

**Rule TS-4. Only completed, context-free answers are published
globally.** An entry goes to the global memo only when it has no
placeholder, it completed (rule TS-7), it never met the depth cut, and
it read no own-table row. Every other entry goes to the body memo.

**Remapping.** Answers are stored as `CanonAnswer`, over placeholders and
global types, and mapped back through the caller's `CanonVars`. No entry
holds a body-local `Ty` or `InferVar`, so a rollback cannot leave a
dangling entry.

```rust
pub struct GlobalMemo {                    // one per run; shared by checking and codegen
    shards: [Mutex<HashMap<MemoKey, u32>>; 16],
    entries: AppendVec<MemoEntry>,         // published entries never change
    records: AppendVec<MemoRecord>,        // key, answer, children, probes, scoped
    envs: Mutex<HashMap<EnvClauses, EnvKey>>,
    hits: AtomicU64, misses: AtomicU64,
}
pub struct BodyMemo {                      // per body; lent per call; never rolled back
    table: HashMap<MemoKey, u32>,          // body entries, and the global slots this body used
    entries: Vec<MemoEntry>, records: Vec<MemoRecord>,
    met: HashSet<MemoKey>,                 // goals this body has been charged for (7.4)
}
#[repr(C)]
pub struct MemoEntry { answer: u32, children: u32, height: u8, kind: MemoKind, heads: u16 } // 12 bytes
```

- **First writer wins** in the global memo. Two threads that compute one
  key compute the same entry (rule TS-1), which debug builds assert.
- **Children are memo slots.** A record's children are entry indices,
  global or body (`BODY_ENTRY` bit), so the TS-5 walk follows indices
  into append-only arenas with no lookup.
- **Repeats take no lock.** The body table remembers each global slot
  the body used, so only a body's first ask of a global goal takes a
  shard lock. The design's per-worker read-through table of 4,096 slots
  in front of the shards is not built; not judged, since no measurement
  shows the shard locks costing.
- **Not persisted.** A warm check rechecks only edited modules.

### 7.2 Depth Is Charged From The Use

The spec counts depth from the use: a bound required directly by a use has
depth 1, and a bound of the impl that proves a depth-`n` bound has depth
`n + 1`. More than 64 is `trait-resolution-depth`, "whether or not a
deeper proof would succeed"
([`trait.bound.depth.limit`](../../spec/lang/09-traits.md#r-trait.bound.depth.limit)).

### 7.3 Heights And Lower Bounds

A memoized answer must not depend on the depth at which it was first
computed:

1. **Exploration is cut at the absolute limit.** A goal computed at depth
   `d` may explore `65 - d` levels, itself included.
2. **A goal that completes without meeting the cut** stores its answer and
   its **height** `h`: the deepest level it used, counted from itself. A
   use at depth `d'` gets `Overflow` when `d' + h > 65`, and the stored
   answer otherwise.
3. **A goal that meets the cut** stores `AtLeast(r)`: "with `r` levels or
   fewer, this is `Overflow`". A later use with at most `r` levels left
   reuses it as `Overflow`; a use with more levels left computes the goal
   again. Such an entry is never global and never a `Fails`, so depth
   exhaustion is never cached as a semantic failure.
4. **A cycle** stores `Overflow` with height 255: no depth suffices.

So whether a goal overflows depends only on the goal and on where it is
used, never on which body computed it first.

**What counts as a level**: a bound-plan step of an impl, an element
obligation of a tuple template, and one step of a compiler-supplied row
(an `Inspectable` argument, a newtype's base). An environment clause and a
trait-value source are leaves at no extra level.

### 7.4 Fuel

The principle is the checker's: fuel used is a pure function of the body
and its frozen inputs ([type checking §11.1](#111-what-counts)).

**Proof nodes, charged once per body.** Each memo record stores its
children: the slots its committed plan asked, in plan order, with repeats
removed (as in `Pair[X, X]`, where both bound steps ask `X: Eq`). Each
entry also stores how many heads its probe returned. The entries and
their children form the goal's proof DAG, which is the same whether an
entry was computed now or found in a memo.

**Rule TS-5. Charge each proof node once per body.** A body's `met` set
records the goals it has paid for. When a body reuses an entry, the
solver walks its proof DAG (`charge_dag`); each node not yet in `met`
costs `1 + heads` and joins `met` before its children are walked. A node
already in `met` costs nothing and is not entered; the asked goal itself
costs 1 when it is already met. A goal computed now is charged the same
way when its probe is done (`charge`). So a body pays once for each
distinct goal in the union of the proof DAGs it used, whatever the memo
held. A rollback refunds nothing and does not clear `met`.

Charging the full DAG on every hit would bill 1,600 `Cents::from(N)`
lines 1,600 times for one goal. Charging only what was computed would
make a warm memo cheaper than a cold one, so a body could pass its budget
on one run and fail on the next.

### 7.5 Limits And Their Diagnostics

| Limit | Value | Counted by | Diagnostic |
| --- | --- | --- | --- |
| proof depth | 64, fixed by the spec | heights (7.3) | `trait-resolution-depth` at the use, from bodies only (#192) |
| body fuel | 2^24 steps per body | proof nodes, once per body (7.4) | see type checking §11.2 |
| placeholders per goal | 255 | canonicalization | none: the goal is asked without the memo |
| candidates in one answer | none | | none needed: bounded by the impls that exist |

**F-626-style blowups are bounded.** The solver writes nothing to the
checker (rule TS-1); one `Instantiations` goal per distinct receiver and
trait is computed once per run and found in the memo after that; no goal
has more than one committed impl (rule TS-2); and shared subgoals are
computed once and charged once per body, so a diamond costs the size of
its DAG. The checker's trials copy some state (type checking §3.5).

### 7.6 Bounded Is Not Linear

Fuel and depth make every case end. They do not make it fast. These are
the algorithms that make the common cases near-linear, each with the
input it is linear in:

| Work | Algorithm | Linear in |
| --- | --- | --- |
| repeated goals | the memo, keyed canonically; repeats cost one lookup | distinct goals |
| shared subgoals | memoized children; each proof node charged once per body (rule TS-5) | distinct goals in the proof DAG |
| impl lookup | head key bucket; the fast path for non-generic traits | candidates that share the head key |
| open trait arguments | the per-universe candidate directory | argument-owned impls of that trait |
| overlap | designed: ground heads hashed, generic heads in a discrimination tree; built: pairwise (section 5.2) | designed: total head size, plus reported overlaps |
| elaboration | once per item, one clause per trait reference | the supertrait closure |
| canonicalization | one walk of the goal | goal size |
| instantiation choice | prefilter candidates by the head of each known argument type before any trial; one trial per surviving candidate | surviving candidates |
| nested trial chains | a per-call-site trial memo (#49) | distinct trial contexts |

## 8. Evidence

### 8.1 What A `Holds` Answer Carries

```rust
pub enum Evidence {
    Impl { row: ImplRef, args: TyList },   // written, derived, delegated, generated, numeric family, tuple template
    Bound { param: ParamRef, index: u16 }, // clause index in the elaborated environment
    TraitValue { trait_: DefId },          // the self type is a trait value of this trait or a subtrait
    Builtin(BuiltinImpl),                  // Any, AnyVal, AnyRef, Inspectable, Tuple, Structure, ...
    Poison,
}
```

Only the **top-level choice** is evidence. The nested proof is not stored
in TIR, because codegen monomorphizes: at each instance the nested bounds
are concrete and are selected again (rule TS-6). The solver keeps the
nested proof only in the memo, for `FailInfo`.

### 8.2 What TIR Records

| Use | TIR | Evidence recorded |
| --- | --- | --- |
| a trait method call with a known self type or a bound | `Call` with a `TraitMethod` callee | the choice: `Impl`, `Bound`, `TraitValue` or `Builtin` (`ChoiceKind`) |
| a conversion to a trait value | `Coerce` of kind `ToTraitValue` or `ToAny` | the concrete type, the trait reference and the `Impl` choice |
| a call through a trait value | `CallDyn` | none: its method-level bounds are proven at the call and reach the erased body through the type witness (section 9.2) |

A tuple template instance is an `Impl` choice and needs none of its own.

### 8.3 What Codegen Does With It

**Rule TS-6. Codegen selects through owner lookup.** At an instance,
every type is concrete. The design's `select` is a head match plus the
plan's `Bind` steps: no proof, no depth limit, no fuel, since the checker
proved the goal generically.

- **Selection, then reconstruction.** A head match fixes only the impl
  parameters that occur in the head. In
  `impl[T < Display, I < Store[Item = T]] Summary for Feed[I]`, matching
  `Feed[ConcreteStore]` fixes `I` but not `T`, and the selected method's
  body needs `T`. So after the head match, the plan's `Bind` steps run in
  plan order with concrete normalization, each fixing its target
  parameter. Every parameter is fixed by the head or by a `Bind` step, or
  resolution has rejected the impl as `unconstrained-impl-parameter`. The
  returned `Selection` holds every impl argument.
- **No depth limit at instances.** The spec's 64 counts from a use in
  source. An instance of generic code may need a deep concrete proof that
  no source use asked for, and the representation of generic code is
  free ([`trait.bound.representation`](../../spec/lang/09-traits.md#r-trait.bound.representation)),
  so this is no language error. Polymorphic recursion is stopped by the
  instantiation depth limit instead
  ([§13.4](../hd_mono/DESIGN.md#134-the-instantiation-depth-limit)).
- **A failed select is an internal error**, never a user diagnostic.
- **Default methods.** A `TraitMethod` call resolved to an impl that does
  not write the method instantiates the trait's default body, with `Self`
  set to the impl's target.
- **Vtables.** For each `(type, trait reference)` that a coercion needs,
  codegen selects the impl of the trait and of each supertrait.

Implementation differs: `select` solves the concrete reference as a full
`Implements` goal (`TableSolver::select`), with its own body fuel of 2^24
steps and the 64-level depth limit, through the program build's context
and the run's shared memo; `Bind` steps run inside the head match
(`apply_binds_in`). The extra proof is wasted work at every instance, and
a deep concrete proof that generic code needs fails as an internal error
instead of selecting; see #40. Mono also keeps a per-build table of picks
keyed by concrete trait reference (crates/hd_mono/DESIGN.md §13.2).

## 9. Trait Values And Dynamic Safety

### 9.1 Which Traits Can Be Values

**Every trait can be a `dyn` type**
([`trait.dyn.any-trait`](../../spec/lang/09-traits.md#r-trait.dyn.any-trait)).
A trait value type is written `dyn Tr`. What cannot work dynamically is
decided **per member**, and the error is at the call, not at the type.

Resolution computes, per trait member at interface time, from headers:

| Member | On a `dyn` value |
| --- | --- |
| an associated function | unavailable |
| a method with `Self` outside the receiver | unavailable |
| a method with method-level type parameters, whatever their bounds | available, through the erased slot (section 9.2; [`trait.dyn.safe.method-type-param`](../../spec/lang/09-traits.md#r-trait.dyn.safe.method-type-param)) |
| a method whose signature mentions an associated type | available when the `dyn` type binds that type |

The trait record stores, per member, an "unavailable" flag. The checker
reads it at a call through a `dyn` value, with no solver goal, and the
trait-value source (section 9.3) reads it through
`Declarations::has_unavailable_member`.

A `dyn` type must bind every associated type
([`trait.dyn.binding.complete`](../../spec/lang/09-traits.md#r-trait.dyn.binding.complete)).
Requirement keys
([`req.key.any-trait`](../../spec/lang/11-requirements-and-suspension.md#r-req.key.any-trait))
follow the same per-member rule.

### 9.2 Vtable Shapes

A **vtable shape** is computed once per trait and stored in the trait
record:

```rust
pub struct VtableShape {
    slots: Box<[SlotRef]>,   // the trait's own methods, in declaration order
    supers: Box<[DefId]>,    // direct supertraits, in declared order; each a pointer to its vtable
    type_id: bool,           // the trait is Inspectable or extends it
}
```

- **Supertraits by pointer, not by copying their slots.** A diamond such as
  `Error < Display & Inspectable` shares one vtable per supertrait.
  Widening to a supertrait value reads one pointer
  ([`trait.dyn.widen`](../../spec/lang/09-traits.md#r-trait.dyn.widen)).
- **Method-level type parameters.** A method available on `dyn` may have
  a parameter `T < Display`, called with any type argument
  ([`types.trait.safe.method-type-arg`](../../spec/lang/04-type-system.md#r-types.trait.safe.method-type-arg)).
  The checker proves each such bound at the call's type argument, as an
  ordinary `Implements` goal, and records nothing more: `CallDyn` has no
  evidence operands. The slot's body is the impl method compiled once,
  erased, and it receives one type witness per method type parameter,
  carrying the layout operations and the bound methods at the caller's
  concrete type. That ABI is codegen's
  ([crates/hd_mono/DESIGN.md §13.5](../hd_mono/DESIGN.md#135-dictionaries-trait-values-and-gadt-evidence)).
- **Row parameters** pass their providers as one bundle and need no slot.

Implementation differs: no `VtableShape` is stored in the trait record;
`hd_mono` lays out vtables itself; not judged.

### 9.3 Trait Values As Self Types

A trait value type satisfies a bound on its own trait and on each
supertrait ([`trait.dyn.bound`](../../spec/lang/09-traits.md#r-trait.dyn.bound)).
These are the rows of source 2 in section 3.1 (`trait_value_holds`):

| Goal | Holds when |
| --- | --- |
| `Tr[A]: Tr[B]` | `A` matches `B` ([`trait.dyn.bound.instantiation`](../../spec/lang/09-traits.md#r-trait.dyn.bound.instantiation)) |
| `Child[A]: Parent[B]` | the elaborated supertraits of `Child[A]` contain `Parent[B]` |
| any member of the trait or its supertraits unavailable on `dyn` | never (`trait.dyn.bound.available`) |
| any other trait | never: the value satisfies no other bound and is no impl target ([`trait.dyn.bound.no-impl`](../../spec/lang/09-traits.md#r-trait.dyn.bound.no-impl)) |

The binding rule ([`trait.dyn.bound.binding`](../../spec/lang/09-traits.md#r-trait.dyn.bound.binding))
is checked by the checker against the normalized projection. The
goal's `mut_` flag is part of its memo key, but the solver does not test
it; the checker rejects a readonly argument for a `T < mut Trait` bound
(type checking §8.2). Whether that covers
[`trait.dyn.bound.mut`](../../spec/lang/09-traits.md#r-trait.dyn.bound.mut)
for trait values is not judged.

### 9.4 Conversion To A Trait Value

`S` converts to `Tr[A, N = U]` when `Implements { S: Tr[A], bindings:
[N = U], mut_ }` holds, with `mut_` set for `mut Tr`
([`trait.dyn.binding.convert`](../../spec/lang/09-traits.md#r-trait.dyn.binding.convert),
[`trait.dyn.mut`](../../spec/lang/09-traits.md#r-trait.dyn.mut)). `Any`
always converts. `Inspectable` converts through its compiler-supplied row.
A trait extending `Inspectable` still needs its own impl
([`trait.erase.child-impl`](../../spec/lang/09-traits.md#r-trait.erase.child-impl)).
A failure is `type-mismatch`, not `unsatisfied-trait-bound`: the checker
picks the code from the site.

## 10. Diagnostics

The solver writes no text. It returns `FailInfo`, and the checker renders
it (rule TS-1).

### 10.1 `FailInfo`

```rust
pub struct FailInfo {
    pub leaf: CanonGoal,        // the deepest failing goal on the committed path: the root cause
    pub chain: Vec<ChainStep>,  // the committed impls from the asked goal down to the leaf
    pub reason: FailReason,     // NoImpl, Binding, NotMutable, NotInspectable, WrongCategory, Sealed
    pub near: Vec<NearMiss>,    // OtherInstantiation, NotAvailable, Derivable
}
pub struct ChainStep { pub impl_row: ImplRef, pub step: u16, pub origin: ImplOrigin }
```

Because of rule TS-2, the failing path is unique: one chain from the
asked goal to one leaf.

Planned, not built: the solver fills no `near` entry yet, so the near-miss
messages of section 10.3 have no input.

### 10.2 Which Code

The checker picks the code from the site and the chain:

| Site or chain | Code |
| --- | --- |
| a bound of a call or a type, failing at the leaf | `unsatisfied-trait-bound` |
| the chain's last step is a bound that a derived head added | `missing-derived-bound` ([`trait.derive.bound-unmet`](../../spec/lang/09-traits.md#r-trait.derive.bound-unmet)) |
| a member obligation in a derive instance of `Eq`, `PartialOrd`, `Ord` or `Hash` | `derive-field-missing-trait`, at the field |
| a member obligation of any other template | `member-not-derivable`, at the opt-in, naming the member |
| a header check of a supertrait | `missing-supertrait-implementation` |
| a conversion to a trait value | `type-mismatch` |
| an operator with no fitting impl | `type-mismatch` naming the missing trait ([`expr.op.no-impl`](../../spec/lang/05-expressions.md#r-expr.op.no-impl)) |
| `Overflow` | `trait-resolution-depth` |

### 10.3 Messages And Fix-Its

**Name the root cause, then the path.** One line, then one hint:

```text
app.hd:12:9: unsatisfied-trait-bound: `Box[Handle]` does not implement `Eq`, because `Handle` does not
hint: add `@derive(Eq)` to `data Handle` (app.hd:3)
```

Planned, not built:

- with `--verbose`, a note listing the chain;
- a missing-derive fix-it that inserts `@derive(Tr)` with any missing law
  partners, when the leaf's type is the user's own and `Tr` has a
  template;
- near-miss messages: "`Shelf` implements `Store[mut User]`, not
  `Store[User]`" ([`trait.target.inner-mut.distinct`](../../spec/lang/09-traits.md#r-trait.target.inner-mut.distinct));
  the `use` fix-it of [`trait.avail.suggest`](../../spec/lang/09-traits.md#r-trait.avail.suggest);
  "`Money`'s `Add` gives `Out = i64`, but `Summable` requires
  `Out = Money`";
- overflow messages that show the first three goals of the chain and the
  last.

### 10.4 One Diagnostic Per Root Cause

The design's root key for a solver failure is the body plus the leaf
goal's canonical key: ten `==` uses on a `Point` without `Eq` in one body
give one error at the first use, with a note "and 9 more uses in this
body". Planned, not built: today each use reports.

A stalled goal still open at the end of the body becomes
`cannot-infer-type`.

## 11. Determinism And Parallelism

**Rule TS-8. Every order the solver shows comes from content.**

- Candidates come in table order (the asking module's own table, then
  folders by id, which follows the folder table's order) and, within a
  table, in row order, which is content rank: the module's rank in
  stable-path order, then the item index in source order. An item index
  does not move when a body above the impl is edited.
- Elaborated clauses are in declaration order, supertraits depth first in
  declared order. A `Bound` index is therefore the same on every run.
- Placeholders are numbered by first occurrence in a fixed walk.
- The bound plan runs in its stored order, so the first failing step is
  fixed.

**Purity and sharing.** Answers are functions of their keys (rule TS-1),
and fuel is charged per distinct proof node (rule TS-5), so a warm memo, a
shuffled check order or another thread count changes no answer and no fuel
charge. The only shared mutable state is the global memo, written
first-writer-wins, and the interners. Impl tables, the candidate directory
and environments are frozen and read through `&`.

## 12. Data Structures

| Structure | Where | Lifetime |
| --- | --- | --- |
| `ImplTable` | one per folder interface, and one for a module's own rows | the folder interface; the body task |
| `FolderImpls` (`arg_impls`, `unowned`) | per folder, built on first use | the run |
| `UniverseImpls` (unowned rows, directory) | per impl universe | the run |
| `ParamEnv` | per item, interned by `EnvKey` | the body; keys for the run |
| `MemoEntry` (12 B) and `MemoRecord` | global or body memo | global: the run; body: the body |
| `Frame` (24 B) | defined but unused; the search keeps a stack of memo keys | one `solve` call |
| tuple-template plans | per template, cached by the driver | the run |

## 13. Performance Targets And Pathological Cases

**Targets** (not measured or gated yet):

| Measure | Target |
| --- | --- |
| memo hit, global or body | under 100 ns |
| a computed goal on the fast path (section 3.3) | under 1 µs |
| a computed goal with three bound steps | under 5 µs |
| solver share of check time on std and the generated packages | under 10% |

**Pathological cases** and what bounds them:

| Case | Danger | What bounds it |
| --- | --- | --- |
| 1,600 `Cents::from(N)` over two `From` impls | a search per call | one `Instantiations` entry, then hits |
| 500 `From[X]` impls for one error type | `Instantiations` returns 500 candidates | the checker's prefilter by argument head |
| a supertrait chain 200 deep | elaboration per goal | elaboration once per item |
| diamond supertraits | duplicate clauses | one clause per trait reference |
| an associated-type chain 60 deep | normalization blowup | each normalization proves a memoized `Implements` goal |
| a recursive impl | a cycle | cycle detection on the stack, `Overflow` at the first repeat (rule TS-7) |
| a goal whose trait arguments grow on each step | unbounded search | depth 64 |
| `==` on a 1,000-element tuple | per-element search | the tuple template's plan: one step per element, memoized per element type |
| many bodies asking the same goals | repeated work | the global memo |
| a method name declared by 50 available traits | 50 goals per call | the method index; one memoized `Instantiations` per trait and receiver |

## 14. Testing

### 14.1 Spec Traceability

| Spec area | Component |
| --- | --- |
| [Supertraits](../../spec/lang/09-traits.md#supertraits), [Supertrait Bindings](../../spec/lang/09-traits.md#supertrait-bindings) | `hd_check::body::add_bound`; `hd_check::header` |
| [Associated Types](../../spec/lang/09-traits.md#associated-types), [Associated Type Bindings](../../spec/lang/09-traits.md#associated-type-bindings) | `hd_check` normalization; `hd_resolve::assoc` |
| [Implementation Targets](../../spec/lang/09-traits.md#implementation-targets), [Implementation Ownership](../../spec/lang/09-traits.md#implementation-ownership) | resolution header checks; `hd_types::lookup` |
| [Overlap](../../spec/lang/09-traits.md#overlap) | `hd_resolve::Universe::overlaps`; bound plans |
| [Derived Implementations](../../spec/lang/09-traits.md#derived-implementations), [Templates](../../spec/lang/14-annotations.md#templates), [Derived Bounds](../../spec/lang/14-annotations.md#derived-bounds) | derived heads; `hd_check::derive` |
| [Method Resolution](../../spec/lang/09-traits.md#method-resolution), [Trait Availability](../../spec/lang/09-traits.md#trait-availability) | `Search::methods`; `hd_check::MethodIndex` |
| [Instantiations Of One Generic Trait](../../spec/lang/09-traits.md#instantiations-of-one-generic-trait), [Inference Through A Bound](../../spec/lang/04-type-system.md#inference-through-a-bound) | `Search::instantiations`; `hd_check::trial` |
| [Generic Bounds And Static Dispatch](../../spec/lang/09-traits.md#generic-bounds-and-static-dispatch), [Mutable Bounds](../../spec/lang/04-type-system.md#mutable-bounds) | `Search::implements`, the memo (depth) |
| [Dynamic Trait Values](../../spec/lang/09-traits.md#dynamic-trait-values) | resolution (per-member availability); `trait_value_holds` |
| [Sealed Traits](../../spec/lang/09-traits.md#sealed-traits), [Runtime Type Identity](../../spec/lang/09-traits.md#runtime-type-identity), [`AnyVal` And `AnyRef`](../../spec/lang/04-type-system.md#anyval-and-anyref) | `hd_types::sealed` |
| [Trait Delegation](../../spec/lang/09-traits.md#trait-delegation) | delegated heads; `hd_check::delegate` |
| [Error Derivation](../../spec/lang/14-annotations.md#error-derivation) | generated heads; `hd_check::error` |
| [Tuple Templates](../../spec/lang/14-annotations.md#tuple-templates) | tuple-template plans (`MemberStep`) |

### 14.2 Test Kinds

| Test | What it checks |
| --- | --- |
| conformance fixtures (`spec/conformance`) | accept or reject and the code of each diagnostic, through the portable command contract; failures in `KNOWN_FAILURES.tsv` |
| solver unit tests (`hd_types/src/solver.rs`) | head index, commit and stall, memo publication, heights and cycles, the per-body charge, sealed rows, tuple templates, instantiation schemes |
| memo invariance (`crates/hd_driver/tests/all/impl_universe.rs`) | two modules asking one open goal with different closures get their own answers, whichever runs first and on any thread count |
| driver tests (`header_goals.rs`, `sealed_rows.rs`, `trials.rs`, `delegation_run.rs` and others) | header goals, compiler-supplied rows, the instantiation choice, delegation |

Planned, not built: an impl-set fuzzer against a naive recursive solver;
a coherence fuzzer that enumerates ground types; a select-soundness check
that re-solves at every instance; the pathological suite of section 13 at
sizes `n` and `2n`, failing above an observed exponent of 1.2; a
one-root-cause fuzzer.
