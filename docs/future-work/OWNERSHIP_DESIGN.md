# Ownership And Borrows: Design Study

## Table Of Contents

- [A. Status](#a-status)
- [B. Core Model](#b-core-model)
- [C. Borrows And Ordinary Values](#c-borrows-and-ordinary-values)
- [D. Linear Versus Affine](#d-linear-versus-affine)
- [E. Suspension, Concurrency, And The Top Level](#e-suspension-concurrency-and-the-top-level)
- [F. NonEscapable Removed](#f-nonescapable-removed)
- [G. Stress Test](#g-stress-test)
- [H. Cost](#h-cost)
- [I. Prior Art](#i-prior-art)
- [J. Owner Questions](#j-owner-questions)
- [Parse Log](#parse-log)
- [Sources](#sources)

## A. Status

This is a design study for issue #163. Nothing in it is accepted
behavior, and the spec is frozen. Every rule ID (`own.*`), diagnostic
code, and spelling below is a placeholder. A rule marked **(agent)** was
written by the agent and needs the owner's review.

The study replaces the direction of
[NonEscapable Types And Lifetimes: Design](NONESCAPE_LIFETIME_DESIGN.md)
(round 8, 176 rules). That record stays as written. Its requirements and
mechanisms are mapped in [F](#f-nonescapable-removed), and its stress
suite is rerun in [G](#g-stress-test).

### A.1 The Owner's Words

The owner, 2026-10-16, about 12:20, verbatim:

> "not specific rules, I think the issue is our fundamental rules are
> wrong. it makes too many ad-hoc rules to fit into "no-annotation"
> design"

> "now i'm thinking the underlying model should just follow Rust,
> (however, it conflicts with our gc language semantics) I'm thinking if
> we can use some new keyword like `own`, something like `mut`, to
> indicate the NonCopyable semnatics and linear type semnatics / then,
> moving `own T` is consuming, to use as ref, you have to use `^T`. and
> now i am thinking if we need NonEscapable at all?"

> "annotation burden can be reduced by using variable name as lifetime,
> but fundamental model should be rust"

Two answers followed on 2026-10-16, verbatim:

| Time | Owner's words | Lands in |
| --- | --- | --- |
| about 12:10 | "i think brwchk should check all ^ local var" | [C](#c-borrows-and-ordinary-values) |
| about 12:15 | "you can also borrow a global/static variable as if it is own" | [E.5](#e5-top-level-bindings-and-the-repl) |

### A.2 Decisions This Study Builds On

| Decision | Source |
| --- | --- |
| The model follows Rust: ownership, moves, first-class borrows with lifetimes, borrows inside types. | owner, 2026-10-16, A.1 |
| `own` marks non-copyable values with linear semantics; using one by value consumes it. | owner, 2026-10-16, A.1 |
| A reference is written `^T`. | owner, 2026-10-16, A.1 |
| A lifetime is named by a variable, as `^{x}`. | owner, 2026-10-16, A.1 |
| NonEscapable is probably not needed. | owner, 2026-10-16, A.1 (a question, read here as a direction) |
| The borrow checker checks every `^` borrow of a local variable, whatever its type. | owner, 2026-10-16, A.1 |
| A top-level binding may be borrowed as if it were owned. | owner, 2026-10-16, A.1 |
| hd stays garbage-collected. | standing |
| `defer` stays; no `Drop`, `using`, or RAII. | standing (owner, 2026-10-14) |
| "If a thing is local, always show it in source." | owner, 2026-10-16, round-7 follow-ups |
| A store is written on the stored value, as `c: ^{dst.T} Conn`. | owner, 2026-10-16, round-7 follow-up 5 |
| A generic body cannot store a `T` in module state; no `T: static` bound. | owner, 2026-10-16, round-7 follow-up 2 |
| A downcast needs a static lifetime. | owner, 2026-10-14, Q6 |
| A mark inside `Fn[...]` is an error with a fix-it to `fn(...)`. | owner, 2026-10-16, round-7 follow-up 8 |

The orchestrator recommended three readings, all pending: linear, `own`
on the declaration, and no call-site move mark. The draft uses them and
asks each in [J](#j-owner-questions).

### A.3 Conventions

Examples are `text` blocks. A line that uses syntax no chapter specifies
carries a comment that starts `# hypothetical syntax`. A line marked
`# error:` gives the diagnostic the proposed checker would report.
The [Parse Log](#parse-log) records a syntax-only parse; nothing is
type-checked.

The examples share this prelude. The I/O methods take `^self`, as Rust
implements `Write` for `&TcpStream`; `close` consumes.

```text
own trait Socket:                              # hypothetical syntax
    fn send!(^self, bytes: List[u8]) -> void   # hypothetical syntax
    fn read!(^self) -> string                  # hypothetical syntax
    fn sent(^self) -> usize                    # hypothetical syntax
    fn close(self) -> void

own trait Acceptor:                            # hypothetical syntax
    fn accept!(^self) -> Conn                  # hypothetical syntax
    fn close(self) -> void

type Conn = dyn Socket
type Listener = dyn Acceptor

fn connect!(host: string) -> Conn
fn listen!(host: string) -> Listener
```

### A.4 Spec Impact

A later, owner-ordered spec edit would touch these rules.

| Spec rule | Change |
| --- | --- |
| `fn.method.no-sigil` | Gains `self` that consumes an own value, `^self`, and `^mut self`. |
| `fn.type.no-names` | A function type may name its parameters. |
| `fn.capture.storage.shared` | Holds for ordinary bindings only; own and borrow captures move or borrow. |
| `types.composite.aliases` | Holds for ordinary types only; an own value has one owner. |
| `req.suspend.type.equivalent` | The lowered frame type carries a lifetime bound. |
| `req.discard.cold` | A frame that holds an own value cannot be discarded. |
| `module.boundary.not-safe` | Adds own types and types that carry a lifetime. |
| `names.definite.*` | Moves reuse the possibly-uninitialized set. |

## B. Core Model

Three kinds of types exist. The kind decides whether a value aliases,
moves, or must be consumed.

| Kind | Examples | Copying the value | Must be consumed | Borrow-checked |
| --- | --- | --- | --- | --- |
| Ordinary | `i32`, `string`, `List[u8]`, plain data, `fn(...)` | aliases, as today | no | only through a written `^` |
| Own | `own data`, `own enum`, `own trait`, their holders, `own fn` | moves | yes (draft: linear) | always |
| Borrow | `^T`, `^mut T`, types that hold a borrow | `^T` copies; `^mut T` moves | no | always |

### B.1 Own Types

```text
own data File:                     # hypothetical syntax
    fd: i32

own data Pair:                     # hypothetical syntax
    a: File
    b: File

data Callbacks:
    on_open: fn() -> void          # ordinary: a function value is not own

data Bad:
    log: File                      # error: missing-own
```

1. r[own.decl] `own data`, `own enum`, and `own trait` declare an **own type**. (owner, 2026-10-16: the keyword; agent: its place, see [Q2](#q2-where-own-is-written))
2. r[own.decl.contain] A data or enum type with a field or payload of own type must be declared `own`. Error: `missing-own`, with a fix-it that adds it. (agent, as the old Q2's declared category)
3. r[own.decl.generic] A generic data or enum type takes an own argument only for a parameter declared `[own T]`. An instantiation is own when a stored field's substituted type is own. (agent, as the old NE-S10)
4. r[own.decl.tuple] A tuple with an own element is own. (agent)
5. r[own.decl.trait] A type that implements an own trait must be own. An own value converts only to `dyn K` of an own trait. Error: `own-erased-to-ordinary`. (agent)
6. r[own.decl.no-mut-view] An own type takes no `mut` view: ownership or `^mut` grants mutation. Writing `mut File` is an error. Error: `mut-on-own`. (agent)

> **Why.** Containment keeps every own value out of aliased storage, as
> Austral's linear universe and Swift's noncopyable structs do. An
> ordinary holder can be aliased, so an own value inside it would have two
> owners. [Q7](#q7-own-values-in-ordinary-holders) asks whether to allow
> it the way Swift classes do.

### B.2 Moves

```text
fn archive(f: File) -> void:       # takes ownership
    f.close()

fn demo!() -> void:
    f := open_file!("log")
    archive(f)                     # moves f
    archive(f)                     # error: use-after-move
```

1. r[own.move] Using an own binding by value moves it. That covers an argument for a by-value parameter, a return, an assignment, a field or element store, and a capture that the closure consumes. (owner, 2026-10-16: "moving `own T` is consuming")
2. r[own.move.after] After a move the binding is uninitialized, and a use is an error. Error: `use-after-move`. (agent; reuses `names.definite.reject`)
3. r[own.move.reinit] Assigning a moved `let` binding initializes it again. (agent)
4. r[own.move.overwrite] Assigning a binding or field that still holds an own value is an error, because the old value would be lost. Error: `own-overwritten`. (agent, follows from linearity)
5. r[own.move.no-mark] A move has no mark at the call site; the parameter type decides. (agent; see [Q3](#q3-a-call-site-move-mark))
6. r[own.move.partial] Moving a field out of an own value is an error. Error: `partial-move`. (agent)
7. r[own.move.destructure] A pattern that binds every field of an own data value, or a match on an own enum, consumes it. Its own fields become own bindings. (agent)
8. r[own.move.destructure.visible] Such a pattern is valid only where every field is visible, so only the type's module ends an own value with private fields. Error: `private-member`. (agent)

> **Why.** Rust allows partial moves out of a struct without `Drop`; Swift
> forbids partial consumption outside the type's definition. One rule is
> simpler: destructure the whole value, or keep the field as an `Option`
> and call `take()` through `^mut`.

### B.3 Borrows

```text
fn peek(f: ^File) -> i32:                    # hypothetical syntax
    f.fd

fn reset(f: ^mut File) -> void:              # hypothetical syntax
    f.fd = 0

fn demo!() -> void:
    f := open_file!("log")
    defer: f.close()
    _ := peek(f)                             # a shared borrow for the call
    reset(f)                                 # a mutable borrow for the call
    let view: ^File = f                      # hypothetical syntax: a shared borrow
    reset(f)                                 # error: borrow-conflict
    _ := view.fd
```

1. r[own.borrow.form] `^T` is a shared borrow and `^mut T` a mutable borrow. A borrow refers to a place that it does not own. (owner, 2026-10-16)
2. r[own.borrow.make] A borrow is made where a place is passed to a `^` or `^mut` parameter or receiver, or bound to a local written `^T`. No expression makes one. (agent)
3. r[own.borrow.mut-place] `^mut` needs a place with mutable access: an own binding, a `let` binding, or a `mut` view. Error: `mutable-upgrade`. (agent)
4. r[own.borrow.xor] While a shared borrow of a place is live, the place must not be assigned, mutated, mutably borrowed, moved, or consumed. Error: `borrow-conflict`. (owner, 2026-10-16: "follow Rust")
5. r[own.borrow.xor.mut] While a mutable borrow of a place is live, the place must not be used except through that borrow. Error: `borrow-conflict`. (owner, 2026-10-16)
6. r[own.borrow.live] A borrow is live from where it is made to the last use of any value whose type names its lifetime, as in Rust's non-lexical lifetimes. (agent)
7. r[own.borrow.copy] A shared borrow copies. A mutable borrow moves, and passing it to a `^mut` parameter reborrows it for the call. (agent, as Rust)
8. r[own.borrow.read] Reading a field or element through a borrow gives a borrow of that part, with the same lifetime. A scalar or `string` read through a borrow is copied out. (agent)
9. r[own.borrow.no-copy-out] A composite value read through a borrow must not flow into a position of its plain type. Error: `borrow-escapes`. (agent; see [Q5](#q5-copying-a-composite-out-of-a-borrow))
10. r[own.borrow.no-move-out] Moving or consuming through a borrow is an error. Error: `move-out-of-borrow`. (agent)

> **Why.** hd has no `&` expression, so the signature or the local's
> written type makes the borrow, as Rust's method calls auto-borrow
> their receiver. The owner's principle still holds: the `^` is in the
> source, on the parameter or the local.

### B.4 Lifetimes Named By Variables

```text
fn longer(a: ^Conn, b: ^Conn) -> ^{a, b} Conn:     # hypothetical syntax
    if a.sent() > b.sent(): a else: b

fn keep_in(dst: ^mut List[^Conn], c: ^{dst.T} Conn) -> void:   # hypothetical syntax
    dst.push(c)

fn dangle(c: Conn) -> ^{c} Conn:                    # hypothetical syntax
    c                                               # error: borrow-of-local
```

1. r[own.life.name] A lifetime is written in braces after `^`, as `^{a}` or `^{a, b}`. An atom names a parameter, `self`, a path, a requirement key `$K`, or `static`. (owner, 2026-10-16: "variable name as lifetime"; agent: the atoms)
2. r[own.life.path] A path names one lifetime inside a parameter's type: `p.src` for a lifetime parameter `src`, `xs.T` for type parameter `T`, `t._0` for a tuple element. (agent; the old round-6 paths and B8)
3. r[own.life.param] Each borrow in a parameter's type has its own lifetime, named by the parameter or a path through it. (agent, as Rust's distinct input lifetimes)
4. r[own.life.meaning] A value bounded by `^{a}` is usable only while `a`'s lifetime lasts. `^{a, b}` lasts while both last. (agent, as one Rust `'a` shared by both inputs)
5. r[own.life.static] `static` is the lifetime of a top-level binding and of every value that holds no borrow. (agent)
6. r[own.life.no-local] A result must not name a parameter that carries no lifetime, such as a by-value own parameter, since its value dies with the call. Error: `borrow-of-local`. (agent, as Rust E0515)
7. r[own.life.store] A borrow stored into another parameter's place writes the destination on the stored value, as `c: ^{dst.T} Conn`. Error: `lifetime-mismatch`. (owner, 2026-10-16, round-7 follow-up 5, carried over)
8. r[own.life.check] Every value that flows into a result, a store, or a binding must have a lifetime that lasts as long as the position needs. Error: `lifetime-mismatch`. (agent)

What a lifetime mark means depends on where it stands:

| Position | Bare `^` | `^{a}` |
| --- | --- | --- |
| Parameter | A borrow with a fresh lifetime, named by the parameter | A store into `a`'s place ([own.life.store](#b4-lifetimes-named-by-variables)) |
| Result | Elided ([B.5](#b5-elision)) | Bounded by `a` |
| Field | Not allowed: name a lifetime parameter | Names a lifetime parameter |
| Type argument | As its position | As its position |
| Local written type | A borrow of the initializer, lifetime inferred | Bounded by `a` |

### B.5 Elision

```text
fn first(pool: ^List[Conn]) -> Option[^Conn]       # hypothetical syntax: elided to pool
fn pick(^self) -> ^Conn                             # hypothetical syntax: elided to self
fn either(a: ^Conn, b: ^Conn) -> ^Conn              # hypothetical syntax: error: ambiguous-lifetime
fn open!(host: string) -> Conn                      # owned: no lifetime
```

1. r[own.elide.one] A bare `^` in a result, with exactly one input lifetime, takes that lifetime. (Rust's rule, as the reference states it)
2. r[own.elide.self] In a method whose receiver is `^self` or `^mut self`, a bare `^` in the result takes the receiver's lifetime. (Rust's rule)
3. r[own.elide.ambiguous] Any other bare `^` in a result is an error. Error: `ambiguous-lifetime`. (Rust's rule)
4. r[own.elide.inputs] Input lifetimes are counted over every borrow in the parameters, lifetime arguments, and the requirement keys of own traits (`$K`). (agent)
5. r[own.elide.owned] A result written without `^` is owned or ordinary and carries no lifetime. (agent, as Rust)

> **Why.** These are Rust's three elision rules, unchanged. The old design
> needed its own table because a value without `^` could still be
> local. Now a value without `^` is owned or ordinary, so it never
> borrows, and the old special cases fall away.

### B.6 Borrows Inside Types

```text
own data Source:                                     # hypothetical syntax
    fd: i32

data Parser[^src]:                                   # hypothetical syntax
    input: ^{src} Source                             # hypothetical syntax
    at: usize

data Session[^c, ^u]:                                # hypothetical syntax
    client: ^{c} Conn                                # hypothetical syntax
    upstream: ^{u} Conn                              # hypothetical syntax

fn parser(s: ^Source) -> Parser[^]:                  # hypothetical syntax: elided to s
    Parser { input: s, at: 0 }

fn client_of(s: Session[^, ^]) -> ^{s.c} Conn:       # hypothetical syntax
    s.client
```

1. r[own.data.lifetime-param] A data or enum declaration may declare lifetime parameters after its type parameters, as `data Parser[^src]` and `data Pair[A, B, ^a]`. (owner direction: "borrows in types"; agent: the spelling, the old B9 order)
2. r[own.data.field] A field that holds a borrow names a declared lifetime parameter, as `input: ^{src} Source`. Error: `undeclared-lifetime`. (agent)
3. r[own.data.lifetime-arg] A use of such a type writes its lifetime arguments after its type arguments, as `Parser[^{text}]`, or a bare `Parser[^]`. A bare one acts as a bare `^` in that position. (agent)
4. r[own.data.lifetime-arg.required] Leaving the lifetime arguments out is an error, by the owner's principle. Error: `missing-lifetime`, with a fix-it that writes `[^]`. (agent)
5. r[own.data.mut-borrow] A type that holds a `^mut` borrow moves, as `^mut` does, but need not be consumed. (agent; see [Q6](#q6-mutable-borrows-inside-types))
6. r[own.data.variance] A lifetime parameter's variance is computed by the `types.polarity` rules: covariant in readonly positions, invariant under `mut` or `^mut`. (agent; carries the old NE-S1, Rust's rule)
7. r[own.data.literal] A data literal infers its lifetime arguments from its field values. (agent)

### B.7 Methods And `self`

1. r[own.self.forms] A receiver is `self`, `^self`, or `^mut self`, shorthand for `self: Self`, `self: ^Self`, and `self: ^mut Self`. (agent)
2. r[own.self.consume] On an own type, `self` consumes the receiver. A method call with such a receiver moves the receiver. (owner direction)
3. r[own.self.ordinary] On an ordinary type, `self` and `mut self` keep today's meaning. `^self` and `^mut self` borrow the receiver's place. (agent)

### B.8 Closures

```text
fn closer(c: Conn) -> own fn() -> void:     # hypothetical syntax
    fn() -> void: c.close()                  # moves c in: the body consumes it

fn counter(c: ^Conn) -> ^{c} fn() -> usize: # hypothetical syntax
    fn() -> usize: c.sent()                  # captures a borrow

fn demo!() -> void:
    c := connect!("db")
    done := closer(c)
    done()                                   # consumes done
    done()                                   # error: use-after-move
```

1. r[own.closure.move] A closure whose body consumes an own capture moves it in. The closure's type is `own fn(...)`. (owner brief; agent: the spelling)
2. r[own.closure.borrow] A closure that only borrows an own capture borrows the binding, shared or mutably, while the closure lives. Its type is bounded by that borrow. (agent, as Rust's capture inference)
3. r[own.closure.borrow-capture] A closure that captures a borrow is bounded by the borrow's lifetime, as `^{c} fn() -> usize`. (agent)
4. r[own.closure.once] Calling an `own fn` consumes it. Its body must consume each own capture ([D](#d-linear-versus-affine)). (agent)
5. r[own.closure.ordinary] A capture of an ordinary binding stays shared storage, unchecked, as `fn.capture.storage.shared` says today. (agent; see [Q10](#q10-closures-over-a-borrowed-local))
6. r[own.closure.no-self] A closure that holds a `^mut` borrow has no self-name, so it never runs inside its own call. (agent)

### B.9 Function Types

1. r[own.fntype.names] A function type may name its parameters, as `fn(c: ^Conn) -> ^{c} Conn`. `fn.type.no-names` must change. (carries the old D8)
2. r[own.fntype.elide] A function type's result elides by [B.5](#b5-elision). For a parameter `f`, `{f}` names its environment. (carries D8)
3. r[own.fntype.fresh] An input `^T` of a function type gets a fresh lifetime at each call, as Rust's higher-ranked `for<'a>`. (agent)
4. r[own.fntype.bound] `^{a} fn(...)` bounds the environment by `a`. An unmarked function type holds no borrow. (agent)
5. r[own.fntype.own] `own fn(...)` is a one-shot function type ([B.8](#b8-closures)). A function item has type `fn(...)`. (agent)
6. r[own.fntype.ctor] `Fn[...]` and `SuspendFn[...]` spell only function types with no borrow, no names, and no `own`. A mark inside one is an error. Error: `misplaced-lifetime`. (owner, 2026-10-16, NE-S5c and round-7 follow-up 8, carried)

### B.10 Generics And `dyn`

```text
fn same[T](v: T) -> T: v                     # T may be a shared borrow
fn keep[own T](xs: ^mut List[T], v: T) -> void:  # hypothetical syntax
    xs.push(v)                               # moves v when T is own
fn inspect(x: dyn Inspectable) -> void: pass
```

1. r[own.generic.copy] A type parameter `[T]` takes ordinary types, shared borrows included, and `T` carries their lifetimes. (owner direction: Rust; carries NE-S1d)
2. r[own.generic.own] `[own T]` also takes own types and types that move. The body uses each `T` value linearly. (owner brief; as Swift's `~Copyable`)
3. r[own.generic.store] A generic body cannot store a `T` in module state, and there is no `T: static` bound. (owner, 2026-10-16, round-7 follow-up 2, carried)
4. r[own.dyn.own] `dyn K` of an own trait is own. (agent)
5. r[own.dyn.bound] `dyn K` holds no borrow. A value that holds a borrow converts to `^{a} dyn K` only. Error: `lifetime-mismatch`. (agent, as Rust's default `'static` object bound)
6. r[own.dyn.downcast] A downcast needs a `static` lifetime. Error: `downcast-non-static`. (owner, 2026-10-14, Q6, carried)

### B.11 `Option`, `Result`, And `List` Of Own

These are stdlib-tier items; the language tier needs only `[own T]`.

```text
impl[own T] List[T]:                                   # hypothetical syntax
    fn push(^mut self, v: T) -> void                   # hypothetical syntax: moves v
    fn pop(^mut self) -> T?                            # hypothetical syntax
    fn at(^self, index: usize) -> Option[^T]           # hypothetical syntax
    fn into_iter(self) -> Iterator[T]

impl[own T] T?:                                        # hypothetical syntax
    fn take(^mut self) -> T?                           # hypothetical syntax
```

- `Option`, `Result`, `List`, `Deque`, and `Map` values declare `[own T]`.
- Methods that copy a `T`, such as `get` and `unwrap_or`, stay in the
  `impl[T]` block, as Swift keeps copying methods in a `Copyable`
  extension.
- A list of own values is consumed by `into_iter`, whose loop consumes
  each element ([D.2](#d2-linear-rules)).

## C. Borrows And Ordinary Values

### C.1 The Decision

The owner, 2026-10-16, about 12:10, verbatim: "i think brwchk should
check all ^ local var".

**Reading.** The borrow checker applies to every `^` and `^mut` borrow of
a local variable, whatever its type, own or ordinary. Ordinary aliasing
without `^`, such as `let b = a` or passing a plain parameter, stays
unchecked, as today. So writing `^` triggers the check, not own-ness. An
own value is always moved or borrowed, so it is always checked.

| Code | Checked? |
| --- | --- |
| `let view: ^List[i32] = xs` | yes: `xs` is frozen while `view` lives |
| `fill(buf)` with `fill(b: ^mut List[u8])` | yes: `buf` is exclusive for the call, and `b` is not kept |
| `fn remember(b: ^List[u8])` storing `b` | yes: `b` must not outlive its lifetime |
| `alias := xs`, `f(xs)` with `f(xs: List[i32])` | no: ordinary GC aliasing |
| A closure that captures `xs` | no ([Q10](#q10-closures-over-a-borrowed-local)) |
| Any use of an own binding | yes |

### C.2 A Borrow Of An Ordinary Local

```text
fn total(xs: ^List[i32]) -> i32:          # hypothetical syntax
    let sum = +0
    for x in xs:
        sum = sum + x
    sum

fn fill(buf: ^mut List[u8]) -> void:      # hypothetical syntax
    buf.push(1)

fn demo() -> void:
    let xs: mut List[i32] = [+1, +2]
    let view: ^List[i32] = xs             # hypothetical syntax
    xs.push(+3)                           # error: borrow-conflict
    _ := total(view)
    let buf: mut List[u8] = []
    fill(buf)                             # fine: exclusive for the call
```

1. r[own.local.freeze] While a shared borrow of an ordinary local lives, the local must not be assigned or used through a `mut` view. Error: `borrow-conflict`. (owner, 2026-10-16: "check all ^ local var"; agent: the reading)
2. r[own.local.exclusive] While a mutable borrow of an ordinary local lives, the local must not be used except through it. Error: `borrow-conflict`. (owner, 2026-10-16; agent: the reading)
3. r[own.local.per-place] The check covers the borrowed place only. An alias made earlier without `^` is not frozen. (agent)
4. r[own.local.not-kept] A `^` parameter of any type is never kept beyond its lifetime. It must not reach a top-level binding, a plain parameter, or a result that does not name it. Error: `borrow-escapes`. (agent; carries the old D4)

```text
let CACHE: mut List[List[u8]] = []

fn remember(b: ^List[u8]) -> void:        # hypothetical syntax
    CACHE.push(b)                         # error: borrow-escapes

fn identity(b: ^List[u8]) -> ^{b} List[u8]:   # hypothetical syntax
    b

fn launder(b: ^List[u8]) -> void:         # hypothetical syntax
    CACHE.push(identity(b))               # error: borrow-escapes

fn sneaky(xs: mut List[i32]) -> void:
    alias := xs
    let view: ^List[i32] = xs             # hypothetical syntax
    alias.push(+1)                        # not checked: an earlier plain alias
    _ := view.len()
```

> **Note.** For an own value, the place is the only path, so exclusivity
> holds for the object. For an ordinary value, it holds for the place,
> and `sneaky` shows the gap. That is the reading of "all ^ local var":
> the check is per variable.

### C.3 Plain Code Is Unaffected

```text
fn register(log: mut List[string]) -> fn(string) -> void:
    fn(m: string) -> void: log.push(m)

fn demo() -> void:
    let log: mut List[string] = []
    add := register(log)
    add("a")
    log.push("b")                         # fine: no ^ is written
```

Today's shared-mutable code compiles unchanged: no `^`, no check. A
borrow appears only where a signature or a local's type writes `^`.

### C.4 What A Borrow Of An Ordinary Value Costs Callers

A borrow is never passed to a plain parameter, by
[own.local.not-kept](#c2-a-borrow-of-an-ordinary-local). So a `^List[u8]`
reaches `len` only when `len` takes `^self`. The std scan in
[H.2](#h2-annotation-burden-in-std) prices this, and
[Q4](#q4-plain-parameters-of-ordinary-type) asks whether a plain
parameter should accept a borrow implicitly.

## D. Linear Versus Affine

### D.1 The Two Readings

| | Linear (draft) | Affine |
| --- | --- | --- |
| An own value | is consumed exactly once on every path | is consumed at most once |
| A forgotten `close` | an error at the scope's end | silently leaks the handle |
| `defer: c.close()` | consumes `c` at block exit | consumes `c` at block exit |
| Cleanup without `defer` | an error | no cleanup |
| Rust | no: `mem::forget` is safe | yes, with `Drop` |
| Austral | yes | no |

Under affine typing, an own value that goes out of scope just ends. With
no `Drop` (standing decision), nothing closes it. So affine hd would
check use-after-close but not leaks. Linear hd checks both, and `defer`
is the place that consumes.

```text
fn forgot!() -> void:
    c := connect!("db")
    c.send!([1])                          # linear: error: own-not-consumed; affine: fine, c leaks
```

### D.2 Linear Rules

```text
fn serve!(l: ^Listener) -> void:          # hypothetical syntax
    c := l.accept!()
    defer: c.close()                      # reserves c
    c.send!([1])                          # fine: a borrow of a reserved binding
    keep(c)                               # error: reserved-by-defer
```

1. r[own.linear.exit] At each exit of a binding's scope, an own binding must be consumed or reserved by a registered `defer`. Error: `own-not-consumed`. (owner, 2026-10-16: "linear type semnatics")
2. r[own.linear.exits] The exits are the normal end, `return`, `break`, `continue`, `?`, and cancellation at a bang call ([E.2](#e2-cancellation)). (agent)
3. r[own.linear.merge] At a merge, an own binding must be consumed on every incoming path or on none. Error: `own-not-consumed`. (agent, as Austral)
4. r[own.linear.loop] A loop body must not consume an own binding declared outside the loop, unless it initializes the binding again before the next iteration. Error: `use-after-move`. (agent, as Austral)
5. r[own.linear.temp] An own value that no binding holds must be consumed where it is made. `_ := connect!("db")` is an error. Error: `own-not-consumed`. (agent)
6. r[own.linear.panic] A path that ends in a panic needs no consumption, since a panic runs no `defer` and poisons the instance (`flow.defer.panic`). (agent)
7. r[own.defer.reserve] A `defer` suite may consume an own binding of its block, and this **reserves** the binding from the registration on. (agent)
8. r[own.defer.reserved] A reserved binding may be borrowed, but not moved, consumed, or assigned. Error: `reserved-by-defer`. (agent)
9. r[own.defer.order] A `defer` suite registered before the consuming suite runs after it, so it must not use the binding. Error: `use-after-move`. (agent)

> **Why.** `defer` stays and nothing else cleans up, so linearity makes
> the compiler demand the `defer` that a careful programmer writes
> anyway. Reservation turns the old parked cleanup-block rules into
> ordinary move checks: a reserved value cannot escape the block.

## E. Suspension, Concurrency, And The Top Level

### E.1 Frames And Borrows

```text
fn echo!(c: ^Conn) -> void:               # hypothetical syntax
    c.send!([1])

fn select!(keep: ^Conn, scratch: ^Conn) -> ^{keep} Conn:   # hypothetical syntax
    _ := scratch.sent()
    keep

fn via_frame!(keep: ^Conn, scratch: ^Conn) -> ^{keep} Conn:   # hypothetical syntax
    let s: ^{keep, scratch} mut Suspend[^{keep} Conn] = select(keep, scratch)   # hypothetical syntax
    s!()
```

1. r[own.suspend.frame] A cold frame is bounded by every borrow it captures: its argument borrows, the called closure's environment, and borrowed providers. (agent; replaces the old `run` slot)
2. r[own.suspend.output] The frame's type argument carries the output's lifetime, as `Suspend[^{keep} Conn]`. (carries the old round-7 follow-up 6)
3. r[own.suspend.weaken] A suspending function weakens to `fn(...) -> ^{S} mut Suspend[T]`, where `S` lists every borrowed parameter. (carries NE-S5d)
4. r[own.suspend.bang] A bang call's borrows end when it returns. (agent)
5. r[own.suspend.own-arg] A plain call must not pass an own argument or call an `own fn!`; only a bang call may. Error: `own-in-cold-frame`. (agent; see [Q8](#q8-own-arguments-to-cold-frames))
6. r[own.suspend.stored] A stored frame keeps its bound: module state takes only a frame bounded by `static`. Error: `lifetime-mismatch`. (agent)

> **Why.** A cold frame may be discarded or cancelled before its first
> poll. Then no line of its body runs, so no `defer` consumes an own
> value it holds. Rule 5 keeps own values out of cold frames.

### E.2 Cancellation

A bang call may be the point where cancellation arrives
(`req.cancel.steps`), and cancellation runs only registered `defer`
suites (`req.cancel.defer`). So every bang call is an exit for
[own.linear.exit](#d2-linear-rules).

```text
fn careless!() -> void:
    c := connect!("db")
    c.send!([1])                          # error: own-not-consumed: cancellation here leaks c
    c.close()

fn careful!() -> void:
    c := connect!("db")
    defer: c.close()
    c.send!([1])                          # fine
```

A handle whose close must suspend cannot be closed in a `defer`
(`flow.defer.suspend`). It is a known limit, the async-cleanup problem that
`req.cancel.hook-synchronous` already leaves open.

### E.3 Structured Children

```text
use std.task.{all, race}

fn mirror!(a: ^Conn, b: ^Conn) -> void:          # hypothetical syntax
    _ := all!(echo(a), echo(b))                  # fine: both frames end inside the call

fn fastest!(a: ^Conn, b: ^Conn) -> string:       # hypothetical syntax
    race!(a.read(), b.read())                    # losers are cancelled before race! returns
```

1. r[own.children.borrow] Children of `all!` and `race!` may borrow the parent's places for the call. (agent; `req.combinator.*`)
2. r[own.children.xor] The borrow check runs over all children of one call together: no two may borrow one place when either borrow is mutable. Error: `borrow-conflict`. (agent, as Rust's `thread::scope`)

### E.4 Providers

```text
own trait Pool:                                  # hypothetical syntax
    fn get(^self) -> ^Conn                       # hypothetical syntax

fn grab() -> ^Conn $ Pool:                       # hypothetical syntax: elided to $Pool
    $.use(Pool).get()

fn serve!(p: ^dyn Pool) -> void:                 # hypothetical syntax
    $.with(Pool=p):
        grab().send!([1])
```

1. r[own.provider.own-key] For a key whose trait is own, `$.with(K = p)` borrows `p` for the block, and `$.use(K)` returns a borrow bounded by `$K`. (agent; carries the old key roots)
2. r[own.provider.ordinary] A provider of an ordinary key is `dyn K`, which holds no borrow ([own.dyn.bound](#b10-generics-and-dyn)). (agent; replaces NE-S6b)
3. r[own.provider.frame] A frame built inside `$.with(K = p)` is bounded by `p`'s borrow. (agent)

### E.5 Top-Level Bindings And The REPL

The owner, 2026-10-16, about 12:15, verbatim: "you can also borrow a
global/static variable as if it is own".

**Reading.** `^G` of a top-level binding is a borrow whose owner is the
binding itself, with lifetime `static`. It is checked as a borrow of an
owned local is. It may be returned as `^{static}` and stored wherever a
static borrow fits.

```text
let CONFIG: Settings = load_settings()

fn settings() -> ^{static} Settings:             # hypothetical syntax
    let s: ^Settings = CONFIG                    # hypothetical syntax
    s

let SHARED: List[^{static} Settings] = [settings()]   # hypothetical syntax
```

1. r[own.top.borrow] A shared borrow of a top-level binding has lifetime `static`. (owner, 2026-10-16: "borrow a global/static variable as if it is own")
2. r[own.top.borrow.check] It is checked as a borrow of an owned local: within a body, the binding must not be assigned or mutated while the borrow lives. Error: `borrow-conflict`. (owner, 2026-10-16; agent: the reading)
3. r[own.top.mut] A `^mut` borrow of a top-level binding is an error in this draft. Error: `borrow-conflict`. (agent; see [Q11](#q11-mutable-borrows-of-a-top-level-binding))
4. r[own.top.own] A top-level binding may hold an own value. It is never consumed: the instance's end releases it. Moving or consuming it is an error. Error: `move-out-of-top-level`. (agent; see [Q9](#q9-own-values-at-the-top-level))

A REPL input's bindings are top-level bindings
(`cli.repl.declarations.bindings`), so the same rules hold:

```text
c := connect!("db")              # a REPL input: an own top-level binding
c.send!([1])                     # a later input: a shared borrow, static
c.close()                        # error: move-out-of-top-level
```

> **Why.** Another function can reach `G` without going through the
> borrow, so a body cannot see every use. An own `G` has no `mut` view,
> so only a `^mut` borrow could conflict, and rule 3 forbids it. An
> ordinary `G` may still be mutated elsewhere through a `mut` view, the
> per-place gap of [C.2](#c2-a-borrow-of-an-ordinary-local). Rust makes
> reading or writing a `static mut` `unsafe` for the same reason (Rust
> reference, items.static.mut.safety).

### E.6 The Serialization Boundary

1. r[own.boundary] An own type, and a type that carries a lifetime, are not boundary-safe. (agent; extends `module.boundary.not-safe`)

[Serializable Closures](SERIALIZABLE_CLOSURE_DESIGN.md) says portable
captures "reach `static`", a term of the old design. Replacement wording,
for its next revision:

| Rule | Replacement |
| --- | --- |
| `portable.capture.boundary` | Every captured value must be boundary-safe, so it holds no borrow and no own value. Error: `portable-capture-not-boundary-safe`. |
| `remote.cap.reach` | The exported object must hold no borrow: its lifetime is `static`. |
| C.7, a handle row | An own handle is not boundary-safe, so it is rejected as an argument, a result, or a sealed value. |
| C.7, the closing sentence | Copying ends a borrow, and an own value never crosses a boundary. |

## F. NonEscapable Removed

### F.1 The Old Requirements

| # | Old requirement | New mechanism |
| --- | --- | --- |
| 1 | NonEscapable is a category of types. | `own` declares a category: non-copyable and linear ([B.1](#b1-own-types)). |
| 2 | Lifetimes are in the semantics and in signatures. | Kept: borrow lifetimes ([B.4](#b4-lifetimes-named-by-variables)). |
| 3 | A result or store depends on any subset of inputs, exactly. | Kept: `^{a, b}` results, `c: ^{dst.T}` stores. |
| 4 | No false alarms from inference; ambiguity needs an annotation. | Kept: Rust's elision, and `ambiguous-lifetime`. |
| 5 | Several independent lifetimes in one struct. | Kept: lifetime parameters, `data Session[^c, ^u]`. |
| 6 | Terse notation. | Kept: `^`, `^{x}`. |
| 7 | Local positions marked; unmarked is global. | Replaced: unmarked is owned or ordinary; `^` is a borrow. |
| 8 | Any parameter of any type may be marked `^`. | Kept, now borrow-checked by the owner's scope decision ([C](#c-borrows-and-ordinary-values)). |
| 9 | One reach per component, written in type arguments. | Kept as Rust's lifetimes in type arguments, `List[^Conn]`. |
| 10 | Exact substitution of published signatures. | Kept: Rust's model. |
| 11 | "If a thing is local, always show it in source." | Kept: every borrow writes `^`, and lifetime arguments are written. |

### F.2 The Old Mechanisms

| Old mechanism | New mechanism, or dropped and why |
| --- | --- |
| The category and `impl NonEscapable` | `own data`, checked against fields. |
| Generic category from stored fields (NE-S10) | Same rule for `[own T]` instantiations. |
| Roots: static, parameter, key, local, type-parameter | Lifetimes: borrowed parameters, `$K`, `static`, and `T`'s own lifetimes. |
| A fresh handle reaches `static` (OQ1) | Dropped: a fresh handle has one owner and must be consumed. |
| Reaches as root sets | Rust lifetimes, solved per body. |
| Declared slots `data S[^c, ^u]` | Lifetime parameters, same spelling. |
| Component paths `xs.T`, `p.A`, `t._0` | Kept as lifetime paths. |
| Frame `run` slot and output | A lifetime bound on the frame; `T` carries the output. |
| One reach per callable environment, `^{f}` stores (NE-S4b) | Dropped: a store's lifetime sits in the destination's type, as in Rust. |
| Bound method stores into the environment (follow-up 9) | Dropped: `dst::push` has type `fn(v: ^{dst.T} Conn) -> void`. |
| Variance from the surface (NE-S1, NE-S1c) | Kept: `types.polarity` on lifetime parameters. |
| Erasure join (B11) | Replaced: `dyn K` holds no borrow, `^{a} dyn K` holds one bound. |
| Downcast needs `static` (Q6) | Kept. |
| Ordinary providers must be static (NE-S6b) | Follows from `dyn K` holding no borrow. |
| Key roots `^{$Pool}` | Kept as the lifetime `$K`. |
| Top-level bindings reach `static` | Kept, and a top-level binding may be borrowed ([E.5](#e5-top-level-bindings-and-the-repl)). |
| `LocalParams` optimizer fact (D4, NE-S7) | Kept: a `^` parameter is never kept, now checked for every type. |
| `Fn[...]` all-global only (NE-S5c) | Kept as "no borrow, no names, no `own`". |
| Closing, `closing` mode, cleanup blocks (parked) | Replaced by linear consumption and `defer` reservation ([D.2](#d2-linear-rules)). |
| Liveness against cleanup blocks (parked) | Replaced by non-lexical borrow liveness. |
| Annotation handles over NonEscapable types (open) | Still open: a handle read through a borrow gives a borrow. |

### F.3 The Old Known Limits

| # | Old limit | Now |
| --- | --- | --- |
| L1 | No strong updates of a heap reach | A local's lifetime follows the flow (NLL); a field's lifetime is its type's, as in Rust. |
| L2 | A list's elements share one reach | Same as Rust: one lifetime per type argument. |
| L4 | A loop-carried target reaches every iteration | Same as Rust. |
| L6 | A function type outside a parameter cannot name its environment | Same; Rust names no closure environment either. |
| L7 | One root per creation site | Gone: each own value has its own owner. |
| L8 | A local `^dyn` cannot be downcast | Same, narrower in Rust. |
| L9 | A written reach applies whatever the value holds | Gone: an owned result carries no lifetime. |
| L10 | A store into an environment reaches every capture | Gone: no environment stores. |

## G. Stress Test

### G.1 Verdicts

Verdicts: **accepted**, **rejected** (as intended), **limit** (a sound
rejection of a reasonable program), and **hole** (unsound or
unspecified). The Rust column gives Rust's verdict on the same shape.

| # | Case | Verdict | Why | Rust |
| --- | --- | --- | --- | --- |
| R1 | `longest` | accepted; `pick` rejected | `^{a, b}` result; `^{keep}` does not cover `other` | same |
| R2 | A dangling reference | rejected | `borrow-of-local` | same |
| R3 | `ImportantExcerpt` | accepted | `data Excerpt[^a]`, `-> Excerpt[^]` | same |
| R4 | `Parser<'c, 's>` | accepted | `data Router[^p]` over `^{p} List[Conn]` | same |
| R5 | A slice iterator | accepted | `next(^mut self) -> Option[^{self.a} Conn]` | same |
| R6 | Evil feeder | rejected | invariance under `^mut` | same |
| R7 | Structured children; a global job list | children accepted; global jobs limit | a job that owns a handle needs `^mut JOBS` ([Q11](#q11-mutable-borrows-of-a-top-level-binding)) | accepted with `'static` |
| R8 | Reassignment | accepted | NLL | same |
| R9 | `accept` | accepted | owned result, no lifetime | same |
| R10 | A borrow across a suspension | bang accepted; stored frame rejected; owned frame limit | `own.suspend.own-arg` | spawn of an owned future accepted |
| R11 | `thread::scope` | accepted | children borrow `c` shared | same |
| R12 | A lifetime-generic method | accepted | `^{self.u}` | same |
| R13 | A map of two lifetimes | rejected | one value lifetime | same |
| R14 | `Option<&T>` with `?` | accepted | elided to `pool` | same |
| R15 | A self-referential struct | limit | a field cannot borrow a sibling | same |
| S1 | `Span` view | accepted | `-> Lines[^]` | - |
| S2 | A mutating store | accepted | `c: ^{self.a} Conn`, or an own pool that moves `c` in | - |
| S3 | A consuming close wrapper | accepted | `fn shutdown(self)` destructures and closes | `drop(self)` accepted |
| S4 | `MutableSpan` | accepted | shared writers; `^mut` writers would conflict | - |
| S5 | Conditional `Escapable` | accepted; `Labeled` rejected | `data Tagged[own T]`; `missing-own` | - |
| S6 | A non-escaping closure stored locally | accepted | `List[^{attempt} fn!() -> bool]` | - |
| M1 | Mojo `ref [a, b]` | accepted | `^{a, b}` | - |
| M2 | Mojo struct origin | accepted | a lifetime parameter | - |
| M3 | Capturing and escaping closures | accepted | `^{c} fn()`; `tally` holds no borrow | same |
| T1 | Independent slots | accepted | `^{s.c}` | same |
| T2 | Swapping slots | rejected | distinct lifetimes | same |
| T3 | A REPL handle | accepted for use; close rejected | [E.5](#e5-top-level-bindings-and-the-repl) | no REPL |
| D1 | A handle in a `Result` through `?` | accepted | `own enum LinkError` moves through `?`; no `dyn Error` | same |
| D2 | A cold frame in a field | rejected | `Worker[^w]` keeps `w` for its life | same |
| D3 | `race!` and cancellation | accepted | children borrow; losers end inside the call | - |
| D4 | An own-trait provider | accepted; store into `KEEP` rejected | the result is bounded by `$Pool` | same shape |
| D5 | A map in a holder | accepted | `own data Registry` moves `c` in | same |
| D6 | A `dyn` list mixing sources | accepted | `List[^Conn]` of two borrows | same |
| D7 | Recursive data | accepted | `own enum Chain`; `Duo[^a, ^b]` | same |
| D8 | A callback registry | borrow rejected; owned limit | an `own fn` needs a list of `own fn` and `^mut HANDLERS` | accepted |
| N1 | An unmarked handle parameter | rejected | `fn ping(c: Conn)` now consumes, and leaks `c` | accepted (drops) |
| N2 | Generic and `dyn` | accepted for borrows; own rejected | `[T]` takes `^Conn`; own needs `[own T]` | same |
| N3 | General locality | accepted; store rejected | [C](#c-borrows-and-ordinary-values) | same |
| N4 | Function types | accepted; `bad` rejected | `ambiguous-lifetime` | same |
| N6 | A concrete holder | rejected | `missing-own` | - |
| X1 | A late cleanup block | rejected | `reserved-by-defer` | - |
| X2 | `defer` order | rejected | `own.defer.order` | - |
| X3 | A `defer` reads the binding at exit | rejected | assigning a reserved binding | - |
| X4 | A cleanup closure | accepted; double close rejected | `own fn` | same |
| X5 | Closing a borrowed handle | rejected | `move-out-of-borrow` | same |
| X6 | Widening through a downcast | rejected | downcast needs `static` | same |
| X7 | A suspending close in a `defer` | limit | async cleanup | open in Rust too |
| C01 | A trait getter | rejected | invariance | same |
| C02 | A closure returning mutable state | rejected | invariance | same |
| C03 | A readonly list of mutable lists | rejected | `types.readonly.nested` | same |
| C04 | An identity helper | rejected | the result names `b` | same |
| C05 | A bound closing method | accepted; reuse rejected | `c::close` moves `c` | same |
| C06 | One suite retargets another | rejected | `reserved-by-defer` | - |
| C07 | A read after close in one suite | rejected | `use-after-move` | same |
| C08 | A frame and its output | accepted | output bounded by `keep` only | same |
| C09 | A callback from a provider | accepted; ambiguous form rejected | `$Pool` counts | - |
| C10 | A lease that keeps its pool | accepted | `own data Lease[^p]` | same |
| C11 | An optional pair | accepted | `^{p.T.a}` | same |
| C12 | A cleared list | limit | the type keeps `c`'s lifetime | same |
| C13 | A shared iterator | accepted | optimizer concern only | - |
| C14 | An ordinary result escapes | accepted | no `^`, unchecked | - |
| C15 | Handles in a loop | rejected as written | the second handle leaks | accepted (drops) |
| C16 | A scoped race | accepted | children borrow | - |
| C17 | Early and delegated close | rejected | `use-after-move` | same |
| C18 | Phantom metadata | accepted | `data Tag[own T]` stores no `T` | same |
| C19 | A local classifier | limit | downcast needs `static` | narrower |
| C20 | A scalar closure factory | accepted | holds no borrow | same |
| R21 | An ordinary provider over a borrow | rejected | `dyn Reader` holds no borrow | same |
| R22 | Optional wrapping, invariant slot | rejected; named form accepted | invariance | same |
| R23 | A tuple hides the widening | rejected; `bundle` accepted | per-element lifetimes | same |
| R24 | A bound appender | accepted; unnamed rejected | `fn(v: ^{dst.T} Conn)` | same |
| R25 | A cold call of a closure | accepted with `^{action}` | frame bound | same |
| R26 | A weakened suspending function | accepted | `^{a, b} mut Suspend[^{a} Conn]` | same |
| R27 | A scoped callback | accepted; borrowed result rejected | fresh input lifetime | same |
| R28 | A closure stores into a capture | rejected; named form accepted | fresh input lifetime | same |
| R29 | A fresh mutable result | accepted | owned result | same |
| R30 | Generic scalar results | accepted | `List[usize]` holds no borrow | same |
| R31 | An ordinary identity helper | accepted | no `^`, unchecked | - |
| R32 | A fresh handle in a registry | limit | as D8 | accepted |
| R33 | A bare callback returned | accepted | `-> ^{f} fn() -> usize` | same |
| R34 | Weakening over a borrowed buffer | rejected; `^{bytes}` accepted | frame bound | same |
| R35 | Generic identity of a `^mut` | accepted with `[own T]` | `[T]` takes only shared borrows | accepted with `T` |
| R36 | A struct of a list and a handle | accepted | `data Bundle[^e, ^k]` | same |
| R37 | A list lookup of a two-lifetime record | accepted | `T` carries both | same |
| R38 | Lists of different lifetimes | rejected | invariance | same |
| R39 | A bound getter | accepted | `T` keeps its lifetime | same |
| R40 | A closure reads one capture, stores into another | accepted | no environment stores | same |
| R41 | `Fn` constructor identity | rejected | `own.fntype.ctor` | same |
| R42 | `iter::from_fn` | accepted | `data Generated[T, ^s]` | same |
| R43 | Frame outputs in a tuple | accepted | per-element | same |
| R44 | Ordinary and own provider keys | accepted; ordinary borrow rejected | E.4 | - |
| R45 | A scoped callback returns its borrow | rejected | fresh input lifetime | same |
| L1 | A forgotten close | rejected | `own-not-consumed` | accepted (drops) |
| L2 | Close in one branch only | rejected | `own.linear.merge` | accepted (drops) |
| L3 | Own in a loop | rejected; per-iteration accepted | `own.linear.loop` | same as hd for the first |
| L4 | Own in a `List` | accepted; `break` rejected | `into_iter` | accepted (drops) |
| L5 | `?` before any `defer` | rejected | `?` is an exit | accepted (drops) |
| L6 | A bang call before any `defer` | rejected | cancellation is an exit | accepted (drops) |
| L7 | A panic path | accepted | `own.linear.panic` | same |
| V1 | Use after move | rejected | `use-after-move` | same |
| V2 | A move in a closure, called twice | rejected | `own fn` is one-shot | same (`FnOnce`) |
| V3 | An own argument to a cold call | rejected | `own-in-cold-frame` | accepted |
| V4 | A partial move | rejected | `partial-move` | accepted |
| B1 | A parser borrowing its source | accepted | `Parser[^src]` | same |
| B2 | A borrow outlives its owner | rejected | `lifetime-mismatch` | same |
| B3 | Mutation under a shared borrow | rejected | `borrow-conflict` | same |
| B4 | Two children borrow mutably | rejected | `own.children.xor` | same |
| B5 | A closure mutates a borrowed local | **hole** | [G.3](#g3-holes) | rejected |
| B6 | A borrowed top-level binding | accepted; `^mut` rejected | [E.5](#e5-top-level-bindings-and-the-repl) | `static mut` is `unsafe` |

### G.2 Tally

| Verdict | Count | Cases |
| --- | --- | --- |
| Accepted, with any intended rejections | 60 | the rest |
| Rejected as intended | 40 | R2, R6, R13, T2, D2, N1, N6, X1 to X3, X5, X6, C01 to C04, C06, C07, C15, C17, R21 to R23, R28, R34, R38, R41, R45, L1 to L3, L5, L6, V1 to V4, B2 to B4 |
| Limit | 8 | R7, R10, R15, D8, X7, C12, C19, R32 |
| Hole | 1 | B5 |

A row counts once. A row with any limit counts as a limit, and a row
whose main form is accepted counts as accepted. Four limits (R7, R10,
D8, R32) are owned work kept in module state or in a cold frame, so
they turn on [Q8](#q8-own-arguments-to-cold-frames) and
[Q11](#q11-mutable-borrows-of-a-top-level-binding). R35 is friction:
an identity over a `^mut` needs `[own T]`.

### G.3 Holes

**B5. A closure mutates a borrowed local.** A closure captures an
ordinary local as shared storage, unchecked
([own.closure.ordinary](#b8-closures)). Calling it while a `^` borrow of
that local lives mutates a frozen place.

```text
fn freeze() -> void:
    let xs: mut List[i32] = []
    add := fn(): xs.push(+1)
    let view: ^List[i32] = xs             # hypothetical syntax
    add()                                 # not checked: mutates xs while view lives
    _ := view.len()
```

Rust rejects it: the closure holds a mutable borrow of `xs` while it
lives. A fix is [Q10](#q10-closures-over-a-borrowed-local). A closure
copied into ordinary storage loses its liveness, so any fix covers only
closures held in locals.

### G.4 Cases

The cases below show the new forms. The other rows follow from the rule
named in their row.

```text
fn branch!(bad: bool) -> void:            # L2
    c := connect!("db")
    if bad:
        c.close()                         # error: own-not-consumed: the else path keeps c

fn loop_outside!(n: usize) -> void:       # L3
    c := connect!("db")
    for _ in 0..n:
        c.close()                         # error: use-after-move: the next iteration
    pass

fn loop_inside!(n: usize) -> void:        # L3, accepted
    for _ in 0..n:
        c := connect!("db")
        defer: c.close()
        c.send!([1])
```

```text
fn pool!(hosts: List[string]) -> void:    # L4
    let conns: List[Conn] = []
    for h in hosts:
        conns.push(connect!(h))           # moves each handle in
    for c in conns.into_iter():
        c.close()                         # consumes every element

fn pool_break!(conns: List[Conn]) -> void:
    for c in conns.into_iter():
        if c.sent() == 0:
            break                         # error: own-not-consumed: c and the rest leak
        c.close()
```

```text
fn early!(host: string) -> Result[void, LinkError]:   # L5
    c := connect!(host)
    check!(c)?                            # error: own-not-consumed: ? exits with c open
    c.close()
    .Ok(())

fn twice() -> void:                       # V2
    c := connect_now("db")
    f := fn(): c.close()
    f()
    f()                                   # error: use-after-move
    c.sent()                              # error: use-after-move: f moved c

fn cold(c: Conn) -> void:                 # V3
    pending := handle(c)                  # error: own-in-cold-frame
    pending.cancel()

fn split(s: Session) -> Conn:             # V4
    s.client                              # error: partial-move
```

```text
fn bad_cursor() -> Parser[^{static}]:     # hypothetical syntax: B2
    s := open_source()
    defer: s.close()
    parser(s)                             # error: lifetime-mismatch: the parser borrows s

fn racing!(c: Conn) -> void:              # B4
    defer: c.close()
    _ := all!(write_to(c), write_to(c))   # error: borrow-conflict: two ^mut borrows of c
```

```text
fn keep_one!() -> Conn?:                  # C15 as written
    let keep: Conn? = .None
    for index in 0..2:
        c := connect!("db")
        if index == 0:
            keep = .Some(c)               # error: own-overwritten on a later pass; c leaks otherwise
    keep
```

C15 shows linearity at work: the review's program leaks the second
handle. The fixed form closes it in the `else` branch and calls
`keep.take()` before assigning.

## H. Cost

### H.1 Rule Count

The old design's 176 `ne.*` rules, by prefix:

| Old family | Prefixes | Rules |
| --- | --- | --- |
| Category, data, slots, components, variance | `data`, `var` | 30 |
| Roots, locals, heap, escapes, body checks | `root`, `local`, `heap`, `escape`, `body` | 24 |
| Syntax, parameters, signatures, elision, calls | `syntax`, `param`, `sig`, `elide`, `call` | 37 |
| Function types and closures | `fntype`, `closure` | 17 |
| Generics and erasure | `generic`, `erase` | 10 |
| Suspension and combinators | `suspend`, `all` | 13 |
| Providers, top level, annotation handles | `provider`, `top`, `handle` | 15 |
| Optimizer facts | `opt` | 8 |
| Closing, cleanup, outlives (parked) | `close`, `cleanup`, `outlive` | 22 |
| **Total** | | **176** |

This study's 94 `own.*` rules:

| Group | Prefixes | Rules |
| --- | --- | --- |
| Own types | `decl` | 6 |
| Moves and destructuring | `move` | 8 |
| Borrows and borrowed locals | `borrow`, `local` | 14 |
| Lifetimes and elision | `life`, `elide` | 13 |
| Borrows inside types | `data` | 7 |
| Methods, closures, function types | `self`, `closure`, `fntype` | 15 |
| Generics and `dyn` | `generic`, `dyn` | 6 |
| Linearity and `defer` | `linear`, `defer` | 9 |
| Suspension, children, providers | `suspend`, `children`, `provider` | 11 |
| Top level and boundary | `top`, `boundary` | 5 |
| **Total** | | **94** |

The count drops by almost half while it adds moves and linearity, which
the old design parked. Most of the cut is the slot, component,
environment-store, and frame machinery that Rust's lifetimes in types
replace. The optimizer facts need no rules of their own: a `^` parameter
is checked as never kept.

### H.2 Annotation Burden In std

Counts come from `lib/std` today. Only `std.net` holds handles.

| Change | Where | Count |
| --- | --- | --- |
| `own trait` | `TcpStream`, `TcpListener`, `UdpSocket` | 3 |
| `mut self` to `^self` | `read!`, `write!`, `accept!`, `send_to!`, `receive!` | 5 |
| `mut self` to consuming `self` | three `close` methods | 3 |
| `mut dyn K` results lose `mut` | `Net.connect!`, `listen!`, `bind_udp!` | 3 |
| `[own T]` opt-in | `Option`, `Result`, `List`, `Deque`, `Map`, `Iterator` | 6 declarations |
| New borrowing or consuming methods | `take`, `pop`, `at`, `into_iter` and the like | about 12 |
| `^self` and `^` parameters, to accept borrows | public methods over ordinary values | up to about 400 public, 860 private |

The last row is the old scan's locality marks (243 receivers and 157
parameters public). Under the draft, a borrow reaches a std method only
when that method writes `^`. They are needed only as borrows appear in
user code, and a lint with a fix-it proposes each one. [Q4](#q4-plain-parameters-of-ordinary-type) would remove
this row and add a different cost.

### H.3 Checker Cost Per Body

The checker is a Rust NLL-style dataflow over each body, using only
callee signatures.

| Step | Work | What it reuses |
| --- | --- | --- |
| Moves and linearity | A forward set of possibly-moved bindings, with its must-dual for consumption | `names.definite.*`: the possibly-uninitialized set in `crates/hd_check/src/body.rs` and its merges in `expr.rs` and `init.rs` |
| Borrow liveness | A backward liveness pass per body, iterated over loops | None in the checker; `crates/hd_wasm/src/live.rs` has the same pass shape for Wasm |
| Lifetime constraints | Subset constraints between lifetime sets, solved least | Row sets and their solver in `crates/hd_check/src/rows.rs`, as the old design planned |
| Variance | Per lifetime parameter, from the declaration | `types.polarity` |
| Conflicts | For each live borrow, test the uses of its place | New |

Cost: about `O((N + E) · ⌈L/64⌉)` per body, for `N` program points, `E`
constraints, and `L` lifetimes and borrows. This is a common-case estimate:
liveness over loops needs a fixed point. No step crosses a body, so an
edited body never invalidates another.

## I. Prior Art

| Language | What it does | What hd takes |
| --- | --- | --- |
| Rust | "Each value in Rust has an owner. There can only be one owner at a time." One mutable reference or any number of immutable ones. | Ownership, moves, the borrow rules |
| Rust, elision | Three rules: distinct input lifetimes; one input lifetime fills every elided output; `&self` fills them in methods. Eliding what cannot be inferred is an error. | [B.5](#b5-elision), unchanged |
| Rust, NLL (RFC 2094) | Lifetimes "based on the control-flow graph, rather than lexical scopes", with liveness computed for lifetimes. | `own.borrow.live` |
| Rust, statics | An `unsafe` block is required to read or write a `static mut`. | [Q11](#q11-mutable-borrows-of-a-top-level-binding) |
| Rust, `mem::forget` | Safe, because Rust does not guarantee that destructors run. Rust is affine. | The affine column of [D.1](#d1-the-two-readings) |
| Swift SE-0390 | `~Copyable` structs and enums have unique ownership. A struct with a noncopyable stored property must be noncopyable too. A class may hold one, but never consume it. | Containment ([B.1](#b1-own-types)); [Q7](#q7-own-values-in-ordinary-holders) |
| Swift SE-0390 | A noncopyable parameter must state `borrowing`, `consuming`, or `inout`. | `^T`, `^mut T`, or by value, always written |
| Swift SE-0377 | `borrowing` and `consuming` parameter modifiers. | The two by-value and borrow forms |
| Swift SE-0427 | Generic parameters are `Copyable` by default; `T: ~Copyable` suppresses it. | `[T]` and `[own T]` |
| Swift SE-0446 | `~Escapable` values cannot be stored in globals or captured by escaping closures. | The old design; replaced by borrows ([F](#f-nonescapable-removed)) |
| Mojo | "Every value has only one owner at a time." `var` takes ownership; the postfix `^` transfers. `ref [a, b]` names argument origins. | Lifetimes named by arguments. Mojo spells a move `^`, the opposite of hd's `^`. |
| Hylo | Four conventions: `let`, `inout`, `sink`, `set`. A `let` or `inout` parameter cannot escape the callee without a copy; `sink` transfers ownership. | `^`, `^mut`, and by-value own parameters |
| Austral | Types are free or linear; a type holding a linear type is linear. A linear variable is used exactly once; once in each `if` branch or in none; never inside a loop body if declared outside. | [D.2](#d2-linear-rules) |
| Austral | Borrowing is "stolen lock, stock, and barrel from Rust". | Borrows over linear values |

## J. Owner Questions

The scope question is answered ([C.1](#c1-the-decision)) and gone. Each
question below is one idea and decides nothing.

### Q1. Linear Or Affine

**Effect.** Linear: a forgotten `close` is a compile error. Affine: it
compiles and leaks the handle, since hd has no `Drop`.

1. Linear: every own value is consumed exactly once (draft).
2. Affine: consumed at most once.

```text
fn forgot!() -> void:
    c := connect!("db")
    c.send!([1])                          # linear: error: own-not-consumed
```

**Recommendation.** Linear. Without `Drop`, affine typing checks only
use-after-close, and `defer` already marks where cleanup goes.

### Q2. Where `own` Is Written

**Effect.** On the declaration, every `File` is own. On the use, as
`mut` is, any type could be owned in one position and shared in another.

1. On the declaration: `own data File` (draft).
2. On the use: `f: own File`, beside `mut File`.

```text
own data File:                            # hypothetical syntax: option 1
    fd: i32
fn archive(f: own File) -> void: f.close()   # hypothetical syntax: option 2
```

**Recommendation.** Option 1. A resource is linear by what it is; with
option 2, one plain alias of a `File` breaks every owned use of it.

### Q3. A Call-Site Move Mark

**Effect.** Without a mark, `archive(f)` moves `f` silently; the reader
learns it from `archive`'s signature or a later error.

1. No mark; the parameter type decides (draft).
2. `move f` at the call, as Mojo's `f^`.

```text
archive(f)                                # option 1
archive(move f)                           # hypothetical syntax: option 2
```

**Recommendation.** Option 1. A later use is a compile error, and hd
writes no mark for a borrow either.

### Q4. Plain Parameters Of Ordinary Type

**Effect.** Under the draft, a `^List[u8]` cannot reach `len(self)` until
std writes `len(^self)`: up to about 400 public marks.

1. No: a plain parameter is an unchecked alias and takes no borrow (draft).
2. Yes: a plain parameter of ordinary type takes a borrow, and storing it becomes an error unless marked.

```text
fn size(b: ^List[u8]) -> usize:           # hypothetical syntax
    b.len()                               # option 1: error until len takes ^self
```

**Recommendation.** Option 1. Option 2 changes the meaning of every
existing signature that stores a parameter. The marks come in phases,
proposed by a lint.

### Q5. Copying A Composite Out Of A Borrow

**Effect.** If a composite read through a borrow may become a plain
value, any borrow can be laundered into a global by one assignment.

1. Error: it stays a borrow (draft).
2. Allowed: it becomes a plain alias, and borrows of ordinary values check only the place.

```text
fn keep(b: ^List[u8]) -> List[u8]:        # hypothetical syntax
    b                                     # option 1: error: borrow-escapes
```

**Recommendation.** Option 1. Option 2 makes a `^` parameter's
"not kept" promise false.

### Q6. Mutable Borrows Inside Types

**Effect.** Rust's `struct Writer<'a> { out: &'a mut Conn }` either
works, with `Writer` moving, or is rejected.

1. Allowed: the holder moves but need not be consumed (draft).
2. Forbidden: a field or type argument holds only shared borrows.

```text
data Writer[^o]:                          # hypothetical syntax
    out: ^{o} mut Conn                    # hypothetical syntax
```

**Recommendation.** Option 1. It is Rust's model; option 2 forces
callers to pass the mutable borrow to every method.

### Q7. Own Values In Ordinary Holders

**Effect.** Today's draft makes any holder of a `File` own. Swift instead
lets a class hold one, borrowed only and never consumed.

1. Containment: the holder must be own (draft).
2. Swift's class rule: an ordinary holder may hold one; it can be borrowed, never moved or consumed.

```text
data Logger:
    file: File                            # option 1: error: missing-own
```

**Recommendation.** Option 1. Option 2 needs dynamic exclusivity checks
for the aliased holder, as Swift does at run time.

### Q8. Own Arguments To Cold Frames

**Effect.** A cold frame cancelled before its first poll runs no
`defer`, so a handle passed to it would leak.

1. Only a bang call may pass an own argument (draft).
2. Allow it, and make cancelling a never-polled frame that holds an own value a panic.

```text
pending := handle(c)                      # option 1: error: own-in-cold-frame
```

**Recommendation.** Option 1. Structured children borrow instead, and
option 2 turns a type error into a run-time failure.

### Q9. Own Values At The Top Level

**Effect.** A module or REPL may keep a connection in a top-level
binding. Nothing can consume it, so linearity does not reach it.

1. Allowed, borrow-only, released when the instance ends (draft).
2. An error: own values live only in function bodies.

```text
c := connect!("db")                       # a REPL input
c.close()                                 # option 1: error: move-out-of-top-level
```

**Recommendation.** Option 1. It matches "borrow a global as if it is
own", and a REPL session needs it.

### Q10. Closures Over A Borrowed Local

**Effect.** A closure that mutates a local can run while a `^` borrow of
that local lives, and nothing reports it ([G.3](#g3-holes)).

1. Leave it unchecked: closures over ordinary locals stay shared storage (draft).
2. A closure held in a local counts as a borrow of each local it captures, mutable when it mutates one, while the closure lives.

```text
add := fn(): xs.push(+1)
let view: ^List[i32] = xs                 # hypothetical syntax: option 2: error: borrow-conflict
```

**Recommendation.** Option 2, limited to locals: it is Rust's rule, and
plain code without `^` is still unaffected.

### Q11. Mutable Borrows Of A Top-Level Binding

**Effect.** Another function can reach `G` without going through a
`^mut G` borrow, so exclusivity across calls cannot be checked per body.
Rust requires `unsafe` to read or write a `static mut` for this reason.

1. Forbid `^mut` of a top-level binding (draft).
2. Allow it only in a body that calls nothing able to reach `G`.
3. Allow it, unchecked across calls.

```text
let JOBS: List[own fn!() -> void] = []    # hypothetical syntax

fn later(job: own fn!() -> void) -> void: # hypothetical syntax
    JOBS.push(job)                        # needs ^mut JOBS: option 1: error: borrow-conflict
```

**Recommendation.** Option 2. The check needs a call graph like the one
module initialization builds for `module.init.definite`, and a call of
a closure or a `dyn` method counts as able to reach `G`. Option 1 rules
out global work queues (R7, D8), and option 3 drops the guarantee.

## Parse Log

Every `text` block, 38 in all, was parsed on 2026-10-16 with the
compiler's parser (`scripts/hd-conformance.sh parse`, the `parse_check`
example). Each block was parsed twice: as written, and with the
hypothetical marks erased (`own`, `[own T]`, `^`, `^{...}`, lifetime
brackets, and `move`). The second parse checks that the rest of the
block is hd syntax. Parsing checks syntax only; nothing was
type-checked, and many blocks call functions the study does not define.

| Result | Count | Blocks |
| --- | --- | --- |
| Parses as written | 13 | B.2 moves; C.3 plain code; D.1 `forgot!`; E.2 cancellation; E.5 REPL inputs; G.4 cases L2/L3, L4, L5/V2/V3/V4, C15; Q1, Q7, Q8, Q9 |
| Fails as written, at a line marked `# hypothetical syntax`; parses with the marks erased | 23 | every other block except the two below |
| Fails as written at a marked line; with the marks erased, fails only at a signature written without a body | 2 | the A.3 prelude (`connect!`, `listen!`) and the B.5 elision block |

Every first failure of a block as written falls on a line marked
`# hypothetical syntax`. Every line that writes `^`, `own`, or `move`
outside a comment carries the mark. A signature without a body is a
sketch of a declaration, as in the old design's prelude, and is not hd
syntax.

## Sources

- Rust Book, What Is Ownership: <https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html>
- Rust Book, References and Borrowing: <https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html>
- Rust Book, Validating References with Lifetimes: <https://doc.rust-lang.org/book/ch10-03-lifetime-syntax.html>
- Rust Reference, Lifetime elision: <https://doc.rust-lang.org/reference/lifetime-elision.html>
- Rust Reference, Expressions (moved and copied types): <https://doc.rust-lang.org/reference/expressions.html>
- Rust Reference, Static items (mutable statics): <https://doc.rust-lang.org/reference/items/static-items.html>
- Rust RFC 2094, Non-lexical lifetimes: <https://rust-lang.github.io/rfcs/2094-nll.html>
- Rust, `std::mem::forget`: <https://doc.rust-lang.org/std/mem/fn.forget.html>
- Rust, `Option::take`: <https://doc.rust-lang.org/std/option/enum.Option.html>
- Swift SE-0390, Noncopyable structs and enums: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0390-noncopyable-structs-and-enums.md>
- Swift SE-0377, `borrowing` and `consuming` parameter ownership modifiers: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0377-parameter-ownership-modifiers.md>
- Swift SE-0427, Noncopyable generics: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0427-noncopyable-generics.md>
- Swift SE-0446, Nonescapable types: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0446-non-escapable.md>
- Mojo Manual, Ownership: <https://docs.modular.com/mojo/manual/values/ownership/>
- Mojo Manual, Lifetimes, origins, and references: <https://docs.modular.com/mojo/manual/values/lifetimes/>
- Hylo, Functions and methods (parameter passing conventions): <https://docs.hylo-lang.org/language-tour/functions-and-methods>
- Hylo, Subscripts: <https://docs.hylo-lang.org/language-tour/subscripts>
- Austral, Linear types tutorial: <https://austral-lang.org/tutorial/linear-types>
- Austral, Specification (linear types, borrowing): <https://austral-lang.org/spec/spec.html>
- hd: `spec/lang/04-type-system.md` (Views), `spec/lang/06-control-flow.md`
  (Deferred Cleanup), `spec/lang/07-functions.md` (Function Types,
  Captures, Methods), `spec/lang/10-modules.md` (Wasm Boundary, Module
  Initialization), `spec/lang/11-requirements-and-suspension.md`
  (Providers, Suspending Functions, Cancellation), `spec/cli/command-line.md`
  (REPL), `lib/std/net.hd`.
