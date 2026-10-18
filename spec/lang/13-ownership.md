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
> parameter, or names a lifetime. So is a moded value that a closure
> captures, that a pattern, `for`, or a built-in method reads, or that a
> suspending body holds. A borrow in a plain position and a lifetime named
> by a requirement key are diagnosed too.

## Modes

A **mode** says how a position holds its value. It is written on the type at
the position, as `mut` is, so one type can be plain in one place and borrowed
in another.

| Mode | Written | A value in this mode |
| --- | --- | --- |
| plain | `T`, `mut T` | keeps the object semantics of the other chapters: copying it makes an alias |
| borrowed | `^T`, `^mut T` | refers to a place it does not own, for a lifetime |

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
type_mode = "^", [ lifetime_set ], [ "mut" ] ;
lifetime_set = "{", lifetime, { ",", lifetime }, [ "," ], "}" ;
lifetime = ( identifier | "self" ), { ".", identifier }
         | "$", qualified_name ;
lifetime_parameter = "^", identifier ;
lifetime_argument = "^", [ lifetime_set ] ;
```

The productions that use them, `type`, `reference_access_type`,
`result_type`, `receiver_parameter`, the generic parameter lists, and the
type-argument lists, are in [Grammar](02-grammar.md#types).

### Borrowed Types

`^T` or `^mut T` writes a borrowed type.

```text
data Conn:
    fd: i32

fn size(c: ^Conn) -> usize: 0

fn fill(buf: ^mut List[u8]) -> void: pass

fn busier(a: ^Conn, b: ^Conn) -> ^{a, b} Conn:
    if size(a) > size(b): a else: b
```

1. r[own.syntax.borrow] `^T` writes a shared borrow, and `^mut T` a mutable borrow.
2. r[own.syntax.borrow-lifetime] A lifetime set comes right after `^` and before `mut`, as in `^{a} T` and `^{a, b} mut T`. A lifetime set after `mut` is an error. Error: `syntax-error`.
3. r[own.syntax.one-mark] A type takes at most one of `mut` and a borrow mark. So `mut ^T` and `^^T` are errors. Error: `syntax-error`.
4. r[own.syntax.optional] A `?` after a marked type wraps the marked type, as it wraps `mut T`: `^T?` is `Option[^T]`.
5. r[own.syntax.positions] A mark may be written on the type of a parameter, a result, a local, a field, an enum payload, a type argument, a tuple element, and a function type's parameters and result. The rules of a later section decide which mode each position admits.
6. r[own.syntax.no-expression] No expression writes a mark: `^` in an expression is the bitwise XOR operator of [Precedence](05-expressions.md#precedence).

```text
data File:
    fd: i32

fn keep(f: mut ^File) -> void: pass                 # error: syntax-error
fn write(f: ^mut {g} File, g: ^File) -> void: pass  # error: syntax-error
```

### Lifetimes

A **lifetime** names how long a borrow lasts. A lifetime set lists the
lifetimes a borrow or a lifetime argument is bounded by.

1. r[own.lifetime.set] A lifetime set is `{`, one or more lifetimes separated by commas, and `}`. An empty set `{}` is an error. Error: `syntax-error`.
2. r[own.lifetime.named-by] A lifetime is written as a variable, that is a parameter or `self`, as a declared lifetime parameter, as a requirement key such as `$Store`, or as `static`.
3. r[own.lifetime.field-path] A variable may be followed by a path of field names, as in `^{u.name}` or `^{u.addr.city}`. The path names part of the variable's value, not a type parameter.

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

### Lifetime Parameters And Arguments

A declaration that holds a borrow lists a lifetime parameter for it, and
each use of the declaration writes a lifetime argument.

```text
data Document:
    lines: List[string]

data Parser[^doc]:
    source: ^{doc} Document
    at: usize

data Pair[A, ^a]:
    left: ^{a} A

fn parser(d: ^Document) -> Parser[^]:
    Parser { source: d, at: 0 }

fn named(text: ^Document) -> Parser[^{text}]: parser(text)

impl[^doc] Parser[^{doc}]:
    fn line(^self) -> usize: self.at
```

1. r[own.lifetime-param.declared-by] A data type, an enum, a trait, a `type` declaration, an implementation, or a function declares a lifetime parameter as `^` and a name, as in `data Parser[^doc]` and `fn first[^c](...)`.
2. r[own.lifetime-param.last] Lifetime parameters follow every type parameter. A type parameter after a lifetime parameter is an error. Error: `syntax-error`.
3. r[own.lifetime-param.plain] A lifetime parameter takes no variance marker, bound, or default. Error: `syntax-error`.
4. r[own.lifetime-param.inner] A function declares a lifetime parameter to name a lifetime that no variable names, such as a borrow inside a parameter's type argument, as in `fn first[^c](x: ^List[^{c} Conn]) -> ^{c} Conn`.
5. r[own.lifetime-arg.write] A use writes a lifetime argument as `^` and a lifetime set, or as a bare `^` that elides it, as in `Parser[^{text}]` and `Parser[^]`.
6. r[own.lifetime-arg.last] Lifetime arguments follow every positional type argument and come before the bindings. Error: `syntax-error`.
7. r[own.lifetime-arg.vs-borrow] In a type-argument list, `^` or `^` and a lifetime set is a lifetime argument when `,` or `]` follows it. When a type follows, it is a borrowed type argument, as in `Suspend[^{keep} Conn]`.
8. r[own.lifetime-arg.types-only] An expression's type-argument list, as in `f::[T]` or `Parser::[T] { ... }`, takes no lifetime argument. Error: `syntax-error`.

```text
data Conn:
    fd: i32

