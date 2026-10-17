# Ownership and Borrows

Status: language specification draft.

This chapter defines the modes in which a position holds its value: plain,
owned, or borrowed.

> **Note.** The chapter grows section by section with the ownership
> prototype (issue #163), and each section arrives with the conformance cases
> that test it. Until a section specifies how a mode is checked, an
> implementation diagnoses a program that uses it, as
> [Specification Status](../README.md#specification-status) requires.

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
2. r[own.lifetime.name] A lifetime is written as a variable such as a parameter, `self`, a path such as `items.T`, a requirement key such as `$Store`, or `static`.

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

1. r[own.lifetime-param.declare] A data type, an enum, a trait, a `type` declaration, or an implementation declares a lifetime parameter as `^` and a name, as in `data Parser[^doc]`.
2. r[own.lifetime-param.last] Lifetime parameters follow every type parameter. A type parameter after a lifetime parameter is an error. Error: `syntax-error`.
3. r[own.lifetime-param.plain] A lifetime parameter takes no variance marker, bound, or default. Error: `syntax-error`.
4. r[own.lifetime-param.function] A function's generic parameters include no lifetime parameter, since a function names lifetimes by its parameters. Error: `syntax-error`.
5. r[own.lifetime-arg.write] A use writes a lifetime argument as `^` and a lifetime set, or as a bare `^` that elides it, as in `Parser[^{text}]` and `Parser[^]`.
6. r[own.lifetime-arg.last] Lifetime arguments follow every positional type argument and come before the bindings. Error: `syntax-error`.
7. r[own.lifetime-arg.vs-borrow] In a type-argument list, `^` or `^` and a lifetime set is a lifetime argument when `,` or `]` follows it. When a type follows, it is a borrowed type argument, as in `Suspend[^{keep} Conn]`.
8. r[own.lifetime-arg.types-only] An expression's type-argument list, as in `f::[T]` or `Parser::[T] { ... }`, takes no lifetime argument. Error: `syntax-error`.

```text
data Conn:
    fd: i32

data Pair[^a, A]:                     # error: syntax-error
    left: ^{a} A

fn pick[^a](c: ^Conn) -> ^{a} Conn: c  # error: syntax-error
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
