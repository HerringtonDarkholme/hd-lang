# NonEscapable Types And Lifetimes: Design

## Table Of Contents

- [Status](#status)
- [1. Owner Requirements](#1-owner-requirements)
- [2. Core Model](#2-core-model)
- [3. Notation](#3-notation)
- [4. Signatures And Elision](#4-signatures-and-elision)
- [5. Data Types And Slots](#5-data-types-and-slots)
- [6. Closing (Parked)](#6-closing-parked)
- [7. Body Checking](#7-body-checking)
- [8. Generics, Function Types, And Erasure](#8-generics-function-types-and-erasure)
- [9. Closures, Suspensions, And Cancellation](#9-closures-suspensions-and-cancellation)
- [10. Providers](#10-providers)
- [11. Top Level, REPL, And Annotation Handles](#11-top-level-repl-and-annotation-handles)
- [12. Checker Cost, Files, And Optimizer Facts](#12-checker-cost-files-and-optimizer-facts)
- [13. Stress-Test Suite](#13-stress-test-suite)
- [14. Owner Questions](#14-owner-questions)
- [Parked: Closing And Cleanup](#parked-closing-and-cleanup)
- [Appendix A. Why A Category And Lifetimes](#appendix-a-why-a-category-and-lifetimes)
- [Appendix B. Five-Language Comparison](#appendix-b-five-language-comparison)
- [Sources](#sources)

## Status

This is the design record for issue #163. It decides nothing beyond the
owner's own decisions, and the spec is frozen. The owner and the
orchestrator settled the direction on 2026-10-14 (round 2). Round 3, the
same day, folded in eight further owner decisions (D1 to D8, below) and
the owner's answers to sixteen open questions (Q1 to Q16, listed in
[14.1](#141-decided)). Round 4, also the same day, folds in the owner's
decisions on the adversarial review
[NONESCAPE_LIFETIME_STRESS_TEST.md](NONESCAPE_LIFETIME_STRESS_TEST.md)
(NE-S1 to NE-S11, below), and parks closing.

**Closing is parked (owner, 2026-10-14).** The owner's reason: "closing
method cannot be modelled well without ownership; don't patch the
design." The `closing` mode, closing methods, cleanup blocks, and every
rule, decision, example, and review case about them now sit in
[Parked: Closing And Cleanup](#parked-closing-and-cleanup). They are kept
as written and are not checked. The active design is about **escaping**
only: the NonEscapable category, lifetimes as reaches, locality (`^`),
elision, generics, frames, providers, variance, the category calculation,
and the optimizer facts. One question follows and is open: without
closing, what fixes a fresh local handle's lifetime ([14.2](#142-open))?

The spellings `NonEscapable`, `^`, and `^{...}` are the owner's (Q16). The
`ne.` rule IDs and the diagnostic codes in [Diagnostics](#diagnostics) are
placeholders. No compiler checks these examples. A line marked `# error:`
gives the diagnostic the proposed checker would report.

**Decisions of 2026-10-14, round 4.** All are binding. Each is marked
"(owner, 2026-10-14, NE-Sn)" where it lands, with a link to the review.

| # | Decision | Review cases | Lands in |
| --- | --- | --- | --- |
| [NE-S1](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s1-recovered-write-authority) | A reach is covariant only when no mutable storage is reachable through the value's exposed surface: fields, element types, method results, and callable results. Otherwise it is invariant. This is Rust's interior-mutability rule, computed by the polarity check. | C01, C02, C03 | [5.3](#53-variance-of-slots) |
| [NE-S2](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s2-ordinary-provenance) | One flow. An ordinary `^` parameter is a root in the same reach machinery, and a `^{b}` result carries `b`'s root to the caller. Only an argument that is already global substitutes `static`. | C04, C14 | [4.1](#41-parameter-marks), [4.4](#44-call-sites), [8.2](#82-function-types) |
| [NE-S5](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s5-frame-retention-and-output) | Two slots on the compiler-known `Suspend`: `run`, what the frame keeps alive, and `out`, what its output may reach. Elision option (a): std writes `^{s.out}` on frame-driving functions, and no elision rule is special to `Suspend`. | C08 | [9](#9-closures-suspensions-and-cancellation) |
| [NE-S6](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s6-provider-elision) | Function-type elision counts NonEscapable keys and row parameters exactly as function elision does. An ambiguous callback result is written. | C09 | [8.2](#82-function-types) |
| [NE-S7](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s7-optimizer-proof-boundaries) | Doc only. `LocalParams` is a checked bound on what the callee keeps. Each optimization lists its own further preconditions. | C13, C14 | [12.6](#126-what-the-optimizer-gets) |
| [NE-S10](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s10-category-calculation) | "NonEscapable" is the instantiated type's category, computed from its stored fields (a fixed point for recursion), never from the spelling of its type arguments. `Tag[Conn]` and `Field[S, F]` are ordinary. Generic data declares no category for now. | C18 | [4.1](#41-parameter-marks), [5.1](#51-the-category) |
| [NE-S11](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s11-precision-boundaries) | C11, C12, C15, and C19 are known limits. "Exact" means exact substitution of published signatures. A closure's reach is exactly the union of what it captures, so a closure that captures nothing tracked has the empty reach and is no root (C20). | C11, C12, C15, C19, C20 | [1](#1-owner-requirements), [9](#9-closures-suspensions-and-cancellation), [13.9](#139-known-limits) |
| [NE-S3](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s3-cleanup-execution-states) | Decided (option 1, cleanup as ordered control flow), then parked with closing. | C06, C07 | [P.5](#p5-parked-owner-decisions) |
| NE-S4, NE-S8, NE-S9 | Parked with closing, not patched. NE-S9's wording is settled with NE-S3. | C05, C07, C10, C17 | [P.5](#p5-parked-owner-decisions) |

**Decisions of 2026-10-14, round 3.** All are binding. Each is marked
"(owner, 2026-10-14)" where it lands.

| # | Decision | Lands in |
| --- | --- | --- |
| D1 | Every NonEscapable parameter is written with `^`. An unmarked parameter is always global. An unmarked NonEscapable parameter is an error with a fix-it. Results keep elision. | [4.1](#41-parameter-marks), [4.3](#43-elision) |
| D2 | NonEscapable values flow only into local positions. `x: T` and `x: dyn K` accept one only when written `^T` or `^dyn K`. This reverses the round-2 "type parameters accept lifetimes by default". | [4.1](#41-parameter-marks), [8.1](#81-generic-substitution) |
| D3 | Not strict locality. Dependent results and stores stay. "Local" means "lives no longer than its sources". | [2](#2-core-model), [4.4](#44-call-sites) |
| D4 | Any parameter of any type may be marked `^`. The callee body is checked. The signature gives the optimizer a `LocalParams` fact. | [4.1](#41-parameter-marks), [7.2](#72-signature-conformance), [12.6](#126-what-the-optimizer-gets) |
| D5 | Function-typed parameters follow the same rule, as in Kotlin. No implicit default for callbacks. `^{static}` on a parameter is gone. A lint suggests `^`. | [4.1](#41-parameter-marks) |
| D6 | Kotlin-style generics. No lifetime inside a type argument. Every value has one reach. Declared slots stay, opt-in. Container methods write `^{self}` on what they store. | [5](#5-data-types-and-slots), [8.1](#81-generic-substitution) |
| D7 | Result elision stays. | [4.3](#43-elision) |
| D8 | Function types name their parameters and use the elision rules of functions. The union default and limit L3 are gone. | [8.2](#82-function-types) |

**The answers to the sixteen questions.** They are binding too, and each
is marked "(owner, 2026-10-14, Qn)" where it lands. The answers about
closing (Q1, Q3, Q4, Q8, Q11, and the naming of cleanup blocks) are
parked ([P.5](#p5-parked-owner-decisions)).

- **Notation (Q16).** A bare `^T` is not kept. Named sources go in braces:
  `^{a, b}` is a union by comma, `^{}` is fresh, `^{static}` is static,
  `^{$Pool}` is a provider key, `^{s.c}` is a slot path. The mark comes
  before `mut` and `dyn`: `^{s.c} mut dyn Socket`. Data declare slots as
  `data S[^c, ^u]`, and fields write `^{c} Conn`.
- **The category (Q2).** A concrete data or enum type with a NonEscapable
  field must declare `impl NonEscapable`. The checker verifies it against
  the fields. Generic data follows its stored fields (NE-S10 refines
  "follows its arguments"). A function-typed field does not count.
- **Downcasts (Q6).** A downcast needs a static reach, as Rust's `Any`
  needs `'static`.

**Who wrote what.** The round-2 direction comes from the owner and the
orchestrator: the category, roots, free handles, "outlives" as liveness,
flow-sensitive locals, the elision table, and the three `closing` bullets
(now parked). Round 3 adds D1 to D8. The owner's answers to the sixteen
questions adopt every agent rule of the previous revision except the ones
in the table below. Round 4 adds the NE-S decisions; the rows marked
"round 4" are agent details that applying them forced. Every rule that
still ends in **(agent)** or **(agent-detail)** was written by the agent
and needs the owner's review.

| Agent choice | Rules | Forced by |
| --- | --- | --- |
| `static` outlives every block | `ne.root.static-outlives` | `ne.root.reach` |
| A construction of a declared holder is a root site | `ne.root.site` | Q2 |
| Captured bindings have one reach | `ne.local.captured` | `fn.capture.storage.shared` |
| Where a lifetime mark may stand | `ne.syntax.position`, `ne.syntax.fn-own` | Q16 |
| Slot arguments are written `Session[^{a}, ^{b}]` | `ne.syntax.slot-args` | Q16 |
| A field of a type with slots names its slots | `ne.data.field-slot`, `ne.data.recursive` | [D7](#d7-recursive-data-of-handles) |
| A `Suspend` field does not count toward the category | `ne.data.declare` | Q2, [S5](#s5-conditional-escapable) |
| Adding `^` is compatible | `ne.param.interface` | D5 |
| The over-tie warning | `ne.elide.lint` | [R9](#r9-accept) |
| Annotation handle paths | `ne.handle.get` | `annot.handle.get` |
| Row parameters are keys named `$R` | `ne.provider.root` | Q16 |
| Round 4: a method that stores into `self` counts as mutable storage of a `dyn` | `ne.var.dyn` | NE-S1 |
| Round 4: `run` and `out` under the wrapper rule | `ne.suspend.stored` | NE-S5, Q15 |
| Round 4: `^{}` mints a caller root only for a NonEscapable result | `ne.call.fresh`, `ne.root.site` | NE-S11 (C20) |
| Round 4: a function type's own result bound may stand inside a type argument | `ne.syntax.one-reach` | NE-S6 |

The owner's standing decisions are unchanged: keep `defer`; no Drop,
`using`, or RAII; garbage collection with no moves; `T` and `mut T`
views; structured concurrency; and a declaration's signature is never
inferred from its body.

**Spec impact.** The spec is frozen, and this record edits none of it.
Q12 needs `fn.type.no-names` (function types have no parameter names) to
change. D1 to D6 and Q16 need a new parameter mark, braces for named
sources, and the receiver forms `^self` and `^mut self`. Q2 needs a new
declaration check for data types. NE-S5 needs two compiler-known slots on
the `Suspend` protocol. All of it is for a later, owner-ordered spec edit.

## 1. Owner Requirements

These are binding (owner, 2026-10-14):

1. NonEscapable is a category of types.
2. Lifetimes are part of the semantics and appear in signatures.
3. A result or a store may depend on any subset of the inputs, tracked
   exactly. One "every input" or `local` rule is too strict.
4. No false alarms from inference. A default that is stricter than needed
   is a bug. Where a source is ambiguous, an annotation is required; the
   checker never guesses conservatively.
5. A struct may hold several handles with independent lifetimes, so data
   declarations have lifetime slots.
6. The notation is terse: no `local x` and no `T from x`.

Round 3 adds (owner, 2026-10-14):

7. Local positions are marked, and nothing else is: a parameter written
   `^` is local, and an unmarked parameter is global (D1, D2, D5).
8. Any parameter of any type may be marked `^`, so locality is a general
   fact the optimizer can read (D4).
9. A value has one reach, and generic types carry no lifetime in their
   type arguments (D6).

Round 4 clarifies requirement 3 (owner, 2026-10-14,
[NE-S11](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s11-precision-boundaries)):

10. "Tracked exactly" means exact substitution of published signatures:
    a call's result and stores take the callee's declared reaches, with
    the arguments' reaches substituted. The abstractions that a signature
    or the representation makes are known limits, not guesses: one reach
    per container, joined slots in a wrapper, one root per creation site,
    and no strong updates ([13.9](#139-known-limits)).

> **Requirement 4 and the global default.** Requirement 4 forbids a
> default that guesses. The unmarked-is-global rule is not a guess: it is
> stated, and a lint proposes the mark. It is still stricter than needed
> where a callback is never stored. A callback that captures a handle,
> passed to an unmarked parameter, is an error with a fix-it, not a false
> alarm. The same holds for a library: a std parameter that lacks `^` is
> an error at the call, with the fix-it on the declaration. Requirement 4
> then governs inference and defaults only (owner, 2026-10-14, Q14).

**What the checker promises.** Escaping only (owner, 2026-10-14). A value
that reaches a local root, a `^` parameter's root, or a NonEscapable key
never reaches a global position: a top-level binding or an unmarked
parameter. A `^` parameter's root reaches only the results and objects
that its signature names. Every result and every store reaches no more
than its signature names, after elision. The checker promises nothing
about closing: when a handle is closed, whether cleanup runs, or whether
an alias is used after a close. A use after `close()` is a run-time
matter, and gives `ResourceError.Disposed` (`module.resource.disposed`).
The round-3 promise about cleanup blocks is parked
([P.1](#p1-the-parked-guarantee)).

## 2. Core Model

A **lifetime** is the set of roots a value reaches. hd is
garbage-collected, so a lifetime never protects memory. It protects one
fact: a value reaches no further than its position and the signatures it
passes through allow. A value that reaches a local, parameter, or key
root never reaches a global position.

A position is **local** when the value in it lives no longer than its
sources: a local binding, a parameter written `^`, a result or store that
names its sources. A position is **global** when the value may live
forever: an unmarked parameter and a top-level binding. "Local" never
means "cannot leave the call": a dependent result and a store into
another parameter stay (owner, 2026-10-14, D3).

```hd
let KEEP: mut List[Conn] = []

fn serve!(l: ^Listener) -> void:
    c := l.accept!()          # a new local root, owned by serve!
    c.send!([1])              # fine: a local use
    KEEP.push(c)              # error: lifetime-mismatch: KEEP is global
```

### Roots

1. r[ne.root.kinds] A **root** is one of: `static`; a **parameter root** for each `^`-marked parameter, of any type (one per declared slot when its type declares slots); a **key root** for each NonEscapable requirement key and row parameter of the declaration, named `$K` in a reach; a **local root** for each root site in the body. There is no root for a type parameter. (owner, 2026-10-14, NE-S2: "of any type")
2. r[ne.root.site] A **root site** is a call whose result has the reach `^{}` and a NonEscapable type ([5.1](#51-the-category)), or a construction of a root type, a declared holder included. Each evaluation creates a new handle, and all evaluations of one site share its local root (known limit L7, [C15](NONESCAPE_LIFETIME_STRESS_TEST.md#c15-distinct-evaluations-at-one-root-site)). A `^{}` call of any other type is no root site ([C20](NONESCAPE_LIFETIME_STRESS_TEST.md#c20-a-scalar-only-closure-factory-gets-a-local-root)). (agent-detail; the NonEscapable condition is round 4)
3. r[ne.root.param-live] A parameter root or key root is live for the whole body.
4. r[ne.root.static-outlives] `static` outlives every block. A reach that contains only `static` fits any bound. (agent)
5. r[ne.root.reach] The **reach** of a value is the set of roots it reaches. A value's reach includes the reach of every value stored in it or captured by it. A value has one reach; only data with declared slots, and the compiler-known `Suspend`, have several (`ne.data.slots`, `ne.suspend.slots`). (owner, 2026-10-14, D6, NE-S5)
6. r[ne.root.free] A local root is constrained only by where its values flow: never into a global position (`ne.call.global`, `ne.top.store`), and into a result or a parameter's object only as the signature allows. While closing is parked, nothing ends a local root's lifetime, so a helper may open a handle and return it. What should fix that lifetime is open ([14.2](#142-open)).
7. r[ne.root.global] A value in a global position reaches only `static`. An unmarked parameter's value reaches `static` in the body, and an argument for it must fit `static` ([4.4](#44-call-sites)). A value with no handle and no `^` parameter root has the empty reach, which fits. (owner, 2026-10-14, D1, D2)

### Locals And The Heap

1. r[ne.local.flow] A local binding has a reach at each program point. Assignment replaces it, and paths that join take the union.
2. r[ne.local.captured] A binding captured by a closure or a suspension frame has one reach for its whole scope: the union of every value assigned to it. (agent)
3. r[ne.heap.object] Each object has one reach for its whole life, or one per declared slot. Every alias shares it.
4. r[ne.heap.site] An object created in the body gets a reach variable at its creation site. The object of a `^`-marked parameter has that parameter's root, or its declared slots' roots. The object of an unmarked parameter has `static`.
5. r[ne.heap.store] Storing a value into a field, element, or capture adds the value's reach to the holder's reach, or to the matching declared slot.
6. r[ne.heap.fixed] Storing into an object whose reach is fixed, such as a parameter's object, requires the value's reach to fit it. A parameter written `^{p}` fits the object of `p`. Error: `lifetime-mismatch`.

> **Why.** hd has shared mutable aliases and no exclusivity. A reach that
> changed with the flow of one alias would be wrong for the others, so
> heap reaches are flow-insensitive and invariant under `mut`. Bindings are
> not shared, so they can follow the flow, as Rust's and Swift's locals do.
> The checker does not reset a heap reach when a field is overwritten or a
> list is cleared: no strong updates for now (owner, 2026-10-14, Q9, known
> limit L1).

## 3. Notation

| Form | Meaning |
| --- | --- |
| `c: ^T` | A parameter that is local and kept nowhere the signature does not name. A bare `^` names no source. Only on a parameter. |
| `c: ^{a} T` | A parameter that is local, and whose reach may flow into `a`'s object: a store. `a` names a parameter, `self`, or a slot path such as `s.u`. |
| `-> ^{a} T` | A result that reaches at most what `a` reaches. `a` names a parameter, `self`, a slot path, a declared slot, a requirement key such as `$Pool`, or `static`. |
| `^{a, b} T` | Reaches at most what `a` or `b` reaches: usable while both live. The comma is a union, as the row `$ A + B` unions its keys. |
| `-> ^{} T` | A result that reaches no input: fresh. It mirrors the empty row `$()`. When `T` is NonEscapable, the caller gets a new local root; otherwise the result reaches nothing (C20). |
| `-> ^{static} T` | A result that reaches only `static`. Only on a result: an unmarked parameter already says "global". |
| `^mut T`, `^{a} mut dyn K` | The mark comes before `mut` and `dyn`, as Rust writes `&'a mut dyn K`. |
| `^self`, `^mut self` | A receiver that is local and not kept: shorthand for `self: ^Self` and `self: ^mut Self`. |
| `f: ^fn(x: ^T) -> ^{x, f} U` | A function type naming its parameters. `{f}` is the closure's environment ([8.2](#82-function-types)). |
| `data S[^c, ^u]` | Declares two lifetime slots. Opt-in, for data holding independent handles. A field writes `^{c} Conn`. |
| `Session[^{a}, ^{b}]` | Applies a slot-bearing type to reaches. |
| `^{s.run}`, `^{s.out}` | The two slots of a frame `s: ^Suspend[T]`: what it keeps alive, and what its output reaches ([9](#9-closures-suspensions-and-cancellation)). |
| `trait K < NonEscapable`, `impl NonEscapable for S` | Marks a root type. |

```hd
fn longer(a: ^Conn, b: ^Conn) -> ^{a, b} Conn      # a union by comma
fn fresh!(host: string) -> ^{} Conn                # no input reached
fn client_mut(s: ^mut Session) -> ^{s.c} mut dyn Socket   # a slot path, mark before mut and dyn
impl Session:
    fn route(^self, hint: ^Conn) -> ^{self.u} Conn  # receiver shorthand
```

```text
lifetime    = "^" [ "{" [ atom { "," atom } ] "}" ]
atom        = "static" | [ "$" ] name { "." name }
param_type  = lifetime type          (bare or with sources: c: ^Conn, c: ^{self} Conn)
result_type = lifetime type          (never bare: -> ^{c} Conn, -> ^{} Conn)
type        = [ "mut" ] [ "dyn" ] name_or_application   (the lifetime stands before all of it)
fn_param    = [ name ":" ] param_type   (a function type may name its parameters)
receiver    = [ "^" ] [ "mut" ] "self"  (shorthand for self: ^Self, self: ^mut Self)
slot_param  = "^" name               (in a data or enum generic list)
slot_arg    = "^" "{" atom { "," atom } "}"   (in a type application)
```

1. r[ne.syntax.prefix] A lifetime is a prefix of the type it bounds, as in Rust's `&'a T`. It comes before `mut` and `dyn`, as in `c: ^mut Conn` and `-> ^{a} mut dyn Socket`. (owner, 2026-10-14, Q16)
2. r[ne.syntax.braces] Named sources are written in braces and separated by commas: `^{a, b}`. A bare `^` names no source, so on a parameter it means "not kept". `^{}` is the empty set, fresh. `^{static}` is `static`. A provider key is `^{$Pool}`, and a slot path is `^{s.c}`. (owner, 2026-10-14, Q16)
3. r[ne.syntax.position] A bare `^` is written only on a parameter. `^{}` and `^{static}` are written only on a result. `^{a}` on a parameter is a store into `a`, and on a result it is a dependence on `a`. A bare `^` on a result, or `^{}` on a parameter, is an error: write the form the position takes. Error: `misplaced-lifetime`. (agent)
4. r[ne.syntax.one-reach] `^{S} T` bounds the one reach of the value, or every declared slot of slot-bearing data. A lifetime is never written inside a type argument: `List[^{c} Conn]` is an error. Error: `lifetime-in-type-argument`. A slot argument of slot-bearing data, as in `Session[^{a}, ^{b}]`, is not a type argument. A function type's own result bound is part of that function type, so it may stand inside a type argument, as in `List[fn!() -> ^{tasks, $R} T $ R]` ([8.2](#82-function-types)). (owner, 2026-10-14, D6; the function-type sentence is agent-detail, round 4, forced by NE-S6)
5. r[ne.syntax.fn-own] On a function type, `^{S}` bounds the function value's own reach only, which is its environment, never its inputs or result. (agent)
6. r[ne.syntax.names] A signature names parameters, slot paths, keys, and `static`, never lifetime variables. A function type may name its own parameters (D8, `ne.fntype.names`). Only `data` and `enum` declarations declare slots; `Suspend`'s two slots are compiler-known (`ne.suspend.slots`).
7. r[ne.syntax.slot-args] Slot arguments come before type arguments, as in `Registry[^{p}]` or `Pair[^{a}, ^{b}, T]`. A declaration lists the slots by name, as `data Pair[^a, ^b, T]`. (agent)
8. r[ne.syntax.receiver] A receiver is a parameter like any other, written `self: ^Self` or `self: ^mut Self`. The shorthand `^self` and `^mut self` stands beside the existing `mut self` (`fn.method.mut-self`), which is shorthand for `self: mut Self`. The shorthand marks the receiver not kept. A receiver that stores into another parameter uses the explicit form. (owner, 2026-10-14, Q10)

> **Why.** A suffix is ambiguous on function types: `fn!() -> void ^static`
> reads as a lifetime on `void`. Naming parameters instead of variables is
> Mojo's `origin_of(x)` and Swift's `@lifetime(copy x)`. Kotlin marks a
> local parameter on the parameter, and so does this design. The braces are
> Scala's capture-set spelling `^{x, y}`, and a bare mark is the common
> case, so it stays one character.

## 4. Signatures And Elision

A signature states, for each parameter, whether it is local; for each
result and each store, which inputs it may reach. Elision fills the common
result cases.

```hd
fn excerpt(c: ^Conn) -> Excerpt                  # elided: ^{c} Excerpt
fn longer(a: ^Conn, b: ^Conn) -> ^{a, b} Conn
impl Pool:
    fn adopt(^mut self, c: ^{self} Conn) -> void # self takes on c's roots
trait Acceptor < NonEscapable:
    fn accept!(^self) -> ^{} Conn
fn register(job: fn!() -> void) -> void          # unmarked: global, may be stored
fn each(xs: ^List[Conn], f: ^fn(c: ^Conn) -> void) -> void   # f is not kept
```

### 4.1 Parameter Marks

1. r[ne.param.mark] A parameter is local when its type is written with `^`: bare `^`, or `^{S}` with sources. A parameter without `^` is **global**. This holds for every type, function types included. (owner, 2026-10-14, D1, D4, D5)
2. r[ne.param.global] A global parameter may be stored anywhere by the callee, so an argument for it must fit `static` ([4.4](#44-call-sites)). In the body its value reaches `static` (`ne.root.global`). (owner, 2026-10-14, D1)
3. r[ne.param.nonescapable] A parameter whose type is NonEscapable must be marked. Whether a type is NonEscapable is its instantiated category, computed from stored fields (`ne.data.category`), never the spelling of its type arguments: `List[Conn]` is NonEscapable because a list stores its elements, and `Tag[Conn]` is ordinary when `Tag` stores no `T` ([C18](NONESCAPE_LIFETIME_STRESS_TEST.md#c18-phantom-metadata-is-not-a-holder)). An unmarked NonEscapable parameter is an error with a fix-it that adds `^`. Error: `unmarked-local-param`. (owner, 2026-10-14, D1, NE-S10)
4. r[ne.param.generic] A parameter whose type is a type parameter, or contains one, or is `dyn K` for a trait `K` that is not a root trait, is global when unmarked, and accepts an argument whose reach is not `static` only when written `^T` or `^dyn K`. Otherwise the call is an error. This reverses the round-2 default that type parameters accept lifetimes, and restores the parked TQ-25's opt-in (Swift's `T: ~Escapable`). Error: `lifetime-mismatch`. (owner, 2026-10-14, D2)
5. r[ne.param.bare] A bare `^` parameter is **never kept**: its parameter root must not reach a global position, another parameter's object, or a result, unless the signature names it. The reach machinery tracks the flow, so a flow into a local object is fine until that object's reach flows out. Error: `lifetime-mismatch`. (owner, 2026-10-14, D1, D3, D4)
6. r[ne.param.sources] `^{S}` names where the parameter's reach may flow beyond the call: a store into the object of `S`, as `c: ^{self} Conn` does. A result that names the parameter, or elides to it, is the other permitted flow. (owner, 2026-10-14, D3, D6)
7. r[ne.param.any-type] Any parameter of any type may be marked `^`. The callee body is checked as for a handle: the parameter has a root in the body, and `ne.param.bare` applies. There is one flow: an ordinary `^` parameter is a root in the same reach machinery as a handle. At a call, the argument carries its own reach: empty for a fresh ordinary object, `static` for a global value, and a `^` parameter's root when the caller forwards its own `^` parameter. (owner, 2026-10-14, D4, [NE-S2](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s2-ordinary-provenance))
8. r[ne.param.ordinary-result] A result of a type that carries nothing may still name a `^` parameter, as `-> ^{xs} List[i32]` does, and the callee may return part of the parameter. The result carries the argument's reach to the caller, so a helper cannot launder a `^` parameter into a global position ([C04](NONESCAPE_LIFETIME_STRESS_TEST.md#c04-ordinary-locality-through-an-identity-helper)). An ordinary result that names no `^` parameter reaches none: the body must not return a value that reaches a `^` parameter's root. (owner, 2026-10-14, Q15, NE-S2)
9. r[ne.param.fn] A function-typed parameter follows the same rule, as in Kotlin. Unmarked, the callee may store it, so a closure that captures a local root is an error at the call, with a fix-it on the declaration. This holds for a library parameter as well. `^fn(T) -> U` is not kept. There is no implicit default for a callback, as Swift has. `^{static}` on a parameter is unnecessary and is not written. (owner, 2026-10-14, D5, Q14)
10. r[ne.param.lint] A **lint** proposes `^` on an unmarked parameter whose body never keeps it. The lint is a suggestion: it never changes a signature, so a signature still never depends on its body. Warning: `could-be-local`. (owner, 2026-10-14, D5)
11. r[ne.param.interface] Adding `^` to a parameter widens what callers may pass and is compatible. Removing it narrows that and is a breaking change. (agent)

```hd
fn ping(c: Conn) -> void:                  # error: unmarked-local-param
    c.send!([0])                           # fix-it: c: ^Conn

fn store_all[T](xs: ^List[T], x: T) -> void:   # x is global: an argument must fit static
    pass
```

> **Why.** Kotlin marks the parameter and has no default for lambdas. A
> default, as Swift's non-escaping closures have, gives two rules for one
> concept. One mark on every kind of parameter lets a check, a lint, and
> the optimizer read the same fact. One flow keeps that fact true across
> helpers: if an ordinary result dropped the root at the caller, an
> identity helper would turn a forbidden store into an allowed one (C04).

### 4.2 Lifetime-Carrying Inputs

1. r[ne.sig.carrying] A type is **lifetime-carrying** when it is a root type, a holder ([5.1](#51-the-category)), a function type, `Suspend[T]`, a `dyn` type, or mentions a type parameter. It may reach roots, or may for some instantiation. Lifetime-carrying decides elision and tracked inputs; under one flow, a `^` parameter of any type still has a root (`ne.param.any-type`).
2. r[ne.sig.input] A **tracked input** is a `^`-marked parameter of lifetime-carrying type, a NonEscapable key in the row, or a row parameter. An unmarked parameter is never one: it reaches only `static`, which fits every bound. A `^` parameter of a type that carries nothing is not one either, so no result elides to it; a result that may return it writes `^{b}` (`ne.param.ordinary-result`). (owner, 2026-10-14, Q7, NE-S2)
3. r[ne.sig.generic-result] A result type that mentions a type parameter is lifetime-carrying, so it elides like any other result. `get` elides to `self`. `map` writes its reach, because the closure's results reach the closure ([8.2](#82-function-types)).

### 4.3 Elision

| Position | Without `^` |
| --- | --- |
| A result, lifetime-carrying, no tracked input | `^{}`: fresh. |
| A result, lifetime-carrying, exactly one tracked input | That input. |
| A method result, lifetime-carrying tracked `self` | `self`. |
| Any other lifetime-carrying result | An error at the declaration. |
| A result of a type that carries nothing | Reaches no `^` parameter (`ne.param.ordinary-result`). |
| A store of one parameter into another | An error at the store, unless the stored parameter is marked `^{p}`. |

1. r[ne.elide.none] A lifetime-carrying result, in a declaration with no tracked input, is `^{}`. (owner, 2026-10-14, D7)
2. r[ne.elide.one] With exactly one tracked input, it is that input. (owner, 2026-10-14, D7)
3. r[ne.elide.self] In a method whose `self` is a tracked input, it is `self`. (owner, 2026-10-14, D7)
4. r[ne.elide.ambiguous] Otherwise the declaration must write `^`. Error: `ambiguous-lifetime`.
5. r[ne.elide.store] A body that stores one parameter's value into another parameter's object is an error unless the stored parameter is marked `^{p}` for that object, as `c: ^{self} Conn` is. A global parameter's value reaches `static`, so storing it needs no mark. Error: `lifetime-mismatch`.
6. r[ne.elide.store-meaning] `c: ^{p} T` on a parameter means the caller's `p` takes on `c`'s roots, as Swift's `@lifetime(p: copy c)` does.
7. r[ne.elide.lint] A result elided by `ne.elide.one` or `ne.elide.self` whose body never makes it reach that input gets a warning that suggests `^{}`. Warning: `overtied-result`. (agent)

> **Why.** Elision picks the only possible source and never a union.
> When there are several sources it asks, because a wrong guess is either
> unsound or a false alarm. The self rule and the one-input rule are
> Rust's. The warning catches the false alarm they can cause, as in
> [R9](#r9-accept). Every parameter now says whether it is local, so
> parameters need no elision at all: the owner keeps elision for results.

### 4.4 Call Sites

1. r[ne.call.result] At a call, a result bounded by `^{a, b}` reaches the reach of the arguments for `a` and `b`, whatever the result's type. This is exact substitution of the published signature (requirement 10). (owner, 2026-10-14, NE-S2, NE-S11)
2. r[ne.call.fresh] A result bounded by `^{}` whose type is NonEscapable reaches a new local root of the caller, created at the call. A `^{}` result of any other type, such as a function value, a frame, or an ordinary generic instance, reaches nothing: its reach is empty and fits `static` ([C20](NONESCAPE_LIFETIME_STRESS_TEST.md#c20-a-scalar-only-closure-factory-gets-a-local-root)). (owner, 2026-10-14, NE-S11; the type condition is agent-detail, round 4)
3. r[ne.call.static] A result bounded by `^{static}` reaches `static`.
4. r[ne.call.store] An argument for `c: ^{p} T` adds its reach to the object of the argument for `p`, by `ne.heap.store` or `ne.heap.fixed`.
5. r[ne.call.global] An argument for a global parameter must have a reach that fits `static`. A reach with a local, parameter, or key root is an error at the call, never an escape. Error: `lifetime-mismatch`. (owner, 2026-10-14, D2)
6. r[ne.call.local] An argument for a `^` parameter adds no constraint beyond its sources and the results that name it. Those carry the argument's reach, whatever its type. Only an argument that is already global substitutes `static`. (owner, 2026-10-14, D4, NE-S2)

## 5. Data Types And Slots

### 5.1 The Category

1. r[ne.data.root-type] A **root type** is a trait that has `NonEscapable` as a direct or transitive supertrait, or a data or enum type with `impl NonEscapable for S`. A `dyn K` of a root trait `K` is a root type. Every value of a root type starts as its own root.
2. r[ne.data.root-impl] A data type that implements a root trait must declare `impl NonEscapable` too, by the ordinary supertrait rule (`trait.super.missing`).
3. r[ne.data.holder] A **holder** is a data or enum type with a field or payload that is lifetime-carrying ([4.2](#42-lifetime-carrying-inputs)). A concrete holder whose field is NonEscapable declares it (`ne.data.declare`) and is then a root type. A generic holder follows its stored fields (`ne.data.generic`). A holder with only function-typed or `Suspend` fields is not NonEscapable.
4. r[ne.data.category] A type is **NonEscapable** when it is a root type; a tuple with a NonEscapable element; or an instantiation of a generic data or enum type with a stored field or payload whose type, after substituting the type arguments, is NonEscapable. Function-typed and `Suspend` fields do not count (Q2). A type argument that no field stores does not count, so `Tag[Conn]` with `data Tag[T]: number: usize` is ordinary, and so is the annotation handle `Field[S, F]`. Recursive types take the least fixed point: every instantiation starts ordinary, and becomes NonEscapable while one of its stored fields is. The spelling of the type arguments never decides. (owner, 2026-10-14, [NE-S10](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s10-category-calculation))
5. r[ne.data.declare] A concrete data or enum type, one with no type parameters, with a NonEscapable field or payload must declare `impl NonEscapable for S`. The checker verifies the declaration against the fields. A field of function type does not count: a closure carries a reach but is not the category. A `Suspend[T]` field does not count either. A type that declares slots writes the impl without slot arguments. Error: `missing-nonescapable-impl`, with a fix-it that adds `impl NonEscapable for S`. (owner, 2026-10-14, Q2, NE-S10; the `Suspend` and slot details are agent-detail)
6. r[ne.data.one-reach] A data or enum type without declared slots has one reach. Every lifetime-carrying field shares it, and it needs no slot declaration. This is Kotlin's `this`. (owner, 2026-10-14, D6)
7. r[ne.data.tuple] A tuple has one reach: the union of its elements'. (owner, 2026-10-14, D6)
8. r[ne.data.generic] Generic data follows its stored fields (`ne.data.category`) and declares nothing for now. `Box[i32]` carries nothing, and `Box[Conn]` is NonEscapable with one reach, which grows when a `Conn` is stored in it. No lifetime appears in the type argument (`ne.syntax.one-reach`). Generic data with a concrete NonEscapable field, as `data Foo[T]: c: Conn, x: T`, declares nothing either: every instantiation is NonEscapable by its field `c`. (owner, 2026-10-14, D6, Q2, NE-S10)

```hd
data Excerpt:
    conn: Conn                # a concrete NonEscapable field

impl NonEscapable for Excerpt # required: missing-nonescapable-impl without it

data Box[T]:
    value: T                  # follows its stored field: no impl

data Tag[T]:
    number: usize             # stores no T: Tag[Conn] is ordinary
```

> **Why.** The category matters where values start, and a holder is a
> value whose parameters need a mark. The owner chose that a concrete
> holder says so in one line: the declaration is checked against the
> fields, so it cannot drift, and a reader sees the category at the type.
> Generic data cannot say it, because the answer depends on the argument,
> so it follows its stored fields, as Rust's auto traits follow fields
> rather than type-argument spelling. This replaces the parked TQ-24,
> which followed type arguments for every type and so missed a
> `Suspend[void]` that holds a handle. Computing from stored fields also
> keeps phantom arguments, as in `Tag[Conn]` and `Field[S, F]`, ordinary
> (C18). One reach per value is Kotlin's model: it loses the independence
> of two handles in one container, which declared slots give back.

### 5.2 Declared Slots

```hd
data Session[^c, ^u]:
    client: ^{c} Conn
    upstream: ^{u} Conn

impl NonEscapable for Session
```

1. r[ne.data.slots] `data S[^c, ^u]` declares the slots `c` and `u`, for data that holds independent handles. A type with one handle, or handles that share a lifetime, declares nothing. Slot names are local to the declaration. (owner, 2026-10-14, D6)
2. r[ne.data.field-slot] In a type with declared slots, every lifetime-carrying field names the slots it reaches, as `client: ^{c} Conn` does. A field that leaves them unnamed is an error. Error: `undeclared-slot`. (agent)
3. r[ne.data.path] Outside the declaration, a value's slot is named by a path: `^{s.c}` is slot `c` of parameter `s`, and `^{self.c}` is slot `c` of the receiver.
4. r[ne.data.whole] A name without a slot path, `^{s}`, names the union of all of `s`'s slots.
5. r[ne.data.recursive] A recursive occurrence of slot-bearing data is a field, so it names its slots like any field, with the union of the declaration's slots when it sits in a wrapper, as in `next: ^{a, b} Duo?`. A slot not declared there is an error. Error: `undeclared-slot`. (agent)
6. r[ne.data.wrapper] A generic type applied to slot-bearing data, `Option[Session]` or `List[Session]`, has one reach: the union of the slots. A value taken out of it has every slot at that union. This loses independence, and is sound (known limit L5, [C11](NONESCAPE_LIFETIME_STRESS_TEST.md#c11-optional-wrapping-destroys-independent-slots)). (owner, 2026-10-14, Q15, NE-S11)
7. r[ne.data.literal] A data literal infers its slot arguments from its field values at the use site.

### 5.3 Variance Of Slots

1. r[ne.var.subtype] `^{S} T` is assignable to `^{S'} T` when `S` is a subset of `S'`, ignoring `static`.
2. r[ne.var.surface] A reach, or a declared slot, is covariant only when no mutable storage is reachable through the value's exposed surface: its fields, its element types, its method results, and its callable results, transitively. Otherwise it is invariant, under every view. Mutable storage is a `mut` field or element, a result typed `mut`, or a method that stores into the value. The check reuses `types.polarity`: mutable storage is an invariant position, as in `types.polarity.mut`. This is Rust's rule for interior mutability: `Cell<&'a T>` is invariant in `'a`. (owner, 2026-10-14, [NE-S1](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s1-recovered-write-authority))
3. r[ne.var.polarity] A declared slot's variance is computed by the `types.polarity` rules over the type's surface, as `+T` is checked, together with `ne.var.surface`. It is not written. (owner, 2026-10-14, Q5, NE-S1)
4. r[ne.var.mut-data] Under a `mut` view, the reach of a data, enum, list, or map value, and every declared slot, is invariant, because a field or element can be reassigned (`types.variance.mut-invariant`).
5. r[ne.var.dyn] The reach of `dyn K` is covariant only when no method of `K` exposes mutable storage: no method result is typed `mut` or reaches mutable storage, and no parameter is marked `^{self}`. Otherwise it is invariant, under every view. So a getter `fn items(^mut self) -> ^{self} mut List[Conn]` makes the reach invariant ([C01](NONESCAPE_LIFETIME_STRESS_TEST.md#c01-a-trait-getter-replaces-the-downcast)). (owner, 2026-10-14, NE-S1; counting a `^{self}` store is agent-detail)
6. r[ne.var.fn] A function value's own reach is covariant only when its result type exposes no mutable storage, nor do the results of the function values it returns. `fn() -> mut List[Conn]` is invariant ([C02](NONESCAPE_LIFETIME_STRESS_TEST.md#c02-a-closure-returns-mutable-captured-state)). A function value has no access permission (`fn.type.no-permission`), but a call may still return write authority (`fn.capture.result-not-weakened`). (owner, 2026-10-14, NE-S1)
7. r[ne.var.suspend] A frame's `run` slot is covariant: a frame exposes nothing but its output. Its `out` slot is covariant only when `T` exposes no mutable storage. (owner, 2026-10-14, NE-S1, NE-S5)
8. r[ne.var.generic] The one reach of a generic container is covariant through a readonly view only when its element type exposes no mutable storage. `List[Conn]` is covariant through a readonly view. `List[mut List[Conn]]` is invariant under every view, because `types.readonly.nested` keeps the inner list mutable ([C03](NONESCAPE_LIFETIME_STRESS_TEST.md#c03-a-readonly-container-contains-mutable-children)). (owner, 2026-10-14, D6, NE-S1)

> **Why.** Widening a reach is safe only when nothing can be written
> through the widened value. A readonly view of the outer value is not
> enough: a getter, a callback result, or a nested `mut` element hands
> back storage that other aliases still see at the narrow reach. Rust
> makes a lifetime under `Cell` or `&mut` invariant for the same reason.
> Computing variance from the declaration, as Rust does, means no slot is
> declared invariant without cause, which requirement 4 forbids. Adding a
> method that stores into a slot, or returns mutable storage, can change
> variance: an interface change hd already accepts for inherent methods
> (`types.variance.surface`). The rules still let a long-lived handle
> enter a short-lived readonly `List[Conn]`.

## 6. Closing (Parked)

Closing is parked (owner, 2026-10-14): "closing method cannot be modelled
well without ownership; don't patch the design." The `closing` mode,
closing methods, `ne.close.*`, and `ne.cleanup.*` are in
[Parked: Closing And Cleanup](#parked-closing-and-cleanup). In the active
design, `close` is an ordinary method, `fn close(^self) -> void`, and a
use after it is a run-time `Disposed` error.

## 7. Body Checking

A body is checked against its own signature and its callees' signatures.
It never reads another body.

### 7.1 Uses (Parked)

Uses and liveness served only the outlives check against cleanup blocks.
They are parked with closing ([P.4](#p4-outlives-and-liveness)). The
active checks below read solved reaches, not liveness.

### 7.2 Signature Conformance

1. r[ne.body.result] Every value the body returns, or propagates with `?`, must have a reach that fits the result's bound, after elision. A local root fits any bound (`ne.root.free`); whether it should fit `^{static}` is part of the open question in [14.2](#142-open). Error: `lifetime-mismatch`.
2. r[ne.body.param-store] A store into a parameter's object must fit that parameter's reach, which `ne.elide.store` extends only as written. Error: `lifetime-mismatch`.
3. r[ne.body.local] A `^` parameter is checked with the same reach machinery, whatever its type. Its root must not flow into a global position, into a parameter's object the signature does not name, or into a result that does not name it. The check uses the flow-sensitive reaches of `ne.local.flow` and the solved reaches of [12.2](#122-per-body). Error: `lifetime-mismatch`. (owner, 2026-10-14, D4, NE-S2)
4. r[ne.body.global-arg] A call that passes a value to a global parameter needs the value's reach to fit `static` (`ne.call.global`). In a body, a `^` parameter's root is never `static`, so a `^` parameter cannot be passed to a global parameter. (owner, 2026-10-14, D2)

### 7.3 Escapes

1. r[ne.escape.def] A local root **escapes** when its reach flows into the result or into a parameter's object.
2. r[ne.escape.free] An escape that the signature allows is fine. The caller sees the escaped root as the new root of a `^{}` result, or inside the reach of the named result or object.
3. r[ne.escape.global] A flow into a top-level binding or a global parameter is not an escape. It is an error, because a NonEscapable value flows only into local positions. Error: `lifetime-mismatch`. (owner, 2026-10-14, D2)

### 7.4 What Is Checked

| Check | Rule |
| --- | --- |
| Result and stores fit the signature | `ne.body.result`, `ne.body.param-store` |
| A `^` parameter is not kept | `ne.body.local` |
| No local value flows to a global position | `ne.call.global`, `ne.escape.global`, `ne.top.store` |
| A widening keeps invariant reaches | `ne.var.surface` |
| A concrete holder declares its category | `ne.data.declare` |

## 8. Generics, Function Types, And Erasure

### 8.1 Generic Substitution

1. r[ne.generic.default] A type parameter accepts no NonEscapable argument by default. `x: T` is a global parameter (`ne.param.generic`), and `x: ^T` accepts one. This reverses the round-2 default and restores the parked TQ-25's opt-in. (owner, 2026-10-14, D2)
2. r[ne.generic.carry] A generic type applied to a lifetime-carrying type argument has one reach, which grows with each store. `List[Conn]` has the reach of the handles stored in it. No lifetime is written inside the argument. (owner, 2026-10-14, D6)
3. r[ne.generic.abstract] In a generic body, a value of type `T` reaches `static` when its parameter is unmarked, and the parameter's root when it is `x: ^T`. A value of type `T` taken out of a container reaches the container's reach. Nothing else is known about it.
4. r[ne.generic.store] A generic function stores a `^T` argument into a container with the store mark: `fn push(^mut self, v: ^{self} T)`. A generic function that stores an argument in module state takes it unmarked, `x: T`. There is no `^{static} T` parameter (`ne.param.fn`). (owner, 2026-10-14, D5, D6)
5. r[ne.generic.infer] Slot arguments of generic data are inferred at use sites like type arguments. A declaration's slots are never inferred from its body.
6. r[ne.generic.no-lifetime-arg] An explicit type argument carries no `^`. `List[^{c} Conn]` and `keep::[^{c} Conn](x)` are errors (`ne.syntax.one-reach`). (owner, 2026-10-14, D6)

```hd
impl[T] List[T]:
    fn push(^mut self, v: ^{self} T) -> void       # stores v
    fn contains(^self, v: ^T) -> bool              # keeps nothing
    fn get(^self, index: usize) -> T?              # elided: ^{self}
```

### 8.2 Function Types

A function type is elided by the rules a function uses (owner, 2026-10-14,
D8). It replaces the round-2 union default and the known limit L3.

1. r[ne.fntype.own] A function type has one own reach: the reach of its captures, called its **environment**.
2. r[ne.fntype.names] A function type may name its parameters: `fn(x: ^T) -> ^{x} U`. A name is needed only for a `^` in the result to refer to. `fn.type.no-names` must change for this. (owner, 2026-10-14, D8, Q12)
3. r[ne.fntype.inputs] An input of a function type follows `ne.param.mark`: `fn(x: ^T)` is local, and an unmarked input is global. A callback that is called with a handle writes `^` on that input. (owner, 2026-10-14, Q12)
4. r[ne.fntype.env] In a function type that is a parameter `f: ^fn(...)`, or sits inside one's type, `{f}` names the environment, the closure's captures. The environment is a tracked input of the function type. (owner, 2026-10-14, Q12)
5. r[ne.fntype.elide] A result of a function type that is lifetime-carrying is elided by the tracked inputs of that type, counted exactly as `ne.sig.input` counts them for a function: its `^` inputs, its environment, and the NonEscapable keys and row parameters of its row. No tracked input gives `^{}`. Exactly one gives that input, so a function type with no parameters and no such row depends on its environment. Anything else must be written, and an unwritten one is an error at the declaration, as in `^fn() -> ^{$Pool} Conn $ Pool`. Error: `ambiguous-lifetime`. A function item used as a value has an empty environment, so only its parameters and row count. (owner, 2026-10-14, D8, [NE-S6](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s6-provider-elision))
6. r[ne.fntype.outside] A function type that is not a parameter, such as a result, a field, or a local, has no name for its environment, so an ambiguous result there cannot be written. This is a known limit (L6). A data wrapper with a method gives it a parameter to name. (owner, 2026-10-14, Q12)
7. r[ne.fntype.call] A call of a function value instantiates its bound: each named input becomes its argument's reach, and the environment name becomes the reach of the function value being called.
8. r[ne.fntype.row] A NonEscapable key in a function type's row is resolved at each call, so the result may reach that call's provider.
9. r[ne.fntype.subtype] Inputs are contravariant, and the result and the environment covariant, subject to `ne.var.fn`. A result bound `^{x}` is assignable where `^{x, f}` is expected.
10. r[ne.fntype.one-flow] A written reach applies whatever the instantiated result type. `map` over a pool of handles, yielding a `List[usize]`, still reaches the pool and the closure's environment: exact substitution of the published signature (known limit L9). Only an argument that is already global substitutes `static`. This replaces round 3's `ne.fntype.carry-only`, which dropped the reach of an ordinary result. (owner, 2026-10-14, NE-S2)

```hd
fn map[T, U](xs: ^List[T], f: ^fn(x: ^T) -> ^{x, f} U) -> ^{xs, f} List[U]
fn each(xs: ^List[Conn], f: ^fn(c: ^Conn) -> void) -> void     # void: nothing to name
fn retry![T, E, $R](times: usize, attempt: ^fn!() -> ^{attempt, $R} Result[T, E] $ R) -> ^{attempt, $R} Result[T, E] $ R
fn all_list![T, $R](tasks: ^List[fn!() -> ^{tasks, $R} T $ R]) -> ^{tasks, $R} List[T] $ R
```

> **Why.** The owner's example writes `x: T` unmarked. Under D2 that input
> would be global, so `^{x}` would name nothing and the callback could not
> take a handle. The inputs are written `x: ^T` here, and the owner
> accepted that (Q12). `map` writes both results because an element of the
> new list may be a value of `f`'s environment or of `xs`. `retry!` and
> `all_list!` now write their results too. Their callbacks and their own
> rows both carry the row parameter `$R`, which may hold a provider, so
> each has two tracked inputs. Round 3 elided them to the environment
> only, which counted `$R` for a function and not for a function type
> (C09). The same counting now applies to both.

### 8.3 Erasure And Downcasting

1. r[ne.erase.slot] Every `dyn K`, including `dyn Any` and `dyn Inspectable`, has one reach.
2. r[ne.erase.join] Erasing a value to `dyn K` joins all its slots into that one reach.
3. r[ne.erase.allowed] Erasing a lifetime-carrying value is allowed into a local position: a binding, or a parameter written `^dyn K`. A parameter `x: dyn K` is global, so it takes only a `static` value (`ne.param.generic`). This replaces the parked TQ-24 "erasure is rejected", so a `dyn Error` may hold a handle in a local. (owner, 2026-10-14, D2)
4. r[ne.erase.downcast] A downcast needs a static reach. `downcast`, `downcast_mut`, and `downcast_val` require the `dyn` value's reach to be `static`, and the result reaches only `static`. A `dyn` value that may reach a local, parameter, or key root cannot be downcast, even when it holds an ordinary value (known limit L8, [C19](NONESCAPE_LIFETIME_STRESS_TEST.md#c19-a-local-classifier-cannot-downcast-ordinary-values)). Error: `downcast-non-static`. (owner, 2026-10-14, Q6, NE-S11)
5. r[ne.erase.identity] Slots are not part of runtime identity. `TypeId::of::[Session[^{a}, ^{b}]]()` equals `TypeId::of::[Session[^{c}, ^{d}]]()`.

> **Why.** Rule 4 follows Rust's `Any: 'static`, with one difference.
> Rust tests the erased type's contents, so a short-lived `&dyn Any` to a
> `'static` value still downcasts. hd tests the erased value's reach, so a
> local `^dyn Inspectable` cannot downcast even a scalar (L8). The owner
> chose the whole rule over the narrower rule of an earlier revision,
> which forbade only downcasts to a type with an invariant reach
> ([X6](#x6-widening-through-a-downcast)): one sentence to state and to
> check. Erasure itself stays allowed, so a `dyn Error` can carry a handle
> through a local and be inspected by its trait methods, but not taken
> apart.

## 9. Closures, Suspensions, And Cancellation

1. r[ne.closure.reach] A closure's own reach is exactly the union of the reaches of what it captures. Each captured binding has one reach (`ne.local.captured`). A closure that captures nothing tracked has the empty reach, which fits `static`: it is no root ([C20](NONESCAPE_LIFETIME_STRESS_TEST.md#c20-a-scalar-only-closure-factory-gets-a-local-root)). (owner, 2026-10-14, NE-S11)
2. r[ne.suspend.slots] The compiler-known `Suspend[T]` has two slots. `run` is what the frame keeps alive: its arguments, its captures, and its NonEscapable providers. `out` is what its output may reach. A path names them, as `^{s.run}` and `^{s.out}`, and `^{s}` names both (`ne.data.whole`). (owner, 2026-10-14, [NE-S5](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s5-frame-retention-and-output))
3. r[ne.suspend.cold] A plain call `f(a, b)` of a `fn!` builds a cold frame. Its `run` is the union of the argument reaches and the construction-time providers (`req.lowering.frame`, `req.bind.captured`). Its `out` is the callee's declared result reach with the argument reaches substituted, as `ne.call.result` does for a direct call. (owner, 2026-10-14, NE-S5)
4. r[ne.suspend.exact] Both slots are exact. `run` is exact because the frame stores every argument. `out` is exact because it substitutes the published signature (requirement 10). (owner, 2026-10-14, NE-S5, NE-S11)
5. r[ne.suspend.drive] Driving a frame yields a value whose reach is the frame's `out`, never its `run`. A bang call, a poll that completes, and a combinator all drive. (owner, 2026-10-14, NE-S5)
6. r[ne.suspend.bang] A bang call `f!(a, b)` keeps neither its frame nor its arguments after it returns. Its result reaches `out`, which is the reach a direct call gives.
7. r[ne.suspend.stored] A stored frame is an ordinary value. The frame value's own reach, for storage and capture, is `run`: a holder of a frame takes on `run` (`ne.heap.store`). A generic wrapper of frames, as `List[Suspend[T]]` or `Suspend[T]?`, joins both slots into its one reach (`ne.data.wrapper`). (owner, 2026-10-14, NE-S5; the wrapper detail is agent-detail, round 4)
8. r[ne.suspend.elide] No elision rule is special to `Suspend`. A std function that drives a frame passed as a parameter writes `^{s.out}` on its result, as `fn finish![T](s: ^mut Suspend[T]) -> ^{s.out} T` does. Left elided, such a result would be `^{s}`, both slots. (owner, 2026-10-14, NE-S5, option (a))
9. r[ne.all.typing] `all!` and `race!` take their children's frames for the call only, so their child parameters are written `^`. They are compiler intrinsics (`req.combinator.all-typing`, `req.combinator.race-signature`). The result tuple of `all!` joins the children's `out` slots, and the value of `race!` is the join of their `out` slots. Neither reaches a child's `run`. (owner, 2026-10-14, NE-S5)
10. r[ne.all.list] `all_list!` takes a list of closures, as `tasks: ^List[fn!() -> ^{tasks, $R} T $ R]`. The list is an ordinary value with one reach.
11. r[ne.all.global] A combinator parameter left unmarked is global, so a frame that captures a handle is an error at the call. (owner, 2026-10-14, D5)

```hd
fn select!(keep: ^Conn, scratch: ^Conn) -> ^{keep} Conn
fn finish![T](s: ^mut Suspend[T]) -> ^{s.out} T

fn via_frame!(keep: ^Conn, scratch: ^Conn) -> ^{keep} Conn:
    let s: mut Suspend[Conn] = select(keep, scratch)   # run: keep, scratch; out: keep
    finish!(s)                                         # fine: the output reaches keep only
```

> **Why.** A frame keeps more alive than its output depends on, as a Rust
> future does beside its `Output` type. One reach for both would either
> tie `via_frame!`'s result to `scratch`, a false alarm, or drop the frame's
> hold on `scratch` when it is stored, a hole (C08). hd has no detached
> spawn, so a frame driven by a bang call needs nothing more. A stored
> frame carries `run`, as in [R10](#r10-a-borrow-across-a-suspension) and
> [D2](#d2-a-cold-suspension-in-a-field).

## 10. Providers

1. r[ne.provider.key] A requirement key may be a NonEscapable trait.
2. r[ne.provider.root] In a declaration, each NonEscapable key of its row is a tracked input and a key root, named `$K` in a reach, as `^{$Pool}`. A row parameter `$R` is named `$R`. A key is not a parameter, so `ne.param.nonescapable` does not apply to it. (owner, 2026-10-14, Q7; the row-parameter spelling is agent-detail)
3. r[ne.provider.use] `$.use(K)` reaches the key root `$K`.
4. r[ne.provider.with] Inside `$.with(K=p)`, calls instantiate `$K` with `p`'s reach.
5. r[ne.provider.frame] A frame built inside that scope captures the provider, so its `run` includes `p`'s reach. Its `out` includes `p`'s reach only where the callee's result names `$K`. (owner, 2026-10-14, NE-S5)
6. r[ne.provider.closure] A closure captures no provider (`fn.capture.no-providers`). Its row carries the key, and each call resolves it there.
7. r[ne.provider.flow] `req.use.value.flow` and `req.use.value.outlives-scope` stay true, bounded by the provider's reach.

## 11. Top Level, REPL, And Annotation Handles

1. r[ne.top.static] A top-level binding, and a binding made by a REPL input, reaches `static`. A REPL session may keep `c := connect!("db")` across inputs (`cli.repl.declarations.bindings`).
2. r[ne.top.elide] A lifetime-carrying top-level binding's declared type is `^{static}`, without writing it. (owner, 2026-10-14, Q7)
3. r[ne.top.store] A store into a top-level binding's object requires the value's reach to fit `static`. A value that reaches a local, parameter, or key root is an error, not an escape. Error: `lifetime-mismatch`. (owner, 2026-10-14, D2)
4. r[ne.handle.no-slot] An annotation handle `Field[S, F]` has no reach and may escape (`annot.handle.escape`), since it stores no `S` or `F`. By the category calculation it is ordinary, even when `S` or `F` is NonEscapable (`ne.data.category`). How operations through it carry the target's reach is open ([14.2](#142-open)). (owner, 2026-10-14, NE-S10)
5. r[ne.handle.get] `h.get(x)` has the type of `x.member` (`annot.handle.get`), including its slot path, as in `^{x.c} Conn`. (agent)
6. r[ne.handle.build] A value that `build` passes for a member must fit that member's slot in the value being built.

## 12. Checker Cost, Files, And Optimizer Facts

### 12.1 What A Signature Exports

- For each parameter: whether it is `^`-marked, and its sources (the objects it may be stored into). This is the `LocalParams` fact ([12.6](#126-what-the-optimizer-gets)).
- For each result and each `c: ^{p}` store: a reach, as a set of atoms over parameters, slot paths, keys, row parameters, and `static`, or `{}`. A function type inside a signature exports its own result reaches, over its named inputs, its environment, and its row. A frame-driving result exports a reach over `s.out` slot paths.
- For each data and enum type: its declared slot list, the slot each field uses, each slot's computed variance, and whether it declares `NonEscapable`. For generic data, the stored fields that decide the category of an instantiation.
- For each trait: whether it is a root trait, and each method's signature as above.

All of it is written, or elided by local rules, so no export depends on a
body.

### 12.2 Per Body

1. **Variables.** One reach variable per lifetime-carrying binding version (SSA), object site, and call result, one per declared slot of slot-bearing data, and two per frame (`run`, `out`). Most types have one reach.
2. **Constraints.** Each store and call adds subset constraints
   (`reach(x) ⊆ reach(o)`), and each invariant position adds an equality.
   A callee signature is instantiated by substituting argument reaches
   for parameter names; `^{}` mints a local root when the result is
   NonEscapable. A global parameter adds `reach(arg) ⊆ {static}`.
3. **Solving.** The least solution over a finite set of roots: a
   worklist over subset edges with bitset reaches, and union-find for the
   equalities. A body rarely has more than 64 roots, so a reach is one
   machine word.
4. **Escape.** Set tests on the solved reaches: results, stores, and
   global positions.
5. **Local parameters.** For each `^` parameter, of any type, test that
   its root is absent from every reach it may not enter: the globals,
   other parameters' objects, and results that do not name it. These are
   set tests on the solved reaches. At its callers, a `^{b}` result
   carries the argument's reach: empty for a fresh ordinary object, and a
   root only when the caller forwards one of its own `^` parameters
   (NE-S2).
6. **Declared holders.** For each concrete data or enum type, compare its
   fields with its `impl NonEscapable`. This is one pass over the
   declaration, not over bodies.
7. **Generic category.** For each generic instantiation used, compute
   whether it is NonEscapable from its stored fields, as a least fixed
   point over recursive types, cached per instantiation (NE-S10).

Cleanup blocks and liveness, items 4 and 5 of round 3, are parked
([P.4](#p4-outlives-and-liveness)).

Cost: about `O((N + E) · ⌈R/64⌉)` per body, for `N` program points, `E`
constraints, and `R` roots. This is a common-case estimate, not a derived
worst-case bound: a monotone worklist may revisit an edge as a reach
grows (review, "Checker Cost Needs A Real Bound"). There is no global
fixed point over bodies: signatures are summaries. Marking every
parameter of an ordinary type adds one root per marked parameter in that
body.

### 12.3 Incremental And Cross-File Effects

- A body's result depends only on its own text and its callees'
  signatures. Editing a body never invalidates another body.
- Slot variance depends on the declaration's surface, as type variance
  already does (`types.variance.surface`). Adding a method that stores
  into a slot, or returns mutable storage, can change variance, which is
  an interface change (NE-S1).
- Adding a NonEscapable field to a concrete data type needs the impl in
  the same declaration, so the category is part of the type's interface
  and never changes behind an importer's back. Adding a field that stores
  a type parameter to generic data changes the category of its
  instantiations, which is an interface change of the same kind.
- Interface files carry the exports in 12.1. A change to a reach in a
  public signature is an API change, like a change to a row.
- Adding `^` to a parameter is compatible, and removing it is breaking
  (`ne.param.interface`). So a lint that proposes `^` ([4.1](#41-parameter-marks))
  never makes an upgrade unsafe.

### 12.4 What It Reuses

| Need | Existing structure |
| --- | --- |
| Reach as a set with union and the empty set | Requirement rows: `RowData { keys, params }` in `crates/hd_types/src/pool.rs`; union and membership entailment (`req.row.set.union`, `req.row.entail.membership`) |
| Instantiating a reach at a call | Row fitting in `crates/hd_check/src/rows.rs` (`fit_row`, `solve_exact_row`, `collect_infer`); the least solution of `req.row.least.solution` |
| A second parameter kind | The kind check behind `generic-kind-mismatch` and the `$` marker (`req.row.param.marked`) |
| Slot variance, and the mutable-storage test of NE-S1 | The `types.polarity` check |
| Liveness | Parked with closing ([P.4](#p4-outlives-and-liveness)). |
| The `LocalParams` fact | Per-body facts in `crates/hd_opt/src/passes.rs` (`BodyFacts`), which `devirt_known` and `closure_spec` already read |
| The holder check | The supertrait check behind `trait.super.missing`, run against a type's fields |

Rows forbid parameters on data (`req.row.param.no-data`). Declared slots
must not copy that rule: they are required for independent handles (owner
requirement 5). So slot parameters are a new kind, beside type-kinded and
row-kinded parameters, and the type pool needs slot arguments on data
types. Under D6 only data that holds independent handles pays for this,
and `Suspend`, whose two slots are compiler-known.

### 12.5 Annotation Burden In std (Estimate)

Counts come from a scan of `lib/std/**/*.hd`: 1261 `fn` declarations.
A parameter is classified by its type text, so the edges are approximate.
The lists for stores and callbacks were made by hand. "Public" means a
`pub fn` or a trait method. Leaving any parameter unmarked is sound, since
unmarked means global. The marks are needed to accept code that passes a
handle, or a closure that captures one, except where the table says an
error. The notation of the owner's Q16 changes the spelling and not the
count: `^self` is the same one mark as before, and `^{self}` on a store is
the same mark with braces.

| Class | What | Public | Private |
| --- | --- | --- | --- |
| **Must change** | Handle traits: `< NonEscapable` on 3 traits, `^` on 8 receivers, `close` included (D1; closing is parked) | 3 + 8 | 0 |
| **Follows from marking** | Written result reaches on adapters and `map`-like methods (D3, D8): `List` 4, `Option` 2, `Result` 3, `Iterator` 7 | 16 | not counted |
| **Follows from marking** | Written function-type results (D8): `Option` 2, `Iterator` 3, `Result` 3, `List` 4, `std.testing` 6 more; row-polymorphic callbacks such as `retry!` and `all_list!` add more (NE-S6), not counted | 12 | 0 |
| **Follows from marking** | `^{s.out}` on frame-driving functions (NE-S5), not counted | - | - |
| Accept handles | `^{}` on `accept!` (the three `Net` methods no longer need it) | 1 | 0 |
| Accept handles | Store marks `^{self}` (D6): `List` `push`, `insert`, `extend`; `Deque` `push_back`, `push_front`; `Set.insert`; `Heap.push` | 7 | 0 |
| Accept handles | Bare `^` on generic receivers and parameters (D2, D6): 109 receivers, 92 parameters | 201 | 237 |
| Accept handles | Bare `^` on `dyn` and function-typed parameters (D2, D5): 5 and 35 | 40 | 40 |
| Accept handles | `^` on the inputs of callback types, `fn(x: ^T)` (D2, D8): 25, and 6 in `std.testing` | 25 | about 10 |
| Declare the category | `impl NonEscapable for S` on a concrete data or enum type with a handle field (Q2) | 0 today | 0 today |
| **Locality marks** | `^` on parameters of an ordinary heap type (strings, lists of scalars, plain data): 243 receivers, 157 parameters | 400 | 863 |
| Never needed | Scalars, passed by value | 83 | 239 |

Reading the table:

- **Must change.** Eight receivers and three trait lines. These are the
  only edits whose absence is an error today: an unmarked NonEscapable
  parameter is `unmarked-local-param`. The three `close` receivers now
  take `^self`, since closing is parked. The three `Net` methods drop out
  because `Net`'s receiver is unmarked, so it is not a tracked input and
  `connect!` elides to `^{}`.
- **Follows from marking.** Once an adapter marks its callback `^`, the
  result must say whether it keeps the callback. 28 public signatures
  write a reach, against none in round 2. Round 2's claim "collection,
  iterator, option, result, and task signatures: 0" is gone, because D6
  removed lifetimes from type arguments and D8 removed the union default.
  NE-S6 adds written results wherever a callback's row carries `$R`.
- **Accept handles.** About 274 public marks. This is the price of the
  opt-in of D2, and it is mechanical: the lint proposes each one and a
  fix-it applies it. The migration is phased (owner, 2026-10-14, Q13):
  first the 8 receivers, the 7 stores, and the callbacks that the stress
  tests use, then the rest. Until a parameter is marked, a call that
  passes it a handle is an error with a fix-it on the declaration, and not
  a false alarm (Q14).
- **Declare the category.** No std type is a root type yet: the scan
  finds no `NonEscapable` in `lib/std`. So the Q2 rule costs nothing in
  std today. Each concrete handle-holding data type added later needs one
  line. Generic data declares nothing (NE-S10).
- **Locality marks.** About 400 public marks. Round 3 called them
  optimizer-only, which was too strong (review, "Ordinary Forwarding Is
  Not Optimizer-Only"). A marked parameter has a root in its body, so
  forwarding it to an unmarked parameter is an error (`ne.body.global-arg`),
  and a mark spreads to the helpers it is passed to, `string.len`
  included. Under one flow (NE-S2), a `^{b}` result also carries the root
  to callers that forward their own `^` parameter. They are the lint's
  suggestions, and they feed the facts of
  [12.6](#126-what-the-optimizer-gets). Std adds them where a profile
  shows a gain (Q13).

Collection and iterator methods need markers but no new concepts.
Elements carry their handles in the container's one reach, and callbacks
are function values with an environment.

### 12.6 What The Optimizer Gets

A parameter written `^` tells the optimizer, from the signature alone,
what the callee may keep of the argument (owner, 2026-10-14, D4). Today
`crates/hd_opt/src/value_flow.rs` proves such facts from value flow,
body by body, and `closure_spec` and `devirt_known` read them. The
`escape` and `scalar_replace` passes are registered in
`crates/hd_opt/src/passes.rs` but not implemented (`Instance(not_yet)`).

`LocalParams` is a checked bound on what the callee keeps, and nothing
more. It says nothing about the caller's other aliases, its liveness, or
its effects. Each optimization below lists the further facts it needs
(owner, 2026-10-14,
[NE-S7](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s7-optimizer-proof-boundaries)).

1. r[ne.opt.fact] The signature exports `LocalParams`: for each parameter, whether it is `^`-marked, and its sources (the result, or the objects of other parameters). A marked parameter with no sources is **never kept** by the callee. One with sources is kept only there. The checker verified it (`ne.body.local`), so the optimizer reads it without analysing the callee's body. It is a bound on the callee, not a proof about the caller. (owner, 2026-10-14, D4, NE-S7)
2. r[ne.opt.escape] The `escape` pass may treat a call that passes a value to a never-kept `^` parameter as not escaping through that call. With sources, the value escapes exactly when the named result or object does, whatever its type: a result of an ordinary type, as `identity_bytes` has, is an escape edge, and an empty resource reach never means that an object is dead ([C14](NONESCAPE_LIFETIME_STRESS_TEST.md#c14-an-ordinary-dependent-result-escapes)). An unmarked parameter is assumed to escape. Further precondition: the pass's own value flow in the caller. (owner, 2026-10-14, D4, NE-S2, NE-S7)
3. r[ne.opt.closure] `closure_spec` and `devirt_known` take their licence not to keep a closure from the signature. A closure literal passed to a `^fn` parameter is not stored by the callee. Further precondition: a per-call-site instance from specialization, which may then call the closure directly and drop the closure value afterwards. (owner, 2026-10-14, D4, NE-S7)
4. r[ne.opt.reuse] An object created in a loop body, and passed only to never-kept `^` parameters, may have its allocation reused across iterations. Further precondition, from the caller's liveness: no binding live into the next iteration reaches the object, and no sourced parameter carries it into a value that outlives the iteration, as `saved.push(identity_bytes(b))` does (C14). (owner, 2026-10-14, D4, NE-S7)
5. r[ne.opt.scalar] Scalar replacement of an object passed to a `^` parameter needs inlining or specialization of the callee, so that its body is visible. The mark alone does not license it. (owner, 2026-10-14, NE-S7)
6. r[ne.opt.fusion] An iterator adapter writes `^{self, transform}` on its result. That bounds what the next stage keeps; it does not show that the next stage is the only alias of the previous one. Fusing a chain `it.map(f).filter(g)` into one loop, as #65 wants, needs a further single-consumer alias proof in the caller: no other alias of an intermediate stage is used. A program that advances the source after building the adapter keeps both ([C13](NONESCAPE_LIFETIME_STRESS_TEST.md#c13-a-dependent-iterator-result-is-not-a-unique-pipeline)). (owner, 2026-10-14, D4, NE-S7)
7. r[ne.opt.honest] None of this removes an environment or a cell by itself. Wasm cannot point at locals, so a callee that mutates a captured binding through a `^` closure still needs the closure environment or a cell, unless specialization or inlining makes the binding visible in the callee's body. The mark licenses dropping the environment after specialization, and does not replace specialization. (owner, 2026-10-14, D4)
8. r[ne.opt.parked] Non-local `return` or `break` from a local callback, as Kotlin's inline lambdas allow, is a separate future feature. A `^` parameter does not enable it. It is parked, and is not part of this design. (owner, 2026-10-14, D4)

```hd
fn fill(buf: ^mut List[u8], n: usize) -> void:   # buf is never kept
    for i in 0..n:
        buf.push(1)

fn encode!(frames: List[usize]) -> void:
    for n in frames:
        let buf: mut List[u8] = []               # a fresh object each iteration
        fill(buf, n)                             # never kept
        emit!(buf)                               # emit! marks buf ^ too: no binding keeps buf
```

> **Why.** The facts exist, but each needs a proof today. `ValueFlow`
> sees a closure only when it is made in the call's own body, or passed on
> unchanged. A signature mark proves the callee's half across calls, and
> so across files. The caller's half (its aliases, its liveness, its
> effects) stays the optimizer's own proof.

## 13. Stress-Test Suite

The suite uses this prelude. It drops `Result` handling for brevity; the
real `std.net` methods return `Result` and take `mut self`. Every
NonEscapable parameter is marked (D1), and receivers use the shorthand
`^self` (Q10). `close` is an ordinary method, since closing is parked.

```hd
trait NonEscapable                # placeholder std marker

trait Socket < NonEscapable:
    fn send!(^self, bytes: List[u8]) -> void
    fn read!(^self) -> string
    fn sent(^self) -> usize
    fn close(^self) -> void       # an ordinary method: closing is parked

trait Acceptor < NonEscapable:
    fn accept!(^self) -> ^{} Conn
    fn close(^self) -> void

type Conn = dyn Socket
type Listener = dyn Acceptor

fn connect!(host: string) -> Conn      # no tracked input: ^{}
fn listen!(host: string) -> Listener
```

The container methods the examples use, under D6:

```hd
impl[T] List[T]:
    fn push(^mut self, v: ^{self} T) -> void
    fn get(^self, index: usize) -> T?
    fn contains(^self, v: ^T) -> bool
    fn clear(^mut self) -> void
```

### 13.1 Verdicts

The fifth column is the round-3 re-check (D1 to D8 and Q1 to Q16). The
last column is the round-4 re-check: closing parked, and NE-S1, NE-S2,
NE-S5, NE-S6, NE-S7, NE-S10, and NE-S11 applied. "Same" means the
verdict stands. "Escape form" means the example that showed a cleanup
error now shows the same flow as an escape error, and its cleanup form
is parked ([P.6](#p6-parked-stress-cases)). Every holder example gains
one line, `impl NonEscapable for S` (Q2), which the table does not repeat
unless it matters. The rows for S3, N5, X1 to X5, and X7 moved to the
parked section.

| # | Source | Verdict | Change forced | Re-check, round 3 | Re-check, round 4 |
| --- | --- | --- | --- | --- | --- |
| R1 | Rust book `longest` | Works | none | same | same; escape form |
| R2 | Rust book dangling reference | Works | none | same | same; escape form |
| R3 | Rust `ImportantExcerpt` | Works | one reach (`ne.data.one-reach`) | same; `Excerpt` declares the category (`ne.data.declare`) | same |
| R4 | Rust `Parser<'c, 's>` | Works with one lifetime | none | same; `Router` declares it | same |
| R5 | Rust slice iterator | Works | none | same; `ConnIter` declares it | same |
| R6 | Rustonomicon `evil_feeder` | Works; loop form rejected, as in Rust | example corrected | same; the unmarked signature is now rejected at the call | same; escape form; the loop form is L4 |
| R7 | Rust `thread::spawn` | Works | prefix notation | same; `register` loses `^{static}`, a handle-capturing job is rejected at its call | same |
| R8 | Rust reassignment | Works | none | same | same |
| R9 | Rust `TcpListener::accept` | Needs `^{}` | `^{}`; `overtied-result` warning | **changed** in round 3: `accept!` only; `Net` methods elide to `^{}` | same; escape form |
| R10 | Rust borrow across `.await` | Works | none | same | same; the frame's `run` reaches `c` (NE-S5) |
| R11 | Rust `std::thread::scope` | Works | none | same; `all_list!` marks `tasks` `^` | `broadcast!` same; `fan_out!` is a cleanup case, parked |
| R12 | Rust lifetime-generic method | Works with one path | slot paths on methods | same; `^{self.u}` | same |
| R13 | Rust `HashMap` of handles | Works; mixed map rejected, as in Rust | known limit L2 | same; the map's one reach replaces its value slot | same; escape form |
| R14 | Rust `Option<&T>` with `?` | Works | none | same | same |
| R15 | Rust self-referential struct | Works, unlike Rust | none | same; `Lines` and `Reader` declare it | same |
| S1 | Swift `Span` | Works | none | same | same |
| S2 | Swift `@lifetime(self: copy other)` | Works | none | same; `push` writes `^{self}` | same |
| S4 | Swift `MutableSpan`, `@_lifetime(&self)` | Works, no exclusivity | none | same | same |
| S5 | Swift conditional `Escapable` | Works | generic data follows arguments | same; a concrete holder without the impl is rejected, a generic one follows its arguments | same; generic data follows its stored fields (NE-S10) |
| S6 | Swift non-escaping closure parameter | Works | none | same; the parameter needs `^` | same |
| M1 | Mojo `ref [a, b]` | Works | none | same | same |
| M2 | Mojo struct origin parameter | Works | none | same; no declared slot needed | same |
| M3 | Mojo capturing versus escaping closures | Works | none | same | same; `tally`'s closure reaches nothing and may be stored globally (C20) |
| T1 | Two independent slots | Works | none | same | same; escape form |
| T2 | Swapping two slots | Rejected, as in Rust | none | same | same |
| T3 | REPL top-level handle | Works | static roots | same; a `defer` that closes it is `cleanup-static-root` (Q8) | same; the `defer` half is parked |
| D1 | A handle in a `Result` through `?` | Rejected correctly | `ne.outlive.return` | same; the erased error can no longer be downcast (Q6) | same; escape form |
| D2 | A cold `Suspend` in a field | Rejected; strict | known limit L1 | same; `Worker` needs no impl, since a `Suspend` field does not count | same; escape form; the holder takes on `run` |
| D3 | `race!` and cancellation with a `defer` | Works | `ne.suspend.cancel-order` | same; `race!` marks `tasks` `^` | same; the value joins the `out` slots; the cancellation half is parked |
| D4 | A NonEscapable provider | Works | key roots `^{$Pool}` | same | same |
| D5 | `Map[string, Conn]` in a slot | Works | none | **changed** in round 3: no declared slot needed (D6) | same |
| D6 | `dyn Conn` list mixing lifetimes | Works | `ne.erase.join` | same | same |
| D7 | Recursive data of handles | Works | `ne.data.recursive` | **changed** in round 3: implicit reach; wrapper rule for declared slots | same |
| D8 | Callback registry | Works | `^{static}`, top-level elision | **changed** in round 3: a closure that captures a handle is rejected at the call | same |
| N1 | Unmarked NonEscapable parameter | Rejected, fix-it | D1 | new | same |
| N2 | Generic and `dyn` opt-in | Works | D2 | new | same |
| N3 | General locality | Works | D4 | new | **changed**: one flow; a result that names `b` carries its root (NE-S2) |
| N4 | Function types | Works | D8 | new | **changed**: `retry!` and `all_list!` write their results (NE-S6); `sizes!` writes `^{pool}` (NE-S2, L9) |
| N6 | A concrete holder declares the category | Rejected, fix-it | Q2 | new | same |
| X6 | Widening through a downcast | Hole | `ne.erase.downcast` | same hole; closed by the whole rule: any non-static downcast (Q6) | same; NE-S1 closes the same attack without a downcast ([13.10](#1310-review-regression-cases)) |

Round 4 changed two verdicts in substance, N3 and N4, and turned eleven
examples into escape forms, whose cleanup forms are parked. Under the
round-4 rules, the review's cases C01 to C04, C08, C09, and C18 are no
longer breaks, and C20 is no longer friction ([13.10](#1310-review-regression-cases)).
Round 3 changed five verdicts in substance: R9, D5, D7, D8, and the way
X1 arose. R6, R7, and S5 kept their verdict but gained a rejected form
that the global default produces.

### 13.2 Rust

#### R1. `longest`

Rust: `fn longest<'a>(x: &'a str, y: &'a str) -> &'a str`.

```hd
fn longer(a: ^Conn, b: ^Conn) -> ^{a, b} Conn:
    if a.sent() > b.sent(): a else: b

fn pick(keep: ^Conn, other: ^Conn) -> ^{keep} Conn:   # two inputs: written
    longer(keep, other)       # error: lifetime-mismatch: the result reaches other
```

The cleanup form, which closed the second handle in a `defer`, is parked
([P.6](#p6-parked-stress-cases)).

#### R2. A Dangling Reference

```hd
fn dangle(c: ^Conn) -> ^{} Conn:
    c                         # error: lifetime-mismatch: ^{} names no input

fn fresh!() -> Conn:          # elided: ^{}
    connect!("db")            # fine: a new handle
```

The cleanup form, which returned a handle closed by its own `defer`, is
parked.

#### R3. `ImportantExcerpt`

Rust: `struct ImportantExcerpt<'a> { part: &'a str }`.

```hd
data Excerpt:
    conn: Conn                # one reach for the whole value

impl NonEscapable for Excerpt

fn excerpt(c: ^Conn) -> Excerpt:     # elided: ^{c}
    Excerpt { conn: c }
```

Round 2 wrote `data Excerpt < NonEscapable:`, which is not hd syntax
(data types take no supertrait list). A concrete holder declares the
category in an `impl`, and the checker verifies it against the field
(`ne.data.declare`). It has no slot declaration (`ne.data.one-reach`). A
parameter of type `Excerpt` is NonEscapable, so it must carry `^`
(`ne.param.nonescapable`).

#### R4. `Parser<'c, 's>`

Rust needs two lifetimes because the parser borrows a temporary context.

```hd
data Router:
    pool: List[Conn]

impl NonEscapable for Router

impl Router:
    fn pick(^self) -> Conn:               # elided: ^{self}
        self.pool[0]

fn route(pool: ^List[Conn]) -> Conn:      # elided: ^{pool}
    Router { pool: pool }.pick()          # reaches pool's roots: fine
```

A lifetime is the set of handles a value reaches, never the life of the
value itself. A reference to a context carries no lifetime of its own.
Round 2 wrote `fn pick(self: Router)` as a free function, which hd does
not allow; methods live in `impl` blocks (`fn.method.form`).

#### R5. A Slice Iterator

Rust: `fn next(&mut self) -> Option<&'a T>`.

```hd
data ConnIter:
    conns: List[Conn]
    at: usize

impl NonEscapable for ConnIter

impl ConnIter:
    fn next(^mut self) -> Conn?:          # elided: ^{self}, the conns it holds
        found := self.conns.get(self.at)
        self.at = self.at + 1
        found
```

hd has no borrows, so no `&mut self` lifetime is kept apart from `'a`,
and there is no lending-iterator problem.

#### R6. Evil Feeder

Rustonomicon: invariance under `&mut`.

```hd
data Box[T]:
    value: T                          # generic: follows its stored field, no impl

fn feed[T](slot: ^mut Box[T], v: ^{slot} T) -> void:   # v is stored into slot
    slot.value = v

fn main!(tmp: ^Conn) -> ^{} Box[Conn]:
    keep := Box { value: connect!("a") }
    feed(keep, tmp)                   # keep's reach now includes tmp (invariant under mut)
    keep                              # error: lifetime-mismatch: keep reaches tmp, and ^{} names no input
```

`keep`'s reach grows by the store, because a `mut` reach is invariant
and cannot be narrowed back. The program is a true alarm at the signature
level: the caller of `main!` would receive a box that holds `tmp`. A loop
that feeds a loop-carried target reaches every iteration's roots (known
limit L4). The cleanup form, with the `if` block whose `defer` closes
`tmp`, is parked.

Round 2 wrote `fn feed[T](slot: mut Box[T], v: T)`. Under D2 both
parameters are global, so `feed(keep, tmp)` is rejected at the call, since
`keep` and `tmp` reach local roots (`ne.call.global`). The `^{slot}` form
keeps the original lesson.

#### R7. `thread::spawn` Versus Structured Children

```hd
use std.task.all

fn pump!(from: ^Conn, to: ^Conn) -> void:
    to.send!([0])

fn mirror!(a: ^Conn, b: ^Conn) -> void:
    _ := all!(pump(a, b), pump(b, a))    # fine: both frames finish inside the call

let JOBS: mut List[fn!() -> void] = []   # top level: reaches static

fn later(c: ^Conn) -> void:
    JOBS.push(fn!(): c.send!([1]))       # error: lifetime-mismatch: the closure reaches c

fn register(job: fn!() -> void) -> void: # unmarked: global, like Swift's @escaping
    JOBS.push(job)                       # fine: job reaches only static

fn launch(c: ^Conn) -> void:
    register(fn!(): c.send!([1]))        # error: lifetime-mismatch: a global parameter, a local root
```

`register` was `job: ^static fn!() -> void`. Unmarked now says the same,
so `^{static}` is not written on a parameter.

#### R8. Reassignment

```hd
fn best(a: ^Conn, b: ^Conn) -> ^{a} Conn:
    let pick = b
    _ := pick.sent()
    pick = a
    pick                      # fine: the binding's reach follows its last assignment
```

#### R9. `accept`

In Rust an owned result has no lifetime. Here the one-input and self
rules would tie the accepted connection to its listener:

```hd
fn handoff!(l: ^Listener) -> ^{} Conn:
    l.accept!()               # fine with ^{}; with the elided ^{self}, an error: the result reaches l
```

So `accept!` writes `-> ^{} Conn`. The `overtied-result` warning tells
an implementer whose body returns a fresh handle under an elided input.
A trait method has no body, so its author must write `^{}`. The cleanup
form, which closed the listener in a `defer`, is parked.

Round 2 needed `^{}` four times in std, on `accept!` and on `Net`'s
`connect!`, `listen!`, and `bind_udp!`. Under D1 the `Net` receiver is
unmarked, so it is not a tracked input, and those three results elide to
`^{}` by `ne.elide.none`. std needs it once
([12.5](#125-annotation-burden-in-std-estimate)).

#### R10. A Borrow Across A Suspension

Rust: an `async fn` holds `&TcpStream` across `.await`, and
`tokio::spawn` needs `'static`.

```hd
let QUEUE: mut List[mut Suspend[void]] = []

fn echo!(c: ^Conn) -> void:
    _ := c.read!()            # the frame holds c across both bang calls
    c.send!([1])

fn serve!(l: ^Listener) -> void:
    c := l.accept!()
    echo!(c)                  # fine: the bang call keeps nothing
    QUEUE.push(echo(c))       # error: lifetime-mismatch: the frame's run reaches c, QUEUE reaches static
```

`QUEUE` is hd's `tokio::spawn`: storing a frame in module state needs a
`static` reach for its `run` slot, as spawning needs `'static`.

#### R11. `std::thread::scope`

```hd
use std.task.all_list

fn broadcast!(c: ^Conn, frames: List[List[u8]]) -> void:
    _ := all_list!([for f in frames => fn!() -> void: c.send!(f)])   # fine
```

`all_list!` is the scope: its children may capture handles of the
enclosing block. `all_list!` marks `tasks` `^`, so a list of closures
that capture a handle is a legal argument (`ne.all.list`). The second
half, a closure list that outlives the block whose `defer` closes the
captured handle, is a cleanup case and is parked.

#### R12. A Lifetime-Generic Method

Rust: `impl<'a> Parser<'a> { fn part<'b>(&self, key: &'b str) -> &'a str }`.

```hd
impl Session:
    fn route(^self, hint: ^Conn) -> ^{self.u} Conn:   # neither all of self nor hint
        self.upstream
```

Elided, the result would be `self` by the self rule, the union of both
slots, and a caller who keeps the result with only the upstream's reach
would get a false alarm. One slot path fixes it. No change: slot paths on
methods already exist (`ne.data.path`). `Session` declares two slots
because its handles are independent (`ne.data.slots`), and it declares
the category (`impl NonEscapable for Session`, [5.2](#52-declared-slots)).

#### R13. A `HashMap` Of Handles

```hd
fn mixed(long: ^Conn, tmp: ^Conn) -> ^{long} Conn?:
    let by_name: mut Map[string, Conn] = {}
    by_name["long"] = long
    by_name["tmp"] = tmp
    by_name.get("long")       # error: lifetime-mismatch: by_name reaches tmp too
```

Rust rejects the same program: a `HashMap<&str, &'a S>` has one `'a`.
Under D6 the verdict is the same for a plainer reason: a `Map` has one
reach, the union of everything stored in it, and an index store adds to it
(`ne.heap.store`). Removing `"tmp"` first does not help, because a heap
reach is one reach for the object's life (known limits L1 and L2). The
workaround is a separate map per source. A map of two independent kinds
of handle would need a data type with declared slots. The cleanup form,
with `tmp` closed in an inner block, is parked.

#### R14. `Option<&T>` With `?`

```hd
fn first_open(pool: ^List[Conn]) -> Conn?:     # elided: ^{pool}
    c := pool.get(0)?
    if c.sent() == 0: return .None
    .Some(c)
```

`get` elides to `self`, so `c` reaches `pool`'s reach. The signature of
`first_open` writes one `^`, on the parameter.

#### R15. A Self-Referential Struct

Rust cannot let a field borrow a sibling field.

```hd
data Lines:
    conn: Conn

impl NonEscapable for Lines

data Reader:
    conn: Conn
    lines: Lines              # a view over conn: the same root

impl NonEscapable for Reader

fn reader(c: ^Conn) -> Reader:        # elided: ^{c}
    Reader { conn: c, lines: Lines { conn: c } }
```

It works. A lifetime is a set of roots, not a memory region, and hd has
no moves, so nothing invalidates the view.

### 13.3 Swift

#### S1. A View Over A Handle

Swift: `Span` with `@_lifetime(borrow self)`.

```hd
impl Lines:
    fn next!(^self) -> string: self.conn.read!()

fn lines(c: ^Conn) -> Lines:          # elided: ^{c}
    Lines { conn: c }

fn first_line!(l: ^Listener) -> string:
    c := l.accept!()
    lines(c).next!()                  # fine: a string carries no lifetime
```

#### S2. A Mutating Store

Swift: `@_lifetime(self: copy other)`.

```hd
data Pool:
    conns: mut List[Conn]

impl NonEscapable for Pool

impl Pool:
    fn adopt(^mut self, c: ^{self} Conn) -> void:
        self.conns.push(c)
```

`List[Conn]` carries no lifetime in its type. `push` writes
`v: ^{self} T`, so `self.conns.push(c)` adds `c`'s reach to the list, and
so to `self`, as `c: ^{self} Conn` allows. Swift does not yet allow an
`Array` of `~Escapable` elements. `Pool`'s reach is invariant, because
its `conns` field is mutable storage (NE-S1).

S3, the cleanup wrapper, is parked ([P.6](#p6-parked-stress-cases)).

#### S4. `MutableSpan` And `@_lifetime(&self)`

Swift's `mutableSpan` property depends on `self` through an exclusive
mutable borrow (`&`).

```hd
data Writer:
    conn: Conn
    buffer: mut List[u8]

impl NonEscapable for Writer

impl Writer:
    fn flush!(^mut self) -> void:
        self.conn.send!(self.buffer)
        self.buffer = []

fn writer(c: ^Conn) -> Writer:        # elided: ^{c}
    Writer { conn: c, buffer: [] }
```

It works. Two writers over one `c` may coexist: hd promises no
exclusivity, only that neither escapes further than `c`. Swift's `&`
scope buys exclusivity, which hd does not want. `send!`'s `bytes`
parameter is unmarked and global, and `self.buffer` reaches nothing, so it
fits.

#### S5. Conditional `Escapable`

Swift: `struct Tagged<T: ~Escapable>: ~Escapable` plus
`extension Tagged: Escapable where T: Escapable`.

```hd
data Tagged[T]:
    tag: string
    value: T                          # follows its stored field: no impl

data Labeled:
    tag: string
    conn: Conn                        # error: missing-nonescapable-impl
                                      # fix-it: impl NonEscapable for Labeled

let SEEN: mut List[Tagged[i32]] = []
let SEEN_CONNS: mut List[Tagged[Conn]] = []

fn remember(t: Tagged[i32]) -> void:
    SEEN.push(t)                      # fine: Tagged[i32] carries nothing

fn remember_conn(t: Tagged[Conn]) -> void:   # error: unmarked-local-param
    pass                                     # fix-it: t: ^Tagged[Conn]

fn remember_conn(t: ^Tagged[Conn]) -> void:
    SEEN_CONNS.push(t)                # error: lifetime-mismatch: SEEN_CONNS reaches static
```

Nothing is written on `Tagged`: generic data follows its stored fields
(`ne.data.generic`). `Tagged[Conn]` stores a `Conn` in `value`, so it is
NonEscapable, and the unmarked parameter is a declaration error
(`ne.param.nonescapable`). `Labeled` is concrete, so it declares the
category itself, and the checker reads its field (`ne.data.declare`).
That is the owner's choice (Q2) over a rule that lets every holder follow
its fields silently.

#### S6. A Non-Escaping Closure Called Later In The Call

```hd
fn first_ok!(n: usize, attempt: ^fn!() -> bool) -> bool:
    let tries: mut List[fn!() -> bool] = []
    for _ in 0..n:
        tries.push(attempt)           # a local list: its reach never flows out
    for t in tries:
        if t!(): return true
    false
```

It works: `tries` is a local object, and its reach never reaches the
result or a global. `attempt` must be marked `^`: unmarked, the closure's
callers could not pass a closure that captures a handle (`ne.param.fn`).
`attempt` is kept nowhere, because `tries` never reaches the result.
Storing `attempt` in `self` would need `attempt: ^{self} fn!() -> bool`.
Swift does not let a non-escaping closure enter an array at all; hd's
reach rule is more permissive.

### 13.4 Mojo

#### M1, M2. Origins

`fn pick(a, b) -> ref [a, b] String` is `-> ^{a, b} Conn`.
`struct Span[T, origin: Origin]` is `data Window` with one reach, and a
`ref [self]` result is the elided `^{self}`. A declared slot is needed only
for independent handles (`ne.data.slots`).

#### M3. Capturing Versus Escaping Closures

Mojo's reference captures (`{imm x}`, `{mut x}`) cannot outlive their
scope. A copy capture (`{var x}`, `@__copy_capture`) detaches the closure
from the original, so it may escape.

```hd
fn counter(c: ^Conn) -> fn() -> usize:    # elided: ^{c}
    fn() -> usize: c.sent()

fn tally(n: usize) -> fn() -> usize:      # elided: ^{}, no tracked input
    fn() -> usize: n
```

It works with one rule. Copying a handle does not detach it from its
root, so hd needs no copy-capture escape: a closure reaches exactly what
it captures, by value or by binding (`ne.closure.reach`). `tally`'s
closure captures only a scalar, so its reach is empty, and its `^{}`
result mints no root at the caller (`ne.call.fresh`): it may be stored in
a global ([C20](#c20-a-scalar-only-closure-factory)). The result type
`fn() -> usize` has no parameter, so its own result depends on its
environment, and `usize` is a scalar.

### 13.5 Two Slots And The REPL

#### T1. Independent Slots

```hd
fn client_of(s: ^Session) -> ^{s.c} Conn:
    s.client

fn proxy!(l: ^Listener, up: ^Conn) -> ^{} Conn:
    client_of(Session { client: l.accept!(), upstream: up })   # fine: only the accepted handle's fresh root
```

With `^{s}` instead of `^{s.c}`, the result would reach `up`, and `proxy!`
would be a `lifetime-mismatch`. The cleanup form, with the upstream
closed in a `defer`, is parked.

#### T2. Swapping Slots

```hd
fn swap_in(s: ^mut Session) -> void:
    s.client = s.upstream     # error: lifetime-mismatch: ^{s.u} does not fit ^{s.c}
```

As in Rust, two distinct slots never take each other's values.

#### T3. A REPL Handle

A REPL input `c := connect!("db")` binds a static root. Later inputs may
use it, and may pass it to a global parameter, since its reach fits
`static` (`ne.top.static`). The round-3 rule that a `defer` closing it is
an error is parked (Q8).

### 13.6 hd-Specific

#### D1. A Handle In A `Result` Through `?`

```hd
enum LinkError:
    Broken(conn: Conn)                # the error holds the failed handle

impl NonEscapable for LinkError

fn check!(c: ^Conn) -> Result[void, LinkError]:    # elided: the error reaches c
    if c.sent() == 0: return .Err(LinkError.Broken(c))
    .Ok(())

fn talk!(c: ^Conn) -> Result[void, LinkError]:     # elided: ^{c}
    check!(c)?                # fine: the result names c
    .Ok(())

fn talk_fresh!(c: ^Conn) -> ^{} Result[void, LinkError]:
    check!(c)?                # error: lifetime-mismatch: ? returns an error that reaches c
    .Ok(())
```

`?` is a return, so the propagated error must fit the result's bound
(`ne.body.result`). `LinkError` may erase to `dyn Error` in a local
position, with one reach. That `dyn Error` cannot be downcast, because a
downcast needs a static reach (`ne.erase.downcast`). Its trait methods stay
available. The cleanup form, which propagated a handle closed by the
caller's own `defer`, is parked.

#### D2. A Cold Suspension In A Field

```hd
data Worker:
    next: mut Suspend[void]?          # no impl: a Suspend field does not count

fn arm!(w: ^mut Worker, c: ^Conn) -> void:
    w.next = .Some(echo(c))   # error: lifetime-mismatch: w's reach is the caller's, and c is not named for it

fn staged!(c: ^Conn) -> ^{} Worker:
    w := Worker { next: .None }
    w.next = .Some(echo(c))   # w takes on the frame's run: c
    w.next = .None            # discarded cold: nothing ran
    w                         # error: lifetime-mismatch: w still reaches c
```

`arm!` is a true alarm: `c: ^{w} Conn` would allow it. `staged!` is
strict: the frame is gone, yet `w`'s reach still includes `c`, because a
heap reach is one reach for the object's life (known limit L1, as in
[C12](#c12-a-cleared-list)). Rust rejects the same program: a struct's
lifetime parameter covers its whole life. The smallest fix would be a
strong update for an object no other binding can reach, which Rust's
borrow checker does for locals but not for fields. The owner decided not
to add it for now (Q9). The workaround is to keep the frame in a binding.
`Worker` is not NonEscapable, so its `^{}` result mints no root
(`ne.call.fresh`). The cleanup forms are parked.

#### D3. `race!` And Its Children

```hd
use std.task.race

fn fastest!(a: ^Conn, b: ^Conn) -> string:
    race!(a.read(), b.read())         # the loser is cancelled before race! returns
```

The frames capture `a` and `b`, so their `run` slots reach them, and
`race!` marks its child parameter `^` (`ne.all.typing`), so they are legal
arguments. The value of `race!` joins the children's `out` slots, which
are empty: `read!` returns a `string` that names no parameter. The
round-3 half of this case, that a cancelled child finishes before its
parent's `defer` closes a handle, is a cleanup case and is parked with
`ne.suspend.cancel-order`.

#### D4. A NonEscapable Provider

```hd
trait Pool < NonEscapable:
    fn get(^self) -> Conn             # elided: ^{self}
    fn close(^self) -> void

fn grab() -> Conn $ Pool:             # elided: ^{$Pool}, the only tracked input
    $.use(Pool).get()

fn open_pool!() -> dyn Pool

let KEEP: mut List[Conn] = []

fn serve!() -> void:
    p := open_pool!()
    c := $.with(Pool=p):
        grab()                        # c reaches p
    c.send!([1])                      # fine
    KEEP.push(c)                      # error: lifetime-mismatch: KEEP reaches static
```

The key is a tracked input named `$Pool` (`ne.provider.root`), and it
is not a parameter, so it needs no `^`. The round-3 rule that closing a
provider reached through `$.use` is `closing-not-owned` is parked.

#### D5. `Map[string, Conn]` In A Holder

```hd
data Registry:
    by_name: mut Map[string, Conn]       # one reach: no declared slot

impl NonEscapable for Registry

impl Registry:
    fn add(^mut self, name: string, c: ^{self} Conn) -> void:
        self.by_name[name] = c

    fn find(^self, name: string) -> Conn?:    # elided: ^{self}
        self.by_name.get(name)
```

It works. The value reach is one reach for every value, as in Rust. Round
2 declared `Registry[^p]` and wrote `^self.p`. Under D6 a type with one
reach declares no slot, and `c: ^{self} Conn` says the same. It does
declare the category, because its field is NonEscapable (Q2).

#### D6. A `dyn Conn` List Mixing Lifetimes

```hd
data Fake: pass

impl NonEscapable for Fake

impl Socket for Fake:
    fn send!(^self, bytes: List[u8]) -> void: pass
    fn read!(^self) -> string: ""
    fn sent(^self) -> usize: 0
    fn close(^self) -> void: pass

fn mix!(real: ^Conn) -> void:
    let all: mut List[Conn] = [real, Fake {}]   # reaches real's root and a new one
    for c in all:
        c.send!([1])
```

It works: erasure joins the slots (`ne.erase.join`), and `Fake {}` is a
root site. An error arises only where the list flows past a bound, as in
R13.

#### D7. Recursive Data Of Handles

```hd
enum Chain:
    End
    Link(conn: Conn, next: Chain)     # one reach for the whole chain

impl NonEscapable for Chain

data Duo[^a, ^b]:
    x: ^{a} Conn
    y: ^{b} Conn
    next: ^{a, b} Duo?                # a wrapper: the union of both slots

impl NonEscapable for Duo
```

`Chain` works with no slot declaration. `Duo` is the declared-slot case
(`ne.data.recursive`): the `Option` wrapper has one reach, so a `Duo`
taken out of `next` has both slots at the union (`ne.data.wrapper`,
known limit L5). Round 2 showed `Ring[^a]` with one slot, which is now a
plain holder.

#### D8. A Callback Registry

```hd
let HANDLERS: mut List[fn(string) -> void] = []    # top level: reaches static

fn on_message(h: fn(string) -> void) -> void:      # unmarked: global
    HANDLERS.push(h)

fn log_line(m: string) -> void: pass

fn setup!(l: ^Listener) -> void:
    c := l.accept!()
    on_message(log_line)                  # fine: no captures
    on_message(fn(m): _ := c.sent())      # error: lifetime-mismatch: a global parameter, a local root
```

`h` is the registry's whole annotation, and it is the absence of one.
Round 2 accepted the last line, because `c`'s root was free and so could
escape into `HANDLERS`. D2 forbids that: a NonEscapable value flows only
into local positions. A caller who wants the handler to see a connection
passes it as an argument, or stores the callback in a local object.

### 13.7 Owner Decisions Of 2026-10-14

#### N1. An Unmarked NonEscapable Parameter

```hd
fn ping(c: Conn) -> void:         # error: unmarked-local-param
    c.send!([0])                  # fix-it: write c: ^Conn

fn ping(c: ^Conn) -> void:        # fine
    c.send!([0])
```

D1. An unmarked parameter is global, and a handle never fits a global
position, so the declaration could never be called with a local handle.
The error names the fix.

#### N2. Generic And `dyn` Opt-In

```hd
fn keep_one[T](x: T) -> void: pass
fn look_at[T](x: ^T) -> void: pass
fn inspect_one(x: dyn Inspectable) -> void: pass
fn inspect_local(x: ^dyn Inspectable) -> void: pass

fn demo!(c: ^Conn) -> void:
    keep_one(c)                   # error: lifetime-mismatch: x is global, c reaches a parameter root
    look_at(c)                    # fine
    inspect_one(c)                # error: lifetime-mismatch
    inspect_local(c)              # fine: erased into a local position
    keep_one(3)                   # fine: an ordinary value reaches nothing
```

D2. This is the opt-in of the parked TQ-25, spelled on the parameter. A
generic function that wants to accept a handle writes `^T`, and one that
stores its argument in module state stays unmarked.

#### N3. General Locality

```hd
let CACHE: mut List[List[u8]] = []

fn sum_bytes(b: ^List[u8]) -> usize:      # never kept
    b.len()

fn remember_bytes(b: ^List[u8]) -> void:
    CACHE.push(b)                         # error: lifetime-mismatch: b is local, CACHE reaches static

fn fill(buf: ^mut List[u8]) -> void:
    buf.push(1)                           # fine

fn demo() -> void:
    _ := sum_bytes([1, 2, 3])             # a fresh ordinary object: its reach is empty
```

D4. A `^` parameter of an ordinary type is checked in the callee with the
reach machinery. Round 3 said the caller tracks nothing. Under one flow
(NE-S2) the caller tracks what the signature names: a result written
`^{b}` carries the argument's reach to the caller, so a helper that
returns its parameter cannot launder it into `CACHE`
([C04](#c04-an-identity-helper)). Here the argument is a fresh literal,
whose reach is empty, so `demo` gets no constraint. `ne.opt.fact` makes
`sum_bytes` and `fill` known as never keeping their argument.

#### N4. Function Types

```hd
fn map[T, U](xs: ^List[T], f: ^fn(x: ^T) -> ^{x, f} U) -> ^{xs, f} List[U]
# as the std method: fn map[U](^self, transform: ^fn(x: ^T) -> ^{x, transform} U) -> ^{self, transform} List[U]
fn each(xs: ^List[Conn], f: ^fn(c: ^Conn) -> void) -> void
fn retry![T, E, $R](times: usize, attempt: ^fn!() -> ^{attempt, $R} Result[T, E] $ R) -> ^{attempt, $R} Result[T, E] $ R
fn all_list![T, $R](tasks: ^List[fn!() -> ^{tasks, $R} T $ R]) -> ^{tasks, $R} List[T] $ R

fn bad[T, U](f: ^fn(x: ^T) -> U) -> U    # error: ambiguous-lifetime: the function type has x and its environment

fn sizes!(pool: ^List[Conn]) -> ^{pool} List[usize]:
    pool.map(fn(c): c.sent())            # the result reaches pool, by map's signature

fn sizes_unwritten!(pool: ^List[Conn]) -> List[usize]:
    pool.map(fn(c): c.sent())            # error: lifetime-mismatch: the result reaches pool
```

D8. `map` writes both sources on the function type and on its own result,
because a mapped element may reach `xs` or the closure's captures. `each`
returns `void`, so there is no result to name. `retry!` and `all_list!`
are **changed** in round 4: each callback type has its environment and
the row parameter `$R` as tracked inputs, and so has each outer
signature, so both results are written (NE-S6). `bad` leaves the function
type's result unnamed with two tracked inputs. Round 2 gave such a type
the union by default, which was the one guess left (known limit L3, now
resolved). The same signature as a result or a field could not name the
environment at all (`ne.fntype.outside`, L6).

`sizes!` is **changed** in round 4. The call instantiates `^{xs, f}` with
`pool` and the closure. The closure captures nothing, so its reach is
empty (`ne.closure.reach`), but `pool`'s root enters the result, though
`List[usize]` holds no handle. Round 3 dropped it (`ne.fntype.carry-only`);
one flow keeps it (`ne.fntype.one-flow`). The over-tie is the price of
exact substitution (known limit L9). `sizes!` writes `^{pool}`, and a
caller that passes a fresh ordinary list gets a result with an empty
reach.

#### N6. A Concrete Holder Declares The Category

```hd
data Pair:
    a: Conn
    b: Conn                       # error: missing-nonescapable-impl

impl NonEscapable for Pair        # the fix-it

data Callbacks:
    on_open: fn() -> void         # fine: a function-typed field does not count
```

Q2. A concrete data or enum type with a NonEscapable field writes the
impl, and the checker compares it with the fields (`ne.data.declare`).
`Callbacks` declares nothing. A closure still carries a reach, so a
`Callbacks` value that holds a closure over a local handle is rejected by
the reach rules where it flows to a global position, as in D8.

N5, `closing` implies `^`, is parked ([P.6](#p6-parked-stress-cases)).

### 13.8 Holes Found

X1 to X5 and X7 are cleanup and closing holes, parked with their fixes
([P.6](#p6-parked-stress-cases)). X6 stays active.

#### X6. Widening Through A Downcast

```hd
use std.inspect.Inspectable

data Stash:
    items: mut List[Conn]             # mutable storage: the reach is invariant

impl NonEscapable for Stash

fn smuggle(s: ^mut Stash, tmp: ^Conn) -> void:
    let e: mut dyn Inspectable = s
    let wide: ^{s, tmp} mut dyn Inspectable = e  # covariant: Inspectable's surface has no mutable storage
    match wide.downcast_mut::[Stash]():          # error: downcast-non-static
        .Some(back) => back.items.push(tmp)      # would put tmp into the caller's list
        .None => pass
```

Round 1 said `downcast` returns the `dyn` value's reach. Widening the
`dyn` reach first and downcasting then widens an invariant reach. The
previous revision forbade only a downcast to a type with an invariant
reach. Fix, by the owner (Q6): a downcast needs a static reach
(`ne.erase.downcast`), as in Rust. Here `wide` reaches `s` and `tmp`, so
it is rejected. So would a downcast of `e` alone, which reaches `s`'s
root. The downcast results reach only `static`, so they do not make
`dyn Inspectable` invariant. The review's C01 to C03 rebuild the same
attack without a downcast, through a getter, a callback, and a nested
element; NE-S1 closes those
([13.10](#1310-review-regression-cases)). Borrows: Rust's `Any: 'static`.

### 13.9 Known Limits

| # | Limit | Example | Rust |
| --- | --- | --- | --- |
| L1 | A heap reach keeps a root after the value is overwritten or the container is cleared | D2, [C12](#c12-a-cleared-list) | same |
| L2 | A container's values share one reach | R13 | same |
| L3 | A function type's result is the union of its inputs and captures | `ne.fntype.result` | **resolved**: D8 elides it like a function (owner, 2026-10-14) |
| L4 | A loop-carried mutable target reaches every iteration's roots | R6 loop form | same |
| L5 | A wrapper of declared-slot data joins its slots | D7, [C11](#c11-an-optional-pair) | none: Rust's `Option<Pair<'a, 'b>>` keeps both |
| L6 | A function type outside a parameter cannot name its environment | N4, `ne.fntype.outside` | none: Rust names no closure environment |
| L7 | All evaluations of one root site share one local root | [C15](#c15-one-root-per-creation-site) | none: Rust's moves give each value its own owner |
| L8 | A local `^dyn` value cannot be downcast, even when it holds an ordinary value | [C19](#c19-a-local-classifier) | narrower: Rust tests the erased type's contents |
| L9 | A generic result carries every named source, even when the instantiated result holds no handle | N4 `sizes!` | none: an owned Rust result drops the lifetime |

L1 and L2 are false alarms in the strict sense, though the alarm is true
at the signature level: the object does hold, or did hold, a value that
reaches the root. The owner decided to keep L1 and add no strong updates
for now (Q9). L3 is resolved, and no union default is left. L5 is
accepted (Q15). L6 is the known limit of Q12, accepted by the owner. L7
and L8 are stated by the owner (NE-S11); L8 is the cost of Q6. L7 is
inert while closing is parked: a local root is constrained only by where
it flows, and every evaluation of one site flows to the same places. It
matters once something ends a local root's lifetime ([14.2](#142-open)).
L9 follows from exact substitution (requirement 10).

### 13.10 Review Regression Cases

The review [NONESCAPE_LIFETIME_STRESS_TEST.md](NONESCAPE_LIFETIME_STRESS_TEST.md)
tested round 3 with twenty cases. These are its cases under the round-4
rules, in this doc's notation, with the review's verdict and the new one.
C05, C06, C07, C10, and C17 are cleanup and closing cases, parked
([P.6](#p6-parked-stress-cases)). Where a case's code used a `defer`
only to set the stage, the escape form below keeps the flow and drops the
`defer`.

| Case | Review verdict | Round 4 | Rule |
| --- | --- | --- | --- |
| C01 | Breaks | Rejected at the widening | `ne.var.dyn` (NE-S1) |
| C02 | Breaks | Rejected at the widening | `ne.var.fn` (NE-S1) |
| C03 | Breaks | Rejected at the widening | `ne.var.generic` (NE-S1) |
| C04 | Breaks | Rejected at the store | `ne.param.ordinary-result` (NE-S2) |
| C08 | Breaks | Works | `ne.suspend.*`, `ne.all.typing` (NE-S5) |
| C09 | Breaks | Works when written; an ambiguous error otherwise | `ne.fntype.elide` (NE-S6) |
| C11 | Friction | Friction, known limit L5 | `ne.data.wrapper` (NE-S11) |
| C12 | Friction | Friction, known limit L1 | Q9 (NE-S11) |
| C13 | Breaks (optimizer) | Accepted; fusion needs its own proof | `ne.opt.fusion` (NE-S7) |
| C14 | Works, with an obligation | Works; the escape edge is kept | `ne.opt.escape`, `ne.opt.reuse` (NE-S2, NE-S7) |
| C15 | Friction | Known limit L7, inert while closing is parked | `ne.root.site` (NE-S11) |
| C16 | Works | Works; the cancellation half is parked | `ne.all.typing` |
| C18 | Breaks | Works: `Tag[Conn]` is ordinary | `ne.data.category` (NE-S10) |
| C19 | Friction | Friction, known limit L8 | `ne.erase.downcast` (NE-S11) |
| C20 | Friction | Works | `ne.closure.reach`, `ne.call.fresh` (NE-S11) |

#### C01. A Trait Getter

```hd
trait Window:
    fn items(^mut self) -> ^{self} mut List[Conn]

fn insert_hidden(w: ^mut dyn Window, c: ^Conn) -> void:
    let wide: ^{w, c} mut dyn Window = w    # error: lifetime-mismatch: items() exposes mutable storage, so the reach is invariant
    wide.items().push(c)
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c01-a-trait-getter-replaces-the-downcast).
`items` returns mutable storage, so the reach of `dyn Window` is invariant
(`ne.var.dyn`). Without the widening, `w.items().push(c)` is rejected
too: the list reaches `w`, and `c` is not named for `w`'s object
(`ne.heap.fixed`). The fix is `c: ^{w} Conn`.

#### C02. A Closure Returning Mutable State

```hd
fn put_through_getter(get: ^fn() -> mut List[Conn], c: ^Conn) -> void:
    let wide: ^{get, c} fn() -> mut List[Conn] = get   # error: lifetime-mismatch: the callable result is mutable storage
    wide().push(c)
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c02-a-closure-returns-mutable-captured-state).
The callback's result elides to its environment, and it is typed `mut`,
so the function value's reach is invariant (`ne.var.fn`).

#### C03. A Readonly List Of Mutable Lists

```hd
fn put_through_row(rows: ^List[mut List[Conn]], c: ^Conn) -> void:
    let wide: ^{rows, c} List[mut List[Conn]] = rows  # error: lifetime-mismatch: the element type is mutable storage
    wide[0].push(c)
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c03-a-readonly-container-contains-mutable-children).
The outer view is readonly, but `types.readonly.nested` keeps the inner
lists mutable, so the container's reach is invariant (`ne.var.generic`).
`rows[0].push(c)` without the widening is rejected by `ne.heap.fixed`.
Since the helper is rejected, the review's caller, which keeps no
dependency on `c`, is safe.

#### C04. An Identity Helper

```hd
let CACHE: mut List[List[u8]] = []

fn identity_bytes(b: ^List[u8]) -> ^{b} List[u8]:
    b

fn hidden_retention(b: ^List[u8]) -> void:
    CACHE.push(identity_bytes(b))     # error: lifetime-mismatch: the result carries b's root

fn fresh_ok() -> void:
    CACHE.push(identity_bytes([1]))   # fine: a fresh ordinary object reaches nothing
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c04-ordinary-locality-through-an-identity-helper).
One flow (NE-S2): the `^{b}` result carries the argument's reach, which
in `hidden_retention` is `b`'s parameter root. Round 3's "the caller
tracks nothing" would have let the helper launder the store that N3
forbids directly.

#### C08. A Frame And Its Output

```hd
use std.task.all

fn select!(keep: ^Conn, scratch: ^Conn) -> ^{keep} Conn:
    _ := scratch.sent()
    keep

fn via_frame!(keep: ^Conn, scratch: ^Conn) -> ^{keep} Conn:
    let (out,) = all!(select(keep, scratch))   # run: keep, scratch; out: keep
    out                                        # fine: all! joins the outs, never the runs
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c08-a-frame-retains-more-than-its-output-depends-on).
With one reach per frame, `out` would reach `scratch`, a false alarm.
The two slots keep them apart (`ne.suspend.cold`, `ne.all.typing`). A
helper that drives a frame it is passed writes `^{s.out}`
(`ne.suspend.elide`). The review's form closes `scratch` in a `defer`;
that half is parked.

#### C09. A Callback From A Provider

```hd
trait Pool < NonEscapable:
    fn get(^self) -> Conn             # elided: ^{self}

fn from_pool() -> Conn $ Pool:        # elided: ^{$Pool}
    $.use(Pool).get()

fn invoke(f: ^fn() -> ^{$Pool} Conn $ Pool) -> ^{$Pool} Conn $ Pool:
    f()                               # fine; invoke(from_pool) fits

fn invoke_bad(f: ^fn() -> Conn $ Pool) -> ^{$Pool} Conn $ Pool   # error: ambiguous-lifetime: f's environment and $Pool
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c09-a-callback-obtains-its-result-from-a-provider).
The callback type counts its environment and its key `$Pool`, as a
function would (`ne.fntype.elide`). Two tracked inputs make the result
ambiguous, so it is written. `from_pool` has an empty environment, and
its result names `$Pool`, so it fits.

#### C11. An Optional Pair

```hd
data Pair[^a, ^b]:
    first: ^{a} Conn
    second: ^{b} Conn

impl NonEscapable for Pair

fn first_of(p: ^Pair) -> ^{p.a} Conn:
    p.first                            # fine: one slot

fn first_optional(p: ^Pair?) -> ^{p} Conn:   # the Option joins both slots: ^{p.a} cannot be written
    match p:
        .Some(pair) => pair.first
        .None => panic("missing pair")
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c11-optional-wrapping-destroys-independent-slots).
Friction, known limit L5 (Q15). The workaround is a dedicated holder that
declares its slots.

#### C12. A Cleared List

```hd
fn reuse_list(c: ^Conn) -> ^{} List[Conn]:
    let items: mut List[Conn] = []
    items.push(c)
    items.clear()
    items                     # error: lifetime-mismatch: items still reaches c
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c12-an-empty-container-remains-contaminated).
Friction, known limit L1: no strong updates (Q9). A fresh list for each
use works. The review's form closes the handle in a `defer`; this escape
form keeps the same flow.

#### C13. A Shared Iterator

```hd
fn shared_pipeline(xs: List[usize]) -> List[usize]:
    let source = xs.iter()
    let mapped = source.map(fn(n): n + 1)
    _ := source.next()
    mapped.collect()
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c13-a-dependent-iterator-result-is-not-a-unique-pipeline).
The checker accepts it. The adapter's `^{self, transform}` bounds what
`mapped` keeps, but `source` is still used, so the single-consumer proof
that `ne.opt.fusion` needs fails, and the chain is not fused (NE-S7).

#### C14. An Ordinary Result Escapes

```hd
fn retain_examples() -> List[List[u8]]:
    let saved: mut List[List[u8]] = []
    for _ in 0..2:
        let b: mut List[u8] = []
        saved.push(identity_bytes(b))
    saved
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c14-an-ordinary-dependent-result-escapes).
Works: `b` is a fresh ordinary object, so its reach is empty and the
checker accepts. The optimizer keeps the `^{b}` result as an escape edge
(`ne.opt.escape`), so `b`'s allocation is not reused across iterations
(`ne.opt.reuse`).

#### C15. One Root Per Creation Site

```hd
fn distinct_instances() -> Conn?:
    let keep: Conn? = .None
    for index in 0..2:
        c := connect!("db")
        if index == 0:
            keep = .Some(c)
    keep
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c15-distinct-evaluations-at-one-root-site).
Both handles share the local root of the `connect!` site (known limit
L7). Under the escape rules this changes no verdict: both flow to the
same result. The review's form closes the second handle in a `defer`,
where the shared root would make the kept handle look closed; that half
is parked, and the limit matters for any answer to [14.2](#142-open).

#### C16. A Scoped Race

```hd
use std.task.race

fn read!(c: ^Conn) -> usize:
    c.sent()

fn scoped_race!(a: ^Conn, b: ^Conn) -> usize:
    race!(read(a), read(b))   # fine: the frames are taken for the call only, and the outs carry nothing
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c16-a-child-is-cancelled-before-parent-cleanup).
Works (`ne.all.typing`). The review's form opens both handles locally and
closes them in `defer` suites; its proof that losers finish before parent
cleanup rests on `ne.suspend.cancel-order`, which is parked.

#### C18. Phantom Metadata

```hd
data Tag[T]:
    number: usize

fn tag_number(t: Tag[Conn]) -> usize:   # fine: Tag stores no T, so Tag[Conn] is ordinary
    t.number

fn tag_list(xs: List[Conn]) -> usize:   # error: unmarked-local-param: List stores its elements
    xs.len()
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c18-phantom-metadata-is-not-a-holder).
The category is computed from stored fields (`ne.data.category`), so the
two definitions of round 3 no longer disagree. `Field[S, F]` is ordinary
in the same way; how operations through it carry a reach is open
([14.2](#142-open)).

#### C19. A Local Classifier

```hd
use std.inspect.{Inspectable, downcast_val}

fn is_integer(value: ^dyn Inspectable) -> bool:
    match downcast_val::[i32](value):     # error: downcast-non-static
        .Some(_) => true
        .None => false

fn is_integer_global(value: dyn Inspectable) -> bool:   # fine: a global parameter reaches static
    match downcast_val::[i32](value):
        .Some(_) => true
        .None => false
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c19-a-local-classifier-cannot-downcast-ordinary-values).
Friction, known limit L8, the cost of Q6. Comparing `TypeId`s answers the
boolean question without a downcast, but recovers no typed payload.

#### C20. A Scalar-Only Closure Factory

```hd
let COUNTERS: mut List[fn() -> usize] = []

fn counter(n: usize) -> fn() -> usize:   # elided: ^{}
    fn() -> usize: n                     # captures nothing tracked: the empty reach

fn install(f: fn() -> usize) -> void:
    COUNTERS.push(f)

fn install_counter() -> void:
    install(counter(3))                  # fine: a ^{} function result mints no root
```

[Review case](NONESCAPE_LIFETIME_STRESS_TEST.md#c20-a-scalar-only-closure-factory-gets-a-local-root).
A closure's reach is exactly what it captures (`ne.closure.reach`), and
a `^{}` result that is not NonEscapable mints no root at the caller
(`ne.call.fresh`). Round 3 minted one, so `install` rejected it.

### Diagnostics

| Code | When |
| --- | --- |
| `lifetime-mismatch` | A value's reach does not fit the bound it flows into: a result, a store, a global parameter, a top-level binding, a widening of an invariant reach, or a `^` parameter kept where it is not named. |
| `unmarked-local-param` | A parameter whose type is NonEscapable has no `^`. The fix-it adds it. |
| `missing-nonescapable-impl` | A concrete data or enum type has a NonEscapable field and no `impl NonEscapable`. The fix-it adds it. |
| `lifetime-in-type-argument` | A `^` appears inside a type argument, outside a function type's own signature. |
| `misplaced-lifetime` | A bare `^` on a result, or `^{}` or `^{static}` on a parameter. |
| `ambiguous-lifetime` | Elision has no single answer, in a function or a function type, so `^{...}` must be written. |
| `undeclared-slot` | A field or recursive occurrence names no declared slot. |
| `downcast-non-static` | A downcast of a `dyn` value whose reach is not `static`. |
| `overtied-result` (warning) | An elided result is tied to an input the body never returns. |
| `could-be-local` (warning) | An unmarked parameter that the body never keeps. The lint proposes `^`. |

The cleanup and closing diagnostics are parked ([P.7](#p7-parked-diagnostics)).

## 14. Owner Questions

They are ordered core first, spellings last. The decided ones are listed
first, with the decision. Questions already settled in earlier rounds are
not repeated. The decisions about closing moved to
[P.5](#p5-parked-owner-decisions).

### 14.1 Decided

| Was | Decision | Where |
| --- | --- | --- |
| Round-2 Q5, the self rule | Keep result elision as the doc has it, with the `overtied-result` warning (owner, 2026-10-14, D7). | `ne.elide.*` |
| Round-2 Q6, generic defaults | **Reversed.** Type parameters accept no lifetime-carrying argument by default. `x: ^T` opts in (owner, 2026-10-14, D2). TQ-24 to TQ-26 are replaced by sections 4, 5, and 8.3. | `ne.param.generic`, `ne.generic.default` |
| Round-2 Q8, function-type results | Name the parameters and use the function elision rules. No union default (owner, 2026-10-14, D8). | `ne.fntype.*` |
| New (D1) | Every NonEscapable parameter is written `^`. Unmarked is global, and an unmarked NonEscapable parameter is an error with a fix-it. | `ne.param.mark`, `ne.param.nonescapable` |
| New (D3) | Dependent results and stores stay. "Local" means "lives no longer than its sources". | section 2 |
| New (D4) | Any parameter of any type may be marked `^`, checked in the callee, and read by the optimizer as `LocalParams`. Non-local `return` is parked. | `ne.param.any-type`, [12.6](#126-what-the-optimizer-gets) |
| New (D5) | Function-typed parameters follow the same rule. No implicit default. `^{static}` is removed from parameters. A lint proposes `^`. | `ne.param.fn`, `ne.param.lint` |
| New (D6) | No lifetime inside a type argument. One reach per value. Declared slots stay, opt-in. Containers write `^{self}` on what they store. | `ne.syntax.one-reach`, `ne.data.*`, `ne.generic.*` |
| Q2, where the category is declared | A concrete data or enum type with a NonEscapable field must declare `impl NonEscapable`, checked against its fields. Generic data follows its stored fields (NE-S10). A function-typed field does not count. The orchestrator's option (a), only root types declare it, is not taken (owner, 2026-10-14). | `ne.data.declare`, `ne.data.generic` |
| Q5, slot variance | Computed from the declaration surface (owner, 2026-10-14), with NE-S1. | `ne.var.polarity`, `ne.var.surface` |
| Q6, downcasts | Forbid all non-static downcasts, as Rust's `Any: 'static`. The narrower invariant-only rule is dropped (owner, 2026-10-14). | `ne.erase.downcast`, [X6](#x6-widening-through-a-downcast), L8 |
| Q7, providers and top level | NonEscapable requirement keys are tracked inputs, and top-level binding types elide to `static` (owner, 2026-10-14). | `ne.provider.root`, `ne.sig.input`, `ne.top.elide` |
| Q9, strong updates | Not now (owner, 2026-10-14). | L1 |
| Q10, the receiver | The shorthand `^self` and `^mut self` (owner, 2026-10-14). | `ne.syntax.receiver` |
| Q12, function types | The environment is named by the parameter, callback inputs are written `^T`, and other positions are a known limit. `fn.type.no-names` must change in the spec (owner, 2026-10-14). | `ne.fntype.*`, L6 |
| Q13, std migration | Phased: first the 8 receivers, the 7 stores, and the callbacks the stress tests use, then the rest. Optimizer marks come where a profile shows a gain (owner, 2026-10-14). | [12.5](#125-annotation-burden-in-std-estimate) |
| Q14, the global default | A missing `^` in a library is an error with a fix-it, not a false alarm. Requirement 4 governs inference and defaults (owner, 2026-10-14). | requirement 4, `ne.param.fn` |
| Q15, two small choices | A wrapper of declared-slot data has one reach, and an ordinary result may name a `^` parameter (owner, 2026-10-14). | `ne.data.wrapper`, `ne.param.ordinary-result` |
| Q16, spellings | `^` before `mut` and `dyn`. A bare `^T` is not kept. Named sources go in braces: `^{a, b}`, `^{}`, `^{static}`, `^{$Pool}`, `^{s.c}`. Data slots are `data S[^c, ^u]` with fields `^{c} Conn`. Keep `NonEscapable`. The `closing` spelling and the name "cleanup block" are parked with closing (owner, 2026-10-14). | section 3 |
| Park closing (round 4) | "Closing method cannot be modelled well without ownership; don't patch the design." Everything about closing moves to a parked section, unpatched. The design focuses on escaping (owner, 2026-10-14). | [Parked](#parked-closing-and-cleanup) |
| NE-S1, recovered write authority (round 4) | A reach is covariant only when no mutable storage is reachable through the exposed surface; otherwise invariant. Replaces the covariance claims of `ne.var.generic`, `ne.var.fn`, and `ne.var.dyn` (owner, 2026-10-14). | `ne.var.*`, C01 to C03 |
| NE-S2, ordinary provenance (round 4) | One flow: ordinary `^` parameters are roots; `^{b}` results carry `b`'s root to the caller. "The caller tracks nothing" and the reach-dropping of `ne.fntype.carry-only` are deleted; only already-global arguments substitute `static` (owner, 2026-10-14). | `ne.param.any-type`, `ne.param.ordinary-result`, `ne.call.local`, `ne.fntype.one-flow`, C04, C14 |
| NE-S5, frames (round 4) | Two slots on `Suspend`, `run` and `out`. A cold call fills `run` from argument reaches and `out` by substitution; driving yields `out`; storage takes `run`; `all!` and `race!` join outs. Elision option (a): std writes `^{s.out}` (owner, 2026-10-14). | `ne.suspend.*`, `ne.all.typing`, C08 |
| NE-S6, provider elision (round 4) | Function-type elision counts NonEscapable keys and row parameters as function elision does; an ambiguous callback result is written (owner, 2026-10-14). | `ne.fntype.elide`, C09 |
| NE-S7, optimizer proofs (round 4) | Doc only. `LocalParams` bounds what the callee keeps; each optimization lists its own preconditions: caller liveness for loop reuse, inlining or specialization for scalar replacement, a single-consumer alias proof for fusion (owner, 2026-10-14). | [12.6](#126-what-the-optimizer-gets), C13, C14 |
| NE-S10, category calculation (round 4) | "NonEscapable" is the instantiated category from stored fields, a fixed point for recursion, never argument spelling. `Tag[Conn]` and `Field[S, F]` are ordinary. Generic data declares no category for now (owner, 2026-10-14). | `ne.data.category`, `ne.param.nonescapable`, `ne.data.generic`, C18 |
| NE-S11, precision boundaries (round 4) | C11, C12, C15, and C19 are known limits; "exact" means exact substitution of published signatures. A closure's reach is exactly what it captures, and capturing nothing tracked gives the empty reach and no root (owner, 2026-10-14). | requirement 10, L1, L5, L7, L8, `ne.closure.reach`, C20 |

### 14.2 Open

1. **Without closing, what fixes a fresh local handle's lifetime?**
   Recorded for the owner, not decided (owner, 2026-10-14). With closing
   parked, a local root is constrained only by where it flows
   (`ne.root.free`). The active rules are not consistent about it: a
   fresh handle cannot be passed to a global parameter
   (`ne.call.global`), yet it fits any result bound, `^{static}` included
   (`ne.body.result`), so a helper can return it as static, and a closure
   that captures it can be returned under `^{}` with an empty reach.
   Candidate answers, from this doc's own material:
   - **(a) Nothing: liveness only.** A fresh root reaches `static` from
     its creation, as a REPL handle does (`ne.top.static`). Escape checks
     then bite only on `^` parameters and providers. It is the simplest,
     and it drops round 3's D8 verdict that a callback over a fresh
     handle cannot be registered.
   - **(b) The frames it passes through.** A fresh root is local in the
     body that creates it, and in every caller that receives it through a
     `^{}` result, and never fits `static`. This is Kotlin's `local`. The
     change is one sentence: a local root fits any result bound except
     `^{static}` and a `^{}` result that mints no caller root
     (`ne.call.fresh`). It needs no liveness. Known limit L7 stays inert.
   - **(c) A scope that owns it.** The block that declares its first
     binding, or a scope-providing call such as `$.with(K=p)`
     (`ne.provider.with`) or a combinator, fixes the lifetime, and a
     result or a store is the only way out. This brings back the parked
     outlives check ([P.4](#p4-outlives-and-liveness)) without closing,
     and makes L7 visible. The parked cleanup block is the closing form
     of this answer, which the owner parked for want of ownership.
2. **Annotation handles over NonEscapable types (owner, later).** An
   annotation handle such as `Field[Session, Conn]` is metadata with no
   reach (`ne.handle.no-slot`), and it is ordinary by the category
   calculation (NE-S10). Operations through it must still carry the
   target's reach: reading a `Session`'s field through the handle gives a
   value that reaches the session, as in
   `fn get(h: Field[Session, Conn], s: ^Session) -> ^{s} Conn`, or the
   slot path of `ne.handle.get`. The owner said this may need a change,
   for a later pass. `build` and generated templates need the same
   substitution (`ne.handle.build`).
3. **A `Suspend` field and the category (Q2), agent-detail.** NE-S10
   settled the mixed generic case: generic data declares nothing and
   follows its stored fields. One case is left: a `Suspend[T]` field,
   which holds a frame as a function value holds a closure (`Worker` in
   D2). **Recommendation.** It does not count, like a function type. The
   unmarked `Worker` parameter is then global, and a worker that holds a
   frame over a local handle is still rejected at the call by the reach
   rules, through the frame's `run` slot, so nothing is unsound. The doc
   has it in `ne.data.declare` and `ne.data.category`.

## Parked: Closing And Cleanup

**Parked by the owner, 2026-10-14:** "closing method cannot be modelled
well without ownership; don't patch the design." Everything in this
section is kept as round 3 wrote it, and is not part of the active
design. No rule here is checked, and active rules do not depend on it.
The review's findings against it (NE-S3, NE-S4, NE-S8, NE-S9; cases C05,
C06, C07, C10, and C17) are recorded here and not patched. Rule IDs are
kept so the history reads.

### P.1 The Parked Guarantee

The round-3 promise (owner, 2026-10-14, Q1): **What the checker
promises.** No alias of a value that a `defer` closes outlives the block
that `defer` belongs to. It does not promise that cleanup runs, or runs
once. An early `close()` outside a `defer` is a run-time matter only: it
gives no cleanup block, and a later use gives `ResourceError.Disposed`
(`module.resource.disposed`).

The round-3 core model: a lifetime protects one fact, that aliases die
before the cleanup that closes them runs. A **cleanup block** is the
block whose `defer` closes a handle. The handle's lifetime ends when that
block exits (owner, 2026-10-14: this name replaces "pin").

```hd
fn serve!(l: ^Listener) -> void:
    c := l.accept!()          # a new root, owned by serve!
    defer: c.close()          # the body block is the cleanup block of that root
    c.send!([1])              # fine: used inside the block
```

Round 3's `ne.root.free` read: "A local root that has no cleanup block is
**free**. Its uses alone constrain it, so a helper may open a handle and
return it." Its `ne.root.site` added: "The root of a holder is free
unless a closing call gives it a cleanup block, so it adds no constraint
of its own."

The review found that this promise has non-equivalent readings: no alias
past the owning block, no use after a deferred close within cleanup, and
no disposed use across delegated cleanup (NE-S9, C07, C17). The owner
settled the wording with NE-S3 and parked it ([P.5](#p5-parked-owner-decisions)).

### P.2 The `closing` Mode

A handle is closed through a **closing call**: a call that passes it to a
`closing` parameter or `closing self`.

```hd
trait Socket < NonEscapable:
    fn close(closing self) -> void

impl Session:
    fn shutdown(closing self) -> void:
        self.client.close()
        self.upstream.close()
```

1. r[ne.close.mode] `closing` is a parameter mode, like Swift's `consuming`. It is written before the parameter name, or as `closing self`. It implies `^` (`ne.param.closing`). (owner, 2026-10-14, Q11)
2. r[ne.close.type] A `closing` parameter must have a lifetime-carrying type. Error: `type-mismatch`.
3. r[ne.close.owned] A closing call is valid only when every root in the argument's reach is a local root, the root of a `closing` parameter, or `static`. Error: `closing-not-owned`. (owner, 2026-10-14, Q3)
4. r[ne.close.fields] Inside a body, closing `self.client` through `closing self` closes a root `self` reaches, so it is valid.
5. r[ne.close.in-defer] A closing call written directly in a `defer` suite gives the block a cleanup block, by `ne.cleanup.closing-defer`.
6. r[ne.close.early] A closing call outside a `defer` is an **early close**. It gives no cleanup block, the checker does not follow it, and a later use gives `Disposed` at run time. (owner, 2026-10-14, Q1)
7. r[ne.close.defer-sync] A closing call in a `defer` must be a call of a function that does not suspend. A plain call of a `fn!` there is an error, because its cold frame closes nothing. Error: `closing-suspension-in-defer`. (agent)
8. r[ne.close.closure] A closure body must not make a closing call on a captured value. Error: `closing-in-closure`. (owner, 2026-10-14, Q3)
9. r[ne.close.closure-own] A closure may close a handle it created itself.
10. r[ne.close.static] A closing call on a value whose reach is `static` is an early close, and is allowed anywhere. A `defer` that closes it is an error (`ne.cleanup.static`). (owner, 2026-10-14, Q8)
11. r[ne.close.trait] A trait method may be declared `closing self`. Through `dyn K` it is a closing call like any other.

Two more parked rules sat elsewhere in round 3:

- r[ne.param.closing] A `closing` parameter is local without writing `^`. Writing both is allowed. (owner, 2026-10-14, Q11)
- r[ne.provider.close] A provider reached only through `$.use` is not owned, so closing it is an error. Error: `closing-not-owned`.

Round 3's notation row: `closing c: T`, `closing self`: the callee may
close this parameter's handle. It implies `^`. Round 3's exports listed,
for each parameter, whether it is `closing`.

```hd
fn handle!(closing c: Conn) -> void:
    defer: c.close()          # valid: the caller handed c over to be closed
    c.send!([1])

fn relay!(client: ^Conn) -> void:
    s := Session { client: client, upstream: connect!("db") }
    defer: s.shutdown()       # error: closing-not-owned: s reaches the caller's client
```

> **Why (round 3).** Without `closing`, a wrapper that closes its fields
> would give its handles no cleanup block ([S3](#s3-a-cleanup-wrapper)).
> Ownership per root closes the same gap through locals
> ([X5](#x5-closing-a-borrowed-handle-through-a-local)). The closure rule
> is Swift's ban on consuming a captured noncopyable value
> ([X4](#x4-a-cleanup-closure)). An early close stays a run-time matter,
> because making its block a cleanup block would reject the common "close
> on the error path, return on the success path" helper.

### P.3 Cleanup Blocks

1. r[ne.cleanup.closing-defer] A closing call written in a `defer` suite makes the block the suite is registered on the **cleanup block** of every root in its closing argument's reach. (owner, 2026-10-14, naming)
2. r[ne.cleanup.at-exit] The argument's reach is taken at each exit of that block, because a suite reads captured storage at cleanup time (`flow.defer.captures`). (owner, 2026-10-14, Q4)
3. r[ne.cleanup.target] Only a local root or the root of a `closing` parameter may have a cleanup block. A `defer` that closes any other root is an error. Error: `closing-not-owned`.
4. r[ne.cleanup.static] A `defer` that closes a `static` handle is an error, because the handle's lifetime does not end with the block. An early close of a static handle is fine (`ne.close.static`). Error: `cleanup-static-root`. (owner, 2026-10-14, Q8)
5. r[ne.cleanup.escaped] A root that escapes must not have a cleanup block anywhere in the body, whatever the textual order. Error: `cleanup-escaped-root`. (owner, 2026-10-14, Q4)
6. r[ne.cleanup.many] A root may have a cleanup block in several places. Each is checked on its own.
7. r[ne.cleanup.defer-order] A `defer` suite registered on `B` before the suite that closes the handle runs after it. A read in that suite of a value reaching the handle's root is an error. Error: `handle-outlives-cleanup`. (owner, 2026-10-14, Q4)

Round 3's `ne.escape.free` read: "An escaped root is fine while it is
free. By `ne.cleanup.escaped`, it can never have a cleanup block."
Round 3's `ne.top.no-cleanup` read: "Top level is not a cleanup scope
(`flow.defer.not-scopes`), so no root there has a cleanup block."

### P.4 Outlives And Liveness

1. r[ne.outlive.def] A value **outlives** a block when it is live on an exit edge of that block. Being declared outside the block is not enough.
2. r[ne.outlive.check] A value whose reach contains a root with cleanup block `B` must not outlive `B`. Error: `handle-outlives-cleanup`.
3. r[ne.outlive.loop] The next iteration of a loop is after the body block, so a value live on the back edge outlives the body.
4. r[ne.outlive.return] A `return` operand, a value propagated by `?`, and a block's final value are live after the block's own `defer` suites run (`flow.defer.operands`).

```hd
fn dangle!() -> Conn:
    c := connect!("db")
    defer: c.close()
    c                         # error: handle-outlives-cleanup
```

> **Why (round 3).** "Outlives" as liveness is Rust's non-lexical
> lifetimes. A declared-outside rule rejects
> [R6](#r6-evil-feeder) without the final use, which is safe.

Round 3's section 7.1, Uses:

1. r[ne.body.use] A **use** of a value is any read of it: a call argument, a receiver, a field read, a return, a store, or a capture.
2. r[ne.body.kill] Assigning a binding with `=` is not a use of its old value.
3. r[ne.body.poll-cancel] Polling or cancelling a suspension is a use of it. Dropping the last reference is not (`req.discard.abandoned`).
4. r[ne.body.live] A value is **live** on an edge when some path from the edge may use it, directly or through a live binding whose reach contains it.

Round 3's suspension rules about cleanup order:

- r[ne.suspend.cancel-order] Cancelling a parent runs each unfinished child's cleanup before the parent's `defer` suites (`req.cancel.defer`). So a child driven by a bang call never sees a handle that its parent's cleanup block has closed.
- r[ne.all.race] `race!` cancels the losers before it returns (`req.combinator.race-losers`), so no loser is live after the call.

Round 3's checker steps (12.2, items 4 and 5):

4. **Cleanup blocks.** For each `defer` closing call, take the argument's
   reach at each exit of its block.
5. **Liveness.** One backward bitvector pass over the body's control
   flow, as non-lexical lifetimes use. For each root with a cleanup block,
   test the live-out set of the block's exits, and the suites registered
   before the closing suite.

The review notes that liveness on cyclic control flow needs a fixed
point, not one pass, and that cleanup suites add ordered edges (review,
"Checker Cost Needs A Real Bound").

Round 3's table rows (7.4): each root with a cleanup block is owned
(`ne.cleanup.target`, `ne.close.owned`); a `defer` never closes a static
handle (`ne.cleanup.static`); no root with a cleanup block escapes
(`ne.cleanup.escaped`); no value reaching such a root is live on the
cleanup block's exits (`ne.outlive.check`); no earlier-registered suite of
the block reads it (`ne.cleanup.defer-order`).

### P.5 Parked Owner Decisions

The round-3 decisions about closing, as decided (owner, 2026-10-14):

| Was | Decision | Where |
| --- | --- | --- |
| Naming | The invented word "pin" is gone. The block whose `defer` closes a handle is its **cleanup block**, and the handle's lifetime ends when that block exits. Rule IDs `ne.pin.*` became `ne.cleanup.*`. | P.1, P.3 |
| Q1, early close | Run-time only. A closing call outside a `defer` gives no cleanup block, and a later use gives `Disposed`. | `ne.close.early` |
| Q3, ownership and closing | Adopt `ne.close.owned` and `ne.close.closure`. | [X4](#x4-a-cleanup-closure), [X5](#x5-closing-a-borrowed-handle-through-a-local) |
| Q4, cleanup blocks | Adopt `ne.cleanup.at-exit`, `ne.cleanup.defer-order`, and `ne.cleanup.escaped`, which were `ne.pin.*`. | [X1](#x1-a-late-cleanup-block), [X2](#x2-defer-order), [X3](#x3-a-defer-reads-the-binding-at-exit) |
| Q8, static values | A static handle may be closed anywhere as an early close. A `defer` that closes one is an error. | `ne.close.static`, `ne.cleanup.static` |
| Q11, `closing` | `closing` implies `^`. | `ne.param.closing` |

The review's findings about closing, as the owner left them on
2026-10-14:

- **[NE-S3](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s3-cleanup-execution-states),
  cleanup execution states.** Decided, option 1, then parked: model
  cleanup as real control flow. Suites run in LIFO order with evolving
  variable values; a closing call ends the handle it actually closes at
  that point; any later use, in the same suite or a later one, is an
  error. This catches C06 and C07. It replaces `ne.cleanup.at-exit` and
  extends `ne.cleanup.defer-order`, but the replacement is not written
  into P.3, by the owner's instruction not to patch.
- **[NE-S9](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s9-guarantee-wording),
  guarantee wording.** Settled with NE-S3, then parked: no alias outlives
  its cleanup block, and nothing uses a handle after the deferred close
  that ends it runs.
- **[NE-S4](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s4-closing-function-values),
  closing function values.** Not decided. Parked. Bound and unbound
  method references to a closing method (C05).
- **[NE-S8](NONESCAPE_LIFETIME_STRESS_TEST.md#ne-s8-closing-a-dependency-is-not-closing-a-resource),
  closing a dependency.** Not decided. Parked. The whole-reach rule of
  `ne.close.owned` rejects a lease that keeps a borrowed pool (C10).

### P.6 Parked Stress Cases

The round-3 verdicts below are kept as written.

| # | Source | Verdict | Change forced | Re-check, round 3 |
| --- | --- | --- | --- | --- |
| S3 | Swift `consuming` close wrapper | Hole in round 1 | `closing` mode | same |
| N5 | `closing` implies `^` | Works | Q11 | new |
| X1 | A late cleanup block | Hole | `ne.cleanup.escaped` | same hole; now arises through a store |
| X2 | `defer` order | Hole | `ne.cleanup.defer-order` | same |
| X3 | A `defer` reads the binding at exit | Hole and false alarm | `ne.cleanup.at-exit` | same |
| X4 | A cleanup closure | Hole | `ne.close.closure` | same |
| X5 | Closing a borrowed handle through a local | Hole | `ne.close.owned` | same |
| X7 | A suspending close in a `defer` | Hole | `ne.close.defer-sync` | same |

The review's closing cases, with its verdicts:

| Case | Review verdict | Problem | Status |
| --- | --- | --- | --- |
| C05 | Breaks | A bound closing method is a cleanup callback | Parked, NE-S4 |
| C06 | Breaks | One cleanup suite changes another suite's target | Parked, NE-S3 option 1 |
| C07 | Breaks | A read follows a close within the same suite | Parked, NE-S3 option 1, NE-S9 |
| C10 | Friction | A lease needs its pool but does not close it | Parked, NE-S8 |
| C17 | Works, with a guarantee boundary | Early and delegated close leave disposed aliases | Parked, NE-S9 |

#### S3. A Cleanup Wrapper

Swift: `consuming func close()`; Rust: `drop(self)`.

```hd
impl Session:
    fn shutdown(closing self) -> void:
        self.client.close()
        self.upstream.close()

fn proxy!(l: ^Listener) -> Conn:
    s := Session { client: l.accept!(), upstream: connect!("db") }
    defer: s.shutdown()       # a closing call: the body is the cleanup block of every root of s
    s.client                  # error: handle-outlives-cleanup
```

Without the `closing` mode, a wrapper that closes its fields gives its
handles no cleanup block, and `s.client` escapes while closed. Round 2
wrote `defer: s.shutdown!()`; a `defer` cannot bang-call
(`flow.defer.restricted`), so the wrapper does not suspend. The
construction of `s` is a root site of its own (`ne.root.site`), so `s` has
a root beside the roots of its fields. That root is local, so closing `s`
is owned.

#### N5. `closing` Implies A Local Parameter

```hd
fn shut(closing c: Conn) -> void:     # no ^ written: closing implies it
    c.close()

fn shut_twice(closing c: ^Conn) -> void:   # allowed, redundant
    c.close()
```

Owner choice (`ne.param.closing`, Q11). A closing parameter is closed by
the callee and so never kept.

#### X1. A Late Cleanup Block

```hd
fn setup!(l: ^Listener, reg: ^mut Registry) -> void:
    c := l.accept!()
    reg.add("a", c)                   # c flows into reg's object
    defer: c.close()                  # error: cleanup-escaped-root
```

Round 2 checked cleanup blocks against later uses only, and showed the
escape as `on_message(fn(m): _ := c.sent())` into `HANDLERS`. Under D2
that call is rejected at the call ([D8](#d8-a-callback-registry)), so a
global escape can no longer reach a cleanup block. The same hole remains
through a store into a parameter's object, as above. The caller's `reg`
outlives `c`'s close. Fix: a root that escapes can never have a cleanup
block, in any order (`ne.cleanup.escaped`). Borrows: Rust checks borrows
against all uses of a region, not only the later ones.

#### X2. Defer Order

```hd
fn audit!(l: ^Listener, log: mut List[string]) -> void:
    c := l.accept!()
    defer: log.push("sent ${c.sent()}")   # error: handle-outlives-cleanup: runs after c.close()
    defer: c.close()
    c.send!([1])
```

Suites run last in, first out (`flow.defer.run`), so the first suite
reads a closed handle. Round 2 counted only uses after the block. Fix:
an earlier-registered suite of the same block counts as after the close
(`ne.cleanup.defer-order`). Borrows: Rust's borrow checker knows drop
order and rejects a value borrowed by something dropped later.

#### X3. A `defer` Reads The Binding At Exit

```hd
fn rotate!(hosts: List[string]) -> Conn:
    let c = connect!(hosts[0])
    first := c
    defer: c.close()          # closes what c holds when the block exits
    c = connect!(hosts[1])
    first                     # fine: first's handle is never closed here
```

Returning `c` instead is an error. A suite reads captured storage at
cleanup time (`flow.defer.captures`). A cleanup block fixed at
registration would give `first`'s root one, a false alarm, and miss the
second handle, a hole. Fix: take the closing argument's reach at each exit
(`ne.cleanup.at-exit`). Borrows: nothing directly. Go evaluates a deferred
call's arguments at registration, and hd does not. The review's C06
shows that the exit snapshot is not enough; NE-S3 option 1 decides
ordered cleanup flow instead, parked.

#### X4. A Cleanup Closure

```hd
fn leak!() -> Conn:
    c := connect!("db")
    cleanup := fn() -> void: c.close()    # error: closing-in-closure
    defer: cleanup()
    c
```

The closure is a wrapper, like S3, and `defer: cleanup()` would give `c`
no cleanup block. Fix: a closure never closes a capture
(`ne.close.closure`). Write a function with a `closing` parameter instead;
then `defer: shut(c)` gives the block as cleanup block, and `c` is
rejected at the return. Borrows: Swift rejects consuming a captured
noncopyable value in a closure. The review's C05 rebuilds this with a
bound method reference (NE-S4, parked).

#### X5. Closing A Borrowed Handle Through A Local

```hd
fn relay!(client: ^Conn) -> void:
    s := Session { client: client, upstream: connect!("db") }
    defer: s.shutdown()       # error: closing-not-owned
```

Round 2 allowed closing "a local the function owns". `s` is a local, but
it holds the caller's handle, which the caller may still use. Fix:
ownership is per root (`ne.close.owned`). Write `closing client: Conn`,
or close only `s.upstream`. Borrows: Rust's rule that you cannot move
out of a borrow. The review's C10 shows the cost for a lease that keeps a
borrowed pool (NE-S8, parked).

#### X7. A Suspending Close In A `defer`

```hd
impl Session:
    fn drain!(closing self) -> void:
        self.client.send!([0])
        self.shutdown()

fn run!(l: ^Listener) -> void:
    s := Session { client: l.accept!(), upstream: connect!("db") }
    defer: _ := s.drain()     # error: closing-suspension-in-defer
```

The plain call builds a cold frame and discards it, so nothing is
closed, yet the call would give the block as cleanup block. Fix: a closing
call in a `defer` must not suspend (`ne.close.defer-sync`). Borrows:
nothing; it follows from `flow.defer.restricted`.

#### Cleanup Forms Of Active Examples

These are the round-3 forms of examples whose active forms in section 13
now show escapes. Their round-3 verdicts are kept.

R1, the cleanup form:

```hd
fn pick!(l: ^Listener, keep: ^Conn) -> ^{keep} Conn:   # two inputs: written
    c := l.accept!()
    defer: c.close()
    longer(keep, c)           # error: handle-outlives-cleanup: the result reaches c
```

R2, the cleanup form:

```hd
fn dangle!() -> Conn:
    c := connect!("db")
    defer: c.close()
    c                         # error: handle-outlives-cleanup
```

Without the `defer`, `c` is free and may be returned.

R6, the cleanup form:

```hd
fn main!(hosts: List[string]) -> void:
    keep := Box { value: connect!("a") }
    if hosts.len() > 0:
        tmp := connect!(hosts[0])
        defer: tmp.close()
        feed(keep, tmp)               # keep's reach now includes tmp (invariant under mut)
    keep.value.send!([1])             # error: handle-outlives-cleanup
```

Without the last line the program is accepted: `keep` is not live after
the `if` block. Round 2 used a loop and claimed Rust accepts it without
the final use. It does not: the next iteration's `feed(keep, tmp)`
passes `keep`, which may read the previous `tmp`, and Rust reports
"borrow later used here, in later iteration of loop". hd rejects the loop
form for the same reason (`ne.outlive.loop`).

R9, the cleanup form:

```hd
fn handoff!(host: string) -> Conn:
    l := listen!(host)
    defer: l.close()
    l.accept!()               # fine with ^{}; with the elided ^{self}, an error
```

R10, the cleanup form: `defer: c.close()` after `c := l.accept!()`, and
`echo!(c)` was fine because the bang call returns before the `defer`
runs.

R11, the cleanup form:

```hd
fn fan_out!(hosts: List[string]) -> void:
    let jobs: mut List[fn!() -> void] = []
    for h in hosts:
        c := connect!(h)
        defer: c.close()
        jobs.push(fn!(): c.send!([0]))    # error: handle-outlives-cleanup
    _ := all_list!(jobs)
```

Children may not capture a handle closed in an inner block before the
scope runs, which is the same rule `thread::scope` enforces.

R13, the cleanup form:

```hd
fn mixed!(long: ^Conn, extra: bool) -> void:
    let by_name: mut Map[string, Conn] = {}
    by_name["long"] = long
    if extra:
        tmp := connect!("x")
        defer: tmp.close()
        by_name["tmp"] = tmp
    by_name["long"].send!([1])    # error: handle-outlives-cleanup: by_name holds tmp
```

S1, the cleanup form: `defer: c.close()` before `lines(c).next!()`,
fine because a string carries no lifetime.

T1, the cleanup form:

```hd
fn proxy!(l: ^Listener) -> ^{} Conn:
    keep := l.accept!()
    up := connect!("db")
    defer: up.close()
    client_of(Session { client: keep, upstream: up })   # fine: only keep's root
```

T3, the cleanup form: a `defer` that closes a REPL handle is an error
(`ne.cleanup.static`); an early `c.close()` is fine, anywhere
(`ne.close.static`, owner, 2026-10-14, Q8).

D1, the cleanup form:

```hd
fn talk!() -> Result[void, LinkError]:
    c := connect!("db")
    defer: c.close()
    check!(c)?                # error: handle-outlives-cleanup: ? returns the error
    .Ok(())
```

Rejected correctly: the caller would receive a closed handle inside the
error. `?` is a return, live after the block's `defer` suites
(`ne.outlive.return`).

D2, the cleanup forms:

```hd
fn arm!(w: ^mut Worker) -> void:
    c := connect!("db")
    defer: c.close()
    w.next = .Some(echo(c))   # error: lifetime-mismatch: w's reach is the caller's

fn staged!(ready: bool) -> void:
    w := Worker { next: .None }
    if ready:
        c := connect!("db")
        defer: c.close()
        w.next = .Some(echo(c))
        w.next = .None        # discarded cold: nothing ran
    w.next = .None            # error: handle-outlives-cleanup: w still reaches c
```

D3, the cleanup form:

```hd
fn ask!(l: ^Listener, backup: ^Conn) -> string:
    c := l.accept!()
    defer: c.close()
    fastest!(c, backup)               # fine, even if ask! is cancelled mid-wait
```

If `ask!` is cancelled inside `race!`, both children finish their
cancellation cleanup before `c.close()` runs (`req.cancel.defer`). No
lifetime check is needed. [X2](#x2-defer-order) shows the one ordering
case that does need a check.

D4, the cleanup form: `defer: p.close()` after `p := open_pool!()`, and
`$.use(Pool).close()` inside `grab` would be `closing-not-owned`.

#### The Review's Closing Cases

These are the review's cases, in this doc's notation, recorded and not
patched.

C05, a bound closing method as a cleanup callback
([review](NONESCAPE_LIFETIME_STRESS_TEST.md#c05-a-bound-closing-method-is-a-cleanup-callback)):

```hd
fn cleanup_for(c: ^Conn) -> fn() -> void:
    c::close

fn return_closed() -> Conn:
    c := connect!("db")
    cleanup := cleanup_for(c)
    defer: cleanup()
    c
```

The bound reference closes `c` during cleanup, but no closing call
appears at the registration site, and `ne.close.closure` checks only
written closure bodies. Review verdict: breaks. Parked (NE-S4).

C06, one suite changes another suite's target
([review](NONESCAPE_LIFETIME_STRESS_TEST.md#c06-one-cleanup-suite-changes-another-suites-target)):

```hd
fn changed_target() -> Conn:
    let current = connect!("a")
    second := connect!("b")
    defer: current.close()
    defer: current = second
    second
```

The second suite runs first, so the first closes `second`, which was
returned. The exit snapshot of `ne.cleanup.at-exit` marks the wrong root.
Review verdict: breaks. NE-S3 option 1 decides ordered cleanup flow;
parked.

C07, a read after a close in the same suite
([review](NONESCAPE_LIFETIME_STRESS_TEST.md#c07-a-read-follows-a-close-within-the-same-suite)):

```hd
fn observe_after_cleanup() -> void:
    c := connect!("db")
    defer:
        c.close()
        _ := c.sent()
```

`ne.cleanup.defer-order` covers earlier-registered suites only. Review
verdict: breaks. NE-S3 option 1 and the NE-S9 wording reject it; parked.

C10, a lease that needs its pool but does not close it
([review](NONESCAPE_LIFETIME_STRESS_TEST.md#c10-a-lease-needs-its-pool-but-does-not-close-it)):

```hd
data Lease:
    conn: Conn
    pool: dyn Pool

impl NonEscapable for Lease

impl Lease:
    fn release(closing self) -> void:
        self.conn.close()

fn lease(c: ^Conn, pool: ^dyn Pool) -> ^{c, pool} Lease:
    Lease { conn: c, pool: pool }

fn use_lease(pool: ^dyn Pool) -> void:
    c := connect!("db")
    l := lease(c, pool)
    defer: l.release()        # closing-not-owned: l reaches the borrowed pool
```

Review verdict: friction, by the owner-decided Q3. Parked (NE-S8).

C17, early and delegated close
([review](NONESCAPE_LIFETIME_STRESS_TEST.md#c17-early-and-delegated-close-preserve-disposed-aliases)):

```hd
fn finish(closing c: Conn) -> void:
    defer: c.close()

fn disposed_alias() -> usize:
    c := connect!("db")
    other := c
    finish(c)
    other.sent()              # Disposed at run time; the checker does not follow it
```

Review verdict: works under the early-close decision, with a guarantee
boundary. Parked (NE-S9).

### P.7 Parked Diagnostics

| Code | When |
| --- | --- |
| `handle-outlives-cleanup` | A value reaching a root with a cleanup block is live after that block, or read by an earlier-registered suite. |
| `closing-not-owned` | A closing call reaches a root the function does not own. |
| `closing-in-closure` | A closure closes a captured value. |
| `closing-suspension-in-defer` | A `defer` makes a closing call that would only build a frame. |
| `cleanup-escaped-root` | A root with a cleanup block also escapes. |
| `cleanup-static-root` | A `defer` closes a static value. |

## Appendix A. Why A Category And Lifetimes

Round 1 (the predecessor's study) split the owner's claim into three
parts. Round 2 accepted all three, and requirement 2 makes the third
binding. The examples below argued from cleanup, which round 4 parks;
they are kept as the history of the argument. In the active design, the
category says where roots start and which parameters must be marked, and
the lifetime relation is the escape check.

| Part | Round-1 verdict | Decisive example |
| --- | --- | --- |
| A type-level category | Holds, for annotation burden rather than soundness | `defer: log.push(...)` beside `defer: c.close()` |
| Lifetime relations in the semantics | Holds | `open.push(c)` across a loop body |
| Lifetimes written in signatures | Needed to accept good programs, never to reject bad ones | `longest`, `accept`, the two-handle struct |

Round 3 adds a fourth part (owner, 2026-10-14): which positions are local.
A parameter written `^` is local, and an unmarked parameter is global, so
the checker, a lint, and the optimizer read one mark.

**The category.** In round 3, the checker had to know which values a
`defer` cleans up (parked):

```hd
fn serve_one!(log: mut List[string], l: ^Listener) -> void:
    c := l.accept!()
    defer: log.push("closed")   # log outlives this call: no cleanup block
    defer: c.close()            # c must not outlive this block
    log.push("accepted")
```

Marking roots at each use is sound, but then every composite type carries
a lifetime, and `fn register(u: User)` needs an annotation though it
touches no resource. Marking producers' results misses a user's fake
handle converted to `dyn TcpStream`. The category says where roots
start. A concrete type that holds a handle declares the category in one
line, and the checker compares it with the fields (Q2). Generic types
follow their stored fields (NE-S10).

**Lifetime relations.** A yes/no marker cannot accept one of these and
reject the other (the cleanup form is parked; the escape form is R6):

```hd
fn batch!(hosts: List[string]) -> void:
    let open: mut List[Conn] = []
    for h in hosts:
        c := connect!(h)
        defer: c.close()
        open.push(c)            # error: handle-outlives-cleanup: open is used after the body
    for c in open:
        c.send!([0])
```

Moving `let open` into the loop body makes the push safe. Telling the two
apart is a comparison of blocks, which is a lifetime relation.

**`push` writes one mark.** The owner's argument offered two
choices: reject every push of a handle, or accept the leak. A third
exists. The container has one reach, and `push` says that the argument
joins it: `fn push(^mut self, v: ^{self} T)`. This is Kotlin's
`add(element: E_{this})`, and Swift's setter default
`self: copy self, copy newValue`. Round 2 left it unwritten and let the
element type carry the lifetime, as Rust's `Vec<&'a S>::push` does. D6
removed lifetimes from type arguments, so the mark is written once, in std,
and callers write nothing.

**Three families of notation.** Shipping designs differ in where a
written lifetime may appear:

| Family | Who | A lifetime is | Where it appears |
| --- | --- | --- | --- |
| Lifetime variables | Rust `'a`, Mojo origins, Swift's future lifetime types | an abstract parameter | type arguments, fields, results, parameters |
| Named-value dependence | Kotlin `T_{x}`, Swift `@lifetime(copy x)` | a reference to a parameter | positions only; one per-instance lifetime |
| Capture sets | Scala `T^{x}`, `[C^]` | a set of named values or a set variable | types, including class parameters |

This design is a hybrid. Signatures name parameters, as family 2 does,
and reaches are sets written in braces, as in family 3. Data declares
slots, as family 1 does, which owner requirement 5 needs: family 2 cannot
extract the longer-lived of two fields (T1).

**The parked answers.**

| Parked answer | Replaced by |
| --- | --- |
| TQ-24: declared and checked; a generic type follows its arguments; erasure rejected | concrete holders declare the category and generic data follows its stored fields (5.1); erasure into a local position keeps the reach, and a downcast needs a static reach (8.3) |
| TQ-25: a generic parameter opts in | round 2 reversed it; restored by D2 as `x: ^T` (`ne.generic.default`, owner, 2026-10-14) |
| TQ-26: no NonEscapable returns; later, depend on every parameter | written reaches with elision (4) |

## Appendix B. Five-Language Comparison

Condensed from round 1. The last column is this design.

| Question | Rust | Swift 6.2+ (experimental `Lifetimes`) | Kotlin local lifetimes (note) | Scala capture checking | Mojo origins | hd |
| --- | --- | --- | --- | --- | --- | --- |
| Return a dependent value? | `fn f<'a>(x: &'a T) -> &'a U`, elided with one input | `@_lifetime(copy x)` or `borrow x`; a plain return is an error | `-> Iterator<B>_{this&transform}` | the result names captures: `Writer^{output}` | `-> ref [self] T` | `-> ^{x} T`; one input or `self` elided; `^{}` fresh |
| Store in a struct? | `struct S<'a>`; each field keeps its lifetime | only in a `~Escapable` struct, one lifetime | `local class`, one inferred lifetime | `class C[X^]` or captured at construction | `struct S[origin: Origin]` | holders have one reach; `data S[^c, ^u]` for several |
| Either, both, or one input | both: shared `'a`; one: separate | list the sources | `_{x&y}` or `_{x}` | `^{x, y}` or `^{x}` | `ref [a, b]` or `ref [a]` | `^{a, b}` or `^{a}` |
| Closures | borrow by default; `move` owns; `impl Fn() + 'a` | non-escaping closures may capture `~Escapable` | `(A) ->_{x} B` | `() ->{f} Unit` | capture lists; no escaping closures | a closure reaches exactly its captures; a callback parameter is global unless `^fn`, and may then be stored |
| Async | one future carries `'a`; `spawn` needs `'static` | `Task` closures are escaping, so cannot capture | "lexical suspend" sketched | futures capture: `Future[T]^{C}` | no async | bang-driven children need nothing; a frame has `run` and `out` slots |
| Default | borrows opt in; generics accept them | `~Escapable` opts in; generics escapable | `local` opts in | type parameters accept capturing types | `ref` is a separate convention | root types opt in; a parameter is global unless written `^`; generics opt in with `^T` |
| Syntax | `'a`, `<'a>`, `+ 'a` | `~Escapable`, `@_lifetime(...)` | `local x`, `T_{x}` | `T^{x}`, `->{x}` | `ref [o] T`, `origin_of(x)` | `^`, `^{a}`, `^{a, b}`, `^{}`, `^{static}` on results, `[^c]`; `closing` parked |

Maturity: Rust and Mojo 1.0 ship their models. Swift ships `~Escapable`,
but its lifetime annotations are experimental. Scala's capture checking
is experimental. Kotlin's is a research note.

**What Swift can and cannot express today.** It can: `~Escapable`
structs and enums, `Optional` and `Result` of them, dependent results,
`mutating` and `inout` targets with the setter default
`self: copy self`, non-escaping closures that capture, and function
types with lifetimes. It cannot: an `Array`, `Dictionary`, or `Set` of
`~Escapable` elements; escaping closures, `Task`, globals, or `Any`
holding one; or more than one lifetime per value. The open proposal lists
component lifetimes and lifetime types as future directions.

## Sources

- Kotlin: [Local Lifetimes for Kotlin, design notes](https://github.com/Kotlin/KEEP/blob/main/notes/0007-local-lifetimes.md).
- Swift:
  - [SE-0446 Nonescapable Types](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0446-non-escapable.md);
  - [`@_lifetime` reference guide](https://github.com/swiftlang/swift/blob/main/docs/ReferenceGuides/LifetimeAnnotation.md): `borrow`, `&`, and `copy` scopes, and the `inout` and `mutating` defaults;
  - [SE-0467 MutableSpan review](https://forums.swift.org/t/se-0467-mutablespan/78454) and [acceptance](https://forums.swift.org/t/accepted-se-0467-mutablespan/78875);
  - [Compile-time lifetime dependency annotations, PR #3145](https://github.com/swiftlang/swift-evolution/pull/3145) and its [proposal text](https://raw.githubusercontent.com/swiftlang/swift-evolution/refs/pull/3145/head/proposals/NNNN-lifetime-dependency.md);
  - [Pitch #3](https://forums.swift.org/t/pitch-3-compile-time-lifetime-dependency-annotations/84968);
  - [Pitch #2, on closures](https://forums.swift.org/t/pitch-2-lifetime-dependencies-for-non-escapable-values/78821);
  - [Nonescapable elements in `Span` and `Ref`, May 2026](https://forums.swift.org/t/pitch-allow-nonescapable-elements-in-span-and-ref/86835);
  - [owning containers of `~Escapable` elements to come later](https://forums.swift.org/t/using-unsaferawpointer-to-access-in-memory-escapable-types/86441).
- Scala:
  - [capture-checking basics](https://docs.scala-lang.org/scala3/reference/experimental/capture-checking/basics.html);
  - [capability polymorphism](https://docs.scala-lang.org/scala3/reference/experimental/capture-checking/polymorphism.html).
- Mojo:
  - [lifetimes and origins](https://mojolang.org/docs/manual/values/lifetimes/);
  - [closure declarations](https://mojolang.org/docs/reference/closure-declarations/): "A closure can't outlive the scope where it's declared", and "Mojo doesn't support escaping closures or async execution";
  - [closures](https://mojolang.org/docs/manual/values/closures/): the capture modes `{imm x}`, `{mut x}`, `{var x}`, `{var x^}`;
  - [`@__copy_capture`](https://mojolang.org/docs/reference/decorators/copy-capture/), which lets a closure be passed as escaping.
- Rust:
  - [lifetime syntax](https://doc.rust-lang.org/book/ch10-03-lifetime-syntax.html);
  - [Rustonomicon, subtyping and variance](https://doc.rust-lang.org/nomicon/subtyping.html) (`evil_feeder`, and invariance under `Cell`);
  - [`std::thread::scope`](https://doc.rust-lang.org/std/thread/fn.scope.html);
  - [closure expressions](https://doc.rust-lang.org/reference/expressions/closure-expr.html);
  - [`std::future::Future`](https://doc.rust-lang.org/std/future/trait.Future.html) (`Output` beside the future's own lifetime);
  - [`tokio::spawn`](https://docs.rs/tokio/latest/tokio/task/fn.spawn.html).
- hd: `spec/lang/04-type-system.md` (Variance, Views),
  `spec/lang/06-control-flow.md` (Deferred Cleanup),
  `spec/lang/07-functions.md` (Function Types, Captures),
  `spec/lang/09-traits.md` (Runtime Type Identity),
  `spec/lang/11-requirements-and-suspension.md` (Requirement Rows,
  Provider Values, Suspending Functions, Cancellation),
  `spec/lang/14-annotations.md` (Handles), `spec/std/net.md`,
  `spec/std/task.md`, `crates/hd_types/src/pool.rs`,
  `crates/hd_check/src/rows.rs`, and `crates/hd_opt/src/passes.rs`.
- Review: [NONESCAPE_LIFETIME_STRESS_TEST.md](NONESCAPE_LIFETIME_STRESS_TEST.md),
  the adversarial review of round 3 (cases C01 to C20, problems NE-S1 to
  NE-S11).