data Pair[^a, A]:                     # error: syntax-error
    left: ^{a} A

fn pick[^a, T](c: ^Conn) -> ^{a} Conn: c  # error: syntax-error
```

## Modes In Declarations

A declaration's header says how each position holds its value: the
parameters, the result, and the receiver of a function, and the fields of a
data type or an enum.

```text
data File:
    fd: i32

data Document:
    lines: List[string]

data Parser[^doc]:
    source: ^{doc} Document
    at: usize

trait Socket:
    fn send(^mut self, bytes: List[u8]) -> void
    fn peer(^self) -> ^string

fn size(f: ^File) -> usize: 0

fn parser(d: ^Document) -> Parser[^]: panic("stub")

fn busier(a: ^File, b: ^File) -> ^{a, b} File: panic("stub")

fn first[^c](xs: ^List[^{c} File]) -> ^{c} File: panic("stub")
```

### Marks In Function Types

1. r[own.header.fn-constructor] A mark inside the arguments of `Fn` or `SuspendFn` is an error; the `fn(...)` form takes marks. Error: `misplaced-lifetime`.

```text
use std.function.Fn

data File:
    fd: i32

fn call(f: Fn[(^File,), void]) -> void: pass   # error: misplaced-lifetime
```

### Lifetimes In Declarations

1. r[own.header.lifetime-scope] In a function's header, a lifetime name is first looked up among the function's parameters, `self` included, then among the lifetime parameters in scope, then as `static`. A field's type names only its declaration's lifetime parameters and `static`. A name found nowhere is an error. Error: `unknown-name`.
2. r[own.header.field-path] Each field of a lifetime's field path must be a field of the type before it, with any mode removed, and must be `pub`, so a signature never names a private field. Error: `unknown-field` for a missing field, `private-member` for a field that is not `pub`.
3. r[own.header.field-bare] A borrow in a field's type names its lifetime, as in `^{doc} Document`. A bare `^` there is an error. Error: `missing-lifetime`.
4. r[own.header.parameter-bare] A bare `^` in a parameter's type, at any depth, is a lifetime of its own that nothing else can name.
5. r[own.header.lifetime-arguments] A use of a declaration that has lifetime parameters writes one lifetime argument for each, at least `[^]`. Leaving them out is an error, fixed by writing `[^]`. Error: `missing-lifetime`. More lifetime arguments than lifetime parameters is an error. Error: `argument-count`.
6. r[own.header.elision] A bare `^` in a function's result, at any depth, takes the lifetime of a `^self` or `^mut self` receiver. Without one, it takes the lifetime of the one parameter whose type holds a borrow or a lifetime argument.
7. r[own.header.elision-ambiguous] A bare `^` in a result with no such receiver, and with no such parameter or several, is an error; the result names its sources instead, as in `^{a, b}`. Error: `ambiguous-lifetime`.

```text
data File:
    fd: i32

data User:
    pub name: string
    age: i32

data Parser[^doc]:
    source: ^File                                 # error: missing-lifetime
    at: usize

fn read(p: Parser) -> void: pass                  # error: missing-lifetime
fn busier(a: ^File, b: ^File) -> ^File: panic("stub")   # error: ambiguous-lifetime
fn age_of(u: ^User) -> ^{u.age} i32: panic("stub")      # error: private-member
fn named(u: ^User) -> ^{w} string: panic("stub")        # error: unknown-name
```

> **Why.** A lifetime named by a variable reads as "lives as long as this
> argument", which is what a caller checks. A field path borrows only part of
> an argument, and keeping it to `pub` fields means a signature never shows
> a private field to another module.

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
4. r[own.body.field-read] A `^mut` borrow gives mutable access to its value's fields, and a shared borrow gives readonly access. A field read through a borrow is borrowed for the same lifetimes, mutably only through `^mut` and for a `mut` field. A primitive field is copied out, and a borrowed field is read as it is.
5. r[own.body.result-written] A function whose result is borrowed writes its result type. Error: `missing-result-type`.

```text
data File:
    fd: i32

fn view(f: ^File) -> ^File: f
fn reset(f: ^mut File) -> void: pass

fn wrong(f: File, b: ^File) -> void:
    reset(f)                                # error: mutable-upgrade
    reset(b)                                # error: mutable-upgrade

fn helper(f: ^File): view(f)                # error: missing-result-type
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
> instance, which needs the lifetime checks of a later section.

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

1. r[own.provider.plain] A provider of `$.with` or `$.context` is a plain value. A provider whose type has a borrowed part or a lifetime argument, at any depth, is an error. Error: `provider-not-plain`.
2. r[own.boundary.not-safe] A type with a borrowed part or a lifetime argument, at any depth, is not boundary-safe.
3. r[own.boundary.signature] A boundary signature with such a type is an error, reported at that signature. A host capability method's receiver does not cross, so its mark is allowed. Error: `not-boundary-safe`.

> **Why.** Sibling tasks share a provider with object semantics, so a
> borrowed provider would give two tasks one exclusive borrow. A value that
> crosses a boundary is copied as a tree, so it cannot point back into hd.

See also: [Boundary-Safe Values](10-modules.md#boundary-safe-values),
[Mutable Providers](11-requirements-and-suspension.md#mutable-providers).
