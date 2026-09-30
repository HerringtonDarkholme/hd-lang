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
[Method Calls](../spec/05-expressions.md#method-calls),
[Method Resolution](../spec/09-traits.md#method-resolution),
[Primary Expressions](../spec/02-grammar.md#primary-expressions), and
[Explicit Type Arguments](../spec/07-functions.md#explicit-type-arguments).

**Revision, 2026-09-30 (second pass).** The owner rejected D and C2, and
replaced C3 with C3′, an ordered one-namespace lookup. This pass adds a
[survey of trait-based languages](#survey-fields-methods-and-traits),
corpus counts for C3′, and a side-by-side [comparison](#comparison) of B,
C1, and C3′. The [recommendation](#recommendation) and
[questions](#questions-for-the-owner) are rewritten.

## Owner Direction So Far

Stated by the owner on 2026-09-30 and relayed with this task. None of it is
applied to the specification.

| Item | Direction |
| --- | --- |
| D, no index syntax | Rejected. |
| C2, fall back to a field when no method has the name | Rejected. |
| C3, one namespace as first written | Rejected, because trait methods collide with fields. |
| C3′ | Replaces C3; see [Option C3′](#c3-one-namespace-with-an-ordered-lookup). |
| Remaining options | B, C1, and C3′, with A as the baseline. |

The owner wrote: "i hate syntax ambiguity. i have one crazy thing"

```text
item := list(0)
list(0) = item + 1

var := live(0)
println(var())
var() = 1
```

## Contents

- [Owner Direction So Far](#owner-direction-so-far)
- [Problem](#problem)
- [What hd Has Today](#what-hd-has-today)
- [Use Cases](#use-cases)
- [Survey](#survey)
- [Survey: Fields, Methods, And Traits](#survey-fields-methods-and-traits)
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
| [`names.method-lookup.inherent`](../spec/03-names-and-scopes.md#r-names.method-lookup.inherent) | A visible own inherent method wins over every trait method. |
| [`names.method-lookup.ambiguous`](../spec/03-names-and-scopes.md#r-names.method-lookup.ambiguous) | A promoted candidate beside a trait candidate is `ambiguous-method`. |
| [`names.method-lookup.no-silent`](../spec/03-names-and-scopes.md#r-names.method-lookup.no-silent) | Neither a trait method nor a promoted method silently wins over the other. |
| [`names.conflict.namespace`](../spec/03-names-and-scopes.md#r-names.conflict.namespace) | Fields and methods conflict only within their own namespace. |
| [`names.change.candidate`](../spec/03-names-and-scopes.md#r-names.change.candidate) | A new impl, use, or promoted method can make a call ambiguous, never switch it. |

**History.** hd had one namespace until 2026-09-26. Under that rule, M1, a
field and an inherent method of one name were `duplicate-inherent-member`,
and a field beside a usable trait method made `x.name(args)`
`ambiguous-method`. M2 split the namespaces
([spec Revision Notes](../spec/README.md#revision-notes)). Its reason: a
trait author who adds a method named like a type's field breaks that
type's package without seeing it.

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

## Survey: Fields, Methods, And Traits

The owner asked for languages built on traits or typeclasses rather than
classes. Each row answers three questions. Is there one member namespace?
How does `x.name(...)` choose among a field, an inherent or namespace
function, and a trait method? How are conflicts reported? "Tested" means
run locally; "unverified" means no source was found.

| Language | One namespace? | How `x.name(...)` chooses | Conflicts |
| --- | --- | --- | --- |
| MoonBit | No. `HashMap` has a field `capacity` and a method `HashMap::capacity` (core source). | A method `fn T::m`; a regular method beats a trait method attached with `extend`. A field function is called as `(self.f)()` (8 sites in core). | Implicit dot calls to trait methods are deprecated: "a new default method in an upstream trait can make an existing dot call ambiguous". On `T: Trait`, dot works for the one written bound only. |
| Rust | No, fields and methods are separate. | Per auto-deref step, inherent methods, then trait methods in scope. `s.f()` is always a method; a stored closure is `(s.f)()`. | Two trait candidates at one step are an error at the use (E0034); a qualified call fixes it. |
| Lean 4 | Yes. A field `f` of `S` is the function `S.f`, so fields and namespace functions share `S`'s namespace. | `x.f` finds `S.f` in `x`'s type's namespace, then in parent structures, in C3-linearization order. | Class methods live in the class's namespace, such as `ToString.toString`, so `x.f` does not reach them; this follows from the two rules, unverified as a sentence. Overlapping parent fields must have one type. |
| Haskell | Fields share the module's top-level namespace with functions and class methods. | `x.f` (OverloadedRecordDot) is `getField @"f" x`, fields only. `x.f y` applies the field. Class methods are plain functions, `f x`. | A field and another top-level binding of one name are an error, unless `NoFieldSelectors`. A `HasField` instance for a real field is rejected. |
| Swift | One member namespace with overloading by full name. `var count` and `func count(_:)` coexist; `var count` and `func count()` are "invalid redeclaration" (tested, Swift 6.4). | A conforming type's own member is used instead of a protocol extension's. `d.run(1)` picked a closure property over a method `run(_:)` (tested, undocumented). | Overload ranking, errors at the use. |
| Scala 3 | Yes, one term namespace per class. | Members first. An extension method is tried only when `e.m` finds no member `m`. | "If there is more than one way of rewriting, an ambiguity error results." |
| Go | Yes. For a struct, "the non-blank method and field names must be distinct". | `x.f` is the field or method at the shallowest depth; `x.Run(41)` calls a function field (tested). Interfaces add no members to a concrete type. | Two at the shallowest depth: `ambiguous selector` at the use (tested, Go 1.24). |
| Gleam | No methods. | `record.field` reads a field; functions are module functions. | Traits and type classes are "not planned". |
| Roc | Unverified in detail. | Method calls use static dispatch on nominal types. `(rec.func)(3)` stays parenthesized, "so `2 \|> (rec.func)(3)` does not become a method call". | Unverified. |
| Koka | Yes: fields generate accessor functions of the same name. | `x.f(args)` is sugar for `f(x, args)`; overloads resolve by type. | A use that fits several overloads is an error without an annotation. |
| Nim | No, fields and procs are separate. | `x.f(args)` is `f(x, args)`; "the builtin dot access is preferred if it is available". | "Object fields and accessors can have the same name"; inside the module `x.f` is the field, outside it calls the accessor. |
| D | One aggregate scope (unverified). | A member wins; a free function is used by UFCS only when "the member function does not (or cannot) exist". | Unverified. |
| Zig | Fields and declarations in one container (duplicate rule unverified). | `x.f()` calls a declaration with `x` first; a function-pointer field is applied, as in `a.vtable.alloc(a.ptr, ...)` in `std.mem.Allocator`. | Unverified. |
| Carbon | Yes: one class scope. | Interface methods reach `x.f` only through `extend impl` inside the class; otherwise `x.(Iface.f)`. | "If more than one distinct member is found ... the lookup is ambiguous." An impl without `extend` avoids a name conflict. |
| Mojo | One struct scope. | Trait methods are written inside the struct body, so they are ordinary members. | Field and method of one name: unverified. |
| Java | No: "Fields, methods ... may have the same name". | By syntax. | n/a |
| C# | Yes: a field's name "shall differ from the names of all other members". | By member kind. | Declaration error. |
| Kotlin | Separate; a property with `invoke` is found after functions. | "Functions before properties". | Overload resolution. |

**Takeaways.**
1. The trait-based languages closest to hd keep **C1's shape**. Rust and
   MoonBit keep fields and methods apart, and both call a stored function
   as `(x.f)(args)`.
2. **C3′'s steps 1 and 2 resemble Lean 4 and Go.** Lean 4 puts fields and
   namespace functions in one namespace and searches parents in order. Go
   picks the shallowest member. Neither lets a trait or class method compete
   for `x.f`.
3. **C3′'s step 3 resembles Scala 3 and Swift.** An extension method, or a
   protocol extension's method, is used only when the type's own members do
   not answer. So an own field silently wins over it, as C3′ proposes.
4. **Lean 4 and Haskell keep class methods off the dot entirely.** MoonBit
   deprecated implicit trait dot calls in favor of `extend`, because an
   upstream trait change can make a dot call ambiguous. Carbon requires
   `extend impl`, so a type's API is fixed by its class definition.
5. Where fields and methods share a namespace, a same-name pair is a
   declaration error in Go, Swift (same full name), C#, and Haskell. That
   matches C3′'s step 1.

## Options

Three options remain, ranked later by the
[Design Cost Order](../AGENTS.md#design-cost-order): B, and option C with
its field variants C1 and C3′. A is the baseline.

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

**C2: fall back to the field.** Rejected by owner, 2026-09-30.

**C3: one namespace, as first written.** Rejected by owner, 2026-09-30,
because trait methods collide with fields. C3′ replaces it.

#### C3′: One Namespace With An Ordered Lookup

Fields and methods form one member namespace. `x.name` and `x.name(args)`
resolve `name` by one ordered lookup. `x.name(args)` applies whatever
member it finds: a method is called, and a field is read and applied.

| Step | Members | Clash inside the step |
| --- | --- | --- |
| 1 | the type's own fields and inherent methods | a field and an inherent method of one name: a declaration error, `duplicate-inherent-member` as under M1 |
| 2 | promoted fields and methods, shallowest depth first | same depth: `ambiguous-promoted-member` at the declaration, as today |
| 3 | trait methods | two traits: `ambiguous-method`, as today |

- A promoted member beside a trait method of the same name stays
  `ambiguous-method`, with no silent winner, as
  [`names.method-lookup.no-silent`](../spec/03-names-and-scopes.md#r-names.method-lookup.no-silent)
  and [`names.change.candidate`](../spec/03-names-and-scopes.md#r-names.change.candidate)
  say today.
- An own field beside a trait method is decided by the order: the field
  wins, as an own inherent method wins today
  ([`names.method-lookup.inherent`](../spec/03-names-and-scopes.md#r-names.method-lookup.inherent)).
  The trait method stays reachable as `Trait::name(x)`.
- In generic code, `T < Trait`, and on trait values, only trait methods
  exist, so fields never compete.

```text
trait Named:
    fn name(self) -> string

data User:
    name: string
    tags: List[string]

impl Named for User:
    fn name(self) -> string: self.name

fn first_tag(user: User) -> string:
    user.tags(0)

fn first_byte(user: User) -> u8:
    user.name(0)

fn greeting(user: User) -> string:
    Named::name(user)

fn show[T < Named](value: T) -> string:
    value.name()
```

`user.name(0)` reads byte 0 of the field. `Named::name(user)` and
`value.name()` reach the trait method.

```text
data User:
    tags: List[string]

impl User:
    fn tags(self) -> List[string]:  # error under C3′: duplicate-inherent-member
        self.tags
```

```text
trait Labeled:
    fn label(self) -> string

data Base:
    pub label: fn() -> string

data Page:
    Base

impl Labeled for Page:
    fn label(self) -> string: "page"

fn invalid(page: Page) -> string:
    page.label()  # error under C3′: ambiguous-method
```

**How C3′ answers M2's reason.** A trait that gains a method named like an
own field changes no call, because the field wins at step 1. A trait that
gains a method named like a promoted member makes `x.name(args)`
`ambiguous-method` at the use, as a promoted method does today. Neither
switches a call silently.

**Open points inside C3′.** The owner's text leaves three cases open; each
is a question below.

| Case | Choices |
| --- | --- |
| Bare `x.name` where step 2 finds a field and step 3 a trait method | ambiguous, as for a call; or the bare form skips trait methods, since a method is not a value |
| A private own field beside a trait method of one name | skip the field outside its module, as today's [`names.take-part.trait-caller`](../spec/03-names-and-scopes.md#r-names.take-part.trait-caller) skips a private inherent method; or the field always wins and is `private-member` outside |
| `User::email` for a field | keep [`fn.ref.no-fields`](../spec/07-functions.md#r-fn.ref.no-fields); or a field becomes a member reference |

**C4: other ideas considered.**

| Idea | Why not |
| --- | --- |
| `user.tags.apply(0)` | Works today with no rule, but it is the long spelling; C1 is shorter. |
| Field first, as Ada | The rejected C2 fallback, in the other direction. |
| Merge only for fields whose type is callable | A type-dependent rule; changing a field's type changes lookup. |
| A field-apply operator such as `user.tags.(0)` | A new token for 10 sites. |
| Trait methods reach the dot only through an `extend` line, as MoonBit and Carbon | A new declaration form; it would change every trait dot call, not only indexing. |

### Option D: No Index Syntax

Rejected by owner, 2026-09-30.

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
   beside a same-named method: the exact case C3′ must decide.
4. No code indexes twice in a row, and no code indexes with a tuple key.
   U5 and U6 have no current users.

### C3′ Counts

A second script, on `main` at c2e50a32, read every `data` declaration,
inherent `impl`, trait `impl`, and trait in the same corpus, plus
`typing/warnings`. Trait methods include defaults from the trait
declaration and the prelude traits of
[Built-In Methods](../spec/10-modules.md#built-in-methods). Enums with
shared fields were not scanned. Every hit was checked by hand. The table
lists 10 types; it replaces the first pass's estimate of 6 shared-name
types.

| Collision on one type | Tour | std | Examples | Fixtures |
| --- | --- | --- | --- | --- |
| own field and own inherent method | 0 | 0 | 0 | 1 type, 2 names |
| promoted field and own or promoted method | 0 | 0 | 0 | 0 |
| own field and promoted method | 0 | 0 | 0 | 1 |
| own field and trait method | 0 | 0 | 0 | 8 types |
| promoted field and trait method | 0 | 0 | 0 | 0 |

Sites whose meaning C3′ changes, all in fixtures:

| Site | Today | Under C3′ |
| --- | --- | --- |
| `field-and-inherent-method-share-name.hd`, `user.name()` and `user.Base()` | inherent methods | the type is `duplicate-inherent-member` |
| `outer-field-beside-embedded-method.hd`, `page.label()` | the promoted method | the own field `label: string` applied to `()`: a type error |
| `field-and-trait-method-share-name.hd`, `user.name()` | the trait method, `"trait"` | the own field `name: fn() -> string`, `"field"`: it type-checks and the result changes |
| `function-typed-field-method-call.hd`, `button.on_click(41)` (invalid) | `unknown-method` | valid: applies the field |
| `embedded-field-not-called.hd`, `job.run()` (invalid) | `unknown-method` | valid: applies the promoted field |

Findings:
1. Every collision is a fixture written to test today's separate
   namespaces. The tour, std, and examples have none.
2. Seven of the 8 field-and-trait types follow one idiom: a field `name`
   and a trait getter `name(self)` that returns it. Their calls go through
   `Trait::name`, a bound `T < Trait`, or a trait value, so C3′ leaves them
   alone. A concrete `user.name()` on such a type would apply the field.
3. The 10 `obj.field[i]` sites become `obj.field(i)` with no parentheses.
   None of those receivers has a method or trait method of the field's
   name, so none changes meaning.
4. The 8 `(x.f)(args)` sites keep their meaning and may drop the
   parentheses.

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

The same under C3′:

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
| C3′ | 178 | 0; 8 stored-function calls may drop parentheses | 3 valid fixtures; 2 invalid fixtures become valid |

## Comparison

Kinds follow the Design Cost Order: 1 is a syntax change, 2 a semantic
rule exception, 3 a compiler intrinsic, and 4 a core library addition. C1
and C3′ share call indexing; they differ only in how `x.name(args)` treats
a field.

### Use Cases Side By Side

| | A: keep | B: `::[T]` | C1: call, parentheses | C3′: call, ordered namespace |
| --- | --- | --- | --- | --- |
| U1 list | `xs[0]` | `xs[0]` | `xs(0)` | `xs(0)` |
| U2 map count | `m[k] += 1` | same | `m(k) += 1` | same |
| U3 field index | `t.parts[0]` | same | `(t.parts)(0)` | `t.parts(0)` |
| U3 field store | `cart.items[0].quantity = 0` | same | `(cart.items)(0).quantity = 0` | `cart.items(0).quantity = 0` |
| U4 type arguments | `first[string](xs)` | `first::[string](xs)` | unchanged | unchanged |
| U5 grid | `g[(0, 1)]` | same | `g(0, 1)` | same |
| U6 cell | `c.get()` | same | `c()` | same |
| Stored function | `(h.callback)(e)` | same | same | `h.callback(e)` |

### Rule Accounting

Counts are estimates from the rule lists in
[What hd Has Today](#what-hd-has-today); a spec-update pass would give exact
numbers. The C3′ column counts only what it changes beyond C1.

| | B | C1 | C3′, beyond C1 |
| --- | --- | --- | --- |
| Added | about 2: the `"::", function_type_arguments` production and its rule | about 6: `Apply` and `Update` calls, the call place rule, keys tuples | about 7: one namespace; the step order; a field beside an inherent method is `duplicate-inherent-member`; `x.name(args)` applies a field; an own field wins over a trait method; a promoted field beside a trait method is `ambiguous-method`; the bare-form rule of [Q4](#q4-bare-field-beside-a-trait-method) |
| Removed | 5: `grammar.primary.generic-reference`, `.preserve-ambiguity`, `fn.generic.brackets`, and 2 folded method-type-argument rules | the same 5 | 13: `names.member.namespaces`, `.shared-name`, `.no-hiding`; `names.lookup.field-form`, `.method-form`, `.stored-fn`; `names.conflict.namespace`; `names.method-lookup.hint.field`; `expr.member.no-field-call`, `.stored-fn`, `.stored-fn.error`, `.stored-fn.hint`; `trait.resolve.no-field` |
| Reworded | about 15: `expr.call.generic.*`, `grammar.expr.method-type-arguments`, pipe placeholder rules | about 44 index, place, and assignment rules; `expr.member.stored-fn.hint` widens to applicable fields | about 30: the 5 `names.field-lookup.*` and 12 `names.method-lookup.*` rules merge into one lookup; `names.hide.depth`, `names.conflict.definition`, `.private-own`; the 5 `names.method-example.*`; `names.change.candidate`, `names.promoted.readonly`, `names.take-part.trait-caller`; `expr.member.field-read`, `.method-call`, `.method-not-value`; `trait.resolve.lookup`, `.lookup.kinds`, `.inherent-first`, `.ambiguous.promoted` |
| Chapter 03 lookup | unchanged | unchanged | rewritten; reverses M2 of 2026-09-26 |
| Embedding, chapter 08 | unchanged | unchanged | unchanged rules; promotion now crosses fields and methods |
| Fixtures | 124 sites respelled | 178 sites respelled | also 3 valid fixtures fail and 2 invalid ones pass |
| Costliest kind | 1, a new token pair | 2, some calls are places | 2, several lookup rules |
| Kinds needed | 1 | 1 (a removal), 2, 3 (`Index` renamed `Apply`) | C1's, plus 2 several times |

### Action At A Distance

Each row is a change made somewhere else, and what it does to an existing
`x.name(args)` or `x.name`.

| Change elsewhere | A, B, C1 | C3′ |
| --- | --- | --- |
| A trait gains a method named like an own field | nothing | nothing: the field wins at step 1 |
| A trait gains a method named like a promoted field | nothing | `x.name(args)` becomes `ambiguous-method`, an error at the use |
| A dependency's embedded type gains a shallower `pub` field named like the method a call selects | nothing: fields and methods are apart | the call silently applies the field, by [`names.change.shallower`](../spec/03-names-and-scopes.md#r-names.change.shallower) |
| The type's own package adds a field named like a trait method | nothing | concrete `x.name(args)` silently switches to the field; generic calls do not |
| The type's own package adds a method named like an own field | legal | `duplicate-inherent-member` at the declaration |

The third row is new: today `names.change.shallower` switches only a field
to a field or a method to a method. Under C3′ it can switch a method call to
a field application across packages. The fourth row matches today's rule
that an own inherent method silently beats a trait method, but a field
rarely means what a trait method means.

### Agent-Writability And Readability

| | B | C1 | C3′ |
| --- | --- | --- | --- |
| Agent writes `x.f(i)` for a field | works: indexing stays `x.f[i]` | `unknown-method`; the hint gives `(x.f)(i)` | works |
| Agent writes `(x.f)(i)` from Rust or MoonBit habit | n/a | works | works |
| Agent writes `user.name()` for a trait getter beside a field `name` | works | works | applies the field: a type error, or a wrong value when the field holds a function |
| Agent writes `f[T](x)` from today's hd | `syntax-error` with a fix-it | works | works |
| A reader sees `x.f(i)` | a method call | a method call | a method call or a field read; the type decides |
| Closest languages | Rust (`::<>`), Nim (`[:`) | Rust, MoonBit | Go and Lean 4 for steps 1 and 2; Scala 3 and Swift for step 3 |

## Recommendation

**Recommendation.** Option C1, call indexing with parentheses for a field.
C3′ is the next best; B is third.

- C1 changes no lookup rule and keeps M2, decided four days earlier. Its
  cost is 10 sites of parentheses, in the form `(x.f)(args)` that Rust and
  MoonBit use.
- The survey found no trait-based language that lets trait methods compete
  with fields on the dot without a rule like C3′'s order. Lean 4 and
  Haskell keep class methods off the dot. MoonBit and Carbon make trait dot
  calls explicit with `extend`.
- C3′ solves M2's trait collision for own fields, but it opens a new silent
  switch across packages: a promoted field can take over a promoted method's
  call.
- C3′ turns the fixtures' common idiom, a field `name` beside a trait
  getter `name()`, into a case where `user.name()` applies the field.
- C3′ reads best at the 10 sites and drops the `(x.f)(args)` special form,
  which is its real gain.

**What it gives up.** `t.parts(0)` reads better than `(t.parts)(0)`, and
agents from Go, Swift, Zig, or Scala will first write the former. Under C1
they get `unknown-method` with a fix-it.

**Next best.** C3′, if the owner prefers one member namespace, with
[Q4](#q4-bare-field-beside-a-trait-method) and
[Q5](#q5-private-own-field-beside-a-trait-method) answered. B keeps `[]`
indexing but adds a token pair for 124 sites.

## Questions For The Owner

Q1 to Q3 need an answer under any option but A. Q4 and Q5 apply only if Q1
picks C3′.

### Q1. Which Option

**Effect:** decides whether `[]` or `()` indexes, and whether
`user.tags(0)` reaches a field.

1. **C1** (recommended): `xs(0)`, and a field is indexed as
   `(user.tags)(0)`.
2. C3′: `xs(0)` and `user.tags(0)`, with one ordered member namespace.
3. B: `xs[0]` stays, and type arguments become `first::[string](xs)`.

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

### Q4. Bare Field Beside A Trait Method

**Effect:** only under C3′. If `page.label` is ambiguous when a trait gains
a method `label`, that trait breaks every read of a promoted field.

1. **The bare form skips trait methods** (recommended), since a method is
   not a value: `page.label` reads the promoted field.
2. The bare form uses the same lookup as a call, so `page.label` is
   `ambiguous-method`.

```text
trait Labeled:
    fn label(self) -> string

data Base:
    pub label: string

data Page:
    Base

impl Labeled for Page:
    fn label(self) -> string: "page"

fn read(page: Page) -> string:
    page.label
```

### Q5. Private Own Field Beside A Trait Method

**Effect:** only under C3′. Today a caller that cannot see an own inherent
method skips it and may reach a trait method. For a field, that makes
`user.name()` mean two things, by module.

1. **The field always wins; outside its module the call is
   `private-member`** (recommended): one meaning for every caller.
2. Skip the private field outside its module, as
   [`names.take-part.trait-caller`](../spec/03-names-and-scopes.md#r-names.take-part.trait-caller)
   does for methods.

```text
trait Named:
    fn name(self) -> string

pub data User:
    name: string

impl Named for User:
    fn name(self) -> string: self.name

fn outside(user: User) -> string:
    Named::name(user)
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

Sources for [Survey: Fields, Methods, And Traits](#survey-fields-methods-and-traits),
fetched 2026-09-30:

- MoonBit, Method and Trait: `fn T::m`, "A regular method takes precedence
  over an attached trait method", `extend`, and the deprecation of implicit
  attachment. <https://docs.moonbitlang.com/en/latest/language/methods.html>;
  source <https://github.com/moonbitlang/moonbit-docs/blob/main/next/language/methods.md>
- MoonBit core: field `capacity` in `hashmap/types.mbt` beside
  `HashMap::capacity` in `hashmap/utils.mbt`; `(self.f)()` in
  `builtin/iterator.mbt` and 7 more sites. <https://github.com/moonbitlang/core>
- Rust Reference, method-call expressions (inherent, then trait, per
  auto-deref step; several candidates are an error):
  <https://doc.rust-lang.org/reference/expressions/method-call-expr.html>;
  field expressions: <https://doc.rust-lang.org/reference/expressions/field-expr.html>
- Lean 4 Reference, generalized field notation:
  <https://lean-lang.org/doc/reference/latest/Terms/Function-Application/#generalized-field-notation>;
  structure projections and parents:
  <https://lean-lang.org/doc/reference/latest/The-Type-System/Inductive-Types/>;
  resolution order over all ancestors, Lean 4.14.0, PR #5770:
  <https://lean-lang.org/doc/reference/latest/releases/v4.14.0/>; class
  methods: <https://lean-lang.org/doc/reference/latest/Type-Classes/Class-Declarations/>
- Haskell 2010 Report 3.15.1: selectors "cannot conflict with other top
  level bindings of the same name".
  <https://www.haskell.org/onlinereport/haskell2010/haskellch3.html>; GHC
  OverloadedRecordDot:
  <https://ghc.gitlab.haskell.org/ghc/doc/users_guide/exts/overloaded_record_dot.html>;
  HasField: <https://ghc.gitlab.haskell.org/ghc/doc/users_guide/exts/hasfield.html>;
  NoFieldSelectors: <https://ghc.gitlab.haskell.org/ghc/doc/users_guide/exts/field_selectors.html>
- Swift book, Protocols, Providing Default Implementations: "that
  implementation will be used instead of the one provided by the extension".
  <https://github.com/swiftlang/swift-book/blob/main/TSPL.docc/LanguageGuide/Protocols.md>;
  the redeclaration and closure-property results were tested with Swift 6.4.
- Scala 3 Reference, Extension Methods, Translation of Calls to Extension
  Methods: <https://docs.scala-lang.org/scala3/reference/contextual/extension-methods.html>
- Go specification, Selectors and Method declarations:
  <https://go.dev/ref/spec#Selectors>, <https://go.dev/ref/spec#Method_declarations>;
  the `ambiguous selector` and function-field results were tested with Go 1.24.
- Gleam: record accessors, <https://tour.gleam.run/data-types/record-accessors/>;
  FAQ, type classes "are not planned", <https://gleam.run/frequently-asked-questions/>
- Roc, PR #11760, parentheses kept around a record-field callee:
  <https://github.com/roc-lang/roc/pull/11760>
- Koka book, 3.1.2 Dot selection and 3.3.1 Structs:
  <https://koka-lang.github.io/koka/doc/book.html>
- Nim manual, Method call syntax and Properties:
  <https://nim-lang.org/docs/manual.html>
- D specification, Uniform Function Call Syntax:
  <https://dlang.org/spec/function.html>
- Zig language reference, structs and namespaces:
  <https://ziglang.org/documentation/master/>; `std.mem.Allocator`:
  <https://github.com/ziglang/zig/blob/master/lib/std/mem/Allocator.zig>
- Carbon design, member access and `extend impl`:
  <https://github.com/carbon-language/carbon-lang/blob/trunk/docs/design/expressions/member_access.md>,
  <https://github.com/carbon-language/carbon-lang/blob/trunk/docs/design/generics/details.md>
- Mojo manual, Traits: <https://mojolang.org/docs/manual/traits>
- Java Language Specification SE 21, 8.2 Class Members:
  <https://docs.oracle.com/javase/specs/jls/se21/html/jls-8.html>
- C# specification 15.3.1, Class members:
  <https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/classes>

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
| 13 | C3′ lookup: field, trait method, generic | parses |
| 14 | C3′ field beside an inherent method | parses; the error is semantic |
| 15 | C3′ promoted field beside a trait method | parses; the error is semantic |
| 16 | `interpolate` under C1 | parses |
| 17 | `interpolate` under C3′ | parses |
| 18 | `fill`, map store and call | parses |
| 19 | `first` and `last` | parses |
| 20 | Cart store through an index | parses |
| 21 | Call result indexed | parses |
| 22 | Evaluation-order fixture | parses |
| 23 | Q1 | parses |
| 24 | Q2 | parses |
| 25 | Q3 | parses |
| 26 | Q4 | parses |
| 27 | Q5 | parses |

Reference-parser finding: none. The parser accepts `f(x) = v` and
`(x.f)(k) = v` today, as [`grammar.stmt.assign-target`](../spec/02-grammar.md#r-grammar.stmt.assign-target)
leaves the place check to semantics.
