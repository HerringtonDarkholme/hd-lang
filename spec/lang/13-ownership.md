# Ownership and Borrows

Status: language specification draft.

This chapter defines the modes in which a position holds its value: plain,
owned, or borrowed.

> **Note.** The chapter grows section by section with the ownership
> prototype (issue #163), and each section arrives with the conformance cases
> that test it. Until a section specifies how a mode is checked, an
> implementation diagnoses a program that uses it, as
> [Specification Status](../README.md#specification-status) requires.
>
> So far the chapter checks marks in declarations, ownership by structure,
> the modes values meet in bodies, moves, drops, the prototype's limits,
> and providers and boundaries. Loans are not specified yet.
>
> A body is diagnosed when it holds a value of a type parameter with an
> `own` bound, passes a moded type argument to a type parameter without
> one, or names a lifetime. So is a moded value that a closure captures,
> that a pattern, `for`, or a built-in method reads, or that a suspending
> body holds. So is an owned temporary that a `return`, `break`,
> `continue`, or `?` could leave behind. A borrow in a
> plain position and a lifetime named by a requirement key are diagnosed
> too.

## Modes

A **mode** says how a position holds its value. It is written on the type at
the position, as `mut` is, so one type can be plain in one place and owned in
another.

| Mode | Written | A value in this mode |
| --- | --- | --- |
| plain | `T`, `mut T` | keeps the object semantics of the other chapters: copying it makes an alias |
| owned | `own T` | is the only path to its value |
| borrowed | `^T`, `^mut T` | refers to a place it does not own, for a lifetime |

1. r[own.mode.plain] A type written without `own` or `^` is plain. A plain value keeps the semantics the other chapters define, and the rules of this chapter do not apply to it.

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
> existed. The checks that ownership adds cost nothing where nothing is owned
> or borrowed.

See also: [Mutation Checks](04-type-system.md#mutation-checks).

## Mark Syntax

A **mark** is the `own` or `^` that writes a mode on a type, a binding, a
receiver, or a bound. The grammar accepts marks in every program.

```ebnf
type_mode = "own"
          | "^", [ lifetime_set ], [ "mut" ] ;
lifetime_set = "{", lifetime, { ",", lifetime }, [ "," ], "}" ;
lifetime = ( identifier | "self" ), { ".", identifier }
         | "$", qualified_name ;
lifetime_parameter = "^", identifier ;
lifetime_argument = "^", [ lifetime_set ] ;
```

The productions that use them, `type`, `reference_access_type`,
`result_type`, `receiver_parameter`, `let_statement`, the generic parameter
lists, `trait_bounds`, and the type-argument lists, are in
[Grammar](02-grammar.md#types).

### Owned And Borrowed Types

`own T` writes an owned type, and `^T` or `^mut T` a borrowed one.

```text
data File:
    fd: i32

data Conn:
    fd: i32

fn open(path: string) -> own File: File { fd: 3 }

fn archive(f: own File) -> void: pass

fn on_close(f: own File) -> own fn() -> void:
    fn() -> void: archive(f)

fn size(c: ^Conn) -> usize: 0

fn fill(buf: ^mut List[u8]) -> void: pass

fn busier(a: ^Conn, b: ^Conn) -> ^{a, b} Conn:
    if size(a) > size(b): a else: b
```

1. r[own.syntax.own] `own T` writes an owned type. `own` comes before `dyn` and `fn`, as in `own dyn Socket` and `own fn() -> void`.
2. r[own.syntax.borrow] `^T` writes a shared borrow, and `^mut T` a mutable borrow.
3. r[own.syntax.borrow-lifetime] A lifetime set comes right after `^` and before `mut`, as in `^{a} T` and `^{a, b} mut T`. A lifetime set after `mut` is an error. Error: `syntax-error`.
4. r[own.syntax.no-own-mut] `own` grants mutation, so it never takes `mut`: `own mut T`, `own mut self`, and `let own mut x` are errors. Error: `syntax-error`.
5. r[own.syntax.one-mark] A type takes at most one of `mut`, `own`, and a borrow mark. So `own ^T`, `^own T`, and `mut ^T` are errors. Error: `syntax-error`.
6. r[own.syntax.optional] A `?` after a marked type wraps the marked type, as it wraps `mut T`: `own T?` is `Option[own T]`, and `^T?` is `Option[^T]`.
7. r[own.syntax.positions] A mark may be written on the type of a parameter, a result, a local, a field, an enum payload, a type argument, a tuple element, and a function type's parameters and result. The rules of a later section decide which mode each position admits.
8. r[own.syntax.contextual] `own` is a mark only directly before a type, a `let` pattern, `self`, or a bound, on the same line. Elsewhere it is an ordinary identifier, so `own := 1` and a parameter `own: i32` stay valid. A `(` written right after `own`, as in `let own(v) = w`, keeps `own` a name.
9. r[own.syntax.no-expression] No expression writes a mark: `^` in an expression is the bitwise XOR operator of [Precedence](05-expressions.md#precedence).

```text
data File:
    fd: i32

fn reset(f: own mut File) -> void: pass             # error: syntax-error
fn keep(f: own ^File) -> void: pass                 # error: syntax-error
fn write(f: ^mut {g} File, g: ^File) -> void: pass  # error: syntax-error
```

### Lifetimes

A **lifetime** names how long a borrow lasts. A lifetime set lists the
lifetimes a borrow or a lifetime argument is bounded by.

1. r[own.lifetime.set] A lifetime set is `{`, one or more lifetimes separated by commas, and `}`. An empty set `{}` is an error. Error: `syntax-error`.
2. r[own.lifetime.named-by] A lifetime is written as a variable, that is a parameter or `self`, as a declared lifetime parameter, as a requirement key such as `$Store`, or as `static`.
3. r[own.lifetime.field-path] A variable may be followed by a path of field names, as in `^{u.name}` or `^{u.addr.city}`. The path names part of the variable's value, not a type parameter.

### Owned Bindings And Receivers

`let own` binds an owned local, and a receiver's mark sets its mode.

```text
data User:
    name: string

data Pool:
    size: usize

impl Pool:
    fn plain(self) -> usize: self.size
    fn grow(mut self) -> void: pass
    fn close(own self) -> void: pass
    fn peek(^self) -> usize: self.size
    fn give(^mut self, extra: usize) -> void: pass

fn make() -> void:
    let own user = User { name: "ada" }
```

1. r[own.binding.let-own] `own` after `let` binds an owned local, as in `let own user = value` or `let own buf: Buffer = value`.
2. r[own.receiver.forms] A method's receiver is `self`, `mut self`, `own self`, `^self`, or `^mut self`.

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

### Owned Bounds

`own` at the start of a bound admits owned type arguments.

```text
trait Named:
    fn name(^self) -> string

fn take_first[T < own Any](xs: own List[T]) -> Option[T]: xs.pop()

fn label[T < own Named & Any](x: ^T) -> string: x.name()
```

1. r[own.bound.own] `own` comes first in a bound list, where `mut` may stand, as in `T < own Any`.
2. r[own.bound.one-mark] A bound list takes `own` or `mut`, not both. Error: `syntax-error`.
3. r[own.bound.argument] A type parameter with an `own` bound takes an owned type argument, and a call infers it from an owned argument, as `drop(file)` does ([own.drop.call](#r-own.drop.call)).

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

fn archive(f: own File) -> void: pass

fn size(f: ^File) -> usize: 0

fn parser(d: ^Document) -> Parser[^]: panic("stub")

fn busier(a: ^File, b: ^File) -> ^{a, b} File: panic("stub")

fn first[^c](xs: ^List[^{c} File]) -> ^{c} File: panic("stub")
```

### Owned Positions

1. r[own.header.own-on-value] `own` on a primitive type, `string` included, is an error, since a primitive value is immutable and has no owner. Error: `own-on-value`.
2. r[own.header.own-on-type-parameter] `own T` on a type parameter `T` is an error; a generic that takes owned values bounds its parameter, as in `T < own Any`. `own Self` is allowed. Error: `own-on-type-parameter`.
3. r[own.header.fn-constructor] A mark inside the arguments of `Fn` or `SuspendFn` is an error; the `fn(...)` form takes marks. Error: `misplaced-lifetime`.

```text
use std.function.Fn

data File:
    fd: i32

fn count(n: own i32) -> void: pass             # error: own-on-value
fn keep[T](x: own T) -> void: pass             # error: own-on-type-parameter
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

## Ownership Traits

The module `std.ownership` declares two traits that the compiler knows by
name. Both are prelude names
([Prelude](10-modules.md#prelude)).

```text
pub trait OwnOnly

pub trait DropOwn < OwnOnly:
    fn drop(^mut self) -> void
```

```text
data File:
    fd: i32

impl OwnOnly for File

impl DropOwn for File:
    fn drop(^mut self) -> void: pass

data Pool:
    idle: own List[own File]
    first: own File?

impl OwnOnly for Pool

fn open(path: string) -> own File: panic("stub")

fn size(f: ^File) -> usize: 0
```

1. r[own.own-only.marker] `OwnOnly` is a marker trait with no members. A type that implements it is always owned or borrowed.
2. r[own.own-only.plain] A plain position, that is one that no `own` or `^` wraps directly, of a type that implements `OwnOnly` is an error. A receiver is a position, so `self` and `mut self` count. Error: `own-type-plain`.
3. r[own.own-only.written] Only a written declaration's positions are checked; a derived or delegated method has the signature its trait gives it.
4. r[own.own-only.field] A data type or an enum with a field or payload written `own T` or `^mut T`, under any number of `?`, must implement `OwnOnly`. Error: `own-field-plain-type`.
5. r[own.own-only.module] An `impl OwnOnly` is declared in the module that declares its target type. Elsewhere in the target's package it is an error. Error: `nonlocal-impl`. In another package it is an error. Error: `orphan-impl`.
6. r[own.own-only.unconditional] A written `impl OwnOnly for Box[T]` makes every instance of `Box` implement `OwnOnly`, whatever `T` is.
7. r[own.drop-own.trait] `DropOwn` extends `OwnOnly`, so only an `OwnOnly` type implements it; an `impl DropOwn` for any other type is `missing-supertrait-implementation`. Its `drop` takes `^mut self` and returns `void`.

```text
data File:
    fd: i32

impl OwnOnly for File

data Holder:
    file: own File                       # error: own-field-plain-type

data Plain:
    fd: i32

impl DropOwn for Plain:                  # error: missing-supertrait-implementation
    fn drop(^mut self) -> void: pass

fn bad(f: File) -> void: pass            # error: own-type-plain
fn many(fs: ^List[File]) -> void: pass   # error: own-type-plain
impl OwnOnly for i32                     # error: orphan-impl
```

> **Why.** A plain value may already have aliases, so a plain `File` could
> be read after its owner dropped it. Requiring `own` or `^` at every use of
> an `OwnOnly` type means no producer hands out a plain one.

## Ownership By Structure

A generic type keeps a plain header and may take its ownership from its
fields: an instance implements `OwnOnly` exactly when some field's type
owns, after substitution.

```text
use std.structure.Structure

data Conn:
    fd: i32

data Batch[T]:
    values: List[T]

impl[T] OwnOnly for Batch[T] by Structure

@derive(OwnOnly)
enum Outcome[A, B]:
    Good(value: A)
    Bad(error: B)

@derive(OwnOnly)
data Chain[T]:
    value: T
    next: Chain[T]?

fn accept(batch: own Batch[own Conn]) -> void: pass       # owned: its list owns
fn settle(result: own Outcome[own Conn, string]) -> void: pass
fn sum(batch: Batch[i32]) -> i32: 0                         # plain: nothing owns
fn walk(chain: ^Chain[own Conn]) -> void: pass
```

1. r[own.structure.declare] `@derive(OwnOnly)`, or `impl[T] OwnOnly for Box[T] by Structure`, declares that `Box` implements `OwnOnly` by structure. Like a written impl, it is declared in the module that declares its target type.
2. r[own.structure.instance] An instance of a type that implements `OwnOnly` by structure implements `OwnOnly` when the type of one of its fields or payloads owns, after substituting the instance's type arguments.
3. r[own.structure.owns] A type **owns** by the table below.
4. r[own.structure.recursive] A recursive type owns only when the table reaches an owning part without going around the recursion, that is the least solution of the table.
5. r[own.structure.builtin] `T?` and tuples take ownership from their elements by the table, and `List[T]` and `Map[K, V]` from `T` and `V`, with no declaration.
6. r[own.structure.admission] An `own` type argument is allowed for a list's element and a map's value. It is also allowed for a type that implements `OwnOnly` by structure, or by a written impl for that instance. Any other `own` type argument is an error, a map's key included. Error: `own-in-plain-type`.
7. r[own.structure.exclusive] A `^mut` field makes no instance own, so a type with one implements `OwnOnly` by a written impl, as [own.own-only.field](#r-own.own-only.field) requires.
8. r[own.structure.drop-own] An `impl DropOwn` must cover only instances that implement `OwnOnly`, so a type that owns only for some arguments by structure has no `DropOwn` for every instance. Error: `missing-supertrait-implementation`.
9. r[own.own-only.field-tuple] A tuple element of a field's or payload's type counts as a field for [own.own-only.field](#r-own.own-only.field), so `(own Conn, i32)` needs an `OwnOnly` holder. Error: `own-field-plain-type`.

| A part of type | Owns |
| --- | --- |
| `own U` | always |
| `^U` | never, whatever `U` is |
| `^mut U` | never; see [own.structure.exclusive](#r-own.structure.exclusive) |
| `mut U`, `U?` | when `U` owns |
| a tuple | when one of its elements owns |
| a function type | never, whatever its result is |
| a type parameter | when its argument owns |
| `List[U]`, `Map[K, U]` | when `U` owns |
| a type that implements `OwnOnly` by structure | by [own.structure.instance](#r-own.structure.instance) |
| a type that implements `OwnOnly` by a written impl | always |
| a primitive type, or another type | never |

```text
data Conn:
    fd: i32

@derive(OwnOnly)
data View[T, ^a]:
    value: ^{a} T

@derive(OwnOnly)
data Token[T]:
    id: usize

@derive(OwnOnly)
data Factory[T]:
    make: fn() -> T

fn views(v: View[own Conn, ^]) -> void: pass   # plain: a shared borrow owns nothing
fn tokens(t: Token[own Conn]) -> void: pass    # plain: no field holds a T
fn makers(f: Factory[own Conn]) -> void: pass  # plain: a function type owns nothing
```

```text
data Conn:
    fd: i32

data File:
    fd: i32

data Box[T]:
    item: T

@derive(OwnOnly)
data Batch[T]:
    values: List[T]

@derive(OwnOnly)
data Writer[^a]:
    file: ^{a} mut File                                 # error: own-field-plain-type

@derive(OwnOnly)
data Tracked[T]:
    value: T

impl[T < own Any] DropOwn for Tracked[T]:              # error: missing-supertrait-implementation
    fn drop(^mut self) -> void: pass

data Pair:
    both: (own Conn, i32)                              # error: own-field-plain-type

fn keep(b: own Box[own Conn]) -> void: pass            # error: own-in-plain-type
fn index(m: ^Map[own Conn, i32]) -> void: pass         # error: own-in-plain-type
fn accept(batch: Batch[own Conn]) -> void: pass        # error: own-type-plain
fn many(xs: List[own Conn]) -> void: pass              # error: own-type-plain
```

> **Why.** A container is owned exactly when it holds an owned value, so
> `Batch[i32]` keeps plain copies while `Batch[own Conn]` moves. Reading
> only stored values keeps shared views, unused arguments and factories
> plain. A `^mut` field asks for a written impl, so a holder of an
> exclusive borrow is always owned on purpose.

See also: [Ownership Traits](#ownership-traits).

## Modes In Bodies

Inside a body, a value meets a position's mode where it is passed,
returned, bound, or stored. An owned value stays owned, a value is lent to
a borrowed position, and a plain value stays plain.

```text
data File:
    fd: i32

impl OwnOnly for File

impl File:
    fn size(^self) -> i32: self.fd
    fn reset(^mut self) -> void:
        self.fd = 0

data Buffer:
    bytes: List[u8]
    label: string

fn open(path: string) -> own File: File { fd: 3 }   # a fresh value is owned
fn size(f: ^File) -> i32: f.fd
fn archive(f: own File) -> void: pass
fn fill(b: ^mut Buffer) -> void:
    b.label = "full"

fn report() -> i32:
    f := open("app.log")              # `:=` keeps `own`
    n := size(f)                      # lends `f`
    f.reset()                         # lends `f` mutably
    archive(f)                        # gives `f` away
    n

fn buffers() -> string:
    let own buf = Buffer { bytes: [], label: "new" }
    fill(buf)
    let plain: mut Buffer = Buffer { bytes: [1], label: "old" }
    fill(plain)                       # a plain place with a `mut` view lends mutably
    let view: ^Buffer = plain
    view.label
```

### Owned Values

1. r[own.body.source] An owned value comes from a fresh value, an `own` result, parameter or local, or an `own` field.
2. r[own.body.fresh] A **fresh** value is a data literal, a variant construction, a list, map or tuple literal, or a closure, whose every part is fresh or exempt. A primitive, an owned or borrowed value, and an optional or tuple of these are exempt.
3. r[own.body.upgrade] A plain value where `own T` is expected must be fresh, and becomes owned there; any other plain value is an error, and so is a borrow. Error: `own-upgrade`.
4. r[own.body.owned-to-plain] An owned value where a plain type is expected is an error, a plain field of a fresh value included, unless it moves into a part of an owned value ([own.move.part](#r-own.move.part)). Error: `own-to-plain`.
5. r[own.body.let-own] `let own x = value` binds an owned local, so the value is owned or fresh, and `let own x: T = value` is `let x: own T = value`. Error: `own-on-value` for a primitive.
6. r[own.body.infer] `:=`, and `let` without a type, keep an owned initializer owned; a fresh initializer stays plain.
7. r[own.body.store-fresh] A store into a plain field of an owned value takes a fresh or exempt value. The base is owned when its type is `own T`, or a `^mut` borrow of a type that owns. Error: `own-upgrade`.
8. r[own.body.result-written] A function whose result is owned or borrowed writes its result type. Error: `missing-result-type`.

### Lending

1. r[own.body.lend] A value where `^T` or `^mut T` is expected is lent to that position: an owned value, a plain value, or a borrow. No expression lends.
2. r[own.body.mutable-upgrade] `^mut` lends an owned value, a plain value with a `mut` view, or a `^mut` borrow; a readonly plain value and a shared borrow are errors. Error: `mutable-upgrade`.
3. r[own.body.receiver] A receiver follows the rules of a parameter: `^self` and `^mut self` lend it, `own self` takes an owned or fresh value, and `self` or `mut self` takes a plain value.
4. r[own.body.field-read] An owned value and a `^mut` borrow give mutable access to their fields, and a shared borrow gives readonly access. A field read through a borrow is borrowed for the same lifetimes, mutably only through `^mut` and for a `mut` or `own` field. A primitive field is copied out, and a borrowed field is read as it is.

```text
data File:
    fd: i32

impl File:
    fn close(own self) -> void: pass
    fn plain(self) -> i32: self.fd

data Buffer:
    bytes: List[u8]

fn open() -> own File: File { fd: 3 }
fn archive(f: own File) -> void: pass
fn count(f: File) -> i32: f.fd
fn reset(f: ^mut File) -> void: pass

fn wrong(f: File, b: ^File, xs: List[u8], own_buf: own Buffer) -> void:
    archive(f)                              # error: own-upgrade
    archive(b)                              # error: own-upgrade
    let own buf = Buffer { bytes: xs }      # error: own-upgrade
    own_buf.bytes = xs                      # error: own-upgrade
    f.close()                               # error: own-upgrade
    reset(f)                                # error: mutable-upgrade
    reset(b)                                # error: mutable-upgrade
    count(open())                           # error: own-to-plain
    open().plain()                          # error: own-to-plain
    let own n = 3                           # error: own-on-value

fn helper(): open()                         # error: missing-result-type
```

> **Why.** A plain value may already have aliases, so only a fresh value
> becomes owned: it has no other path yet. Lending marks nothing at the
> call site, so a call reads the same whether it borrows or not.

See also: [Ownership By Structure](#ownership-by-structure).

## Moves

An owned local holds its value until a use gives it away. A use that some
path through the body reaches after such a move is an error, so an owned
value is never used once its owner has given it up.

```text
data File:
    fd: i32

impl OwnOnly for File

fn open(path: string) -> own File: File { fd: 3 }
fn size(f: ^File) -> i32: f.fd
fn archive(f: own File) -> void: pass

fn report(again: bool) -> i32:
    let f = open("app.log")
    n := size(f)                      # lends `f`, which keeps its value
    archive(f)                        # moves `f`
    if again:
        f = open("app.log")           # gives `f` a value again
        return n + size(f)
    n

fn rotate(keep: bool) -> void:
    f := open("tmp.log")
    if keep:
        archive(f)                    # moves `f` on this path only
```

1. r[own.move.local] An **owned local** is a local or a parameter whose type holds an owned value outside any borrow: an `own` type, or an optional, tuple, or type argument that holds one.
2. r[own.move.use] A use of an owned local moves it, except where it is lent to a borrowed position ([own.body.lend](#r-own.body.lend)) or a field of it is read, assigned, or lent. So passing it to an owned position, returning it, storing it, and binding it move it.
3. r[own.move.after] A use of an owned local that a path from a move of it reaches, with no assignment to it in between, is an error. Error: `use-after-move`.
4. r[own.move.paths] The paths follow the body's control flow, into each branch of an `if` and each arm of a `match`. The end of a loop body and a `continue` lead to the loop's next pass, and a `break` to the code after its loop. A `?` that succeeds leads to the code after it. A `return`, `break`, `continue`, or failing `?` leaves the code after it, and so does a value of type `never`.
5. r[own.move.reinit] An assignment to an owned local gives it a value again, so a use after the assignment is not a use after a move.
6. r[own.move.partial] A field or tuple element of an owned value does not move out on its own when its type holds an owned value. Using such a part as a value is an error; lending it or reading a field of it is not. Error: `partial-move`.
7. r[own.move.borrow] A value read through a borrow is borrowed ([own.body.field-read](#r-own.body.field-read)), so it never moves, and giving it to an owned position is an error. Error: `own-upgrade`.
8. r[own.move.defer] A `defer` moves every owned local its suite reads when the `defer` is reached, whether the suite lends the local or moves it. A later use of the local, the same `defer` reached again by a loop included, is a use after a move. Error: `use-after-move`.
9. r[own.move.part] An owned value given to a plain field or payload of a construction moves there when that construction, or one that holds it through plain fields and payloads, becomes owned ([own.body.upgrade](#r-own.body.upgrade)). An owned value stored into a plain field of an owned value ([own.body.store-fresh](#r-own.body.store-fresh)) moves there too. Otherwise it is an error. Error: `own-to-plain`.

```text
data File:
    fd: i32

impl OwnOnly for File

data Holder:
    file: own File
    count: i32

impl OwnOnly for Holder

fn open(path: string) -> own File: File { fd: 3 }
fn size(f: ^File) -> i32: f.fd
fn archive(f: own File) -> void: pass

fn twice() -> void:
    f := open("a.log")
    archive(f)
    archive(f)                        # error: use-after-move

fn each(more: bool) -> void:
    f := open("b.log")
    while more:
        archive(f)                    # error: use-after-move

fn cleanup() -> void:
    file := open("c.log")
    defer: size(file)
    _ := size(file)                   # error: use-after-move

fn release(h: own Holder) -> void:
    archive(h.file)                   # error: partial-move

fn steal(h: ^Holder) -> void:
    archive(h.file)                   # error: own-upgrade
```

```text
data File:
    fd: i32

impl OwnOnly for File

data Buffer:
    bytes: List[u8]
    label: string

fn open(path: string) -> own File: File { fd: 3 }
fn size(f: ^File) -> i32: f.fd
fn read_bytes() -> own List[u8]: [1, 2]

fn borrowed_cleanup() -> i32:
    file := open("input")
    let view: ^File = file
    defer: size(view)                 # the suite reads a borrow, not the owner
    size(file)

fn load() -> own Buffer:
    xs := read_bytes()
    Buffer { bytes: xs, label: "owned" }   # `xs` moves into the owned buffer
```

> **Why.** An owned value has one owner, so after a move the old name holds
> nothing. Following every path, rather than reading the body top to bottom,
> accepts a move on one branch and rejects only the uses a move can reach. A
> `defer` runs later, so it holds what its suite reads from the moment it is
> reached; a `^` local lends the value to it instead.

See also: [Modes In Bodies](#modes-in-bodies).

## Drops

An owned value is **dropped** when its owner gives it up without moving it:
its type's `drop` runs, then the parts it owns drop.

```text
use std.ownership.drop

let log: mut List[string] = []

data File:
    name: string

impl OwnOnly for File

impl DropOwn for File:
    fn drop(^mut self) -> void:
        log.push("closed ${self.name}")

fn open(name: string) -> own File: File { name: name }
fn size(f: ^File) -> i32: 1
fn archive(f: own File) -> void: pass

fn report(keep: bool) -> void:
    let a = open("a.log")
    b := open("b.log")
    if keep:
        archive(b)                    # `b` moves: this path does not drop it
    _ := size(open("tmp.log"))        # the temporary drops at the end of the line
    a = open("c.log")                 # drops "a.log" first
    drop(a)                           # drops "c.log" now
    log.push("end")                   # then `b` drops here, unless it moved
```

### When A Value Drops

1. r[own.drop.scope] An owned local that holds its value when its scope ends is dropped there. A parameter's scope is its function's body.
2. r[own.drop.exits] A scope ends at its normal end, and at each `return`, `break`, `continue`, or failing `?` that leaves it.
3. r[own.drop.moved] A local that moved does not drop. When it moved on some paths only, it drops on the other paths, so its value drops exactly once.
4. r[own.drop.order] At a scope's end, its owned locals drop and its registered `defer` suites run in reverse order of declaration and registration. A body's parameters drop after its locals, the last parameter first.
5. r[own.drop.assign] An assignment to an owned local that holds its value drops the old value. The new value is computed first, and the old value drops before the store.
6. r[own.drop.assign-field] An assignment to a field whose type owns, through an owned value or a `^mut` borrow, drops the field's old value before the store.
7. r[own.drop.temporary] An owned value that its statement makes, and that no binding, owned position, store, or result takes, is a **temporary**. It drops at the end of its statement, the newest temporary first.
8. r[own.drop.condition] The condition of an `if`, `else if`, or `while` counts as a statement of its own, so its temporaries drop before a branch runs.
9. r[own.drop.defer] A `defer` suite holds the owned locals it takes when it registers ([own.move.defer](#r-own.move.defer)). What it holds and does not move drops right after the suite runs, the newest local first.
10. r[own.drop.generic] A local whose type holds a value of a type parameter with an `own` bound, outside any borrow, drops as an owned local does when its type argument owns.
11. r[own.drop.panic] A panic runs no `drop`.

### What A Drop Runs

1. r[own.drop.run] Dropping a value of a type that implements `DropOwn` first calls its `drop` on the value.
2. r[own.drop.parts] Then the value's parts drop in order: a data type's fields in declaration order, the payloads of the variant an enum value holds, an optional's payload, and a tuple's elements.
3. r[own.drop.elements] A list's elements drop in index order, and a map's values in its iteration order.
4. r[own.drop.nothing] A value whose type has no `drop` and owns no part drops nothing. A borrow never drops the value it refers to.

### Ending A Value Early

1. r[own.drop.call] `std.ownership.drop(value)` takes its argument by move and drops it. Its type parameter has an `own` bound, so a plain argument is allowed and drops nothing.
2. r[own.drop.direct] A call or reference of a type's `drop` method is an error; it runs only when a value drops. Error: `direct-drop-call`.

```text
data File:
    fd: i32

impl OwnOnly for File

impl DropOwn for File:
    fn drop(^mut self) -> void: pass

fn open() -> own File: File { fd: 3 }

fn close_twice() -> void:
    f := open()
    f.drop()                          # error: direct-drop-call
```

> **Why.** The compiler knows where each owner gives its value up, so a
> destructor runs there, once, with no `close` protocol to forget. A flag
> decides at run time only where a move happens on some paths, as in Rust.
> Calling `drop` by hand would run it a second time at the scope's end.

See also: [Moves](#moves), [Ownership Traits](#ownership-traits).

## Prototype Limits

Some forms of owned and borrowed values wait for later sections of this
chapter. A program that uses one is rejected with a diagnostic of its own.

```text
use std.task.{all, race}

data Settings:
    level: i32

data Conn:
    fd: i32

impl OwnOnly for Conn

fn open(level: i32) -> own Conn: Conn { fd: level }
fn connect!(addr: string) -> own Conn: Conn { fd: 1 }
fn handle!(c: own Conn) -> void: pass
fn level(s: ^Settings) -> i32: s.level

CONFIG := Settings { level: 1 }
MAIN := open(1)                           # error: not-in-ownership-prototype
let VIEW: ^Settings = CONFIG              # error: not-in-ownership-prototype

fn current() -> i32: level(CONFIG)        # error: not-in-ownership-prototype

fn later(c: own Conn) -> void:
    _ := handle(c)                        # error: not-in-ownership-prototype

fn both!(a: string, b: string) -> void:
    _ := all!(connect(a), connect(b))     # error: not-in-ownership-prototype

fn first!(a: string, b: string) -> void:
    _ := race!(connect(a), connect(b))    # error: not-in-ownership-prototype
```

1. r[own.limit.top-level] A top-level binding whose type has an owned or borrowed part, at any depth, is an error, and so is a top-level `let own`. Error: `not-in-ownership-prototype`.
2. r[own.limit.top-level-borrow] A top-level binding, or a field of one, lent to a borrowed position is an error, receivers included. Error: `not-in-ownership-prototype`.
3. r[own.limit.cold-call] A plain call of a suspending function, which builds a cold suspension, is an error when the type of one of its arguments owns by [own.structure.owns](#r-own.structure.owns). Error: `not-in-ownership-prototype`.
4. r[own.limit.child-result] A child of `all!` or `race!` whose result type owns is an error. Error: `not-in-ownership-prototype`.

> **Why.** A top-level owner moves in statement order and drops when the
> instance ends, and a borrowed global must not move. A cold suspension
> and a combinator's buffer drop the owned values they hold when they are
> cancelled. Each needs drops that later sections specify.

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
    fn keep(self, token: own Token) -> Token  # error: not-boundary-safe
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

1. r[own.provider.plain] A provider of `$.with` or `$.context` is a plain value. A provider whose type has an owned or borrowed part or a lifetime argument, at any depth, is an error. Error: `provider-not-plain`.
2. r[own.boundary.not-safe] A type with an owned or borrowed part or a lifetime argument, at any depth, is not boundary-safe.
3. r[own.boundary.signature] A boundary signature with such a type is an error, reported at that signature. A host capability method's receiver does not cross, so its mark is allowed. Error: `not-boundary-safe`.

> **Why.** Sibling tasks share a provider with object semantics, so an
> owned or borrowed provider would give two tasks one owner or one
> exclusive borrow. A value that crosses a boundary is copied as a tree, so
> it can neither stay the only path to its value nor point back into hd.

See also: [Boundary-Safe Values](10-modules.md#boundary-safe-values),
[Mutable Providers](11-requirements-and-suspension.md#mutable-providers).
