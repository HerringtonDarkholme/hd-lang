# Ownership and Borrows

Status: language specification draft.

This chapter defines the modes in which a position holds its value: plain
or borrowed.

> **Note.** The chapter grows section by section with the ownership
> prototype (issue #163), and each section arrives with the conformance cases
> that test it. Until a section specifies how a mode is checked, an
> implementation diagnoses a program that uses it, as
> [Specification Status](../README.md#specification-status) requires.
>
> So far the chapter checks marks in declarations, the modes values meet
> in bodies, the prototype's limits, and providers and boundaries. Loans
> are not specified yet.
>
> A body is diagnosed when it passes a moded type argument to a type
> parameter. So is a moded value that a closure captures, that a pattern,
> `for`, or a built-in method reads, or that a suspending body holds. A
> borrow in a plain position is diagnosed too.

## Modes

A **mode** says how a position holds its value. It is written on the type at
the position, as `mut` is, so one type can be plain in one place and borrowed
in another.

| Mode | Written | A value in this mode |
| --- | --- | --- |
| plain | `T`, `mut T` | keeps the object semantics of the other chapters: copying it makes an alias |
| borrowed | `^T`, `^mut T` | refers to a place it does not own, and stays inside the call or body that holds it |

1. r[own.mode.plain] A type written without `^` is plain. A plain value keeps the semantics the other chapters define, and the rules of this chapter do not apply to it.

```text
data Counter:
    hits: mut List[i32]

fn record(c: mut Counter) -> void:
    c.hits.push(+1)

fn count_twice() -> usize:
    let first: mut Counter = Counter { hits: [] }
    let second: mut Counter = first    # a plain copy is an alias
    record(second)
    first.hits.len()                   # 1: both names reach one object
```

> **Why.** Code with no mark compiles and runs as it did before modes
> existed. The checks that borrows add cost nothing where nothing is
> borrowed.

See also: [Mutation Checks](04-type-system.md#mutation-checks).

## Mark Syntax

A **mark** is the `^` that writes a mode on a type or a receiver. The
grammar accepts marks in every program.

```ebnf
type_mode = "^", [ "mut" ] ;
```

The productions that use it, `type`, `reference_access_type`,
`result_type`, and `receiver_parameter`, are in
[Grammar](02-grammar.md#types).

### Borrowed Types

`^T` or `^mut T` writes a borrowed type.

```text
data Conn:
    fd: i32

fn size(c: ^Conn) -> usize: 0

fn fill(buf: ^mut List[u8]) -> void: pass

fn busier(a: ^Conn, b: ^Conn) -> usize:
    if size(a) > size(b): size(a) else: size(b)
```

1. r[own.syntax.borrow] `^T` writes a shared borrow, and `^mut T` a mutable borrow.
2. r[own.syntax.one-mark] A type takes at most one of `mut` and a borrow mark. So `mut ^T` and `^^T` are errors. Error: `syntax-error`.
3. r[own.syntax.optional] A `?` after a marked type wraps the marked type, as it wraps `mut T`: `^T?` is `Option[^T]`.
4. r[own.syntax.positions] A mark may be written on the type of a parameter, a result, a local, a field, an enum payload, a type argument, a tuple element, and a function type's parameters and result. The rules of a later section decide which mode each position admits.
5. r[own.syntax.no-expression] No expression writes a mark: `^` in an expression is the bitwise XOR operator of [Precedence](05-expressions.md#precedence).

```text
data File:
    fd: i32

fn keep(f: mut ^File) -> void: pass                 # error: syntax-error
```

### Receivers

A receiver's mark sets its mode.

```text
data Pool:
    size: usize

impl Pool:
    fn plain(self) -> usize: self.size
    fn grow(mut self) -> void: pass
    fn peek(^self) -> usize: self.size
    fn give(^mut self, extra: usize) -> void: pass
```

1. r[own.receiver.forms] A method's receiver is `self`, `mut self`, `^self`, or `^mut self`.

## Modes In Declarations

A declaration's header says how each position holds its value: the
parameters and the receiver of a function take a mode, and a result and a
field are always plain.

```text
data File:
    fd: i32

trait Socket:
    fn send(^mut self, bytes: List[u8]) -> void
    fn peer(^self) -> string

fn size(f: ^File) -> usize: 0

fn fill(buf: ^mut List[u8]) -> void: pass

fn job(run: ^fn() -> void) -> void: pass

fn visit(files: List[File], each: fn(^File) -> void) -> void: pass
```

### Where Marks Go

1. r[own.header.placement] A mark is written only at the top of the type of a parameter, a receiver, a binding, or a parameter of a `fn(...)` type, as in `f: ^File`, `let v: ^File = f`, and `fn(^File) -> void`. A mark anywhere else is an error: on a result, a field, an enum payload, a type argument, a tuple element, the right side of a `type` declaration, or under `?`. So `-> ^File`, `List[^File]`, `^File?`, and `Fn[(^File,), void]` are errors. Error: `misplaced-borrow`.

```text
use std.function.Fn

data File:
    fd: i32

data Reader:
    source: ^File                                 # error: misplaced-borrow

fn view(f: ^File) -> ^File: f                     # error: misplaced-borrow
fn walk(files: List[^File]) -> void: pass         # error: misplaced-borrow
fn peek(f: ^File?) -> void: pass                  # error: misplaced-borrow
fn call(run: Fn[(^File,), void]) -> void: pass    # error: misplaced-borrow
```

> **Why.** A borrowed value stays inside the call or the body that holds
> it. A result, a field, or a type argument would carry it out, and no
> lifetime says how long it may live there.

## Modes In Bodies

Inside a body, a value meets a position's mode where it is passed,
returned, bound, or stored. A value is lent to a borrowed position, and a
plain value stays plain.

```text
data File:
    fd: i32

impl File:
    fn size(^self) -> i32: self.fd
    fn reset(^mut self) -> void:
        self.fd = 0

data Buffer:
    bytes: List[u8]
    label: string

fn open(path: string) -> mut File: File { fd: 3 }
fn size(f: ^File) -> i32: f.fd
fn fill(b: ^mut Buffer) -> void:
    b.label = "full"

fn report() -> i32:
    let f: mut File = open("app.log")
    n := size(f)                      # lends `f`
    f.reset()                         # lends `f` mutably
    n + f.size()

fn buffers() -> string:
    let plain: mut Buffer = Buffer { bytes: [1], label: "old" }
    fill(plain)                       # a plain place with a `mut` view lends mutably
    let view: ^Buffer = plain
    view.label
```

### Lending

1. r[own.body.lend] A value where `^T` or `^mut T` is expected is lent to that position: a plain value or a borrow. No expression lends.
2. r[own.body.mutable-upgrade] `^mut` lends a plain value with a `mut` view, or a `^mut` borrow; a readonly plain value and a shared borrow are errors. Error: `mutable-upgrade`.
3. r[own.body.receiver] A receiver follows the rules of a parameter: `^self` and `^mut self` lend it, and `self` or `mut self` takes a plain value.
4. r[own.body.field-read] A `^mut` borrow gives mutable access to its value's fields, and a shared borrow gives readonly access. A field read through a borrow is borrowed, mutably only through `^mut` and for a `mut` field. A primitive field is copied out, and a borrowed field is read as it is.
5. r[own.body.result-plain] A result is never borrowed, so a function whose result type is left out and whose body gives a borrowed value is an error. Error: `missing-result-type`.

```text
data File:
    fd: i32

fn reset(f: ^mut File) -> void: pass

fn wrong(f: File, b: ^File) -> void:
    reset(f)                                # error: mutable-upgrade
    reset(b)                                # error: mutable-upgrade

fn helper(f: ^File): f                      # error: missing-result-type
```

> **Why.** Lending marks nothing at the call site, so a call reads the
> same whether it borrows or not.

## Prototype Limits

Some forms of borrowed values wait for later sections of this chapter. A
program that uses one is rejected with a diagnostic of its own.

```text
data Settings:
    level: i32

fn level(s: ^Settings) -> i32: s.level

CONFIG := Settings { level: 1 }
let VIEW: ^Settings = CONFIG              # error: not-in-ownership-prototype

fn current() -> i32: level(CONFIG)        # error: not-in-ownership-prototype
```

1. r[own.limit.top-level] A top-level binding whose type has a borrowed part, at any depth, is an error. Error: `not-in-ownership-prototype`.
2. r[own.limit.top-level-borrow] A top-level binding, or a field of one, lent to a borrowed position is an error, receivers included. Error: `not-in-ownership-prototype`.

> **Why.** A borrow of a top-level binding would last as long as the
> instance, which needs the checks of a later section.

```text
data Settings:
    level: i32

trait Sink:
    fn write(self) -> void

data Memory:
    count: i32

impl Sink for Memory:
    fn write(self) -> void: pass

fn plain(s: Settings) -> i32: s.level
fn peek(s: ^Settings) -> i32: s.level

CONFIG := Settings { level: 1 }

fn current() -> i32:
    local := Settings { level: CONFIG.level }
    peek(local) + plain(CONFIG)           # a local is lent; a top-level binding stays plain

fn install(view: ^Settings) -> void:
    $.with(Sink = Memory { count: view.level }):
        pass                              # a plain provider, built from a borrowed value's field
```

## Providers And Boundaries

Providers and values that cross a boundary stay plain.

```text
trait Sink:
    fn write(^mut self) -> void

data Token:
    pub owner: string

trait Vault:                                  # a host capability of the selected runtime profile
    fn peek(self, token: ^Token) -> string    # error: not-boundary-safe

fn emit(sink: ^mut dyn Sink) -> void:
    sink.write()

fn twice(sink: ^mut dyn Sink) -> void:
    emit(sink)                                # an exclusive service is a parameter
    emit(sink)

fn wrong(sink: ^mut dyn Sink) -> void:
    $.with(Sink=sink):                        # error: provider-not-plain
        pass
```

1. r[own.provider.plain] A provider of `$.with` or `$.context` is a plain value. A provider whose type has a borrowed part, at any depth, is an error. Error: `provider-not-plain`.
2. r[own.boundary.not-safe] A type with a borrowed part, at any depth, is not boundary-safe.
3. r[own.boundary.signature] A boundary signature with such a type is an error, reported at that signature. A host capability method's receiver does not cross, so its mark is allowed. Error: `not-boundary-safe`.

> **Why.** Sibling tasks share a provider with object semantics, so a
> borrowed provider would give two tasks one exclusive borrow. A value that
> crosses a boundary is copied as a tree, so it cannot point back into hd.

See also: [Boundary-Safe Values](10-modules.md#boundary-safe-values),
[Mutable Providers](11-requirements-and-suspension.md#mutable-providers).
