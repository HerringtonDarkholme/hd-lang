# Ownership And Borrows: Design Study

This is a design study for issue #163. Nothing in it is accepted
behavior, and the spec is frozen. Every spelling and diagnostic name is
a placeholder. The study replaces the NonEscapable design of
#163, which is deleted; git history keeps it. An adversarial review of
an earlier draft is in [Ownership Adversarial Review](OWNERSHIP_STRESS_TEST.md).

- [Decisions So Far](#decisions-so-far)
- [1. Motivation](#1-motivation)
- [2. The Model On One Page](#2-the-model-on-one-page)
- [3. A Guided Tour](#3-a-guided-tour)
- [4. The Rules](#4-the-rules)
- [5. What Changes From NonEscapable](#5-what-changes-from-nonescapable)
- [6. Open Points](#6-open-points)
- [Appendix: Limits And Holes](#appendix-limits-and-holes)

## Decisions So Far

The owner chose Rust's model: ownership, moves, and borrows with
lifetimes, including borrows inside types. hd stays garbage-collected
and keeps `defer`, with no `Drop` or RAII. NonEscapable is dropped.

Plain `T` and `mut T` keep today's garbage-collected object semantics,
untouched. `own T` and `^T` are new semantics that code opts into, and
the two worlds never mix: an `own T` where a plain `T` is expected is a
type error.

`own` is written at the usage site, on the type at each position, as
`mut` is. `own T` is the only path to its value, and it grants
mutation; there is no `own mut T`. A borrow is written `^T` or `^mut T`,
and a lifetime is named by a variable, as `^{x}`. The checker checks
every `^` of a local, whatever its type, and a global may be borrowed
as if it were owned.

An owned value is unique, not linear: it may simply be dropped. Only a
resource type, such as a connection or a file, must be consumed. A
fresh value may start owned, as `let own user = User {...}`, if its
plain fields are fresh too; borrowed fields are exempt. Generics follow
`mut`: `T` may be `own Conn`.

A closure that captures a value exclusively, an owned value or a `^mut`
borrow, can be called once. A job that must run repeatedly with
exclusive access is named data with `^mut self` methods.

No declaration rule makes a resource always owned. Earlier decisions
hold: anything local shows in source, a store writes its destination on
the stored value, and a downcast needs a `static` lifetime.

Everything else here is the draft's proposal and needs the owner's
review. [Open Points](#6-open-points) lists what is undecided. The full
discussion, with the owner's words, is in this file's git history.

## 1. Motivation

hd is garbage-collected, and the collector handles memory well. It does
not handle what memory points at: a database connection, an open file,
a socket. These must be closed at the right time, and today the type
system says nothing about them.

These three mistakes compile today:

```text
fn report!(db: Conn) -> void:
    db.close()
    db.send!([1])                         # used after close: fails at run time

fn count!(host: string) -> usize:
    c := connect!(host)
    c.sent()                              # never closed: the socket leaks

let RECENT: mut List[Conn] = []

fn audit(c: Conn) -> void:
    RECENT.push(c)                        # kept after the caller closes it
```

Other collected languages rely on conventions: Go's `defer f.Close()`,
Java's try-with-resources, Python's `with`. Nothing checks them. Rust
catches all three mistakes at compile time, with ownership and borrows.

The idea in one sentence: **an `own` value has one owner, a resource
must be consumed exactly once, a `^` value is borrowed for a checked
lifetime, and every plain value stays as it is.**

## 2. The Model On One Page

Every position that holds a value holds it in one of three ways: a
parameter, a result, a local, a field, or a type argument. The way is
written on the type at that position, as `mut` is. So one type, such as
`File`, can be plain in one place and owned in another.

- **Plain** `T` and `mut T` are today's values, with garbage-collected
  object semantics. Copying makes an alias, and nothing is checked.
- **Owned** `own T` is the only path to its value. Using it by value
  moves it, and the owner may mutate it. It may be dropped, unless its
  type is a resource.
- **Borrowed** `^T` or `^mut T` refers to a place it does not own, for a
  lifetime the checker tracks.

A **resource** is a type that must be closed, such as a file or a
connection. The draft marks one with a std trait, here called `Close`.
An owned value with a resource part must be consumed exactly once; any
other owned value may simply go out of scope.

```text
data File:
    fd: i32

impl Close for File:                                             # hypothetical syntax
    fn close(own self) -> void:                                  # hypothetical syntax
        match self:
            File { fd } => sys_close(fd)

fn open(path: string) -> own File: File { fd: sys_open(path) }   # hypothetical syntax
fn size(f: ^File) -> usize: sys_size(f.fd)                       # hypothetical syntax

fn main() -> void:
    f := open("app.log")                  # f is an own File
    n := size(f)                          # lends f for the call
    f.close()                             # gives f away
    m := size(f)                          # error: use-after-move
```

`open` gives ownership, and `:=` keeps it. `size` borrows, so the caller
still owns `f` after the call. `close` takes ownership and takes the
value apart, which consumes it. The last line is rejected with
`use-after-move`. `File` is a resource, so a `main` that never closed
`f` would get `own-not-consumed`.

| | Plain `T` | Owned `own T` | Shared `^T` | Mutable `^mut T` |
| --- | --- | --- | --- | --- |
| Passing or assigning it | makes an alias | moves it | copies the borrow | moves the borrow |
| Must be consumed | no | only with a resource part | no | no |
| May be kept | anywhere | anywhere, by moving | within its lifetime | within its lifetime |
| Mutation | with `mut T` | yes, by the owner | no | yes, exclusive |
| Checked | only where `^` is written | always | always | always |

The two worlds never mix. A plain copy of an owned value would be a
second path, and a plain value may already have aliases. A fresh value
is the one way in, since it has no other path yet.

**How a resource stays owned.** Its producers return `own T`, and no
owned value becomes plain; no rule on the type declaration is needed.
The ways it can fail are the author's own choices, listed in the
[appendix](#appendix-limits-and-holes).

Rust, Swift (`~Copyable`), and Mojo put copyability on the type and the
passing convention on the parameter. hd puts both on the position, as
`mut`. The price is the uniqueness rule for fresh values and two open
holes, B7 and B8, that a type-level category would close.

## 3. A Guided Tour

The tour uses this prelude. The I/O methods borrow the connection, and
`close` consumes it. A socket is a resource, so it extends `Close`. No
declaration carries `own`; `connect!` returns it.

```text
trait Close:
    fn close(own self) -> void                                # hypothetical syntax

trait Socket < Close:
    fn send!(^self, bytes: List[u8]) -> void                  # hypothetical syntax
    fn read!(^self) -> string                                 # hypothetical syntax
    fn sent(^self) -> usize                                   # hypothetical syntax

type Conn = dyn Socket

fn connect!(host: string) -> own Conn: sys_connect!(host)    # hypothetical syntax
```

### 3.1 Opening And Closing With `defer`

```text
fn fetch_user!(host: string, id: i32) -> string:
    c := connect!(host)
    defer: c.close()
    c.send!(encode(id))
    c.read!()
```

`connect!` returns an owned connection. A connection is a resource, so
it must be consumed on every path out of its scope. `defer: c.close()`
consumes `c` when the block exits. Registering it **reserves** `c`: it
may be borrowed, never moved or assigned (`reserved-by-defer`).

The common mistake closes at the end instead:

```text
fn fetch_user!(host: string, id: i32) -> string:
    c := connect!(host)
    c.send!(encode(id))                   # error: own-not-consumed
    reply := c.read!()
    c.close()
    reply
```

The checker reports `own-not-consumed` at `send!`. A bang call is a
point where cancellation may arrive, and cancellation runs only
registered `defer` suites. Cancelled there, `c` would leak. `?`,
`return`, `break`, and `continue` are exits too.

The same error covers a missing `close`, a `close` in one branch only,
and `_ := connect!("db")`. A panic path needs nothing, since a panic
runs no `defer` and poisons the instance. One cost follows: after a bang
call, a resource binding must already be reserved, so it cannot move.

None of this applies to an owned value that is not a resource. A
`let own buf: List[u8] = []` is unique and may be mutated by its owner,
and when the scope ends it is simply dropped.

To validate a connection and then hand it off, std offers an owned
holder, here called `Slot`. Its `defer` closes the content only if it is
still there, and a reserved slot may still be borrowed:

```text
fn adopt!(host: string, pool: ^mut Pool) -> void:   # hypothetical syntax
    let own slot = Slot.of(connect!(host))          # hypothetical syntax
    defer: slot.close()                             # closes the content if still there
    check!(slot.peek())                             # cancelled here: the defer closes it
    pool.give(slot.take())                          # moves the connection out
```

### 3.2 Lending And Giving Away

A log rotator borrows a file to measure it, then gives it away.

```text
fn line_count(f: ^File) -> usize: sys_lines(f.fd)       # hypothetical syntax
fn archive(f: own File) -> void:                        # hypothetical syntax
    sys_copy(f.fd, "archive/")
    f.close()

fn rotate(path: string) -> void:
    f := open(path)
    if line_count(f) > 1000:
        archive(f)
    else:
        f.close()
```

The call site writes no mark; the parameter's mode decides. `line_count`
borrows `f` for the call, and both branches consume it. Using `f` after
`archive(f)` is `use-after-move`.

Passing an owned value to a plain parameter is the other common
mistake:

```text
fn describe(f: File) -> string: "fd ${f.fd}"

fn show(path: string) -> void:
    f := open(path)
    println(describe(f))                  # error: own-to-plain
    f.close()
```

A plain parameter may keep its argument, so it cannot take the only
owner. The fix-its make it `^File`, or `own File` to take ownership. A
plain `self` method on an owned value fails the same way.

### 3.3 Returning A Borrow

```text
fn primary(pool: ^List[own Conn]) -> Option[^Conn]:   # hypothetical syntax
    pool.at(0)

fn busier(a: ^Conn, b: ^Conn) -> ^{a, b} Conn:        # hypothetical syntax
    if a.sent() > b.sent(): a else: b
```

A bare `^` in a result is **elided**: with one borrowed input, the
result lives as long as that input. In a `^self` method it takes the
receiver's lifetime. These are Rust's elision rules, unchanged.

With two borrowed inputs, a bare `^` is ambiguous. The result names its
sources, as `^{a, b}`: it lasts while both last. A caller that uses the
result after either source ends gets `lifetime-mismatch`.

```text
fn busier(a: ^Conn, b: ^Conn) -> ^Conn:               # hypothetical syntax: error: ambiguous-lifetime
    if a.sent() > b.sent(): a else: b

fn dangle(c: own Conn) -> ^{c} Conn:                  # hypothetical syntax
    c                                                 # error: borrow-of-local
```

`dangle` names an owned parameter, which dies with the call. A result
without `^` carries no lifetime. A store into another parameter writes
the destination on the stored value, as `c: ^{dst.T} Conn`.

### 3.4 A Struct That Holds A Borrow

A parser reads a document without copying it.

```text
data Document:
    lines: List[string]

data Parser[^doc]:                                    # hypothetical syntax
    source: ^{doc} Document                           # hypothetical syntax
    at: usize

fn parser(d: ^Document) -> Parser[^]:                 # hypothetical syntax
    Parser { source: d, at: 0 }

fn scan(path: string) -> void:
    let doc = load(path)
    p := parser(doc)
    doc = load("other.md")                # error: borrow-conflict
    _ := p.at
```

`data Parser[^doc]` declares a lifetime parameter after any type
parameters, and the field names it. A use writes the lifetime argument,
as `Parser[^{text}]`, or a bare `Parser[^]` that elides. Leaving it out
is `missing-lifetime`, with a fix-it that writes `[^]`.

`p` borrows `doc`, so `doc` is frozen while `p` lives. A struct may hold
several independent lifetimes, as `data Session[^client, ^upstream]`.
As in Rust, a field cannot borrow a sibling field.

An owned value may hold a borrowed field. A job that owns its output
file while it reads a borrowed document is fine:

```text
data Export[^doc]:                                    # hypothetical syntax
    out: own File                                     # hypothetical syntax
    source: ^{doc} Document                           # hypothetical syntax

fn export(d: ^Document, path: string) -> own Export[^]:   # hypothetical syntax
    Export { out: open(path), source: d }
```

The freshness rule for owned values covers plain fields only; a
borrowed field already states its lifetime and how it may be used.

### 3.5 A Pool That Owns Connections

```text
data Pool:
    idle: own List[own Conn]                          # hypothetical syntax

impl Pool:
    fn give(^mut self, c: own Conn) -> void:          # hypothetical syntax
        self.idle.push(c)
    fn peek(^self) -> Option[^Conn]:                  # hypothetical syntax
        self.idle.at(0)
    fn shutdown(own self) -> void:                    # hypothetical syntax
        for c in self.idle.into_iter():
            c.close()

fn warm!(hosts: List[string]) -> void:
    let own pool = Pool { idle: [] }                  # hypothetical syntax
    defer: pool.shutdown()
    for h in hosts:
        pool.give(connect!(h))
```

The pool owns its connections, and callers borrow them through `peek`.
`shutdown` moves the list out of the consumed pool, and `into_iter`
consumes every element. `List[own Conn]` is the ordinary list with
`T = own Conn`; `push` and `pop` move through `^mut self`.

The pool holds resources, so it has a resource part and must be
consumed too; its `defer` does that. A list whose elements are
resources rejects the methods that would drop them, such as `clear` or
`truncate`, with `resource-dropped`. `drain()` hands the removed values
back instead, as owned values the caller must consume. A list of plain
or non-resource owned values keeps every method.

The mistake here is Rust's too:

```text
fn reuse!(pool: ^mut Pool) -> void:                   # hypothetical syntax
    first := pool.peek()
    pool.give(connect!("backup"))         # error: borrow-conflict
    _ := first
```

`give` borrows the whole pool while `first` still borrows it; finishing
with `first` first fixes it. Through a plain holder, an `own` field can
only be borrowed (`move-out-of-plain`), so two aliases can never both
consume it.

### 3.6 Borrowing Plain Values

The borrow checker checks every `^` of a local, plain or owned. Plain
aliasing without `^` stays unchecked.

```text
fn total(xs: ^List[i32]) -> i32:                      # hypothetical syntax
    let sum = +0
    for x in xs:
        sum = sum + x
    sum

fn reprice() -> void:
    let prices: mut List[i32] = [+5, +7]
    let view: ^List[i32] = prices                    # hypothetical syntax
    prices.push(+9)                                  # error: borrow-conflict
    _ := total(view)
```

Writing `^` starts the check: while `view` lives, `prices` cannot be
changed through `prices`. The check guards the borrow, not the object.
A plain alias made earlier, or a closure that captured `prices`, still
follows garbage-collected semantics and may change the list; that is
plain `T` by design, and code that wants exclusive access uses `own`.

A `^` parameter is never kept beyond its lifetime:

```text
let CACHE: mut List[List[u8]] = []

fn remember(b: ^List[u8]) -> void:                   # hypothetical syntax
    CACHE.push(b)                                    # error: borrow-escapes
```

Code with no `^` and no `own` compiles as today. The cost lands on std:
a borrowed `List[u8]` reaches `len` only when `len` takes `^self`, about
400 public methods, each proposed by a lint.

### 3.7 Closures

A closure can capture a value in three ways, and each follows the mode
of what it captures.

| Capture | The closure | Calls |
| --- | --- | --- |
| A plain value | an ordinary plain closure; the capture is shared, as today | any number |
| A shared borrow `^c` | bounded by `c`'s lifetime, as `^{c} fn() -> usize` | any number, while `c` lives |
| An owned value, or a `^mut` borrow | an `own fn(...)`, holding the capture exclusively | once |

```text
fn on_close(f: own File) -> own fn() -> void:        # hypothetical syntax
    fn() -> void: f.close()

fn counter(c: ^Conn) -> ^{c} fn() -> usize:          # hypothetical syntax
    fn() -> usize: c.sent()

fn tidy() -> void:
    done := on_close(open("tmp.log"))
    done()
    done()                                           # error: use-after-move
```

**Capturing an owned value moves it in.** The closure becomes the only
path to the value, so it is itself owned, `own fn(...)`. A call consumes
it, as Rust's `FnOnce` does. The same holds for a `^mut` capture: two
calls could otherwise overlap, so the closure may run only once.

**A closure that holds a resource is a resource.** `done` above holds an
open file, so it must be consumed: called, passed to an `own fn`
parameter, or reserved with `defer: done()`. Dropping it is
`own-not-consumed`. A closure that holds an owned value that is not a
resource may be dropped, and the value goes with it.

**Repeated exclusive work is named data.** A job that runs many times
and needs exclusive access writes a type with a `^mut self` method,
not a closure:

```text
data Flusher:
    out: own File                                    # hypothetical syntax
    written: usize

impl Flusher:
    fn flush(^mut self, line: string) -> void:       # hypothetical syntax
        sys_write(self.out.fd, line)
        self.written = self.written + line.len()
```

**Closures and concurrency.** A child of `all!` or `race!` that captures
a resource reserves it with `defer` before its first bang call, so a
cancelled child still closes it. A closure stored for later, as in an
`own List[own fn!() -> void]`, keeps its obligation inside the list, and
the list's owner consumes it (see [3.5](#35-a-pool-that-owns-connections)).
An `own fn` is never a provider: `$.with` borrows its value.

A function type may name its parameters, as `fn(c: ^Conn) -> ^{c} Conn`.
A mark inside `Fn[...]` is `misplaced-lifetime`, with a fix-it to
`fn(...)`.

### 3.8 Generics

```text
fn same[T](v: T) -> T: v                             # T may be own Conn

fn first[T](items: ^List[T]) -> Option[^T]:          # hypothetical syntax
    items.at(0)

fn close_all[T < own Close](xs: own List[T]) -> void:    # hypothetical syntax
    for x in xs.into_iter():
        x.close()

fn bad[T](v: own T) -> void: pass                    # hypothetical syntax: error: own-on-type-parameter
```

Generics follow `mut`: `T` may be `own Conn`, as it may be `mut User`.
An unconstrained generic must not write `own T`. A body that consumes
`T` values asks with a bound, `T < own Any` or `T < own Trait`.

A plain `dyn K` holds no borrow, so a value that holds one converts only
to `^{a} dyn K`. A downcast needs a `static` lifetime.

### 3.9 Concurrency And Suspension

A worker borrows connections across children and hands owned ones to
bang calls.

```text
use std.task.{all, race}

fn echo!(c: ^Conn) -> void:                          # hypothetical syntax
    c.send!([1])

fn mirror!(a: ^Conn, b: ^Conn) -> void:              # hypothetical syntax
    _ := all!(echo(a), echo(b))

fn handle!(c: own Conn) -> void:                     # hypothetical syntax
    defer: c.close()
    c.send!([1])

fn later(c: own Conn) -> void:                       # hypothetical syntax
    pending := handle(c)                             # error: own-in-cold-frame
    pending.cancel()
```

Children of `all!` and `race!` may borrow the parent's places, since
they end inside the call. As with Rust's `thread::scope`, two children
cannot both borrow one place when either borrow is mutable.

A bang call such as `handle!(c)` may take an owned argument. A plain
call builds a cold frame, which may be discarded before its first poll,
running no `defer`. So passing it an owned value is `own-in-cold-frame`.

A cold frame is bounded by every borrow it captures, as
`^{keep, scratch} mut Suspend[^{keep} Conn]`.

**Children that produce resources.** If `all!` gets a connection back
from one child and then a sibling fails, the finished child's result
sits in a buffer that no `defer` covers. So in the draft, a child of
`all!` or `race!` must not return a value with a resource part
(`resource-in-combinator`). Each child uses and closes its own
connection, and returns a plain result:

```text
fn both!(a: string, b: string) -> void:
    pair := all!(connect(a), connect(b))     # error: resource-in-combinator

fn ping!(host: string) -> usize:
    c := connect!(host)
    defer: c.close()
    c.send!([1])
    c.sent()

fn ping_both!(a: string, b: string) -> usize:
    counts := all!(ping(a), ping(b))
    counts._0 + counts._1
```

A caller that needs the connections themselves opens them in sequence,
or lets each child give its connection to a pool it borrows.

**Leaving an owning loop early.** `for c in conns.into_iter():` consumes
the list one element at a time. A `break` or `return` inside it would
strand the remaining connections, so when the elements are resources
the draft rejects an early exit there (`resource-dropped`). Code that
must stop early drains the rest explicitly, as `conns.drain()` into a
`close_all`. Other owned elements may be left behind and are dropped.

### 3.10 Providers

```text
trait Upstream:
    fn get(^self) -> ^Conn                           # hypothetical syntax

fn grab() -> ^{$Upstream} Conn $ Upstream:           # hypothetical syntax
    $.use(Upstream).get()

fn serve!(p: ^dyn Upstream) -> void:                 # hypothetical syntax
    $.with(Upstream=p):
        grab().send!([1])
```

`$.with(K = p)` borrows `p` for the block. `$.use(K)` always returns a
borrow bounded by `$K`, and the result names it, since a key never
counts for elision. Plain code that kept a provider in module state is
now rejected.

### 3.11 Top-Level Bindings And The REPL

```text
let CONFIG: Settings = load_settings()

fn settings() -> ^{static} Settings:                 # hypothetical syntax
    let s: ^Settings = CONFIG                        # hypothetical syntax
    s
```

A shared borrow of a top-level binding has lifetime `static`. Within a
body, it is checked as a borrow of an owned local. A `^mut` borrow of a
top-level binding is an error in the draft, since other functions can
reach it without a path.

A top-level binding may hold an owned value. In the draft it is never
consumed; the instance's end releases it. REPL inputs are top-level
bindings, so a handle opened in a REPL cannot be closed under the draft
(open point 9 proposes letting a later input consume it):

```text
c := connect!("db")              # a REPL input: an owned top-level binding
c.send!([1])                     # a later input: a static borrow
c.close()                        # error: move-out-of-top-level
```

### 3.12 The Serialization Boundary

An owned position, a type with an `own` part, and a type that carries a
lifetime are not boundary-safe. So an owned handle cannot be a remote
argument, a remote result, or a sealed value. A portable closure
captures no borrow and no owned value.

```text
@remote
fn store(c: own Conn) -> void: c.close()             # hypothetical syntax: error: not boundary-safe
```

[Serializable Closures](SERIALIZABLE_CLOSURE_DESIGN.md) uses the same
wording: a portable closure captures plain boundary-safe data only.

## 4. The Rules

This section states each rule from the tour once, grouped by topic.

### 4.1 Owned Values

1. Plain `T` and `mut T` keep garbage-collected object semantics. The
   rules below apply only where `own` or `^` is written.
2. `own` is a type modifier, written where `mut` may be: a parameter,
   result, local, field, enum payload, type argument, or receiver.
3. `own T` grants mutation to its owner. `own` comes before `dyn`, as
   `own dyn K`, and `own mut T` is not a form.
4. `own` on a primitive type, `string` included, is `own-on-value`. A
   tuple keeps a mode per element. `own fn(...)` is a one-shot function.
5. An owned value comes from a fresh value, an `own` result or
   parameter, or a move of an owned place.
6. Any other plain value used as `own T` is `own-upgrade`. An owned value
   used as a plain `T` is `own-to-plain`.
7. `let own x = value` binds an owned local. `:=` keeps `own` from an
   owned initializer, and a fresh value under `:=` stays plain.
8. Every plain composite field of a fresh owned value is fresh or moved
   from an owned place. A primitive field value and a borrowed field are
   exempt.
9. A receiver (`own self`, `^self`, `^mut self`, and the plain forms)
   follows the same mode rules as a parameter.
10. A resource type implements the std trait `Close`. A value has a
    resource part when it is a resource, or an owned holder, list,
    closure, or frame that holds one.

### 4.2 Moves

1. Using an owned place by value moves it, and a later use is
   `use-after-move`. Assigning it again initializes it.
2. Assigning a place that still holds an owned value is
   `own-overwritten`.
3. Moving a place needs an owned step at every field on its path;
   otherwise it is `move-out-of-plain`.
4. Through an owned path, one field may move out alone. The holder is
   unusable whole until the field is assigned again.
5. A pattern that binds every field of an owned value consumes it. A
   private field moves out only in its type's module.

### 4.3 Borrows

1. A borrow is made by passing a place to a `^` parameter or receiver,
   or by binding it to a local written `^T`. No expression makes one.
2. `^mut` needs an owned place or a plain place with a `mut` view;
   otherwise it is `mutable-upgrade`.
3. While a shared borrow lives, its place is not assigned, mutated,
   moved, or mutably borrowed. That is `borrow-conflict`.
4. While a mutable borrow lives, its place is used only through it.
5. A borrow lives until the last use of any value whose type names its
   lifetime, as in Rust's non-lexical lifetimes.
6. A shared borrow copies. A mutable borrow moves, and a `^mut`
   parameter reborrows it for the call.
7. A part read through a borrow is a borrow with the same lifetime; a
   primitive is copied out. Moving through a borrow is an error.
8. A `^mut` borrow of a receiver starts after the arguments run, so
   `xs.push(xs.len())` is valid.
9. Rules 3 and 4 apply to every `^` of a local, owned or plain, and
   cover the borrowed variable only. A plain alias or a plain capture of
   the same object keeps garbage-collected semantics.
10. A borrow never reaches a top-level binding, a plain position, or a
    result that does not name it. That is `borrow-escapes`.

### 4.4 Lifetimes And Elision

1. A lifetime is written `^{a}` or `^{a, b}`. A name is a parameter,
   `self`, a path such as `xs.T`, a requirement key `$K`, or `static`.
2. Each borrow in a parameter's type has its own lifetime. A value
   bounded by `^{a, b}` is usable while both last.
3. `static` is the lifetime of a top-level binding and of every value
   that holds no borrow.
4. A value that flows into a result, store, or binding must last long
   enough; otherwise it is `lifetime-mismatch`.
5. A bare `^` in a result takes the one input lifetime, or the
   receiver's. Otherwise it is `ambiguous-lifetime`. Requirement keys
   never count.
6. A declaration lists lifetime parameters after its type parameters, as
   `data Pair[A, B, ^a]`. A use writes them, or `[^]`.
7. A lifetime parameter is covariant in readonly positions and invariant
   under `mut` or `^mut`.

| Position | Bare `^` | `^{a}` |
| --- | --- | --- |
| Parameter | a borrow with its own lifetime | a store into `a`'s place |
| Result | elided | bounded by `a` |
| Field | not allowed | names lifetime parameter `a` |
| Local written type | a borrow, lifetime inferred | bounded by `a` |

### 4.5 Resources And `defer`

1. An owned value without a resource part may be dropped at any exit.
2. At each exit of its scope, an owned binding with a resource part is
   consumed or reserved by a `defer`. Otherwise it is
   `own-not-consumed`.
3. The exits are the normal end, `return`, `break`, `continue`, `?`, and
   every bang call.
4. Where paths merge, a resource binding is consumed on all of them or
   on none. A loop consumes an outer one only if it assigns it again.
5. A resource temporary is consumed where it is made. A path that ends
   in a panic needs no consumption.
6. A `defer` that consumes a binding reserves it. A reserved binding may
   be borrowed, never moved or assigned; that is `reserved-by-defer`.
7. A method that would drop elements, such as `clear` or `truncate`, is
   `resource-dropped` on a container whose elements are resources.
   Removal that returns the values, such as `drain`, stays.
8. An early `break` or `return` out of a loop over an owning iterator of
   resources is `resource-dropped`.
9. A child of `all!` or `race!` must not return a value with a resource
   part; that is `resource-in-combinator`.

### 4.6 Generics, `dyn`, Closures, And Frames

1. A type parameter may be instantiated with an owned type, as
   `List[own Conn]`.
2. An unconstrained generic must not write `own T`. `T < own Any` asks
   for owned values, and such a body may consume them.
3. Inference never makes `T` owned. `^T` with `T = own U` is `^U`.
4. `own U` converts to `own dyn K`. A value holding a borrow converts
   only to `^{a} dyn K`, and a downcast needs `static`.
5. A closure that captures an owned value moves it in. A closure that
   captures an owned value or a `^mut` borrow is `own fn(...)`, and a
   call consumes it.
6. A closure that captures a shared borrow is bounded by it and may be
   called any number of times while it lives.
7. A capture of a plain binding stays shared storage, unchecked, as
   plain `T` is.
8. A cold frame is bounded by every borrow it captures. Only a bang call
   may take an owned argument; otherwise it is `own-in-cold-frame`.
9. `$.with(K = p)` borrows `p` for its block, and `$.use(K)` returns a
   borrow bounded by `$K`.
10. A top-level binding may be borrowed as `static`, never as `^mut`.
    Moving an owned one is `move-out-of-top-level`.
11. Owned positions and types that carry a lifetime are not
    boundary-safe.

## 5. What Changes From NonEscapable

The old design made NonEscapable a category of types. Here `own` and `^`
are modes at positions, an unmarked type is plain rather than global,
and lifetimes are Rust's. Use after close and a forgotten close, parked
before, are now checked, and `^mut` is exclusive.

| Use case | Old spelling | New spelling | Works? |
| --- | --- | --- | --- |
| A view over a handle | `data Excerpt: conn: ^Conn` with `impl NonEscapable` | `data Excerpt[^a]: conn: ^{a} Conn` | yes |
| A producer of a fresh handle | `fn accept!(^self) -> Conn` | `-> own Conn`, and the caller writes a `defer` | yes; a leak is now an error |
| A pool whose handles callers use | `conns: ^mut List[^Conn]`, `adopt(c: ^{self} Conn)` | `conns: own List[own Conn]`, `adopt(c: own Conn)` | yes, differently: the pool owns, callers borrow |
| Concurrency | `fn echo!(c: ^Conn)` | the same | yes for borrows; an owned cold frame is rejected |
| Providers | `trait Pool < NonEscapable`, `-> ^Conn $ Pool` | `trait Pool`, `-> ^{$Pool} Conn $ Pool` | yes |
| Callbacks | `List[^fn!() -> bool]` | `List[^{attempt} fn!() -> bool]` | yes; a global registry of owning closures is lost |
| An error carrying a handle | `enum LinkError: Broken(conn: ^Conn)` | `enum LinkError[^c]: Broken(conn: ^{c} Conn)` | yes |
| The REPL | `c := connect!("db")` | the same | use works; closing is rejected |
| Generic code | `fn look_at[T](x: ^T)` | the same | yes; an owned `T` in an unconstrained body is a hole |
| Locality of plain values | `fn fill(buf: ^mut List[u8])` | the same | yes, and the borrow freezes the variable |

Over these cases, the annotation count is about the same: 34 marks
before, 35 after. The extra marks are the `defer` lines that resources
demand; an owned value that is not a resource needs none.

The old design promised no false alarms. This one inherits Rust's and
adds a few of its own. Each rejects code the old design accepted:

- **Get or insert** (NLL problem case #3). A lookup's borrow stays live
  on the path that inserts. Rust rejects it too; the workaround checks
  `contains` first, then looks up once.
- **A method over one field.** A `^mut self` call while a borrow of a
  different field lives, as in Rust.
- **A helper that always panics** but returns `void` leaves a resource
  unconsumed. Declaring it `-> never` fixes it.
- **Mutating a plain local** while a `^` view of it lives, by the
  owner's decision to check every `^`.
- **Keeping a provider** from `$.use(K)` in module state.
- **An owned cold frame** polled later.

## 6. Open Points

1. **How a type becomes a resource.** Draft: it implements the std
   trait `Close`. Alternative: automatic for any type that holds a host
   handle, with nothing to write.
2. **A call-site move mark.** Draft: none; the parameter decides.
   Alternative: `archive(move f)`, as Mojo's `f^`.
3. **Plain parameters and borrows.** Draft: a plain parameter takes no
   borrow, so std writes about 400 `^self` marks. Alternative: it
   accepts one, and storing it becomes an error.
4. **A composite copied out of a borrow.** Draft: an error. Alternative:
   it becomes a plain alias, which breaks the "never kept" promise.
5. **Mutable borrows inside types.** Draft: allowed; the holder moves.
   Alternative: types hold only shared borrows.
6. **Primitive fields of an owned value.** Draft: exempt from the
   freshness rule. Alternative: every field value is fresh or owned.
7. **An owned value moved into a plain holder** (hole B7). Draft:
   allowed; a resource inside then leaks. Recommended: a holder that
   receives an owned value must itself be owned. Alternative: a type
   with an `own` field is never plain.
8. **An unconstrained generic with an owned `T`** (hole B8). Draft:
   allowed. Recommended: only `T < own Any` takes owned types, as
   Swift's `~Copyable`. Alternatives: every body moves `T`, or check each
   instantiation.
9. **Owned values at the top level.** Draft: borrow-only, never closed.
   Recommended: a later statement or REPL input may consume it.
   Alternative: forbid them.
10. **`^mut` of a top-level binding.** Draft: forbidden, which rules out
    global job queues. Recommended: allowed in a body that calls nothing
    able to reach it. Alternative: allowed, unchecked.
11. **Owned arguments to cold frames.** Draft: only a bang call may pass
    one. Alternative: allow it, and panic when such a frame is discarded
    unpolled.
12. **Resource results from `all!` and `race!`.** Draft: forbidden.
    Alternative: the combinator takes a cleanup function, called on each
    buffered resource when a sibling fails or the call is cancelled.
13. **Early exit from a loop over resources.** Draft: forbidden; drain
    the rest explicitly. Alternative: the loop closes the remaining
    elements, which needs every element type to implement `Close`.
14. **Rust's false alarms.** Draft: accept them as known limits.
    Alternative: avoid named ones, starting with problem case #3, as
    Polonius does.

## Appendix: Limits And Holes

Holes, where the draft is unsound:

- **B7.** A resource moved into a plain holder can never be consumed ([3.5](#35-a-pool-that-owns-connections)).
- **B8.** An unconstrained generic body copies an owned `T`, as `fn dup[T](v: T) -> (T, T)` does ([3.8](#38-generics)).

A plain alias or a plain closure capture that changes a borrowed
variable's object is not a hole: plain values keep garbage-collected
semantics by design ([3.6](#36-borrowing-plain-values)).

Limits, where the draft rejects a reasonable program:

- Owned handles in a global registry or job list need `^mut` of a global ([3.11](#311-top-level-bindings-and-the-repl)).
- A handle opened at the top level or in a REPL cannot be closed ([3.11](#311-top-level-bindings-and-the-repl)).
- An owned value cannot go into a cold frame polled later ([3.9](#39-concurrency-and-suspension)).
- After a bang call, a resource binding hands off only through a `Slot` ([3.1](#31-opening-and-closing-with-defer)).
- A child of `all!` or `race!` cannot return a resource ([3.9](#39-concurrency-and-suspension)).
- A list of resources has no `clear` or `truncate`, and its owning loop has no early exit ([3.5](#35-a-pool-that-owns-connections), [3.9](#39-concurrency-and-suspension)).
- A close that must suspend cannot run in a `defer` ([3.1](#31-opening-and-closing-with-defer)).
- A struct cannot borrow its own field ([3.4](#34-a-struct-that-holds-a-borrow)).
- A cleared list keeps its old elements' lifetime, as in Rust ([4.4](#44-lifetimes-and-elision)).
- A local `^dyn` value cannot be downcast ([3.8](#38-generics)).
- A borrowed value reaches a std method only once it takes `^self` ([3.6](#36-borrowing-plain-values)).
- Rust's false alarms, and the draft's own ([5](#5-what-changes-from-nonescapable)).

Ways a resource stops being owned, all by its author's choice ([2](#2-the-model-on-one-page)):

- A producer declared `-> Conn` instead of `-> own Conn` hands out handles nobody can close.
- Public fields let any module write `let own f = File { fd: 3 }`; private fields stop it.
- A handle type that derives `Default` or `Deserialize` can be forged the same way.

## Sources

- Rust Book, [ownership](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html) and [lifetimes](https://doc.rust-lang.org/book/ch10-03-lifetime-syntax.html)
- Rust Reference, [lifetime elision](https://doc.rust-lang.org/reference/lifetime-elision.html)
- Rust [RFC 2094, non-lexical lifetimes](https://rust-lang.github.io/rfcs/2094-nll.html)
- Swift [SE-0377, SE-0390, SE-0427, and SE-0446](https://github.com/swiftlang/swift-evolution/tree/main/proposals)
- Mojo Manual, [ownership](https://docs.modular.com/mojo/manual/values/ownership/)
- Hylo, [parameter passing conventions](https://docs.hylo-lang.org/language-tour/functions-and-methods)
- Austral, [linear types](https://austral-lang.org/tutorial/linear-types)

## Parse Log

Every `text` block, 25 in all, was parsed on 2026-10-16 with
`scripts/hd-conformance.sh parse`. Each was parsed as written and with
the marks erased (`own`, `^`, `^{...}`, lifetime brackets). Parsing
checks syntax only. Nothing was type-checked, and the blocks call
functions this study does not define.

| Result | Blocks |
| --- | --- |
| Parses as written | 6, among them section 1, both blocks of 3.1, the `all!` block of 3.9, and the REPL inputs in 3.11 |
| Fails only at the new marks; parses with the marks erased | the other 19 |
