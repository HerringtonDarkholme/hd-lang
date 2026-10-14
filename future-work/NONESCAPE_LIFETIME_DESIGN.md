# NonEscapable Types And Lifetimes: Design Study

## Table Of Contents

- [Status](#status)
- [Summary](#summary)
- [A. Verdict On The Claim](#a-verdict-on-the-claim)
- [B. Comparison](#b-comparison)
- [C. Candidate Shapes](#c-candidate-shapes)
- [D. Annotation Burden](#d-annotation-burden)
- [E. Owner Questions](#e-owner-questions)
- [F. Round 2: Revised Design And Stress Tests](#f-round-2-revised-design-and-stress-tests)
- [Sources](#sources)

## Status

This is the design study for issue #163. It decides nothing. The spec is
frozen, and nothing here is applied. The owner's decisions stay as
recorded in
[Open Issues](OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy) and
[Ownership and Escape Research](OWNERSHIP_AND_ESCAPE_RESEARCH.md).

Every spelling here is a placeholder: `NonEscapable`, `local`, `escaping`,
and `^a`. The agent wrote these, not the owner:

- the defer pin rule ([A.2](#a2-what-the-checker-promises));
- the defaults D1 to D3 ([A.5](#a5-iii-written-lifetime-relations));
- lifetime variance worked out from the declaration surface
  ([C.4](#c4-rules-all-three-shapes-share)).

Treat them as proposals until the owner vets them.

The hd examples use real syntax, plus the placeholder marker. A line
marked `# error:` gives the diagnostic the proposed checker would report.
No compiler checks these lines today.

## Summary

The owner claims that hd needs a NonEscapable type category and also
lifetimes. Split into three parts, the claim holds up like this:

| Part | Verdict | Decisive example |
| --- | --- | --- |
| (i) A type-level category is needed | **Holds, for burden rather than soundness.** Marking only at use sites can be sound, but it then puts annotations on code that touches no resource. | [`defer: log.push(...)` next to `defer: c.close()`](#a3-i-a-type-level-category) |
| (ii) The semantics need lifetime relations | **Holds.** A yes/no marker cannot tell a list declared in the loop body from one declared outside it. | [`open.push(c)` across a loop body](#a4-ii-lifetime-relations-in-the-semantics) |
| (iii) Lifetimes must be written in signatures or types | **Overstated.** None of the ten patterns needs a written lifetime to reject a bad program. Written markers are needed only to *accept* some good programs. Only independent field lifetimes need lifetime variables in types. | [`push` needs no annotation](#pattern-1-a-generic-container-push), [the two-handle `Session`](#pattern-4-a-struct-with-two-handles-from-different-scopes) |

The owner's `push` argument offers two choices: reject every push of a
handle, or accept the leak. It misses a third. A mutating method's `mut`
target takes on the lifetimes of the values passed in. Swift's setter
default already does this, as `@_lifetime(self: copy self, copy newValue)`,
and Kotlin writes it as `add(element: E_{this})`. The element type needs
no lifetime. The container does need one lifetime slot, so a pure yes/no
marker is still not enough.

The three candidate shapes are nested:

- **Shape A** (one lifetime per value, elided): Swift's current model with
  hd defaults. It has no written lifetime syntax.
- **Shape B** (Shape A plus named-value dependencies): adds Kotlin's
  `local` parameter modifier and Swift's `escaping`.
- **Shape C** (lifetime parameters): Rust's and Mojo's model, elided by
  default. Its lifetime variables can appear in type arguments.

**Recommendation: Shape B.** It covers nine of the ten patterns precisely.
For the tenth, the two-handle struct, it is sound but coarse. Across 127
std signatures in scope it needs 3 type markers, 0 required annotations,
and 8 recommended `local` markers.

## A. Verdict On The Claim

### A.1 Three Families Of Lifetime Notation

Shipping and proposed designs fall into three families. They differ in
where a written lifetime may appear.

| Family | Who | A lifetime is | It may appear in |
| --- | --- | --- | --- |
| (1) Lifetime variables | Rust `'a`, Mojo origin parameters, Swift's future "lifetime types" | an abstract parameter of a function or type | type arguments, fields, results, parameters |
| (2) Named-value dependence | Kotlin `local` and `T_{x}`, Swift `@lifetime(copy x)` and `@lifetime(self: copy v)` | a reference to a parameter, or `this` | positions: parameters, results, fields. Each aggregate gets one per-instance lifetime, `this`. |
| (3) Capture sets | Scala `T^{x}`, `[C^]` | a set of named values, or a capture-set variable | types, including type arguments and class parameters |

The families are not pure in practice:

- Kotlin allows `_{this}` inside a type argument, as in
  `Sequence<Sequence<T>_{this}>`.
- Kotlin also has explicit locality parameters on functions, as in
  `fun <A, B, local this, local transform> ...`. These are its route toward
  family (1). The note shows no class with locality parameters, though. A
  class has only `this`.
- Scala capture-set variables appear on classes, as in `class Source[X^]`
  and `class ConcatIterator[A, C^](var iterators: mutable.ListBuffer[IterableOnce[A]^{C}])`.
  Scala is therefore a hybrid of (2) and (1).
- Swift today keeps one lifetime per value. Its open proposal names
  "lifetime types" (`Span<'a, Int>`) and component lifetimes
  (`@lifetime(.a: copy arg1, .b: copy arg2)`) as future directions.

The three probes:

| Probe | Family (1) | Family (2) | Family (3) |
| --- | --- | --- | --- |
| `List[TcpStream].push` | `Vec<&'a S>::push(&mut self, v: &'a S)`: the lifetime travels in the type argument, so `push` needs no annotation | `add(element: E_{this})`, or Swift's `@_lifetime(self: copy self, copy v)`: the list's one lifetime narrows to cover `v` | `iterators += it` on `ListBuffer[IterableOnce[A]^{C}]`: the element's capture set is `C` |
| A struct with two handles from different scopes | `struct Session<'c, 'u>`: each field keeps its own lifetime | `local class Session(val client: T_{this}, val upstream: T_{this})`: `this` is the shorter of the two | `Session^{client, upstream}`, or precise with `class Session[C^, U^]` |
| Reading an element back | `&'u S`: it keeps its own lifetime | `copy self`: it gets the container's lifetime | `T^{C}`: it keeps the element's set |

**What family (2) loses against (1).** It cannot extract the longer-lived
field. In family (2), `s.upstream` has `s`'s single lifetime, which is the
shorter one, so storing it past `client`'s scope is rejected. Family (1)
gives it `'u`. The same loss shows up for any container that holds values
of different lifetimes: what you read back is bounded by the shortest.
Swift's proposal raises a second case. A `Span` over an `Array` with
non-Escapable elements would merge the dependency on the array's memory
with the elements' own dependencies. The proposal's example ends
`use(e) // error: e depends on 'a' (even though the original e1 and e2 didn't)`.
**This case does not arise in hd.** hd is garbage-collected and has no
borrow of memory, so a list has no lifetime apart from its elements'.

**Is Swift's "no `Array` of `~Escapable`" a limit of its model?** Partly.
The guess that function-level dependencies cannot express it is wrong.
Swift's own setter default, `self: copy self, copy newValue`, is a push. A
single-lifetime `Array<NE>` fits the current model. Three things stand in
the way:

- The `Array<Element>` declaration does not suppress `Escapable`.
- The proposal's precision problem: memory versus elements, above.
- An open question. "Mutating `~Escapable` elements in a referential
  container brings up questions about the lifetime requirements for those
  elements". That text is from the May 2026 pitch for `Span` and `Ref`
  elements, which leaves mutable containers out of scope.

Andrew Trick wrote in May 2026 that owning containers such as
`UniqueArray<~Escapable>` are expected to be proposed later. The limit is
therefore unfinished work, plus a stated wish for component lifetimes.
It is not an impossibility.

hd has the mutation question in a sharper form than Swift. hd allows
shared mutable aliases, and Swift's flow-sensitive narrowing of `inout`
relies on exclusivity. hd needs two things instead:

- lifetime slots that are invariant under `mut`;
- a lifetime held by the value, which every alias shares.

[C.4](#c4-rules-all-three-shapes-share) gives both rules.

### A.2 What The Checker Promises

To judge the claim, first fix the guarantee. The shapes below promise
one thing:

> **No alias of a value that a `defer` suite cleans up outlives that
> suite's block.**

That is lexical non-escape at the cleanup boundary. It does not promise
that cleanup runs at all, that it runs exactly once, or that an early
manual `close()` is caught. Those stay as checked
`ResourceError.Disposed` results under `module.resource.disposed`.

hd is garbage-collected, so a lifetime here never protects memory. It
protects one thing: the guarantee that aliases die before cleanup runs.
That makes the problem smaller than Rust's in three ways:

- Lifetimes come only from **cleanup roots**. A root is a value that a
  `defer` closes, or a parameter supplied by the caller.
- Lifetimes are lexical blocks, and blocks form a tree. A `defer` runs
  when its block exits, so non-lexical lifetimes, reborrows, and
  exclusivity buy nothing here.
- The only operation on a lifetime is inheriting it. Swift calls this
  `copy`. hd has no `borrow` form, because it has no memory to borrow.

**The defer pin (agent proposal).** A `defer` suite that mentions a
binding of a NonEscapable type pins that binding's lifetime to the
suite's block. No value stored in the binding may outlive the block. A
fresh handle that no `defer` mentions has a free lifetime, and inference
solves it from its uses. That lets a constructor helper return the handle
it opened:

```hd
fn open_db!() -> Result[mut dyn TcpStream, NetError] $ Net:
    let c: mut dyn TcpStream = $.use(Net).connect!("db", 5432)?
    _ := c.write!([1])
    .Ok(c)   # fine: no defer pins c, so its lifetime is the caller's
```

The same helper with a `defer` is the bug the checker exists to catch:

```hd
fn open_db!() -> Result[mut dyn TcpStream, NetError] $ Net:
    let c: mut dyn TcpStream = $.use(Net).connect!("db", 5432)?
    defer: _ := c.close()
    .Ok(c)   # error: c is closed when this block exits
```

The alternative is to root a fresh handle at its binding's block, as
Kotlin's `local val` does. It rejects the first helper too. See
[E, question 3](#e-owner-questions).

**Review note: the pin must reach the roots (orchestrator, 2026-10-14).**
As written, the pin bounds only the binding the `defer` mentions. Under
D1 and D2, a list or closure that holds `c` lives no longer than `c`.
Bounding the list therefore says nothing about `c`, and this program
passes:

```hd
fn leak!(ports: List[u16]) -> Result[mut dyn TcpStream, NetError] $ Net:
    let open: mut List[mut dyn TcpStream] = []
    let c: mut dyn TcpStream = $.use(Net).connect!("db", ports[0])?
    open.push(c)
    defer: close_all(open)   # pins open, not c
    .Ok(c)                   # c is closed when this block exits
```

The same hole opens through a closure: `cleanup := fn(): _ := c.close()`
then `defer: cleanup()`. The fix is to pin every root that each mentioned
value depends on: the handles it stores, and the handles it captures,
followed through the D1 and D2 dependencies. Lifetimes then behave as
sets of roots, as Kotlin's dependency sets do, not only as upper bounds.
Shapes A and B need this rule. Shape C needs it only where a slot is
elided.

### A.3 (i) A Type-Level Category

**Verdict: holds, but for annotation burden rather than soundness.**

The checker must know which values a `defer` pins. Here is the smallest
program that forces the question:

```hd
fn serve_one!(log: mut List[string], listener: mut dyn TcpListener) -> Result[void, ResourceError[NetError]]:
    let c: mut dyn TcpStream = listener.accept!()?
    defer: log.push("closed")   # log outlives this call: must not be pinned
    defer: _ := c.close()       # c must not outlive this block
    log.push("accepted")
    .Ok(())
```

If every value a `defer` mentions were pinned, `log` would be pinned too.
A parameter cannot be pinned, so the program would fail. Something must
separate `c` from `log`. There are three options:

| Option | Sound? | Cost |
| --- | --- | --- |
| Type category: `trait TcpStream < NonEscapable` | yes, for every value of a handle type | 3 markers in std; ordinary types never carry a lifetime |
| Mark the root at the use site, such as `defer c: ...` | yes | Any type can be pinned, so every composite type carries a lifetime. `fn register(u: User): USERS.push(u)` then needs a written `escaping u`, though it touches no resource. |
| Mark the producer's result, as Kotlin can with `-> T_{...}` | only for values from marked producers | a user's `FakeStream {}` converted to `dyn TcpStream` escapes the check |

So use-site marking is possible. It moves the burden onto code that
touches no resource. The category keeps the checker out of that code.
That is the real reason for (i).

The category must not be the parked TQ-24 rule, "a generic type is
NonEscapable exactly when an argument is". Two counterexamples refute it:

- **Annotation handles.** A handle has type `Field[S, F]`
  (`annot.handle.constants`). It stores no `S` or `F` value, and
  `annot.handle.escape` lets it escape. Under TQ-24 it would turn
  NonEscapable whenever a derived type holds a handle field.
- **Closures and suspensions.** A `mut Suspend[Result[void, ResourceError[NetError]]]`
  that captured `c` has no NonEscapable type argument, yet it holds `c`.

Propagation has to follow **stored fields**, not type arguments. Closure,
`Suspend`, and `dyn` values also carry their environment's lifetime. Rust
does this with `dyn Trait + 'a`, Kotlin with `(A) ->_{x} B`, and Scala
with `() ->{c} Unit`.

### A.4 (ii) Lifetime Relations In The Semantics

**Verdict: holds.** A yes/no marker treats every handle alike, but the
safety of a store depends on which block each value lives in:

```hd
fn batch!(ports: List[u16]) -> Result[void, NetError] $ Net:
    let open: mut List[mut dyn TcpStream] = []
    for port in ports:
        let c: mut dyn TcpStream = $.use(Net).connect!("db", port)?
        defer: _ := c.close()
        open.push(c)   # error: open outlives the loop body, where c is closed
    for c in open:
        _ := c.write!([0])   # each write would return Disposed
    .Ok(())
```

Move `let open` into the loop body, and the same `push` is safe. A
yes/no marker gives a checker two options:

- forbid storing a handle into any list, which rejects the safe version;
- allow it, which accepts the leak.

To accept one and reject the other, the checker has to compare blocks.
That comparison is a lifetime relation, even though nobody writes it.
Everything after this point concerns the third part of the claim.

### A.5 (iii) Written Lifetime Relations

**Verdict: overstated.** Inference handles every function body. A
signature needs a default for how lifetimes flow between inputs, the
result, and `mut` targets. These defaults are an agent proposal.
"Lifetime-carrying" means:

- a NonEscapable type;
- a closure, `Suspend`, or `dyn` value;
- a generic type whose fields hold one of these.

| Default | Rule | Precedent |
| --- | --- | --- |
| D1 | The result depends on every lifetime-carrying input: the parameters, `self`, the environment of a function-typed value, and the providers of NonEscapable requirement keys. | Swift `@_lifetime(copy a, copy b)`; Kotlin `T_{x&y}` |
| D2 | Each lifetime-carrying `mut` target, including `mut self`, takes on every lifetime-carrying input. | Swift's setter default `self: copy self, copy newValue`; Kotlin `add(element: E_{this})` |
| D3 | A result with no lifetime-carrying input is fresh. The caller's use solves its lifetime. | Swift `immortal`, but bounded by use |

With these defaults, the ten patterns come out as follows.

| # | Pattern | Shape A | Shape B | Shape C | Written? |
| --- | --- | --- | --- | --- | --- |
| 1 | `List[TcpStream].push` | rejects the leak (D2) | same | same, through the type argument | no |
| 2 | Setter on a `mut` aggregate | rejects the leak (D2) | same | same | no |
| 3 | Result depends on some inputs | sound but ties too much | precise with `local` | precise with `^a` | to accept good programs |
| 4 | Struct with two handles from different scopes | sound, one lifetime | sound, one lifetime | precise, `Session[^c, ^u]` | only for precision |
| 5 | Closure passed to a function-typed parameter | accepted | accepted; storing it in module state needs `escaping` | accepted; needs a static bound | only for module state |
| 6 | Cold `Suspend` stored, then driven or dropped | rejects the leak | same | same | no |
| 7 | `all!` children sharing a handle | accepted, structure suffices | same | same | no |
| 8 | Module top level and the REPL | static root, accepted | same | same | no |
| 9 | Provider values | D1 counts NonEscapable providers | same | same | no |
| 10 | Annotation handles | escapable, by the field rule | same | same | no |

**Writing is never needed to reject a bad program. It is needed to accept
some good ones.**

#### Pattern 1: A Generic Container `push`

This is the owner's example:

```hd
fn serve!(pool: mut List[mut dyn TcpStream]) -> Result[void, NetError] $ Net:
    let c: mut dyn TcpStream = $.use(Net).connect!("db", 5432)?
    defer: _ := c.close()
    pool.push(c)   # error: pool outlives the block where c is closed
    .Ok(())
```

`push(mut self, value: T)` needs no written annotation:

1. By D2, `pool`'s lifetime must fit within `c`'s.
2. `c` is pinned to the body.
3. `pool` is a parameter, so its lifetime outlives the body. That is a
   contradiction, so the push is an error.

In Shape C, the lifetime travels inside `T` instead, as in Rust.

#### Pattern 2: A Setter On A `mut` Aggregate

```hd
data Session:
    conn: mut dyn TcpStream

impl NonEscapable for Session

impl Session:
    fn attach(mut self, c: mut dyn TcpStream) -> void:
        self.conn = c   # fine: D2 says self takes on c's lifetime
```

The caller's `session.attach(c)` makes `session`'s lifetime fit within
`c`'s. Any later use of `session` outside `c`'s block is an error. The
lifetime belongs to the value, not to the binding, so every alias shares
it, including `server.session`. Swift narrows `inout` flow-sensitively
instead, and that is sound only with exclusivity. hd has none.

#### Pattern 3: A Result That Depends On Some Inputs

```hd
data Upstreams:
    conns: List[mut dyn TcpStream]

impl NonEscapable for Upstreams

impl Upstreams:
    fn choose(self, client: dyn TcpStream) -> mut dyn TcpStream:
        self.conns[0]   # a real pick would hash the client's peer

fn proxy!(ups: Upstreams, listener: mut dyn TcpListener) -> Result[void, ResourceError[NetError]]:
    let sticky: mut List[mut dyn TcpStream] = []
    while true:
        let client: mut dyn TcpStream = listener.accept!()?
        defer: _ := client.close()
        sticky.push(ups.choose(client))   # Shape A: error, D1 ties the result to client
```

The program is safe: the result comes from `ups`. D1 cannot know that.

- Shape B writes `fn choose(self, local client: dyn TcpStream)`, as
  Kotlin's `local` and Swift's dependency lists do.
- Shape C writes `fn choose[^u](self ^u, client: dyn TcpStream) -> mut dyn TcpStream ^u`.
- Shape A offers a workaround: pass the client's address, a `string`.

Either input versus both inputs: no surveyed language lets a result
depend on "one input, chosen at run time". It depends on both. Rust's
shared `'a`, Kotlin's `x&y`, Scala's `{x, y}`, and Mojo's `ref [a, b]` all
do this, and so does D1.

#### Pattern 4: A Struct With Two Handles From Different Scopes

```hd
data Relay:
    client: mut dyn TcpStream
    upstream: mut dyn TcpStream

impl NonEscapable for Relay

fn relay!(idle: mut List[mut dyn TcpStream], listener: mut dyn TcpListener) -> Result[void, ResourceError[NetError]]:
    let up: mut dyn TcpStream = idle.pop().expect("no idle upstream")
    let client: mut dyn TcpStream = listener.accept!()?
    defer: _ := client.close()
    let r: mut Relay = Relay { client: client, upstream: up }
    idle.push(r.upstream)   # Shapes A and B: error, r has client's lifetime; Shape C: fine
    .Ok(())
```

With one lifetime per value, `r.upstream` gets `r`'s lifetime, and `r`
cannot outlive `client`. This is the family (2) loss from
[A.1](#a1-three-families-of-lifetime-notation). Only Shape C fixes it:

```text
data Relay[^c, ^u]:
    client: mut dyn TcpStream ^c
    upstream: mut dyn TcpStream ^u
```

The workaround in Shapes A and B is to keep `up` in its own binding and
not bundle it. Swift accepts the same limit today: "it models every value
as having at most one lifetime dependency".

#### Pattern 5: A Closure Passed To A Function-Typed Parameter

```hd
use std.task.retry

fn send_all!(c: mut dyn TcpStream, frames: List[List[u8]]) -> Result[void, ResourceError[NetError]]:
    for frame in frames:
        retry!(3, fn!() -> Result[void, ResourceError[NetError]]: c.write!(frame))?
    .Ok(())
```

The closure's lifetime is `c`'s. `retry!` only calls `attempt`, so by D1
its result is tied to `attempt` and nothing else goes wrong. This works
only if a function-typed parameter accepts a capturing closure by
default. The parked TQ-25 answer says a parameter must opt in. Under that
answer, `retry!`, `all_list!`, `map`, `filter`, and every `Iterator`
adapter would need the opt-in.

Storing a callback in module state is the case that needs writing:

```hd
let handlers: mut List[fn() -> void] = []

fn on_close(handler: fn() -> void) -> void:
    handlers.push(handler)   # error: handler may capture a handle; write `escaping handler`
```

A closure captures *bindings*, not values: `let` storage is shared, by
`fn.capture.storage.shared`. A captured binding therefore has one
lifetime, and every later assignment to it must fit that lifetime.

#### Pattern 6: A Cold `Suspend` Holding A Handle

```hd
fn ping!(c: mut dyn TcpStream) -> Result[void, ResourceError[NetError]]:
    c.write!([0])

fn warm_up!(queue: mut List[mut Suspend[Result[void, ResourceError[NetError]]]]) -> Result[void, NetError] $ Net:
    let c: mut dyn TcpStream = $.use(Net).connect!("db", 5432)?
    defer: _ := c.close()
    queue.push(ping(c))   # error: the cold frame holds c, and queue outlives c's block
    _ := ping!(c)          # fine: driven inside the block
    .Ok(())
```

A frame is a closure over its arguments and its construction-time
providers (`req.lowering.frame`, `req.bind.captured`). Under one rule for
all lifetime-carrying values, the owner's Shape B decision falls out with
no special case. The research document worried that capturing a handle
must "invalidate the caller's binding". **That worry goes away.** The
frame and the caller both alias `c` under the same root. Cleanup stays
with the root's `defer`, so nothing is transferred and nothing needs to be
affine.

Dropping a cold frame inside its block is fine (`req.discard.cold`). A
frame that has started and is then dropped must still be cancelled
(`req.discard.started`). That rule is a runtime matter and does not
change.

#### Pattern 7: `all!` Children Sharing A Handle

```hd
use std.task.all

fn exchange!(c: mut dyn TcpStream) -> Result[List[u8], ResourceError[NetError]]:
    let (sent, got) = all!(c.write([1]), c.read(64))   # two frames share c: fine in every shape
    sent?
    got
```

No lifetime check is needed for children that are driven by a bang call.
Structure already guarantees two things:

- The parent's block cannot exit while its bang call is pending.
- On cancellation, children finish their own cleanup before the parent's
  `defer` suites run (`req.cancel.defer`).

So the owner cannot be cleaned up while a child is suspended. hd has no
detached spawn, so a lifetime is needed only when a suspension is stored,
as in pattern 6. Both children aliasing `c` is allowed. Non-escape says
nothing about interleaving.

#### Pattern 8: Module Top Level And The REPL

Top level is not a cleanup scope (`flow.defer.not-scopes`), so nothing can
pin a value there. A fresh handle bound at top level is a **static
root**: it has no cleanup boundary to outlive. This holds for the entry
module's `block_on` and for REPL inputs, whose bindings are module
globals (`cli.repl.declarations.bindings`). A REPL session can keep
`conn := $.use(Net).connect!("example.com", 80)` and use it in later
inputs. No shape needs a session-wide lifetime.

#### Pattern 9: Provider Values

```hd
trait Pool < NonEscapable:
    fn get(mut self) -> mut dyn TcpStream

fn grab() -> mut dyn TcpStream $ Pool:
    $.use(Pool).get()   # D1: the result depends on the Pool provider
```

`req.use.value.flow` lets a provider value flow anywhere. For a
NonEscapable key it would then flow anywhere its lifetime allows. D1
treats row keys as inputs, because `req.use.value.row-meaning` already
says a row is an unresolved lookup. A suspension built under `$.with(Pool=p)`
captures `p` and inherits `p`'s lifetime, by `req.bind.captured`. No new
syntax is needed. This also answers the open-issues question of whether
provider values can be `NonEscapable`: yes, with no extra rule.

#### Pattern 10: Annotation Handles

`annot.handle.escape` stays as it is. A `Field[S, F]` stores no `S` or `F`,
so under the field rule it is escapable even when `F` is
`mut dyn TcpStream`. `h.get(x)` returns `F`. By D1 the result depends on
`h`, which is static, and on `x`, which is exact. This is the
counterexample to TQ-24 in [A.3](#a3-i-a-type-level-category).

### A.6 Must Lifetimes Live In Types Or On Functions?

Lifetimes must be **tracked** in types. Every lifetime-carrying value has
at least one slot. Swift's `@lifetime` is written on functions, yet Swift
also attaches one lifetime to each `~Escapable` value. Without that slot,
`let x = f(a); store(x)` could not be checked.

The question is where lifetimes are **written**:

| Shape | Slots per value | Written on |
| --- | --- | --- |
| A | one, inferred | nothing |
| B | one, inferred | function parameters (`local`, `escaping`) |
| C | one per declared parameter | functions and types (`[^a]`, `T ^a`) |

**What Swift can express today (6.2 and 6.3, experimental `Lifetimes`):**

- `~Escapable` structs and enums. Stored fields need a `~Escapable`
  container (SE-0446).
- `Optional` and `Result` of `~Escapable` values (SE-0465).
- Results with `@_lifetime(copy x)`, `borrow x`, `&x`, or `immortal`.
- Targets of `mutating` methods and `inout`, as in
  `@_lifetime(span: copy another)`. The setter default is
  `self: copy self, copy newValue`.
- Nonescaping closures that capture `~Escapable` values.
- Function types with lifetimes, as in
  `@_lifetime(copy ne0) (_ ne0: NE, NE) -> NE`.

**What Swift cannot express today:**

- `Array`, `Dictionary`, or `Set` of `~Escapable` elements.
- `Span` or `Ref` of `~Escapable` elements (pitched in May 2026).
- Escaping closures, closures passed to `Task`, `Task.detached`, and
  `addTask`, globals, and `Any` holding a `~Escapable` value.
- More than one lifetime per value.

The `@lifetime` proposal is still open as of February 2026 (#3145). It
lists component lifetimes and lifetime types as future directions.

### A.7 The Parked Answers

| Parked answer | Status after this study |
| --- | --- |
| TQ-24: declared and checked; "a generic type is NonEscapable exactly when an argument is"; erasure to `Any` or a trait value is rejected | Keep "declared and checked" for concrete data types. Replace the argument rule with the field rule. Replace "erasure rejected" with "erasure keeps the lifetime" (`dyn Trait` carries a slot, like Rust's `dyn Trait + 'a`). |
| TQ-25: a generic parameter opts in, like Swift | Reverse it: lifetime-carrying by default, as Rust's `T` is. `escaping` opts out. Opt-in would put markers on every callback API in std (pattern 5). |
| TQ-26: no NonEscapable returns; later, depend on every NonEscapable parameter | Returns are needed now: `connect!`, `accept!`, and `Iterator::from_fn` all return one. "Depend on all" becomes D1, and `local` narrows it. |

## B. Comparison

The eight questions for each language, and then for hd under Shape B.

| Question | Rust | Swift (6.2+, experimental `Lifetimes`) | Kotlin local lifetimes (design note) | Scala capture checking (3.9, experimental) | Mojo origins | hd, Shape B |
| --- | --- | --- | --- | --- | --- | --- |
| 1. Return a borrowed value? | Yes. `fn f<'a>(x: &'a T) -> &'a U`, elided with one input | Yes. `@_lifetime(copy x)` or `borrow x`; a same-type default exists. A plain return is an error | Yes. `-> Iterator<B>_{this&transform}` | Yes. The result type names its captures: `Writer^{output}` | Yes. `-> ref [self] T`; the origin is required on a `ref` return | Yes. D1: depends on every lifetime-carrying input, `local` removes one, and no input means fresh |
| 2. Store in a struct? Does the struct inherit? | Yes. `struct S<'a> { r: &'a T }`; each field keeps its own lifetime | Only in a `~Escapable` struct, which has one lifetime | Only in a `local class` with `T_{this}` fields; the instance gets one inferred lifetime | Yes. The class captures what its constructor keeps (`Logger^{fs}`), or `class C[X^]` | Yes. `struct ImmutRef[origin: ImmOrigin]`; origins are parameters | Yes. Concrete data declares `impl NonEscapable for S`, and generics follow their fields; one lifetime per value |
| 3. Multiple inputs: either, both, or one selected | Both: a shared `'a`. One: separate `'a` and `'b` | Both: `copy a, copy b`. One: list only that one | Both: `_{x&y}`. One: `_{x}` | Both: `^{x, y}`. One: `^{x}` | Both: `ref [a, b]`. One: `ref [a]` | Both: D1. One: `local` on the others. Either: both |
| 4. Closures: borrow or own captures? | Borrow by default, mode inferred; `move` owns. Typed as `impl Fn() + 'a` | Nonescaping closures may capture `~Escapable` values; escaping closures may not | They capture; `(A) ->_{x} B`; `local` callback parameters | The type records captures: `() ->{f} Unit`. `->` is pure and `=>` is any | Explicit lists such as `{imm x}`, `{mut x}`, `{var x^}` (move); "A closure can't outlive the scope where it's declared" | A closure captures bindings, and its lifetime is theirs. Function-typed parameters accept it by default. `escaping` is needed to store it in module state |
| 5. Async: survive suspension, and what keeps the owner alive? | Yes, inside one future, whose type carries `'a`. The borrow checker keeps the owner alive while the future exists. `tokio::spawn` needs `'static` | SE-0446's rules apply to async functions and `async let`. `Task`, `Task.detached`, and `addTask` take escaping closures, so they cannot capture | The note sketches a "lexical suspend" for `local` parameters. It needs coroutines that are not state machines, and it does not settle whether local values survive suspension | Futures capture: `collect[T, C^](fs: Set[Future[T]^{C}])(using Async^)`. Scope bounds what may escape | "Mojo doesn't support escaping closures or async execution" | Bang-driven children: structure alone suffices (pattern 7). A stored cold `Suspend` carries its captures' lifetime (pattern 6) |
| 6. Non-escape by default, or opt-in? | Borrows are opt-in through `&` types; generics accept borrowed types by default, and `T: 'static` opts out | Opt-in `~Escapable`; generics are `Escapable` by default | Opt-in `local`; values are `global` by default | Opt-in through the language import; type parameters accept capturing types | `ref` is a separate convention, and owned values escape | The category is opt-in per type (handles). Generic and function-typed positions accept lifetimes by default; `escaping` opts out |
| 7. On the type or at each use? | Type: `&'a T` and lifetime parameters | Type: `~Escapable` | Use: `local` parameters, plus `local class` for aggregates | Both: capability classes, or `T^` at use | Type parameters (`Span[T, origin]`) plus `ref` at use | Type, for roots. Use (`local`), for precision |
| 8. Syntax | `'a`, `<'a>`, `+ 'a`, `T: 'a` | `~Escapable`, `@_lifetime(copy x)`, `@_lifetime(self: copy v)` | `local x`, `T_{x}`, `_{x&y}`, `->_{x}`, `<local x>` | `T^{x}`, `->{x}`, `[C^]`, `any` | `ref [o] T`, `[o: Origin[mut]]`, `origin_of(x)` | None in bodies; `trait K < NonEscapable`, `impl NonEscapable for S`, `local x`, `escaping x` |

Maturity: Rust and Mojo 1.0 ship their models. Swift ships
`~Escapable`, but its lifetime annotations are experimental. Scala's
capture checking is experimental. Kotlin's is a research note, not a
proposal.

## C. Candidate Shapes

### C.1 Shape A: One Lifetime Per Value, Elided

This is Swift's current model, with hd defaults and nothing written.

| Item | Form |
| --- | --- |
| Category | `pub trait TcpStream < NonEscapable:`; `impl NonEscapable for Relay` on concrete data |
| Functions | no lifetime syntax; D1, D2, D3 |
| Data types | one lifetime, the shortest of the fields; generic types follow their fields |
| Traits | a method signature uses D1 to D3; a `dyn` value carries one slot |
| Closures and function types | `fn(A) -> B` carries its environment's lifetime, and its result depends on its arguments and environment (D1) |
| `Suspend` | a frame is a closure over its arguments and providers |
| Elision | all of it; bodies infer per-value slots over the block tree |
| Generics | type parameters accept lifetime-carrying arguments, and a slot's variance follows `types.polarity` |
| Erasure | `dyn Trait` and `dyn Any` keep the slot. `downcast` returns the slot of the `dyn` |
| `defer` | pins every NonEscapable binding it mentions to its block |

**What Shape A rejects that users want:**

- results tied to more inputs than they use (pattern 3);
- extracting the longer field (pattern 4);
- storing any callback or generic value in module state (pattern 5). With
  no syntax, there is no way out.

That last gap makes Shape A too small on its own. It is the floor, not a
candidate to ship.

Borrows from: Swift SE-0446 and the defaults of its lifetime-annotation
reference.

### C.2 Shape B: Shape A Plus Named-Value Dependencies

This adds two parameter modifiers and nothing in types.

```text
impl Upstreams:
    fn choose(self, local client: dyn TcpStream) -> mut dyn TcpStream

fn on_close(escaping handler: fn() -> void) -> void

impl[T < Eq & Hash] Set[T]:
    pub fn remove(mut self, local value: T) -> bool
```

| Item | Form |
| --- | --- |
| `local x` | `x` is not retained. It is dropped from D1 and D2. The body must not store `x` or return anything derived from it (Kotlin `local`) |
| `escaping x` | `x` must be static. The body may store it anywhere, and callers cannot pass a value tied to a scope (Swift `@escaping`, Rust `'static`) |
| Generic bound | `[T < Escapable]`, for erasing `T` into module state |
| Data, traits, closures, `Suspend`, `defer` | as in Shape A |
| Elision | everything except the two modifiers |

**What Shape B rejects that users want:** extracting the longer field
(pattern 4). The workaround is to keep separate bindings. Swift and Kotlin
ship with the same gap.

Borrows from: Kotlin's local-lifetimes note (`local`), Swift's
`@escaping`, and Rust's `'static`.

### C.3 Shape C: Lifetime Parameters, Elided By Default

This is family (1): Rust, Mojo, and Swift's future "lifetime types".

```text
data Relay[^c, ^u]:
    client: mut dyn TcpStream ^c
    upstream: mut dyn TcpStream ^u

data Iterator[T, ^s]:
    step: fn() -> T? ^s

trait TcpListener < NonEscapable:
    fn accept![^r](mut self) -> Result[mut dyn TcpStream ^r, ResourceError[NetError]]
```

| Item | Form |
| --- | --- |
| Functions | optional `[^a]` binders, and `T ^a` on lifetime-carrying types |
| Data types | lifetime parameters in the type-parameter list |
| Generics | `T` carries its own lifetimes inside, so `push` and `get` need nothing written |
| Elision | every unnamed slot in a signature is one shared lifetime; write `^a` only to separate slots |
| Variance | per lifetime parameter, checked like `+T` and `-T` |
| Erasure | `dyn Trait ^a`. `Inspectable` must require static values, because `downcast_mut` cannot rebuild several invariant slots from one. This is why Rust has `Any: 'static`. |
| `defer` | as in Shape A |

**What Shape C rejects that users want:**

- `Set.remove` with a probe shorter-lived than the set;
- `Error` values that hold handles, since `Error < Inspectable` and
  `Inspectable` must be static.

**What it costs:**

- a second kind of type parameter;
- lifetime variance;
- `'` is taken by character literals, so a new sigil is needed;
- Rust-style diagnostics about which lifetime does not outlive which.

Borrows from: Rust lifetimes, Mojo origins, and Swift's
`@lifetime(.a: copy arg1, .b: copy arg2)` future direction.

### C.4 Rules All Three Shapes Share

1. **Guarantee.** Stated in [A.2](#a2-what-the-checker-promises).
2. **The defer pin.** A `defer` pins the NonEscapable bindings it
   mentions. A value assigned to a pinned binding is pinned too, and
   cannot be weakened on the way in.
3. **A parameter cannot be pinned.** The Go idiom of
   `defer conn.Close()` in the callee is therefore an error. The caller
   writes the `defer` instead. Without moves, hd cannot hand cleanup to a
   callee. This is the price of having no affine types.
4. **The lifetime belongs to the value.** It is flow-insensitive and
   shared by all aliases. A slot under `mut` is invariant. Swift's
   flow-sensitive narrowing of `inout` needs exclusivity, which hd lacks.
5. **Slot variance comes from the declaration surface** (agent proposal).
   The existing `types.polarity` rules decide it, as they decide variance
   for `+T`. `TcpStream`'s methods take no lifetime-carrying input, so a
   `mut dyn TcpStream` slot is covariant. A long-lived handle may then
   enter a short-lived list. `Relay` has `mut` fields, so its slot is
   invariant.
6. **Static roots.** A value bound at top level, and every value with no
   root, is static.
7. **Cost model.** Each function body gets one variable per
   lifetime-carrying binding or expression. A constraint says that one
   lifetime fits within another, or within a block. Blocks form a tree,
   so solving is a meet over the ancestors of each block, plus union-find
   for the invariant equalities. That is linear in the number of
   constraints. Signatures act as summaries, so checking stays one
   function at a time, as `fn.decl.result-required-pub` intends. Shape C
   adds lifetime arguments to generic instantiation.

## D. Annotation Burden

**Scope: 127 signatures.**

| Module | Signatures | What they cover |
| --- | --- | --- |
| `std.net` | 12 | the methods of `Net` and of the three handle traits |
| `std.process` | 2 | `run!` and `ScriptedProcess::new`. std.process has **no child-process handle**: `run!` is one closed call, so nothing in it carries a lifetime |
| `std.task` | 7 | `race!`, `all!` (a typing rule), `all_list!`, `retry!`, `retry_with!`, `cancel`, `block_on` |
| `std.iter` | 20 | the `Iterator` methods, `from_iter`, and `iter` |
| `std.collections` | 70 | `List` (library methods and intrinsics), `Map`, `ListView`, `Deque`, `Set`, `Heap`, and `counts` |
| `option`, `result` | 16 | their methods |

| Count | Shape A | Shape B | Shape C |
| --- | --- | --- | --- |
| Type markers (`< NonEscapable` on `TcpStream`, `TcpListener`, `UdpSocket`) | 3 | 3 | 3 |
| Lifetime annotations that must be written | 0 | 0 | 2 (the `Iterator` declaration, `accept!`) |
| Annotations that are fully elided | 127 | 119 recommended, 109 minimum | 125 |
| Results tied to more inputs than they use (D1) | 7 | 0 with 7 `local` | 0 |
| `mut` targets tied to more inputs than they use (D2) | 11 | 1 with `local` on `Set.remove`, 10 left | 11 |
| `escaping` or a static bound needed (module state) | 0 | 0 | 0 |

The 7 D1 over-ties are `List.filter`, `sorted_by`, `sorted_by_key`,
`partition`, `find`, `Iterator.find`, and `TcpListener.accept!`. Their
callbacks return `bool`, `Ordering`, or a key that is thrown away, so the
result does not depend on them. `accept!`'s stream does not depend on the
listener.

`map`, `flat_map`, `fold`, `group_by`, `get_or`, `unwrap_or`, and the
`std.task` combinators are **exact** under D1. Their callbacks' results,
or the fallback, flow into what they return.

The 11 D2 over-ties are:

- 10 `Iterator` methods whose source iterator takes on the callback's
  lifetime: `filter`, `take_while`, `map`, `flat_map`, `any`, `all`,
  `find`, `fold`, `zip`, and `chain`;
- `Set.remove`.

The iterator ties bite only when the source is a named, long-lived
iterator that is used after the callback's block. Usually it is a
temporary, as in `xs.iter().filter(...)`. Removing all 18 over-ties in
Shape B takes 18 `local` markers, or 14% of the signatures. The
recommended 8 cover every tie that reaches a value the caller keeps.

Shape C needs no `local` markers, because elements carry their own
lifetime. It pays instead with a lifetime parameter on `Iterator`, which
holds a closure, and on `accept!`. It also changes every type grammar
position.

No std function in scope stores a `T` or a callback in module state.
No std function in scope erases a `T` to `dyn Any`. So `escaping` costs
nothing in std.

## E. Owner Questions

They are ordered so that each answer unblocks later ones.

1. **What does the checker promise?**
   Options:
   - (a) no alias of a value cleaned up by `defer` outlives the suite's
     block;
   - (b) also no alias of any NonEscapable value outlives the block of its
     binding;
   - (c) exactly-once close, which needs affine handles.

   **Recommendation: (a).** It is the defect that
   `flow.defer.alias-escape` names. (c) brings back moves, which hd
   ruled out.

2. **Category on the type, or at each use?**
   Options:
   - (a) a marker supertrait or impl: `trait TcpStream < NonEscapable`,
     `impl NonEscapable for Relay`;
   - (b) mark the root in the `defer`;
   - (c) mark producers' results, as Kotlin does.

   **Recommendation: (a).** (b) puts lifetimes on every composite type,
   and `fn register(u: User)` would need `escaping`. (c) misses
   user-written fakes ([A.3](#a3-i-a-type-level-category)).

3. **What fixes a fresh handle's lifetime?**
   Options:
   - (a) the block of each `defer` that mentions it (the defer pin);
   - (b) the block of its first binding.

   **Recommendation: (a).** (b) rejects the constructor helper `open_db!`
   in [A.2](#a2-what-the-checker-promises), which opens, configures, and
   returns a handle.

4. **How does the category propagate?**
   Options:
   - (a) through stored fields; closures, `Suspend`, and `dyn` carry their
     environment;
   - (b) through type arguments (TQ-24).

   **Recommendation: (a).** (b) is wrong both ways: `Field[S, F]` is
   needlessly restricted, and a `Suspend[void]` holding `c` escapes.
   Concrete data declares `impl NonEscapable`. Generic data follows its
   fields with no declaration, as `annot.self-ref.needs` does.

5. **Do type parameters and function-typed parameters accept
   lifetime-carrying values by default?**
   Options:
   - (a) yes, as Rust's `T` does, with `escaping` and `T < Escapable` to
     opt out;
   - (b) no, opt in, as Swift does and TQ-25 answered.

   **Recommendation: (a).** No std signature in scope needs the opt-out.
   Under (b), every callback API (`retry!`, `all_list!`, `map`, the
   adapters) needs the opt-in.

6. **Signature defaults.** Adopt D1, D2, and D3 from
   [A.5](#a5-iii-written-lifetime-relations)?
   **Recommendation: yes.** They make `push`, setters, `Suspend`, and
   providers sound with nothing written.

7. **Which shape?**
   Options: A, B, or C ([C](#c-candidate-shapes)).
   **Recommendation: B.** Shape A cannot store a callback in module state
   at all. Shape C buys only the two-handle extraction. It costs a second
   kind of type parameter, lifetime variance, a new sigil, and a static
   `Inspectable`.

8. **`defer` on a parameter: error or unchecked?**
   Options:
   - (a) error, and the caller writes the `defer`;
   - (b) allowed, as an early close that the caller sees as `Disposed`.

   **Recommendation: (a).** (b) quietly voids the guarantee for every
   alias the caller holds. The cost is the Go idiom of closing in the
   callee.

9. **Lifetime variance.** Work out a slot's variance from the
   declaration surface with `types.polarity`, and make it invariant under
   `mut`?
   **Recommendation: yes.** It is sound under shared mutable aliasing. It
   lets long-lived handles, whose trait surface takes no lifetime-carrying
   input, enter short-lived lists.

10. **Erasure.** Should `dyn Trait` and `dyn Any` keep the slot, as
    Rust's `dyn Trait + 'a` does?
    **Recommendation: yes.** TQ-24's "erasure is rejected" would forbid
    `dyn Debug` of a value that holds a handle. Under Shapes A and B,
    `downcast` returns the slot of the `dyn`. Under Shape C, `Inspectable`
    requires static values.

11. **Module top level and the REPL.** Should a handle bound there be a
    static root?
    **Recommendation: yes.** There is no cleanup boundary at top level,
    so there is nothing to outlive.

12. **Provider values.** May a requirement key be NonEscapable, with
    rows counted as inputs to D1 and D2?
    **Recommendation: yes.** `req.use.value.flow` and
    `req.use.value.outlives-scope` stay true for providers that can
    escape.

13. **Retire TQ-24 to TQ-26** in favour of answers 4, 5, and 6.
    **Recommendation: yes** ([A.7](#a7-the-parked-answers)).

14. **Spellings** (last): `NonEscapable` (Swift's established term),
    `local` (Kotlin), and `escaping` (Swift). Shape C's sigil cannot be
    `'`, which is the character literal.
    **Recommendation: take the established terms.** Choose a sigil only
    if Shape C is chosen.

## F. Round 2: Revised Design And Stress Tests

**Status.** This section is a draft that the orchestrator wrote with the
owner on 2026-10-14. It replaces sections C and E as the working
direction, and a follow-up job of #163 will merge it into the rest of
this file. Every spelling in it is a placeholder.

### F.1 What The Owner Asked For

1. NonEscapable is a category of types.
2. Lifetimes are part of the semantics and appear in signatures.
3. A result or a store may depend on any subset of the inputs, and the
   checker tracks which subset exactly.
4. The checker raises no false alarms. A default that is stricter than
   needed counts as a bug.
5. A struct may hold several handles whose lifetimes are independent.
6. Signatures stay short: no `local x` and no `T from x`.

### F.2 Core Model

- **A NonEscapable type has lifetime parameters.** This is Rust's model
  without borrows or exclusivity.
- **A lifetime is a set of roots.** A root is one of these:
  - the static scope: a top-level binding or a REPL input;
  - the caller, for a parameter;
  - the block whose `defer` closes the handle.
- **A handle that no `defer` here closes is free.** Its uses decide its
  lifetime, so a helper may open a handle and return it.
- **"Outlives" means used or escaping after the block**, as in Rust's
  non-lexical lifetimes. Being declared outside the block is not enough.
- **Locals are flow-sensitive.** Fields and container elements take
  their lifetime from their type's slots.
- **Closures, `Suspend` values and `dyn` values** each have one inferred
  slot. Erasing a value to `dyn` joins its slots into one.

### F.3 Notation

| Form | Meaning |
| --- | --- |
| `^a Conn` | A `Conn` usable while `a` lives. `a` is a parameter, `self`, a slot path such as `s.u`, a slot, or `static`. |
| `^a+b Conn` | Usable while both `a` and `b` live. This is a union, like `$ A + B` in rows. |
| `^() Conn` | Fresh: the value depends on no input. This mirrors the empty row `$()`. |
| `data S[^c, ^u]` | Declares two lifetime slots. A type that declares none has one implicit slot. |
| `closing c` | The callee closes this parameter's handle. |

The lifetime is a prefix, as in Rust's `&'a T`. A suffix is ambiguous on
function types: `fn!() -> void^static` reads as a lifetime on `void`.
Functions name parameters and paths, never lifetime variables, as Mojo's
`origin_of(x)` does. `data` declarations declare slots.

### F.4 Elision

| Position | Without `^` |
| --- | --- |
| A parameter | Its own lifetime, named by the parameter. |
| A result with no NonEscapable input | Fresh. |
| A result with one NonEscapable input | That input. |
| A method result with a NonEscapable `self` | `self`. |
| Any other result | An error at the declaration: write `^…`. |
| Storing one parameter into another | An error at that line, unless the type says so, as in `c: ^self Conn`. |

A lint suggests `^()` when a body never uses its input in the result.

### F.5 Closing

- `closing` is a parameter mode, like Swift's `consuming`. A handle may be
  closed only through a `closing` parameter or `self`, or through a local
  the function owns. Closing any other parameter is an error.
- Calling a closing method inside a `defer` pins every root of its
  argument to that block.
- A closing call outside a `defer` is an early close: a later use gives
  `Disposed` at run time.

### F.6 Stress Tests

The examples use `Conn` for a socket, `Listener`, and `connect!`. They
leave out `Result` handling.

| # | Source | Verdict |
| --- | --- | --- |
| R1 | Rust book `longest` | Works |
| R2 | Rust book dangling reference | Works |
| R3 | Rust `ImportantExcerpt` | Works, with nothing written |
| R4 | Rust `Parser<'c, 's>` | Works with one lifetime |
| R5 | Rust slice iterator | Works; no lending-iterator problem |
| R6 | Rustonomicon `evil_feeder` | Works; forced "outlives means used or escaping after" |
| R7 | Rust `thread::spawn` versus `thread::scope` | Works; forced prefix notation |
| R8 | Rust non-lexical lifetimes, reassignment | Works |
| R9 | Rust `TcpListener::accept` | Forced `^()` |
| S1 | Swift `Span` | Works, as a view over a handle |
| S2 | Swift `@lifetime(self: copy other)` | Works; `List[Conn]` needs nothing written |
| S3 | Swift `consuming` close wrapper | Found a hole; forced the `closing` parameter mode |
| M1 | Mojo `ref [a, b]` | Works |
| M2 | Mojo struct origin parameter | Works |
| T1 | Two independent slots | Works |
| T2 | Swapping two slots | Rejected, as in Rust |
| T3 | REPL top-level handle | Works: a static root |

**R1. `longest`** (Rust: `fn longest<'a>(x: &'a str, y: &'a str) -> &'a str`).

```hd
fn longer(a: Conn, b: Conn) -> ^a+b Conn:
    if a.sent() > b.sent(): a else: b
```

A caller whose `b` is closed in an inner block cannot carry the result
out of that block.

**R2. A dangling reference.**

```hd
fn dangle!() -> Conn:
    c := connect!("db")
    defer: c.close!()
    c                 # error: c is closed when dangle! returns
```

Without the `defer`, `c` is fresh and may be returned.

**R3. `ImportantExcerpt`** (Rust: `struct ImportantExcerpt<'a> { part: &'a str }`).

```hd
data Excerpt < NonEscapable:
    conn: Conn

fn excerpt(c: Conn) -> Excerpt:       # elided: ^c Excerpt
    Excerpt { conn: c }
```

**R4. `Parser<'c, 's>`.** Rust needs two lifetimes because the parser
borrows a temporary `Context`.

```hd
data Router < NonEscapable:
    pool: Pool

fn pick(self: Router) -> Conn:         # elided: ^self, the conns the router reaches
    self.pool.first()

fn route(pool: Pool) -> Conn:
    Router { pool: pool }.pick()       # the result is ^pool: fine
```

A lifetime is the set of handles a value reaches, never the life of the
value itself. So a reference to a context carries no lifetime of its own,
and most of Rust's two-lifetime examples need only one lifetime here.

**R5. A slice iterator** (Rust: `next(&mut self) -> Option<&'a T>`).

```hd
data ConnIter < NonEscapable:
    conns: List[Conn]
    at: usize

fn next(self: mut ConnIter) -> Conn?   # ^self means the conns, not the iterator
```

hd has no borrows, so there is no `&mut self` lifetime to keep apart
from `'a`, and no lending-iterator problem.

**R6. `evil_feeder`** (Rustonomicon: invariance under `&mut`).

```hd
fn feed[T](slot: mut Box[T], v: T) -> void:
    slot.value = v

fn main!(ports: List[u16]) -> void:
    let keep: mut Box[Conn] = Box { value: connect!("a") }
    for p in ports:
        tmp := connect!("b", p)
        defer: tmp.close!()
        feed(keep, tmp)                # keep becomes ^tmp Box (invariant under mut)
    keep.value.write!([1])             # error: keep is used after tmp's block
```

Without the last line, Rust accepts this program, so "outlives" has to
mean used or escaping after the block.

**R7. `thread::spawn` versus `thread::scope`.**

```hd
fn mirror!(a: Conn, b: Conn) -> void:
    all!(pump!(a, b), pump!(b, a))     # fine: the children finish before any defer runs

let JOBS: mut List[fn!() -> void] = []      # a global, so its elements are ^static

fn later(c: Conn) -> void:
    JOBS.push(fn!(): c.write!([1]))    # error: the closure is ^c, and JOBS needs ^static

fn register(h: ^static fn!() -> void) -> void:
    JOBS.push(h)                       # fine, like Swift's @escaping
```

**R8. Reassignment.**

```hd
fn best(a: Conn, b: Conn) -> ^a Conn:
    let pick = b
    pick.write!([0])
    pick = a
    pick                               # fine: locals are flow-sensitive
```

**R9. `accept`.** In Rust an owned result has no lifetime. Here every
NonEscapable result has a source, so the one-input rule would tie the
accepted connection to its listener.

```hd
fn accept!(self: Listener) -> ^() Conn
```

**S1. A view over a handle** (Swift `Span` with `@lifetime(borrow self)`).

```hd
fn lines(c: Conn) -> Lines             # elided: ^c Lines

fn first_line!(l: Listener) -> string:
    c := l.accept!()
    defer: c.close!()
    lines(c).next!()                   # fine: string is not NonEscapable
```

**S2. A mutating store** (Swift `@lifetime(self: copy other)`).

```hd
fn adopt(self: mut Pool, c: ^self Conn) -> void:
    self.conns.push(c)
```

`List[Conn]` itself needs nothing written, because the element type
carries the lifetime. Swift does not yet support `Array<~Escapable>`.

**S3. A cleanup wrapper** (Swift `consuming func close()`, Rust `drop(self)`).

```hd
data Session[^c, ^u] < NonEscapable:
    client: ^c Conn
    upstream: ^u Conn

fn shutdown!(closing self: Session) -> void:
    self.client.close!()
    self.upstream.close!()

fn proxy!(l: Listener) -> Conn:
    s := Session { client: l.accept!(), upstream: connect!("db") }
    defer: s.shutdown!()               # a closing call: pins every root of s
    s.client                           # error

fn handle!(closing c: Conn) -> void:
    defer: c.close!()                  # legal: the caller handed c over to be closed
```

Without the `closing` mode, a wrapper that closes its fields would pin
nothing, and `s.client` would escape while closed.

**M1, M2. Mojo.** `fn pick(a, b) -> ref [a, b] String` is
`-> ^a+b Conn`. `struct Span[T, origin: Origin]` is
`data Window[^src]`, and a `ref [self]` result is the elided `^self`.

**T1. Independent slots.**

```hd
fn client_of(s: Session) -> ^s.c Conn:
    s.client

fn proxy!(l: Listener) -> Conn:
    keep := l.accept!()
    up := connect!("db")
    defer: up.close!()
    client_of(Session { client: keep, upstream: up })   # fine: only ^keep comes out
```

**T2.** `s.client = s.upstream` is an error unless `^c` and `^u` are the
same lifetime, as in Rust.

**T3.** A REPL input `c := connect!("db")` binds a static root.


## Sources

- Kotlin: [Local Lifetimes for Kotlin, design notes](https://github.com/Kotlin/KEEP/blob/main/notes/0007-local-lifetimes.md).
- Swift:
  - [SE-0446 Nonescapable Types](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0446-non-escapable.md);
  - [`@_lifetime` reference guide](https://github.com/swiftlang/swift/blob/main/docs/ReferenceGuides/LifetimeAnnotation.md);
  - [Compile-time lifetime dependency annotations, PR #3145](https://github.com/swiftlang/swift-evolution/pull/3145) and its [proposal text](https://raw.githubusercontent.com/swiftlang/swift-evolution/refs/pull/3145/head/proposals/NNNN-lifetime-dependency.md);
  - [earlier draft, PR #2750](https://github.com/swiftlang/swift-evolution/pull/2750) and its [proposal text](https://raw.githubusercontent.com/swiftlang/swift-evolution/refs/pull/2750/head/proposals/NNNN-lifetime-dependency.md);
  - [Pitch #3](https://forums.swift.org/t/pitch-3-compile-time-lifetime-dependency-annotations/84968);
  - [Pitch #2, on closures](https://forums.swift.org/t/pitch-2-lifetime-dependencies-for-non-escapable-values/78821);
  - [Nonescapable elements in `Span` and `Ref`, May 2026](https://forums.swift.org/t/pitch-allow-nonescapable-elements-in-span-and-ref/86835);
  - [owning containers of `~Escapable` elements to come later](https://forums.swift.org/t/using-unsaferawpointer-to-access-in-memory-escapable-types/86441).
- Scala:
  - [capture-checking basics](https://docs.scala-lang.org/scala3/reference/experimental/capture-checking/basics.html);
  - [capability polymorphism](https://docs.scala-lang.org/scala3/reference/experimental/capture-checking/polymorphism.html).
- Mojo:
  - [lifetimes and origins](https://mojolang.org/docs/manual/values/lifetimes/);
  - [closure declarations](https://mojolang.org/docs/reference/closure-declarations/);
  - [closures](https://mojolang.org/docs/manual/values/closures/).
- Rust:
  - [lifetime syntax](https://doc.rust-lang.org/book/ch10-03-lifetime-syntax.html);
  - [closure expressions](https://doc.rust-lang.org/reference/expressions/closure-expr.html);
  - [`tokio::spawn`](https://docs.rs/tokio/latest/tokio/task/fn.spawn.html).
- hd: `spec/lang/06-control-flow.md` (Deferred Cleanup),
  `spec/lang/11-requirements-and-suspension.md` (Provider Values,
  Suspending Functions, Cancellation), `spec/lang/14-annotations.md`
  (Handles), `spec/std/net.md`, `spec/std/process.md`, and `lib/std`.
