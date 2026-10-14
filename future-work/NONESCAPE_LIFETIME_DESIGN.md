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
- [12. Checker Cost And Files](#12-checker-cost-and-files)
- [13. Stress-Test Suite](#13-stress-test-suite)
- [14. Owner Questions](#14-owner-questions)
- [Appendix A. Why A Category And Lifetimes](#appendix-a-why-a-category-and-lifetimes)
- [Appendix B. Five-Language Comparison](#appendix-b-five-language-comparison)
- [Sources](#sources)

## Status

This is the design record for issue #163. It decides nothing, and the
spec is frozen. The owner and the orchestrator settled the direction on
2026-10-14 (round 2). This revision turns that direction into numbered
rules, stress-tests it, and lists the questions that remain.

Every spelling is a placeholder: `NonEscapable`, `^`, `^()`, `^static`,
`closing`, the `ne.` rule IDs, and the diagnostic codes in
[Diagnostics](#diagnostics). No compiler checks these examples. A line
marked `# error:` gives the diagnostic the proposed checker would report.

**Who wrote what.** The round-2 direction comes from the owner and the
orchestrator: the category, roots, free handles, "outlives" as liveness,
flow-sensitive locals, the notation, the elision table, and the three
`closing` bullets. Every rule that ends in **(agent)** was written by the
agent in this revision and needs the owner's review. The table lists the
larger ones.

| Agent choice | Rules | Forced by |
| --- | --- | --- |
| A pin uses the binding's value at block exit | `ne.pin.at-exit` | [X3](#x3-a-defer-reads-the-binding-at-exit) |
| A suite registered before the pin runs after it | `ne.pin.defer-order` | [X2](#x2-defer-order) |
| A root that escapes can never be pinned | `ne.pin.escaped` | [X1](#x1-a-late-pin) |
| Ownership is per root, not per binding | `ne.close.owned` | [X5](#x5-closing-a-borrowed-handle-through-a-local) |
| A closure never closes a capture | `ne.close.closure` | [X4](#x4-a-cleanup-closure) |
| A closing call in a `defer` must not suspend | `ne.close.defer-sync` | [X7](#x7-a-suspending-close-in-a-defer) |
| Captured bindings have one reach | `ne.local.captured` | `fn.capture.storage.shared` |
| Root types versus holders; implicit slots | `ne.data.*` | [R3](#r3-importantexcerpt), [S5](#s5-conditional-escapable) |
| Slot variance computed from the declaration | `ne.var.*` | [T2](#t2-swapping-slots) |
| Downcast rule for invariant slots | `ne.erase.downcast-invariant` | [X6](#x6-widening-through-a-downcast) |
| Union default inside function types | `ne.fntype.result` | function types have no parameter names |
| Key roots `^Pool`, top-level `^static`, handle paths | `ne.provider.*`, `ne.top.*`, `ne.handle.*` | [D4](#d4-a-nonescapable-provider), [D8](#d8-a-callback-registry) |
| The over-tie warning | `ne.elide.lint` | [R9](#r9-accept) |

The owner's standing decisions are unchanged: keep `defer`; no Drop,
`using`, or RAII; garbage collection with no moves; `T` and `mut T`
views; structured concurrency; and a declaration's signature is never
inferred from its body.

## 1. Owner Requirements

These are binding (owner, 2026-10-14):

1. NonEscapable is a category of types.
2. Lifetimes are part of the semantics and appear in signatures.
3. A result or a store may depend on any subset of the inputs, tracked
   exactly. One "every input" or `local` rule is too strict.
4. No false alarms. A default that is stricter than needed is a bug.
   Where a source is ambiguous, an annotation is required; the checker
   never guesses conservatively.
5. A struct may hold several handles with independent lifetimes, so data
   declarations have lifetime slots.
6. The notation is terse: no `local x` and no `T from x`.

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

```hd
fn serve!(l: Listener) -> void:
    c := l.accept!()          # a new root, owned by serve!
    defer: c.close()          # pins that root to the body block
    c.send!([1])              # fine: used inside the block
```

### Roots

1. r[ne.root.kinds] A **root** is one of: `static`; a **parameter root** for each own slot of each parameter; a **key root** for each NonEscapable requirement key and row parameter of the declaration; a **type root** for each type parameter; a **local root** for each root site in the body.
2. r[ne.root.site] A **root site** is a call whose result has the reach `^()`, or a construction of a root type ([5.1](#51-the-category)). Each evaluation creates a new handle, and all evaluations of one site share its local root.
3. r[ne.root.param-live] A parameter root, key root, or type root is live for the whole body.
4. r[ne.root.static-outlives] `static` outlives every block. A reach that contains only `static` fits any bound. (agent)
5. r[ne.root.reach] The **reach** of a value is the set of roots it reaches. A value's reach includes the reach of every value stored in it or captured by it.
6. r[ne.root.free] A local root that no `defer` pins is **free**. Its uses alone constrain it, so a helper may open a handle and return it.

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
3. r[ne.heap.object] Each object has one reach per slot of its type, for the object's whole life. Every alias shares it.
4. r[ne.heap.site] An object created in the body gets slot variables at its creation site. A parameter's object has the slots its declared type gives.
5. r[ne.heap.store] Storing a value into a field, element, or capture adds the value's reach to the matching slot of the holder.
6. r[ne.heap.fixed] Storing into an object whose slots are fixed, such as a parameter's object, requires the value's reach to fit the fixed slot. Error: `lifetime-mismatch`.

> **Why.** hd has shared mutable aliases and no exclusivity. A slot that
> changed with the flow of one alias would be wrong for the others, so
> heap slots are flow-insensitive and invariant under `mut`. Bindings are
> not shared, so they can follow the flow, as Rust's and Swift's locals do.

## 3. Notation

| Form | Meaning |
| --- | --- |
| `^a T` | A `T` that reaches at most what `a` reaches. `a` names a parameter, `self`, a slot path such as `s.u`, a declared slot, a requirement key, or `static`. |
| `^a+b T` | Reaches at most what `a` or `b` reaches: usable while both live. The union mirrors `$ A + B`. |
| `^() T` | Reaches no input: fresh. It mirrors the empty row `$()`. On a result, the caller gets a new local root. |
| `^static T` | Reaches only `static`. On a parameter, the callee may store the value anywhere. |
| `data S[^c, ^u]` | Declares two lifetime slots. |
| `Session[^a, ^b]` | Applies a slot-bearing type to reaches. |
| `closing c: T`, `closing self` | The callee may close this parameter's handle. |
| `trait K < NonEscapable`, `impl NonEscapable for S` | Marks a root type. |

```text
lifetime   = "^" reach
reach      = "()" | atom { "+" atom }
atom       = "static" | name { "." name }
slot_param = "^" name            (in a data or enum generic list)
```

1. r[ne.syntax.prefix] A lifetime is a prefix of the type it bounds, as in Rust's `&'a T`. It comes before `mut`, as in `^a mut Conn`.
2. r[ne.syntax.deep] `^S T` bounds every own slot of `T`. For a type with no own slot, it bounds the own slots of its type arguments that carry no `^` of their own, recursively. So `^c Result[Conn, NetError]` is `Result[^c Conn, NetError]`.
3. r[ne.syntax.fn-own] On a function type, `^S` bounds the function value's own slot only, never its inputs or result. (agent)
4. r[ne.syntax.names] A signature names parameters, slot paths, keys, and `static`, never lifetime variables. Only `data` and `enum` declarations declare slots.
5. r[ne.syntax.slot-args] Slot arguments come before type arguments, as in `Registry[^p]` or `Pair[^a, ^b, T]`.

> **Why.** A suffix is ambiguous on function types: `fn!() -> void ^static`
> reads as a lifetime on `void`. Naming parameters instead of variables is
> Mojo's `origin_of(x)` and Swift's `@lifetime(copy x)`.

## 4. Signatures And Elision

A signature states, for each result and each store, which inputs it may
reach. Elision fills the common cases.

```hd
fn excerpt(c: Conn) -> Excerpt          # elided: ^c Excerpt
fn longer(a: Conn, b: Conn) -> ^a+b Conn
fn adopt(mut self, c: ^self Conn) -> void   # self takes on c's roots
fn accept!(self) -> ^() Conn
fn register(job: ^static fn!() -> void) -> void
```

### 4.1 Lifetime-Carrying Inputs

1. r[ne.sig.own-slot] A type has an **own slot** when it is a root type, a data or enum type with slots, a function type, `Suspend[T]`, or a `dyn` type.
2. r[ne.sig.input] A **lifetime-carrying input** is a parameter whose type has an own slot or mentions a type parameter, a NonEscapable requirement key in the row, or a row parameter.
3. r[ne.sig.generic-free] A result type whose only lifetimes come through type parameters needs no elision. `push`, `get`, `map`, and `filter` write nothing.

### 4.2 Elision

| Position | Without `^` |
| --- | --- |
| A parameter's own slot | That parameter's root, named by the parameter. |
| A result own slot, no lifetime-carrying input | `^()`: fresh. |
| A result own slot, exactly one lifetime-carrying input | That input. |
| A method result own slot, lifetime-carrying `self` | `self`. |
| Any other result own slot | An error at the declaration. |
| A store of one parameter into another | An error at the store, unless the stored parameter's type says so. |

1. r[ne.elide.param] A parameter's own slots without `^` take that parameter's roots, named `p` or by slot path `p.c`.
2. r[ne.elide.none] A result own slot without `^`, in a declaration with no lifetime-carrying input, is `^()`.
3. r[ne.elide.one] With exactly one lifetime-carrying input, it is that input.
4. r[ne.elide.self] In a method whose `self` is lifetime-carrying, it is `self`.
5. r[ne.elide.ambiguous] Otherwise the declaration must write `^`. Error: `ambiguous-lifetime`.
6. r[ne.elide.store] A body that stores one parameter's value into another parameter's object is an error unless the stored parameter's type names that object, as `c: ^self Conn` does. Error: `lifetime-mismatch`.
7. r[ne.elide.store-meaning] `c: ^p T` on a parameter means the caller's `p` takes on `c`'s roots, as Swift's `@lifetime(p: copy c)` does.
8. r[ne.elide.lint] A result elided by `ne.elide.one` or `ne.elide.self` whose body never makes it reach that input gets a warning that suggests `^()`. Warning: `overtied-result`. (agent)

> **Why.** Elision picks the only possible source and never a union.
> When there are several sources it asks, because a wrong guess is either
> unsound or a false alarm. The self rule and the one-input rule are
> Rust's. The warning catches the false alarm they can cause, as in
> [R9](#r9-accept).

### 4.3 Call Sites

1. r[ne.call.result] At a call, a result bounded by `^a+b` reaches the reach of the arguments for `a` and `b`, and of their named slots.
2. r[ne.call.fresh] A result bounded by `^()` reaches a new local root of the caller, created at the call.
3. r[ne.call.static] A result bounded by `^static` reaches `static`.
4. r[ne.call.store] An argument for `c: ^p T` adds its reach to the slots of the argument object for `p`, by `ne.heap.store` or `ne.heap.fixed`.
5. r[ne.call.static-param] An argument for a `^static` parameter makes every local root in its reach escape. A parameter root or key root in its reach is an error. Error: `lifetime-mismatch`.

## 5. Data Types And Slots

### 5.1 The Category

1. r[ne.data.root-type] A **root type** is a trait that has `NonEscapable` as a direct or transitive supertrait, or a data or enum type with `impl NonEscapable for S`. Every value of a root type starts as its own root.
2. r[ne.data.root-impl] A data type that implements a root trait must declare `impl NonEscapable` too, by the ordinary supertrait rule (`trait.super.missing`).
3. r[ne.data.holder] A **lifetime-carrying type** is a root type, a type with a field, payload, or type argument that is lifetime-carrying, a function type, `Suspend[T]`, or any `dyn` type.
4. r[ne.data.implicit] A data or enum type with a lifetime-carrying field and no declared slots has one implicit slot. Every lifetime-carrying field without `^` shares it, and it needs no declaration. (agent)
5. r[ne.data.generic] A generic data type is lifetime-carrying exactly for the instantiations whose arguments are. `Box[i32]` carries nothing, and `Box[Conn]` carries `Conn`'s slot through `T`.

> **Why.** The category matters where values start, so only root types
> declare it. Holders follow their fields, as Rust's auto traits do; this
> replaces the parked TQ-24 rule, which followed type arguments and so
> missed a `Suspend[void]` that holds a handle.

### 5.2 Declared Slots

```hd
data Session[^c, ^u]:
    client: ^c Conn
    upstream: ^u Conn
```

1. r[ne.data.slots] `data S[^c, ^u]` declares the slots `c` and `u`. Slot names are local to the declaration.
2. r[ne.data.field-slot] In a type with declared slots, every own slot of a field must name declared slots. A field that leaves one unnamed is an error. Error: `undeclared-slot`. (agent)
3. r[ne.data.path] Outside the declaration, a value's slot is named by a path: `^s.c` is slot `c` of parameter `s`, and `^self.c` is slot `c` of the receiver.
4. r[ne.data.whole] A bare `^s` names the union of all of `s`'s slots.
5. r[ne.data.recursive] A recursive occurrence of the type applies it to its own slots or unions of them, as in `next: Ring[^a]?`. A slot not declared there is an error. Error: `undeclared-slot`. (agent)
6. r[ne.data.literal] A data literal infers its slot arguments from its field values at the use site.

### 5.3 Variance Of Slots

1. r[ne.var.subtype] `^S T` is assignable to `^S' T` when `S` is a subset of `S'`, ignoring `static`.
2. r[ne.var.polarity] A declared slot's variance is computed by the `types.polarity` rules over the type's readonly surface, as `+T` is checked. It is not written. (agent)
3. r[ne.var.mut-data] Under a `mut` view, every slot of a data or enum type is invariant, because a field can be reassigned (`types.variance.mut-invariant`).
4. r[ne.var.dyn] The slot of `dyn K` is covariant when no method of `K`, `mut self` methods included, takes a value that reaches `^self`. Otherwise it is invariant, even under a readonly view. (agent)
5. r[ne.var.fn] A function value's own slot and a `Suspend`'s slot are covariant: a function value carries no access permission (`fn.type.no-permission`).
6. r[ne.var.args] A slot inside a type argument takes the argument's declared variance (`types.polarity.generic`).

> **Why.** Rust computes lifetime variance from the declaration. hd
> declares type variance but would need a new marker for slots; the owner
> decides ([Q7](#14-owner-questions)). The rules let a long-lived handle
> enter a short-lived readonly list, and stop a `mut` alias from widening
> a slot that another alias relies on.

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

1. r[ne.close.mode] `closing` is a parameter mode, like Swift's `consuming`. It is written before the parameter name, or as `closing self`.
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

fn relay!(client: Conn) -> void:
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
2. r[ne.body.param-store] A store into a parameter's object must fit that parameter's slot, which `ne.elide.store` extends only as written. Error: `lifetime-mismatch`.

### 7.3 Escapes

1. r[ne.escape.def] A local root **escapes** when its reach flows into the result, into a parameter's object, into a top-level binding, or into a `^static` parameter.
2. r[ne.escape.free] An escaped root is fine while it is free. By `ne.pin.escaped`, it can never be pinned.

### 7.4 What Is Checked

| Check | Rule |
| --- | --- |
| Result and stores fit the signature | `ne.body.result`, `ne.body.param-store` |
| Each pinned root is owned | `ne.pin.param`, `ne.close.owned` |
| No pinned root escapes | `ne.pin.escaped` |
| No value reaching a pinned root is live on the pin block's exits | `ne.outlive.check` |
| No earlier-registered suite of the block reads it | `ne.pin.defer-order` |

## 8. Generics, Function Types, And Erasure

### 8.1 Generic Substitution

1. r[ne.generic.default] A type parameter accepts lifetime-carrying arguments by default, as Rust's `T` does. This reverses the parked TQ-25. (agent)
2. r[ne.generic.carry] Substitution carries an argument's slots unchanged, so `List[Conn]` is `List[^ρ Conn]` for the root `ρ` of each element.
3. r[ne.generic.abstract] In a generic body, a value of type `T` reaches the type root `T`, and nothing about it is known.
4. r[ne.generic.bound] `^S T` on a type-parameter type bounds every slot of `T`'s argument by `S`. `x: ^static T` is how a generic function stores `x` in module state.
5. r[ne.generic.infer] Slot arguments of generic data are inferred at use sites like type arguments. A declaration's slots are never inferred from its body.
6. r[ne.generic.explicit] An explicit type argument may carry `^`, as in `List[^c Conn]` or `keep::[^c Conn](x)`.

### 8.2 Function Types

1. r[ne.fntype.own] A function type has one own slot: the reach of its captures.
2. r[ne.fntype.inputs] Each input's own slots without `^` take the argument's reach at each call.
3. r[ne.fntype.result] A result own slot without `^` reaches the function value's own slot and every lifetime-carrying input. (agent)
4. r[ne.fntype.names] Inside a function type, `^` may name the enclosing declaration's parameters, `static`, or `()`. It cannot name the function type's own inputs, which have no names (`fn.type.no-names`).
5. r[ne.fntype.row] A NonEscapable key in a function type's row is resolved at each call, so the result may reach that call's provider.
6. r[ne.fntype.subtype] Inputs are contravariant, the result and the own slot covariant.

> **Why.** Rule 3 is the one place a union default remains. A function
> type cannot name one of its inputs, as Swift's `@_lifetime(copy ne0)
> (_ ne0: NE, NE) -> NE` can. The owner decides whether to add names or
> require `^` ([Q8](#14-owner-questions)).

### 8.3 Erasure And Downcasting

1. r[ne.erase.slot] Every `dyn K`, including `dyn Any` and `dyn Inspectable`, has one slot.
2. r[ne.erase.join] Erasing a value to `dyn K` joins all its slots into that one slot.
3. r[ne.erase.allowed] Erasure of a lifetime-carrying value is allowed. This replaces the parked TQ-24 "erasure is rejected", so a `dyn Error` may hold a handle.
4. r[ne.erase.downcast] `downcast`, `downcast_mut`, and `downcast_val` return `T` with every slot set to the `dyn` value's slot.
5. r[ne.erase.downcast-invariant] When `T` has a slot that is invariant at the result's view, the `dyn` value's slot must be `static`. Error: `downcast-invariant-slot`. (agent)
6. r[ne.erase.identity] Slots are not part of runtime identity. `TypeId::of::[Session[^a, ^b]]()` equals `TypeId::of::[Session[^c, ^d]]()`.

> **Why.** Rule 5 is Rust's `Any: 'static`, narrowed to the slots that
> can be widened unsoundly ([X6](#x6-widening-through-a-downcast)).
> A covariant slot widened to the union stays sound.

## 9. Closures, Suspensions, And Cancellation

1. r[ne.closure.slot] A closure's own slot is the reach of the bindings it captures. Each captured binding has one reach (`ne.local.captured`).
2. r[ne.suspend.slot] A cold frame's own slot is the reach of its arguments and its construction-time providers (`req.lowering.frame`, `req.bind.captured`).
3. r[ne.suspend.exact] So a plain call `f(a, b)` of a `fn!` has a frame that reaches `a` and `b`. This union is exact: the frame stores them all.
4. r[ne.suspend.bang] A bang call uses its suspension and its arguments only until the call returns.
5. r[ne.suspend.stored] A stored suspension is an ordinary value: its holder reaches its roots, and polling or cancelling it later is a use.
6. r[ne.suspend.cancel-order] Cancelling a parent runs each unfinished child's cleanup before the parent's `defer` suites (`req.cancel.defer`). So a child driven by a bang call never sees its parent's pinned handles closed.
7. r[ne.all.typing] `all!` and `race!` take their children's frames for the call only. The result tuple, or value, carries its own types' slots.
8. r[ne.all.race] `race!` cancels the losers before it returns (`req.combinator.race-losers`), so no loser is live after the call.
9. r[ne.all.list] `all_list!` takes a list of closures. The list is an ordinary value, live after the call when a binding still uses it.

> **Why.** hd has no detached spawn, so structure covers every child
> driven by a bang call. A lifetime is needed only for a suspension that is
> stored, as in [R10](#r10-a-borrow-across-a-suspension) and
> [D2](#d2-a-cold-suspension-in-a-field).

## 10. Providers

1. r[ne.provider.key] A requirement key may be a NonEscapable trait.
2. r[ne.provider.root] In a declaration, each NonEscapable key of its row is a key root, named by the key, as `^Pool`. A row parameter `$R` is named `^R`. (agent)
3. r[ne.provider.use] `$.use(K)` reaches the key root `K`.
4. r[ne.provider.with] Inside `$.with(K=p)`, calls instantiate `^K` with `p`'s reach.
5. r[ne.provider.frame] A frame built inside that scope captures the provider, so its slot includes `p`'s reach.
6. r[ne.provider.closure] A closure captures no provider (`fn.capture.no-providers`). Its row carries the key, and each call resolves it there.
7. r[ne.provider.close] A provider reached only through `$.use` is not owned, so closing it is an error. Error: `closing-not-owned`.
8. r[ne.provider.flow] `req.use.value.flow` and `req.use.value.outlives-scope` stay true, bounded by the provider's reach.

## 11. Top Level, REPL, And Annotation Handles

1. r[ne.top.static] A top-level binding, and a binding made by a REPL input, reaches `static`.
2. r[ne.top.elide] An own slot in a top-level binding's declared type without `^` is `^static`. (agent)
3. r[ne.top.store] A store into a top-level binding's object makes the stored value's local roots escape. A parameter or key root in it is an error. Error: `lifetime-mismatch`.
4. r[ne.top.no-pin] Top level is not a cleanup scope (`flow.defer.not-scopes`), so nothing is pinned there, and a REPL session may keep `c := connect!("db")` across inputs (`cli.repl.declarations.bindings`).
5. r[ne.handle.no-slot] An annotation handle `Field[S, F]` has no slot and may escape (`annot.handle.escape`), since it stores no `S` or `F`.
6. r[ne.handle.get] `h.get(x)` has the type of `x.member` (`annot.handle.get`), including its slot path, as in `^x.c Conn`. (agent)
7. r[ne.handle.build] A value that `build` passes for a member must fit that member's slot in the value being built.

## 12. Checker Cost And Files

### 12.1 What A Signature Exports

- For each parameter: its own slot names and whether it is `closing`.
- For each result and each `c: ^p` store: a reach, as a set of atoms over parameters, slot paths, keys, row parameters, and `static`, or `()`.
- For each data and enum type: its slot list, the slot each field uses, and each slot's computed variance.
- For each trait: whether it is a root trait, and each method's signature as above.

All of it is written, or elided by local rules, so no export depends on a
body.

### 12.2 Per Body

1. **Variables.** One reach variable per slot of each lifetime-carrying
   binding version (SSA), object site, and call result. Most types have
   one slot.
2. **Constraints.** Each store and call adds subset constraints
   (`reach(x) ⊆ slot(o)`), and each invariant position adds an equality.
   A callee signature is instantiated by substituting argument reaches
   for parameter names; `^()` mints a local root.
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

Cost: `O((N + E) · ⌈R/64⌉)` per body, for `N` program points, `E`
constraints, and `R` roots, so linear in practice. There is no global
fixed point: signatures are summaries.

### 12.3 Incremental And Cross-File Effects

- A body's result depends only on its own text and its callees'
  signatures. Editing a body never invalidates another body.
- Slot variance depends on the declaration's readonly surface, as type
  variance already does (`types.variance.surface`). Adding a method that
  stores into a slot can change variance, which is an interface change.
- Interface files carry the exports in 12.1. A change to a reach in a
  public signature is an API change, like a change to a row.

### 12.4 What It Reuses

| Need | Existing structure |
| --- | --- |
| Reach as a set with `+` and `()` | Requirement rows: `RowData { keys, params }` in `crates/hd_types/src/pool.rs`; union and membership entailment (`req.row.set.union`, `req.row.entail.membership`) |
| Instantiating a reach at a call | Row fitting in `crates/hd_check/src/rows.rs` (`fit_row`, `solve_exact_row`, `collect_infer`); the least solution of `req.row.least.solution` |
| A second parameter kind | The kind check behind `generic-kind-mismatch` and the `$` marker (`req.row.param.marked`) |
| Slot variance | The `types.polarity` check |
| Liveness | None in `hd_check` today. `hd_opt` has passes for code generation, but the checker needs its own. |

Rows forbid parameters on data (`req.row.param.no-data`). Slots must not
copy that rule: lifetime slots on `data` are required (owner requirement
5). So slot parameters are a new kind, beside type-kinded and row-kinded
parameters, and the type pool needs slot arguments on data types.

### 12.5 Annotation Burden In std (Estimate)

This is an estimate from the signatures in `spec/std`, not a recount:

| Change | Count |
| --- | --- |
| `< NonEscapable` on `TcpStream`, `TcpListener`, `UdpSocket` | 3 |
| `close(closing self)` on those traits | 3 |
| `^()` on `accept!`, and on `Net`'s `connect!`, `listen!`, `bind_udp!` | 4 |
| Collection, iterator, option, result, and task signatures | 0 |

The `Net` methods need `^()` because the self rule would tie each new
handle to the `Net` provider, whose reach is `static`, and a `static`
handle cannot be pinned. Collection and iterator methods need nothing,
because elements carry their own slots and callbacks are function values.

## 13. Stress-Test Suite

The suite uses this prelude. It drops `Result` handling for brevity; the
real `std.net` methods return `Result` and take `mut self`.

```hd
trait NonEscapable                # placeholder std marker

trait Socket < NonEscapable:
    fn send!(self, bytes: List[u8]) -> void
    fn read!(self) -> string
    fn sent(self) -> usize
    fn close(closing self) -> void

trait Acceptor < NonEscapable:
    fn accept!(self) -> ^() Conn
    fn close(closing self) -> void

type Conn = dyn Socket
type Listener = dyn Acceptor

fn connect!(host: string) -> Conn      # no lifetime-carrying input: ^()
fn listen!(host: string) -> Listener
```

### 13.1 Verdicts

| # | Source | Verdict | Change forced |
| --- | --- | --- | --- |
| R1 | Rust book `longest` | Works | none |
| R2 | Rust book dangling reference | Works | none |
| R3 | Rust `ImportantExcerpt` | Works, nothing written | implicit slot (`ne.data.implicit`) |
| R4 | Rust `Parser<'c, 's>` | Works with one lifetime | none |
| R5 | Rust slice iterator | Works | none |
| R6 | Rustonomicon `evil_feeder` | Works; loop form rejected, as in Rust | example corrected |
| R7 | Rust `thread::spawn` | Works | prefix notation |
| R8 | Rust reassignment | Works | none |
| R9 | Rust `TcpListener::accept` | Needs `^()` | `^()`; `overtied-result` warning |
| R10 | Rust borrow across `.await` | Works | none |
| R11 | Rust `std::thread::scope` | Works | none |
| R12 | Rust lifetime-generic method | Works with one path | slot paths on methods |
| R13 | Rust `HashMap` of handles | Works; mixed map rejected, as in Rust | known limit L2 |
| R14 | Rust `Option<&T>` with `?` | Works, nothing written | none |
| R15 | Rust self-referential struct | Works, unlike Rust | none |
| S1 | Swift `Span` | Works | none |
| S2 | Swift `@lifetime(self: copy other)` | Works | none |
| S3 | Swift `consuming` close wrapper | Hole in round 1 | `closing` mode |
| S4 | Swift `MutableSpan`, `@_lifetime(&self)` | Works, no exclusivity | none |
| S5 | Swift conditional `Escapable` | Works, nothing written | holders follow fields |
| S6 | Swift non-escaping closure parameter | Works | none |
| M1 | Mojo `ref [a, b]` | Works | none |
| M2 | Mojo struct origin parameter | Works | none |
| M3 | Mojo capturing versus escaping closures | Works | none |
| T1 | Two independent slots | Works | none |
| T2 | Swapping two slots | Rejected, as in Rust | none |
| T3 | REPL top-level handle | Works | static roots |
| D1 | A handle in a `Result` through `?` | Rejected correctly | `ne.outlive.return` |
| D2 | A cold `Suspend` in a field | Rejected; strict | known limit L1 |
| D3 | `race!` and cancellation with a `defer` | Works | `ne.suspend.cancel-order` |
| D4 | A NonEscapable provider | Works | key roots `^Pool` |
| D5 | `Map[string, Conn]` in a slot | Works | none |
| D6 | `dyn Conn` list mixing lifetimes | Works | `ne.erase.join` |
| D7 | Recursive data of handles | Works | `ne.data.recursive` |
| D8 | Callback registry | Works | `^static`, top-level elision |
| X1 | A late pin | Hole | `ne.pin.escaped` |
| X2 | `defer` order | Hole | `ne.pin.defer-order` |
| X3 | A `defer` reads the binding at exit | Hole and false alarm | `ne.pin.at-exit` |
| X4 | A cleanup closure | Hole | `ne.close.closure` |
| X5 | Closing a borrowed handle through a local | Hole | `ne.close.owned` |
| X6 | Widening through a downcast | Hole | `ne.erase.downcast-invariant` |
| X7 | A suspending close in a `defer` | Hole | `ne.close.defer-sync` |

### 13.2 Rust

#### R1. `longest`

Rust: `fn longest<'a>(x: &'a str, y: &'a str) -> &'a str`.

```hd
fn longer(a: Conn, b: Conn) -> ^a+b Conn:
    if a.sent() > b.sent(): a else: b

fn pick!(l: Listener, keep: Conn) -> ^keep Conn:   # two inputs: written
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
    conn: Conn                # one implicit slot

fn excerpt(c: Conn) -> Excerpt:     # elided: ^c Excerpt
    Excerpt { conn: c }
```

Round 2 wrote `data Excerpt < NonEscapable:`, which is not hd syntax
(data types take no supertrait list). A holder needs no declaration
(`ne.data.implicit`).

#### R4. `Parser<'c, 's>`

Rust needs two lifetimes because the parser borrows a temporary context.

```hd
data Router:
    pool: List[Conn]

impl Router:
    fn pick(self) -> Conn:            # elided: ^self
        self.pool[0]

fn route(pool: List[Conn]) -> Conn:   # elided: ^pool
    Router { pool: pool }.pick()      # reaches pool's roots: fine
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
    fn next(mut self) -> Conn?:       # elided: ^self, the conns it holds
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

fn feed[T](slot: mut Box[T], v: T) -> void:
    slot.value = v

fn main!(hosts: List[string]) -> void:
    keep := Box { value: connect!("a") }
    if hosts.len() > 0:
        tmp := connect!(hosts[0])
        defer: tmp.close()
        feed(keep, tmp)               # keep's slot now reaches tmp (invariant under mut)
    keep.value.send!([1])             # error: handle-outlives-cleanup
```

Without the last line the program is accepted: `keep` is not live after
the `if` block. Round 2 used a loop and claimed Rust accepts it without
the final use. It does not: the next iteration's `feed(keep, tmp)`
passes `keep`, which may read the previous `tmp`, and Rust reports
"borrow later used here, in later iteration of loop". hd rejects the loop
form for the same reason (`ne.outlive.loop`). It is a true alarm at the
signature level, because `feed` may read `slot` before writing it.

#### R7. `thread::spawn` Versus Structured Children

```hd
use std.task.all

fn pump!(from: Conn, to: Conn) -> void:
    to.send!([0])

fn mirror!(a: Conn, b: Conn) -> void:
    _ := all!(pump(a, b), pump(b, a))    # fine: both frames finish inside the call

let JOBS: mut List[fn!() -> void] = []   # elided: elements are ^static

fn later(c: Conn) -> void:
    JOBS.push(fn!(): c.send!([1]))       # error: lifetime-mismatch: the closure reaches c

fn register(job: ^static fn!() -> void) -> void:
    JOBS.push(job)                       # fine, like Swift's @escaping
```

#### R8. Reassignment

```hd
fn best(a: Conn, b: Conn) -> ^a Conn:
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
A trait method has no body, so its author must write `^()`; std needs it
four times ([12.5](#125-annotation-burden-in-std-estimate)).

#### R10. A Borrow Across A Suspension

Rust: an `async fn` holds `&TcpStream` across `.await`, and
`tokio::spawn` needs `'static`.

```hd
let QUEUE: mut List[mut Suspend[void]] = []

fn echo!(c: Conn) -> void:
    _ := c.read!()            # the frame holds c across both bang calls
    c.send!([1])

fn serve!(l: Listener) -> void:
    c := l.accept!()
    defer: c.close()
    echo!(c)                  # fine: the bang call returns before the defer runs
    QUEUE.push(echo(c))       # error: lifetime-mismatch: QUEUE needs ^static
```

No change. `QUEUE` is hd's `tokio::spawn`: storing a frame in module
state needs `^static`, as spawning needs `'static`.

#### R11. `std::thread::scope`

```hd
use std.task.all_list

fn broadcast!(c: Conn, frames: List[List[u8]]) -> void:
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

#### R12. A Lifetime-Generic Method

Rust: `impl<'a> Parser<'a> { fn part<'b>(&self, key: &'b str) -> &'a str }`.

```hd
impl Session:
    fn route(self, hint: Conn) -> ^self.u Conn:   # neither all of self nor hint
        self.upstream
```

Elided, the result would be `^self`, the union of both slots, and a
caller who keeps the result past the client's block would get a false
alarm. One slot path fixes it. No change: slot paths on methods already
exist (`ne.data.path`).

#### R13. A `HashMap` Of Handles

```hd
fn mixed!(long: Conn, extra: bool) -> void:
    let by_name: mut Map[string, Conn] = {}
    by_name["long"] = long
    if extra:
        tmp := connect!("x")
        defer: tmp.close()
        by_name["tmp"] = tmp
    by_name["long"].send!([1])    # error: handle-outlives-cleanup: by_name holds tmp
```

Rust rejects the same program: a `HashMap<&str, &'a S>` has one `'a`.
The alarm is true at the type level, because the map does hold a closed
handle. Removing `"tmp"` before the block ends does not help, because a
heap slot is one reach for the object's life (known limit L2). The
workaround is a map scoped to the inner block.

#### R14. `Option<&T>` With `?`

```hd
fn first_open(pool: List[Conn]) -> Conn?:     # elided: ^pool
    c := pool.get(0)?
    if c.sent() == 0: return .None
    .Some(c)
```

Nothing is written: `get` returns `T?`, and `T` carries the slot.

#### R15. A Self-Referential Struct

Rust cannot let a field borrow a sibling field.

```hd
data Lines:
    conn: Conn

data Reader:
    conn: Conn
    lines: Lines              # a view over conn: the same root

fn reader(c: Conn) -> Reader:         # elided: ^c
    Reader { conn: c, lines: Lines { conn: c } }
```

It works. A lifetime is a set of roots, not a memory region, and hd has
no moves, so nothing invalidates the view.

### 13.3 Swift

#### S1. A View Over A Handle

Swift: `Span` with `@_lifetime(borrow self)`.

```hd
impl Lines:
    fn next!(self) -> string: self.conn.read!()

fn lines(c: Conn) -> Lines:           # elided: ^c
    Lines { conn: c }

fn first_line!(l: Listener) -> string:
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
    fn adopt(mut self, c: ^self Conn) -> void:
        self.conns.push(c)
```

`List[Conn]` needs nothing written, because the element type carries the
slot. Swift does not yet allow an `Array` of `~Escapable` elements.

#### S3. A Cleanup Wrapper

Swift: `consuming func close()`; Rust: `drop(self)`.

```hd
impl Session:
    fn shutdown(closing self) -> void:
        self.client.close()
        self.upstream.close()

fn proxy!(l: Listener) -> Conn:
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
    fn flush!(mut self) -> void:
        self.conn.send!(self.buffer)
        self.buffer = []

fn writer(c: Conn) -> Writer:         # elided: ^c
    Writer { conn: c, buffer: [] }
```

It works with nothing written. Two writers over one `c` may coexist: hd
promises no exclusivity, only that neither outlives `c`'s cleanup.
Swift's `&` scope buys exclusivity, which hd does not want.

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

fn remember_conn(t: Tagged[Conn]) -> void:
    SEEN_CONNS.push(t)                # error: lifetime-mismatch: needs t: ^static
```

Nothing is written on `Tagged`: holders follow their arguments
(`ne.data.generic`).

#### S6. A Non-Escaping Closure Called Later In The Call

```hd
fn first_ok!(n: usize, attempt: fn!() -> bool) -> bool:
    let tries: mut List[fn!() -> bool] = []
    for _ in 0..n:
        tries.push(attempt)           # a local list: dead before the call returns
    for t in tries:
        if t!(): return true
    false
```

It works: `tries` is not live after the body. Storing `attempt` in
`self` would need `attempt: ^self fn!() -> bool`. Swift does not let a
non-escaping closure enter an array at all; hd's liveness rule is more
permissive.

### 13.4 Mojo

#### M1, M2. Origins

`fn pick(a, b) -> ref [a, b] String` is `-> ^a+b Conn`.
`struct Span[T, origin: Origin]` is `data Window[^src]`, and a
`ref [self]` result is the elided `^self`.

#### M3. Capturing Versus Escaping Closures

Mojo's reference captures (`{imm x}`, `{mut x}`) cannot outlive their
scope. A copy capture (`{var x}`, `@__copy_capture`) detaches the closure
from the original, so it may escape.

```hd
fn counter(c: Conn) -> fn() -> usize:     # elided: ^c
    fn() -> usize: c.sent()

fn tally(n: usize) -> fn() -> usize:      # elided: ^(), nothing carries a root
    fn() -> usize: n
```

It works with one rule. Copying a handle does not detach it from its
root, so hd needs no copy-capture escape: a closure reaches what it
captures, by value or by binding.

### 13.5 Two Slots And The REPL

#### T1. Independent Slots

```hd
fn client_of(s: Session) -> ^s.c Conn:
    s.client

fn proxy!(l: Listener) -> ^() Conn:
    keep := l.accept!()
    up := connect!("db")
    defer: up.close()
    client_of(Session { client: keep, upstream: up })   # fine: only keep's root
```

#### T2. Swapping Slots

```hd
fn swap_in(s: mut Session) -> void:
    s.client = s.upstream     # error: lifetime-mismatch: ^s.u does not fit ^s.c
```

As in Rust, two distinct slots never take each other's values.

#### T3. A REPL Handle

A REPL input `c := connect!("db")` binds a static root. Later inputs may
use it. A `defer` that closes it is an error (`ne.pin.static`); an early
`c.close()` is fine.

### 13.6 hd-Specific

#### D1. A Handle In A `Result` Through `?`

```hd
enum LinkError:
    Broken(conn: Conn)                # the error holds the failed handle

fn check!(c: Conn) -> Result[void, LinkError]:    # elided: the error reaches c
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
(`ne.outlive.return`). Since `LinkError` erases to `dyn Error` with its
slot, and the slot is covariant, `downcast` stays available
(`ne.erase.downcast`).

#### D2. A Cold Suspension In A Field

```hd
data Worker:
    next: mut Suspend[void]?

fn arm!(w: mut Worker) -> void:
    c := connect!("db")
    defer: c.close()
    w.next = .Some(echo(c))   # error: lifetime-mismatch: w's slot is the caller's

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
slot still reaches `c`, because a heap slot is one reach for the object's
life (known limit L1). Rust rejects the same program: a struct's
lifetime parameter covers its whole life. The smallest fix would be a
strong update for an object no other binding can reach, which Rust's
borrow checker does for locals but not for fields ([Q12](#14-owner-questions)).
The workaround is to keep the frame in a binding inside the block.

#### D3. `race!` And Cancellation With A `defer`

```hd
use std.task.race

fn fastest!(a: Conn, b: Conn) -> string:
    race!(a.read(), b.read())         # the loser is cancelled before race! returns

fn ask!(l: Listener, backup: Conn) -> string:
    c := l.accept!()
    defer: c.close()
    fastest!(c, backup)               # fine, even if ask! is cancelled mid-wait
```

If `ask!` is cancelled inside `race!`, both children finish their
cancellation cleanup before `c.close()` runs (`req.cancel.defer`). No
lifetime check is needed. [X2](#x2-defer-order) shows the one ordering
case that does need a check.

#### D4. A NonEscapable Provider

```hd
trait Pool < NonEscapable:
    fn get(self) -> Conn              # elided: ^self
    fn close(closing self) -> void

fn grab() -> Conn $ Pool:             # elided: ^Pool, the only lifetime-carrying input
    $.use(Pool).get()

fn open_pool!() -> dyn Pool

let KEEP: mut List[Conn] = []

fn serve!() -> void:
    p := open_pool!()
    defer: p.close()
    c := $.with(Pool=p):
        grab()                        # c reaches p
    c.send!([1])                      # fine
    KEEP.push(c)                      # error: lifetime-mismatch: KEEP needs ^static
```

The key is a lifetime-carrying input named by the key
(`ne.provider.root`). `$.use(Pool).close()` inside `grab` would be
`closing-not-owned`.

#### D5. `Map[string, Conn]` In A Slot

```hd
data Registry[^p]:
    by_name: mut Map[string, ^p Conn]

impl Registry:
    fn add(mut self, name: string, c: ^self.p Conn) -> void:
        self.by_name[name] = c

    fn find(self, name: string) -> ^self.p Conn?:
        self.by_name.get(name)
```

It works. The value lifetime is one slot for every value, as in Rust.
With one slot the declaration may drop `[^p]` and the paths: the
implicit slot and `c: ^self Conn` say the same.

#### D6. A `dyn Conn` List Mixing Lifetimes

```hd
data Fake: pass

impl NonEscapable for Fake

impl Socket for Fake:
    fn send!(self, bytes: List[u8]) -> void: pass
    fn read!(self) -> string: ""
    fn sent(self) -> usize: 0
    fn close(closing self) -> void: pass

fn mix!(real: Conn) -> void:
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
    Link(conn: Conn, next: Chain)     # one implicit slot for the whole chain

data Ring[^a]:
    conn: ^a Conn
    next: Ring[^a]?                   # a recursive occurrence passes its own slot
```

It works (`ne.data.recursive`).

#### D8. A Callback Registry

```hd
let HANDLERS: mut List[fn(string) -> void] = []    # elided: ^static elements

fn on_message(h: ^static fn(string) -> void) -> void:
    HANDLERS.push(h)

fn log_line(m: string) -> void: pass

fn setup!(l: Listener) -> void:
    c := l.accept!()
    on_message(log_line)                  # fine
    on_message(fn(m): _ := c.sent())      # fine: c's root is free, so it escapes
```

It works. `^static` is the registry's whole annotation. Top-level slots
elide to `^static` (`ne.top.elide`).

### 13.7 Holes Found

#### X1. A Late Pin

```hd
fn setup!(l: Listener) -> void:
    c := l.accept!()
    on_message(fn(m): _ := c.sent())  # c escapes into HANDLERS
    defer: c.close()                  # error: pin-escaped-root
```

Round 2 checked pins against later uses only. Here the escape comes
first, and the closure in `HANDLERS` outlives `c`'s close. Fix: a root
that escapes can never be pinned, in any order (`ne.pin.escaped`).
Borrows: Rust checks borrows against all uses of a region, not only the
later ones.

#### X2. Defer Order

```hd
fn audit!(l: Listener, log: mut List[string]) -> void:
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
fn relay!(client: Conn) -> void:
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
    items: mut List[Conn]             # beneath mut: the slot is invariant

fn smuggle(s: mut Stash, tmp: Conn) -> void:
    let e: mut dyn Inspectable = s
    let wide: ^s+tmp mut dyn Inspectable = e     # covariant: Inspectable takes no lifetime input
    match wide.downcast_mut::[Stash]():          # error: downcast-invariant-slot
        .Some(back) => back.items.push(tmp)      # would put tmp into the caller's list
        .None => pass
```

Round 1 said `downcast` returns the `dyn` value's slot. Widening the
`dyn` slot first and downcasting then widens an invariant slot. Fix:
downcasting to a type with an invariant slot needs a `static` slot
(`ne.erase.downcast-invariant`). Borrows: Rust's `Any: 'static`, kept
only where it is needed.

#### X7. A Suspending Close In A `defer`

```hd
impl Session:
    fn drain!(closing self) -> void:
        self.client.send!([0])
        self.shutdown()

fn run!(l: Listener) -> void:
    s := Session { client: l.accept!(), upstream: connect!("db") }
    defer: _ := s.drain()     # error: closing-suspension-in-defer
```

The plain call builds a cold frame and discards it, so nothing is
closed, yet the call would pin. Fix: a closing call in a `defer` must not
suspend (`ne.close.defer-sync`). Borrows: nothing; it follows from
`flow.defer.restricted`.

### 13.8 Known Limits

| # | Limit | Example | Rust |
| --- | --- | --- | --- |
| L1 | A heap slot keeps a root after the value is overwritten | D2 | same |
| L2 | A container's values share one slot | R13 | same |
| L3 | A function type's result is the union of its inputs and captures | `ne.fntype.result` | Rust names inputs with `for<'a>` |
| L4 | A loop-carried mutable target reaches every iteration's roots | R6 loop form | same |

L1 and L2 are false alarms in the strict sense, though the alarm is
true at the signature level: the object does hold, or did hold, a closed
handle. L3 is a union default, the only one left. Each is listed for the
owner ([Q8](#14-owner-questions), [Q12](#14-owner-questions)).

### Diagnostics

| Code | When |
| --- | --- |
| `handle-outlives-cleanup` | A value reaching a pinned root is live after the pin's block, or read by an earlier-registered suite. |
| `lifetime-mismatch` | A value's reach does not fit the bound it flows into. |
| `ambiguous-lifetime` | Elision has no single answer, so `^` must be written. |
| `closing-not-owned` | A closing call reaches a root the function does not own. |
| `closing-in-closure` | A closure closes a captured value. |
| `closing-suspension-in-defer` | A `defer` makes a closing call that would only build a frame. |
| `pin-escaped-root` | A pinned root also escapes. |
| `pin-static-root` | A `defer` closes a static value. |
| `undeclared-slot` | A field or recursive occurrence names no declared slot. |
| `downcast-invariant-slot` | A downcast would widen an invariant slot. |
| `overtied-result` (warning) | An elided result is tied to an input the body never returns. |

## 14. Owner Questions

They are ordered core first, spellings last. Questions already settled
in round 2 are not repeated.

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
     `impl NonEscapable for S`); holders get an implicit slot;
   - (b) every holder declares it too, as the parked TQ-24 said.

   **Recommendation: (a)** (agent). Under (b), `Iterator`, every data
   type with a callback field, and every `Suspend` holder need a
   declaration, because a function type carries a slot.

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

5. **The self rule.** Keep round 2's elision of a method result to
   `self`?
   Options:
   - (a) keep it, with the `overtied-result` warning;
   - (b) drop it: a method with another lifetime-carrying input must
     write `^`.

   **Recommendation: (a).** It is Rust's rule, and the warning reports
   each false alarm it causes to the author. (b) follows requirement 4
   more strictly but adds `^self` to most methods that take a callback.

6. **Generic defaults.** Do type parameters accept lifetime-carrying
   arguments by default (`ne.generic.default`), retiring TQ-24 to TQ-26?
   **Recommendation: yes** (agent, from round 1). Opt-in would mark
   every callback API in std. TQ-24's argument rule and erasure ban, and
   TQ-26's "no NonEscapable returns", are replaced by sections 5, 8.3,
   and 4.

7. **Slot variance.** Options:
   - (a) computed from the declaration surface, as Rust does
     (`ne.var.polarity`);
   - (b) declared with a marker, as hd's `+T` is, unmarked meaning
     invariant.

   **Recommendation: (a)** (agent). Under (b) an unmarked slot is
   stricter than needed, which requirement 4 forbids. Adding a storing
   method then changes variance, an interface change hd already accepts
   for inherent methods (`types.variance.surface`).

8. **Function-type results.** Options:
   - (a) the union default (`ne.fntype.result`), tightened with `^()` or
     enclosing parameter names;
   - (b) require `^` when a function type has a lifetime-carrying input
     and an own-slot result;
   - (c) allow names in function types, as Swift does.

   **Recommendation: (b)** (agent). It is the only rule here that would
   otherwise guess. Such types are rare in std; (c) changes
   `fn.type.no-names` for little gain.

9. **Downcasts.** Adopt `ne.erase.downcast-invariant`?
   **Recommendation: yes** (agent). X6 shows the hole. Rust forbids all
   non-static downcasts; this forbids only invariant slots.

10. **Providers and top level.** Name key roots `^Pool` and elide
    top-level slots to `^static`?
    **Recommendation: yes** (agent). Neither adds syntax beyond `^`.

11. **Static values.** May a `static` handle be closed early anywhere,
    with a `defer` that closes it an error (`ne.pin.static`)?
    **Recommendation: yes** (agent). A REPL or `main` closes globals,
    and nothing after a top-level binding's last use can be checked.

12. **Strong updates.** Should the checker reset a slot when a field of
    an object no other binding reaches is overwritten (L1)?
    **Recommendation: no, for now** (agent). Rust ships without it. It
    needs an alias analysis that the cost model does not have.

13. **Spellings** (last). `^` for lifetimes (`'` is the character
    literal), `^()` mirroring `$()`, `^static`, `closing` (Swift's
    `consuming` is about ownership transfer, which hd does not have), and
    `NonEscapable` (Swift's term).
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

**The category.** The checker must know which values a `defer` cleans up:

```hd
fn serve_one!(log: mut List[string], l: Listener) -> void:
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

**`push` needs nothing written.** The owner's argument offered two
choices: reject every push of a handle, or accept the leak. A third
exists. The element type carries the lifetime, as in Rust's
`Vec<&'a S>::push`, so `List[Conn].push` is checked through `T`. Swift
writes the same flow as the setter default `self: copy self, copy newValue`,
and Kotlin as `add(element: E_{this})`.

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
| TQ-24: declared and checked; a generic type follows its arguments; erasure rejected | root types declare it, holders follow fields (5.1); erasure keeps the slot (8.3) |
| TQ-25: a generic parameter opts in | accepted by default (`ne.generic.default`) |
| TQ-26: no NonEscapable returns; later, depend on every parameter | written reaches with elision (4) |

## Appendix B. Five-Language Comparison

Condensed from round 1. The last column is this design.

| Question | Rust | Swift 6.2+ (experimental `Lifetimes`) | Kotlin local lifetimes (note) | Scala capture checking | Mojo origins | hd |
| --- | --- | --- | --- | --- | --- | --- |
| Return a dependent value? | `fn f<'a>(x: &'a T) -> &'a U`, elided with one input | `@_lifetime(copy x)` or `borrow x`; a plain return is an error | `-> Iterator<B>_{this&transform}` | the result names captures: `Writer^{output}` | `-> ref [self] T` | `-> ^x T`; one input or `self` elided; `^()` fresh |
| Store in a struct? | `struct S<'a>`; each field keeps its lifetime | only in a `~Escapable` struct, one lifetime | `local class`, one inferred lifetime | `class C[X^]` or captured at construction | `struct S[origin: Origin]` | holders get an implicit slot; `data S[^c, ^u]` for several |
| Either, both, or one input | both: shared `'a`; one: separate | list the sources | `_{x&y}` or `_{x}` | `^{x, y}` or `^{x}` | `ref [a, b]` or `ref [a]` | `^a+b` or `^a` |
| Closures | borrow by default; `move` owns; `impl Fn() + 'a` | non-escaping closures may capture `~Escapable` | `(A) ->_{x} B` | `() ->{f} Unit` | capture lists; no escaping closures | a closure reaches its captures; `^static` to store globally |
| Async | one future carries `'a`; `spawn` needs `'static` | `Task` closures are escaping, so cannot capture | "lexical suspend" sketched | futures capture: `Future[T]^{C}` | no async | bang-driven children need nothing; a stored frame carries its captures |
| Default | borrows opt in; generics accept them | `~Escapable` opts in; generics escapable | `local` opts in | type parameters accept capturing types | `ref` is a separate convention | root types opt in; generics and function types accept lifetimes |
| Syntax | `'a`, `<'a>`, `+ 'a` | `~Escapable`, `@_lifetime(...)` | `local x`, `T_{x}` | `T^{x}`, `->{x}` | `ref [o] T`, `origin_of(x)` | `^a`, `^a+b`, `^()`, `^static`, `[^c]`, `closing` |

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
