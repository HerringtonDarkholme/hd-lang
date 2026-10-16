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
- [6. Still To Think Through](#6-still-to-think-through)
- [Appendix: Limits And Holes](#appendix-limits-and-holes)

## Decisions So Far

The owner chose Rust's model: ownership, moves, borrows with lifetimes,
including borrows inside types, and destructors. An owned value runs
its type's `drop` when its owner's scope ends without moving it; the
compiler knows that point exactly. The trait is `DropOwn`, since only
owned values run it, and a type that implements it is held only owned
or borrowed. `defer` stays as sugar for a std guard whose `drop` runs a
closure. NonEscapable is dropped.

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

An owned value is unique, not linear: nothing forces code to consume
it, and dropping it runs its destructor. No type is marked as a
resource, and there is no `close` protocol. A fresh value may start
owned, as `let own user = User {...}`, if its plain fields are fresh
too; borrowed fields and immutable values such as numbers and strings
are exempt. A holder that receives an owned value is owned too. A plain
type parameter never takes an owned type; a generic that accepts one
says `T < own Any`.

A borrow reaches only `^` positions, so std's read-only methods take
`^self`. A composite part read through a borrow is a borrow; the plain
contents of a borrowed collection are plain values. A type may hold a
`^mut` borrow, and such a holder moves.

A later top-level statement or REPL input may move a top-level owned
value. A `^mut` of a top-level binding is not allowed; code that needs a
global mutable queue uses plain values. A cleanup that must suspend or
can fail is an explicit method called before the drop. A move has no
mark at the call site. Rust's known false alarms are accepted as
limits.

A closure that captures a value exclusively, an owned value or a `^mut`
borrow, can be called once. A job that must run repeatedly with
exclusive access is named data with `^mut self` methods.

No declaration rule makes a handle always owned. Earlier decisions
hold: anything local shows in source, a store writes its destination on
the stored value, and a downcast needs a `static` lifetime.

Agent-written details beyond these, such as rule wording and diagnostic
names, need the owner's review. The full discussion, with the owner's
words, is in this file's git history.

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
rules out all three: a use after close and a kept borrow are compile
errors, and a forgotten close cannot happen, since the owner's scope
end runs the destructor.

The idea in one sentence: **an `own` value has one owner and is dropped
at a known point, a `^` value is borrowed for a checked lifetime, and
every plain value stays as it is.**

## 2. The Model On One Page

Every position that holds a value holds it in one of three ways: a
parameter, a result, a local, a field, or a type argument. The way is
written on the type at that position, as `mut` is. So one type, such as
`File`, can be plain in one place and owned in another.

- **Plain** `T` and `mut T` are today's values, with garbage-collected
  object semantics. Copying makes an alias, and nothing is checked.
- **Owned** `own T` is the only path to its value. Using it by value
  moves it, and the owner may mutate it. When the owner's scope ends
  and the value was not moved away, it is dropped.
- **Borrowed** `^T` or `^mut T` refers to a place it does not own, for a
  lifetime the checker tracks.

A type may implement the std trait `DropOwn`, whose `drop` method runs
when an owned value of that type is dropped. Because an owned value has
one owner, and no borrow outlives it, the compiler knows the exact
point: the end of the owner's scope, an assignment over it, or a
cancelled bang call. A type that implements `DropOwn` is never held
plain, so every value of it is dropped exactly once.

```text
data File:
    fd: i32

impl DropOwn for File:                                              # hypothetical syntax
    fn drop(^mut self) -> void: sys_close(self.fd)               # hypothetical syntax

fn open(path: string) -> own File: File { fd: sys_open(path) }   # hypothetical syntax
fn size(f: ^File) -> usize: sys_size(f.fd)                       # hypothetical syntax
fn archive(f: own File) -> void: sys_copy(f.fd, "archive/")      # hypothetical syntax

fn main() -> void:
    f := open("app.log")                  # f is an own File
    n := size(f)                          # lends f for the call
    archive(f)                            # gives f away; archive's scope end closes it
    m := size(f)                          # error: use-after-move
```

`open` gives ownership, and `:=` keeps it. `size` borrows, so the caller
still owns `f` after the call. `archive` takes ownership, and the file
closes when `archive` returns. The last line is rejected with
`use-after-move`. Had `main` kept `f`, it would close at the end of
`main`.

| | Plain `T` | Owned `own T` | Shared `^T` | Mutable `^mut T` |
| --- | --- | --- | --- | --- |
| Passing or assigning it | makes an alias | moves it | copies the borrow | moves the borrow |
| At its scope's end | nothing; the collector frees it later | dropped, running `drop` | the borrow ends | the borrow ends |
| May be kept | anywhere | anywhere, by moving | within its lifetime | within its lifetime |
| Mutation | with `mut T` | yes, by the owner | no | yes, exclusive |
| Checked | only where `^` is written | always | always | always |

The two worlds never mix. A plain copy of an owned value would be a
second path, and a plain value may already have aliases. A fresh value
is the one way in, since it has no other path yet.

**Why ownership gives a reliable `drop`.** A destructor in a collected
language cannot run at a known time: an alias may still be reachable,
so only the collector can decide, and finalizers run late or never.
Here the owner is the only path, and every borrow ends before the
owner's scope does. So the end of that scope is the last moment
anything can see the value, and `drop` runs exactly there.

**How a handle stays owned.** Its producers return `own T`, and no
owned value becomes plain; no rule on the type declaration is needed.
The ways it can fail are the author's own choices, listed in the
[appendix](#appendix-limits-and-holes).

Rust, Swift (`~Copyable`), and Mojo put copyability on the type and the
passing convention on the parameter. hd puts both on the position, as
`mut`. The price is the uniqueness rule for fresh values and two open
holes, B7 and B8, that a type-level category would close.

## 3. A Guided Tour

The tour uses this prelude. The I/O methods borrow the connection, and
dropping it closes it, since a socket implements `DropOwn`. No declaration
carries `own`; `connect!` returns it.

```text
trait Socket < DropOwn:
    fn send!(^self, bytes: List[u8]) -> void                  # hypothetical syntax
    fn read!(^self) -> string                                 # hypothetical syntax
    fn sent(^self) -> usize                                   # hypothetical syntax

type Conn = dyn Socket

fn connect!(host: string) -> own Conn: sys_connect!(host)    # hypothetical syntax
```

### 3.1 Opening And Closing

```text
fn fetch_user!(host: string, id: i32) -> string:
    c := connect!(host)
    c.send!(encode(id))
    c.read!()
```

`connect!` returns an owned connection. Nothing closes it explicitly:
when `fetch_user!` returns, `c` is dropped and its `drop` closes the
socket. The same happens on every other way out: `?`, `return`,
`break`, an early error, and cancellation at either bang call. A
forgotten close cannot happen.

Handing a connection off after checking it needs nothing special:

```text
fn adopt!(host: string, pool: ^mut Pool) -> void:   # hypothetical syntax
    c := connect!(host)
    check!(c)                                       # cancelled here: c is dropped and closed
    pool.give(c)                                    # moved into the pool: not dropped here
```

A value that was moved away is not dropped by its old owner. Where a
value may or may not have moved, the compiler tracks it with a drop
flag, as Rust does, so the destructor runs exactly once.

An owned value whose type has no `drop` is simply freed by the
collector later. A `let own buf: List[u8] = []` is unique and may be
mutated by its owner, and nothing runs when it goes away. A panic runs
no destructor, as it runs no `defer`: it poisons the instance, and the
host closes what the instance held.

Cleanup that is not tied to one value, such as restoring a setting or
writing a log line on exit, uses a guard. `on_exit` returns an owned
guard whose `drop` runs the closure, so it runs at the same points a
destructor does. `defer: body` stays as sugar for exactly that.

```text
fn with_verbose!(job: own fn!() -> void) -> void:  # hypothetical syntax
    set_verbose(true)
    defer: set_verbose(false)                      # == guard := on_exit(fn(): set_verbose(false))
    job()                                          # cancelled or failed: it still runs
```

A `drop` must not suspend and cannot fail. Cleanup that must, such as a
buffered writer that flushes over the network, is an explicit method
called before the drop:

```text
fn upload!(w: own Writer, bytes: List[u8]) -> Result[void, NetError]:   # hypothetical syntax
    w.write!(bytes)?
    w.finish!()       # flushes, may fail, consumes w
```

If `upload!` returns early or is cancelled, `w` is dropped, and its
`drop` closes it without flushing.

### 3.2 Lending And Giving Away

A log rotator borrows a file to measure it, then gives it away.

```text
fn line_count(f: ^File) -> usize: sys_lines(f.fd)       # hypothetical syntax
fn archive(f: own File) -> void:                        # hypothetical syntax
    sys_copy(f.fd, "archive/")

fn rotate(path: string) -> void:
    f := open(path)
    if line_count(f) > 1000:
        archive(f)
```

The call site writes no mark; the parameter's mode decides. `line_count`
borrows `f` for the call. If `archive` takes `f`, it closes when
`archive` returns; otherwise it closes at the end of `rotate`. Using
`f` after `archive(f)` is `use-after-move`.

Passing an owned value to a plain parameter is the other common
mistake:

```text
fn describe(f: File) -> string: "fd ${f.fd}"

fn show(path: string) -> void:
    f := open(path)
    println(describe(f))                  # error: own-to-plain
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

fn warm!(hosts: List[string]) -> void:
    let own pool = Pool { idle: [] }                  # hypothetical syntax
    for h in hosts:
        pool.give(connect!(h))
```

The pool owns its connections, and callers borrow them through `peek`.
`List[own Conn]` is the ordinary list with `T = own Conn`; `push` and
`pop` move through `^mut self`.

Dropping an owned value drops what it owns, fields in order. So when
`warm!` returns, `pool` is dropped, then its list, then each
connection, which closes. No shutdown method is needed. `clear`,
`truncate`, and an early `break` out of `for c in conns.into_iter()`
drop the elements they discard, so every list method keeps working.

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
    fn() -> void: archive(f)

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

**Dropping a closure drops what it owns.** `done` above holds an open
file. If it is never called, dropping `done` drops the file, which
closes. If it is called, the file moves into `archive`, and the closure
is gone.

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

**Closures and concurrency.** A cancelled child of `all!` or `race!`
drops what it owns, captured values included. A closure stored for
later, as in an `own List[own fn!() -> void]`, is dropped with the list
if it never runs. An `own fn` is never a provider: `$.with` borrows its
value.

A function type may name its parameters, as `fn(c: ^Conn) -> ^{c} Conn`.
A mark inside `Fn[...]` is `misplaced-lifetime`, with a fix-it to
`fn(...)`.

### 3.8 Generics

```text
fn same[T](v: T) -> T: v                             # T is never owned here

fn first[T](items: ^List[T]) -> Option[^T]:          # hypothetical syntax
    items.at(0)

fn take_first[T < own Any](xs: own List[T]) -> Option[own T]:   # hypothetical syntax
    xs.into_iter().next()                            # the rest of xs is dropped

fn bad[T](v: own T) -> void: pass                    # hypothetical syntax: error: own-on-type-parameter
```

A plain type parameter never takes an owned type, since its body may
copy a `T`. A generic that accepts owned values says so with a bound,
`T < own Any` or `T < own Trait`, as Swift's `~Copyable`; its body moves
`T` values and never copies them. `List` declares `[T < own Any]`, so
`List[own Conn]` works. A generic must not write `own T` itself.

This inverts Rust's default on purpose. In Rust an unbounded `T` is
move-only, so `fn dup<T>(v: T) -> (T, T) { (v, v) }` is an error unless
`T: Copy`. In hd a plain value already aliases freely, and every
generic body written today uses that. So plain `[T]` keeps it, as
Rust's `T: Copy` world does, and `[T < own Any]` is hd's spelling of
Rust's unbounded `<T>`: owned values allowed, the body move-only.

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
    c.send!([1])

fn later(c: own Conn) -> void:                       # hypothetical syntax
    pending := handle(c)                             # an owned cold frame holding c
    pending.cancel()                                 # dropping the frame drops c, which closes
```

Children of `all!` and `race!` may borrow the parent's places, since
they end inside the call. As with Rust's `thread::scope`, two children
cannot both borrow one place when either borrow is mutable.

A plain call of a bang function builds a cold frame. A frame that holds
an owned argument is itself an owned value: discarded before its first
poll, it is dropped, and so is the argument. A cold frame is bounded by
every borrow it captures, as `^{keep, scratch} mut Suspend[^{keep} Conn]`.

**Children that return owned values.** If `all!` gets a connection back
from one child and a sibling then fails, the finished result sits in a
buffer the combinator owns. Failing, it drops the buffer, and the
connection closes. So children may return owned values freely:

```text
fn both!(a: string, b: string) -> (own Conn, own Conn):   # hypothetical syntax
    all!(connect(a), connect(b))           # one fails: the other's connection is dropped
```

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
top-level binding is an error, since other functions can reach it
without a path. Code that needs a global mutable queue uses plain
values, with garbage-collected semantics.

A top-level binding may hold an owned value. A later top-level statement
or REPL input may move it, checked in statement order; a function never
may, since it could run before or after. Otherwise it is dropped when
the instance ends.

```text
c := connect!("db")              # a REPL input: an owned top-level binding
c.send!([1])                     # a later input: a static borrow
drop(c)                          # a later input moves it: the socket closes
c.send!([1])                     # error: use-after-move
```

### 3.12 The Serialization Boundary

An owned position, a type with an `own` part, and a type that carries a
lifetime are not boundary-safe. So an owned handle cannot be a remote
argument, a remote result, or a sealed value. A portable closure
captures no borrow and no owned value.

```text
@remote
fn store(c: own Conn) -> void: pass                  # hypothetical syntax: error: not boundary-safe
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
10. A type may implement the std trait `DropOwn`, whose
    `drop(^mut self)` runs when an owned value of the type is dropped.
    Such a type is held only as `own T` or `^T`; a plain position of it
    is `drop-type-plain`.
11. A holder that receives an owned value at construction is owned; a
    plain holder receiving one is `own-upgrade`.

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
7. A field read through a borrow is a borrow with the same lifetime; a
   primitive is copied out. An element of a borrowed collection is a
   plain value when its type is plain, and a borrow when it is owned.
   Moving through a borrow is an error.
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

### 4.5 Drop

1. An owned binding that still holds its value when its scope ends is
   dropped there. The exits are the normal end, `return`, `break`,
   `continue`, `?`, and cancellation at a bang call.
2. Assigning to an owned place that holds a value drops the old value
   first.
3. Dropping a value runs its type's `drop`, if any, then drops its owned
   fields in declaration order, and an owned list's elements in order.
   Bindings in one scope drop in reverse order of declaration.
4. A moved-out value is not dropped by its old owner. Where a value is
   moved on some paths only, a drop flag decides at run time, so `drop`
   runs exactly once.
5. `drop` takes `^mut self`, must not suspend, and cannot be called
   directly; std's `drop(x)` moves `x` in and ends it.
6. A panic runs no `drop`. It poisons the instance, and the host releases
   what the instance held.
7. A plain value never runs `drop`, even when its type implements it.
8. An owned temporary that is not bound is dropped at the end of its
   statement.

### 4.6 Generics, `dyn`, Closures, And Frames

1. A plain type parameter `[T]` takes only plain types, and its body may
   copy `T` values.
2. `[T < own Any]` or `[T < own Trait]` takes owned and plain types. Its
   body moves, borrows, or drops `T` values and never copies them.
3. A call infers `T = own U` when the argument is owned and the
   parameter's bound allows it. `^T` with `T = own U` is `^U`.
4. A generic must not write `own T` itself.
5. A std container declares `[T < own Any]`. Its methods that copy an
   element live in an `impl` for plain `T` only.
6. `own U` converts to `own dyn K`. A value holding a borrow converts
   only to `^{a} dyn K`, and a downcast needs `static`.
7. A closure that captures an owned value moves it in. A closure that
   captures an owned value or a `^mut` borrow is `own fn(...)`, and a
   call consumes it. Dropping it uncalled drops its captures.
8. A closure that captures a shared borrow is bounded by it and may be
   called any number of times while it lives.
9. A capture of a plain binding stays shared storage, unchecked, as
   plain `T` is.
10. A cold frame is bounded by every borrow it captures. A frame that
    holds an owned argument is owned, and dropping it drops the argument.
11. `$.with(K = p)` borrows `p` for its block, and `$.use(K)` returns a
    borrow bounded by `$K`.
12. A top-level binding may be borrowed as `static`, never as `^mut`.
    Moving an owned one is `move-out-of-top-level`.
13. Owned positions and types that carry a lifetime are not
    boundary-safe.

## 5. What Changes From NonEscapable

The old design made NonEscapable a category of types and relied on
`defer` for cleanup. Here `own` and `^` are modes at positions, an
unmarked type is plain rather than global, lifetimes are Rust's, and an
owned value's destructor does the cleanup. A use after close is a
compile error, a forgotten close cannot happen, and `^mut` is
exclusive.

| Use case | Old spelling | New spelling | Works? |
| --- | --- | --- | --- |
| A view over a handle | `data Excerpt: conn: ^Conn` with `impl NonEscapable` | `data Excerpt[^a]: conn: ^{a} Conn` | yes |
| A producer of a fresh handle | `fn accept!(^self) -> Conn` | `fn accept!(^self) -> own Conn` | yes; it closes when dropped |
| A pool whose handles callers use | `conns: ^mut List[^Conn]`, `adopt(c: ^{self} Conn)` | `conns: own List[own Conn]`, `adopt(c: own Conn)` | yes, differently: the pool owns, callers borrow |
| Concurrency | `fn echo!(c: ^Conn)` | the same | yes; a dropped frame or buffer drops what it owns |
| Providers | `trait Pool < NonEscapable`, `-> ^Conn $ Pool` | `trait Pool`, `-> ^{$Pool} Conn $ Pool` | yes |
| Callbacks | `List[^fn!() -> bool]` | `List[^{attempt} fn!() -> bool]` | yes; a global registry uses plain closures |
| An error carrying a handle | `enum LinkError: Broken(conn: ^Conn)` | `enum LinkError[^c]: Broken(conn: ^{c} Conn)` | yes |
| The REPL | `c := connect!("db")` | the same | yes; a later input may `drop(c)` |
| Generic code | `fn look_at[T](x: ^T)` | the same, or `[T < own Any]` to take owned values | yes |
| Locality of plain values | `fn fill(buf: ^mut List[u8])` | the same | yes, and the borrow freezes the variable |

Over these cases, the annotation count is the same, 34 marks before and
after: `own` on a producer's result costs one, and the pool's `defer`
line goes away.

The old design promised no false alarms. This one inherits Rust's and
adds a few of its own. Each rejects code the old design accepted:

- **Get or insert** (NLL problem case #3). A lookup's borrow stays live
  on the path that inserts. Rust rejects it too; the workaround checks
  `contains` first, then looks up once.
- **A method over one field.** A `^mut self` call while a borrow of a
  different field lives, as in Rust.
- **Mutating a plain local** while a `^` view of it lives, by the
  owner's decision to check every `^`.
- **Keeping a provider** from `$.use(K)` in module state.

## 6. Still To Think Through

Every open point of the earlier drafts is decided. The owner asked for
more thought on generics and ownership before the design settles:

1. **Iteration.** How `for x in xs` reads a list of owned values: by
   borrow, by consuming the list, or both, and which form is the default.
2. **Traits over owned values.** How `Eq`, `Hash`, `Ord`, and `Display`
   take their receiver (`^self`), and how a `Map` keyed by owned values
   looks keys up by borrow.
3. **Copying an owned value.** Whether a `Clone`-style trait gives
   `own T` from `^T`, and whether derivation can provide it.
4. **Drop and generics.** How a generic body's drop of a `T` finds the
   instantiation's `drop`, through monomorphization or `dyn`, and the
   cost of drop flags in generic code.
5. **Higher-order functions.** The shapes of `map`, `filter`, and
   `fold` over owned elements, and how an `own fn` callback composes
   with them.
6. **Migration.** The order of std's `^self` marks and its
   `[T < own Any]` declarations, the lint that proposes them, and the
   spec chapters each decision touches.

## Appendix: Limits And Holes

No known hole remains: B7 and B8 are closed by the owner's rules on
owned holders and on `T < own Any`. A plain alias or a plain closure
capture that changes a borrowed variable's object is not a hole: plain
values keep garbage-collected semantics by design
([3.6](#36-borrowing-plain-values)).

Limits, where the draft rejects a reasonable program:

- A global mutable queue of owned values is not possible; it uses plain values ([3.11](#311-top-level-bindings-and-the-repl)).
- A `drop` cannot suspend or fail, so a flushing close is an explicit call ([3.1](#31-opening-and-closing)).
- A struct cannot borrow its own field ([3.4](#34-a-struct-that-holds-a-borrow)).
- A cleared list keeps its old elements' lifetime, as in Rust ([4.4](#44-lifetimes-and-elision)).
- A local `^dyn` value cannot be downcast ([3.8](#38-generics)).
- A borrowed value reaches a std method only once it takes `^self` ([3.6](#36-borrowing-plain-values)).
- Rust's known false alarms, accepted as limits, and the draft's own ([5](#5-what-changes-from-nonescapable)).

Ways a handle escapes its destructor, all by its author's choice ([2](#2-the-model-on-one-page)):

- A producer that builds a handle type outside `DropOwn`, or a handle type that forgets to implement it, closes nothing.
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

Every `text` block, 26 in all, was parsed on 2026-10-16 with
`scripts/hd-conformance.sh parse`. Each was parsed as written and with
the marks erased (`own`, `^`, `^{...}`, lifetime brackets). Parsing
checks syntax only. Nothing was type-checked, and the blocks call
functions this study does not define.

| Result | Blocks |
| --- | --- |
| Parses as written | 4, among them section 1 and the REPL inputs in 3.11 |
| Fails only at the new marks; parses with the marks erased | the other 22 |
