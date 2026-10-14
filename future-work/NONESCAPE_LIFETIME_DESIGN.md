# NonEscapable Types And Lifetimes: Design

## Table Of Contents

- [Status](#status)
- [1. Owner Requirements](#1-owner-requirements)
- [2. Core Model](#2-core-model)
- [3. Notation](#3-notation)
- [4. Signatures And Elision](#4-signatures-and-elision)
- [5. Data Types And Slots](#5-data-types-and-slots)
- [6. Closing](#6-closing)
- [7. Body Checking](#7-body-checking)
- [8. Generics, Function Types, And Erasure](#8-generics-function-types-and-erasure)
- [9. Closures, Suspensions, And Cancellation](#9-closures-suspensions-and-cancellation)
- [10. Providers](#10-providers)
- [11. Top Level, REPL, And Annotation Handles](#11-top-level-repl-and-annotation-handles)
- [12. Checker Cost, Files, And Optimizer Facts](#12-checker-cost-files-and-optimizer-facts)
- [13. Stress-Test Suite](#13-stress-test-suite)
- [14. Owner Questions](#14-owner-questions)
- [Appendix A. Why A Category And Lifetimes](#appendix-a-why-a-category-and-lifetimes)
- [Appendix B. Five-Language Comparison](#appendix-b-five-language-comparison)
- [Sources](#sources)

## Status

This is the design record for issue #163. It decides nothing beyond the
owner's own decisions, and the spec is frozen. The owner and the
orchestrator settled the direction on 2026-10-14 (round 2). Round 3, the
same day, folds in eight further owner decisions (D1 to D8, below). This
revision turns that direction into numbered rules, stress-tests it, and
lists the questions that remain.

Every spelling is a placeholder: `NonEscapable`, `^`, `^()`, `^static`,
`closing`, the `ne.` rule IDs, and the diagnostic codes in
[Diagnostics](#diagnostics). No compiler checks these examples. A line
marked `# error:` gives the diagnostic the proposed checker would report.

**Decisions of 2026-10-14, round 3.** All are binding. Each is marked
"(owner, 2026-10-14)" where it lands.

| # | Decision | Lands in |
| --- | --- | --- |
| D1 | Every NonEscapable parameter is written with `^`. An unmarked parameter is always global. An unmarked NonEscapable parameter is an error with a fix-it. Results keep elision. | [4.1](#41-parameter-marks), [4.3](#43-elision) |
| D2 | NonEscapable values flow only into local positions. `x: T` and `x: dyn K` accept one only when written `^T` or `^dyn K`. This reverses the round-2 "type parameters accept lifetimes by default". | [4.1](#41-parameter-marks), [8.1](#81-generic-substitution) |
| D3 | Not strict locality. Dependent results and stores stay. "Local" means "lives no longer than its sources". | [2](#2-core-model), [4.4](#44-call-sites) |
| D4 | Any parameter of any type may be marked `^`. The callee body is checked. The signature gives the optimizer a `LocalParams` fact. | [4.1](#41-parameter-marks), [7.2](#72-signature-conformance), [12.6](#126-what-the-optimizer-gets) |
| D5 | Function-typed parameters follow the same rule, as in Kotlin. No implicit default for callbacks. `^static` on a parameter is gone. A lint suggests `^`. | [4.1](#41-parameter-marks) |
| D6 | Kotlin-style generics. No lifetime inside a type argument. Every value has one reach. Declared slots stay, opt-in. Container methods write `^self` on what they store. | [5](#5-data-types-and-slots), [8.1](#81-generic-substitution) |
| D7 | Result elision stays. | [4.3](#43-elision) |
| D8 | Function types name their parameters and use the elision rules of functions. The union default and limit L3 are gone. | [8.2](#82-function-types) |

**Who wrote what.** The round-2 direction comes from the owner and the
orchestrator: the category, roots, free handles, "outlives" as liveness,
flow-sensitive locals, the notation, the elision table, and the three
`closing` bullets. Round 3 adds D1 to D8. Every rule that ends in
**(agent)** was written by the agent and needs the owner's review. D8's
naming of the environment by the function-typed parameter is an
orchestrator proposal, flagged **(agent-detail)**. The table lists the
larger agent choices.

| Agent choice | Rules | Forced by |
| --- | --- | --- |
| A pin uses the binding's value at block exit | `ne.pin.at-exit` | [X3](#x3-a-defer-reads-the-binding-at-exit) |
| A suite registered before the pin runs after it | `ne.pin.defer-order` | [X2](#x2-defer-order) |
| A root that escapes can never be pinned | `ne.pin.escaped` | [X1](#x1-a-late-pin) |
| Ownership is per root, not per binding | `ne.close.owned` | [X5](#x5-closing-a-borrowed-handle-through-a-local) |
| A closure never closes a capture | `ne.close.closure` | [X4](#x4-a-cleanup-closure) |
| A closing call in a `defer` must not suspend | `ne.close.defer-sync` | [X7](#x7-a-suspending-close-in-a-defer) |
| Captured bindings have one reach | `ne.local.captured` | `fn.capture.storage.shared` |
| Root types versus holders | `ne.data.*` | [R3](#r3-importantexcerpt), [S5](#s5-conditional-escapable) |
| Slot variance computed from the declaration | `ne.var.*` | [T2](#t2-swapping-slots) |
| Downcast rule for an invariant reach | `ne.erase.downcast-invariant` | [X6](#x6-widening-through-a-downcast) |
| `closing` implies `^` | `ne.param.closing` | [N5](#n5-closing-implies-a-local-parameter) |
| An ordinary result may name a `^` parameter | `ne.param.ordinary-result` | [N3](#n3-general-locality) |
| A wrapper of slot-bearing data has one reach | `ne.data.wrapper` | [D7](#d7-recursive-data-of-handles) |
| The lint, and adding `^` is compatible | `ne.param.lint`, `ne.param.interface` | D5 |
| The environment is named by the parameter | `ne.fntype.env` | D8, [N4](#n4-function-types) |
| Key roots `^Pool`, top-level elision, handle paths | `ne.provider.*`, `ne.top.*`, `ne.handle.*` | [D4](#d4-a-nonescapable-provider) |
| The over-tie warning | `ne.elide.lint` | [R9](#r9-accept) |

The owner's standing decisions are unchanged: keep `defer`; no Drop,
`using`, or RAII; garbage collection with no moves; `T` and `mut T`
views; structured concurrency; and a declaration's signature is never
inferred from its body.

**Spec impact.** The spec is frozen, and this record edits none of it.
D8 needs `fn.type.no-names` (function types have no parameter names) to
change, and D1 to D6 need a new parameter mark. Both are for a later,
owner-ordered spec edit.

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

> **Requirement 4 and the global default.** Requirement 4 forbids a
> default that guesses. The unmarked-is-global rule is not a guess: it is
> stated, and a lint proposes the mark. It is still stricter than needed
> where a callback is never stored, so such a callback that captures a
> handle is an error with a fix-it, not a false alarm
> ([Q14](#14-owner-questions)).

**What the checker promises.** No alias of a value that a `defer` closes
outlives the block that `defer` belongs to. It does not promise that
cleanup runs, or runs once. An early `close()` outside a `defer` stays a
run-time matter: a later use gives `ResourceError.Disposed`
(`module.resource.disposed`).

## 2. Core Model

A **lifetime** is the set of roots a value reaches. A value is usable
while every root it reaches is live. hd is garbage-collected, so a
lifetime never protects memory. It protects one fact: aliases die before
the cleanup that closes them runs.

A position is **local** when the value in it lives no longer than its
sources: a local binding, a parameter written `^`, a result or store that
names its sources. A position is **global** when the value may live
forever: an unmarked parameter and a top-level binding. "Local" never
means "cannot leave the call": a dependent result and a store into
another parameter stay (owner, 2026-10-14, D3).

```hd
fn serve!(l: ^Listener) -> void:
    c := l.accept!()          # a new root, owned by serve!
    defer: c.close()          # pins that root to the body block
    c.send!([1])              # fine: used inside the block
```

### Roots

1. r[ne.root.kinds] A **root** is one of: `static`; a **parameter root** for each `^`-marked parameter (one per declared slot when its type declares slots); a **key root** for each NonEscapable requirement key and row parameter of the declaration; a **local root** for each root site in the body. There is no root for a type parameter.
2. r[ne.root.site] A **root site** is a call whose result has the reach `^()`, or a construction of a root type ([5.1](#51-the-category)). Each evaluation creates a new handle, and all evaluations of one site share its local root.
3. r[ne.root.param-live] A parameter root or key root is live for the whole body.
4. r[ne.root.static-outlives] `static` outlives every block. A reach that contains only `static` fits any bound. (agent)
5. r[ne.root.reach] The **reach** of a value is the set of roots it reaches. A value's reach includes the reach of every value stored in it or captured by it. A value has one reach; only data with declared slots has several (`ne.data.slots`). (owner, 2026-10-14, D6)
6. r[ne.root.free] A local root that no `defer` pins is **free**. Its uses alone constrain it, so a helper may open a handle and return it.
7. r[ne.root.global] A value in a global position reaches only `static`. An unmarked parameter's value reaches `static` in the body, and an argument for it must fit `static` ([4.4](#44-call-sites)). A value with no handle has the empty reach, which fits. (owner, 2026-10-14, D1, D2)

### Pins

1. r[ne.pin.closing-defer] A closing call ([6](#6-closing)) written in a `defer` suite **pins** every root in its closing argument's reach to the block the suite is registered on.
2. r[ne.pin.at-exit] The argument's reach is taken at each exit of that block, because a suite reads captured storage at cleanup time (`flow.defer.captures`). (agent)
3. r[ne.pin.param] Only a local root or the root of a `closing` parameter may be pinned. A pin of any other root is an error. Error: `closing-not-owned`.
4. r[ne.pin.static] A pin of `static` is an error. Error: `pin-static-root`. (agent)
5. r[ne.pin.escaped] A root that escapes ([7.3](#73-escapes)) must not be pinned anywhere in the body, whatever the textual order. Error: `pin-escaped-root`. (agent)
6. r[ne.pin.many] A root may be pinned in several blocks. Each pin is checked on its own.

### Outlives

1. r[ne.outlive.def] A value **outlives** a block when it is live on an exit edge of that block. Being declared outside the block is not enough.
2. r[ne.outlive.check] A value whose reach contains a root pinned to block `B` must not outlive `B`. Error: `handle-outlives-cleanup`.
3. r[ne.outlive.loop] The next iteration of a loop is after the body block, so a value live on the back edge outlives the body.
4. r[ne.outlive.return] A `return` operand, a value propagated by `?`, and a block's final value are live after the block's own `defer` suites run (`flow.defer.operands`).
5. r[ne.pin.defer-order] A `defer` suite registered on `B` before the pinning suite runs after it. A read in that suite of a value reaching the pinned root is an error. Error: `handle-outlives-cleanup`. (agent)

```hd
fn dangle!() -> Conn:
    c := connect!("db")
    defer: c.close()
    c                         # error: handle-outlives-cleanup
```

> **Why.** "Outlives" as liveness is Rust's non-lexical lifetimes. A
> declared-outside rule rejects [R6](#r6-evil-feeder) without the final
> use, which is safe.

### Locals And The Heap

1. r[ne.local.flow] A local binding has a reach at each program point. Assignment replaces it, and paths that join take the union.
2. r[ne.local.captured] A binding captured by a closure or a suspension frame has one reach for its whole scope: the union of every value assigned to it. (agent)
3. r[ne.heap.object] Each object has one reach for its whole life, or one per declared slot. Every alias shares it.
4. r[ne.heap.site] An object created in the body gets a reach variable at its creation site. The object of a `^`-marked parameter has that parameter's root, or its declared slots' roots. The object of an unmarked parameter has `static`.
5. r[ne.heap.store] Storing a value into a field, element, or capture adds the value's reach to the holder's reach, or to the matching declared slot.
6. r[ne.heap.fixed] Storing into an object whose reach is fixed, such as a parameter's object, requires the value's reach to fit it. A parameter written `^p` fits the object of `p`. Error: `lifetime-mismatch`.

> **Why.** hd has shared mutable aliases and no exclusivity. A reach that
> changed with the flow of one alias would be wrong for the others, so
> heap reaches are flow-insensitive and invariant under `mut`. Bindings are
> not shared, so they can follow the flow, as Rust's and Swift's locals do.

## 3. Notation

| Form | Meaning |
| --- | --- |
| `c: ^T` | A parameter that is local and kept nowhere the signature does not name. Bare `^`, only on a parameter. |
| `c: ^a T` | A parameter that is local, and whose reach may flow into `a`'s object: a store. `a` names a parameter, `self`, or a slot path such as `s.u`. |
| `-> ^a T` | A result that reaches at most what `a` reaches. `a` names a parameter, `self`, a slot path, a declared slot, a requirement key, or `static`. |
| `^a+b T` | Reaches at most what `a` or `b` reaches: usable while both live. The union mirrors `$ A + B`. |
| `-> ^() T` | A result that reaches no input: fresh. It mirrors the empty row `$()`. The caller gets a new local root. |
| `-> ^static T` | A result that reaches only `static`. Only on a result: an unmarked parameter already says "global". |
| `f: ^fn(x: ^T) -> ^x+f U` | A function type naming its parameters. `^f` is the closure's environment ([8.2](#82-function-types)). |
| `data S[^c, ^u]` | Declares two lifetime slots. Opt-in, for data holding independent handles. |
| `Session[^a, ^b]` | Applies a slot-bearing type to reaches. |
| `closing c: T`, `closing self` | The callee may close this parameter's handle. It implies `^`. |
| `trait K < NonEscapable`, `impl NonEscapable for S` | Marks a root type. |

```text
lifetime    = "^" [ reach ]
reach       = "()" | atom { "+" atom }
atom        = "static" | name { "." name }
param_type  = lifetime type          (bare or with sources: c: ^Conn, c: ^self Conn)
result_type = "^" reach type         (never bare)
fn_param    = [ name ":" ] param_type   (a function type may name its parameters)
slot_param  = "^" name               (in a data or enum generic list)
```

1. r[ne.syntax.prefix] A lifetime is a prefix of the type it bounds, as in Rust's `&'a T`. It comes before `mut`, as in `c: ^mut Conn` and `-> ^a mut Conn`.
2. r[ne.syntax.position] A bare `^` is written only on a parameter. `^()` and `^static` are written only on a result. `^a` on a parameter is a store into `a`, and on a result it is a dependence on `a`. Error: `misplaced-lifetime`. (agent)
3. r[ne.syntax.one-reach] `^S T` bounds the one reach of the value, or every declared slot of slot-bearing data. A lifetime is never written inside a type argument: `List[^c Conn]` is an error. Error: `lifetime-in-type-argument`. A slot argument of slot-bearing data, as in `Session[^a, ^b]`, is not a type argument. (owner, 2026-10-14, D6)
4. r[ne.syntax.fn-own] On a function type, `^S` bounds the function value's own reach only, which is its environment, never its inputs or result. (agent)
5. r[ne.syntax.names] A signature names parameters, slot paths, keys, and `static`, never lifetime variables. A function type may name its own parameters (D8, `ne.fntype.names`). Only `data` and `enum` declarations declare slots.
6. r[ne.syntax.slot-args] Slot arguments come before type arguments, as in `Registry[^p]` or `Pair[^a, ^b, T]`.
7. r[ne.syntax.receiver] A receiver is a parameter like any other, written `self: ^Self` or `self: ^mut Self`. The existing `mut self` is shorthand for `self: mut Self` (`fn.method.mut-self`), so a marked receiver may be spelled `^self` and `^mut self`. (agent, [Q10](#14-owner-questions))

> **Why.** A suffix is ambiguous on function types: `fn!() -> void ^static`
> reads as a lifetime on `void`. Naming parameters instead of variables is
> Mojo's `origin_of(x)` and Swift's `@lifetime(copy x)`. Kotlin marks a
> local parameter on the parameter, and so does this design.

## 4. Signatures And Elision

A signature states, for each parameter, whether it is local; for each
result and each store, which inputs it may reach. Elision fills the common
result cases.

```hd
fn excerpt(c: ^Conn) -> Excerpt                  # elided: ^c Excerpt
fn longer(a: ^Conn, b: ^Conn) -> ^a+b Conn
fn adopt(self: ^mut Pool, c: ^self Conn) -> void # self takes on c's roots
fn accept!(self: ^Self) -> ^() Conn
fn register(job: fn!() -> void) -> void          # unmarked: global, may be stored
fn each(xs: ^List[Conn], f: ^fn(c: ^Conn) -> void) -> void   # f is not kept
```

### 4.1 Parameter Marks

1. r[ne.param.mark] A parameter is local when its type is written with `^`: bare `^`, or `^S` with sources. A parameter without `^` is **global**. This holds for every type, function types included. (owner, 2026-10-14, D1, D4, D5)
2. r[ne.param.global] A global parameter may be stored anywhere by the callee, so an argument for it must fit `static` ([4.4](#44-call-sites)). In the body its value reaches `static` (`ne.root.global`). (owner, 2026-10-14, D1)
3. r[ne.param.nonescapable] A parameter whose type is NonEscapable as written must be marked. A **NonEscapable type** is a root type, or a data type, enum type, tuple, or type application with a NonEscapable field or type argument as written, such as `Excerpt` or `List[Conn]`. An unmarked one is an error with a fix-it that adds `^`. Error: `unmarked-local-param`. (owner, 2026-10-14, D1)
4. r[ne.param.generic] A parameter whose type is a type parameter, or contains one, or is `dyn K` for a trait `K` that is not a root trait, is global when unmarked, and accepts an argument whose reach is not `static` only when written `^T` or `^dyn K`. Otherwise the call is an error. This reverses the round-2 default that type parameters accept lifetimes, and restores the parked TQ-25's opt-in (Swift's `T: ~Escapable`). Error: `lifetime-mismatch`. (owner, 2026-10-14, D2)
5. r[ne.param.bare] A bare `^` parameter is **never kept**: its parameter root must not reach a global position, another parameter's object, or a result, unless the signature names it. The reach machinery tracks the flow, so a flow into a local object is fine until that object's reach flows out. Error: `lifetime-mismatch`. (owner, 2026-10-14, D1, D3, D4)
6. r[ne.param.sources] `^S` names where the parameter's reach may flow beyond the call: a store into the object of `S`, as `c: ^self Conn` does. A result that names the parameter, or elides to it, is the other permitted flow. (owner, 2026-10-14, D3, D6)
7. r[ne.param.any-type] Any parameter of any type may be marked `^`. The callee body is checked as for a handle: the parameter has a root in the body, and `ne.param.bare` applies. For a value that is not NonEscapable the caller tracks nothing extra, because its reach is empty. (owner, 2026-10-14, D4)
8. r[ne.param.ordinary-result] A result of a type that carries nothing may still name a `^` parameter, as `-> ^xs List[i32]` does. The caller tracks nothing, and the callee may return part of the parameter. (agent)
9. r[ne.param.fn] A function-typed parameter follows the same rule, as in Kotlin. Unmarked, the callee may store it, so a closure that captures a local root is an error at the call. `^fn(T) -> U` is not kept. There is no implicit default for a callback, as Swift has. `^static` on a parameter is unnecessary and is not written. (owner, 2026-10-14, D5)
10. r[ne.param.closing] A `closing` parameter is local without writing `^`. Writing both is allowed. (agent)
11. r[ne.param.lint] A **lint** proposes `^` on an unmarked parameter whose body never keeps it. The lint is a suggestion: it never changes a signature, so a signature still never depends on its body. Warning: `could-be-local`. (owner, 2026-10-14, D5)
12. r[ne.param.interface] Adding `^` to a parameter widens what callers may pass and is compatible. Removing it narrows that and is a breaking change. (agent)

```hd
fn ping(c: Conn) -> void:                  # error: unmarked-local-param
    c.send!([0])                           # fix-it: c: ^Conn

fn store_all[T](xs: ^List[T], x: T) -> void:   # x is global: an argument must fit static
    pass
```

> **Why.** Kotlin marks the parameter and has no default for lambdas. A
> default, as Swift's non-escaping closures have, gives two rules for one
> concept. One mark on every kind of parameter lets a check, a lint, and
> the optimizer read the same fact.

### 4.2 Lifetime-Carrying Inputs

1. r[ne.sig.carrying] A type is **lifetime-carrying** when it is a root type, a holder ([5.1](#51-the-category)), a function type, `Suspend[T]`, a `dyn` type, or mentions a type parameter. It may reach roots, or may for some instantiation.
2. r[ne.sig.input] A **tracked input** is a `^`-marked parameter of lifetime-carrying type, a NonEscapable key in the row, or a row parameter. An unmarked parameter is never one: it reaches only `static`, which fits every bound. A `^` parameter of a type that carries nothing is not one either.
3. r[ne.sig.generic-result] A result type that mentions a type parameter is lifetime-carrying, so it elides like any other result. `get` elides to `self`. `map` writes its reach, because the closure's results reach the closure ([8.2](#82-function-types)).

### 4.3 Elision

| Position | Without `^` |
| --- | --- |
| A result, lifetime-carrying, no tracked input | `^()`: fresh. |
| A result, lifetime-carrying, exactly one tracked input | That input. |
| A method result, lifetime-carrying tracked `self` | `self`. |
| Any other result | An error at the declaration. |
| A store of one parameter into another | An error at the store, unless the stored parameter is marked `^p`. |

1. r[ne.elide.none] A lifetime-carrying result, in a declaration with no tracked input, is `^()`. (owner, 2026-10-14, D7)
2. r[ne.elide.one] With exactly one tracked input, it is that input. (owner, 2026-10-14, D7)
3. r[ne.elide.self] In a method whose `self` is a tracked input, it is `self`. (owner, 2026-10-14, D7)
4. r[ne.elide.ambiguous] Otherwise the declaration must write `^`. Error: `ambiguous-lifetime`.
5. r[ne.elide.store] A body that stores one parameter's value into another parameter's object is an error unless the stored parameter is marked `^p` for that object, as `c: ^self Conn` is. A global parameter's value reaches `static`, so storing it needs no mark. Error: `lifetime-mismatch`.
6. r[ne.elide.store-meaning] `c: ^p T` on a parameter means the caller's `p` takes on `c`'s roots, as Swift's `@lifetime(p: copy c)` does.
7. r[ne.elide.lint] A result elided by `ne.elide.one` or `ne.elide.self` whose body never makes it reach that input gets a warning that suggests `^()`. Warning: `overtied-result`. (agent)

> **Why.** Elision picks the only possible source and never a union.
> When there are several sources it asks, because a wrong guess is either
> unsound or a false alarm. The self rule and the one-input rule are
> Rust's. The warning catches the false alarm they can cause, as in
> [R9](#r9-accept). Every parameter now says whether it is local, so
> parameters need no elision at all: the owner keeps elision for results.

### 4.4 Call Sites

1. r[ne.call.result] At a call, a result bounded by `^a+b` reaches the reach of the arguments for `a` and `b`.
2. r[ne.call.fresh] A result bounded by `^()` reaches a new local root of the caller, created at the call.
3. r[ne.call.static] A result bounded by `^static` reaches `static`.
4. r[ne.call.store] An argument for `c: ^p T` adds its reach to the object of the argument for `p`, by `ne.heap.store` or `ne.heap.fixed`.
5. r[ne.call.global] An argument for a global parameter must have a reach that fits `static`. A reach with a local, parameter, or key root is an error at the call, never an escape. Error: `lifetime-mismatch`. (owner, 2026-10-14, D2)
6. r[ne.call.local] An argument for a `^` parameter adds no constraint beyond its sources and the results that name it. For a value that carries nothing, the caller tracks nothing. (owner, 2026-10-14, D4)

## 5. Data Types And Slots

### 5.1 The Category

1. r[ne.data.root-type] A **root type** is a trait that has `NonEscapable` as a direct or transitive supertrait, or a data or enum type with `impl NonEscapable for S`. Every value of a root type starts as its own root.
2. r[ne.data.root-impl] A data type that implements a root trait must declare `impl NonEscapable` too, by the ordinary supertrait rule (`trait.super.missing`).
3. r[ne.data.holder] A **holder** is a type with a field, payload, or type argument that is lifetime-carrying. A holder is NonEscapable as written when that field or argument is ([4.1](#41-parameter-marks)).
4. r[ne.data.one-reach] A data or enum type without declared slots has one reach. Every lifetime-carrying field shares it, and it needs no declaration. This is Kotlin's `this`. (owner, 2026-10-14, D6)
5. r[ne.data.tuple] A tuple has one reach: the union of its elements'. (owner, 2026-10-14, D6)
6. r[ne.data.generic] A generic data type is a holder exactly for the instantiations whose arguments are lifetime-carrying. `Box[i32]` carries nothing, and `Box[Conn]` has one reach, which grows when a `Conn` is stored in it. No lifetime appears in the type argument (`ne.syntax.one-reach`). (owner, 2026-10-14, D6)

> **Why.** The category matters where values start, so only root types
> declare it. Holders follow their fields, as Rust's auto traits do; this
> replaces the parked TQ-24 rule, which followed type arguments and so
> missed a `Suspend[void]` that holds a handle. One reach per value is
> Kotlin's model: it loses the independence of two handles in one
> container, which declared slots give back.

### 5.2 Declared Slots

```hd
data Session[^c, ^u]:
    client: ^c Conn
    upstream: ^u Conn
```

1. r[ne.data.slots] `data S[^c, ^u]` declares the slots `c` and `u`, for data that holds independent handles. A type with one handle, or handles that share a lifetime, declares nothing. Slot names are local to the declaration. (owner, 2026-10-14, D6)
2. r[ne.data.field-slot] In a type with declared slots, every lifetime-carrying field names the slots it reaches, as `client: ^c Conn` does. A field that leaves them unnamed is an error. Error: `undeclared-slot`. (agent)
3. r[ne.data.path] Outside the declaration, a value's slot is named by a path: `^s.c` is slot `c` of parameter `s`, and `^self.c` is slot `c` of the receiver.
4. r[ne.data.whole] A bare `^s` names the union of all of `s`'s slots.
5. r[ne.data.recursive] A recursive occurrence of slot-bearing data is a field, so it names its slots like any field, with the union of the declaration's slots when it sits in a wrapper, as in `next: ^a+b Duo?`. A slot not declared there is an error. Error: `undeclared-slot`. (agent)
6. r[ne.data.wrapper] A generic type applied to slot-bearing data, `Option[Session]` or `List[Session]`, has one reach: the union of the slots. A value taken out of it has every slot at that union. This loses independence, and is sound ([L5](#139-known-limits)). (agent)
7. r[ne.data.literal] A data literal infers its slot arguments from its field values at the use site.

### 5.3 Variance Of Slots

1. r[ne.var.subtype] `^S T` is assignable to `^S' T` when `S` is a subset of `S'`, ignoring `static`.
2. r[ne.var.polarity] A declared slot's variance is computed by the `types.polarity` rules over the type's readonly surface, as `+T` is checked. It is not written. (agent)
3. r[ne.var.mut-data] Under a `mut` view, the reach of a data, enum, list, or map value, and every declared slot, is invariant, because a field or element can be reassigned (`types.variance.mut-invariant`).
4. r[ne.var.dyn] The reach of `dyn K` is covariant when no method of `K`, `mut self` methods included, has a parameter marked `^self`. Otherwise it is invariant, even under a readonly view. (agent)
5. r[ne.var.fn] A function value's own reach and a `Suspend`'s are covariant: a function value carries no access permission (`fn.type.no-permission`).
6. r[ne.var.generic] The one reach of a generic container is covariant through a readonly view. There are no lifetimes inside type arguments, so there is nothing to compose. (owner, 2026-10-14, D6)

> **Why.** Rust computes lifetime variance from the declaration. hd
> declares type variance but would need a new marker for slots; the owner
> decides ([Q5](#14-owner-questions)). The rules let a long-lived handle
> enter a short-lived readonly list, and stop a `mut` alias from widening
> a reach that another alias relies on.

## 6. Closing

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

1. r[ne.close.mode] `closing` is a parameter mode, like Swift's `consuming`. It is written before the parameter name, or as `closing self`. It implies `^` (`ne.param.closing`).
2. r[ne.close.type] A `closing` parameter must have a lifetime-carrying type. Error: `type-mismatch`.
3. r[ne.close.owned] A closing call is valid only when every root in the argument's reach is a local root, the root of a `closing` parameter, or `static`. Error: `closing-not-owned`. (agent)
4. r[ne.close.fields] Inside a body, closing `self.client` through `closing self` closes a root `self` reaches, so it is valid.
5. r[ne.close.in-defer] A closing call written directly in a `defer` suite pins, by `ne.pin.closing-defer`.
6. r[ne.close.early] A closing call outside a `defer` is an **early close**. It pins nothing, and a later use gives `Disposed` at run time.
7. r[ne.close.defer-sync] A closing call in a `defer` must be a call of a function that does not suspend. A plain call of a `fn!` there is an error, because its cold frame closes nothing. Error: `closing-suspension-in-defer`. (agent)
8. r[ne.close.closure] A closure body must not make a closing call on a captured value. Error: `closing-in-closure`. (agent)
9. r[ne.close.closure-own] A closure may close a handle it created itself.
10. r[ne.close.static] A closing call on a value whose reach is `static` is always an early close.
11. r[ne.close.trait] A trait method may be declared `closing self`. Through `dyn K` it is a closing call like any other.

```hd
fn handle!(closing c: Conn) -> void:
    defer: c.close()          # valid: the caller handed c over to be closed
    c.send!([1])

fn relay!(client: ^Conn) -> void:
    s := Session { client: client, upstream: connect!("db") }
    defer: s.shutdown()       # error: closing-not-owned: s reaches the caller's client
```

> **Why.** Without `closing`, a wrapper that closes its fields would pin
> nothing ([S3](#s3-a-cleanup-wrapper)). Ownership per root closes the
> same gap through locals ([X5](#x5-closing-a-borrowed-handle-through-a-local)).
> The closure rule is Swift's ban on consuming a captured noncopyable
> value ([X4](#x4-a-cleanup-closure)).

## 7. Body Checking

A body is checked against its own signature and its callees' signatures.
It never reads another body.

### 7.1 Uses

1. r[ne.body.use] A **use** of a value is any read of it: a call argument, a receiver, a field read, a return, a store, or a capture.
2. r[ne.body.kill] Assigning a binding with `=` is not a use of its old value.
3. r[ne.body.poll-cancel] Polling or cancelling a suspension is a use of it. Dropping the last reference is not (`req.discard.abandoned`).
4. r[ne.body.live] A value is **live** on an edge when some path from the edge may use it, directly or through a live binding whose reach contains it.

### 7.2 Signature Conformance

1. r[ne.body.result] Every value the body returns, or propagates with `?`, must have a reach that fits the result's bound, after elision. Free local roots fit any bound. Error: `lifetime-mismatch`.
2. r[ne.body.param-store] A store into a parameter's object must fit that parameter's reach, which `ne.elide.store` extends only as written. Error: `lifetime-mismatch`.
3. r[ne.body.local] A `^` parameter is checked with the same reach machinery, whatever its type. Its root must not flow into a global position, into a parameter's object the signature does not name, or into a result that does not name it. The check uses the flow-sensitive reaches of 7.1 and the solved reaches of [12.2](#122-per-body). Error: `lifetime-mismatch`. (owner, 2026-10-14, D4)
4. r[ne.body.global-arg] A call that passes a value to a global parameter needs the value's reach to fit `static` (`ne.call.global`). In a body, a `^` parameter's root is never `static`, so a `^` parameter cannot be passed to a global parameter. (owner, 2026-10-14, D2)

### 7.3 Escapes

1. r[ne.escape.def] A local root **escapes** when its reach flows into the result or into a parameter's object.
2. r[ne.escape.free] An escaped root is fine while it is free. By `ne.pin.escaped`, it can never be pinned.
3. r[ne.escape.global] A flow into a top-level binding or a global parameter is not an escape. It is an error, because a NonEscapable value flows only into local positions. Error: `lifetime-mismatch`. (owner, 2026-10-14, D2)

### 7.4 What Is Checked

| Check | Rule |
| --- | --- |
| Result and stores fit the signature | `ne.body.result`, `ne.body.param-store` |
| A `^` parameter is not kept | `ne.body.local` |
| No local value flows to a global position | `ne.call.global`, `ne.escape.global` |
| Each pinned root is owned | `ne.pin.param`, `ne.close.owned` |
| No pinned root escapes | `ne.pin.escaped` |
| No value reaching a pinned root is live on the pin block's exits | `ne.outlive.check` |
| No earlier-registered suite of the block reads it | `ne.pin.defer-order` |

## 8. Generics, Function Types, And Erasure

### 8.1 Generic Substitution

1. r[ne.generic.default] A type parameter accepts no NonEscapable argument by default. `x: T` is a global parameter (`ne.param.generic`), and `x: ^T` accepts one. This reverses the round-2 default and restores the parked TQ-25's opt-in. (owner, 2026-10-14, D2)
2. r[ne.generic.carry] A generic type applied to a lifetime-carrying type argument has one reach, which grows with each store. `List[Conn]` has the reach of the handles stored in it. No lifetime is written inside the argument. (owner, 2026-10-14, D6)
3. r[ne.generic.abstract] In a generic body, a value of type `T` reaches `static` when its parameter is unmarked, and the parameter's root when it is `x: ^T`. A value of type `T` taken out of a container reaches the container's reach. Nothing else is known about it.
4. r[ne.generic.store] A generic function stores a `^T` argument into a container with the store mark: `fn push(self: ^mut List[T], v: ^self T)`. A generic function that stores an argument in module state takes it unmarked, `x: T`. There is no `^static T` parameter (`ne.param.fn`). (owner, 2026-10-14, D5, D6)
5. r[ne.generic.infer] Slot arguments of generic data are inferred at use sites like type arguments. A declaration's slots are never inferred from its body.
6. r[ne.generic.no-lifetime-arg] An explicit type argument carries no `^`. `List[^c Conn]` and `keep::[^c Conn](x)` are errors (`ne.syntax.one-reach`). (owner, 2026-10-14, D6)

```hd
impl[T] List[T]:
    fn push(self: ^mut List[T], v: ^self T) -> void       # stores v
    fn contains(self: ^List[T], v: ^T) -> bool            # keeps nothing
    fn get(self: ^List[T], index: usize) -> T?            # elided: ^self
```

### 8.2 Function Types

A function type is elided by the rules a function uses (owner, 2026-10-14,
D8). It replaces the round-2 union default and the known limit L3.

1. r[ne.fntype.own] A function type has one own reach: the reach of its captures, called its **environment**.
2. r[ne.fntype.names] A function type may name its parameters: `fn(x: ^T) -> ^x U`. A name is needed only for a `^` in the result to refer to. `fn.type.no-names` must change for this. (owner, 2026-10-14, D8)
3. r[ne.fntype.inputs] An input of a function type follows `ne.param.mark`: `fn(x: ^T)` is local, and an unmarked input is global. A callback that is called with a handle writes `^` on that input.
4. r[ne.fntype.env] In a function type that is a parameter `f: ^fn(...)`, or sits inside one's type, `^f` names the environment, the closure's captures. The environment is a tracked input of the function type. (agent-detail: orchestrator proposal, naming the environment by the parameter.)
5. r[ne.fntype.elide] A result of a function type that is lifetime-carrying is elided by the tracked inputs of that type: its `^` inputs and its environment. No tracked input gives `^()`. Exactly one gives that input, so a function type with no parameters depends on its environment. Anything else must be written, and an unwritten one is an error at the declaration. Error: `ambiguous-lifetime`. A function item used as a value has an empty environment, so only its parameters count. (owner, 2026-10-14, D8)
6. r[ne.fntype.call] A call of a function value instantiates its bound: each named input becomes its argument's reach, and the environment name becomes the reach of the function value being called.
7. r[ne.fntype.row] A NonEscapable key in a function type's row is resolved at each call, so the result may reach that call's provider.
8. r[ne.fntype.subtype] Inputs are contravariant, and the result and the environment covariant. A result bound `^x` is assignable where `^x+f` is expected.
9. r[ne.fntype.carry-only] A written reach applies only where the instantiated result carries lifetimes. `map` over a `List[usize]` result tracks nothing.

```hd
fn map[T, U](xs: ^List[T], f: ^fn(x: ^T) -> ^x+f U) -> ^xs+f List[U]
fn each(xs: ^List[Conn], f: ^fn(c: ^Conn) -> void) -> void     # void: nothing to name
fn retry![T, E, $R](times: usize, attempt: ^fn!() -> Result[T, E] $ R) -> Result[T, E] $ R
fn all_list![T, $R](tasks: ^List[fn!() -> T $ R]) -> List[T] $ R
```

> **Why.** The owner's example writes `x: T` unmarked. Under D2 that input
> would be global, so `^x` would name nothing and the callback could not
> take a handle. The inputs are written `x: ^T` here ([Q12](#14-owner-questions)).
> `map` writes both results because an element of the new list may be a
> value of `f`'s environment or of `xs`. `retry!` and `all_list!` write
> nothing: a function type with no parameters depends on its environment,
> and the outer result then elides to the one tracked parameter.

### 8.3 Erasure And Downcasting

1. r[ne.erase.slot] Every `dyn K`, including `dyn Any` and `dyn Inspectable`, has one reach.
2. r[ne.erase.join] Erasing a value to `dyn K` joins all its slots into that one reach.
3. r[ne.erase.allowed] Erasing a lifetime-carrying value is allowed into a local position: a binding, or a parameter written `^dyn K`. A parameter `x: dyn K` is global, so it takes only a `static` value (`ne.param.generic`). This replaces the parked TQ-24 "erasure is rejected", so a `dyn Error` may hold a handle in a local. (owner, 2026-10-14, D2)
4. r[ne.erase.downcast] `downcast`, `downcast_mut`, and `downcast_val` return `T` with every slot set to the `dyn` value's reach.
5. r[ne.erase.downcast-invariant] When `T`'s reach is invariant at the result's view, the `dyn` value's reach must be `static`. Error: `downcast-invariant-slot`. (agent)
6. r[ne.erase.identity] Slots are not part of runtime identity. `TypeId::of::[Session[^a, ^b]]()` equals `TypeId::of::[Session[^c, ^d]]()`.

> **Why.** Rule 5 is Rust's `Any: 'static`, narrowed to the reaches that
> can be widened unsoundly ([X6](#x6-widening-through-a-downcast)).
> A covariant reach widened to the union stays sound.

## 9. Closures, Suspensions, And Cancellation

1. r[ne.closure.slot] A closure's own reach is the reach of the bindings it captures. Each captured binding has one reach (`ne.local.captured`).
2. r[ne.suspend.slot] A cold frame's own reach is the reach of its arguments and its construction-time providers (`req.lowering.frame`, `req.bind.captured`).
3. r[ne.suspend.exact] So a plain call `f(a, b)` of a `fn!` has a frame that reaches `a` and `b`. This union is exact: the frame stores them all.
4. r[ne.suspend.bang] A bang call uses its suspension and its arguments only until the call returns.
5. r[ne.suspend.stored] A stored suspension is an ordinary value: its holder reaches its roots, and polling or cancelling it later is a use.
6. r[ne.suspend.cancel-order] Cancelling a parent runs each unfinished child's cleanup before the parent's `defer` suites (`req.cancel.defer`). So a child driven by a bang call never sees its parent's pinned handles closed.
7. r[ne.all.typing] `all!` and `race!` take their children's frames for the call only, so their child parameters are written `^`. The result tuple, or value, carries its own types' reach.
8. r[ne.all.race] `race!` cancels the losers before it returns (`req.combinator.race-losers`), so no loser is live after the call.
9. r[ne.all.list] `all_list!` takes a list of closures, as `tasks: ^List[fn!() -> T $ R]`. The list is an ordinary value, live after the call when a binding still uses it.
10. r[ne.all.global] A combinator parameter left unmarked is global, so a frame that captures a handle is an error at the call. (owner, 2026-10-14, D5)

> **Why.** hd has no detached spawn, so structure covers every child
> driven by a bang call. A lifetime is needed only for a suspension that is
> stored, as in [R10](#r10-a-borrow-across-a-suspension) and
> [D2](#d2-a-cold-suspension-in-a-field).

## 10. Providers

1. r[ne.provider.key] A requirement key may be a NonEscapable trait.
2. r[ne.provider.root] In a declaration, each NonEscapable key of its row is a key root, named by the key, as `^Pool`. A row parameter `$R` is named `^R`. A key is not a parameter, so `ne.param.nonescapable` does not apply to it. (agent)
3. r[ne.provider.use] `$.use(K)` reaches the key root `K`.
4. r[ne.provider.with] Inside `$.with(K=p)`, calls instantiate `^K` with `p`'s reach.
5. r[ne.provider.frame] A frame built inside that scope captures the provider, so its reach includes `p`'s reach.
6. r[ne.provider.closure] A closure captures no provider (`fn.capture.no-providers`). Its row carries the key, and each call resolves it there.
7. r[ne.provider.close] A provider reached only through `$.use` is not owned, so closing it is an error. Error: `closing-not-owned`.
8. r[ne.provider.flow] `req.use.value.flow` and `req.use.value.outlives-scope` stay true, bounded by the provider's reach.

## 11. Top Level, REPL, And Annotation Handles

1. r[ne.top.static] A top-level binding, and a binding made by a REPL input, reaches `static`.
2. r[ne.top.elide] A lifetime-carrying top-level binding's declared type is `^static`, without writing it. (agent)
3. r[ne.top.store] A store into a top-level binding's object requires the value's reach to fit `static`. A value that reaches a local, parameter, or key root is an error, not an escape. Error: `lifetime-mismatch`. (owner, 2026-10-14, D2)
4. r[ne.top.no-pin] Top level is not a cleanup scope (`flow.defer.not-scopes`), so nothing is pinned there, and a REPL session may keep `c := connect!("db")` across inputs (`cli.repl.declarations.bindings`).
5. r[ne.handle.no-slot] An annotation handle `Field[S, F]` has no reach and may escape (`annot.handle.escape`), since it stores no `S` or `F`.
6. r[ne.handle.get] `h.get(x)` has the type of `x.member` (`annot.handle.get`), including its slot path, as in `^x.c Conn`. (agent)
7. r[ne.handle.build] A value that `build` passes for a member must fit that member's slot in the value being built.

## 12. Checker Cost, Files, And Optimizer Facts

### 12.1 What A Signature Exports

- For each parameter: whether it is `^`-marked, its sources (the objects it may be stored into), and whether it is `closing`. This is the `LocalParams` fact ([12.6](#126-what-the-optimizer-gets)).
- For each result and each `c: ^p` store: a reach, as a set of atoms over parameters, slot paths, keys, row parameters, and `static`, or `()`. A function type inside a signature exports its own result reaches, over its named inputs and its environment.
- For each data and enum type: its declared slot list, the slot each field uses, and each slot's computed variance.
- For each trait: whether it is a root trait, and each method's signature as above.

All of it is written, or elided by local rules, so no export depends on a
body.

### 12.2 Per Body

1. **Variables.** One reach variable per lifetime-carrying binding version (SSA), object site, and call result, and one per declared slot of slot-bearing data. Most types have one reach.
2. **Constraints.** Each store and call adds subset constraints
   (`reach(x) ⊆ reach(o)`), and each invariant position adds an equality.
   A callee signature is instantiated by substituting argument reaches
   for parameter names; `^()` mints a local root. A global parameter adds
   `reach(arg) ⊆ {static}`.
3. **Solving.** The least solution over a finite set of roots: a
   worklist over subset edges with bitset reaches, and union-find for the
   equalities. A body rarely has more than 64 roots, so a reach is one
   machine word.
4. **Pins.** For each `defer` closing call, take the argument's reach at
   each exit of its block.
5. **Liveness.** One backward bitvector pass over the body's control
   flow, as non-lexical lifetimes use. For each pin, test the live-out
   set of the block's exits, and the suites registered before it.
6. **Ownership and escape.** Set tests on the solved reaches.
7. **Local parameters.** For each `^` parameter, test that its root is
   absent from every reach it may not enter: the globals, other
   parameters' objects, and results. These are set tests on the solved
   reaches. A parameter of a type that carries nothing gets a root only
   in its own body, and none at its callers.

Cost: `O((N + E) · ⌈R/64⌉)` per body, for `N` program points, `E`
constraints, and `R` roots, so linear in practice. There is no global
fixed point: signatures are summaries. Marking every parameter of an
ordinary type adds one root per marked parameter in that body only.

### 12.3 Incremental And Cross-File Effects

- A body's result depends only on its own text and its callees'
  signatures. Editing a body never invalidates another body.
- Slot variance depends on the declaration's readonly surface, as type
  variance already does (`types.variance.surface`). Adding a method that
  stores into a slot can change variance, which is an interface change.
- Interface files carry the exports in 12.1. A change to a reach in a
  public signature is an API change, like a change to a row.
- Adding `^` to a parameter is compatible, and removing it is breaking
  (`ne.param.interface`). So a lint that proposes `^` ([4.1](#41-parameter-marks))
  never makes an upgrade unsafe.

### 12.4 What It Reuses

| Need | Existing structure |
| --- | --- |
| Reach as a set with `+` and `()` | Requirement rows: `RowData { keys, params }` in `crates/hd_types/src/pool.rs`; union and membership entailment (`req.row.set.union`, `req.row.entail.membership`) |
| Instantiating a reach at a call | Row fitting in `crates/hd_check/src/rows.rs` (`fit_row`, `solve_exact_row`, `collect_infer`); the least solution of `req.row.least.solution` |
| A second parameter kind | The kind check behind `generic-kind-mismatch` and the `$` marker (`req.row.param.marked`) |
| Slot variance | The `types.polarity` check |
| Liveness | None in `hd_check` today. `hd_opt` has passes for code generation, but the checker needs its own. |
| The `LocalParams` fact | Per-body facts in `crates/hd_opt/src/passes.rs` (`BodyFacts`), which `devirt_known`, `closure_spec`, and `escape` already read |

Rows forbid parameters on data (`req.row.param.no-data`). Declared slots
must not copy that rule: they are required for independent handles (owner
requirement 5). So slot parameters are a new kind, beside type-kinded and
row-kinded parameters, and the type pool needs slot arguments on data
types. Under D6 only data that holds independent handles pays for this.

### 12.5 Annotation Burden In std (Estimate)

Counts come from a scan of `lib/std/**/*.hd`: 1261 `fn` declarations.
A parameter is classified by its type text, so the edges are approximate.
The lists for stores and callbacks were made by hand. "Public" means a
`pub fn` or a trait method. Leaving any parameter unmarked is sound, since
unmarked means global. The marks are needed to accept code that passes a
handle, or a closure that captures one, except where the table says an
error.

| Class | What | Public | Private |
| --- | --- | --- | --- |
| **Must change** | Handle traits: `< NonEscapable`, `closing self` on `close`, `^` on the other receivers (D1) | 3 + 3 + 5 | 0 |
| **Follows from marking** | Written result reaches on adapters and `map`-like methods (D3, D8): `List` 4, `Option` 2, `Result` 3, `Iterator` 7 | 16 | not counted |
| **Follows from marking** | Written function-type results (D8): `Option` 2, `Iterator` 3, `Result` 3, `List` 4, `std.testing` 6 more | 12 | 0 |
| Accept handles | `^()` on `accept!` (the three `Net` methods no longer need it) | 1 | 0 |
| Accept handles | Store marks `^self` (D6): `List` `push`, `insert`, `extend`; `Deque` `push_back`, `push_front`; `Set.insert`; `Heap.push` | 7 | 0 |
| Accept handles | Bare `^` on generic receivers and parameters (D2, D6): 109 receivers, 92 parameters | 201 | 237 |
| Accept handles | Bare `^` on `dyn` and function-typed parameters (D2, D5): 5 and 35 | 40 | 40 |
| Accept handles | `^` on the inputs of callback types, `fn(x: ^T)` (D2, D8): 25, and 6 in `std.testing` | 25 | about 10 |
| **Optimizer-only** | `^` on parameters of an ordinary heap type (strings, lists of scalars, plain data): 243 receivers, 157 parameters | 400 | 863 |
| Never needed | Scalars, passed by value | 83 | 239 |

Reading the table:

- **Must change.** Eight receivers and three trait lines. These are the
  only edits whose absence is an error today: an unmarked NonEscapable
  parameter is `unmarked-local-param`. Round 2 counted 10 edits for the
  handle traits. The three `Net` methods drop out because `Net`'s receiver
  is unmarked, so it is not a tracked input and `connect!` elides to
  `^()`.
- **Follows from marking.** Once an adapter marks its callback `^`, the
  result must say whether it keeps the callback. 28 public signatures
  write a reach, against none in round 2. Round 2's claim "collection,
  iterator, option, result, and task signatures: 0" is gone, because D6
  removed lifetimes from type arguments and D8 removed the union default.
- **Accept handles.** About 274 public marks. This is the price of the
  opt-in of D2, and it is mechanical: the lint proposes each one and a
  fix-it applies it ([Q13](#14-owner-questions)). A first pass can mark
  only the containers and callbacks that the stress tests use.
- **Optimizer-only.** About 400 public marks that no checker result
  depends on. They are the lint's suggestions, and they feed the facts of
  [12.6](#126-what-the-optimizer-gets). Std can add them where a profile
  shows a gain.

Collection and iterator methods need markers but no new concepts.
Elements carry their handles in the container's one reach, and callbacks
are function values with an environment.

### 12.6 What The Optimizer Gets

A parameter written `^` tells the optimizer, from the signature alone,
that the callee does not keep the argument (owner, 2026-10-14, D4). Today
the facts in `crates/hd_opt/src/value_flow.rs` and the passes in
`crates/hd_opt/src/passes.rs` get this only by proving it from value
flow, body by body.

1. r[ne.opt.fact] The signature exports `LocalParams`: for each parameter, whether it is `^`-marked, and its sources (the result, or the objects of other parameters). A marked parameter with no sources is **never kept**. One with sources is kept only there. The checker verified it (`ne.body.local`), so the optimizer reads it without analysing the body. (owner, 2026-10-14, D4)
2. r[ne.opt.escape] The `escape` pass treats a call that passes a value to a never-kept `^` parameter as not escaping through that call. With sources, the value escapes exactly when the named result or object does. An unmarked parameter is assumed to escape. (owner, 2026-10-14, D4)
3. r[ne.opt.closure] `closure_spec` and `devirt_known` take their licence from the signature. A closure literal passed to a `^fn` parameter is not stored by the callee, so a per-call-site instance may call it directly and drop the closure value afterwards, with no value-flow proof that the parameter is not stored. (owner, 2026-10-14, D4)
4. r[ne.opt.reuse] An object created in a loop body and passed only to never-kept `^` parameters dies with the iteration, so its allocation may be reused across iterations. This needs no inlining, because the fact is in the signature. (owner, 2026-10-14, D4)
5. r[ne.opt.fusion] An iterator adapter writes `^self+transform` on its result, so the intermediate iterator is reachable only through the next stage. A chain `it.map(f).filter(g)` is then a stack-shaped pipeline whose stages die together, which is the licence #65 needs to fuse it into one loop. (owner, 2026-10-14, D4)
6. r[ne.opt.honest] None of this removes an environment or a cell by itself. Wasm cannot point at locals, so a callee that mutates a captured binding through a `^` closure still needs the closure environment or a cell, unless specialization or inlining makes the binding visible in the callee's body. The mark licenses dropping the environment after specialization, and does not replace specialization. (owner, 2026-10-14, D4)
7. r[ne.opt.parked] Non-local `return` or `break` from a local callback, as Kotlin's inline lambdas allow, is a separate future feature. A `^` parameter does not enable it. It is parked, and is not part of this design. (owner, 2026-10-14, D4)

```hd
fn fill(buf: ^mut List[u8], n: usize) -> void:   # buf is never kept
    for i in 0..n:
        buf.push(1)

fn encode!(frames: List[usize]) -> void:
    for n in frames:
        let buf: mut List[u8] = []               # a fresh object each iteration
        fill(buf, n)                             # never kept
        emit!(buf)                               # emit! marks buf ^ too: dies here
```

> **Why.** The facts exist, but each needs a proof today. `ValueFlow`
> sees a closure only when it is made in the call's own body, or passed on
> unchanged. A signature mark proves it across calls, and so across files.

## 13. Stress-Test Suite

The suite uses this prelude. It drops `Result` handling for brevity; the
real `std.net` methods return `Result` and take `mut self`. Every
NonEscapable parameter is marked (D1), and the closing methods write
`closing self`.

```hd
trait NonEscapable                # placeholder std marker

trait Socket < NonEscapable:
    fn send!(self: ^Self, bytes: List[u8]) -> void
    fn read!(self: ^Self) -> string
    fn sent(self: ^Self) -> usize
    fn close(closing self) -> void

trait Acceptor < NonEscapable:
    fn accept!(self: ^Self) -> ^() Conn
    fn close(closing self) -> void

type Conn = dyn Socket
type Listener = dyn Acceptor

fn connect!(host: string) -> Conn      # no tracked input: ^()
fn listen!(host: string) -> Listener
```

The container methods the examples use, under D6:

```hd
impl[T] List[T]:
    fn push(self: ^mut List[T], v: ^self T) -> void
    fn get(self: ^List[T], index: usize) -> T?
    fn contains(self: ^List[T], v: ^T) -> bool
```

### 13.1 Verdicts

The last column is the re-check under the owner's decisions of 2026-10-14
(D1 to D8). "Same" means the verdict stands and only the notation changed.

| # | Source | Verdict | Change forced | Re-check, 2026-10-14 |
| --- | --- | --- | --- | --- |
| R1 | Rust book `longest` | Works | none | same |
| R2 | Rust book dangling reference | Works | none | same |
| R3 | Rust `ImportantExcerpt` | Works | one reach (`ne.data.one-reach`) | same |
| R4 | Rust `Parser<'c, 's>` | Works with one lifetime | none | same |
| R5 | Rust slice iterator | Works | none | same |
| R6 | Rustonomicon `evil_feeder` | Works; loop form rejected, as in Rust | example corrected | same; the unmarked signature is now rejected at the call |
| R7 | Rust `thread::spawn` | Works | prefix notation | same; `register` loses `^static`, a handle-capturing job is rejected at its call |
| R8 | Rust reassignment | Works | none | same |
| R9 | Rust `TcpListener::accept` | Needs `^()` | `^()`; `overtied-result` warning | **changed**: `accept!` only; `Net` methods elide to `^()` |
| R10 | Rust borrow across `.await` | Works | none | same |
| R11 | Rust `std::thread::scope` | Works | none | same; `all_list!` marks `tasks` `^` |
| R12 | Rust lifetime-generic method | Works with one path | slot paths on methods | same |
| R13 | Rust `HashMap` of handles | Works; mixed map rejected, as in Rust | known limit L2 | same; the map's one reach replaces its value slot |
| R14 | Rust `Option<&T>` with `?` | Works | none | same |
| R15 | Rust self-referential struct | Works, unlike Rust | none | same |
| S1 | Swift `Span` | Works | none | same |
| S2 | Swift `@lifetime(self: copy other)` | Works | none | same; `push` writes `^self` |
| S3 | Swift `consuming` close wrapper | Hole in round 1 | `closing` mode | same |
| S4 | Swift `MutableSpan`, `@_lifetime(&self)` | Works, no exclusivity | none | same |
| S5 | Swift conditional `Escapable` | Works | holders follow fields | same; an unmarked `Tagged[Conn]` parameter is now a declaration error |
| S6 | Swift non-escaping closure parameter | Works | none | same; the parameter needs `^` |
| M1 | Mojo `ref [a, b]` | Works | none | same |
| M2 | Mojo struct origin parameter | Works | none | same; no declared slot needed |
| M3 | Mojo capturing versus escaping closures | Works | none | same |
| T1 | Two independent slots | Works | none | same |
| T2 | Swapping two slots | Rejected, as in Rust | none | same |
| T3 | REPL top-level handle | Works | static roots | same |
| D1 | A handle in a `Result` through `?` | Rejected correctly | `ne.outlive.return` | same |
| D2 | A cold `Suspend` in a field | Rejected; strict | known limit L1 | same |
| D3 | `race!` and cancellation with a `defer` | Works | `ne.suspend.cancel-order` | same; `race!` marks `tasks` `^` |
| D4 | A NonEscapable provider | Works | key roots `^Pool` | same |
| D5 | `Map[string, Conn]` in a slot | Works | none | **changed**: no declared slot needed (D6) |
| D6 | `dyn Conn` list mixing lifetimes | Works | `ne.erase.join` | same |
| D7 | Recursive data of handles | Works | `ne.data.recursive` | **changed**: implicit reach; wrapper rule for declared slots |
| D8 | Callback registry | Works | `^static`, top-level elision | **changed**: a closure that captures a handle is rejected at the call |
| N1 | Unmarked NonEscapable parameter | Rejected, fix-it | D1 | new |
| N2 | Generic and `dyn` opt-in | Works | D2 | new |
| N3 | General locality | Works | D4 | new |
| N4 | Function types | Works | D8 | new |
| N5 | `closing` implies `^` | Works | agent | new |
| X1 | A late pin | Hole | `ne.pin.escaped` | same hole; now arises through a store |
| X2 | `defer` order | Hole | `ne.pin.defer-order` | same |
| X3 | A `defer` reads the binding at exit | Hole and false alarm | `ne.pin.at-exit` | same |
| X4 | A cleanup closure | Hole | `ne.close.closure` | same |
| X5 | Closing a borrowed handle through a local | Hole | `ne.close.owned` | same |
| X6 | Widening through a downcast | Hole | `ne.erase.downcast-invariant` | same |
| X7 | A suspending close in a `defer` | Hole | `ne.close.defer-sync` | same |

Five verdicts changed in substance: R9, D5, D7, D8, and the way X1
arises. R6, R7, and S5 keep their verdict but gain a rejected form that
the new default produces.

### 13.2 Rust

#### R1. `longest`

Rust: `fn longest<'a>(x: &'a str, y: &'a str) -> &'a str`.

```hd
fn longer(a: ^Conn, b: ^Conn) -> ^a+b Conn:
    if a.sent() > b.sent(): a else: b

fn pick!(l: ^Listener, keep: ^Conn) -> ^keep Conn:   # two inputs: written
    c := l.accept!()
    defer: c.close()
    longer(keep, c)           # error: handle-outlives-cleanup: the result reaches c
```

#### R2. A Dangling Reference

```hd
fn dangle!() -> Conn:
    c := connect!("db")
    defer: c.close()
    c                         # error: handle-outlives-cleanup
```

Without the `defer`, `c` is free and may be returned.

#### R3. `ImportantExcerpt`

Rust: `struct ImportantExcerpt<'a> { part: &'a str }`.

```hd
data Excerpt:
    conn: Conn                # one reach for the whole value

fn excerpt(c: ^Conn) -> Excerpt:     # elided: ^c
    Excerpt { conn: c }
```

Round 2 wrote `data Excerpt < NonEscapable:`, which is not hd syntax
(data types take no supertrait list). A holder needs no declaration
(`ne.data.one-reach`). A parameter of type `Excerpt` is NonEscapable as
written, so it must carry `^` (`ne.param.nonescapable`).

#### R4. `Parser<'c, 's>`

Rust needs two lifetimes because the parser borrows a temporary context.

```hd
data Router:
    pool: List[Conn]

impl Router:
    fn pick(self: ^Self) -> Conn:         # elided: ^self
        self.pool[0]

fn route(pool: ^List[Conn]) -> Conn:      # elided: ^pool
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

impl ConnIter:
    fn next(self: ^mut Self) -> Conn?:    # elided: ^self, the conns it holds
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
    value: T

fn feed[T](slot: ^mut Box[T], v: ^slot T) -> void:   # v is stored into slot
    slot.value = v

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
form for the same reason (`ne.outlive.loop`). It is a true alarm at the
signature level, because `feed` may read `slot` before writing it.

Round 2 wrote `fn feed[T](slot: mut Box[T], v: T)`. Under D2 both
parameters are global, so `feed(keep, tmp)` is rejected at the call, since
`keep` and `tmp` reach local roots (`ne.call.global`). The program was
rejected before it reached the invariance; the `^slot` form keeps the
original lesson.

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
so `^static` is not written on a parameter.

#### R8. Reassignment

```hd
fn best(a: ^Conn, b: ^Conn) -> ^a Conn:
    let pick = b
    _ := pick.sent()
    pick = a
    pick                      # fine: the binding's reach follows its last assignment
```

#### R9. `accept`

In Rust an owned result has no lifetime. Here the one-input and self
rules would tie the accepted connection to its listener:

```hd
fn handoff!(host: string) -> Conn:
    l := listen!(host)
    defer: l.close()
    l.accept!()               # fine with ^(); with the elided ^self, an error
```

So `accept!` writes `-> ^() Conn`. The `overtied-result` warning tells
an implementer whose body returns a fresh handle under an elided input.
A trait method has no body, so its author must write `^()`.

Round 2 needed `^()` four times in std, on `accept!` and on `Net`'s
`connect!`, `listen!`, and `bind_udp!`. Under D1 the `Net` receiver is
unmarked, so it is not a tracked input, and those three results elide to
`^()` by `ne.elide.none`. std needs it once
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
    defer: c.close()
    echo!(c)                  # fine: the bang call returns before the defer runs
    QUEUE.push(echo(c))       # error: lifetime-mismatch: QUEUE reaches static
```

No change. `QUEUE` is hd's `tokio::spawn`: storing a frame in module
state needs a `static` reach, as spawning needs `'static`.

#### R11. `std::thread::scope`

```hd
use std.task.all_list

fn broadcast!(c: ^Conn, frames: List[List[u8]]) -> void:
    _ := all_list!([for f in frames => fn!() -> void: c.send!(f)])   # fine

fn fan_out!(hosts: List[string]) -> void:
    let jobs: mut List[fn!() -> void] = []
    for h in hosts:
        c := connect!(h)
        defer: c.close()
        jobs.push(fn!(): c.send!([0]))    # error: handle-outlives-cleanup
    _ := all_list!(jobs)
```

`all_list!` is the scope: its children may capture handles of the
enclosing block. They may not capture a handle closed in an inner block
before the scope runs, which is the same rule `thread::scope` enforces.
`all_list!` marks `tasks` `^`, so a list of closures that capture a
handle is a legal argument (`ne.all.list`).

#### R12. A Lifetime-Generic Method

Rust: `impl<'a> Parser<'a> { fn part<'b>(&self, key: &'b str) -> &'a str }`.

```hd
impl Session:
    fn route(self: ^Self, hint: ^Conn) -> ^self.u Conn:   # neither all of self nor hint
        self.upstream
```

Elided, the result would be `self` by the self rule, the union of both
slots, and a caller who keeps the result past the client's block would get
a false alarm. One slot path fixes it. No change: slot paths on methods
already exist (`ne.data.path`). `Session` declares two slots because its
handles are independent (`ne.data.slots`).

#### R13. A `HashMap` Of Handles

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

Rust rejects the same program: a `HashMap<&str, &'a S>` has one `'a`.
Under D6 the verdict is the same for a plainer reason: a `Map` has one
reach, the union of everything stored in it, and an index store adds to it
(`ne.heap.store`). The alarm is true at the type level, because the map
does hold a closed handle. Removing `"tmp"` before the block ends does not
help, because a heap reach is one reach for the object's life (known limit
L2). The workaround is a map scoped to the inner block. A map of two
independent kinds of handle would need a data type with declared slots.

#### R14. `Option<&T>` With `?`

```hd
fn first_open(pool: ^List[Conn]) -> Conn?:     # elided: ^pool
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

data Reader:
    conn: Conn
    lines: Lines              # a view over conn: the same root

fn reader(c: ^Conn) -> Reader:        # elided: ^c
    Reader { conn: c, lines: Lines { conn: c } }
```

It works. A lifetime is a set of roots, not a memory region, and hd has
no moves, so nothing invalidates the view.

### 13.3 Swift

#### S1. A View Over A Handle

Swift: `Span` with `@_lifetime(borrow self)`.

```hd
impl Lines:
    fn next!(self: ^Self) -> string: self.conn.read!()

fn lines(c: ^Conn) -> Lines:          # elided: ^c
    Lines { conn: c }

fn first_line!(l: ^Listener) -> string:
    c := l.accept!()
    defer: c.close()
    lines(c).next!()                  # fine: a string carries no lifetime
```

#### S2. A Mutating Store

Swift: `@_lifetime(self: copy other)`.

```hd
data Pool:
    conns: mut List[Conn]

impl Pool:
    fn adopt(self: ^mut Self, c: ^self Conn) -> void:
        self.conns.push(c)
```

`List[Conn]` carries no lifetime in its type. `push` writes
`v: ^self T`, so `self.conns.push(c)` adds `c`'s reach to the list, and so
to `self`, as `c: ^self Conn` allows. Swift does not yet allow an `Array`
of `~Escapable` elements.

#### S3. A Cleanup Wrapper

Swift: `consuming func close()`; Rust: `drop(self)`.

```hd
impl Session:
    fn shutdown(closing self) -> void:
        self.client.close()
        self.upstream.close()

fn proxy!(l: ^Listener) -> Conn:
    s := Session { client: l.accept!(), upstream: connect!("db") }
    defer: s.shutdown()       # a closing call: pins every root of s
    s.client                  # error: handle-outlives-cleanup
```

Without the `closing` mode, a wrapper that closes its fields pins
nothing, and `s.client` escapes while closed. Round 2 wrote
`defer: s.shutdown!()`; a `defer` cannot bang-call
(`flow.defer.restricted`), so the wrapper does not suspend.

#### S4. `MutableSpan` And `@_lifetime(&self)`

Swift's `mutableSpan` property depends on `self` through an exclusive
mutable borrow (`&`).

```hd
data Writer:
    conn: Conn
    buffer: mut List[u8]

impl Writer:
    fn flush!(self: ^mut Self) -> void:
        self.conn.send!(self.buffer)
        self.buffer = []

fn writer(c: ^Conn) -> Writer:        # elided: ^c
    Writer { conn: c, buffer: [] }
```

It works. Two writers over one `c` may coexist: hd promises no
exclusivity, only that neither outlives `c`'s cleanup. Swift's `&` scope
buys exclusivity, which hd does not want. `send!`'s `bytes` parameter is
unmarked and global, and `self.buffer` reaches nothing, so it fits.

#### S5. Conditional `Escapable`

Swift: `struct Tagged<T: ~Escapable>: ~Escapable` plus
`extension Tagged: Escapable where T: Escapable`.

```hd
data Tagged[T]:
    tag: string
    value: T

let SEEN: mut List[Tagged[i32]] = []
let SEEN_CONNS: mut List[Tagged[Conn]] = []

fn remember(t: Tagged[i32]) -> void:
    SEEN.push(t)                      # fine: Tagged[i32] carries nothing

fn remember_conn(t: Tagged[Conn]) -> void:   # error: unmarked-local-param
    pass                                     # fix-it: t: ^Tagged[Conn]

fn remember_conn(t: ^Tagged[Conn]) -> void:
    SEEN_CONNS.push(t)                # error: lifetime-mismatch: SEEN_CONNS reaches static
```

Nothing is written on `Tagged`: holders follow their arguments
(`ne.data.generic`). `Tagged[Conn]` is NonEscapable as written, so the
unmarked parameter is a declaration error (`ne.param.nonescapable`).

#### S6. A Non-Escaping Closure Called Later In The Call

```hd
fn first_ok!(n: usize, attempt: ^fn!() -> bool) -> bool:
    let tries: mut List[fn!() -> bool] = []
    for _ in 0..n:
        tries.push(attempt)           # a local list: dead before the call returns
    for t in tries:
        if t!(): return true
    false
```

It works: `tries` is a local object, and it is not live after the body.
`attempt` must be marked `^`: unmarked, the closure's callers could not
pass a closure that captures a handle (`ne.param.fn`). `attempt` is kept
nowhere, because `tries` never reaches the result. Storing `attempt` in
`self` would need `attempt: ^self fn!() -> bool`. Swift does not let a
non-escaping closure enter an array at all; hd's liveness rule is more
permissive.

### 13.4 Mojo

#### M1, M2. Origins

`fn pick(a, b) -> ref [a, b] String` is `-> ^a+b Conn`.
`struct Span[T, origin: Origin]` is `data Window` with one reach, and a
`ref [self]` result is the elided `^self`. A declared slot is needed only
for independent handles (`ne.data.slots`).

#### M3. Capturing Versus Escaping Closures

Mojo's reference captures (`{imm x}`, `{mut x}`) cannot outlive their
scope. A copy capture (`{var x}`, `@__copy_capture`) detaches the closure
from the original, so it may escape.

```hd
fn counter(c: ^Conn) -> fn() -> usize:    # elided: ^c
    fn() -> usize: c.sent()

fn tally(n: usize) -> fn() -> usize:      # elided: ^(), no tracked input
    fn() -> usize: n
```

It works with one rule. Copying a handle does not detach it from its
root, so hd needs no copy-capture escape: a closure reaches what it
captures, by value or by binding. The result type `fn() -> usize` has no
parameter, so its own result depends on its environment, and `usize`
carries nothing (`ne.fntype.carry-only`).

### 13.5 Two Slots And The REPL

#### T1. Independent Slots

```hd
fn client_of(s: ^Session) -> ^s.c Conn:
    s.client

fn proxy!(l: ^Listener) -> ^() Conn:
    keep := l.accept!()
    up := connect!("db")
    defer: up.close()
    client_of(Session { client: keep, upstream: up })   # fine: only keep's root
```

#### T2. Swapping Slots

```hd
fn swap_in(s: ^mut Session) -> void:
    s.client = s.upstream     # error: lifetime-mismatch: ^s.u does not fit ^s.c
```

As in Rust, two distinct slots never take each other's values.

#### T3. A REPL Handle

A REPL input `c := connect!("db")` binds a static root. Later inputs may
use it, and may pass it to a global parameter, since its reach fits
`static`. A `defer` that closes it is an error (`ne.pin.static`); an early
`c.close()` is fine.

### 13.6 hd-Specific

#### D1. A Handle In A `Result` Through `?`

```hd
enum LinkError:
    Broken(conn: Conn)                # the error holds the failed handle

fn check!(c: ^Conn) -> Result[void, LinkError]:    # elided: the error reaches c
    if c.sent() == 0: return .Err(LinkError.Broken(c))
    .Ok(())

fn talk!() -> Result[void, LinkError]:
    c := connect!("db")
    defer: c.close()
    check!(c)?                # error: handle-outlives-cleanup: ? returns the error
    .Ok(())
```

Rejected correctly: the caller would receive a closed handle inside the
error. `?` is a return, live after the block's `defer` suites
(`ne.outlive.return`). `LinkError` may erase to `dyn Error` in a local
position, with one reach, and the reach is covariant, so `downcast` stays
available (`ne.erase.downcast`).

#### D2. A Cold Suspension In A Field

```hd
data Worker:
    next: mut Suspend[void]?

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

`arm!` is a true alarm. `staged!` is strict: the frame is gone, yet `w`'s
reach still includes `c`, because a heap reach is one reach for the
object's life (known limit L1). Rust rejects the same program: a struct's
lifetime parameter covers its whole life. The smallest fix would be a
strong update for an object no other binding can reach, which Rust's
borrow checker does for locals but not for fields ([Q9](#14-owner-questions)).
The workaround is to keep the frame in a binding inside the block.

#### D3. `race!` And Cancellation With A `defer`

```hd
use std.task.race

fn fastest!(a: ^Conn, b: ^Conn) -> string:
    race!(a.read(), b.read())         # the loser is cancelled before race! returns

fn ask!(l: ^Listener, backup: ^Conn) -> string:
    c := l.accept!()
    defer: c.close()
    fastest!(c, backup)               # fine, even if ask! is cancelled mid-wait
```

If `ask!` is cancelled inside `race!`, both children finish their
cancellation cleanup before `c.close()` runs (`req.cancel.defer`). No
lifetime check is needed. [X2](#x2-defer-order) shows the one ordering
case that does need a check. `race!` marks its child parameter `^`
(`ne.all.typing`), so the frames, which capture `a` and `b`, are legal
arguments.

#### D4. A NonEscapable Provider

```hd
trait Pool < NonEscapable:
    fn get(self: ^Self) -> Conn       # elided: ^self
    fn close(closing self) -> void

fn grab() -> Conn $ Pool:             # elided: ^Pool, the only tracked input
    $.use(Pool).get()

fn open_pool!() -> dyn Pool

let KEEP: mut List[Conn] = []

fn serve!() -> void:
    p := open_pool!()
    defer: p.close()
    c := $.with(Pool=p):
        grab()                        # c reaches p
    c.send!([1])                      # fine
    KEEP.push(c)                      # error: lifetime-mismatch: KEEP reaches static
```

The key is a tracked input named by the key (`ne.provider.root`), and it
is not a parameter, so it needs no `^`. `$.use(Pool).close()` inside
`grab` would be `closing-not-owned`.

#### D5. `Map[string, Conn]` In A Holder

```hd
data Registry:
    by_name: mut Map[string, Conn]       # one reach: no declared slot

impl Registry:
    fn add(self: ^mut Self, name: string, c: ^self Conn) -> void:
        self.by_name[name] = c

    fn find(self: ^Self, name: string) -> Conn?:    # elided: ^self
        self.by_name.get(name)
```

It works. The value reach is one reach for every value, as in Rust. Round
2 declared `Registry[^p]` and wrote `^self.p`. Under D6 a type with one
reach declares nothing, and `c: ^self Conn` says the same.

#### D6. A `dyn Conn` List Mixing Lifetimes

```hd
data Fake: pass

impl NonEscapable for Fake

impl Socket for Fake:
    fn send!(self: ^Self, bytes: List[u8]) -> void: pass
    fn read!(self: ^Self) -> string: ""
    fn sent(self: ^Self) -> usize: 0
    fn close(closing self) -> void: pass

fn mix!(real: ^Conn) -> void:
    let all: mut List[Conn] = [real, Fake {}]   # reaches real's root and a new one
    for c in all:
        c.send!([1])
```

It works: erasure joins the slots (`ne.erase.join`), and `Fake {}` is a
root site. An error arises only when one of the roots is pinned and the
list outlives that pin, as in R13.

#### D7. Recursive Data Of Handles

```hd
enum Chain:
    End
    Link(conn: Conn, next: Chain)     # one reach for the whole chain

data Duo[^a, ^b]:
    x: ^a Conn
    y: ^b Conn
    next: ^a+b Duo?                   # a wrapper: the union of both slots
```

`Chain` works with no declaration. `Duo` is the declared-slot case
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
    sum_bytes([1, 2, 3])                  # the caller tracks nothing
```

D4. A `^` parameter of an ordinary type is checked in the callee with the
reach machinery. The value carries no handle, so the caller adds no
tracking. A call to `remember_bytes` gets no extra constraint, and the
callee's own body is the error. `ne.opt.fact` makes `sum_bytes` and
`fill` known as never keeping their argument.

#### N4. Function Types

```hd
fn map[T, U](xs: ^List[T], f: ^fn(x: ^T) -> ^x+f U) -> ^xs+f List[U]
# as the std method: fn map[U](self: ^List[T], transform: ^fn(x: ^T) -> ^x+transform U) -> ^self+transform List[U]
fn each(xs: ^List[Conn], f: ^fn(c: ^Conn) -> void) -> void
fn retry![T, E, $R](times: usize, attempt: ^fn!() -> Result[T, E] $ R) -> Result[T, E] $ R
fn all_list![T, $R](tasks: ^List[fn!() -> T $ R]) -> List[T] $ R

fn bad[T, U](f: ^fn(x: ^T) -> U) -> U    # error: ambiguous-lifetime: the function type has x and its environment

fn sizes!(pool: ^List[Conn]) -> List[usize]:
    pool.map(fn(c): c.sent())            # fine: the result type carries nothing
```

D8. `map` writes both sources on the function type and on its own result,
because a mapped element may reach `xs` or the closure's captures. `each`
returns `void`, so there is no result to name. `retry!` has a function
type with no parameter, which depends on its environment, and the outer
result elides to `attempt`, the one tracked input. `all_list!` is the
same through `tasks`. `bad` leaves the function type's result unnamed
with two tracked inputs. Round 2 gave such a type the union by default,
which was the one guess left (known limit L3, now resolved).

The call in `sizes!` instantiates `^xs+f` with `pool` and the closure,
but `List[usize]` carries nothing, so no root enters the result
(`ne.fntype.carry-only`).

#### N5. `closing` Implies A Local Parameter

```hd
fn shut(closing c: Conn) -> void:     # no ^ written: closing implies it
    c.close()

fn shut_twice(closing c: ^Conn) -> void:   # allowed, redundant
    c.close()
```

Agent choice (`ne.param.closing`). A closing parameter is closed by the
callee and so never kept ([Q11](#14-owner-questions)).

### 13.8 Holes Found

#### X1. A Late Pin

```hd
fn setup!(l: ^Listener, reg: ^mut Registry) -> void:
    c := l.accept!()
    reg.add("a", c)                   # c flows into reg's object
    defer: c.close()                  # error: pin-escaped-root
```

Round 2 checked pins against later uses only, and showed the escape as
`on_message(fn(m): _ := c.sent())` into `HANDLERS`. Under D2 that call is
rejected at the call ([D8](#d8-a-callback-registry)), so a global escape
can no longer reach a pin. The same hole remains through a store into a
parameter's object, as above. The caller's `reg` outlives `c`'s close. Fix:
a root that escapes can never be pinned, in any order (`ne.pin.escaped`).
Borrows: Rust checks borrows against all uses of a region, not only the
later ones.

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
an earlier-registered suite of the same block counts as after the pin
(`ne.pin.defer-order`). Borrows: Rust's borrow checker knows drop order
and rejects a value borrowed by something dropped later.

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
cleanup time (`flow.defer.captures`). A pin taken at registration would
pin `first`'s root, a false alarm, and miss the second handle, a hole.
Fix: pin the reach at each exit (`ne.pin.at-exit`). Borrows: nothing
directly. Go evaluates a deferred call's arguments at registration, and
hd does not.

#### X4. A Cleanup Closure

```hd
fn leak!() -> Conn:
    c := connect!("db")
    cleanup := fn() -> void: c.close()    # error: closing-in-closure
    defer: cleanup()
    c
```

The closure is a wrapper, like S3, and `defer: cleanup()` would pin
nothing. Fix: a closure never closes a capture (`ne.close.closure`).
Write a function with a `closing` parameter instead; then `defer: shut(c)`
pins, and `c` is rejected at the return. Borrows: Swift rejects consuming
a captured noncopyable value in a closure.

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
out of a borrow.

#### X6. Widening Through A Downcast

```hd
use std.inspect.Inspectable

data Stash:
    items: mut List[Conn]             # beneath mut: the reach is invariant

fn smuggle(s: ^mut Stash, tmp: ^Conn) -> void:
    let e: mut dyn Inspectable = s
    let wide: ^s+tmp mut dyn Inspectable = e     # covariant: Inspectable has no ^self parameter
    match wide.downcast_mut::[Stash]():          # error: downcast-invariant-slot
        .Some(back) => back.items.push(tmp)      # would put tmp into the caller's list
        .None => pass
```

Round 1 said `downcast` returns the `dyn` value's reach. Widening the
`dyn` reach first and downcasting then widens an invariant reach. Fix:
downcasting to a type with an invariant reach needs a `static` reach
(`ne.erase.downcast-invariant`). Borrows: Rust's `Any: 'static`, kept
only where it is needed.

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
closed, yet the call would pin. Fix: a closing call in a `defer` must not
suspend (`ne.close.defer-sync`). Borrows: nothing; it follows from
`flow.defer.restricted`.

### 13.9 Known Limits

| # | Limit | Example | Rust |
| --- | --- | --- | --- |
| L1 | A heap reach keeps a root after the value is overwritten | D2 | same |
| L2 | A container's values share one reach | R13 | same |
| L3 | A function type's result is the union of its inputs and captures | `ne.fntype.result` | **resolved**: D8 elides it like a function (owner, 2026-10-14) |
| L4 | A loop-carried mutable target reaches every iteration's roots | R6 loop form | same |
| L5 | A wrapper of declared-slot data joins its slots | D7 | none: Rust has no wrapper of lifetime-generic types that joins |

L1 and L2 are false alarms in the strict sense, though the alarm is
true at the signature level: the object does hold, or did hold, a closed
handle. L3 is resolved, and no union default is left. L5 is new, and is an
agent choice. Each is listed for the owner ([Q9](#14-owner-questions),
[Q15](#14-owner-questions)).

### Diagnostics

| Code | When |
| --- | --- |
| `handle-outlives-cleanup` | A value reaching a pinned root is live after the pin's block, or read by an earlier-registered suite. |
| `lifetime-mismatch` | A value's reach does not fit the bound it flows into: a result, a store, a global parameter, a top-level binding, or a `^` parameter kept where it is not named. |
| `unmarked-local-param` | A parameter whose type is NonEscapable as written has no `^`. The fix-it adds it. |
| `lifetime-in-type-argument` | A `^` appears inside a type argument. |
| `misplaced-lifetime` | A bare `^` on a result, or `^()` or `^static` on a parameter. |
| `ambiguous-lifetime` | Elision has no single answer, in a function or a function type, so `^` must be written. |
| `closing-not-owned` | A closing call reaches a root the function does not own. |
| `closing-in-closure` | A closure closes a captured value. |
| `closing-suspension-in-defer` | A `defer` makes a closing call that would only build a frame. |
| `pin-escaped-root` | A pinned root also escapes. |
| `pin-static-root` | A `defer` closes a static value. |
| `undeclared-slot` | A field or recursive occurrence names no declared slot. |
| `downcast-invariant-slot` | A downcast would widen an invariant reach. |
| `overtied-result` (warning) | An elided result is tied to an input the body never returns. |
| `could-be-local` (warning) | An unmarked parameter that the body never keeps. The lint proposes `^`. |

## 14. Owner Questions

They are ordered core first, spellings last. The decided ones are listed
first, with the decision. Questions already settled in round 2 are not
repeated.

### 14.1 Decided

| Was | Decision | Where |
| --- | --- | --- |
| Round-2 Q5, the self rule | Keep result elision as the doc has it, with the `overtied-result` warning (owner, 2026-10-14, D7). | `ne.elide.*` |
| Round-2 Q6, generic defaults | **Reversed.** Type parameters accept no lifetime-carrying argument by default. `x: ^T` opts in (owner, 2026-10-14, D2). TQ-24 to TQ-26 are replaced by sections 4, 5, and 8.3. | `ne.param.generic`, `ne.generic.default` |
| Round-2 Q8, function-type results | Name the parameters and use the function elision rules. No union default (owner, 2026-10-14, D8). The environment is named by the parameter (agent-detail). | `ne.fntype.*` |
| New (D1) | Every NonEscapable parameter is written `^`. Unmarked is global, and an unmarked NonEscapable parameter is an error with a fix-it. | `ne.param.mark`, `ne.param.nonescapable` |
| New (D3) | Dependent results and stores stay. "Local" means "lives no longer than its sources". | section 2 |
| New (D4) | Any parameter of any type may be marked `^`, checked in the callee, and read by the optimizer as `LocalParams`. Non-local `return` is parked. | `ne.param.any-type`, [12.6](#126-what-the-optimizer-gets) |
| New (D5) | Function-typed parameters follow the same rule. No implicit default. `^static` is removed from parameters. A lint proposes `^`. | `ne.param.fn`, `ne.param.lint` |
| New (D6) | No lifetime inside a type argument. One reach per value. Declared slots stay, opt-in. Containers write `^self` on what they store. | `ne.syntax.one-reach`, `ne.data.*`, `ne.generic.*` |

### 14.2 Open

1. **Early close.** Should a closing call outside a `defer` stay a
   run-time matter?
   Options:
   - (a) yes: it pins nothing, and a later use gives `Disposed`;
   - (b) pin it to the enclosing block.

   **Recommendation: (a).** (b) rejects the common "close on the error
   path, return on the success path" helper. The guarantee then covers
   exactly the handles a `defer` closes.

2. **Where the category is declared.** Options:
   - (a) only root types declare it (`trait K < NonEscapable`,
     `impl NonEscapable for S`); holders have one reach;
   - (b) every holder declares it too, as the parked TQ-24 said.

   **Recommendation: (a)** (agent). Under (b), `Iterator`, every data
   type with a callback field, and every `Suspend` holder need a
   declaration, because a function type carries a reach.

3. **Ownership and closing.** Adopt `ne.close.owned` (ownership per
   root) and `ne.close.closure` (closures never close captures)?
   **Recommendation: yes** (agent). Without them, X4 and X5 close a
   handle without pinning it. Each borrows a shipped rule (Rust's
   no-move-out-of-borrow, Swift's no-consuming-capture).

4. **Pins.** Adopt `ne.pin.at-exit`, `ne.pin.defer-order`, and
   `ne.pin.escaped`?
   **Recommendation: yes** (agent). They fix X3, X2, and X1, and cost
   nothing in the cost model: they reuse the solved reaches and the
   liveness pass.

5. **Slot variance.** Options:
   - (a) computed from the declaration surface, as Rust does
     (`ne.var.polarity`);
   - (b) declared with a marker, as hd's `+T` is, unmarked meaning
     invariant.

   **Recommendation: (a)** (agent). Under (b) an unmarked slot is
   stricter than needed, which requirement 4 forbids. Adding a storing
   method then changes variance, an interface change hd already accepts
   for inherent methods (`types.variance.surface`). D6 makes declared
   slots rare, so the choice affects little code.

6. **Downcasts.** Adopt `ne.erase.downcast-invariant`?
   **Recommendation: yes** (agent). X6 shows the hole. Rust forbids all
   non-static downcasts; this forbids only invariant reaches.

7. **Providers and top level.** Name key roots `^Pool` and elide
   top-level types to `^static`?
   **Recommendation: yes** (agent). Neither adds syntax beyond `^`.

8. **Static values.** May a `static` handle be closed early anywhere,
   with a `defer` that closes it an error (`ne.pin.static`)?
   **Recommendation: yes** (agent). A REPL or `main` closes globals,
   and nothing after a top-level binding's last use can be checked.

9. **Strong updates.** Should the checker reset a reach when a field of
   an object no other binding reaches is overwritten (L1)?
   **Recommendation: no, for now** (agent). Rust ships without it. It
   needs an alias analysis that the cost model does not have.

10. **How `self` is marked, and `mut`.** A receiver is a parameter, so it
    is written `self: ^Self` or `self: ^mut Self`, and the `mut` of a
    view comes after the `^` (`ne.syntax.prefix`). Options:
    - (a) the explicit typed form only;
    - (b) a shorthand `^self` and `^mut self`, beside `mut self`
      (`fn.method.mut-self`);
    - (c) a default per `impl` block, so a block marks every receiver at
      once.

    **Recommendation: (b)** (agent). A marked receiver is the commonest
    mark in std, 109 public receivers of generic types, and (b) costs one
    character. (c) breaks D1's rule that unmarked is always global. A
    `mut` view needs no mark of its own: `^mut T` and `mut T` differ only
    in the `^`, and a `mut` parameter of an ordinary type stays unmarked
    until the lint proposes it.

11. **`closing` implies `^`.** Adopt `ne.param.closing`?
    **Recommendation: yes** (agent). A closing parameter is closed by the
    callee, so it is never kept. Writing `^` as well would repeat it.

12. **Function types: the environment name, unmarked inputs, and other
    positions.** D8 names the environment by the function-typed parameter,
    an orchestrator proposal. Three details follow:
    - the owner's example writes the input unmarked, `fn(x: T)`, which
      under D2 is global and cannot take a handle. The doc writes
      `fn(x: ^T)`;
    - a function type outside a parameter, as a result, field, or local,
      has no name for its environment, so an ambiguous one cannot be
      written;
    - `fn.type.no-names` changes, which needs a spec edit.

    **Recommendation:** keep the parameter name for the environment,
    write `^` on callback inputs, and leave the other positions as a known
    limit: a function type that is a result and has two tracked inputs is
    rare in std. If it matters, a data wrapper with a method gives it a
    parameter to name.

13. **std migration order and burden.** [12.5](#125-annotation-burden-in-std-estimate)
    counts about 274 public marks to accept handles and about 400 more
    for the optimizer. Options:
    - (a) one pass, driven by the lint's fix-it;
    - (b) first the 8 receivers, the 7 stores, and the callbacks the stress
      tests use, then the rest;
    - (c) only the 8 receivers and wait for demand.

    **Recommendation: (b)**, with the optimizer marks added where a
    profile shows a gain. (c) leaves `List[Conn]` unusable.

14. **The global default and requirement 4.** D1, D2, and D5 make an
    unmarked non-retaining callback reject a closure that captures a
    handle. Should that count as an error with a fix-it, and not as a false
    alarm under requirement 4?
    **Recommendation: yes.** The rule is stated, the lint finds the
    parameter, and the fix-it is one character. Requirement 4 then
    governs inference only.

15. **Two small agent choices.** (a) A wrapper of declared-slot data has
    one reach, joining its slots (`ne.data.wrapper`, L5). (b) A result of
    an ordinary type may name a `^` parameter (`ne.param.ordinary-result`).
    **Recommendation: yes to both.** (a) is sound and rare. (b) lets a
    callee return part of an ordinary parameter without tracking at the
    caller.

16. **Spellings** (last). `^` for lifetimes (`'` is the character
    literal), `^()` mirroring `$()`, `^static` on results only, `closing`
    (Swift's `consuming` is about ownership transfer, which hd does not
    have), `NonEscapable` (Swift's term), and `^f` for an environment.
    **Recommendation: keep them.** `closing` names exactly what the mode
    permits.

## Appendix A. Why A Category And Lifetimes

Round 1 (the predecessor's study) split the owner's claim into three
parts. Round 2 accepted all three, and requirement 2 makes the third
binding.

| Part | Round-1 verdict | Decisive example |
| --- | --- | --- |
| A type-level category | Holds, for annotation burden rather than soundness | `defer: log.push(...)` beside `defer: c.close()` |
| Lifetime relations in the semantics | Holds | `open.push(c)` across a loop body |
| Lifetimes written in signatures | Needed to accept good programs, never to reject bad ones | `longest`, `accept`, the two-handle struct |

Round 3 adds a fourth part (owner, 2026-10-14): which positions are local.
A parameter written `^` is local, and an unmarked parameter is global, so
the checker, a lint, and the optimizer read one mark.

**The category.** The checker must know which values a `defer` cleans up:

```hd
fn serve_one!(log: mut List[string], l: ^Listener) -> void:
    c := l.accept!()
    defer: log.push("closed")   # log outlives this call: not pinned
    defer: c.close()            # c must not outlive this block
    log.push("accepted")
```

Marking roots at each use is sound, but then every composite type carries
a lifetime, and `fn register(u: User)` needs an annotation though it
touches no resource. Marking producers' results misses a user's fake
handle converted to `dyn TcpStream`. Round 2's `closing` mode now does
the separating, and the category says where roots start.

**Lifetime relations.** A yes/no marker cannot accept one of these and
reject the other:

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
joins it: `fn push(self: ^mut List[T], v: ^self T)`. This is Kotlin's
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
and reaches are sets, as in family 3. Data declares slots, as family 1
does, which owner requirement 5 needs: family 2 cannot extract the
longer-lived of two fields (T1).

**The parked answers.**

| Parked answer | Replaced by |
| --- | --- |
| TQ-24: declared and checked; a generic type follows its arguments; erasure rejected | root types declare it, holders follow fields (5.1); erasure into a local position keeps the reach (8.3) |
| TQ-25: a generic parameter opts in | round 2 reversed it; restored by D2 as `x: ^T` (`ne.generic.default`, owner, 2026-10-14) |
| TQ-26: no NonEscapable returns; later, depend on every parameter | written reaches with elision (4) |

## Appendix B. Five-Language Comparison

Condensed from round 1. The last column is this design.

| Question | Rust | Swift 6.2+ (experimental `Lifetimes`) | Kotlin local lifetimes (note) | Scala capture checking | Mojo origins | hd |
| --- | --- | --- | --- | --- | --- | --- |
| Return a dependent value? | `fn f<'a>(x: &'a T) -> &'a U`, elided with one input | `@_lifetime(copy x)` or `borrow x`; a plain return is an error | `-> Iterator<B>_{this&transform}` | the result names captures: `Writer^{output}` | `-> ref [self] T` | `-> ^x T`; one input or `self` elided; `^()` fresh |
| Store in a struct? | `struct S<'a>`; each field keeps its lifetime | only in a `~Escapable` struct, one lifetime | `local class`, one inferred lifetime | `class C[X^]` or captured at construction | `struct S[origin: Origin]` | holders have one reach; `data S[^c, ^u]` for several |
| Either, both, or one input | both: shared `'a`; one: separate | list the sources | `_{x&y}` or `_{x}` | `^{x, y}` or `^{x}` | `ref [a, b]` or `ref [a]` | `^a+b` or `^a` |
| Closures | borrow by default; `move` owns; `impl Fn() + 'a` | non-escaping closures may capture `~Escapable` | `(A) ->_{x} B` | `() ->{f} Unit` | capture lists; no escaping closures | a closure reaches its captures; a callback parameter is global unless `^fn`, and may then be stored |
| Async | one future carries `'a`; `spawn` needs `'static` | `Task` closures are escaping, so cannot capture | "lexical suspend" sketched | futures capture: `Future[T]^{C}` | no async | bang-driven children need nothing; a stored frame carries its captures |
| Default | borrows opt in; generics accept them | `~Escapable` opts in; generics escapable | `local` opts in | type parameters accept capturing types | `ref` is a separate convention | root types opt in; a parameter is global unless written `^`; generics opt in with `^T` |
| Syntax | `'a`, `<'a>`, `+ 'a` | `~Escapable`, `@_lifetime(...)` | `local x`, `T_{x}` | `T^{x}`, `->{x}` | `ref [o] T`, `origin_of(x)` | `^`, `^a`, `^a+b`, `^()`, `^static` on results, `[^c]`, `closing` |

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
  - [Rustonomicon, subtyping and variance](https://doc.rust-lang.org/nomicon/subtyping.html) (`evil_feeder`);
  - [`std::thread::scope`](https://doc.rust-lang.org/std/thread/fn.scope.html);
  - [closure expressions](https://doc.rust-lang.org/reference/expressions/closure-expr.html);
  - [`tokio::spawn`](https://docs.rs/tokio/latest/tokio/task/fn.spawn.html).
- hd: `spec/lang/04-type-system.md` (Variance, Views),
  `spec/lang/06-control-flow.md` (Deferred Cleanup),
  `spec/lang/07-functions.md` (Function Types, Captures),
  `spec/lang/09-traits.md` (Runtime Type Identity),
  `spec/lang/11-requirements-and-suspension.md` (Requirement Rows,
  Provider Values, Suspending Functions, Cancellation),
  `spec/lang/14-annotations.md` (Handles), `spec/std/net.md`,
  `spec/std/task.md`, `crates/hd_types/src/pool.rs`, and
  `crates/hd_check/src/rows.rs`.
