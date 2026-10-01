# Call Indexing: `list(0)` Instead Of `list[0]`

Status: design exploration, 2026-09-30, with the owner's decisions in
[Owner Decisions](#owner-decisions). All four are in the specification:
D1's `::[` type arguments and D3 in
[Type Arguments In Expressions](../spec/02-grammar.md#type-arguments-in-expressions),
and D1's callable values, D2, and D4 in
[Callable Values](../spec/05-expressions.md#callable-values).
Under review:
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

**Revision, 2026-09-30 (third pass).** The owner rejected D, C2, C3, and
then C3′, an ordered one-namespace lookup. Only B and C1 remain. This pass
keeps the [survey of trait-based languages](#survey-fields-methods-and-traits),
drops the C3′ material, and rewrites the [comparison](#comparison) of B
and C1, the [recommendation](#recommendation), and the
[questions](#questions-for-the-owner). The owner then noted that C1
gives live variables but makes array indexing read oddly, so
[Live Variables Without Call Indexing](#live-variables-without-call-indexing)
shows two ways to get `var()` or `var[]` under B. The owner chose to
try B+F first; [Trying B+F](#trying-bf) applies it.

## Owner Decisions

Final, 2026-09-30. Each takes the recommended option. All four are
applied; see the Applied note below the table.

| # | Question | Decision |
| --- | --- | --- |
| D1 | [Q1](#q1-b-or-c1) and [Q5](#q5-live-variables-under-b): which option | **B+F.** `[]` after an operand is always indexing. Type arguments in an expression take `::[`, as in `first::[string](xs)` and `parser.parse::[User](text)`. A type opts in to `var()` and `var() = v` through `Apply` and `Update`. |
| D2 | [Q6](#q6-arity-of-callable-values): arity of callable values | **Zero keys only.** `Apply` and `Update` take no keys, so `[]` and `()` never overlap; grids keep `g[(0, 1)]`. |
| D3 | [Q2](#q2-type-name-expressions-under-b): type arguments after a type name | **`::[` there too**: `Box::[i32] { ... }` and `Add::[i32]::add(price, 5)`. One rule for every expression, with no exception. |
| D4 | [Q7](#q7-does-a-live-variable-need-mut): does a live variable need `mut` | **Yes.** `update` takes `mut self`, so `var() = 1` needs a `mut` cell and a write shows in the holder's type. The owner writes `let mut var = live(0)`, and passes a readonly view to a child, as a UI component does. |

D4 needs no new permission rule. `var := live(0)` is always a readonly
view ([`types.bind.short`](../spec/04-type-system.md#r-types.bind.short)),
so it can read `var()` but not store. `let mut var = live(0)` keeps
`live`'s `mut Cell[i32]`
([`types.bind.let-mut-infer`](../spec/04-type-system.md#r-types.bind.let-mut-infer)).
A parameter typed `Cell[i32]` is a readonly view: the child reads, and a
store is `readonly-root`.

```text
fn parent() -> void:
    let mut count = live(0)
    count() += 1
    badge(count)

fn badge(count: Cell[i32]) -> string:
    "clicked " + count().to_string()
```

| Question | Status |
| --- | --- |
| [Q3](#q3-keys-argument-shape), keys argument shape | Moot: D2 removes keys. |
| [Q4](#q4-map-read-type), map read type under C1 | Moot: C1 is not chosen. The `m[k]` read type was carried by [Syntax And Semantics Cost Q8](SYNTAX_SEMANTICS_COST.md#q8-map-read-type), which batch 26 answered: `m[k]` reads `V` and panics, and `m.get(k)` reads `V?`. |

**Applied, 2026-09-30.** D1's type-argument part and D3 are in
[Type Arguments In Expressions](../spec/02-grammar.md#type-arguments-in-expressions)
(batch 22 CI1 and CI3). The callable values, `Apply` and `Update`, and D2
and D4 are in [Callable Values](../spec/05-expressions.md#callable-values),
language tier, since call syntax uses the traits. Examples there declare
a local `Cell` and `live`; no std cell type is planned, since a live cell
is a user-land library built on `Apply` and `Update` (BFF1, closed
2026-10-01).

**Follow-ups, batch 25 (owner decision, 2026-09-30).** Final and
applied. Applying D1 to D4 raised six points; the owner answered each.
Only BF(c) and BFF2 change any text: BF(c) in
[SYNTAX_NOTES](../SYNTAX_NOTES.md#built-in-collections) and its
permission sections, and BFF2 in
[Callable Values](../spec/05-expressions.md#callable-values), where
`xs(0)` is [`expr.call.apply.not-callable`](../spec/05-expressions.md#r-expr.call.apply.not-callable).

| # | Point | Decision |
| --- | --- | --- |
| BF(a) | May a method reference take any receiver, as in `xs[0]::describe`? | **No change.** A method reference keeps a name or path receiver only; `xs[0]::describe` stays invalid, so bind the element first. |
| BF(b) | May a bare pipe step with type arguments drop `_`? | **No change.** It still needs `_`: `x \|> parse::[i32](_)`. |
| BF(c) | [SYNTAX_NOTES](../SYNTAX_NOTES.md) blocks 60, 62, and 83 are bare type fragments, which no longer parse as expressions. | **Fix them** so they parse: wrap each fragment in a declaration or a typed binding. Do not leave or demote them. |
| BFF1 | Should std ship a cell type for live variables? | **Not yet** ("no live var now, park"). Apply and Update stay in the spec as applied; spec examples keep their local `Cell` and `live`. **Closed (owner, 2026-10-01, batch 33 LIVE-CELL):** "remove live cell from todo, it can be user land lib". A live cell is a user-land library built on `Apply` and `Update`, not a std or spec item. |
| BFF2 | Is `expr.call.apply.builtin-none` needed? | **Drop it** as redundant. The owner: "Apply/Update is special std.ops trait. List/Map/string do not impl it. type check should already rejects, no new rule needed." `xs(0)` on a list is rejected by the general `not-callable` rule. |
| BFF3 | Should chapter 04's list of mutation forms name call stores? | **No change.** Leave the list; chapter 05's rule covers call stores. |

**Still open.**
- A separate idea, not part of this record: the owner wants "an escape
  hatch to get mut from readonly" (2026-09-30). It is a core question
  about [Access Permission](../spec/04-type-system.md#access-permission)
  and needs its own brainstorm. D4 does not depend on it.

## Owner Direction So Far

Stated by the owner on 2026-09-30 and relayed with this task. None of it is
applied to the specification.

| Item | Direction |
| --- | --- |
| D, no index syntax | Rejected. |
| C2, fall back to a field when no method has the name | Rejected. |
| C3, one namespace as first written | Rejected, because trait methods collide with fields. |
| C3′, one namespace with an ordered lookup (own members, then promoted, then trait methods) | Rejected, after the second pass. |
| Remaining options | B and C1, with A as the baseline. |
| B+F, B with callable values | Try first; see [Trying B+F](#trying-bf). Then decided: see [Owner Decisions](#owner-decisions). |

The owner wrote: "i hate syntax ambiguity. i have one crazy thing"

```text
item := list(0)
list(0) = item + 1

var := live(0)
println(var())
var() = 1
```

## Contents

- [Owner Decisions](#owner-decisions)
- [Owner Direction So Far](#owner-direction-so-far)
- [Problem](#problem)
- [What hd Has Today](#what-hd-has-today)
- [Use Cases](#use-cases)
- [Survey](#survey)
- [Survey: Fields, Methods, And Traits](#survey-fields-methods-and-traits)
- [Options](#options)
- [Stress Test](#stress-test)
- [Comparison](#comparison)
- [Trying B+F](#trying-bf)
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
| `grammar.primary.generic-reference` (retired by D1) | Name resolution tells `first[string](names)` from indexing. |
| `grammar.primary.preserve-ambiguity` (retired by D1) | A parser may keep the ambiguity until name resolution. |
| `grammar.expr.method-type-arguments` (retired by D1) | After member resolution, brackets after a generic method name are type arguments. |
| [`grammar.primary.member-type-arguments.rules`](../spec/02-grammar.md#r-grammar.primary.member-type-arguments.rules) | Such a bracket is valid only when the selected member is generic. |
| `fn.generic.brackets` (retired by D1) | Name resolution tells the brackets from an indexing operation. |
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
| [`names.method-lookup.ambiguous`](../spec/03-names-and-scopes.md#r-names.method-lookup.ambiguous) | A promoted candidate beside a trait candidate is `ambiguous-method`: neither silently wins. |
| [`names.member.no-hiding`](../spec/03-names-and-scopes.md#r-names.member.no-hiding) | A field and a method never hide each other, because no use looks in both namespaces. |
| [Dependency Changes](../spec/03-names-and-scopes.md#dependency-changes) | A new impl, use, or promoted method can make a call ambiguous, never switch it (a Note since batch 32). |

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
2. **One namespace is found in Go, Lean 4, Scala 3, and Swift.** Each
   then needs an order between fields, own methods, and extension or trait
   methods, which is what the rejected C3′ tried to supply.
3. **Lean 4 and Haskell keep class methods off the dot entirely.** MoonBit
   deprecated implicit trait dot calls in favor of `extend`, because an
   upstream trait change can make a dot call ambiguous. Carbon requires
   `extend impl`, so a type's API is fixed by its class definition.
4. Where fields and methods share a namespace, a same-name pair is a
   declaration error in Go, Swift (same full name), C#, and Haskell.

## Options

Two options remain, ranked later by the
[Design Cost Order](../AGENTS.md#design-cost-order): B, and option C with
its field variant C1. A is the baseline.

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
(`fn.type.ctor.input-kind`, since retired for [`fn.type.ctor.inputs-tuple`](../spec/07-functions.md#r-fn.type.ctor.inputs-tuple)).
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
| `Map[K, V]` | `Apply[(K,)]`, `Out = V` or `V?`: see [Q4](#q4-map-read-type) | `Update[(K,), V]`, inserts or replaces |
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

Today's `expr.assign.compound.place` (retired 2026-09-30)
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
because trait methods collide with fields.

**C3′: one namespace with an ordered lookup.** Rejected by owner,
2026-09-30.

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
   beside a same-named method: the case a one-namespace rule must decide.
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

## Comparison

Kinds follow the Design Cost Order: 1 is a syntax change, 2 a semantic
rule exception, 3 a compiler intrinsic, and 4 a core library addition.
Both options make brackets context-free. They differ in which of the two
bracket uses moves.

### Use Cases Side By Side

| | A: keep | B: `::[T]` | C1: call indexing |
| --- | --- | --- | --- |
| U1 list | `xs[0]` | `xs[0]` | `xs(0)` |
| U2 map count | `m[k] += 1` | same | `m(k) += 1` |
| U3 field index | `t.parts[0]` | same | `(t.parts)(0)` |
| U3 field store | `cart.items[0].quantity = 0` | same | `(cart.items)(0).quantity = 0` |
| U4 type arguments | `first[string](xs)` | `first::[string](xs)` | unchanged |
| U4 on a method | `parser.parse[User](text)` | `parser.parse::[User](text)` | unchanged |
| U5 grid | `g[(0, 1)]` | same | `g(0, 1)` |
| U6 cell | `c.get()` | same | `c()` |

### Rule Accounting

Counts are estimates from the rule lists in
[What hd Has Today](#what-hd-has-today); a spec-update pass would give exact
numbers.

| | B | C1 |
| --- | --- | --- |
| Added | about 2: the `"::", function_type_arguments` production and its rule | about 6: `Apply` and `Update` calls, the call place rule, keys tuples |
| Removed | 5: `grammar.primary.generic-reference`, `.preserve-ambiguity`, `fn.generic.brackets`, and 2 folded method-type-argument rules | the same 5, and the `postfix_suffix` index form |
| Reworded | about 15: `expr.call.generic.*`, `grammar.expr.method-type-arguments`, pipe placeholder rules | about 44 index, place, and assignment rules; `expr.member.stored-fn.hint` widens to applicable fields |
| Chapter 03 lookup | unchanged | unchanged |
| Index traits | unchanged: `Index`, `IndexSet` | renamed and generalized: `Apply[Keys]`, `Update[Keys, V]` |
| Sites respelled | 124, up to about 220 with type-name expressions | 178, and 10 gain parentheses |
| Costliest kind | 1, a new token pair | 2, some calls are places |
| Kinds needed | 1 | 1 (a removal), 2, 3 (`Index` renamed `Apply`) |

### Where They Differ

| Question | B | C1 |
| --- | --- | --- |
| What a reader sees at `x(0)` | always a call | a call or an element read; the callee's type decides |
| What marks a place | `[...]`, syntactically | the callee's type: `f(x) = v` is a store only when `f` has `Update` |
| Three calls in a row, `list_of(log, xs)(note(log, "i", 1))` | cannot happen: it would be `list_of(log, xs)[note(log, "i", 1)]` | legal, and only types say which call is the store |
| Type-name expressions, `Box[i32]::get` and `Shipment[i32] { ... }` | ambiguous again: `values[0]::describe` has the same shape, so they need `::[` too, or a rule by name; see [Q2](#q2-type-name-expressions-under-b) | unchanged: `[` after an operand is always type arguments |
| Grids and cells, U5 and U6 | not given; a multi-key `Index` would be a separate change | given by the keys tuple |
| `Map` read type, [Special Cases Q9](SPECIAL_CASES.md#q9-map-indexing) | stays a separate question | must be decided now: `Apply` has one `Out` ([Q4](#q4-map-read-type)) |
| Pipes, `x \|> f[0]` | `f[0]` is always an index, so the placeholder exception can go | `f[0]` is always type arguments, so the exception can go too |
| Change to std and the tour | small: 3 method sites in std | 39 std index sites and the tour's indexing section |

### Agent-Writability

hd is written mostly by agents
([Roadmap, area 8](ROADMAP.md#8-tooling-for-agents)). Each option breaks
one habit, and the habits differ in strength.

| Habit an agent brings | B | C1 |
| --- | --- | --- |
| `xs[0]`, from Python, Rust, Go, TypeScript, Java, and Swift | works | a type-arguments error on a value; a fix-it gives `xs(0)` |
| `f[T](x)`, from today's hd and Go | an index of a function: an error with a fix-it to `f::[T](x)` | works |
| `f::<T>(x)`, from Rust | a `syntax-error`; the fix is one character | a `syntax-error` |
| `x.f(i)` for a field, from Go, Swift, or Scala | works as a method call only | `unknown-method`; the hint gives `(x.f)(i)` |

Indexing is far more common than explicit type arguments in the code
agents learned from. In hd's own corpus the two are close, 178 index sites
against 124 type-argument sites. No trial has measured either option; a
Haiku probe on the same tasks under B and C1 would, as AGENTS.md
"Writing hd Code" allows.

### Live Variables Without Call Indexing

The owner wants `var()` and `var() = 1`, but not `xs(0)` for arrays. The
two are separable: a live variable needs a zero-key read and store, not
call indexing. Two ways give it under B.

| | B+Z: zero-key index | B+F: callable values |
| --- | --- | --- |
| Live variable | `var[]`, `var[] = 1`, `var[] += 1` | `var()`, `var() = 1`, `var() += 1` |
| Arrays | `xs[0]` | `xs[0]` |
| Grid | `g[0, 1]` | `g[0, 1]`, or `g(0, 1)` if the type chooses |
| Mechanism | `Index` and `IndexSet` take a keys tuple, as C1's `Apply` does; `[` after an operand may hold zero or several keys | C1's `Apply[Keys]` and `Update[Keys, V]` for calls; std's `List` and `Map` do not implement them |
| New rules | grammar: `[]` and `[a, b]` after an operand; keys tuples | C1's call rules and its place exception: a call is a store when the callee has `Update` |
| Costliest kind | 1, a grammar extension, beside B's own kind 1 | 2, some calls are places |
| Precedent | Swift: `subscript()` with no parameters, read, set, and `+=` (tested, Swift 6.4) | Swift `callAsFunction` and Kotlin `invoke` read only: Swift rejects `v() = 1`, "expression is not assignable" (tested). Scala's `update` allows it. |
| Risk | `var[]` reads less well than `var()` | a library can still make its collection callable, so `xs(0)` stays possible by convention only |

B+Z needs B: today `f[]` would read as empty type arguments. B+F does not
need B; it could be added to A as well.

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

fn demo(names: List[string], var: mut Cell[i32]) -> string:
    var() = var() + 1
    names[0]
```

The block above is B+F: `names[0]` indexes, and `var()` reads the cell.

## Trying B+F

The owner chose to try B+F first (2026-09-30): `[]` indexes, `::[`
marks type arguments in expressions, and a type opts in to `var()` and
`var() = v`. This section applies it to the corpus and to the
live-variable case. Nothing here is decided.

### B+F Site Counts

Same corpus as [Counts](#counts), with the type-name forms counted by a
heuristic script on main at eac2289f.

| Site | Tour | std | Fixtures | Total | Under B+F |
| --- | --- | --- | --- | --- | --- |
| Explicit type arguments on a function | 13 | 0 | 61 | 74 | `f::[T](x)` |
| Explicit type arguments on a method | 2 | 3 | 45 | 50 | `x.m::[T]()` |
| Type-name literal, `Box[i32] { ... }` | 2 | 3 | 22 | 27 | [Q2](#q2-type-name-expressions-under-b) |
| Type-name path, `Add[i32]::add` | 0 | 0 | 11 | 11 | [Q2](#q2-type-name-expressions-under-b) |
| Index sites | 9 | 39 | 130 | 178 | unchanged |
| Live variables | 0 | 0 | 0 | 0 | new |

The first pass guessed about 100 type-name sites; the count is 38. All 11
paths are trait-qualified calls such as `Add[i32]::add(price, 5)`.

### Rewrites

`lib/std/testing.hd` puts an index and a type-argument call on one line.
Under B+F each bracket says what it is:

```text
fn pick[T < Arbitrary](c: mut Choices, examples: List[T], index: i32) -> T:
    if index >= 0: examples[index] else: c.draw::[T]()  # hypothetical syntax
```

The tour's reflection chain:

```text
fn limit() -> MaxLen:
    shape::[User]().fields.display_name.metadata::[MaxLen]()  # hypothetical syntax
```

A live variable in a local, the owner's sample with `let mut` (D4), and a
counter closure that captures it:

```text
fn demo() -> void:
    let mut var = live(0)
    println(var())
    var() = 1
    var() += 1

fn counter() -> fn() -> i32:
    let mut count = live(0)
    fn() -> i32:
        count() += 1
        count()
```

A live variable in a field. `state.count()` is a method call, so the read
takes parentheses, or a method the cell type defines, such as `get`:

```text
data AppState:
    count: mut Cell[i32]

fn bump(state: AppState) -> i32:
    (state.count)() += 1
    state.count.get()
```

The owner expects cells in fields to be rare. The corpus has no cells at
all, so it gives no evidence either way. The field case costs parentheses,
the same rule `(x.f)(args)` that stored functions use.

### What B+F Surfaced

Ranked by how much each affects the design.

| # | Finding | Effect | Question |
| --- | --- | --- | --- |
| 1 | Two keyed-access trait pairs | `Index`/`IndexSet` serve `[]` and `Apply`/`Update` serve `()`. A grid could use either, which is two mechanisms for one job. Limiting F to zero keys removes the overlap. | [Q6](#q6-arity-of-callable-values) |
| 2 | Type-name paths | `Add[i32]::add` has the shape of `values[0]::describe`, a method reference on an element. It needs `Add::[i32]::add`, or a rule. | [Q2](#q2-type-name-expressions-under-b) |
| 3 | Type-name literals | `Box[i32] { ... }` is never an index followed by a block, since hd blocks use `:`. A parser can tell it by the `{`, with no marker. | [Q2](#q2-type-name-expressions-under-b) |
| 4 | Calls as places | `var() = 1` makes a call a place when its type has `Update`. Today's `expr.assign.compound.place` (retired 2026-09-30) says a call is never one. | none; it is F's one exception |
| 5 | Cells in fields | `(state.count)()`, as for stored functions | none; the owner expects it to be rare |
| 6 | Mutable binding | `var() = 1` needs `var` to hold a `mut Cell`. `var := live(0)` is readonly by `types.bind.short`, so a writer binds `let mut var = live(0)`. | answered by D4 |

With F limited to zero keys, the traits lose their `Keys` parameter, and
Q3 and Q4 no longer apply:

```text
pub trait Apply:
    type Out
    fn apply(self) -> Self::Out

pub trait Update[V]:
    fn update(mut self, value: V) -> void
```

## Recommendation

**Recommendation.** Option B for the brackets, with callable values, B+F,
if the owner wants `var()` now. C1 is next best. Within B+F, zero keys
only ([Q6](#q6-arity-of-callable-values)) and `::[` after type names too
([Q2](#q2-type-name-expressions-under-b)) add no second mechanism and no
exception.

- The live variable does not need call indexing. B+F gives the owner's
  `var()` and `var() = 1` exactly, and arrays keep `xs[0]`.
- B keeps the `[]` mark on element reads and stores, and the indexing
  every agent already writes.
- B+F's costliest change is C1's own place exception, kind 2. It applies
  only to types that opt in with `Update`.
- B's cost is a kind 1 token pair and two spellings of type arguments,
  `List[i32]` and `first::[i32]`. Its type-name expressions need
  [Q2](#q2-type-name-expressions-under-b).

**What it gives up.** One spelling of type arguments, and the Design Cost
Order's preference for C1, whose costliest change is kind 2 against B's
kind 1. `var()` and `xs[0]` also look different for two kinds of read.

**Next best.** C1, if one bracket meaning and one spelling of type
arguments outweigh `xs(0)`. B+Z, `var[]`, if calls should never be places.

## Questions For The Owner

All seven are answered or moot; see [Owner Decisions](#owner-decisions).
Q1 decides the option. Q2, Q5, and Q6 apply only under B; Q3 and Q4
apply under C1, and Q3 under B+F only if Q6 allows keys.

### Q1. B Or C1

**Answered, 2026-09-30:** B+F (D1).

**Effect:** decides which bracket use moves: type arguments in expressions
under B, or indexing under C1.

1. **B** (recommended): `xs[0]` stays, and type arguments become
   `first::[string](xs)`. Q5 decides the live variable.
2. C1: `xs(0)`, `first[string](xs)`, and a field is indexed as
   `(user.tags)(0)`.
3. Decide after a Haiku probe writes the same tasks under both.

```text
data User:
    tags: List[string]

fn first_tag(user: User) -> string:
    (user.tags)(0)
```

### Q2. Type-Name Expressions Under B

**Answered, 2026-09-30:** `::[` after type names too (D3).

**Effect:** only under B. 38 sites write type arguments after a type name:
27 literals such as `Box[i32] { ... }` and 11 trait-qualified paths such as
`Add[i32]::add`. The path has the shape of `values[0]::describe`.

1. **`::[` after a type name too** (recommended): `Box::[i32] { ... }`
   and `Add::[i32]::add(price, 5)`. One rule for every expression, and no
   exception.
2. Literals by the `{` that follows, which no index can precede; paths
   by a rule that a method reference on an index result needs
   parentheses, `(values[0])::describe`. The 38 sites keep today's
   spelling, and B gains two exceptions.

```text
fn demo(names: List[string]) -> string:
    names[0]
```

### Q3. Keys Argument Shape

**Moot:** zero keys only (D2).

**Effect:** under C1 or B+F. A bound on a one-key type spells a
one-element tuple.

1. **A tuple-kinded `Keys`, as `Fn` has** (recommended): `Apply[(i32,)]`,
   and `g(0, 1)` passes `(0, 1)`.
2. One key type `K`: `Apply[i32]`, and a grid takes a tuple key,
   `g((0, 1))`.

```text
use std.ops.Apply

fn head[C < Apply[(i32,)]](items: C) -> C::Out:
    items(0)
```

### Q4. Map Read Type

**Moot:** C1 is not chosen. The `m[k]` read type stays open in [Syntax And Semantics Cost Q8](SYNTAX_SEMANTICS_COST.md#q8-map-read-type).

**Effect:** only under C1. Today `m[k]` reads `V?`, but `m[k] += 1` reads
`V`. Under call indexing, `Apply` has one `Out` for both.

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

### Q5. Live Variables Under B

**Answered, 2026-09-30:** B+F, `var()` (D1).

**Effect:** only under B. Decides how a cell or signal reads and stores
while arrays keep `xs[0]`.

1. **B+F, callable values** (recommended): `var()` and `var() = 1`,
   through `Apply` and `Update`; std collections do not implement them.
2. B+Z, zero-key index: `var[]` and `var[] = 1`, through a keys-tuple
   `Index`.
3. Neither for now: a cell uses methods, `var.get()` and `var.set(1)`.

```text
fn bump(var: mut Cell[i32]) -> void:
    var() += 1
```

### Q6. Arity Of Callable Values

**Answered, 2026-09-30:** zero keys only (D2).

**Effect:** only under B+F. With keys, `Apply` and `Index` both give keyed
reads, so a grid has two ways to be written.

1. **Zero keys only** (recommended): `var()` and `var() = v`; grids keep
   `g[(0, 1)]`. `Apply` and `Update` lose their `Keys` parameter, and Q3
   no longer applies.
2. Any keys, as C1's `Apply[Keys]`: `g(0, 1)` is possible, and a type
   picks `[]` or `()`.

```text
fn bump(var: mut Cell[i32]) -> void:
    var() += 1
```

### Q7. Does A Live Variable Need `mut`?

**Answered, 2026-09-30:** yes, keep `mut` (D4). The escape hatch moves to
its own brainstorm.

**Effect:** `Update` as sketched takes `mut self`, so `var() = 1` needs a
`mut Cell`, and every holder spells `mut`. The owner is unsure about that,
and wrote: "i'm pretty sure i want one thing, an escape hatch to get mut
from readonly."

This is a core question about [Access Permission](../spec/04-type-system.md#access-permission),
wider than call syntax, and it needs its own brainstorm. Candidate
answers:

1. **Keep `mut`.** `update(mut self, ...)`; a write shows in the holder's
   type, which is what `mut` is for
   ([`types.path.mutation.forms`](../spec/04-type-system.md#r-types.path.mutation.forms)).
   No escape hatch.
2. **One interior-mutable std type, as Rust's `Cell`.** A lang item whose
   `update` takes a readonly `self`, so a readonly holder may write. Rust:
   cell types "may be mutated through shared references", and "interior
   mutability is something of a last resort". A compiler intrinsic, kind 3.
3. **A general escape hatch**, an intrinsic that turns a readonly view of
   any value into `mut`. It serves every case, and it removes the readonly
   guarantee everywhere.

Option 1 was chosen. Options 2 and 3 remain inputs to the separate
escape-hatch brainstorm.

```text
fn bump(var: mut Cell[i32]) -> void:
    var() += 1
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
- Rust, `std::cell`, interior mutability:
  <https://doc.rust-lang.org/std/cell/index.html>
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
| 1 | Owner Decisions: `let mut` and a readonly child | parses |
| 2 | Problem: the owner's sample | parses |
| 3 | Option A | parses |
| 4 | Option B, `::[T]` | `syntax-error` at line 4, on a line marked `# hypothetical syntax` (lines 4 and 5 are both marked) |
| 5 | Option C, U1, U2, U4 | parses |
| 6 | `Apply` and `Update` traits | parses |
| 7 | Grid, two keys | parses |
| 8 | Cell, zero keys | parses |
| 9 | Map `tally` | parses |
| 10 | Compound desugaring | parses |
| 11 | Invalid places | parses; the errors are semantic |
| 12 | `user.tags(0)` today | parses; the error is semantic |
| 13 | C1 parentheses | parses |
| 14 | `interpolate` under C1 | parses |
| 15 | `fill`, map store and call | parses |
| 16 | `first` and `last` | parses |
| 17 | Cart store through an index | parses |
| 18 | Call result indexed | parses |
| 19 | Evaluation-order fixture | parses |
| 20 | B+F cell beside an index | parses |
| 21 | B+F: an index and type arguments on one line | `syntax-error` at line 2, on a line marked `# hypothetical syntax` |
| 22 | B+F: reflection chain | `syntax-error` at line 2, on a line marked `# hypothetical syntax` |
| 23 | B+F: a local live variable and a counter | parses |
| 24 | B+F: a live variable in a field | parses |
| 25 | Zero-key `Apply` and `Update` | parses |
| 26 | Q1 | parses |
| 27 | Q2 | parses |
| 28 | Q3 | parses |
| 29 | Q4 | parses |
| 30 | Q5 | parses |
| 31 | Q6 | parses |
| 32 | Q7 | parses |

Reference-parser finding: none. The parser accepts `f(x) = v` and
`(x.f)(k) = v` today, as `grammar.stmt.assign-target` (retired 2026-09-30)
leaves the place check to semantics.
