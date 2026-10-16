# Ownership And Borrows: Design Study

## Table Of Contents

- [A. Status](#a-status)
- [B. Core Model](#b-core-model)
- [C. Borrows And Plain Values](#c-borrows-and-plain-values)
- [D. Linear Versus Affine](#d-linear-versus-affine)
- [E. Suspension, Concurrency, And The Top Level](#e-suspension-concurrency-and-the-top-level)
- [F. NonEscapable Removed](#f-nonescapable-removed)
- [Use Cases Against NonEscapable](#use-cases-against-nonescapable)
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

This revision writes `own` at the usage site, on the type at each
position, as `mut` is written. The first version put `own` on the type
declaration (`own data File`); the owner rejected that on 2026-10-16.

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

Answers and corrections followed on 2026-10-16, verbatim:

| Time | Owner's words | Lands in |
| --- | --- | --- |
| about 12:10 | "i think brwchk should check all ^ local var" | [C](#c-borrows-and-plain-values) |
| about 12:15 | "you can also borrow a global/static variable as if it is own" | [E.5](#e5-top-level-bindings-and-the-repl) |
| about 12:45 | "one thing, you are using own at declaration site, i'm thinking own at usage site, like mut. i have expressed that before and you have not caught that. redo" | [B.1](#b1-owned-positions) |

About 13:00, the owner answered five questions on the usage-site
model, verbatim:

| # | Question | Owner's words | Lands in |
| --- | --- | --- | --- |
| 1 | May an `own T` be passed where a plain `T` is expected? | "no, type error" | [own.no-plain](#b2-where-owned-values-come-from) |
| 2 | May a plain value become `own`? | "yes, data User: ...; a plain vdata, let own user = User {...} should be allowed, but all its field should be owned unique as well" | [B.2](#b2-where-owned-values-come-from) |
| 3 | How does a resource type's author guarantee its values are always owned? | "they don't. if you disagree, give me counter examples" | [B.13](#b13-how-a-resource-stays-owned) |
| 4 | What stops two aliases of a plain holder from consuming its `own` field? | "they cannot? you can only consume own T once." | [own.move.path](#b3-moves) |
| 5 | Can a type parameter be instantiated with `own Conn`? | "no, same as mut" | [B.11](#b11-generics-and-dyn) |

### A.2 Decisions This Study Builds On

| Decision | Source |
| --- | --- |
| The model follows Rust: ownership, moves, first-class borrows with lifetimes, borrows inside types. | owner, 2026-10-16, A.1 |
| `own` marks non-copyable values with linear semantics; using one by value consumes it. | owner, 2026-10-16, A.1 |
| `own` is written at the usage site, like `mut`: on the type at a position (`c: own Conn`, `-> own Conn`, `let c: own Conn`, `field: own Conn`), never on a declaration. | owner, 2026-10-16, about 12:45, A.1 |
| An `own T` where a plain `T` is expected is a type error. | owner, 2026-10-16, about 13:00, answer 1 |
| A fresh plain value may become owned, as `let own user = User {...}`, and all its fields are then owned and unique too. | owner, 2026-10-16, about 13:00, answer 2 |
| No declaration-level rule guarantees that a resource is always owned. | owner, 2026-10-16, about 13:00, answer 3 |
| An `own` value is consumed once: consuming a field needs an owned path to it. | owner, 2026-10-16, about 13:00, answer 4 |
| `own` in type arguments follows hd's rule for `mut`. | owner, 2026-10-16, about 13:00, answer 5 |
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

The orchestrator recommended two readings that are still pending:
linear, and no call-site move mark. The draft uses them and asks each in
[J](#j-owner-questions).

> **Note.** The orchestrator's summary of answer 5 said that a generic
> that wants owned values "writes `own T`". hd's rule for `mut` forbids
> `mut T` in an unconstrained generic and uses a bound instead
> (`types.generic.no-mut-t`, `types.generic.mut-bound`). The draft
> mirrors the spec rule ([B.11](#b11-generics-and-dyn)).

### A.3 Conventions

Examples are `text` blocks. A line that uses syntax no chapter specifies
carries a comment that starts `# hypothetical syntax`. A line marked
`# error:` gives the diagnostic the proposed checker would report.
Examples leave out the consumption that [D](#d-linear-versus-affine)
requires unless it is the point. The [Parse Log](#parse-log) records a
syntax-only parse; nothing is type-checked.

The examples share this prelude. The I/O methods take `^self`, as Rust
implements `Write` for `&TcpStream`; `close` consumes. No declaration
carries `own`: the producers return `own`, and `close` takes `own self`.

```text
trait Socket:
    fn send!(^self, bytes: List[u8]) -> void   # hypothetical syntax
    fn read!(^self) -> string                  # hypothetical syntax
    fn sent(^self) -> usize                    # hypothetical syntax
    fn close(own self) -> void                 # hypothetical syntax

trait Acceptor:
    fn accept!(^self) -> own Conn              # hypothetical syntax
    fn close(own self) -> void                 # hypothetical syntax

type Conn = dyn Socket
type Listener = dyn Acceptor

data File:
    fd: i32

impl File:
    fn close(own self) -> void: sys_close(self.fd)   # hypothetical syntax

fn connect!(host: string) -> own Conn          # hypothetical syntax
fn listen!(host: string) -> own Listener       # hypothetical syntax
```

### A.4 Spec Impact

A later, owner-ordered spec edit would touch these rules.

| Spec rule | Change |
| --- | --- |
| `grammar.type.mut`, `grammar.type.dyn.mut` | `own` joins `mut` as a type modifier: `own mut T`, `own dyn K`. |
| `grammar.data.mut-type` | A field may declare `own T`, as it declares `mut T`. |
| `names.self.mut`, `fn.method.no-sigil` | Gain `own self`, `own mut self`, `^self`, and `^mut self`. |
| `types.prim.no-mut`, `types.tuple.no-mut` | Gain twins for `own`: `own-on-value`, `own-on-tuple`. |
| `types.assign`, `types.mut.weaken` | No rule converts `own T` to `T`; `types.mut.no-upgrade` gains a twin for `own`. |
| `types.bind.short`, `types.bind.let-mut*` | `:=` keeps `own`; `let own` mirrors `let mut`. |
| `types.generic.mut-argument`, `types.generic.no-mut-t`, `types.generic.mut-bound` | Gain twins for `own`. |
| `types.mut.permission` | Unchanged: `mut` is a permission; `own` is the ownership modifier. |
| `fn.type.no-mut` | `own fn(...)` is valid, unlike `mut fn(...)`. |
| `fn.type.no-names` | A function type may name its parameters. |
| `fn.capture.storage.shared` | Holds for plain bindings only; owned and borrowed captures move or borrow. |
| `types.composite.aliases` | Holds for plain positions only; an owned value has one owner. |
| `req.suspend.type.equivalent` | The lowered frame type carries a lifetime bound. |
| `req.discard.cold` | A frame that holds an owned value cannot be discarded. |
| `module.boundary.not-safe` | Adds owned positions and types that carry a lifetime. |
| `names.definite.*` | Moves reuse the possibly-uninitialized set. |

## B. Core Model

A type no longer decides whether a value aliases, moves, or must be
consumed. The **mode** written at each position decides, and one type,
such as `File` or `List[u8]`, may be plain in one position and owned in
another.

| Mode | Readonly | Mutable | Copying the value | Must be consumed | Borrow-checked |
| --- | --- | --- | --- | --- | --- |
| Plain | `T` | `mut T` | aliases, as today | no | only through a written `^` |
| Owned | `own T` | `own mut T` | moves | yes (draft: linear) | always |
| Borrowed | `^T` | `^mut T` | `^T` copies; `^mut T` moves | no | always |

### B.1 Owned Positions

```text
data Logger:
    file: own File                             # hypothetical syntax: an owned field
    name: string

fn archive(f: own File) -> void:               # hypothetical syntax: takes ownership
    f.close()

fn open_file!(path: string) -> own mut File:   # hypothetical syntax: gives ownership
    File { fd: sys_open!(path) }

fn demo!() -> void:
    f := open_file!("log")                     # own File: := keeps own
    let files: own List[own File] = []         # hypothetical syntax
    let pair: (own File, i32) = (open_file!("b"), +1)   # hypothetical syntax
    let n: own i32 = +0                        # hypothetical syntax: error: own-on-value
    let g: mut own File = open_file!("c")      # hypothetical syntax: error: syntax-error
```

1. r[own.mod] `own` is a type modifier, as `mut` is (`grammar.type.mut`). `own T` at a position holds the only owner of its value. (owner, 2026-10-16, about 12:45: "own at usage site, like mut")
2. r[own.mod.positions] `own` may be written on a parameter, a result, a local's type, a field, an enum payload, a type argument, and a receiver. A type declaration carries no `own`. (owner, 2026-10-16, about 12:45)
3. r[own.mod.order] `own` comes before `mut` and `dyn`: `own mut T`, `own dyn K`, `own mut dyn K`. Any other order is an error. Error: `syntax-error`. (agent, as `grammar.type.dyn.mut`)
4. r[own.mod.mut] `own T` is a readonly owner, and `own mut T` an owner with mutable access. (agent; see [Q12](#q12-own-and-mut))
5. r[own.mod.value] `own` on a primitive type, `string` included, is an error, since the value is copied. Error: `own-on-value`. (agent, as `types.prim.no-mut`)
6. r[own.mod.tuple] A tuple type has no `own` form; each element keeps its own mode, as in `(own File, i32)`. Error: `own-on-tuple`. (agent, as `types.tuple.no-mut`)
7. r[own.mod.optional] `own T?` is an optional owned `T`, since optionality applies to the whole access type (`grammar.type.optional`). (agent)
8. r[own.mod.nested] `own own T`, `own ^T`, and `^own T` are errors. Error: `own-misplaced`. (agent, as `grammar.type.mut.semantic`)
9. r[own.mod.fn] `own fn(...)` is a one-shot function type ([B.10](#b10-function-types)). Unlike `mut`, `own` may precede `fn`. (agent)

> **Why.** `mut` already works this way: a permission written on the
> type at each position, with no category on the declaration. `own`
> takes the same places, and its rejected forms mirror `mut`'s, except
> that a function type may be owned.

### B.2 Where Owned Values Come From

```text
data User:
    name: string
    tags: List[string]

fn log(c: Conn) -> void: pass

fn demo!(shared: List[string]) -> void:
    let own a = User { name: "Ada", tags: ["x"] }      # hypothetical syntax: fresh parts
    let own b = User { name: "Lin", tags: shared }     # hypothetical syntax: error: own-upgrade
    plain := User { name: "Bo", tags: shared }         # plain, as today
    let own c = plain                                  # hypothetical syntax: error: own-upgrade
    conn := connect!("db")                             # own Conn: the result is declared own
    log(conn)                                          # error: own-to-plain
```

1. r[own.source] An owned value comes only from four places: a fresh value (a data literal, an enum construction, a list or map expression) used where an `own T` is expected; a call whose result is declared `own T`; an `own` parameter; and a move of an owned place. (owner, 2026-10-16, about 13:00, answer 2; agent: the list)
2. r[own.no-upgrade] Any other plain value used where an `own T` is expected is an error. Error: `own-upgrade`. (owner, answer 2; as `types.mut.no-upgrade`)
3. r[own.no-plain] An owned value used where a plain `T` is expected is an error. Error: `own-to-plain`, with fix-its that make the position `^T` or `own T`. (owner, answer 1: "no, type error")
4. r[own.let] `let own name = value` binds an owned local, and its initializer is used as `own T`. (owner, answer 2; as `types.bind.let-mut-expected`)
5. r[own.bind.infer] `:=` and an unannotated `let` keep `own` from an owned initializer, since dropping it would be `own-to-plain`. A fresh value under `:=` stays plain, by `types.bind.short`. (agent)
6. r[own.result] A call declared to return a plain `T` never gives an owned value, even when its body builds a fresh one. (agent, as `types.bind.call-result`)
7. r[own.unique] Every composite part of an owned value is held uniquely, recursively, whatever its field's declared type. (owner, answer 2: "all its field should be owned unique as well")
8. r[own.unique.init] So in a fresh value used as `own T`, each composite field value must be fresh or moved from an owned place. A plain alias is an error. Error: `own-upgrade`. (owner, answer 2; agent: the reading)
9. r[own.unique.exempt] A field value of primitive type, `string` included, is exempt, since sharing it is harmless. (agent; see [Q13](#q13-primitive-field-values-in-an-owned-value))
10. r[own.unique.store] Assigning a composite field through an owned path follows the same rule. Error: `own-upgrade`. (agent)
11. r[own.unique.read] Reading a composite field through an owned path gives an owned place. It may be borrowed or moved ([B.3](#b3-moves)), and passing it to a plain position is `own-to-plain`. (agent)

> **Why.** `own` keeps one meaning, the only path to a value. A plain
> alias may have siblings, so it never becomes owned, and an owned value
> never becomes plain, since a plain copy would be a second path. A fresh
> value has no other path yet, so it may start owned, and so must every
> composite value inside it.

### B.3 Moves

```text
data Pair:
    a: own File                    # hypothetical syntax
    b: own File                    # hypothetical syntax

fn demo!() -> void:
    f := open_file!("log")
    archive(f)                     # moves f
    archive(f)                     # error: use-after-move

fn split(p: own Pair) -> own File: # hypothetical syntax
    p.b.close()                    # consumes one field
    p.a                            # moves the other out: p is used up

fn steal(p: Pair) -> own File:     # hypothetical syntax
    p.a                            # error: move-out-of-plain
```

1. r[own.move] Using an owned place by value moves it. That covers an argument for an `own` parameter, a return, an assignment, a field or element store, and a capture that the closure consumes. (owner, 2026-10-16: "moving `own T` is consuming")
2. r[own.move.after] After a move the place is uninitialized, and a use is an error. Error: `use-after-move`. (agent; reuses `names.definite.reject`)
3. r[own.move.reinit] Assigning a moved `let` binding or field initializes it again. (agent)
4. r[own.move.overwrite] Assigning a binding or field that still holds an owned value is an error, because the old value would be lost. Error: `own-overwritten`. (agent, follows from linearity)
5. r[own.move.no-mark] A move has no mark at the call site; the parameter's mode decides. (agent; see [Q3](#q3-a-call-site-move-mark))
6. r[own.move.path] Moving or consuming a place requires every step of its path to be owned: an owned root, and an owned holder at each field on the way. Error: `move-out-of-plain`. (owner, answer 4: "you can only consume own T once"; agent: the reading)
7. r[own.move.plain-holder] So through a plain holder, an `own` field can only be borrowed. (owner, answer 4, reading)
8. r[own.move.partial] Through an owned path, one field may be moved out alone. The holder is then partly moved: it cannot be used whole until the field is assigned again, and its other owned fields must still be consumed. (agent, as Rust's partial moves; follows the reading of answer 4)
9. r[own.move.destructure] A pattern that binds every field of an owned data value, or a match on an owned enum, consumes it. Each field becomes a binding with the field's mode; under `own.unique`, a composite field binds owned. (agent)
10. r[own.move.visible] Moving a private field out, alone or by a pattern, is valid only in the type's module. Error: `private-member`. (agent)

> **Why.** Two aliases of a plain `Pair` cannot both consume `p.a`,
> because neither can consume it at all: the plain step in the path
> leaves only a borrow. An owned `Pair` has one path, so moving one field
> out is safe, as Rust's partial moves are without `Drop`.

### B.4 Borrows

```text
fn peek(f: ^File) -> i32:                    # hypothetical syntax
    f.fd

fn reset(f: ^mut File) -> void:              # hypothetical syntax
    f.fd = 0

fn demo!() -> void:
    let f: own mut File = open_file!("log")  # hypothetical syntax
    defer: f.close()
    _ := peek(f)                             # a shared borrow for the call
    reset(f)                                 # a mutable borrow for the call
    let view: ^File = f                      # hypothetical syntax: a shared borrow
    reset(f)                                 # error: borrow-conflict
    _ := view.fd
```

1. r[own.borrow.form] `^T` is a shared borrow and `^mut T` a mutable borrow. A borrow refers to a place that it does not own. (owner, 2026-10-16)
2. r[own.borrow.make] A borrow is made where a place is passed to a `^` or `^mut` parameter or receiver, or bound to a local written `^T`. No expression makes one. (agent)
3. r[own.borrow.mut-place] `^mut` needs a place with mutable access: an `own mut` place, or a plain place with a `mut` view. Error: `mutable-upgrade`. (agent)
4. r[own.borrow.xor] While a shared borrow of a place is live, the place must not be assigned, mutated, mutably borrowed, moved, or consumed. Error: `borrow-conflict`. (owner, 2026-10-16: "follow Rust")
5. r[own.borrow.xor.mut] While a mutable borrow of a place is live, the place must not be used except through that borrow. Error: `borrow-conflict`. (owner, 2026-10-16)
6. r[own.borrow.live] A borrow is live from where it is made to the last use of any value whose type names its lifetime, as in Rust's non-lexical lifetimes. (agent)
7. r[own.borrow.copy] A shared borrow copies. A mutable borrow moves, and passing it to a `^mut` parameter reborrows it for the call. (agent, as Rust)
8. r[own.borrow.read] Reading a field or element through a borrow gives a borrow of that part, with the same lifetime. A primitive read through a borrow is copied out. (agent)
9. r[own.borrow.no-copy-out] A composite value read through a borrow must not flow into a plain position. Error: `borrow-escapes`. (agent; see [Q5](#q5-copying-a-composite-out-of-a-borrow))
10. r[own.borrow.no-move-out] Moving or consuming through a borrow is an error. Error: `move-out-of-borrow`. (agent)
11. r[own.borrow.two-phase] A `^mut` borrow of a call's receiver starts when the call runs, after its arguments are evaluated, so `xs.push(xs.len())` is valid. (agent, as Rust's two-phase borrows)

> **Why.** hd has no `&` expression, so the signature or the local's
> written type makes the borrow, as Rust's method calls auto-borrow
> their receiver. The owner's principle still holds: the `^` is in the
> source, on the parameter or the local.

### B.5 Lifetimes Named By Variables

```text
fn longer(a: ^Conn, b: ^Conn) -> ^{a, b} Conn:     # hypothetical syntax
    if a.sent() > b.sent(): a else: b

fn keep_in(dst: ^mut List[^Conn], c: ^{dst.T} Conn) -> void:   # hypothetical syntax
    dst.push(c)

fn dangle(c: own Conn) -> ^{c} Conn:                # hypothetical syntax
    c                                               # error: borrow-of-local
```

1. r[own.life.name] A lifetime is written in braces after `^`, as `^{a}` or `^{a, b}`. An atom names a parameter, `self`, a path, a requirement key `$K`, or `static`. (owner, 2026-10-16: "variable name as lifetime"; agent: the atoms)
2. r[own.life.path] A path names one lifetime inside a parameter's type: `p.src` for a lifetime parameter `src`, `xs.T` for type parameter `T`, `t._0` for a tuple element. (agent; the old round-6 paths and B8)
3. r[own.life.param] Each borrow in a parameter's type has its own lifetime, named by the parameter or a path through it. (agent, as Rust's distinct input lifetimes)
4. r[own.life.meaning] A value bounded by `^{a}` is usable only while `a`'s lifetime lasts. `^{a, b}` lasts while both last. (agent, as one Rust `'a` shared by both inputs)
5. r[own.life.static] `static` is the lifetime of a top-level binding and of every value that holds no borrow. (agent)
6. r[own.life.no-local] A result must not name a parameter that carries no lifetime, such as an `own` parameter, since its value dies with the call. Error: `borrow-of-local`. (agent, as Rust E0515)
7. r[own.life.store] A borrow stored into another parameter's place writes the destination on the stored value, as `c: ^{dst.T} Conn`. Error: `lifetime-mismatch`. (owner, 2026-10-16, round-7 follow-up 5, carried over)
8. r[own.life.check] Every value that flows into a result, a store, or a binding must have a lifetime that lasts as long as the position needs. Error: `lifetime-mismatch`. (agent)

What a lifetime mark means depends on where it stands:

| Position | Bare `^` | `^{a}` |
| --- | --- | --- |
| Parameter | A borrow with a fresh lifetime, named by the parameter | A store into `a`'s place ([own.life.store](#b5-lifetimes-named-by-variables)) |
| Result | Elided ([B.6](#b6-elision)) | Bounded by `a` |
| Field | Not allowed: name a lifetime parameter | Names a lifetime parameter |
| Type argument | As its position | As its position |
| Local written type | A borrow of the initializer, lifetime inferred | Bounded by `a` |

### B.6 Elision

```text
fn first(pool: ^List[Conn]) -> Option[^Conn]       # hypothetical syntax: elided to pool
fn pick(^self) -> ^Conn                             # hypothetical syntax: elided to self
fn either(a: ^Conn, b: ^Conn) -> ^Conn              # hypothetical syntax: error: ambiguous-lifetime
fn open!(host: string) -> own Conn                  # hypothetical syntax: owned: no lifetime
```

1. r[own.elide.one] A bare `^` in a result, with exactly one input lifetime, takes that lifetime. (Rust's rule, as the reference states it)
2. r[own.elide.self] In a method whose receiver is `^self` or `^mut self`, a bare `^` in the result takes the receiver's lifetime. (Rust's rule)
3. r[own.elide.ambiguous] Any other bare `^` in a result is an error. Error: `ambiguous-lifetime`. (Rust's rule)
4. r[own.elide.inputs] Input lifetimes are counted over every borrow in the parameters and lifetime arguments. A requirement key never counts; a result bounded by one names it, as `^{$Pool}`. (agent; the first version counted the keys of own traits, which no longer exist)
5. r[own.elide.owned] A result written without `^` is owned or plain and carries no lifetime. (agent, as Rust)

> **Why.** These are Rust's three elision rules, unchanged. The old design
> needed its own table because a value without `^` could still be
> local. Now a value without `^` is owned or plain, so it never
> borrows, and the old special cases fall away.

### B.7 Borrows Inside Types

```text
data Source:
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

### B.8 Methods And `self`

1. r[own.self.forms] A receiver is `self`, `mut self`, `own self`, `own mut self`, `^self`, or `^mut self`: shorthand for `self` typed with that mode on `Self`. (agent, as `names.self.mut`)
2. r[own.self.consume] An `own self` method consumes its receiver, so a call moves it, by [own.move.path](#b3-moves). (owner direction: "moving `own T` is consuming")
3. r[own.self.borrow] A call of a `^self` or `^mut self` method borrows the receiver's place for the call, whatever its mode. (agent)
4. r[own.self.plain] A `self` or `mut self` method called on an owned receiver is `own-to-plain`. (owner, answer 1, applied to receivers)
5. r[own.self.upgrade] An `own self` method called on a plain receiver is `own-upgrade`. (owner, answer 2, applied to receivers)

### B.9 Closures

```text
fn closer(c: own Conn) -> own fn() -> void:  # hypothetical syntax
    fn() -> void: c.close()                  # moves c in: the body consumes it

fn counter(c: ^Conn) -> ^{c} fn() -> usize:  # hypothetical syntax
    fn() -> usize: c.sent()                  # captures a borrow

fn demo!() -> void:
    c := connect!("db")
    done := closer(c)
    done()                                   # consumes done
    done()                                   # error: use-after-move
```

1. r[own.closure.move] A closure whose body consumes an owned capture moves it in. The closure's type is `own fn(...)`. (owner brief; agent: the spelling)
2. r[own.closure.borrow] A closure that only borrows an owned capture borrows the binding, shared or mutably, while the closure lives. Its type is bounded by that borrow. (agent, as Rust's capture inference)
3. r[own.closure.borrow-capture] A closure that captures a borrow is bounded by the borrow's lifetime, as `^{c} fn() -> usize`. (agent)
4. r[own.closure.once] Calling an `own fn` consumes it. Its body must consume each owned capture ([D](#d-linear-versus-affine)). (agent)
5. r[own.closure.plain] A capture of a plain binding stays shared storage, unchecked, as `fn.capture.storage.shared` says today. (agent; see [Q10](#q10-closures-over-a-borrowed-local))
6. r[own.closure.no-self] A closure that holds a `^mut` borrow has no self-name, so it never runs inside its own call. (agent)

### B.10 Function Types

1. r[own.fntype.names] A function type may name its parameters, as `fn(c: ^Conn) -> ^{c} Conn`. `fn.type.no-names` must change. (carries the old D8)
2. r[own.fntype.elide] A function type's result elides by [B.6](#b6-elision). For a parameter `f`, `{f}` names its environment. (carries D8)
3. r[own.fntype.fresh] An input `^T` of a function type gets a fresh lifetime at each call, as Rust's higher-ranked `for<'a>`. (agent)
4. r[own.fntype.bound] `^{a} fn(...)` bounds the environment by `a`. An unmarked function type holds no borrow. (agent)
5. r[own.fntype.own] `own fn(...)` is a one-shot function type ([B.9](#b9-closures)). A function item has type `fn(...)`. (agent)
6. r[own.fntype.ctor] `Fn[...]` and `SuspendFn[...]` spell only function types with no borrow, no names, and no `own`. A mark inside one is an error. Error: `misplaced-lifetime`. (owner, 2026-10-16, NE-S5c and round-7 follow-up 8, carried)

### B.11 Generics And `dyn`

The owner's answer 5, "no, same as mut", points at hd's rules for `mut`
in generics (`spec/lang/04-type-system.md`, Generics and Mutable
Bounds):

| Rule for `mut` | Text | Twin for `own` |
| --- | --- | --- |
| `types.generic.mut-argument` | "A type parameter `T` may therefore be instantiated with either `User` or `mut User`." | [own.generic.arg](#b11-generics-and-dyn) |
| `types.mut.generic-field` | A generic field `P` keeps its substituted type when `P = mut U`. | [own.generic.field](#b11-generics-and-dyn) |
| `types.generic.no-mut-t` | "An unconstrained generic declaration must not write `mut T`." | [own.generic.no-own-t](#b11-generics-and-dyn) |
| `types.generic.mut-any`, `types.generic.mut-trait` | `T < mut Any` accepts any mutable-root type; `T < mut Trait` also requires `Trait`. | [own.generic.bound](#b11-generics-and-dyn) |
| `types.generic.direct` | "An unconstrained generic declaration stores or passes `T` directly." | [own.generic.direct](#b11-generics-and-dyn) |

```text
trait Closer:
    fn close(own self) -> void                 # hypothetical syntax

fn same[T](v: T) -> T: v                       # T may be own Conn: v passes through
fn first[T](items: ^List[T]) -> Option[^T]:    # hypothetical syntax: elided to items
    items.at(0)
fn close_all[T < own Closer](xs: own List[T]) -> void:   # hypothetical syntax
    for x in xs.into_iter():
        x.close()                              # consumes each T
fn bad[T](v: own T) -> void: pass              # hypothetical syntax: error: own-on-type-parameter
fn inspect(x: dyn Inspectable) -> void: pass
```

1. r[own.generic.arg] A type parameter `T` may be instantiated with `Conn` or `own Conn`, as `List[own Conn]`. (owner, answer 5: "same as mut"; as `types.generic.mut-argument`)
2. r[own.generic.field] A generic field declared `P` keeps its substituted type when `P = own U`. Moving it out still needs an owned path ([own.move.path](#b3-moves)). (agent, as `types.mut.generic-field`)
3. r[own.generic.no-own-t] An unconstrained generic must not write `own T`. Error: `own-on-type-parameter`. (owner, answer 5; as `types.generic.no-mut-t`)
4. r[own.generic.bound] Owned requirements use a bound: `T < own Any` accepts any owned type, and `T < own Trait` also requires `Trait`. Such a body may consume `T` values. (owner, answer 5; as `types.generic.mut-any` and `types.generic.mut-trait`)
5. r[own.generic.direct] An unconstrained generic body stores or passes `T` directly. (`types.generic.direct`, carried; see hole [B8](#g3-holes) and [Q15](#q15-an-unconstrained-body-and-an-owned-t))
6. r[own.generic.infer] Inference never makes `T` owned from a plain argument, and `own X` and `X` do not meet: no weakening joins them. Error: `type-mismatch`. (agent, as `types.mut.no-upgrade.inference` and `types.generic.infer.join`)
7. r[own.generic.borrow] `^T` with `T = own U` is `^U`: a borrow does not own. (agent)
8. r[own.generic.store] A generic body cannot store a `T` in module state, and there is no `T: static` bound. (owner, 2026-10-16, round-7 follow-up 2, carried)
9. r[own.dyn.own] `own U` converts to `own dyn K` when `U` implements `K`, as `types.assign.trait-value.mut` builds `mut dyn K`. (agent)
10. r[own.dyn.bound] `dyn K` holds no borrow. A value that holds a borrow converts to `^{a} dyn K` only. Error: `lifetime-mismatch`. (agent, as Rust's default `'static` object bound)
11. r[own.dyn.downcast] A downcast needs a `static` lifetime. Error: `downcast-non-static`. (owner, 2026-10-14, Q6, carried)

> **Note.** The twin of `types.generic.mut-argument` is sound for `mut`
> because a body that does not know `T` is mutable simply never mutates
> it. A body that does not know `T` is owned may copy it, and a copy of
> an owned value is a second owner. That is hole
> [B8](#g3-holes).

### B.12 `Option`, `Result`, And `List` Of Owned Values

These are stdlib-tier items; the language tier needs only
[own.generic.arg](#b11-generics-and-dyn).

```text
impl[T] List[T]:
    fn push(^mut self, v: T) -> void              # hypothetical syntax: moves v when T is owned
    fn pop(^mut self) -> T?                       # hypothetical syntax
    fn at(^self, index: usize) -> Option[^T]      # hypothetical syntax
    fn into_iter(own self) -> own Iterator[T]     # hypothetical syntax

impl[T] T?:
    fn take(^mut self) -> T?                      # hypothetical syntax
```

- `List`, `Option`, `Result`, `Deque`, and `Map` keep their declarations:
  `List[own Conn]` instantiates `T = own Conn`.
- `push`, `pop`, and `take` move an element in or out through `^mut`.
  They are intrinsics, so the move inside their bodies is not a user
  move out of a borrow.
- `into_iter` consumes an owned list, and a loop over it consumes each
  element ([D.2](#d2-linear-rules)).
- A plain `List[own Conn]` holds elements that can only be borrowed
  ([own.move.path](#b3-moves)). Hole [B7](#g3-holes) is how such a list
  arises.
- Methods that copy a `T` out, such as `get`, `unwrap_or`, and indexing,
  copy an owned element when `T = own Conn`: hole [B8](#g3-holes).
- A call result that carries an owned payload is written owned at both
  levels, as `-> own Result[own Conn, LinkError]`, since `?` moves the
  payload out and [own.move.path](#b3-moves) needs every step owned.

### B.13 How A Resource Stays Owned

The owner's answer 3, about 13:00, verbatim: "they don't. if you
disagree, give me counter examples".

**Reading.** No declaration-level rule exists. A resource stays owned
because every producer returns `own T`, as `connect!` and `accept!`
return `own Conn`, and because [own.no-plain](#b2-where-owned-values-come-from)
lets no owned value become plain. The table lists the counter-examples
considered. A row marked **breaks** really breaks the guarantee; no rule
is added for it here.

| # | Case | Outcome |
| --- | --- | --- |
| 1 | The author's function returns a plain `Conn` made from a producer: `fn wrap!() -> Conn: connect!("db")` | Rejected: `own-to-plain`. Holds. |
| 2 | The author's function returns a plain `Conn` it builds itself, such as a test fake | Accepted. It is a plain value, which cannot call `close(own self)`, and no host handle is involved. Holds. |
| 3 | A host or std method declared `-> Conn` instead of `-> own Conn` | The handle is plain from birth: usable through borrows, never closable, so it leaks. **Breaks**, by the declaring author's mistake. |
| 4 | Public fields: `let own f = File { fd: 3 }` in another module | Accepted, since a fresh literal may be owned (answer 2). Any module can forge an owner of fd 3 and close it. **Breaks** for a type whose fields are public; private fields stop it (`private-member`). |
| 5 | `@derive(Default)` on `File` | The derived value is fresh, so `let own f = File.default()` forges an owner, as row 4. **Breaks** when the author derives it for a handle type. |
| 6 | `@derive(Deserialize)` on `File` | As row 5, with the fd taken from input. **Breaks** when the author derives it; `own.boundary` does not cover a type whose fields are plain `i32`. |
| 7 | Generic code: `fn same[T](v: T) -> T` with `T = own Conn` | Holds: `v` is passed through. A generic body that copies `v` does not hold: **breaks**, hole [B8](#g3-holes). |
| 8 | An owned handle moved into a plain holder by a literal | The handle can be borrowed, never consumed, so it leaks. **Breaks** linearity, not uniqueness: hole [B7](#g3-holes). |
| 9 | Erasure to a plain trait value: `let d: dyn Display = c` | Rejected: `own-to-plain`. Holds. |
| 10 | A closure that captures an owned handle | It borrows the handle or moves it in ([B.9](#b9-closures)). Holds. |
| 11 | A top-level binding that holds an owned handle | Never consumed; released when the instance ends ([E.5](#e5-top-level-bindings-and-the-repl)). By design ([Q9](#q9-owned-values-at-the-top-level)). |
| 12 | A panic path | Not consumed; the instance is poisoned (`own.linear.panic`). By design. |

Rows 3 to 6 are choices of the type's own author, and rows 7 and 8 are
open holes with owner questions ([Q15](#q15-an-unconstrained-body-and-an-owned-t),
[Q14](#q14-an-owned-value-moved-into-a-plain-holder)).

## C. Borrows And Plain Values

### C.1 The Decision

The owner, 2026-10-16, about 12:10, verbatim: "i think brwchk should
check all ^ local var".

**Reading.** The borrow checker applies to every `^` and `^mut` borrow of
a local variable, whatever its mode, owned or plain. Plain aliasing
without `^`, such as `let b = a` or passing a plain parameter, stays
unchecked, as today. So writing `^` triggers the check, not `own`. An
owned value is always moved or borrowed, so it is always checked.

| Code | Checked? |
| --- | --- |
| `let view: ^List[i32] = xs` | yes: `xs` is frozen while `view` lives |
| `fill(buf)` with `fill(b: ^mut List[u8])` | yes: `buf` is exclusive for the call, and `b` is not kept |
| `fn remember(b: ^List[u8])` storing `b` | yes: `b` must not outlive its lifetime |
| `alias := xs`, `f(xs)` with `f(xs: List[i32])` | no: plain GC aliasing |
| A closure that captures `xs` | no ([Q10](#q10-closures-over-a-borrowed-local)) |
| Any use of an owned binding | yes |

### C.2 A Borrow Of A Plain Local

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

1. r[own.local.freeze] While a shared borrow of a plain local lives, the local must not be assigned or used through a `mut` view. Error: `borrow-conflict`. (owner, 2026-10-16: "check all ^ local var"; agent: the reading)
2. r[own.local.exclusive] While a mutable borrow of a plain local lives, the local must not be used except through it. Error: `borrow-conflict`. (owner, 2026-10-16; agent: the reading)
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

> **Note.** For an owned value, the place is the only path, so exclusivity
> holds for the object. For a plain value, it holds for the place,
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

Today's shared-mutable code compiles unchanged: no `^`, no `own`, no
check. A borrow appears only where a signature or a local's type writes
`^`, and an owned value only where a position writes `own` or a producer
returns it. One plain pattern changes: a provider read by `$.use(K)` is
now a borrow ([E.4](#e4-providers)).

### C.4 What A Borrow Costs Callers

A borrow is never passed to a plain parameter, by
[own.local.not-kept](#c2-a-borrow-of-a-plain-local), and an owned value
never is either, by [own.no-plain](#b2-where-owned-values-come-from). So a
`^List[u8]`, or an `own List[u8]`, reaches `len` only when `len` takes
`^self`. The std scan in [H.2](#h2-annotation-burden-in-std) prices
this, and [Q4](#q4-plain-parameters-and-borrows) asks whether a plain
parameter should accept a borrow implicitly.

## D. Linear Versus Affine

### D.1 The Two Readings

| | Linear (draft) | Affine |
| --- | --- | --- |
| An owned value | is consumed exactly once on every path | is consumed at most once |
| A forgotten `close` | an error at the scope's end | silently leaks the handle |
| `defer: c.close()` | consumes `c` at block exit | consumes `c` at block exit |
| Cleanup without `defer` | an error | no cleanup |
| `let own user = User {...}` | must be destructured or passed on | may just end |
| Rust | no: `mem::forget` is safe | yes, with `Drop` |
| Austral | yes | no |

Under affine typing, an owned value that goes out of scope just ends.
With no `Drop` (standing decision), nothing closes it. So affine hd would
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

1. r[own.linear.exit] At each exit of a binding's scope, an owned binding must be consumed or reserved by a registered `defer`. Error: `own-not-consumed`. (owner, 2026-10-16: "linear type semnatics")
2. r[own.linear.exits] The exits are the normal end, `return`, `break`, `continue`, `?`, and cancellation at a bang call ([E.2](#e2-cancellation)). (agent)
3. r[own.linear.merge] At a merge, an owned binding must be consumed on every incoming path or on none. Error: `own-not-consumed`. (agent, as Austral)
4. r[own.linear.loop] A loop body must not consume an owned binding declared outside the loop, unless it initializes the binding again before the next iteration. Error: `use-after-move`. (agent, as Austral)
5. r[own.linear.temp] An owned value that no binding holds must be consumed where it is made. `_ := connect!("db")` is an error. Error: `own-not-consumed`. (agent)
6. r[own.linear.panic] A path that ends in a panic needs no consumption, since a panic runs no `defer` and poisons the instance (`flow.defer.panic`). (agent)
7. r[own.defer.reserve] A `defer` suite may consume an owned binding of its block, and this **reserves** the binding from the registration on. (agent)
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
5. r[own.suspend.own-arg] A plain call must not pass an owned argument or call an `own fn!`; only a bang call may. Error: `own-in-cold-frame`. (agent; see [Q8](#q8-owned-arguments-to-cold-frames))
6. r[own.suspend.stored] A stored frame keeps its bound: module state takes only a frame bounded by `static`. Error: `lifetime-mismatch`. (agent)

> **Why.** A cold frame may be discarded or cancelled before its first
> poll. Then no line of its body runs, so no `defer` consumes an owned
> value it holds. Rule 5 keeps owned values out of cold frames.

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
trait Pool:
    fn get(^self) -> ^Conn                       # hypothetical syntax

fn grab() -> ^{$Pool} Conn $ Pool:               # hypothetical syntax
    $.use(Pool).get()

fn serve!(p: ^dyn Pool) -> void:                 # hypothetical syntax
    $.with(Pool=p):
        grab().send!([1])
```

1. r[own.provider.with] `$.with(K = p)` borrows `p` for the block, whatever `p`'s mode. (agent; carries the old key roots)
2. r[own.provider.use] `$.use(K)` returns a borrow of the provider bounded by `$K`, so it cannot be kept beyond the call. (agent; replaces NE-S6b and the first version's split between own and plain keys)
3. r[own.provider.frame] A frame built inside `$.with(K = p)` is bounded by `p`'s borrow. (agent)

> **Why.** Without own traits, a key's trait no longer says whether its
> provider is borrowed, and `$.use(K)` has one static type for every
> provider. Making it always a borrow is the one rule that is sound for
> borrowed and owned providers alike. It costs plain code that keeps a
> provider in module state.

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
4. r[own.top.own] A top-level binding may hold an owned value. It is never consumed: the instance's end releases it. Moving or consuming it is an error. Error: `move-out-of-top-level`. (agent; see [Q9](#q9-owned-values-at-the-top-level))

A REPL input's bindings are top-level bindings
(`cli.repl.declarations.bindings`), so the same rules hold:

```text
c := connect!("db")              # a REPL input: an owned top-level binding
c.send!([1])                     # a later input: a shared borrow, static
c.close()                        # error: move-out-of-top-level
```

> **Why.** Another function can reach `G` without going through the
> borrow, so a body cannot see every use. An owned `G` is mutated only
> through a `^mut` borrow, and rule 3 forbids it. A plain `G` may still
> be mutated elsewhere through a `mut` view, the per-place gap of
> [C.2](#c2-a-borrow-of-a-plain-local). Rust makes reading or writing a
> `static mut` `unsafe` for the same reason (Rust reference,
> items.static.mut.safety).

### E.6 The Serialization Boundary

1. r[own.boundary] An owned position, a type with an `own` part, and a type that carries a lifetime are not boundary-safe. (agent; extends `module.boundary.not-safe`)

[Serializable Closures](SERIALIZABLE_CLOSURE_DESIGN.md) says portable
captures "reach `static`", a term of the old design. Replacement wording,
for its next revision:

| Rule | Replacement |
| --- | --- |
| `portable.capture.boundary` | Every captured value must be boundary-safe, so it holds no borrow and no owned value. Error: `portable-capture-not-boundary-safe`. |
| `remote.cap.reach` | The exported object must hold no borrow: its lifetime is `static`. |
| C.7, a handle row | An owned handle is not boundary-safe, so it is rejected as an argument, a result, or a sealed value. |
| C.7, the closing sentence | Copying ends a borrow, and an owned value never crosses a boundary. |

## F. NonEscapable Removed

### F.1 The Old Requirements

| # | Old requirement | New mechanism |
| --- | --- | --- |
| 1 | NonEscapable is a category of types. | Replaced: no category. `own` is a mode at each position ([B.1](#b1-owned-positions)). |
| 2 | Lifetimes are in the semantics and in signatures. | Kept: borrow lifetimes ([B.5](#b5-lifetimes-named-by-variables)). |
| 3 | A result or store depends on any subset of inputs, exactly. | Kept: `^{a, b}` results, `c: ^{dst.T}` stores. |
| 4 | No false alarms from inference; the checker never guesses conservatively; ambiguity needs an annotation. | Relaxed: inherits Rust's known false alarms. Rust's borrow checker is conservative: it rejects NLL problem case #3 (RFC 2094), which Polonius targets ([false alarms](#false-alarms), [Q16](#q16-rusts-false-alarms)). Elision and `ambiguous-lifetime` are kept. |
| 5 | Several independent lifetimes in one struct. | Kept: lifetime parameters, `data Session[^c, ^u]`. |
| 6 | Terse notation. | Kept: `^`, `^{x}`, `own`. |
| 7 | Local positions marked; unmarked is global. | Replaced: unmarked is plain; `own` is owned; `^` is a borrow. |
| 8 | Any parameter of any type may be marked `^`. | Kept, now borrow-checked by the owner's scope decision ([C](#c-borrows-and-plain-values)). |
| 9 | One reach per component, written in type arguments. | Kept as Rust's lifetimes in type arguments, `List[^Conn]`. |
| 10 | Exact substitution of published signatures. | Kept: Rust's model. |
| 11 | "If a thing is local, always show it in source." | Kept: every borrow writes `^`, every owned position writes `own`, and lifetime arguments are written. |

### F.2 The Old Mechanisms

| Old mechanism | New mechanism, or dropped and why |
| --- | --- |
| The category and `impl NonEscapable` | Dropped: `own` at positions, as `mut`. |
| Generic category from stored fields (NE-S10) | Dropped: `T` may be `own Conn`, as it may be `mut User`. |
| Roots: static, parameter, key, local, type-parameter | Lifetimes: borrowed parameters, `$K`, `static`, and `T`'s own lifetimes. |
| A fresh handle reaches `static` (OQ1) | Dropped: a fresh handle is returned `own` and must be consumed. |
| Reaches as root sets | Rust lifetimes, solved per body. |
| Declared slots `data S[^c, ^u]` | Lifetime parameters, same spelling. |
| Component paths `xs.T`, `p.A`, `t._0` | Kept as lifetime paths. |
| Frame `run` slot and output | A lifetime bound on the frame; `T` carries the output. |
| One reach per callable environment, `^{f}` stores (NE-S4b) | Dropped: a store's lifetime sits in the destination's type, as in Rust. |
| Bound method stores into the environment (follow-up 9) | Dropped: `dst::push` has type `fn(v: ^{dst.T} Conn) -> void`. |
| Variance from the surface (NE-S1, NE-S1c) | Kept: `types.polarity` on lifetime parameters. |
| Erasure join (B11) | Replaced: `dyn K` holds no borrow, `^{a} dyn K` holds one bound. |
| Downcast needs `static` (Q6) | Kept. |
| Ordinary providers must be static (NE-S6b) | Replaced: `$.use(K)` is always a borrow bounded by `$K`. |
| Key roots `^{$Pool}` | Kept as the lifetime `$K`, always written. |
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
| L7 | One root per creation site | Gone: each owned value has its own owner. |
| L8 | A local `^dyn` cannot be downcast | Same, narrower in Rust. |
| L9 | A written reach applies whatever the value holds | Gone: an owned result carries no lifetime. |
| L10 | A store into an environment reaches every capture | Gone: no environment stores. |

### F.4 What The Old Design Guaranteed That This One Does Not

| # | Area | Old design | This study | Kind |
| --- | --- | --- | --- | --- |
| a | Borrows of plain values | Promised only "not kept" for a `^` parameter. | "Not kept" is sound. "Frozen while borrowed" holds per variable only: an earlier plain alias is not frozen (`sneaky`, [C.2](#c2-a-borrow-of-a-plain-local)), and a closure can mutate a frozen local (hole B5). | A new promise that leaks, not a regression. |
| b | An owned handle in module state (R7, D8, R32) | Accepted: a fresh handle reached `static`, so a global registry could hold it. | A limit until [Q9](#q9-owned-values-at-the-top-level) and [Q11](#q11-mutable-borrows-of-a-top-level-binding) are answered. The idiomatic path is a provider bound by `$.with`, which borrows the handle for a block (D4). | A regression for global registries. |
| c | A handle opened at the top level or in the REPL (T3) | Usable and closable; closing was an ordinary method while the closing design was parked. | Under the borrow-only draft of Q9, it can never be closed. Q9 option 3 would let a later statement consume it. | A regression until Q9 is answered. |

### F.5 What Ownership Adds

| Check | Old design | This study |
| --- | --- | --- |
| Use after close | Not checked: `close` was an ordinary method. | `use-after-move`: `close(own self)` consumes the handle ([B.3](#b3-moves)). |
| A forgotten close | Not checked. | A compile error, `own-not-consumed`, under the linear draft ([D.2](#d2-linear-rules)). |
| The parked closing design | Closing modes, cleanup blocks, and liveness against them were parked (22 rules). | Modelled as moves: consumption and `defer` reservation, 9 rules ([D.2](#d2-linear-rules)). |
| Exclusive mutation | `^mut` promised no exclusivity (S4). | `^mut` is exclusive for owned values, and per variable for plain ones ([B.4](#b4-borrows)). |

## Use Cases Against NonEscapable

The owner, 2026-10-16, about 13:15, verbatim: "check the nonescapable's
use case and check it against your new ownership doc.
semantics/mechanism can be different. focus on use case/false
alarm/annotation burden. then update ownership doc".

This section compares by use case, not by mechanism. The old spellings
are the round-8 forms of
[NonEscapable Types And Lifetimes](NONESCAPE_LIFETIME_DESIGN.md) §13.1.

### Use Cases

A count adds one for each `^`, `^{..}`, `^mut`, `own`, lifetime or slot
parameter, `impl NonEscapable` line or `< NonEscapable` bound, and each
extra line that linearity forces, such as a `defer`. Only the signature
lines shown are counted.

| # | Use case (cases) | Old spelling | New spelling | Old | New | Outcome | False alarms |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | A view over a handle (R1, R3, S1, R5, R14) | `data Excerpt: conn: ^Conn`; `impl NonEscapable for Excerpt`; `fn excerpt(c: ^Conn) -> ^Excerpt`; `fn first_open(pool: ^List[^Conn]) -> Option[^Conn]` | `data Excerpt[^a]: conn: ^{a} Conn`; `fn excerpt(c: ^Conn) -> Excerpt[^]`; `fn first_open(pool: ^List[own Conn]) -> Option[^Conn]` | 7 | 7 | Works | none new |
| 2 | A producer of a fresh handle (R9, R2) | `fn accept!(^self) -> Conn`; caller `c := l.accept!()` | `fn accept!(^self) -> own Conn`; caller `c := l.accept!()` and `defer: c.close()` | 1 | 3 | Works, and a leak is now an error | none |
| 3 | A holder or pool whose handles callers also use (S2, D5, R13, D6, D7) | `data Pool: conns: ^mut List[^Conn]`; `impl NonEscapable for Pool`; `fn adopt(^mut self, c: ^{self} Conn)` | `data Pool: conns: own List[own Conn]`; `fn adopt(^mut self, c: own Conn)`; the owner's `defer: pool.close()` | 5 | 5 | Works differently: the pool owns its handles, and callers borrow them through `find(^self) -> Option[^Conn]` | new: a `^mut self` method while a borrow from `find` is live; R13's mixed map is rejected in both |
| 4 | Concurrency (R10, R11, D3, D2) | `fn echo!(c: ^Conn)`; `fn fastest!(a: ^Conn, b: ^Conn) -> string` | the same | 3 | 3 | Works for borrows; a cold frame that owns a handle is lost (R10, `own-in-cold-frame`) | new: R10's owned frame, polled later, is rejected ([Q8](#q8-owned-arguments-to-cold-frames)) |
| 5 | Providers (D4) | `trait Pool < NonEscapable: fn get(^self) -> ^Conn`; `fn grab() -> ^Conn $ Pool`; `fn serve_with!(p: ^dyn Pool)` | `trait Pool: fn get(^self) -> ^Conn`; `fn grab() -> ^{$Pool} Conn $ Pool`; `fn serve!(p: ^dyn Pool)` | 5 | 4 | Works; `$.use(K)` is always a borrow | new: keeping a plain provider in module state (R44) |
| 6 | Callbacks (S6, D8, R7, M3, N4) | `fn first_ok!(n: usize, attempt: ^fn!() -> bool)` with `tries: List[^fn!() -> bool]`; `fn on_message(h: fn(string) -> void)`; `fn counter(c: ^Conn) -> ^fn() -> usize` | `attempt: ^fn!() -> bool` with `tries: List[^{attempt} fn!() -> bool]`; `on_message` unchanged; `fn counter(c: ^Conn) -> ^fn() -> usize` | 4 | 4 | Works; a registry or job list of closures that own a handle is lost until Q11 (D8, R7) | none new beyond the limit |
| 7 | An error that carries a handle through `?` (D1) | `enum LinkError: Broken(conn: ^Conn)`; `impl NonEscapable for LinkError`; `fn check!(c: ^Conn) -> Result[void, ^LinkError]` | `enum LinkError[^c]: Broken(conn: ^{c} Conn)`; `fn check!(c: ^Conn) -> Result[void, LinkError[^]]` | 4 | 4 | Works; an owned error is `own Result[void, own LinkError]` | none |
| 8 | The REPL (T3) | `c := connect!("db")` | the same | 0 | 0 | Works differently: under Q9's draft, `c.close()` is rejected | new: closing a top-level handle ([Q9](#q9-owned-values-at-the-top-level)) |
| 9 | Generic code (N2, R6) | `fn look_at[T](x: ^T)`; `fn inspect_local(x: ^dyn Inspectable)`; `fn feed[T](slot: ^mut Box[T], v: T)` | the same | 3 | 3 | Works; an owned `T` in an unconstrained body is hole B8 | none |
| 10 | Locality of plain values (N3, the "not kept" fact) | `fn sum_bytes(b: ^List[u8]) -> usize`; `fn fill(buf: ^mut List[u8])` | the same | 2 | 2 | Works, and a borrowed local is now frozen per variable | new: mutating a plain local while a `^` view of it lives |
| | **Total** | | | **34** | **35** | | |

### False Alarms

Cases where this study rejects safe code that the old design accepted.

| # | Case | Old | New | Status |
| --- | --- | --- | --- | --- |
| 1 | NLL problem case #3: return a borrow from a lookup, or insert and look up again (RFC 2094) | accepted: reaches were sets, not regions | rejected: the lookup's borrow is live on the inserting path too | Known false alarm, as in Rust; [Q16](#q16-rusts-false-alarms) |
| 2 | A `^mut self` method called while a borrow of one field lives: the method form of disjoint fields | accepted: no exclusivity | rejected: the method borrows the whole receiver | Known false alarm, as in Rust; write the field access in the body or pass the field |
| 3 | Two-phase borrows: `xs.push(xs.len())` with `push(^mut self)` | accepted | accepted: [own.borrow.two-phase](#b4-borrows) adopts Rust's two-phase borrows | Fixed in the draft |
| 4 | A path where the value is gone in fact, as after a helper that always panics | accepted: no linearity | rejected with `own-not-consumed` when the helper returns `void`; accepted when it returns `never` (`own.linear.panic`) | Fixed when the helper is declared `-> never`; known false alarm otherwise |
| 5 | Mutating a plain local while a `^` view of it lives | accepted: GC keeps it safe | rejected: `borrow-conflict` | By the owner's decision "check all ^ local var" |
| 6 | Keeping a plain provider from `$.use(K)` in module state | accepted: plain providers reached `static` | rejected: `$.use(K)` is a borrow ([E.4](#e4-providers)) | Known false alarm of the draft |
| 7 | A cold frame over an owned handle, polled later (R10) | accepted: a fresh handle reached `static` | rejected: `own-in-cold-frame` | Owner question [Q8](#q8-owned-arguments-to-cold-frames) |

```text
fn get_or_add(m: ^mut Map[string, own Conn], key: string) -> ^Conn:   # hypothetical syntax
    match m.get(key):
        .Some(c) => c                     # the borrow of m flows to the result
        .None =>
            m.insert(key, connect_now(key))   # error: borrow-conflict: m is still borrowed
            m.get(key).unwrap()
```

The Rust workaround checks first and looks up twice: `if
!m.contains(key): m.insert(...)`, then one `get`.

### Verdict

Every old use case still works, and the annotation count is the same
within one mark: 34 against 35 over the representative cases. The one
added mark per producer is the `defer` that linearity forces, which buys
the use-after-close and leak checks the old design parked. Three use
cases lose ground: owned handles kept in module state (D8, R7, R32) or
in a cold frame (R10), and closing a top-level handle (T3); all three
turn on owner questions. The new false alarms are Rust's (problem case
#3, method calls over disjoint fields) plus three of this draft's own
(the per-variable freeze of plain locals, kept plain providers, and
owned cold frames). The old design had none of these, because it
promised no exclusivity.

## G. Stress Test

### G.1 Verdicts

Verdicts: **accepted**, **rejected** (as intended), **limit** (a sound
rejection of a reasonable program), and **hole** (unsound or
unspecified). The Rust column gives Rust's verdict on the same shape.
Rows changed by the usage-site model are N1, N2, N6, S2, S3, S5, D1, D4,
D5, D7, C09, C10, C18, R21, R35, R44, V4, and the new U rows. The
[use-case comparison](#use-cases-against-nonescapable) regroups these
rows by what a program is trying to do.

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
| R9 | `accept` | accepted | `-> own Conn`, no lifetime | same |
| R10 | A borrow across a suspension | bang accepted; stored frame rejected; owned frame limit | `own.suspend.own-arg` | spawn of an owned future accepted |
| R11 | `thread::scope` | accepted | children borrow `c` shared | same |
| R12 | A lifetime-generic method | accepted | `^{self.u}` | same |
| R13 | A map of two lifetimes | rejected | one value lifetime | same |
| R14 | `Option<&T>` with `?` | accepted | elided to `pool` | same |
| R15 | A self-referential struct | limit | a field cannot borrow a sibling | same |
| S1 | `Span` view | accepted | `-> Lines[^]` | - |
| S2 | A mutating store | accepted | `c: ^{self.a} Conn`, or an `own` pool field that moves `c` in | - |
| S3 | A consuming close wrapper | accepted | `fn shutdown(own self)` destructures and closes | `drop(self)` accepted |
| S4 | `MutableSpan` | accepted | shared writers; `^mut` writers would conflict | - |
| S5 | Conditional `Escapable` | accepted | `Tagged[own Conn]` by `own.generic.arg`; `Labeled` writes `own` on its field | - |
| S6 | A non-escaping closure stored locally | accepted | `List[^{attempt} fn!() -> bool]` | - |
| M1 | Mojo `ref [a, b]` | accepted | `^{a, b}` | - |
| M2 | Mojo struct origin | accepted | a lifetime parameter | - |
| M3 | Capturing and escaping closures | accepted | `^{c} fn()`; `tally` holds no borrow | same |
| T1 | Independent slots | accepted | `^{s.c}` | same |
| T2 | Swapping slots | rejected | distinct lifetimes | same |
| T3 | A REPL handle | accepted for use; close rejected | [E.5](#e5-top-level-bindings-and-the-repl) | no REPL |
| D1 | A handle in a `Result` through `?` | accepted | `-> own Result[void, own LinkError]`; `?` moves the owned error | same |
| D2 | A cold frame in a field | rejected | `Worker[^w]` keeps `w` for its life | same |
| D3 | `race!` and cancellation | accepted | children borrow; losers end inside the call | - |
| D4 | A provider that hands out a borrow | accepted with `^{$Pool}`; bare `^` and store into `KEEP` rejected | keys are not elision inputs; the result is bounded by `$Pool` | same shape |
| D5 | A map in a holder | accepted | `own Registry` with an `own Map[string, own Conn]` field moves `c` in | same |
| D6 | A `dyn` list mixing sources | accepted | `List[^Conn]` of two borrows | same |
| D7 | Recursive data | accepted | `enum Chain` with `own` payloads; `Duo[^a, ^b]` | same |
| D8 | A callback registry | borrow rejected; owned limit | an `own fn` needs `own List[own fn]` and `^mut HANDLERS` | accepted |
| N1 | An unmarked handle parameter | rejected | `fn ping(c: Conn)` cannot take an owned `c`: `own-to-plain`, fix-it `^Conn` | accepted (moves, drops) |
| N2 | Generic and `dyn` | accepted | `[T]` takes `^Conn` and `own Conn`; a body that copies is hole B8 | same |
| N3 | General locality | accepted; store rejected | [C](#c-borrows-and-plain-values) | same |
| N4 | Function types | accepted; `bad` rejected | `ambiguous-lifetime` | same |
| N6 | A concrete holder | rejected | a plain field `conn: Conn` takes no owned handle: `own-to-plain`; `conn: own Conn` is accepted | - |
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
| C09 | A callback from a provider | accepted with `^{$Pool}`; bare form rejected | keys are not elision inputs | - |
| C10 | A lease that keeps its pool | accepted | `data Lease[^p]`, used as `own Lease[^]` | same |
| C11 | An optional pair | accepted | `^{p.T.a}` | same |
| C12 | A cleared list | limit | the type keeps `c`'s lifetime | same |
| C13 | A shared iterator | accepted | optimizer concern only | - |
| C14 | A plain result escapes | accepted | no `^`, unchecked | - |
| C15 | Handles in a loop | rejected as written | the second handle leaks | accepted (drops) |
| C16 | A scoped race | accepted | children borrow | - |
| C17 | Early and delegated close | rejected | `use-after-move` | same |
| C18 | Phantom metadata | accepted | `data Tag[T]` stores no `T`; `Tag[own Conn]` | same |
| C19 | A local classifier | limit | downcast needs `static` | narrower |
| C20 | A scalar closure factory | accepted | holds no borrow | same |
| R21 | A provider over a borrow | accepted | `$.with` borrows `p`; `$.use` is bounded by `$Reader` | same |
| R22 | Optional wrapping, invariant slot | rejected; named form accepted | invariance | same |
| R23 | A tuple hides the widening | rejected; `bundle` accepted | per-element lifetimes | same |
| R24 | A bound appender | accepted; unnamed rejected | `fn(v: ^{dst.T} Conn)` | same |
| R25 | A cold call of a closure | accepted with `^{action}` | frame bound | same |
| R26 | A weakened suspending function | accepted | `^{a, b} mut Suspend[^{a} Conn]` | same |
| R27 | A scoped callback | accepted; borrowed result rejected | fresh input lifetime | same |
| R28 | A closure stores into a capture | rejected; named form accepted | fresh input lifetime | same |
| R29 | A fresh mutable result | accepted | plain result, as today | same |
| R30 | Generic scalar results | accepted | `List[usize]` holds no borrow | same |
| R31 | A plain identity helper | accepted | no `^`, unchecked | - |
| R32 | A fresh handle in a registry | limit | as D8 | accepted |
| R33 | A bare callback returned | accepted | `-> ^{f} fn() -> usize` | same |
| R34 | Weakening over a borrowed buffer | rejected; `^{bytes}` accepted | frame bound | same |
| R35 | Generic identity of a `^mut` | accepted | `[T]` passes `^mut Conn` through | same |
| R36 | A struct of a list and a handle | accepted | `data Bundle[^e, ^k]` | same |
| R37 | A list lookup of a two-lifetime record | accepted | `T` carries both | same |
| R38 | Lists of different lifetimes | rejected | invariance | same |
| R39 | A bound getter | accepted | `T` keeps its lifetime | same |
| R40 | A closure reads one capture, stores into another | accepted | no environment stores | same |
| R41 | `Fn` constructor identity | rejected | `own.fntype.ctor` | same |
| R42 | `iter::from_fn` | accepted | `data Generated[T, ^s]` | same |
| R43 | Frame outputs in a tuple | accepted | per-element | same |
| R44 | Two provider keys | accepted; keeping a provider rejected | `own.provider.use` | - |
| R45 | A scoped callback returns its borrow | rejected | fresh input lifetime | same |
| L1 | A forgotten close | rejected | `own-not-consumed` | accepted (drops) |
| L2 | Close in one branch only | rejected | `own.linear.merge` | accepted (drops) |
| L3 | Owned value in a loop | rejected; per-iteration accepted | `own.linear.loop` | same as hd for the first |
| L4 | Owned values in a `List` | accepted; `break` rejected | `into_iter` | accepted (drops) |
| L5 | `?` before any `defer` | rejected | `?` is an exit | accepted (drops) |
| L6 | A bang call before any `defer` | rejected | cancellation is an exit | accepted (drops) |
| L7 | A panic path | accepted | `own.linear.panic` | same |
| V1 | Use after move | rejected | `use-after-move` | same |
| V2 | A move in a closure, called twice | rejected | `own fn` is one-shot | same (`FnOnce`) |
| V3 | An owned argument to a cold call | rejected | `own-in-cold-frame` | accepted |
| V4 | A partial move | accepted; plain holder rejected | `own.move.partial`; `move-out-of-plain` | same |
| B1 | A parser borrowing its source | accepted | `Parser[^src]` | same |
| B2 | A borrow outlives its owner | rejected | `lifetime-mismatch` | same |
| B3 | Mutation under a shared borrow | rejected | `borrow-conflict` | same |
| B4 | Two children borrow mutably | rejected | `own.children.xor` | same |
| B5 | A closure mutates a borrowed local | **hole** | [G.3](#g3-holes) | rejected |
| B6 | A borrowed top-level binding | accepted; `^mut` rejected | [E.5](#e5-top-level-bindings-and-the-repl) | `static mut` is `unsafe` |
| U1 | One type, owned and plain | accepted | `let own buf` and a plain `List[u8]` side by side | - |
| U2 | An owned value to a plain parameter | rejected | `own-to-plain` | moves |
| U3 | A plain alias in an owned literal | rejected | `own-upgrade` (`own.unique.init`) | moves |
| U4 | A `string` field in an owned literal | accepted | `own.unique.exempt` ([Q13](#q13-primitive-field-values-in-an-owned-value)) | copies |
| U5 | Two aliases of a plain holder consume its field | rejected | `move-out-of-plain` | no aliases |
| U6 | An owned handle moved into a plain holder | **hole** | [G.3](#g3-holes) | - |
| U7 | An unconstrained generic copies an owned `T` | **hole** | [G.3](#g3-holes) | rejected |
| U8 | `own i32` | rejected | `own-on-value` | - |
| U9 | An owned buffer calls `len(self)` | limit | `own-to-plain` until `len` takes `^self` ([Q4](#q4-plain-parameters-and-borrows)) | accepted |
| U10 | A forged owner through public fields | accepted | a fresh literal may be owned ([B.13](#b13-how-a-resource-stays-owned), row 4) | accepted (`unsafe` needed for an fd) |
| U11 | NLL problem case #3: get or insert | limit | the lookup's borrow stays live on the inserting path ([false alarms](#false-alarms), [Q16](#q16-rusts-false-alarms)) | rejected; Polonius accepts |

### G.2 Tally

| Verdict | Count | Cases |
| --- | --- | --- |
| Accepted, with any intended rejections | 65 | the rest |
| Rejected as intended | 42 | R2, R6, R13, T2, D2, N1, N6, X1 to X3, X5, X6, C01 to C04, C06, C07, C15, C17, R22, R23, R28, R34, R38, R41, R45, L1 to L3, L5, L6, V1 to V3, B2 to B4, U2, U3, U5, U8 |
| Limit | 10 | R7, R10, R15, D8, X7, C12, C19, R32, U9, U11 |
| Hole | 3 | B5, U6, U7 |

A row counts once. A row with any limit counts as a limit, and a row
whose main form is accepted counts as accepted. Against the first
version: V4 and R21 move from rejected to accepted, and eleven U rows
are new. Four limits (R7, R10, D8, R32) are owned work kept in module state
or in a cold frame, so they turn on
[Q8](#q8-owned-arguments-to-cold-frames) and
[Q11](#q11-mutable-borrows-of-a-top-level-binding).

### G.3 Holes

**B5. A closure mutates a borrowed local.** A closure captures a
plain local as shared storage, unchecked
([own.closure.plain](#b9-closures)). Calling it while a `^` borrow of
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
copied into plain storage loses its liveness, so any fix covers only
closures held in locals.

**B7 (U6). An owned value moved into a plain holder.** A fresh literal
used plainly may take an owned value into an `own` field. The holder
is plain, so by [own.move.path](#b3-moves) nobody can consume the field,
and [own.linear.exit](#d2-linear-rules) sees no owned binding to check.
The handle leaks. Uniqueness holds; linearity does not.

```text
data Holder:
    c: own Conn                           # hypothetical syntax

fn lose!() -> void:
    h := Holder { c: connect!("db") }     # plain h; the Conn moves in
    h.c.send!([1])                        # a borrow: fine
    h.c.close()                           # error: move-out-of-plain: c can never close
```

[Q14](#q14-an-owned-value-moved-into-a-plain-holder) asks whether to
close it.

**B8 (U7). An unconstrained body copies or drops an owned `T`.** By
[own.generic.arg](#b11-generics-and-dyn), `T` may be `own Conn`, and
by `types.generic.direct`, an unconstrained body may use a `T` value
twice, or not at all, as `keep_one[T](x: T): pass` does. The body is
checked once, without knowing `T` is owned.

```text
fn dup[T](v: T) -> (T, T): (v, v)

fn twice!() -> void:
    pair := dup(connect!("db"))           # two owners of one Conn
    pair._0.close()
    pair._1.close()                       # closes it again
```

Rust rejects `dup`: a generic `T` moves unless bounded by `Copy`.
[Q15](#q15-an-unconstrained-body-and-an-owned-t) asks how to close it.

### G.4 Cases

The cases below show the new forms. The other rows follow from the rule
named in their row.

```text
fn pool!(hosts: List[string]) -> void:    # L4
    let conns: own List[own Conn] = []    # hypothetical syntax
    for h in hosts:
        conns.push(connect!(h))           # moves each handle in
    for c in conns.into_iter():
        c.close()                         # consumes every element

fn pool_break!(conns: own List[own Conn]) -> void:   # hypothetical syntax
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

fn cold(c: own Conn) -> void:             # hypothetical syntax: V3
    pending := handle(c)                  # error: own-in-cold-frame
    pending.cancel()
```

```text
fn keep_one!() -> Conn?:                  # C15 as written
    let keep: Conn? = .None
    for index in 0..2:
        c := connect!("db")
        if index == 0:
            keep = .Some(c)               # error: own-to-plain: keep is a plain Conn?
    keep
```

C15 shows the model at work: the review's program keeps a handle in a
plain optional and leaks the second one. The fixed form declares
`let keep: own Conn? = .None` and `-> own Conn?`, closes the second
handle in the `else` branch, and calls `keep.take()` before assigning.

```text
fn size(buf: own List[u8]) -> usize:      # hypothetical syntax: U9
    buf.len()                             # error: own-to-plain: len takes plain self

fn ping(c: Conn) -> void: pass            # N1, U2

fn misuse!(shared: List[string]) -> void:
    c := connect!("db")
    ping(c)                               # error: own-to-plain
    let own u = User { name: "Ada", tags: shared }   # hypothetical syntax: U3: error: own-upgrade
    let own v = User { name: "Ada", tags: [] }       # hypothetical syntax: U4: name is exempt
```

## H. Cost

### H.1 Rule Count

The old design had 176 `ne.*` rules, 22 of them for the parked closing
and cleanup design. This study's 118 `own.*` rules:

| Group | Prefixes | Rules |
| --- | --- | --- |
| Owned positions | `mod` | 9 |
| Where owned values come from | `source`, `no-upgrade`, `no-plain`, `let`, `bind`, `result`, `unique` | 11 |
| Moves, paths, and destructuring | `move` | 10 |
| Borrows and borrowed locals | `borrow`, `local` | 15 |
| Lifetimes and elision | `life`, `elide` | 13 |
| Borrows inside types | `data` | 7 |
| Methods, closures, function types | `self`, `closure`, `fntype` | 17 |
| Generics and `dyn` | `generic`, `dyn` | 11 |
| Linearity and `defer` | `linear`, `defer` | 9 |
| Suspension, children, providers | `suspend`, `children`, `provider` | 11 |
| Top level and boundary | `top`, `boundary` | 5 |
| **Total** | | **118** |

The first version had 94 rules. The usage-site model adds 30: the
modifier's forms and rejections (9, as `mut` has), the sources and
uniqueness of owned values (11), the path rule and partial moves (2),
receivers (2), the generic twins of the `mut` rules (5), and two-phase
borrows (1). It drops the six declaration rules (`own.decl.*`). The
count is still a third below the old design's 176.

### H.2 Annotation Burden In std

Counts come from `lib/std` today. Only `std.net` holds handles.

| Change | Where | Count |
| --- | --- | --- |
| Trait declarations | `TcpStream`, `TcpListener`, `UdpSocket` stay as they are | 0 |
| `mut self` to `^self` | `read!`, `write!`, `accept!`, `send_to!`, `receive!` | 5 |
| `mut self` to `own self` | three `close` methods | 3 |
| `Result[mut dyn K, E]` to `own Result[own dyn K, E]` | `Net.connect!`, `listen!`, `bind_udp!`, `accept!` | 4 results, 8 marks |
| Generic declarations | `Option`, `Result`, `List`, `Deque`, `Map`, `Iterator` keep theirs | 0 (6 under [Q15](#q15-an-unconstrained-body-and-an-owned-t) option 2) |
| New borrowing or consuming methods | `take`, `pop`, `at`, `into_iter` and the like | about 12 |
| `^self` and `^` parameters, to accept borrows and owned values | public methods over plain types | up to about 400 public, 860 private |

The last row is the old scan's locality marks (243 receivers and 157
parameters public). Under the draft, a borrow or an owned value reaches a
std method only when that method writes `^`. They are needed only as
borrows and owned values appear in user code, and a lint with a fix-it
proposes each one. [Q4](#q4-plain-parameters-and-borrows) would remove
the row for borrows; owned values would still need it, by answer 1.

### H.3 Checker Cost Per Body

The checker is a Rust NLL-style dataflow over each body, using only
callee signatures.

| Step | Work | What it reuses |
| --- | --- | --- |
| Modes | `own` beside `mut` on every type; `own-to-plain` and `own-upgrade` beside `mutable-upgrade` | the permission checks of `types.mut.*` |
| Moves and linearity | A forward set of possibly-moved places, with its must-dual for consumption | `names.definite.*`: the possibly-uninitialized set in `crates/hd_check/src/body.rs` and its merges in `expr.rs` and `init.rs` |
| Borrow liveness | A backward liveness pass per body, iterated over loops | None in the checker; `crates/hd_wasm/src/live.rs` has the same pass shape for Wasm |
| Lifetime constraints | Subset constraints between lifetime sets, solved least | Row sets and their solver in `crates/hd_check/src/rows.rs`, as the old design planned |
| Variance | Per lifetime parameter, from the declaration | `types.polarity` |
| Conflicts | For each live borrow, test the uses of its place | New |

Cost: about `O((N + E) · ⌈L/64⌉)` per body, for `N` program points, `E`
constraints, and `L` lifetimes and borrows. This is a common-case estimate:
liveness over loops needs a fixed point. Partial moves track one bit per
owned field path, which adds to `L`. No step crosses a body, so an
edited body never invalidates another.

## I. Prior Art

### I.1 Where Ownership Is Written

| Language | On the type declaration | At each use |
| --- | --- | --- |
| Rust | Every type moves; `Copy` is opted into on the type ("Rust has a special annotation called the `Copy` trait"). | `&T`, `&mut T`, or by value, at every parameter, result, and field. |
| Swift | `~Copyable` suppresses copying for the whole type (SE-0390). | `borrowing` and `consuming` parameter modifiers, which "do not affect the caller-side syntax" (SE-0377). |
| Mojo | `Copyable` and `Movable` conformance on the type. | Argument conventions: `read` (default), `mut`, and `var`, where "The function takes ownership of a value"; the caller may add the postfix `^` transfer sigil. |
| hd (this study) | Nothing. | `own T`, `^T`, or plain `T` at every position, fields and type arguments included, as `mut`. |

All three put copyability on the type and the passing convention on the
parameter. hd puts both on the position: one type can be owned in one
place and plain in another. The price is answer 2's uniqueness rule and
the holes B7 and B8, which a declaration-level category would close.

### I.2 Languages

| Language | What it does | What hd takes |
| --- | --- | --- |
| Rust | "Each value in Rust has an owner. There can only be one owner at a time." One mutable reference or any number of immutable ones. | Ownership, moves, the borrow rules |
| Rust, elision | Three rules: distinct input lifetimes; one input lifetime fills every elided output; `&self` fills them in methods. Eliding what cannot be inferred is an error. | [B.6](#b6-elision), unchanged |
| Rust, NLL (RFC 2094) | Lifetimes "based on the control-flow graph, rather than lexical scopes", with liveness computed for lifetimes. | `own.borrow.live` |
| Rust, statics | An `unsafe` block is required to read or write a `static mut`. | [Q11](#q11-mutable-borrows-of-a-top-level-binding) |
| Rust, `mem::forget` | Safe, because Rust does not guarantee that destructors run. Rust is affine. | The affine column of [D.1](#d1-the-two-readings) |
| Swift SE-0390 | `~Copyable` structs and enums have unique ownership. A struct with a noncopyable stored property must be noncopyable too. A class may hold one, but never consume it. | The class rule is the path rule of [own.move.path](#b3-moves): through a plain holder, borrow only |
| Swift SE-0377 | `borrowing` and `consuming` parameter modifiers; inside the body a `consuming` parameter may be mutated. | `^T` and `own T` at parameters |
| Swift SE-0427 | Generic parameters are `Copyable` by default; `T: ~Copyable` suppresses it. | [Q15](#q15-an-unconstrained-body-and-an-owned-t) option 2 |
| Swift SE-0446 | `~Escapable` values cannot be stored in globals or captured by escaping closures. | The old design; replaced by borrows ([F](#f-nonescapable-removed)) |
| Mojo | "Every value has only one owner at a time." `var` takes ownership; the postfix `^` transfers. `ref [a, b]` names argument origins. | Lifetimes named by arguments; `own` at the use, as Mojo's conventions. Mojo spells a move `^`, the opposite of hd's `^`. |
| Hylo | Four conventions: `let`, `inout`, `sink`, `set`. A `let` or `inout` parameter cannot escape the callee without a copy; `sink` transfers ownership. | `^`, `^mut`, and `own` parameters |
| Austral | Types are free or linear; a type holding a linear type is linear. A linear variable is used exactly once; once in each `if` branch or in none; never inside a loop body if declared outside. | [D.2](#d2-linear-rules) |
| Austral | Borrowing is "stolen lock, stock, and barrel from Rust". | Borrows over linear values |

## J. Owner Questions

The scope question ([C.1](#c1-the-decision)) and the five usage-site
questions (A.1) are answered. Q2 (where `own` is written) and Q7 (owned
values in plain holders) are settled by those answers and removed; the
other numbers are kept. Each question below is one idea and decides
nothing.

### Q1. Linear Or Affine

**Effect.** Linear: a forgotten `close` is a compile error, and so is a
`let own user = User {...}` that is never destructured or passed on.
Affine: both compile, and the handle leaks, since hd has no `Drop`.

1. Linear: every owned value is consumed exactly once (draft).
2. Affine: consumed at most once.

```text
fn forgot!() -> void:
    c := connect!("db")
    c.send!([1])                          # linear: error: own-not-consumed
```

**Recommendation.** Linear. Without `Drop`, affine typing checks only
use-after-close, and `defer` already marks where cleanup goes.

### Q3. A Call-Site Move Mark

**Effect.** Without a mark, `archive(f)` moves `f` silently; the reader
learns it from `archive`'s signature or a later error.

1. No mark; the parameter's mode decides (draft).
2. `move f` at the call, as Mojo's `f^`.

```text
archive(f)                                # option 1
archive(move f)                           # hypothetical syntax: option 2
```

**Recommendation.** Option 1. A later use is a compile error, and hd
writes no mark for a borrow either.

### Q4. Plain Parameters And Borrows

**Effect.** Under the draft, a `^List[u8]` cannot reach `len(self)` until
std writes `len(^self)`: up to about 400 public marks.

1. No: a plain parameter is an unchecked alias and takes no borrow (draft).
2. Yes: a plain parameter takes a borrow, and storing it becomes an error unless marked.

```text
fn size(b: ^List[u8]) -> usize:           # hypothetical syntax
    b.len()                               # option 1: error until len takes ^self
```

**Recommendation.** Option 1. Option 2 changes the meaning of every
existing signature that stores a parameter. The marks come in phases,
proposed by a lint. An owned value needs the same marks either way.

### Q5. Copying A Composite Out Of A Borrow

**Effect.** If a composite read through a borrow may become a plain
value, any borrow can be laundered into a global by one assignment.

1. Error: it stays a borrow (draft).
2. Allowed: it becomes a plain alias, and borrows of plain values check only the place.

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

### Q8. Owned Arguments To Cold Frames

**Effect.** A cold frame cancelled before its first poll runs no
`defer`, so a handle passed to it would leak.

1. Only a bang call may pass an owned argument (draft).
2. Allow it, and make cancelling a never-polled frame that holds an owned value a panic.

```text
pending := handle(c)                      # option 1: error: own-in-cold-frame
```

**Recommendation.** Option 1. Structured children borrow instead, and
option 2 turns a type error into a run-time failure.

### Q9. Owned Values At The Top Level

**Effect.** A module or REPL may keep a connection in a top-level
binding. Under option 1 nothing can consume it, so a handle opened at
the top level can never be closed (T3).

1. Allowed, borrow-only, released when the instance ends (draft).
2. An error: owned values live only in function bodies.
3. Allowed, and a later top-level statement or REPL input may consume it. The binding is then dead, checked in statement order: no later statement may use it or call anything able to reach it.

```text
c := connect!("db")                       # a REPL input
c.send!([1])                              # a later input: a borrow
c.close()                                 # option 1: error: move-out-of-top-level; option 3: fine, c is dead
c.sent()                                  # option 3: error: use-after-move
```

**Recommendation.** Option 3. It keeps "borrow a global as if it is
own" and lets a REPL session close what it opened. Its reach check is
the call-graph check of [Q11](#q11-mutable-borrows-of-a-top-level-binding)
option 2, so both answers share one analysis.

### Q10. Closures Over A Borrowed Local

**Effect.** A closure that mutates a local can run while a `^` borrow of
that local lives, and nothing reports it ([G.3](#g3-holes)).

1. Leave it unchecked: closures over plain locals stay shared storage (draft).
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
let JOBS: own List[own fn!() -> void] = []    # hypothetical syntax

fn later(job: own fn!() -> void) -> void:     # hypothetical syntax
    JOBS.push(job)                            # needs ^mut JOBS: option 1: error: borrow-conflict
```

**Recommendation.** Option 2. The check needs a call graph like the one
module initialization builds for `module.init.definite`, and a call of
a closure or a `dyn` method counts as able to reach `G`. Option 1 rules
out global work queues (R7, D8), and option 3 drops the guarantee.

### Q12. `own` And `mut`

**Effect.** If `own` grants mutation, `let own user = User {...}` may
assign `user.name`. If it does not, that needs `own mut User`, and
`open_file!` must return `own mut File` for `^mut` borrows.

1. Separate: `own T` is a readonly owner, `own mut T` may mutate, as `^T` and `^mut T` (draft).
2. `own T` grants mutation, and `own mut T` is an error.

```text
let own user = User { name: "Ada", tags: [] }   # hypothetical syntax
user.name = "Lin"                                # option 1: error: readonly-root
```

**Recommendation.** Option 1. `mut` keeps marking where values change,
and the three modes pair the same way with it.

### Q13. Primitive Field Values In An Owned Value

**Effect.** Under answer 2, every field of `let own user` must be
unique. A `string` or `i32` field copied from a local is shared, but no
one can change it through the sharing.

1. Exempt: a primitive field value, `string` included, may come from anywhere (draft).
2. No exemption: every field value must be a literal, a fresh value, or owned.

```text
fn make(name: string) -> own User:              # hypothetical syntax
    User { name: name, tags: [] }               # option 2: error: own-upgrade
```

**Recommendation.** Option 1. Primitives are passed by value
(`types.prim.by-value`), so a shared one is not a second path to
anything.

### Q14. An Owned Value Moved Into A Plain Holder

**Effect.** `Holder { c: connect!("db") }` used plainly takes the
handle in, and then nobody can close it: a silent leak (hole B7).

1. Allowed: the value can only be borrowed from then on (draft).
2. A fresh value that takes in an owned value is itself owned, so it must be bound `own` or passed on.
3. A data type with an `own` field is always used `own` or `^`; a plain position of it is an error.

```text
h := Holder { c: connect!("db") }         # option 2: h is own Holder and must be consumed
```

**Recommendation.** Option 2. It keeps linearity with one rule on
fresh values, as answer 2 already ties a fresh value's parts to its own
mode. Option 3 puts the burden on every position of `Holder`.

### Q15. An Unconstrained Body And An Owned `T`

**Effect.** With `T = own Conn` allowed for every generic, `dup[T]`
returns two owners of one handle, and `keep_one[T]` drops one unclosed
(hole B8). `mut` needs no fix here,
because an unknown permission is simply unused, but an unknown owner
can be copied.

1. Every generic body moves `T` values, as Rust's: using one twice, dropping one, or copying one out of a list, is an error in any generic body.
2. An unconstrained `T` takes no owned type; `T < own Any` means "may be owned", and its body moves `T` values, as Swift's `~Copyable`.
3. Check each instantiation against the body it instantiates.

```text
fn dup[T](v: T) -> (T, T): (v, v)         # option 1: error: use-after-move
_ := dup(connect!("db"))                  # option 2: error: own-type-argument
```

**Recommendation.** Option 2. It keeps every existing generic body
valid, including the spec's own `first[T]`, which copies `items[0]`.
It departs from "same as mut" at one point, the unconstrained `T`, and
`List` and the other containers then declare `T < own Any`. Option 1
breaks those bodies, and option 3 makes a body's text part of its
signature.

### Q16. Rust's False Alarms

**Effect.** The old design promised that the checker never guesses
conservatively. Rust's borrow checker does: NLL problem case #3 (RFC
2094), returning a borrow from a lookup or else inserting, is rejected,
and Polonius targets it. Adopting Rust's checker adopts its false
alarms ([false alarms](#false-alarms)).

1. Accept the false alarms of Rust's shipped checker, listed as known limits with their workarounds (draft).
2. Name the false alarms hd must avoid, starting with problem case #3, and check those with a location-sensitive analysis, as Polonius does.

```text
match m.get(key):
    .Some(c) => c
    .None =>
        m.insert(key, connect_now(key))   # option 1: error: borrow-conflict; option 2: fine
        m.get(key).unwrap()
```

**Recommendation.** Option 1. A location-sensitive analysis tracks each
lifetime at each program point, which multiplies H.3's `L` by `N`, and
Rust has not shipped one. The workaround, a `contains` check before one
lookup, is short.

## Parse Log

Every `text` block, 44 in all, was parsed on 2026-10-16 with the
compiler's parser (`scripts/hd-conformance.sh parse`, the `parse_check`
example). Each block was parsed twice: as written, and with the
hypothetical marks erased (`own`, `^`, `^{...}`, lifetime brackets, and
`move`). The second parse checks that the rest of the block is hd
syntax. Parsing checks syntax only; nothing was type-checked, and many
blocks call functions the study does not define.

| Result | Count | Blocks |
| --- | --- | --- |
| Parses as written | 12 | C.3 plain code; D.1 `forgot!`; E.2 cancellation; E.5 REPL inputs; G.3 B8 `dup`; G.4 C15; Q1, Q8, Q9, Q14, Q15, Q16 |
| Fails as written, at a line marked `# hypothetical syntax`; parses with the marks erased | 30 | every other block except the two below |
| Fails as written at a marked line; with the marks erased, fails only at a signature written without a body | 2 | the A.3 prelude (`connect!`, `listen!`) and the B.6 elision block |

Every first failure of a block as written falls on a line marked
`# hypothetical syntax`. Every line that writes `^`, `own`, or `move`
outside a comment carries the mark. A signature without a body is a
sketch of a declaration, as in the old design's prelude, and is not hd
syntax.

## Sources

Swift SE-0377, SE-0390, and SE-0427, the Mojo ownership page, and the
Rust Book's ownership chapter were fetched on 2026-10-16 for the
quotations in [I](#i-prior-art).

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
- hd: `spec/lang/02-grammar.md` (Modifiers, Data Members),
  `spec/lang/03-names-and-scopes.md` (Locals, Receivers And Captures),
  `spec/lang/04-type-system.md` (Primitive Types, Tuples, Views,
  Bindings And Fresh Values, Generics, Mutable Bounds),
  `spec/lang/06-control-flow.md` (Deferred Cleanup),
  `spec/lang/07-functions.md` (Function Types, Captures, Methods),
  `spec/lang/10-modules.md` (Wasm Boundary, Module Initialization),
  `spec/lang/11-requirements-and-suspension.md` (Providers, Suspending
  Functions, Cancellation), `spec/lang/14-annotations.md` (Typed
  Derivation), `spec/cli/command-line.md` (REPL), `lib/std/net.hd`.
