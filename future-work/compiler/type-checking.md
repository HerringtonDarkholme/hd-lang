# New Compiler: Type Checking

Status: Design, not decided. Frontend lane, 2026-10-07.

This document details the body checker of the new compiler. It refines
these sections of [COMPILER_DESIGN.md](design-overview.md) (part D1):

- [§3 Core Data Structures](data-structures.md#3-core-data-structures):
  IDs, interners, types, arenas, the poison type, diagnostics;
- [§4.9 Name Resolution](resolution-and-interfaces.md#49-name-resolution) and
  [§4.10 Folder Interface Construction](resolution-and-interfaces.md#410-folder-interface-construction):
  what the checker receives;
- [§4.12 Traits, Impls And Coherence](resolution-and-interfaces.md#412-traits-impls-and-coherence):
  what the checker asks the trait solver;
- [§4.13 Body Checking](checking-and-tir.md#413-body-checking): the three
  per-module phases M1, M2 and M3, which this document replaces in detail;
- [§4.14 Diagnostics](checking-and-tir.md#414-diagnostics) and
  [§4.15 Limits](checking-and-tir.md#415-limits);
- [§6 Scheduler](scheduler.md#6-scheduler-and-task-graph) and
  [§8 Determinism And Soundness Tests](testing-the-compiler.md#8-determinism-and-soundness-tests).

The trait solver gets its own design, `trait-solver.md`, after this one.
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
   ([§4.13.11](checking-and-tir.md#41311-the-typed-ir-tir)). This
   document says what the checker needs from the builder and what it
   promises in return (section 1.5).
2. **Data-oriented storage.** Types live in an InternPool as a tag, a
   data word and `extra` words; TIR is columns; rollback is truncation of
   append-only columns plus the inference trail
   ([§3.9](data-structures.md#39-data-oriented-encoding),
   [§3.10](data-structures.md#310-ir-abstraction-contracts)). This
   design reads types through an abstract accessor API (section 1.4),
   which the pool's generated views satisfy, and uses D2's one rollback
   protocol (section 3.5).

## 1. Scope And Interfaces

### 1.1 Who Owns What

| Component | Owns | Does not own |
| --- | --- | --- |
| Resolution (`hd_resolve`, before) | uses, module scope, paths in headers, folder interfaces, impl tables, derived heads, header checks | anything in a body except path lookup through the frozen scope |
| **Type checker (`hd_check`, this document)** | name lookup of locals, every expression, statement and pattern in a body; inference variables; coercions; rows used by bodies; suspension checks; GADT refinement; exhaustiveness and decision trees; mutability; definite initialization; omitted results and rows; body diagnostics; calls to the TIR builder | trait impl search, impl matching, coherence, TIR encoding, instantiation |
| Trait solver (`hd_types::solve`, beside) | goals of the form "type implements trait", projections, the instantiations of a generic trait, method candidates, its memo | inference variables (it reads them, never writes them), diagnostic text |
| TIR builder (D2's `hd_tir`, called by the checker) | the instruction encoding, scratch checkpoints, the final type sweep, the TIR verifier | types, names, any decision the checker makes |
| Collection and emission (D2, after) | instances, layout, Wasm | re-deriving anything TIR states |

**Rule TC-1. The checker decides; the builder records.** Every semantic
choice is made in the checker: which callee, which impl or bound, which
coercion, which literal width, which arm is impossible. TIR states it.
D2 never consults names, scopes or the solver to reconstruct a choice.

### 1.2 Inputs

All inputs are frozen before a body task starts. A body task reads them
through shared references and never locks
([§3.5](data-structures.md#35-arenas-and-lifetimes)).

| Input | Producer | Used for |
| --- | --- | --- |
| `ModuleScope` | `ModulePrep` (M1) | module names, uses, prelude, local impl table |
| `FolderIface` of every used folder | `FolderIface` tasks | signatures, fields, variants, bounds, rows, templates |
| `PrivateSigs` of the module | M1 | private headers lowered to types; inferred results (M1) |
| impl tables | resolution | through the solver only |
| body syntax | parser | the green tree and its typed views |
| `CheckConfig` | driver | fuel per body, limits, emit mode, tests overlay on or off |

### 1.3 Outputs

```rust
/// What one body check produces. Owned by the body task until ModuleFinish.
pub struct BodyResult {
    pub item: BodyKey,                    // (module, item index) or init body or derive instance
    pub diags: Vec<Diagnostic>,           // unsorted; ModuleFinish sorts (§12)
    pub result_ty: Option<Ty>,            // M1 bodies only: the inferred result
    pub row_facts: RowFacts,              // M3 input (§5.5)
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
([Lifetime And The `tir` Entry](checking-and-tir.md#lifetime-and-the-tir-entry)).

### 1.4 The Type Accessor API

The checker reads and builds types only through this API. D1 §3.4
stores types in the InternPool and decodes them through generated views
([§3.9.6](data-structures.md#396-one-schema-generated-accessors));
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
    Assoc { base: Ty, trait_: DefId, name: Symbol },
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
  ([Views](../../spec/lang/04-type-system.md#views)). D1's `Readonly(Ty)`
  is inverted; see section 17.
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
([§4.13.11](checking-and-tir.md#41311-the-typed-ir-tir)). The checker
is the builder's only caller. This section lists what the checker relies
on, and what it promises in return.

**What the checker uses.**

| Builder operation | Checker use |
| --- | --- |
| `konst`, `local`, `get`, `set`, `prim`, `emit` | every expression, in evaluation order |
| `call` with a `Callee` record and `Providers` | every resolved call; providers in key order, or `Pending(row variable)` for a private callee whose row M3 solves (section 5.5) |
| `await_` | bang calls, `s!()`, `all!`, `race!`; the builder records the enclosing scope chain |
| `coerce` | each coercion found by `coerce` (section 4.2) |
| `open_block`/`close_block`, `open_scope`/`defer`/`close_scope`, `open_loop`, `sub_body` | every block, cleanup scope, loop and closure, opened and closed in source nesting |
| `checkpoint`, `rollback` | every trial (section 3.5) |
| `finish(solution)` | once per body, at its end: the final type sweep and, in debug builds, the verifier |

**What the checker promises.** These are the invariants of §4.13.11
seen from the caller's side.

1. **Calls are resolved.** A `Callee` is an item with its type
   arguments, a trait method with its choice (the impl's `DefId` or the
   bound's index), or stored evidence. Operators, interpolation,
   indexing, iteration, `?`, pipes, compound assignment, comprehensions
   and default arguments arrive desugared, as D2's desugaring table says.
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
([§3.10.2](data-structures.md#3102-lifetimes-sizes-and-peak-memory)).
This design keeps that default and adds one mode:

| Mode | Used by | TIR |
| --- | --- | --- |
| emit (default) | `hd check`, `hd build`, `hd run`, `hd test` | built per body in the worker's columns; moved to the module result; serialized and freed at `ModuleFinish` |
| emit and verify | debug builds of `hd`, `--verify`, CI on std | as above, plus the verifier at each `finish` |
| no-emit (mine) | the playground's check-as-you-type, `hd fix` rounds, `hd doc` | never built; the checker is generic over the builder (`Checker<B: TirSink>`), and the discarding sink's methods are no-ops that return dummy handles |

The no-emit mode saves about 200 bytes per source line of transient
memory and the emission time where no build can follow. Its `check`
entry is written without a `tir` entry, so a later build rechecks that
module. Section 16 question 5 asks whether the mode is worth having.

### 1.6 The Trait Solver Interface

The checker asks the solver four kinds of goals. The solver never
writes the checker's state, never reports a diagnostic, and never sees
spans.

```rust
pub enum Goal {
    /// `ty: Trait[args, bindings]`.
    Implements { ty: Ty, trait_: DefId, args: TyList, bindings: AssocList },
    /// Normalize `<base as Trait[args]>::name`.
    Project { base: Ty, trait_: DefId, args: TyList, name: Symbol },
    /// Every instantiation `Trait[A..]` that `ty` implements.
    /// For bound-only parameters and for choosing among instantiations.
    Instantiations { ty: Ty, trait_: DefId },
    /// The method candidates named `name` for a receiver type,
    /// inherent first, then available traits.
    Methods { receiver: Ty, name: Symbol },
}

pub enum Answer {
    Holds { evidence: Evidence, learned: SmallVec<[(InferVar, Ty); 2]> },
    Many(SmallVec<[Candidate; 4]>),          // Instantiations and Methods only
    Fails(FailInfo),                         // closest impl, failed subgoal, for the message
    Stalled { on: SmallVec<[InferVar; 2]> }, // needs more inference first
    Overflow,                                // trait-resolution-depth
    OutOfFuel,
}

pub enum Evidence {
    Impl { def: DefId, args: TyList },       // a written, derived or template impl
    Bound { param: ParamRef, index: u16 },   // from the parameter environment
    Builtin(BuiltinImpl),                    // tuples at every arity, numeric families, Fn
    Coinductive,                             // a derived impl's own member check
    Poison,
}

pub struct SolveCx<'a> {
    pub env: &'a ParamEnv,          // declared bounds plus GADT arm equalities (§6.1)
    pub infer: &'a dyn InferRead,   // shallow resolution of inference variables
    pub avail: AvailKey,            // which traits are available in this module
    pub local_impls: &'a LocalImpls,
    pub memo: &'a mut BodyMemo,     // per-body memo for goals with variables; owned by BodyCx
}

pub trait Solver: Sync {
    fn solve(&self, cx: &mut SolveCx<'_>, goal: Goal, fuel: &mut Fuel) -> Answer;
}
```

**How the checker uses answers.**

- `Holds` with `learned` bindings: the checker applies each binding
  through its own unifier, on its own trail (section 3.5). The solver
  stays pure, so its answers can be memoized.
- `Stalled`: the checker keeps the goal as an **obligation** and watches
  the listed variables. It asks again when one of them is bound, and at
  the latest at the end of the statement or the body (section 2.7).
- `Fails`: the checker reports the error, once per root cause, with
  `FailInfo` for the message.
- `Overflow` and `OutOfFuel`: the checker reports the limit diagnostic
  and poisons the expression.

**What the checker sees of caching and budgets.**

- **Memo.** The solver canonicalizes a goal (inference variables
  renumbered by first occurrence) and memoizes it: globally when it has
  no variables and no arm equality, per body otherwise
  ([§4.12.2](resolution-and-interfaces.md#4122-solving)). The checker never
  reads or writes memo entries itself.
- **Fuel.** The checker passes its body's `Fuel`. The solver charges one
  step per candidate tried and per subgoal expanded. A memo hit charges
  the steps stored with the entry. So the fuel a goal costs is the same
  on every run, thread count and cache state (lesson 3 of
  [Lessons For hd](prior-art-issues.md#lessons-for-hd)).
- **Depth.** The solver counts nesting itself and answers `Overflow` at
  `trait-resolution-depth`.
- **Stalled goals are not memoized globally**, since their answer depends
  on the body. The per-body memo lives in the checker's `BodyCx` and is
  lent to the solver for each call, so the solver itself holds no
  per-body state.

### 1.7 Body Tasks And The Exactly-Once Rule

D1's phases stay. This design adds one rule.

| Phase | Task | Bodies checked here |
| --- | --- | --- |
| M1 | `ModulePrep(m)`, serial | the module's top-level statements; every non-public function and inherent method whose result type is omitted; depth first (section 9) |
| M2 | `Body(m, i)`, parallel | every other body: functions and methods with written results, impl members, derive instances, test bodies, fact and default expressions |
| M3 | `ModuleFinish(m)`, serial | no body: the row solve, deferred row checks, init summary, diagnostic sort |

**Rule TC-4. Each body is checked exactly once per run.** A body checked
in M1 for its result type is not checked again in M2: its `BodyResult`
is final. Rows never cause a recheck (section 5.5). Literal widths never
cause a statement recheck (section 3.6). The only repeated work is a
speculative trial (section 3.5), which is bounded and rolled back.

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
| `x is y` | infer both | none | identity rules |
| `_` | check | records `want` for the hole diagnostic | `placeholder-outside-pipe` (section 10.4) |

### 2.3 Statements And Blocks

1. Statements are checked in source order. Each statement is a **literal
   scope**: open literal variables still unbound at its end take their
   default ([`types.literal.local.statement`](../../spec/lang/04-type-system.md#r-types.literal.local.statement)).
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
   A missing argument with a default is filled by a `Default`
   instruction.
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
7. **Bound-only parameters.** After the other parameters, each bound-only
   parameter asks `Instantiations` of its bounded parameter's type, and
   solves when exactly one instantiation fits. The step repeats until
   nothing changes
   ([`types.generic.infer.bound.fixed-point`](../../spec/lang/04-type-system.md#r-types.generic.infer.bound.fixed-point)).
   The repeat count is at most the number of bound-only parameters.
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
For each candidate, in the solver's content order, the checker checks the
arguments against the candidate's parameters inside a trial
(section 3.5) and rolls back. A candidate fits if the trial has no
error. Then:

- one fit: check the call again for real against it;
- several fits, and they differ only in the width of an open literal
  argument: default the literal and choose again
  ([`trait.resolve.literal-arg`](../../spec/lang/09-traits.md#r-trait.resolve.literal-arg));
- several fits otherwise: `ambiguous-method`;
- none: `type-mismatch` listing the instantiations.

Trial cost is bounded. Arguments that check the same way for every
candidate (they have no expected-type dependence) are inferred once
before the trials, and a trial only unifies their types. Only postponed
arguments (step 4 above) are checked once per candidate. Every trial
charges fuel. So a nested chain of such calls costs at most
candidates × postponed-argument size per level, and fuel stops a
pathological program with `item-too-complex`.

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

### 2.6 Closures

1. With `Expect::Fn`, unannotated parameters take the expected parameter
   types, and the result is checked against the expected result.
2. Without it, every parameter needs an annotation:
   `closure-parameter-needs-annotation`.
3. An omitted result is the LCT of the final value and every `return`
   operand (section 4.3).
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
| literal (integer or float) | as soon as it meets a concrete numeric type; else at the end of its statement, by default | never open past its statement |
| use-site type argument | when unified; else at the end of the call's statement, after defaults | `cannot-infer-type` or `ambiguous-type` |
| local binding's type | from its initializer, in its statement | an empty collection literal can leave its element open to the end of the body (section 2.8) |
| closure parameter | from `Expect::Fn` or the annotation | `closure-parameter-needs-annotation` |
| omitted private result | at the end of its body in M1 | never open |
| private row variable | in M3 | never open |

**Obligations.** A stalled goal goes on the body's obligation list with
the variables it waits on. Each variable has a watch list (section 13).
Binding a variable wakes its watchers. Woken obligations are retried at
the next statement boundary, and before any step that needs their
answer, such as method lookup on a stalled receiver. They are retried in
creation order, so the result does not depend on which binding woke them.

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
([CA-05](../../audit/compiler/findings-2026-10-04.md#ca-05-unequal-rollback-contracts)).
D2 fixed one protocol for every IR builder: a checkpoint is a tuple of
column lengths, and rollback truncates them all, plus the inference trail
([§3.9.5](data-structures.md#395-building-scratch-buffer-checkpoints-truncation)).
The checker uses exactly that protocol.

**Rule TC-5. One rollback contract.** Every piece of body state that a
check can change is one of:

1. an **append-only column**: TIR instructions, `extra` and locals
   through the builder; the scratch buffer; the local pool; buffered
   diagnostics; obligations; row facts; init facts; or
2. a **slot written only through the trail**: union-find parents and
   ranks, bindings, variable kinds, GADT arm equalities, local flags, the
   definite-assignment bits.

A checkpoint records each column's length and the trail's length.
Rollback pops the trail, restoring each old value, and truncates each
column. Nothing else in a body is mutable, and no checker state is ever
cloned.

```rust
#[derive(Copy, Clone)]
pub struct Checkpoint {                  // the checker's part; `tir` is D2's (§3.9.5)
    trail: u32, diags: u32, obligations: u32, local_pool: u32,
    row_facts: u32, init_facts: u32, tir: tir::Checkpoint,
}

enum Undo {                              // 8 bytes: tag + index; old value in `trail_old`
    Parent(InferVar), Rank(InferVar), Bind(InferVar), Kind(InferVar),
    ArmEq(u32), LocalFlag(LocalId), Assigned(LocalId),
}

impl<B: TirSink> Checker<'_, B> {
    fn trial<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> (R, bool /* had error */) {
        let c = self.checkpoint();
        let errors_before = self.error_count;
        let r = f(self);
        let failed = self.error_count > errors_before;
        self.rollback(c);
        (r, failed)
    }
}
```

- **Path halving writes too.** `find` shortens paths, which writes
  parents. Inside a trial those writes go on the trail like any other.
  When no trial and no GADT arm is open, the trail is cleared at each
  statement boundary, since nothing can roll back past that point, so
  halving costs nothing extra there.
- **The per-body solver memo is not rolled back, and need not be.** Its
  keys are canonical goals with variables already resolved, so an entry
  stays true after a rollback. Fuel is charged on every hit, so keeping
  it changes no budget (TC-8).
- **Users of `trial`:** the expected-result attempt (section 2.4), the
  instantiation choice (section 2.5), LCT joins (section 4.3), key
  collision checks (section 5.2), and hole candidates (section 10.4). GADT
  arms use a checkpoint with a scoped pop (section 6.1). No other code
  rolls back.
- **Trials nest, within fuel.** Every trial charges a fixed cost plus the
  steps it takes, so fuel bounds the product of nested trials.
- **Debug check (mine).** In debug builds, `rollback` compares a hash of
  the inference table, flags and column lengths with the hash taken at
  the checkpoint. A mismatch is an internal error naming the trial's
  span. This is the verifier for TC-5.

### 3.6 Literal Widths

This follows [Open Literal Width](../../spec/lang/04-type-system.md#open-literal-width)
and the plan in [Literal Inference](goals.md#literal-inference).

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

**No statement retry (mine, needs confirming).** The spec permits a
checker to meet [`types.literal.local.join`](../../spec/lang/04-type-system.md#r-types.literal.local.join)
by re-checking a failed statement once per width its failure names. The
prototype did that, with a weaker rollback and with widths scraped from
diagnostic text. With classes, a literal binds to the width it meets the
moment it meets it, so the default is used only when nothing decides the
class. The claim is that this gives the spec's result for every form in
the spec's table. The one place it may differ is a statement whose
literals form two or more separate classes meeting different widths;
section 16 question 1 asks which reading the spec means. A test-only
**oracle mode** implements the spec's retry rule literally, over the same
trail, and the differential fuzzer compares the two (section 14).

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
| `Variance` | `C[A]` → `C[B]` by declared variance | **missing** (section 17) | none, by [Representation-Preserving Variance](../../spec/lang/04-type-system.md#representation-preserving-variance) |
| `WrapSome` | `T` → `T?` | option wrap | build `.Some` |
| `RowSubsume` | `fn ... $ R1` → `fn ... $ R2` | row subsumption | an adapter that passes only `R1`'s providers |
| `ToTraitValue` | `S` → `Tr`, `mut S` → `mut Tr`; `Any` included | to trait value, to `Any` | box with its dispatch table; the impl choice is in the record |
| `Supertrait` | child trait value → parent trait value | **missing** (section 17) | re-table |
| `SuspendFnToCtor` | `fn!` type → constructor type | suspending function to constructor | none, or a thin adapter |

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
   joined type, as a `Coerce` instruction.

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
pub struct PendingRow { var: RowVar, minus: RowId }   // RowVar(f) minus these keys
```

- Keys are sorted by `Ty` value for in-run merges. They are re-sorted by
  stable content for printing and hashing (D1 §4.13.4).
- Entailment is membership after alias expansion
  ([Entailment](../../spec/lang/11-requirements-and-suspension.md#entailment)).
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
- A body with an inferred row (a closure or a private function) records
  each key it uses outside its own `$.with` blocks as a row fact.

### 5.3 Call Checking

For a call to a callee whose substituted row is `R`:

| `R` holds | Check |
| --- | --- |
| concrete keys | each key in `available`, else `missing-requirement` naming the key; when the caller is a function whose header the user wrote, a fix-it adds the key to that header |
| a row parameter `$R` of the callee | `R` was solved from an argument's function type (least solution, section 5.6), then as above |
| the caller's own row parameter | entailed only if the caller's row lists it |
| `RowVar(g)` of a private callee | a deferred check, recorded as a row fact (section 5.5) |

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

### 5.5 Private Rows And The M3 Fixpoint

A non-public function, method or local `fn` without a `$` clause has an
inferred row
([`req.row.omitted.inferred-private`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.inferred-private)).
Rows never order checking (D1 §4.13.1): bodies record facts, and M3
solves them.

```rust
pub enum RowFact {
    Uses     { f: RowVar, keys: RowId },                         // f's body uses keys
    Includes { f: RowVar, g: RowVar, minus: RowId, at: Span },   // f calls g inside with-blocks for `minus`
    Entails  { avail: RowId, params: ParamSetRef, g: RowVar, minus: RowId, at: Span },
}
```

An `Entails` fact comes from a call in a body whose row is written, or
from a subsumption against a known row.

**The solve, in M3, per module:**

1. Build the graph of row variables from the `Includes` facts.
2. Compute its SCCs with Tarjan's algorithm, visiting nodes in source
   order, so the SCC order is content order.
3. In reverse topological order, solve each SCC as a least fixpoint.
   Start from the union of its `Uses` keys. Then add `row(g)` minus
   `minus` into `row(f)` for each `Includes` edge into the SCC, until
   nothing changes
   ([`req.row.omitted.cycle`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.omitted.cycle)).
4. Rows only grow, and each row holds at most the module's distinct
   keys. So the loop ends after at most `keys × variables` additions per
   SCC, and in practice after one pass per key. It is serial, monotone
   and bounded (lesson 7 of the prior-art lessons).
5. Run each `Entails` check against the solved rows. A missing key is
   `missing-requirement` at the call. A note names the callee chain that
   brought the key in: each row keeps, per key, the first edge that
   added it.
6. Fill the calls' pending providers. A call to a private callee was
   emitted with `Providers::Pending(row variable)`. M3 now writes one
   provider per key of the solved row, which is one of the two in-place
   patches D2 allows
   ([§3.9.5](data-structures.md#395-building-scratch-buffer-checkpoints-truncation)).
   After it, no call is pending (TIR invariant 8).

No body is checked again (TC-4). The prototype rechecked every relevant
body in each row round.

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
yet. This design reports that case as `cannot-infer-type`, with a fix-it
that writes the private callee's `$` clause. Section 16 question 2 asks
the owner to confirm.

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
item 6). Section 16 lists the spec text that still says "transitive".

## 6. GADT Refinement And Tuples

### 6.1 Arm-Local Equalities

For a `match` on `E[S1..Sn]` and an arm whose variant result is
`E[R1..Rn]` ([Refinement Algorithm](../../spec/lang/13-gadts.md#refinement-algorithm)):

1. Take a trail mark for the arm.
2. Give each variant-local parameter that does not occur in the result a
   fresh **rigid** placeholder (an existential). Give each one that
   occurs a fresh variable.
3. Unify each `Si` with `Ri`, first-order and nominal, after alias
   expansion. Where `Si` is a rigid parameter `T` of the enclosing
   declaration, record an **arm equality** `T ≡ Ri` instead of failing.
4. A contradiction makes the arm impossible: two different nominal
   heads, or `T ≡ A` and `T ≡ B` with `A ≠ B`. That is
   `impossible-gadt-pattern` on the arm, and exhaustiveness skips the
   variant.
5. Check the arm's patterns, guard and body with the equalities active.
   Shallow resolution of a rigid parameter consults the equality table,
   and the solver sees the equalities through `ParamEnv` (section 1.6).
6. Check the arm's result against the match's type. A result type that
   mentions an existential is `type-mismatch`
   ([`gadt.existential.no-escape`](../../spec/lang/13-gadts.md#r-gadt.existential.no-escape)).
7. At the arm's end, pop the equalities. Bindings of ordinary variables
   made in the arm stay, unless they mention a refined parameter's
   equality or an existential. Such a binding is `type-mismatch`, since
   the refinement would escape
   ([`gadt.check.no-escape`](../../spec/lang/13-gadts.md#r-gadt.check.no-escape)).

Step 7 needs a **scoped pop**: it undoes only `ArmEq` entries above the
mark and keeps `Bind` entries. The escape check scans the `Bind` entries
above the mark, which are exactly the bindings the arm made. Nested GADT
patterns compose their equalities within one arm
([`gadt.unify.nested`](../../spec/lang/13-gadts.md#r-gadt.unify.nested)).

An existential with bounds carries its evidence in the value
([`gadt.runtime.evidence`](../../spec/lang/13-gadts.md#r-gadt.runtime.evidence)).
A trait call through the existential is emitted with TIR's `Evidence`
callee, which names the matched value, the bound and the method.

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
- `Tuple`, and `Eq`, `Ord` and `Hash` for tuples, are compiler-supplied
  at every arity. The solver answers them with `Evidence::Builtin`.
- `Fn[Args, O, $ R]` unifies `Args` with a function type's parameter
  tuple.

## 7. Patterns And Exhaustiveness

### 7.1 Pattern Typing

Patterns are checked against the scrutinee's type with no coercion
(`check_exact`):

| Pattern | Rule |
| --- | --- |
| `_`, a bare name | catch-all; a name binds the subject's type; a name that resolves to a variant of the subject enum is `bare-variant-pattern` |
| `.V(p..)`, `E.V(p..)` | the subject's enum must be known (`missing-contextual-enum-type`); payload arity (`pattern-arity`); named payloads; GADT refinement (section 6.1) |
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
2. The constructors of a type: the variants that can inhabit the subject
   (GADT-impossible ones removed); `true` and `false`; one tuple or data
   constructor; `()`; and for integers, **disjoint ranges** split from
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

### 8.3 Local Flags

Each local has a flags byte, written through the trail: `READ`,
`ASSIGNED`, `MUTATED`, `CAPTURED`, `CAPTURED_ASSIGNED`. With no extra
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
([§4.13.10](checking-and-tir.md#41310-module-initialization)).

## 9. Omitted Result Types (M1)

### 9.1 Who Can Omit

A non-public function, a non-public inherent method and a local `fn` may
omit `-> T`
([`fn.decl.result-omitted-private`](../../spec/lang/07-functions.md#r-fn.decl.result-omitted-private)).
Public functions, trait methods and impl methods cannot
(`missing-result-type`, a header error). So an omitted result never
reaches a folder interface, and inference never enters a dependent's
cache key.

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
  type is reported like a function cycle (section 16 question 3).
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
([CA-07](../../audit/compiler/findings-2026-10-04.md#ca-07-recovery-and-failure-boundaries)).
A Rust panic in the checker is a compiler bug. The scheduler catches it
at the task boundary and reports an internal error naming the item
([§6.4](scheduler.md#64-budgets-cancellation-and-the-memory-cap)).

### 10.2 Poison

The poison type follows [§3.6](data-structures.md#36-the-poison-type):

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
afterwards, so it causes nothing else. The design list's `todo()` has no
std function today; section 16 question 4 asks.

### 10.5 Fix-Its For Common Mistakes

The writing log (`audit/hd-writing-log.md`) and the
[diagnostics wishlist](../../audit/diagnostics-wishlist-2026-10-06.md)
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
| a solver candidate tried, a subgoal expanded | 1 (charged by the solver) |
| a solver memo hit | the steps stored with the entry |
| a trial started | 8, plus its steps |
| a usefulness matrix cell visited | 1 |
| an LCT join step | 1 |
| an obligation retried | 1 |

The default is 2,000,000 steps per body (D1 §4.15). Fuel is per body, so
one item's complexity never fails another item.

**Rule TC-8. Fuel used is a pure function of the body and its frozen
inputs.** Memo hits charge their stored cost, and trials are charged
whether or not they commit. So a body passes or fails its budget the
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
| nested instantiation choices, `a.add(b.add(c.add(...)))` | trials multiply | arguments inferred once outside trials; only postponed arguments retried; fuel |
| a type that grows on each step, `List[List[...]]` built by inference | memory | type size limit at `mk` |
| many obligations waiting on one variable | quadratic waking | watch lists; each obligation wakes once per binding of a variable it waits on |
| a long chain of private functions in M1 | native recursion | the explicit M1 stack |
| huge rows | quadratic set operations | sorted merges, linear |
| an ill-typed version of each of the above | Swift's slowest cases are errors | poison stops cascades, and the pathological suite runs every case ill-typed too (section 14) |

## 12. Determinism And Parallelism

**Rule TC-9. No shared mutable checker state.** A body task owns one
`BodyCx`. It reads frozen inputs through `&` and writes only:

- its own `BodyCx` and arena;
- the append-only global interners (types, rows, strings), whose IDs
  never reach output (D1 §3.1);
- the solver's global memo, whose entries are pure functions of frozen
  inputs, written first-writer-wins.

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
    tir: B,                             // the TIR builder, or the discarding sink
    fuel: Fuel,
    // inference table, SoA, indexed by InferVar
    parent: Vec<u32>,                   // 4 B; union-find parent (self = root)
    rank: Vec<u8>,                      // 1 B
    kind: Vec<VarKind>,                 // 1 B; General | IntLit{signed} | FloatLit
    value: Vec<Ty>,                     // 4 B; Ty::NONE when unbound
    blame: Vec<SpanIdx>,                // 4 B; first deciding span
    watch_head: Vec<u32>,               // 4 B; first obligation waiting on this var
    // trail
    trail: Vec<Undo>,                   // 8 B each
    trail_old: Vec<u32>,                // 4 B; old values for Parent/Rank/Bind/Kind
    // obligations, SoA
    ob_goal: Vec<Goal>,                 // 16 B
    ob_span: Vec<SpanIdx>,              // 4 B
    ob_next_watch: Vec<u32>,            // 4 B; next obligation on the same var
    ob_state: Vec<u8>,                  // 1 B; Pending | Woken | Done
    // locals, SoA, indexed by LocalId
    local_name: Vec<Symbol>, local_ty: Vec<Ty>, local_span: Vec<SpanIdx>,
    local_flags: Vec<u8>,               // READ, ASSIGNED, MUTATED, CAPTURED, ...
    // scopes and contexts
    scopes: Vec<Scope>,                 // name -> LocalId chain, ~16 B per scope
    loops: Vec<LoopCx>,                 // break type slot, label, ~12 B
    fns: Vec<FnCx>,                     // result slot, row, driver flag, ~24 B
    avail: Vec<RowId>,                  // the available stack (§5.2)
    restricted: u16,                    // §5.8 depth
    literal_scope: Vec<InferVar>,       // open literal classes of the current statement
    assigned: BitStack,                 // definite assignment sets (§8.4)
    // outputs, append-only
    diags: Vec<Diagnostic>,
    row_facts: Vec<RowFact>,
    init_facts: Vec<InitFact>,
    arm_eqs: Vec<(ParamRef, Ty)>,       // §6.1, trail-managed
    // reusable scratch, cleared per use, never shrunk within a body
    scratch_tys: Vec<Ty>, scratch_refs: Vec<Ref>, scratch_args: Vec<ArgSlot>,
    memo: BodyMemo,                     // lent to the solver per goal (§1.6)
    spine: Vec<ExprId>,                 // §11.3 spine iteration
}
```

- **Inference table:** 18 bytes per variable. A typical body has tens to
  hundreds of variables, so the table fits in a few cache lines.
- **Trail:** 8 bytes per entry, plus 4 for an old value. Entries exist
  only while a trial or a GADT arm is open, or since the body's start
  when no trial is open; the trail is cleared at each statement boundary
  outside trials, since nothing can roll back past a finished statement.
- **No expectation stack.** The expected type is an argument of `check`,
  passed by value. The context stacks (`scopes`, `loops`, `fns`,
  `avail`) are the only stacks, and they are vectors.
- **Scratch buffers** are owned by `BodyCx` and reused, so steady-state
  checking allocates only for outputs.
- **Body-local types** live in the body-local pool
  ([§3.9.2](data-structures.md#392-the-internpool)). At body end the
  checker resolves every type in its outputs, and TIR's `finish` sweeps
  the `ty` column; both intern the variable-free results globally.
- **Budget check.** The inference table and obligations add a few dozen
  bytes per variable and per goal, beside TIR's 26 bytes per instruction
  ([§3.9.7](data-structures.md#397-byte-budgets-and-linear-passes)).

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
| [GADTs](../../spec/lang/13-gadts.md) | `gadt`, `usefulness` |
| [Annotations](../../spec/lang/14-annotations.md) (templates, facts, `@error`) | `derive`, `fact` |

### 14.2 Test Kinds

| Test | What it checks |
| --- | --- |
| conformance typing fixtures (`spec/conformance`) | accept or reject and the `code:` of each diagnostic, through the portable command contract; failures listed in the new compiler's `KNOWN_FAILURES.tsv` |
| differential against the frozen prototype | on the suite and on generated programs: accept or reject, and the set of codes per file; each disagreement triaged as a prototype bug, a new-compiler bug or a spec gap |
| literal oracle | the test-only oracle mode (section 3.6) against the normal mode on every fixture and on fuzzed statements; any difference is a bug or a spec question |
| determinism matrix (D1 §8.1) plus two dimensions | emit versus no-emit (TC-3); `fuel_used` per body equal across threads, shuffled orders, cold and warm memo (TC-8) |
| rollback verifier | debug builds hash body state at each mark and compare at rollback (TC-5) |
| TIR verifier | D2's verifier ([§3.10.3](data-structures.md#3103-invariants-and-verifiers)) on every body in debug builds and CI; a failure is a checker bug, since the checker is the builder's only caller |
| exactly-once counter | a debug counter per body; the run fails if any body is checked twice (TC-4) |
| pathological suite | each case of section 11.3, well-typed and ill-typed, within its fuel and wall-time budget, with its limit diagnostic |
| one-root-cause fuzzer | mutate one token of a well-typed program; count error diagnostics; more than two flags a cascade for review |
| crash fuzzer | `cargo fuzz` over generated and mutated programs: no panic, no internal error, no poison type in a rendered message |

## 15. Prototype Failures And The Rules That Prevent Them

From [the checker audit](../../audit/compiler/checker-2026-10-04.md),
[the findings](../../audit/compiler/findings-2026-10-04.md) and
[the architecture directions](../../audit/compiler/architecture-directions-2026-10-05.md):

| Prototype failure | Evidence | Rule here |
| --- | --- | --- |
| types are canonical strings, parsed and compared as text | [CA-02](../../audit/compiler/findings-2026-10-04.md#ca-02-textual-types-and-semantic-cycles) | TC-2: types are handles; printing only in the renderer; `Ty` has no `Display` |
| 18 checker classes share one mutable `CheckerContext` with 21 constructor parameters | [CA-03](../../audit/compiler/findings-2026-10-04.md#ca-03-shared-checker-state-and-repeated-body-checks) | TC-9: one owned `BodyCx` per body; frozen inputs by `&`; components are functions over explicit contexts |
| `LazySignatures.get` runs another checker inside a lookup | checker audit, Signature Inference | M1 is an explicit serial phase (section 9); lookups in M2 are pure reads |
| one body checked during lazy result inference, again per row round, again for final HIR | CA-03 | TC-4: each body exactly once; rows by M3 facts; the exactly-once counter |
| two rollback mechanisms with different promises | [CA-05](../../audit/compiler/findings-2026-10-04.md#ca-05-unequal-rollback-contracts) | TC-5: one trail plus append-only truncation; the rollback verifier |
| literal retry scrapes widths from diagnostic text and re-checks statements | `literal-retry.ts` (`namedWidths`) | literal classes in union-find; no statement retry; the oracle mode for checking it |
| literal joining mutates HIR in place; a global span-keyed map retains literal spans across compilations | checker audit; [CS-01](../../audit/compiler/findings-2026-10-04.md#cs-01-defaulted-literal-spans-accumulate-across-compilations) | literal widths are variables; IR types resolve once in `finish`; no global literal state (TC-9) |
| `fail()` throws `CheckFailure`; a function stops at its first fatal error | [CA-07](../../audit/compiler/findings-2026-10-04.md#ca-07-recovery-and-failure-boundaries) | TC-7: errors are values; poison; checking continues; panics are internal errors at the task boundary |
| no verifier for checker output | CA-07; directions §2 | the TIR verifier, the rollback verifier, the interface validator (D1 §4.10) |
| captured-cell conversion on HIR, imported by the emitter | [CA-04](../../audit/compiler/findings-2026-10-04.md#ca-04-cross-stage-representation-and-distributed-abi) | TC-1: the checker states capture facts; representation is D2's |
| unused-local warnings re-walk the finished HIR | checker audit | local flags during checking (section 8.3) |
| the winning candidate is re-checked outside the transaction | checker audit, Candidate Transactions | kept, but only for the instantiation choice, with arguments inferred once outside trials |
| speculation snapshots large state; exponential trait search (F-626) | perf F1, F-626 | no state copies; fuel with memo-hit charging (TC-8) |
| internal names (`__std_...`, `mut:List[char]`) and std errors at the user's line 1 | wishlist group 3 | stable-path rendering; std spans stay in std files (section 10.5) |

## 16. Open Questions For The Owner

1. **Literal width: per class or per statement?**
   [`types.literal.local.join`](../../spec/lang/04-type-system.md#r-types.literal.local.join)
   says that when a statement fails at the default types, "the literals
   take that width": one width for all of the statement's literals. In
   `(pick(1, big), pick(2, small))` with `big: i64` and `small: i32`, one
   width makes the statement an error, while per-class widths accept it
   (`1` is `i64`, `2` is `i32`). The literal plan's union-find gives
   per-class widths. **Recommendation:** per class. Reword rule 5 so that
   each group of literals that meet each other takes the width it meets.
   It is the more permissive reading, it needs no retry, and every
   example in the spec agrees with it.
2. **A type parameter solved from a not-yet-inferred private row.**
   Matching a row pattern such as `Repo[A] + Repo[B]` against the row of
   a private function whose row is omitted needs that row in M2, but M3
   solves it (section 5.6). **Recommendation:** make it an error,
   `cannot-infer-type`, with a fix-it that writes the callee's `$`
   clause. The case is rare and the fix is one line. The alternative,
   inferring such rows in M1, would move most private bodies into the
   serial phase.
3. **Type cycles through top-level bindings.** `let a = fn(): b()` and
   `let b = fn(): a()` at top level make a cycle of inferred types that
   no rule names, and nothing is read at initialization.
   **Recommendation:** report it as `recursive-function-needs-result-type`
   on the first binding in source order, with the hint "annotate one
   binding". No new code.
4. **Typed holes: `_` only, or also `todo()`?** The design list names
   `_` and `todo()`, but std has no `todo`. **Recommendation:** `_` only
   for the first release. It already has a code
   (`placeholder-outside-pipe`), while a `todo()` that compiles would
   need a std entry and a panic category.
5. **A no-emit check mode.** D2 has every `hd check` emit TIR for the
   `tir` cache entry. The playground's check-as-you-type and `hd fix`
   rounds never build, so emission there is wasted (about 200 bytes per
   line and the emission time). **Recommendation:** add the no-emit mode
   of section 1.5 for those two callers only, and keep `hd check`
   emitting. It costs one generic parameter, and TC-3's test keeps the
   two modes equal.

### 16.1 Inconsistencies Found

1. **The `block_on` ban is still transitive in the spec.**
   [`req.drive.block-on.transitive`](../../spec/lang/11-requirements-and-suspension.md#r-req.drive.block-on.transitive),
   `req.drive.block-on.unprovable`,
   [`flow.defer.block-on`](../../spec/lang/06-control-flow.md#r-flow.defer.block-on)
   ("direct or transitive") and
   [`annot.fact.no-block-on`](../../spec/lang/14-annotations.md#r-annot.fact.no-block-on)
   with its `unprovable` rule all predate answer 13: direct-only, with a
   run-time panic for an indirect call. The spec pass should rewrite
   them.
2. **`println` in the ban.** Answer 13 and D1 §4.13.5 ban a direct
   `println` as well as `block_on`, but no spec rule names `println`.
   Section 5.8 follows the answer.
3. **Readonly versus `mut` in D1's types.** D1 §3.4 and the pool's tag
   table have `Readonly(Ty)`, but the spec's marked form is `mut T`, and
   `T` alone is readonly. Section 1.4 uses `Mut(Ty)`.
4. **Literal variables in D1.** D1 §3.4 has a separate `IntLit` kind and
   no float literal kind. Here one `Infer` form carries its kind in the
   inference table, because unification merges kinds.
5. **Redundancy warnings.** This design's brief asks for redundancy
   warnings, but the spec makes an unreachable arm an error
   (`unreachable-match-arm`). The design follows the spec.
6. **Checks over TIR.** Revised D1 §4.13.3 runs mutability checks "on
   TIR places" and definite initialization and `let-else` as "a forward
   dataflow pass over the body's TIR", and §4.13.10 computes the init
   summary from TIR. Here all of them run during checking, on the
   checker's own tables (section 8). Both work while TIR is always
   emitted. Only the checker's way keeps checking independent of
   emission (TC-3), and it reports with the syntax still at hand.
7. **Coercion kinds.** TIR's `Coerce` has no kind for a declared
   variance conversion or a supertrait widening, yet both change the
   type, so invariant 4 needs them (section 4.2).
8. **Ambiguity codes.** D1 §4.13.2 says an unsolved variable is "an
   ambiguity error". The spec separates `cannot-infer-type` (no solution)
   from `ambiguous-type` (several).

## 17. Changes Needed In COMPILER_DESIGN.md

For the owners of D1 and D2 to make later. This document does not edit
COMPILER_DESIGN.md.

1. **§3.4 and §3.9.2 (types):** replace `Readonly(Ty)` with `Mut(Ty)`, in
   `TyKind` and in the pool's tag table. Drop `IntLit` and keep one
   `Infer(InferVar)`, with the variable's kind in the inference table.
   State section 1.4's accessor API as the checker's contract with the
   pool's generated views.
2. **§4.13.1:** add rule TC-4 (each body checked exactly once). Add that
   M1 checks the module's top-level statements first, because
   unannotated top-level bindings get their types there.
3. **§4.13.2:** replace "ambiguity error" with the two codes. Add the
   permission join (section 3.4), the family rule and the no-retry
   literal plan (section 3.6), and the instantiation choice with its
   trial bound (section 2.5).
4. **§4.13.3 and §4.13.10:** move mutability, definite initialization,
   `let-else` and init summaries from "over TIR" to "during checking",
   with a pointer to section 8.
5. **§4.13.4:** add the `minus` part of a pending row and the `Entails`
   fact (section 5.5), and the M2 limit of section 5.6.
6. **§4.13.6:** add the scoped pop and the escape check of section 6.1.
7. **§4.13.11, catalog:** add `Coerce` kinds for declared variance and
   for supertrait widening. Say which callee choice a compiler-supplied
   impl uses (tuples at every arity, numeric families), since a
   `TraitMethod` choice today is an impl `DefId` or a bound index.
8. **§4.13.11, `Closure`:** a capture's mode (`Copy`, `Move`, `Shared`)
   depends on statements after the closure. Either `finish` patches the
   mode from the checker's `Solution`, a third in-place patch of a fixed
   word beside the two of §3.9.5, or the builder computes it in `finish`
   from `LocalSet` order. Pick one.
9. **§4.13.11, builder:** let the checker be generic over a sink trait
   (`TirSink`) that `TirBuilder` implements, for the no-emit mode of
   section 1.5, if question 5 is accepted.
10. **§4.15:** add the fuel cost table of section 11.1 and the
    out-of-fuel policy of section 11.2.
11. **§8.1:** add the emit and no-emit dimension and the `fuel_used`
    comparison to the determinism matrix.
12. **§10.1:** add inconsistencies 1, 2 and 5 of section 16.1.
13. **§2.1, crates:** give the solver its own module boundary in
    `hd_types` (`hd_types::solve`), matching the separate
    `trait-solver.md`.
