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
- [6. Still To Decide](#6-still-to-decide)
- [Appendix: Limits And Holes](#appendix-limits-and-holes)

## Decisions So Far

The owner chose Rust's model: ownership, moves, borrows with lifetimes,
including borrows inside types, and destructors. An owned value runs
its type's `drop` when its owner's scope ends without moving it; the
compiler knows that point exactly. The trait is `DropOwn`, since only
owned values run it, and only an `OwnOnly` type may implement it. `defer` stays as sugar for a std guard whose `drop` runs a
closure. NonEscapable is dropped.

Plain `T` and `mut T` keep today's garbage-collected object semantics,
untouched. `own T` and `^T` are new semantics that code opts into, and
the two worlds never mix: an `own T` where a plain `T` is expected is a
type error.

`own` is written at the usage site, on the type at each position, as
`mut` is. `own T` is the only path to its value, and it grants
mutation; there is no `own mut T`. A type whose values are always owned
implements the std marker trait `OwnOnly`, as
`impl OwnOnly for File`: every use is then `own File` or `^File`,
written out at each use. A borrow is written `^T` or `^mut T`, and a
lifetime is named by a variable, as `^{x}`. The checker checks
every `^` of a local, whatever its type, and a global may be borrowed
as if it were owned.

An owned value is unique, not linear: nothing forces code to consume
it, and dropping it runs its destructor. No type is marked as a
resource, and there is no `close` protocol. A fresh value may start
owned, as `let own user = User {...}`, if its plain fields are fresh
too; borrowed fields and immutable values such as numbers and strings
are exempt. Only an `OwnOnly` type may have a field written `own T`
or `^mut T`, so no plain object can hand out an untracked read of an
owned value. A generic data type keeps a plain header, `data Box[T]`,
and derives ownership from its structure: with
`impl OwnOnly for Box[T] by Structure`, an instance is `OwnOnly`
exactly when some field's type is, so `Box[File]` is owned while
`Box[i32]` stays plain. std's containers do the same. A generic
function that accepts owned values says `T < own Any`; a plain type
parameter never takes one.

A borrow reaches only `^` positions, so std's read-only methods take
`^self`. A composite part read through a borrow is a borrow; the plain
contents of a borrowed collection are plain values. A value
whose type has a destructor keeps the borrows it holds live until its
drop. Providers are plain values only; an owned or borrowed service is
passed as a parameter.

A later top-level statement or REPL input may move a top-level owned
value, unless some function borrows it, which makes it immovable. A
`^mut` of a top-level binding is not allowed; code that needs a
global mutable queue uses plain values. A cleanup that must suspend or
can fail is an explicit method called before the drop. A move has no
mark at the call site. A guarded `match` arm takes ownership only after
its guard succeeds. There is no general `replace`; an owner that must be
swapped out lives in an `Option[own T]` and leaves through `take()`. Rust's known false alarms are accepted as
limits.

A closure that captures a value exclusively, an owned value or a `^mut`
borrow, can be called once. A job that must run repeatedly with
exclusive access is named data with `^mut self` methods.

A handle type implements `OwnOnly`, so no producer can hand out a
plain one. Earlier decisions hold: anything local shows in source, a store writes its destination on
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

impl OwnOnly for File                                        # hypothetical syntax
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

**How a handle stays owned.** It implements `OwnOnly`, so every use
of it is `own File` or `^File`, and a bare `File` is `own-type-plain`,
with a fix-it to either. No producer can hand out a plain handle, and
only an `OwnOnly` type may implement `DropOwn`.

Rust, Swift (`~Copyable`), and Mojo put copyability on the type and the
passing convention on the parameter; none of them is garbage-collected.
hd puts both on the position, as `mut`, and marks a type that is always
owned with the `OwnOnly` trait, as Rust and Swift mark copyability with
`Copy` and `Copyable`. The price is the uniqueness rule for fresh
values, plus the rule that only `T < own Any` takes an owned type.

The closest precedent is OxCaml, Jane Street's OCaml, which keeps the
garbage collector and adds modes written on values, not types: `local`
(the value does not escape its region, so it lives on the stack),
`unique` (one reference, so a destructive operation such as an explicit
`free` is safe), and `once` (a closure called at most once). Ordinary
values keep the default modes and the heap, as hd's plain `T` does, and
some types cross modes, as hd exempts primitives. OxCaml has no
destructors: its cleanup is an explicit consuming call. hd's `DropOwn`
goes further, because an owned value has one owner and no borrow
outlives it, so its drop point is known.

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

A guard's `drop` runs with no requirement row, so a `defer` body may
call only code that needs none. Code that needs a provider fetches it
first and calls methods on the captured value:

```text
fn work() -> void $ Audit:
    a := $.use(Audit)
    defer: a.note()                                # fine: needs no row
    defer: record()                                # error: a defer body cannot need $ Audit
```

A `defer` body that names an owned value captures it, and the guard
moves it in, so a later use of the value is `use-after-move`; the error
names the `defer` that moved it. To keep using the value, the body
captures a borrow instead:

```text
fn copy_log(path: string) -> void:
    f := open(path)
    let view: ^File = f                            # hypothetical syntax
    defer: report_size(view)                       # captures the borrow, not the file
    append(f, "done")                              # fine: f is still owned here
```

The guard is declared after `f`, so it drops first, while `f` is still
open.

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
without `^` carries no lifetime of its own, though its type may carry
lifetime arguments, as `Parser[^]` does in 3.4. A store into another
parameter writes
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

A data type never owns a handle; it may borrow one. A job that writes
to a file while it reads a borrowed document borrows both, and the
function that opened the file owns it:

```text
data Export[^doc, ^out]:                              # hypothetical syntax
    out: ^{out} mut File                              # hypothetical syntax
    source: ^{doc} Document                           # hypothetical syntax

impl[^doc, ^out] OwnOnly for Export[^{doc}, ^{out}]   # hypothetical syntax: holds a ^mut

fn export(d: ^Document, path: string) -> void:       # hypothetical syntax
    f := open(path)                                   # the owner: closes at the end
    let own job = Export { out: f, source: d }        # holds a ^mut: held own
    run(job)
```

The freshness rule for owned values covers plain fields only; a
borrowed field already states its lifetime and how it may be used.

### 3.5 A Pool That Owns Connections

A pool owns its connections, so it implements `OwnOnly`, the only
kind of type that may have an `own` field:

```text
data Pool:
    idle: own List[own Conn]                          # hypothetical syntax

impl OwnOnly for Pool                             # hypothetical syntax

impl Pool:
    fn give(^mut self, c: own Conn) -> void:          # hypothetical syntax
        self.idle.push(c)
    fn peek(^self) -> Option[^Conn]:                  # hypothetical syntax
        self.idle.at(0)

fn warm!(hosts: List[string]) -> void:
    let own pool = Pool { idle: [] }                  # hypothetical syntax
    for h in hosts:
        pool.give(connect!(h))
    serve!(pool)                                      # lends the pool
```

`List[own Conn]` is the ordinary list with `T = own Conn`; `push` and
`pop` move through `^mut self`. A plain `data` type cannot declare the
list as an `own` field, because a plain object could then hand out
reads of the connections that no check follows. Since `Pool` is
`OwnOnly`, every use writes `own Pool` or `^Pool`. Leaving out the
impl is `own-field-plain-type`, with a fix-it that adds it.

When `warm!` returns, `pool` is dropped, then its list, then each
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
with `first` first fixes it.

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
data Flusher[^f]:                                # hypothetical syntax
    out: ^{f} mut File                               # hypothetical syntax
    written: usize

impl[^f] OwnOnly for Flusher[^{f}]           # hypothetical syntax

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

fn take_first[T < own Any](xs: own List[T]) -> Option[T]:       # hypothetical syntax
    xs.into_iter().next()                            # the rest of xs is dropped

fn bad[T](v: own T) -> void: pass                    # hypothetical syntax: error: own-on-type-parameter
```

A plain type parameter never takes an owned type, since its body may
copy a `T`. A generic that accepts owned values says so with a bound,
`T < own Any` or `T < own Trait`, as Swift's `~Copyable`; its body moves
`T` values and never copies them. A data type such as `List` keeps a
plain header and derives ownership from its structure,
`impl OwnOnly for List[T] by Structure`, so `List[own Conn]` works. A generic must not write `own T` itself.

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

Providers stay plain, exactly as today. `$.with(K = p)` takes a plain
value, and `$.use(K)` returns it; sibling tasks share it with
garbage-collected semantics, as the spec's mutable providers do now. An
owned or borrowed value is never a provider:

```text
fn emit!(sink: ^mut dyn Sink) -> void:               # hypothetical syntax
    sink.write!()

fn twice!(sink: ^mut dyn Sink) -> void:              # hypothetical syntax
    emit!(sink)                                      # an exclusive service is a parameter
    emit!(sink)

fn wrong!(sink: ^mut dyn Sink) -> void:              # hypothetical syntax
    $.with(Sink=sink):                               # error: provider-not-plain
        pass
```

So a service that must be used exclusively, or that owns handles, is a
parameter, and its borrows follow the ordinary rules: two children
cannot hold one `^mut` at once.

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

A function that borrows a top-level owner can return the borrow as
`^{static}`, and a caller may hold it past any later statement. So a
top-level owned binding that any function borrows never moves; it
drops when the instance ends:

```text
GLOBAL := open("global")

fn current() -> ^{static} File: GLOBAL               # hypothetical syntax

drop(GLOBAL)                     # error: GLOBAL is borrowed by current(); it cannot move
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
10. A type implements the std marker trait `OwnOnly` to be always
    owned. The impl is written in the type's own module. Every use of a
    `OwnOnly` type is `own T` or `^T`; a plain position of it is
    `own-type-plain`.
11. Only an `OwnOnly` type may have a field written `own T` or
    `^mut T`; otherwise it is `own-field-plain-type`.
12. A generic data type keeps a plain header, `data Box[T]`. Its argument
    may be owned only if the type derives ownership from its structure,
    `impl OwnOnly for Box[T] by Structure` (or `@derive(OwnOnly)`); an
    instance is then `OwnOnly` exactly when some field's type is, after
    substitution. A hand-written, unconditional `impl OwnOnly for Box[T]`
    also admits owned arguments, since it makes every instance owned. An
    `own U` argument counts as `OwnOnly`. Otherwise `Box[own Conn]` is
    `own-in-plain-type`.
13. `OwnOnly` by `Structure` is a compiler-known rule, not a std
    template: `OwnOnly` has no members to generate. A type owned when
    either of two arguments is, as `Outcome[A, B]`, needs only the one
    derivation. The rule reads each field's type after substitution: an
    `own` part or an `OwnOnly` type makes the instance `OwnOnly`; a
    shared borrow `^U` does not, whatever `U` is; a `^mut` field needs a
    hand-written `OwnOnly` impl, by rule 11; an unused type argument
    counts for nothing. A recursive type is `OwnOnly` only if a
    non-recursive path reaches such a field (the least fixed point).
14. `DropOwn` extends `OwnOnly`, so only an `OwnOnly` type has a
    destructor. Its `drop(^mut self)` runs when an owned value of the
    type is dropped.

### 4.2 Moves

1. Using an owned place by value moves it, and a later use is
   `use-after-move`. Assigning it again initializes it.
2. Assigning a place that still holds an owned value drops the old
   value first (4.5).
3. A move takes a whole owned binding, or an element out of an owned
   container through its methods (`pop`, `take`, `into_iter`). There is
   no partial move: a field of an owned generic instance comes out only
   by taking the whole value apart.
4. A pattern that binds every field of an owned value consumes it. A
   private field moves out only in its type's module. A type that
   implements `DropOwn` cannot be taken apart this way, since its
   `drop` must see a whole value.
5. In a `match` arm with a guard, the names the pattern binds are shared
   borrows inside the guard; the arm takes ownership only when the guard
   succeeds and its body runs, as Rust has done since 1.39. A false
   guard leaves the subject whole for the next arm. Moving a binding
   inside a guard is `move-in-guard`.
6. No general `replace` exists for an owned place behind a `^mut`. A
   replaceable owner is an `Option[own T]` field, swapped with the
   built-in `take()`; `pop`, `take`, and `into_iter` are the only ways to
   remove an owner from a container. After `take()` the field holds
   `None` until something is put back, and a cancelled bang call between
   the two leaves it so. Code that must keep the field full prepares the
   replacement before taking the old value, or handles `None` in later
   methods and in `drop`.

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
8. A `^mut` borrow of a receiver has two phases, as Rust's two-phase
   borrows do. While the arguments run, the receiver's place is
   reserved: it may be read, but mutating or moving it, or taking a
   `^mut` of anything that contains it, is `borrow-conflict`. Then the
   exclusive borrow activates. So `xs.push(xs.len())` is valid, and
   `holder.file.inspect_after(reset(holder))` is rejected.
9. Rules 3 and 4 apply to every `^` of a local, owned or plain, and
   cover the borrowed variable only. A plain alias or a plain capture of
   the same object keeps garbage-collected semantics.
10. A borrow never reaches a top-level binding, a plain position, or a
    result that does not name it. That is `borrow-escapes`.
11. A value whose type has a destructor uses every borrow it holds when
    it is dropped, so each such borrow lives until that drop. Ending the
    borrowed place earlier is `borrow-conflict`, as Rust's drop check
    makes it.

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
6. Top-level owned bindings drop at the end of the instance in reverse
   dependency order: the initialization-order analysis also follows the
   functions each binding's `drop` can reach, so a global that another
   global's `drop` uses is dropped after it. A cycle, where two globals'
   drops each reach the other, is `global-drop-cycle`.
7. A panic runs no `drop`. It poisons the instance, and the host releases
   what the instance held.
8. A type that implements `DropOwn` never appears in a plain position
   (4.1), so every value of it is dropped exactly once.
9. An owned temporary that is not bound is dropped at the end of its
   statement.
10. `defer: body` means `guard := on_exit(fn(): body)`. Its body runs
    with no requirement row; a call that needs one is
    `defer-needs-row`.

### 4.6 Generics, `dyn`, Closures, And Frames

1. A plain type parameter `[T]` takes only plain types, and its body may
   copy `T` values.
2. `[T < own Any]` or `[T < own Trait]` takes owned and plain types. Its
   body moves, borrows, or drops `T` values and never copies them. The
   body is checked once, where it is written, for every mode the bound
   allows: a value is move-only there when its type can be owned under
   some allowed `T`, as `T`, `Boxed[T]` or `List[T]` can. A shared
   borrow `^T` and a plain function type `fn() -> T` never own a `T`,
   so they stay copyable. So `fn dup[T < own Any](x: Boxed[T])`
   returning `(x, x)` is an error at the definition, never at a caller,
   while `(r, r)` with `r: ^T` is fine.
3. A call infers `T = own U` when the argument is owned and the
   parameter's bound allows it. `^T` with `T = own U` is `^U`.
4. In generic code, a position's mode is decided after substitution: a
   type written with `T`, as `Option[T]` or a field `List[T]`, is owned
   wherever its instance is `OwnOnly`, and plain otherwise. Concrete
   code writes `own` explicitly.
5. A generic must not write `own T` itself.
6. A std container keeps a plain header and derives `OwnOnly` by
   `Structure` (4.1), so `List[own Conn]` is `OwnOnly`. Its methods that work on
   owned elements live in an `impl[T < own Any]`; those that copy an
   element live in an `impl` for plain `T` only.
7. `own U` converts to `own dyn K`. A value holding a borrow converts
   only to `^{a} dyn K`, and a downcast needs `static`.
8. A closure that captures an owned value moves it in. A closure that
   captures an owned value or a `^mut` borrow is `own fn(...)`, and a
   call consumes it. Dropping it uncalled drops its captures.
9. A closure that captures a shared borrow is bounded by it and may be
   called any number of times while it lives.
10. A capture of a plain binding stays shared storage, unchecked, as
    plain `T` is.
11. A cold frame is bounded by every borrow it captures. A frame that
    holds an owned argument is owned, and dropping it drops the argument.
12. A provider is a plain value. `$.with(K = p)` with an owned or
    borrowed `p` is `provider-not-plain`.
13. A top-level binding may be borrowed as `static`, never as `^mut`.
    A later top-level statement may move an owned one, checked in
    statement order; a function body never may
    (`move-out-of-top-level`). A top-level owned binding that any
    function borrows never moves.
14. Owned positions and types that carry a lifetime are not
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
| A pool whose handles callers use | `conns: ^mut List[^Conn]`, `adopt(c: ^{self} Conn)` | an owned `List[own Conn]` local, lent as `^mut` | yes, differently: a function owns the pool, callers borrow |
| Concurrency | `fn echo!(c: ^Conn)` | the same | yes; a dropped frame or buffer drops what it owns |
| Providers | `trait Pool < NonEscapable`, `-> ^Conn $ Pool` | a plain provider; owned handles are passed as parameters | yes, differently |
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

## 6. Still To Decide

**Decided by the owner, 2026-10-17:**
- 6.1 Iteration: `for x in xs` lends an owned list (each `x` is a `^T`); `xs.into_iter()` is the one consuming loop. `Iterator` gains a lifetime parameter, as Rust's `slice::Iter<'a, T>`.
- 6.2 Maps: keys stay plain; only values may be owned.
- 6.3 Copying: a std `Clone` trait now (not only per-type methods): `clone(^self) -> own Self`, derivable. In general, cloning a value whose type is annotated `own` deep-copies everything, plain fields included, so the copy shares nothing with the original and is fresh (owner: "if the type is annotated as `own`, deep clone everything and keep even plain field cloned").
- 6.4 Drop in generic code: how compiled code finds `drop` is a compiler choice outside the spec; types without a destructor keep sharing one body, each type with a destructor gets its own instance and drop flags.
- 6.5 Higher-order functions: owned lists reach `map`, `filter` and `fold` through `iter()` and `into_iter()`, plus `retain`; no owned twins on `List`.
- 6.6 Migration: bottom up: language, traits, containers, handles with streaming, then the rest of std by lint.
- Freshness on stores (prototype question 7, review O10): the freshness rule also applies to a later store into a plain field of an owned value, so `own` stays the only path to the whole value.
- Lifetime names (owner, 2026-10-17): type-parameter paths are removed (`x.T`, `s.A` and `s::A` all expose a declaration's generic names). A path lifetime now names a FIELD path for partial capture: `fn name_of(u: ^User) -> ^{u.name} string` borrows only `u.name`, so the caller keeps the other fields usable while the result lives. Every field a path lifetime names must be `pub` (owner: "partial capturing only allow public visibility"), so a signature never exposes a private field. A function declares lifetime parameters, last in its generic list, to name inner lifetimes no variable can denote: `fn first[^c](x: ^List[^{c} Conn]) -> ^{c} Conn`; `fn add[^c](dst: ^mut List[^{c} Conn], conn: ^{c} Conn)`. Variables still name outer borrows, with elision unchanged. A declared lifetime parameter may appear inside a parameter's type argument; a bare nested `^` is anonymous. Where the text below still shows a path lifetime, this note overrides it.

The option discussions below are kept as the record of why.

The owner asked for an options study of six more points before a
prototype. Each item below shows the problem, the options, what Rust,
Swift, Mojo and OxCaml do, and the cost, then gives one recommendation
and one question. Every rule above stays as decided. Where a decided rule already answers part of an
item, the item says so and asks only about the rest.

### 6.1 Iteration Over Owned Lists

A pool pings every connection, and a shutdown hands each one off:

```text
fn ping_all!(pool: ^Pool) -> void:                   # hypothetical syntax
    for c in pool.idle:
        c.send!([1])

fn shutdown(conns: own List[own Conn]) -> void:      # hypothetical syntax
    for c in conns.into_iter():
        retire(c)
```

The decided rules settle the first loop. `for` calls `iter()`, a std
read-only method, so `iter` takes `^self` and lends the list without a
mark. A borrowed list yields its owned elements as borrows, so `c` is a
`^Conn`. Two points stay open: whether a loop over an owned list should
consume it instead, and what `into_iter()` returns.

**Option A: lend by default, consume through `into_iter()`.** Both loops
mean what they say. `into_iter(own self)` returns std's owned cursor
`IntoIter[T]`, which `for` accepts beside `Iterator[T]`. A `break` drops
the cursor and the elements not yet visited, as 3.5 already states.

**Option B: consume by default.** `for c in conns` moves an owned list,
as Rust's `for x in v` does; lending writes `conns.iter()`. hd has no
borrow expression, so the loop picks `into_iter` because the iterable is
an owned place: a new rule, unlike every plain loop.

**Option C: lend only.** There is no consuming loop. A drain matches
`conns.pop()` inside `while true` and breaks at `.None`.

**Elsewhere.** Rust's `Vec` implements `IntoIterator` by value, which
consumes, and for `&Vec` and `&mut Vec`, which yield references
([Vec](https://doc.rust-lang.org/std/vec/struct.Vec.html),
[loop expressions](https://doc.rust-lang.org/reference/expressions/loop-expr.html)).
Mojo's collection iterators return references, and `for ref` makes them
mutable ([control flow](https://docs.modular.com/mojo/manual/control-flow/)).
Swift's `for in` needs `Sequence`, which needs copyable elements;
borrowing iteration is future work
([SE-0437](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0437-noncopyable-stdlib-primitives.md)).
OxCaml's `borrow_` lends a unique value as aliased and local for one
region ([borrowing](https://oxcaml.org/documentation/uniqueness/borrow/)).

**Cost.** The lending cursor's closure captures `^self`, so `Iterator`
gains a lifetime parameter under every option, as Rust's
`slice::Iter<'a, T>` has. A adds one std type and one case to what `for`
accepts. B adds a loop rule. C adds nothing, but drains from the back.

A user's named producer with a `^mut self` step, the review's U9, is
driven by `while` under every option, not by `for`.

**Recommendation: A.** It is what the decided `^self` rule and 3.5
already imply, and the consuming loop shows `into_iter()` at the site.

**Question.** Does `for x in xs` lend an owned list, with
`xs.into_iter()` as the one consuming loop?

### 6.2 Traits Over Owned Values

```text
fn same_peer(a: ^Conn, b: ^Conn) -> bool:            # hypothetical syntax
    a == b

fn route(table: ^Map[string, own Conn], host: string) -> Option[^Conn]:   # hypothetical syntax
    table.get(host)
```

The receivers are already answered. `Eq`, `Hash`, `Ord`, `Display` and
`Debug` are std read-only methods, so the decided rule gives them
`^self`. The second operand follows for the same reason, as
`fn eq(^self, other: ^Self) -> bool`, so `a == b` lends both sides. What
stays open is whether a `Map` key may be owned.

**Option A: keys stay plain; values may be owned.** `route` above works:
a connection table keyed by host is the common case. An owned key fails
the plain bound `K < Eq & Hash`, as an owned type fails any plain type
parameter.

**Option B: owned keys, looked up by a borrow.** `get(^self, key: ^K)`
lends the probe key. Inserting moves a key in, and dropping the map
drops its keys.

**Option C: lookup by a borrowed form.** As in Rust, a
`Map[own Token, V]` is probed by a type the key can lend, through a new
std trait.

**Elsewhere.** Rust's `PartialEq::eq`, `Hash::hash` and `Display::fmt`
all take `&self`, and `HashMap::get` takes any `&Q` where `K: Borrow<Q>`
([PartialEq](https://doc.rust-lang.org/std/cmp/trait.PartialEq.html),
[HashMap](https://doc.rust-lang.org/std/collections/struct.HashMap.html)).
Swift's
[SE-0499](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0499-support-non-copyable-simple-protocols.md)
lets `Equatable`, `Comparable`, `Hashable` and `CustomStringConvertible`
take noncopyable types, with `borrowing` operands. Mojo's default
argument convention is an immutable reference, so plain parameters
already borrow ([ownership](https://docs.modular.com/mojo/manual/values/ownership/)).
In OxCaml, a borrowed value is aliased and local, so it reaches any
function that takes a local argument
([borrowing](https://oxcaml.org/documentation/uniqueness/borrow/)).

**Cost.** The answered part adds no rule. It rewrites the derive
templates and about 250 hand-written impls in std: 109 `Debug`, 59
`Eq`, 45 `Display`, 17 `Hash` and 22 orderings. A adds nothing more. B moves
`Map` into an `[K < own Any]` impl. C adds a trait and a generic lookup.

**Recommendation: A.** No program in std or the six reviews keys a map
by an owned value. B can follow later without breaking A.

**Question.** Do `Map` keys stay plain, with only values allowed to be
owned?

### 6.3 Copying An Owned Value

A request draft owns its body buffer, and a retry needs a second copy:

```text
data Draft:
    url: string
    body: own Buffer                                 # hypothetical syntax

impl OwnOnly for Draft                               # hypothetical syntax

impl Draft:
    fn duplicate(^self) -> own Draft:                # hypothetical syntax
        Draft { url: self.url, body: self.body.duplicate() }

fn send_twice!(d: own Draft) -> void:               # hypothetical syntax
    post!(d.duplicate())
    post!(d)
```

**Option A: no trait.** A type that can be copied writes an ordinary
method returning `own Self`, as above. An owned value may already come
from an `own` result, so nothing is added. Generic code under
`T < own Any` cannot copy a `T`, which is what the bound says.

**Option B: a std `Clone` trait.** `fn clone(^self) -> own Self`, with a
derivation that clones each field. Two costs follow. `own` on a
primitive is `own-on-value`, so the trait cannot cover `i32` or
`string`. A fresh owned value needs fresh plain composite fields, so a
derived clone must deep-copy plain lists and maps too.

**Option C: `Clone` for every type.** As Rust's, plain values included.
It adds a deep copy beside aliasing to all of std.

**Elsewhere.** Rust's `Clone::clone` takes `&self` and returns `Self`,
and `derive` implements it when every field is `Clone`
([Clone](https://doc.rust-lang.org/std/clone/trait.Clone.html)). Mojo's
`Copyable` provides `copy()`, and its manual prefers that explicit call
to implicit copies ([life](https://docs.modular.com/mojo/manual/lifecycle/life/)).
Swift's explicit `copy x`
([SE-0377](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0377-parameter-ownership-modifiers.md))
works on copyable values; a noncopyable type is never copied
([SE-0390](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0390-noncopyable-structs-and-enums.md)).
In OxCaml an aliased value never becomes unique again
([uniqueness](https://oxcaml.org/documentation/uniqueness/intro/)).

**Cost.** A: no rule and no std change. B: one trait, one derivation,
and deep copies of plain composites. C: B plus a copy on every std type.

**Recommendation: A.** Handles, the main `OwnOnly` types, cannot be
duplicated anyway, and plain values already copy by aliasing. A trait
can be added later without breaking A.

**Question.** Is copying an owned value an ordinary method per type,
with no std `Clone` trait for now?

### 6.4 Drop In Generic Code

```text
fn pick[T < own Any](a: T, b: T, first: bool) -> T:   # hypothetical syntax
    if first: a else: b                                # the other one is dropped here
```

The meaning is decided. The value not returned is dropped at the end of
`pick`, exactly once. A drop flag decides where the compiler cannot tell
statically, as in Rust. What stays open is how the compiled body
finds `T`'s `drop`, which is not observable, so it is a compiler choice.

hd's compiler monomorphizes but folds instances by representation
([hd_mono](../../crates/hd_mono/DESIGN.md)). So `List[Point].push` and
`List[User].push` share one body, which cannot know which `drop` to run.

**Option A: drop-aware folding.** Types without a destructor keep
folding. Each type with a `drop` gets its own instance of a body that
drops a `T`, which calls `drop` directly. Only those instances carry
drop flags.

**Option B: drop through a table.** Every instance folds, and each
owned type supplies its `drop` through a table, as a `dyn` value's
vtable does. Each drop is an indirect call, and every folded body keeps
its flags.

**Elsewhere.** Rust monomorphizes generic code
([generics](https://doc.rust-lang.org/book/ch10-01-syntax.html)), and a
trait object runs the destructor of its underlying type
([destructors](https://doc.rust-lang.org/reference/destructors.html)).
Its drop flags live on the stack and are checked at run time only where
static analysis cannot decide
([drop flags](https://doc.rust-lang.org/nomicon/drop-flags.html)).

Swift's type metadata carries a value witness table with operations such
as copying and destroying a value
([type metadata](https://github.com/swiftlang/swift/blob/main/docs/ABI/TypeMetadata.rst)).
Mojo destroys a value after its last use, not at scope end
([death](https://docs.modular.com/mojo/manual/lifecycle/death/)).
OxCaml has no destructors.

**Cost.** Neither option adds a rule. A grows code only for owned
instantiations whose type has a destructor, which are few. B keeps code
small but pays an indirect call per drop and flags in every instance.

**Recommendation: A**, as a compiler choice outside the spec, with its
footprint reported as for any compiler change.

**Question.** Is generic drop dispatch a compiler choice outside the
spec, starting with drop-aware folding?

### 6.5 Higher-Order Functions

A pool drops its idle connections and names the rest:

```text
fn prune(pool: ^mut Pool) -> List[string]:            # hypothetical syntax
    pool.idle.retain(fn(c: ^Conn) -> bool: c.sent() > 0)                # hypothetical syntax
    pool.idle.iter().map(fn(c: ^Conn) -> string: c.peer()).collect()   # hypothetical syntax
```

Today `List.map` and `List.filter` hand each element to a callback
taking `T`. For an owned element, that moves it out of a list the caller
still owns. So by the decided container split, those methods stay in the
plain-only impl. The open point is which shapes owned lists get.

**Option A: the item carries the mode.** Adapters keep their signatures
on `Iterator`. `iter()` on an owned list yields `^Conn` items, so `map`
above is today's `map` with `T = ^Conn`. `List` gains
`retain(^mut self, keep: fn(^T) -> bool)`, which drops what it removes.

Consuming work goes through `into_iter()`, whose cursor has `map`,
`fold`, and a `filter` that tests `^T` and drops what it rejects. They
run eagerly and return a new cursor. A lazy one would be a named
producer, which `for` does not take (6.1).

**Option B: owned twins on `List`.** Every adapter gets two more names,
as `map_borrowed` and `map_consuming`. Swift's SE-0437 weighed
`borrowingMap` and `consumingMap`, then preferred views.

**Option C: borrowed callbacks everywhere.** Every list method moves to
an `[T < own Any]` impl with `fn(^T)` callbacks. A `^Point` is not a
`Point`, so every lambda over a plain list changes type.

The one-shot closure rule stays, so a callback that reads an owned value
captures a borrow of it. Exclusive state rides in `fold`'s accumulator
or in a loop:

```text
fn names(ids: ^List[usize], table: own Table) -> List[string]:   # hypothetical syntax
    let t: ^Table = table                                         # hypothetical syntax
    ids.iter().map(fn(id: usize) -> string: t.lookup(id)).collect()
```

**Elsewhere.** Rust's `map` takes `FnMut(Self::Item) -> B`, and `filter`
takes `FnMut(&Self::Item) -> bool`
([Iterator](https://doc.rust-lang.org/std/iter/trait.Iterator.html)). So
the item type decides what a callback sees, and `Vec::retain` lends
each element.
Swift plans consuming `map`, `filter` and `reduce`, with borrowing
through a view, once a type can store a borrow (SE-0437). A Mojo capture
list says per value whether a closure reads, mutates, copies or moves it
in ([closures](https://docs.modular.com/mojo/manual/functions/closures/)).
An OxCaml closure that consumes a unique value is `once`.

**Cost.** A: the owned cursor's adapters and `retain`, std only, with a
fix-it from `conns.map(f)` to `conns.iter().map(f)`. B: about twice the
adapter names. C: a breaking change to every plain callback.

**Recommendation: A.** It reuses every adapter and matches Rust, where
the same `map` serves `iter()` and `into_iter()`.

**Question.** Do owned lists reach `map`, `filter` and `fold` through
`iter()` and `into_iter()`, plus `retain`, with no owned twins on `List`?

### 6.6 Migration Order

The phasing is decided: std marks `^self` in steps, each proposed by a
lint. The order, what the lint checks, and the spec chapters each step
touches are open.

**Option A: bottom up.** Each step needs only the steps above it:

| Step | Change | Spec |
| --- | --- | --- |
| 1. Language | `own`, `^`, moves, borrows, drop, `OwnOnly`, `DropOwn`, `defer` as a guard | 01 to 11, 14 |
| 2. Traits | `^self` on `Eq`, `Hash`, `Ord`, `Display`, `Debug`, `Iterable`; `OwnOnly` by `Structure` on `Option`, `Result`, `List`, `Map`, `Set` | 06, 09, 10; std cmp, hash, format, iter |
| 3. Containers | owned-element methods in `[T < own Any]` impls, `IntoIter`, `retain` | 10; std collections, iter, option, result |
| 4. Handles | sockets, listeners and readers become `OwnOnly` with `DropOwn` | std net, fs, process, http |
| 5. The rest | `^self` wherever the lint proposes it | std, module by module |

**Option B: handles first.** The safety payoff comes first. But an
owned handle cannot be compared, printed or kept in a list until steps 2
and 3 land.

**Option C: all at once.** One large change, with no state where an
owned value meets an unmarked method.

The lint reuses checks the compiler already runs. It proposes `^self`
where a method's body still checks with a borrowed receiver. It proposes
`[T < own Any]` where a container method never copies a `T`.

**Elsewhere.** Swift moved in steps. Noncopyable structs came in 5.9
(SE-0390), generics and std primitives in 6.0
([SE-0427](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0427-noncopyable-generics.md),
SE-0437), and `Equatable` with its neighbors in 6.4 (SE-0499). Rust
shipped ownership in 1.0, so it had no migration.

**Recommendation: A**, with step 4 done together with the streaming
redesign, since both rewrite the same handle traits.

**Question.** Migrate in the order language, traits, containers,
handles with streaming, then the rest of std by lint?

### Consequences For Std Handles

This decides nothing; the streaming design builds on it. Under these
recommendations, `TcpStream`, `TcpListener`, `UdpSocket` and the
planned readers for HTTP bodies, process output and files implement
`OwnOnly` and `DropOwn`. So `connect!` returns `own dyn TcpStream`, and
dropping a handle replaces every `close` method, on each exit and on
cancellation. A writer whose close can fail keeps a consuming `finish!`
that returns a `Result`, as 3.1 decides. The decided `Closed` variant
can no longer occur, since a call after a drop or a move is
`use-after-move`. Reads and writes take `^mut self`, so a helper takes
`s: ^mut dyn TcpStream`; each call reborrows it, and the caller still
owns and drops it. A line splitter holds a `^mut`, so it is `OwnOnly`
named data, driven by a loop, since `for` takes no suspending cursor.

## Appendix: Limits And Holes

The holes found so far are closed by the owner's rules: B7 by allowing
`own` and `^mut` fields only in `OwnOnly` types, with ownership
derived by `Structure` for generic holders; B8 by `T < own Any`; and the
second review's U4 to U8 by no partial moves, drop counting as a use of held borrows,
`OwnOnly` for holders of `^mut`, plain-only providers, and
immovable borrowed globals. A plain alias or a plain closure capture
that changes a borrowed variable's object is not a hole: plain values
keep garbage-collected semantics by design
([3.6](#36-borrowing-plain-values)).
No safety argument is complete until the spec is written; the second
review is in [OWNERSHIP_STRESS_TEST_2.md](OWNERSHIP_STRESS_TEST_2.md).

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
- Lorenzen, White, Dolan, Eisenberg, Lindley, [Oxidizing OCaml with Modal Memory Management](https://doi.org/10.1145/3674642), ICFP 2024
- OxCaml documentation: [modes](https://oxcaml.org/documentation/modes/intro/), [stack allocation](https://oxcaml.org/documentation/stack-allocation/intro/), [uniqueness](https://oxcaml.org/documentation/uniqueness/intro/), [borrowing](https://oxcaml.org/documentation/uniqueness/borrow/)
- Section 6 adds: Rust [destructors](https://doc.rust-lang.org/reference/destructors.html), [drop flags](https://doc.rust-lang.org/nomicon/drop-flags.html), [Iterator](https://doc.rust-lang.org/std/iter/trait.Iterator.html), [HashMap](https://doc.rust-lang.org/std/collections/struct.HashMap.html) and [Clone](https://doc.rust-lang.org/std/clone/trait.Clone.html); Swift [SE-0437](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0437-noncopyable-stdlib-primitives.md), [SE-0499](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0499-support-non-copyable-simple-protocols.md) and [type metadata](https://github.com/swiftlang/swift/blob/main/docs/ABI/TypeMetadata.rst); Mojo [life](https://docs.modular.com/mojo/manual/lifecycle/life/), [death](https://docs.modular.com/mojo/manual/lifecycle/death/), [control flow](https://docs.modular.com/mojo/manual/control-flow/) and [closures](https://docs.modular.com/mojo/manual/functions/closures/); all read on 2026-10-17

## Parse Log

Every `text` block, 35 in all, was parsed with
`scripts/hd-conformance.sh parse`: the 29 of sections 1 to 3 on
2026-10-16, and the six of section 6 on 2026-10-17. Each was parsed as written and with
the marks erased (`own`, `^`, `^{...}`, lifetime brackets). Parsing
checks syntax only. Nothing was type-checked, and the blocks call
functions this study does not define.

| Result | Blocks |
| --- | --- |
| Parses as written | 5, among them section 1, the REPL inputs in 3.11, and the `defer` row example in 3.1 |
| Fails only at the new marks; parses with the marks and `impl OwnOnly` lines erased | the other 30, all six of section 6 among them |
