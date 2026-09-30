# Call Indexing: `list(0)` Instead Of `list[0]`

Status: design exploration, 2026-09-30; nothing here is decided or in the
specification. It is a brainstorm with a stress test, and it changes no
decision, spec text, or prototype code. Under review:
[Indexing](../spec/05-expressions.md#indexing),
[Index Traits](../spec/05-expressions.md#index-traits),
[Places](../spec/05-expressions.md#places),
[Compound Assignment](../spec/05-expressions.md#compound-assignment),
[Calls](../spec/05-expressions.md#calls),
[Member Resolution](../spec/03-names-and-scopes.md#member-resolution),
[Primary Expressions](../spec/02-grammar.md#primary-expressions), and
[Explicit Type Arguments](../spec/07-functions.md#explicit-type-arguments).

The owner wrote: "i hate syntax ambiguity. i have one crazy thing"

```text
item := list(0)
list(0) = item + 1

var := live(0)
println(var())
var() = 1
```

## Contents

- [Problem](#problem)
- [What hd Has Today](#what-hd-has-today)
- [Use Cases](#use-cases)
- [Survey](#survey)
- [Options](#options)
- [Stress Test](#stress-test)
- [Comparison](#comparison)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

## Problem

hd writes both type arguments and indexing with `[]` after an operand. So
`first[string](names)` and `handlers[i](event)` have the same shape, and
only name resolution tells them apart. The owner wants every form to have
one reading that a parser can see.

The owner's idea reads indexing as a call, as Scala's `apply` and `update`
do. Then `[]` after an operand always means type arguments, and `[` in
operand position always starts a list literal. `f(args) = v` is an update,
and a zero-argument apply gives cells and signals as ordinary library types.

This record asks three things:

| # | Question | Section |
| --- | --- | --- |
| 1 | What breaks when real hd code moves to call indexing? | [Stress Test](#stress-test) |
| 2 | What does `user.tags(0)` mean, when fields and methods are separate namespaces? | [The Field Conflict](#the-field-conflict) |
| 3 | Is there a cheaper way to remove the ambiguity? | [Options](#options) |

The question is core syntax, not an advanced feature. Signals built on the
zero-argument form are library design, and they wait for the core answer.

## What hd Has Today

### The Ambiguity

| Rule | Text, shortened |
| --- | --- |
| [`grammar.primary.generic-reference`](../spec/02-grammar.md#r-grammar.primary.generic-reference) | Name resolution tells `first[string](names)` from indexing. |
| [`grammar.primary.preserve-ambiguity`](../spec/02-grammar.md#r-grammar.primary.preserve-ambiguity) | A parser may keep the ambiguity until name resolution. |
| [`grammar.expr.method-type-arguments`](../spec/02-grammar.md#r-grammar.expr.method-type-arguments) | After member resolution, brackets after a generic method name are type arguments. |
| [`grammar.primary.member-type-arguments.rules`](../spec/02-grammar.md#r-grammar.primary.member-type-arguments.rules) | Such a bracket is valid only when the selected member is generic. |
| [`fn.generic.brackets`](../spec/07-functions.md#r-fn.generic.brackets) | Name resolution tells the brackets from an indexing operation. |
| [`expr.pipe.bare.needs-placeholder.forms`](../spec/05-expressions.md#r-expr.pipe.bare.needs-placeholder.forms) | `x \|> f[0]` needs `_`, because brackets could index or instantiate. |

The postfix grammar holds both readings: `postfix_suffix` has `"[",
expression, "]"`, and a member suffix may take `function_type_arguments`.

### Indexing

| Area | Rules | Summary |
| --- | --- | --- |
| [Indexing](../spec/05-expressions.md#indexing) | `expr.index.order`, 5 list rules, 5 map rules, 6 string rules | Built-in `List`, `Map`, and `string` indexing. `m[k]` reads `V?`. |
| [Index Traits](../spec/05-expressions.md#index-traits) | 10 `expr.index.trait.*` rules | `Index[K]` with `Out`, and `IndexSet[K, V]` with a `mut self` store. |
| [Built-In Implementations](../spec/05-expressions.md#built-in-implementations) | 7 `expr.index.std.*` rules | Intrinsic bodies; `Map`'s `index` returns `V` and panics. |
| [Places](../spec/05-expressions.md#places) | `expr.place.index`, `expr.place.receiver` | An index is a place; its receiver may be a call. |
| [Compound Assignment](../spec/05-expressions.md#compound-assignment) | `expr.assign.compound.place`, `.once`, `.index-read-write`, `.map-present`, `.map-missing` | `m[k] += 1` reads `V` and panics on a missing key. |

That is 34 `expr.index.*` rules, plus about 10 place and assignment rules
that name indexing. [Special Cases](SPECIAL_CASES.md#c9-one-meaning-for-map-indexing)
already lists the split `Map` read as cut C9, question Q9, still open.

### Fields, Methods, And Calls

| Rule | Text, shortened |
| --- | --- |
| [`names.member.namespaces`](../spec/03-names-and-scopes.md#r-names.member.namespaces) | Each nominal type has a field namespace and a method namespace. |
| [`names.member.shared-name`](../spec/03-names-and-scopes.md#r-names.member.shared-name) | A field and a method may share a name. |
| [`names.lookup.method-form`](../spec/03-names-and-scopes.md#r-names.lookup.method-form) | `x.name(args)` uses method lookup only. It never selects a field. |
| [`names.lookup.stored-fn`](../spec/03-names-and-scopes.md#r-names.lookup.stored-fn) | A function in a field is called as `(x.name)(args)`. |
| [`expr.call.callable`](../spec/05-expressions.md#r-expr.call.callable) | A callee may be any expression of function type. |
| [`fn.type.ctor.inputs`](../spec/07-functions.md#r-fn.type.ctor.inputs) | `Fn[(A, B), O, R]` takes its inputs as one tuple type. |

Calls already have several readings by callee: a function, a closure, a
variant constructor, a newtype constructor such as `Meters(5)`, and a
conversion such as `i64(x)`. The error for anything else is `not-callable`.

## Use Cases

Every option is shown against the same six cases.

| # | Case | Today |
| --- | --- | --- |
| U1 | Read and write a list element | `item := list[0]`, `list[0] = item + 1` |
| U2 | Count words in a map | `counts[word] += 1` |
| U3 | Index a collection held in a field | `t.raw_parts[0]`, `cart.items[0].quantity = 0` |
| U4 | Explicit type arguments on a call | `first[string](names)`, `parser.parse[User](text)` |
| U5 | A two-key grid | needs a tuple key: `grid[(0, 1)]` |
| U6 | A zero-key cell | not expressible with `[]`; a method such as `cell.get()` |

## Survey

| Language | Index syntax | Type arguments | Fields vs methods | Source |
| --- | --- | --- | --- | --- |
| Scala 2 and 3 | `xs(0)` is `xs.apply(0)`; `xs(0) = v` is `xs.update(0, v)` | `f[A](x)` | One term namespace: a `val` and a `def` compete, so `o.tags(0)` reads the `val` and applies it | [Scala spec ch. 6](https://www.scala-lang.org/files/archive/spec/2.13/06-expressions.html), [ch. 2](https://www.scala-lang.org/files/archive/spec/2.13/02-identifiers-names-and-scopes.html) |
| Ada | `A(I)` for arrays and calls; Ada 2012 adds user-defined indexing | not written at calls | A prefixed view `X.Op` is illegal when a visible component has that name, so the component wins | [Ada RM 4.1.3](https://www.adaic.org/resources/add_content/standards/12rm/html/RM-4-1-3.html), [4.1.6](https://www.adaic.org/resources/add_content/standards/12rm/html/RM-4-1-6.html) |
| Fortran | `a(i)` for elements and `f(i)` for calls; declarations decide | none | no methods on arrays | [Fortran 2018, 9.5.3 and 15.5](https://j3-fortran.org/doc/year/18/18-007r1.pdf) |
| MATLAB | `a(i)`; `subsref` and `subsasgn` overload it | none | A variable hides a function of the same name | [Function precedence order](https://www.mathworks.com/help/matlab/matlab_prog/function-precedence-order.html) |
| Kotlin | `a[i]` is `a.get(i)`; `a[i] = b` is `a.set(i, b)`; `a(i)` is `a.invoke(i)` | `f<A>(x)` | "Functions before properties": a method wins, then a property with `invoke` | [Operator overloading](https://kotlinlang.org/docs/operator-overloading.html), [Overload resolution](https://kotlinlang.org/spec/overload-resolution.html) |
| Swift | `subscript` with any number of parameters; `callAsFunction` makes values callable | `f<A>` on types only | A property of function type is called as `o.f(x)` | [Subscripts](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/subscripts/), [SE-0253](https://github.com/apple/swift-evolution/blob/main/proposals/0253-callable.md) |
| Go | `a[i]` | `f[int](x)`, the same shape | Fields and methods share one selector namespace | [Index expressions](https://go.dev/ref/spec#Index_expressions), [Instantiations](https://go.dev/ref/spec#Instantiations), [go/ast IndexListExpr](https://pkg.go.dev/go/ast#IndexListExpr) |
| Rust | `a[i]` is `*a.index(i)` | turbofish `f::<A>(x)` in expressions | Separate namespaces; a stored closure is called as `(s.f)(x)` | [Index](https://doc.rust-lang.org/std/ops/trait.Index.html), [Paths](https://doc.rust-lang.org/reference/paths.html#paths-in-expressions), [Field expressions](https://doc.rust-lang.org/reference/expressions/field-expr.html) |
| Nim | `a[i]` | `p[T](x)`; `x.p[:T]` when the call uses dot syntax | uniform call syntax | [Nim manual, method call syntax](https://nim-lang.org/docs/manual.html) |
| OCaml and F# | OCaml `a.(i)`; F# moved from `a.[i]` to `a[i]` in F# 6 | `'a` inferred | records and methods differ | [F# 6](https://learn.microsoft.com/en-us/dotnet/fsharp/whats-new/fsharp-6) |
| Java | `a[i]` for arrays only; collections use `get(i)` and `set(i, v)` | `obj.<A>f(x)` | separate | [List](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/util/List.html) |

Notes on the table:
- Go's parser builds one `IndexExpr` or `IndexListExpr` node for both
  `a[i]` and `f[T]`, and the type checker decides, as hd does today.
- Nim's manual says `x.p[T]` "is always parsed as `(x.p)[T]`", so it added
  `[:` for dot calls. That is a second bracket form for one idea.
- F# gave up the distinct `a.[i]` form in F# 6. Its reason was learner
  feedback that dot indexing is "an unnecessary divergence from standard
  industry practice".

**Takeaways.**
1. Call indexing is proven: Scala, Ada, Fortran, and MATLAB ship it. For
   `o.name(args)`, Scala merges fields and methods, Ada picks the field,
   and Kotlin, for `invoke`, picks the method. Rust keeps hd's two
   namespaces and its `(s.f)(x)`, but Rust indexes with `[]`.
2. Languages that keep `[]` for both uses resolve the clash either by name
   (Go) or by a second marker (Rust's `::<>`, Nim's `[:`).
3. Moving away from the familiar `a[i]` has a real cost in learners' eyes,
   as F# found.

## Options

Four options, ranked later by the
[Design Cost Order](../AGENTS.md#design-cost-order). Option C has three
variants for the field conflict.

### Option A: Keep Today's Rule

`[]` stays for indexing and type arguments, and name resolution decides.
This is Go's choice. Nothing changes.

```text
fn first[T](items: List[T]) -> T: items[0]

fn demo(names: List[string], counts: mut Map[string, i32]) -> string:
    counts["ada"] += 1
    first[string](names)
```

- **Rules:** none added or removed.
- **Cost:** the parser keeps the ambiguity until name resolution, which is
  what the owner dislikes.
- **U5, U6:** a grid needs a tuple key, and a cell needs a method.

### Option B: Mark Type Arguments In Expressions

`[]` after an operand is always indexing. Type arguments in an expression
take a marker, `::[`, as Rust's turbofish and Nim's `[:` do. Types keep
`List[i32]`.

```text
fn first[T](items: List[T]) -> T: items[0]

fn demo(names: List[string], parser: Parser) -> User:
    head := first::[string](names)  # hypothetical syntax
    parser.parse::[User](head)  # hypothetical syntax
```

- **Rules:** one grammar change, `"::", function_type_arguments` in
  expressions; about 15 rules reworded; the 5 ambiguity rules removed.
- **Sites:** 124 explicit type-argument sites change. Up to about 100
  more change if type arguments after a type name, as in
  `Shipment[i32] { ... }` or `Box[i32]::get`, move too.
- **Cost:** two spellings of one type-argument list, `List[i32]` in types
  and `f::[i32]` in expressions. A new token pair.

### Option C: Call Indexing (The Owner's Idea)

`r(k)` reads through an `Apply` trait and `r(k) = v` stores through an
`Update` trait. `[]` after an operand is always type arguments.

```text
fn first[T](items: List[T]) -> T: items(0)

fn demo(names: mut List[string], counts: mut Map[string, i32]) -> string:
    item := names(0)
    names(0) = item + "!"
    counts("ada") += 1
    first[string](names)
```

This parses with today's grammar, since `postfix_expression =` already
accepts a call on the left. Only the meaning is new.

#### Grammar

| Change | Rules |
| --- | --- |
| Remove the suffix `"[", expression, "]"` | `postfix_suffix` |
| `[` after an operand is always `function_type_arguments` | removes `grammar.primary.generic-reference`, `.preserve-ambiguity`, `fn.generic.brackets`; simplifies `grammar.expr.method-type-arguments` and `grammar.primary.member-type-arguments.rules` |
| `x[a + b]` is a `syntax-error`, with a fix-it to `x(a + b)` | new diagnostic text, no new code |

The parser becomes context-free for brackets: after an identifier, a
bracket is type arguments; after `)`, `]`, or a literal, it is a syntax
error; in operand position, it starts a list literal.

#### Call Resolution By Type

A call `c(args)` is resolved by what `c` is. The cases are disjoint.

| Callee | Meaning | Today |
| --- | --- | --- |
| a name of a function, variant, or type | call, construction, or conversion | unchanged |
| a value of function type (`Fn`, `SuspendFn`) | call | unchanged |
| a value whose type implements `Apply[Keys]` for the argument tuple | index read | new |
| a type parameter bounded by `Apply[Keys]` | index read | new |
| anything else | error `not-callable` | unchanged |

- **Unambiguous:** a function type is a `std.function` constructor, which
  implements no `Apply`, so no value is both.
- **Several `Apply` impls:** they are chosen by the argument types, as
  [`expr.index.trait.choice`](../spec/05-expressions.md#r-expr.index.trait.choice)
  chooses today.
- **Named arguments and spreads:** `m(key=k)` and `m(ks...)` are errors,
  as for function values
  ([`fn.type.positional`](../spec/07-functions.md#r-fn.type.positional)).

#### Traits At Any Arity

The keys argument is tuple-kinded, like `Fn`'s inputs
([`fn.type.ctor.input-kind`](../spec/07-functions.md#r-fn.type.ctor.input-kind)).
`r(a, b)` passes the tuple `(a, b)`, `r(a)` passes `(a,)`, and `r()`
passes `()`. A tuple is never flattened, so a map keyed by pairs is read
as `m((a, b))`.

```text
pub trait Apply[Keys]:
    type Out
    fn apply(self, keys: Keys) -> Self::Out

pub trait Update[Keys, V]:
    fn update(mut self, keys: Keys, value: V) -> void
```

A two-key grid:

```text
use std.ops.{Apply, Update}

data Grid:
    width: i32
    cells: mut List[f64]

impl Apply[(i32, i32)] for Grid:
    type Out = f64
    fn apply(self, keys: (i32, i32)) -> f64:
        (row, col) := keys
        (self.cells)(row * self.width + col)

impl Update[(i32, i32), f64] for Grid:
    fn update(mut self, keys: (i32, i32), value: f64) -> void:
        (row, col) := keys
        (self.cells)(row * self.width + col) = value

fn bump(grid: mut Grid) -> void:
    grid(0, 1) = grid(0, 0) + 1.0
    grid(1, 1) += 2.0
```

A zero-key cell, the owner's `live(0)`:

```text
use std.ops.{Apply, Update}

data Cell[T]:
    value: T

impl[T] Apply[()] for Cell[T]:
    type Out = T
    fn apply(self, keys: ()) -> T: self.value

impl[T] Update[(), T] for Cell[T]:
    fn update(mut self, keys: (), value: T) -> void:
        self.value = value

fn live[T](value: T) -> mut Cell[T]:
    Cell { value: value }

fn demo() -> void:
    var := live(0)
    println(var())
    var() = 1
    var() += 1
```

`Cell` here is plain data. A signal that tracks readers is library design
on top, and it waits, as the [Problem](#problem) says.
[Standard Library Design](STDLIB.md#module-tree) records that `std.cell` was
removed, so a cell type is a new std item in any case.

A map, where `m(k)` reads `V` and panics on a missing key, and `get` gives
`V?`:

```text
fn tally(counts: mut Map[string, i32], word: string) -> i32:
    counts(word) += 1
    match counts.get(word):
        .Some(n) => n
        .None => 0
```

| Type | `Apply` | `Update` |
| --- | --- | --- |
| `List[T]` | `Apply[(i32,)]`, `Out = T`, panics out of range | `Update[(i32,), T]` |
| `Map[K, V]` | `Apply[(K,)]`, `Out = V` or `V?`: see [Q3](#q3-map-read-type) | `Update[(K,), V]`, inserts or replaces |
| `string` | `Apply[(i32,)]`, `Out = u8` | none |

**Compound assignment.** `r(ks) op= v` evaluates `r` and the keys once,
then reads `r(ks)`, applies `op`, and stores with `update`. It is today's
[`expr.assign.compound.index-read-write`](../spec/05-expressions.md#r-expr.assign.compound.index-read-write)
with `()` for `[]`.

```text
fn bump(counts: mut Map[string, i32], word: string) -> void:
    counts(word) += 1
    # means, with `counts` and `word` evaluated once:
    counts(word) = counts(word) + 1
```

**Permissions.** `update` takes `mut self`, so `r(ks) = v` needs mutable
access to `r`, as [`expr.index.trait.mut`](../spec/05-expressions.md#r-expr.index.trait.mut)
says today. A read needs none. `Out` carries the element permission, as
today's `Index` note says.

**Bounds.** A bound spells the tuple: `C < Apply[(i32,)]`, where today it
is `C < Index[i32]`. That is longer for the common one-key case.

#### Places And Errors

| Left side | Meaning |
| --- | --- |
| `r(ks) = v`, where `r`'s type implements `Update[Keys, V]` | a store |
| `r(ks) = v`, where `r` has `Apply` but no `Update` | error `invalid-assignment-target` |
| `f(x) = v`, where `f` is a function or function value | error `invalid-assignment-target` |
| `r(ks).field = v` | a field store through the result of a read, as `users[0].name = "b"` is today |

```text
fn current() -> i32: 1

fn invalid(scale: fn(i32) -> i32, text: string) -> void:
    current() = 2  # error: invalid-assignment-target
    scale(1) = 2   # error: invalid-assignment-target
    text(0) = 65   # error: invalid-assignment-target
```

Today's [`expr.assign.compound.place`](../spec/05-expressions.md#r-expr.assign.compound.place)
says "a call" is not a place. Under C, some calls are places, decided by
the callee's type. That is one reworded rule and the one real semantic
exception in the option.

#### Interactions

| Feature | Effect | Rule change |
| --- | --- | --- |
| `!` | `list!(0)` is `not-suspending`: `apply` and `update` are declared without `!` | none |
| Requirement rows | `apply` has no row, so an index never asks for a provider | none |
| Pipes | `i \|> names` is the bare step `names(i)`, so it indexes | none, or one exception to forbid it |
| Method references | `names::apply` is a bound reference of type `fn((i32,)) -> T`, not `fn(i32) -> T` | none; `fn(i): names(i)` is the usual adapter |
| Trailing blocks | `m(k):` with a block adds a closure key, which no `Apply` accepts: an arity error | none, or `trailing-block-position` |
| `[]` literals | `[1, 2, 3](0)` indexes a literal; `[` in operand position is always a literal | none |
| Evaluation order | receiver, keys left to right, then the value, as today | reworded |

**Does `x()` imply running code?** Not today. `Meters(5)`, `i64(x)`, and
`.Some(x)` construct or convert, and a user `Index` impl already runs code
behind `x[0]`. Suspension, the effect hd marks, keeps its `!`. What a
reader loses is a visual cue that `x(0)` is "only" an element read.

#### The Field Conflict

`x.name(args)` uses method lookup only, so `user.tags(0)` is a call of
a method `tags`, never "field `tags`, element 0".

```text
data User:
    tags: List[string]

fn first_tag(user: User) -> string:
    user.tags(0)  # error: unknown-method
```

**C1: parentheses.** Write `(user.tags)(0)`, as a stored function is
called today. No rule changes.

```text
data User:
    tags: List[string]

fn first_tag(user: User) -> string:
    (user.tags)(0)
```

The `unknown-method` hint of
[`expr.member.stored-fn.hint`](../spec/05-expressions.md#r-expr.member.stored-fn.hint)
widens from function fields to applicable fields.

**C2: fall back to the field.** `x.name(args)` looks for a method first.
When none exists, it reads the field and applies it. Kotlin resolves a
property with `invoke` the same way, after member functions.

```text
data User:
    tags: List[string]

fn first_tag(user: User) -> string:
    user.tags(0)
```

It is name-based resolution again. Adding a method `tags(i: i32)` later
silently changes what every `user.tags(0)` means, and it may still type
check. It also reverses
[`expr.member.stored-fn.error`](../spec/05-expressions.md#r-expr.member.stored-fn.error),
so `button.on_click(41)` becomes valid.

**C3: one member namespace.** A field and a method may no longer share a
name, as in Scala. `x.name(args)` finds the one member: a method is called,
and a field is read and applied.

```text
data User:
    tags: List[string]

impl User:
    fn tags(self) -> List[string]:  # error under C3: a field has this name
        self.tags
```

It removes [`names.member.shared-name`](../spec/03-names-and-scopes.md#r-names.member.shared-name)
and the getter idiom of a private field `len` with a public method `len`.
A trait method named like a field raises a further question: an
implementation in another module could then clash with a field.

**C4: other ideas considered.**

| Idea | Why not |
| --- | --- |
| `user.tags.apply(0)` | Works today with no rule, but it is the long spelling; C1 is shorter. |
| Field first, as Ada | Same action at a distance as C2, in the other direction. |
| Merge only for fields whose type is callable | C3 with a type-dependent rule; changing a field's type changes lookup. |
| A field-apply operator such as `user.tags.(0)` | A new token for 10 sites. |

### Option D: No Index Syntax

Indexing becomes ordinary methods, as Java collections do. `[]` after an
operand is always type arguments, and no call is ever an index.

```text
fn demo(names: mut List[string], counts: mut Map[string, i32]) -> string:
    item := names.at(0)
    names.set(0, item + "!")
    counts.set("ada", counts.at("ada") + 1)
    first[string](names)
```

- **Rules:** about 44 index and place rules deleted, and the `Index` and
  `IndexSet` lang items with them. `at` and `set` join the built-in
  methods. The ambiguity rules go, as in C.
- **Sites:** all 178 index sites become method calls. The 6 compound
  sites lose `op=`.
- **Cost:** numeric and table code reads worse. A cell is
  `cell.get()` and `cell.set(1)`. This is the radical simplification.

## Stress Test

### Method And Counting Rules

The corpus is every valid hd program on `main`, 4173ea46:

| Corpus | Files | Lines |
| --- | --- | --- |
| `text` blocks of [guide/LANGUAGE_TOUR.md](../guide/LANGUAGE_TOUR.md) | 193 blocks | 1,415 |
| `lib/std/*.hd` | 16 | 3,129 |
| fixtures in `runtime/valid`, `runtime/panic`, `typing/valid`, `parse/valid` | 830 | 17,764 |
| `examples/*.hd` | 2 | 33 |

A token script over the reference lexer classified every `[`:
- **Index:** a `[` right after an operand, whose contents are not all type
  names. Its receiver is the token before it.
- **Explicit type arguments:** a `[` after a lowercase name whose contents
  are all types, such as `first[string]` or `x.parse[User]`.
- **Type position:** a `[` inside a parameter, field, result, or `type`
  annotation. These never change.

It is a heuristic. A sample of 17 index sites and every type-argument
site were checked by hand; about 10 type-position sites were counted as
expression sites. The script is not committed.

### Counts

| Site | Tour | std | Fixtures | Total |
| --- | --- | --- | --- | --- |
| Index sites | 9 | 39 | 130 | **178** |
| ...receiver is a local or parameter | 6 | 24 | 110 | 140 |
| ...receiver is `self` | 0 | 11 | 3 | 14 |
| ...receiver is a field, `obj.field[i]` | 1 | 4 | 5 | **10** |
| ...receiver is a call result, `f(x)[0]` | 2 | 0 | 10 | 12 |
| ...receiver is a string literal | 0 | 0 | 2 | 2 |
| Index stores, `r[k] = v` | 0 | 4 | 26 | 30 |
| Index compound stores, `r[k] op= v` | 0 | 0 | 6 | 6 |
| Field stores through an index, `r[k].f = v` | 2 | 0 | 1 | 3 |
| Multi-key indexes, `g[i][j]` | 0 | 0 | 0 | 0 |
| Explicit type arguments on a function | 13 | 0 | 61 | 74 |
| Explicit type arguments on a method | 2 | 3 | 45 | 50 |
| Stored-function calls, `(x.f)(args)` | 0 | 2 | 6 | 8 |
| Types with a field and a method of one name | 0 | 0 | 6 | 6 |
| List literals | 21 | 24 | 300 | 345 |
| Type arguments in type positions | 82 | 172 | 864 | 1,118 |

Findings:
1. Indexing is rare: 178 sites in 22,341 lines, about one in 125 lines.
   Explicit type arguments in calls are almost as common, with 124 sites.
2. Field-then-index is 10 of 178 index sites, 5.6%. Under C1, each gains
   one pair of parentheses.
3. The 6 shared-name types are all fixtures that test
   `names.member.shared-name`. None is in std or the tour, but one,
   `field-and-trait-method-share-name.hd`, has a function-typed field
   beside a same-named method: the exact case C2 and C3 must decide.
4. No code indexes twice in a row, and no code indexes with a tuple key.
   U5 and U6 have no current users.

### Rewrites

`interpolate` in [lib/std/text.hd](../lib/std/text.hd), three field-then-index
sites, under C1:

```text
pub fn interpolate[T < Display](t: Template[T]) -> string:
    let joined = (t.raw_parts)(0)
    let index = 0
    while index < t.values.len():
        joined = joined + (t.values)(index).to_string() + (t.raw_parts)(index + 1)
        index = index + 1
    joined
```

The same under C2 or C3:

```text
pub fn interpolate[T < Display](t: Template[T]) -> string:
    let joined = t.raw_parts(0)
    let index = 0
    while index < t.values.len():
        joined = joined + t.values(index).to_string() + t.raw_parts(index + 1)
        index = index + 1
    joined
```

`Choices.map` in [lib/std/testing.hd](../lib/std/testing.hd): one line
holds a map store and a function call of the same shape.

```text
fn fill[K < Eq & Hash, V](entries: mut Map[K, V], drawn: K, value: fn() -> V) -> void:
    entries(drawn) = value()
```

`List.first` and `last` in [lib/std/collections.hd](../lib/std/collections.hd),
where the receiver is `self`:

```text
fn first[T](items: List[T]) -> T?:
    if items.len() == 0: .None else: .Some(items(0))

fn last[T](items: List[T]) -> T?:
    if items.len() == 0: .None else: .Some(items(items.len() - 1))
```

The tour's permission example, a field store through an indexed element,
under C1:

```text
data LineItem:
    quantity: i32

data Cart:
    items: mut List[mut LineItem]

fn inspect_cart(cart: Cart, item: mut LineItem) -> void:
    (cart.items)(0).quantity = 0
```

A call result indexed, from the tour and `typing/valid/traits.hd`; no
conflict, because the method call ends before the index:

```text
fn domain(email: string) -> string:
    email.split("@")(1)
```

The evaluation-order fixture `assignment-place-before-value.hd`:

```text
fn list_of(log: mut List[string], values: mut List[i32]) -> mut List[i32]:
    log.append("r")
    values

fn note(log: mut List[string], label: string, value: i32) -> i32:
    log.append(label)
    value

fn run(log: mut List[string], values: mut List[i32]) -> void:
    list_of(log, values)(note(log, "i", 1)) = note(log, "v", 7)
```

That last line holds three calls in a row, and only the type of
`list_of(...)` says which one is a place. Today the `[...]` says so.

### Per-Option Site Effects

| Option | Sites rewritten | Sites needing more than a bracket swap | Programs that stop compiling for a new reason |
| --- | --- | --- | --- |
| A | 0 | 0 | 0 |
| B | 124, up to about 220 | 0 | 0 |
| C1 | 178 | 10 gain parentheses | 0 |
| C2 | 178 | 0; 8 stored-function calls may drop parentheses | 0 |
| C3 | 178 | 0 | 6 fixture types that share a field and method name |
| D | 178 | 178 become method calls; 6 compound stores expand | 0 |

## Comparison

Kinds follow the Design Cost Order: 1 is a syntax change, 2 a semantic
rule exception, 3 a compiler intrinsic, and 4 a core library addition.

| | A: keep | B: `::[T]` | C1: call, parens | C2: call, fallback | C3: call, one namespace | D: methods only |
| --- | --- | --- | --- | --- | --- | --- |
| Removes the bracket ambiguity | no | yes | yes | yes | yes | yes |
| Costliest kind of change | none | 1, a new token pair | 2, some calls are places | 2, lookup falls back by name | 2, a namespace merge | 1, a grammar form removed; nothing added above 4 |
| Kinds needed | none | 1 | 1 (removal), 2, 3 (rename `Index` to `Apply`) | 1 (removal), 2 twice, 3 | 1 (removal), 2 twice, 3 | 1 (removal), 4 |
| Rules added | 0 | about 2 | about 6 | about 7 | about 7 | 0 |
| Rules removed | 0 | 5 | 5 | 5 | 7 | about 49 |
| Rules reworded | 0 | about 15 | about 44 | about 50 | about 50 | about 5 |
| U1 list | `xs[0]` | `xs[0]` | `xs(0)` | `xs(0)` | `xs(0)` | `xs.at(0)` |
| U2 map count | `m[k] += 1` | same | `m(k) += 1` | same | same | `m.set(k, m.at(k) + 1)` |
| U3 field index | `t.parts[0]` | same | `(t.parts)(0)` | `t.parts(0)` | `t.parts(0)` | `t.parts.at(0)` |
| U4 type arguments | `first[string](xs)` | `first::[string](xs)` | unchanged | unchanged | unchanged | unchanged |
| U5 grid | `g[(0, 1)]` | same | `g(0, 1)` | same | same | `g.at(0, 1)` |
| U6 cell | `c.get()` | same | `c()` | same | same | `c.get()` |
| Action at a distance | no | no | no | yes: a new method changes old calls | a trait impl can clash with a field | no |
| Agent-writability | known from Python and Rust | a Rust-like marker | Scala-like; one parenthesis rule | easiest to write | Scala-like | verbose but plain |
| Human readability | `[]` flags an element read | two type-argument spellings | calls and reads look alike | same, plus hidden lookup | same | method names flag reads |

Rule counts are estimates from the tables in
[What hd Has Today](#what-hd-has-today); a spec-update pass would give exact
numbers.

## Recommendation

**Recommendation.** Option C1, call indexing with parentheses for a field,
if the owner accepts calls and element reads looking alike. Otherwise keep
option A.

- C1 removes the bracket ambiguity with no new token, and the owner's
  sample parses with today's grammar.
- C1 changes no lookup rule. The one cost, 10 sites of parentheses,
  reuses the stored-function rule `(x.f)(args)`.
- Its costliest change is a kind 2 exception, "a call whose callee type
  implements `Update` is a place", which replaces today's index place rule.
- It adds U5 and U6, grids and cells, with no new mechanism: the keys
  tuple mirrors `Fn`'s inputs.
- C2 and C3 remove the 10 parentheses but bring back name-based
  resolution, which is what the idea set out to remove.

**What it gives up.** The `[]` cue that an expression reads an element, and
Python- and Rust-familiar indexing that F# users asked for. Three calls in
a row, as in `list_of(log, xs)(note(log, "i", 1))`, need types to read.

**Next best.** Option A. B and D each remove the ambiguity at a larger
cost: B adds a token pair for 124 sites, and D gives up `m(k) += 1`.

## Questions For The Owner

### Q1. Field Then Index

**Effect:** `user.tags(0)` is a method call, so indexing a field needs
parentheses at 10 of 178 index sites.

1. **C1, parentheses** (recommended): `(user.tags)(0)`, as a stored
   function is called today.
2. C2: fall back to the field when no method has the name.
3. C3: one namespace for fields and methods.

```text
data User:
    tags: List[string]

fn first_tag(user: User) -> string:
    (user.tags)(0)
```

### Q2. Keys Argument Shape

**Effect:** a bound on a one-key type spells a one-element tuple.

1. **A tuple-kinded `Keys`, as `Fn` has** (recommended): `Apply[(i32,)]`,
   and `g(0, 1)` passes `(0, 1)`.
2. One key type `K`: `Apply[i32]`, and a grid takes a tuple key,
   `g((0, 1))`.

```text
use std.ops.Apply

fn head[C < Apply[(i32,)]](items: C) -> C::Out:
    items(0)
```

### Q3. Map Read Type

**Effect:** today `m[k]` reads `V?`, but `m[k] += 1` reads `V`. Under call
indexing, `Apply` has one `Out` for both.

1. **`m(k)` reads `V` and panics; `m.get(k)` gives `V?`** (recommended), as
   Scala's `Map.apply` does. This is Special Cases question
   [Q9](SPECIAL_CASES.md#q9-map-indexing).
2. `m(k)` reads `V?`, and compound assignment keeps its own `V` read.

```text
fn score(scores: Map[string, i32], name: string) -> i32:
    match scores.get(name):
        .Some(value) => value
        .None => 0
```

### Q4. Call Indexing At All

**Effect:** indexing moves from `xs[0]` to `xs(0)` at 178 sites, and
`[]` after an operand then means only type arguments.

1. **Option C, call indexing** (recommended, with Q1 to Q3 deciding its
   details).
2. Option A, keep `[]` and name resolution.
3. Option B, `::[T]` for type arguments in expressions.
4. Option D, methods only.

```text
fn demo(names: mut List[string]) -> string:
    item := names(0)
    names(0) = item + "!"
    first[string](names)
```

## Sources

- Scala Language Specification 2.13, chapter 6, Function Applications and
  Assignments: "the application is taken to be equivalent to
  `f.apply(e1, ..., em)`", and `f(args) = e` "is interpreted as
  `f.update(args, e)`".
  <https://www.scala-lang.org/files/archive/spec/2.13/06-expressions.html>
- Scala Language Specification 2.13, chapter 2: "There are two different
  name spaces, one for types and one for terms."
  <https://www.scala-lang.org/files/archive/spec/2.13/02-identifiers-names-and-scopes.html>
- Ada 2012 Reference Manual 4.1.3: "The designator of the subprogram shall
  not be the same as that of a component of the tagged type visible at the
  point of the selected_component."
  <https://www.adaic.org/resources/add_content/standards/12rm/html/RM-4-1-3.html>;
  user-defined indexing, 4.1.6:
  <https://www.adaic.org/resources/add_content/standards/12rm/html/RM-4-1-6.html>
- Fortran 2018 draft standard, J3/18-007r1, 9.5.3 (array elements) and
  15.5 (function references). <https://j3-fortran.org/doc/year/18/18-007r1.pdf>
- MATLAB, Function Precedence Order: variables are checked before
  functions. <https://www.mathworks.com/help/matlab/matlab_prog/function-precedence-order.html>
- Kotlin, Operator overloading (indexed access and invoke):
  <https://kotlinlang.org/docs/operator-overloading.html>; Kotlin
  specification, Overload resolution, "functions before properties,
  members before extensions": <https://kotlinlang.org/spec/overload-resolution.html>
- Swift, Subscripts:
  <https://docs.swift.org/swift-book/documentation/the-swift-programming-language/subscripts/>;
  SE-0253, Callable values of user-defined nominal types:
  <https://github.com/apple/swift-evolution/blob/main/proposals/0253-callable.md>
- Go specification, Index expressions and Instantiations:
  <https://go.dev/ref/spec#Index_expressions>, <https://go.dev/ref/spec#Instantiations>;
  go/ast `IndexListExpr`: <https://pkg.go.dev/go/ast#IndexListExpr>
- Rust, `std::ops::Index`: <https://doc.rust-lang.org/std/ops/trait.Index.html>;
  Reference, paths in expressions (turbofish):
  <https://doc.rust-lang.org/reference/paths.html#paths-in-expressions>;
  field expressions (parenthesize to call a stored closure):
  <https://doc.rust-lang.org/reference/expressions/field-expr.html>
- Nim manual, method call syntax: `x.p[T]` "is always parsed as
  `(x.p)[T]`", so `x.p[:T]` exists.
  <https://nim-lang.org/docs/manual.html>
- What's new in F# 6, "Simpler indexing syntax with `expr[idx]`".
  <https://learn.microsoft.com/en-us/dotnet/fsharp/whats-new/fsharp-6>
- Java `List` (`get` and `set`):
  <https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/util/List.html>

## Parse Log

Parsed with `parseSource` from
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts).
Parsing checks syntax only; no block is claimed to type-check. Option C's
blocks parse because today's grammar already accepts a call before `=`;
their meaning is the proposal.

| Block | Where | Result |
| --- | --- | --- |
| 1 | Problem: the owner's sample | parses |
| 2 | Option A | parses |
| 3 | Option B, `::[T]` | `syntax-error` at line 4, on a line marked `# hypothetical syntax` (lines 4 and 5 are both marked) |
| 4 | Option C, U1, U2, U4 | parses |
| 5 | `Apply` and `Update` traits | parses |
| 6 | Grid, two keys | parses |
| 7 | Cell, zero keys | parses |
| 8 | Map `tally` | parses |
| 9 | Compound desugaring | parses |
| 10 | Invalid places | parses; the errors are semantic |
| 11 | `user.tags(0)` today | parses; the error is semantic |
| 12 | C1 parentheses | parses |
| 13 | C2 fallback | parses |
| 14 | C3 shared name | parses; the error is semantic |
| 15 | Option D | parses |
| 16 | `interpolate` under C1 | parses |
| 17 | `interpolate` under C2 or C3 | parses |
| 18 | `fill`, map store and call | parses |
| 19 | `first` and `last` | parses |
| 20 | Cart store through an index | parses |
| 21 | Call result indexed | parses |
| 22 | Evaluation-order fixture | parses |
| 23 | Q1 | parses |
| 24 | Q2 | parses |
| 25 | Q3 | parses |
| 26 | Q4 | parses |

Reference-parser finding: none. The parser accepts `f(x) = v` and
`(x.f)(k) = v` today, as [`grammar.stmt.assign-target`](../spec/02-grammar.md#r-grammar.stmt.assign-target)
leaves the place check to semantics.
