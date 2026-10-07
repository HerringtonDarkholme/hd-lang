# Expressions

Status: language specification draft.

This chapter defines expressions, which compute values.

1. r[expr.static-type] Every expression has a static type.
2. r[expr.evaluated] Every expression is evaluated according to the order defined here and in the relevant feature chapter.

```hd
fn demo(x: i32) -> i32:
    x * 2 + 1
```

## Evaluation Order

This section defines the default order in which subexpressions are evaluated.

1. r[expr.order.left-to-right] Unless a construct states otherwise, subexpressions are evaluated from left to right and exactly once.
2. r[expr.order.applies] This rule applies to tuple and collection elements, data fields, call arguments, operands, and indexing expressions.
3. r[expr.order.conditional] `&&`, `||`, `if`, `match`, loops, optional or result propagation with `?`, and comprehension filters evaluate conditionally as described below.
4. r[expr.order.reorder] A compiler may reorder only when it can prove that the program's observable behavior is unchanged.

```hd
fn demo() -> string:
    let log: mut List[string] = []
    fn mark(s: string) -> string:
        log.push(s)
        s
    _ := "${mark("a")}${mark("b")}"
    log.join(",")   # "a,b": left to right
```

## Expression Categories

This section defines value expressions and place expressions.

1. r[expr.category.value] A **value expression** produces a value.
2. r[expr.category.place] A **place expression** identifies a storage location and may be read or, when permissions allow, assigned.

```hd
fn demo() -> void:
    let x = +1   # x is a place expression; x * 2 is a value expression
    _ := x * 2
```

### Places

The core place expressions are locals, fields, index operations, and
calls through a callable value that accepts a store:

1. r[expr.place.local] A reassignable local introduced by `let` is a core place expression.
2. r[expr.place.field] A field selected through a mutable composite root is a core place expression.
3. r[expr.place.index] An index operation whose receiver and indexing protocol expose mutable storage is a core place expression.
4. r[expr.place.receiver] The receiver of a field or index place may be any expression of mutable composite type, including a call that returns mutable access.
5. r[expr.place.call-update] A call is a place only when its callee's type implements `Update`, as [Callable Values](#callable-values) defines.
6. r[expr.place.value-forms] `:=` bindings, literals, other calls, arithmetic, and temporary values are not places.
7. r[expr.place.tuple-element] A tuple element selection such as `pair._0` is not a place: tuples are immutable, and a changed tuple is built as a new tuple value.
8. r[expr.place.enum-shared-field] A shared enum field such as `status.phrase` is not a place: enum values never change once built.

```hd
fn demo() -> void:
    let x = +1
    x = x + 1   # x is a place: a reassignable local
```

### Assignment

1. r[expr.assign.statement] Assignment is a statement and requires a mutable place on its left side.
2. r[expr.assign.target] Assigning to an expression that is not a place is an error. Error: `invalid-assignment-target`.
3. r[expr.assign.order] Assignment evaluates the place before the right-hand side, then performs one store.
4. r[expr.assign.order.local] A local place requires no subexpression evaluation.
5. r[expr.assign.order.field] A field assignment evaluates its receiver, then the right-hand expression.
6. r[expr.assign.order.index] An indexed assignment evaluates the receiver, the index, and the right-hand expression in that order.
7. r[expr.assign.order.call] An assignment through a call evaluates the callee, then the right-hand expression.
8. r[expr.assign.abrupt] If any step completes abruptly, no store occurs.

```text
fn invalid() -> (i32, i32):
    let pair: (i32, i32) = (0, 0)
    pair._0 = 1  # error: invalid-assignment-target
    pair
```

> **Note.** An embedded field is stored with the copy assignment
> `place ...= value`, as [`data.part.construct`](08-data-and-enums.md#r-data.part.construct)
> states.

See also: [Data Embedding](08-data-and-enums.md#data-embedding).

#### Compound Assignment

A **compound assignment** `place op= value` combines an operator with a
store. For every type, `p op= e` means `p = p op e`:

```text
use std.ops.Add

type Meters(i64)

impl Add for Meters:
    type Out = Meters
    fn add(self, rhs: Meters) -> Meters:
        Meters(i64(self) + i64(rhs))

data Tally:
    count: i64

fn record(tally: mut Tally, hits: List[i64]) -> Meters:
    let log = ""
    let walked = Meters(0)
    for hit in hits:
        tally.count += hit
        walked += Meters(hit)
        log += "."
    walked
```

1. r[expr.assign.compound.form] Compound assignment is a statement, written with one of ten operators: `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, and `>>=`.
2. r[expr.assign.compound.no-others] There is no `**=`, `&&=`, or `||=`.
3. r[expr.assign.compound.place-form] The left side must have the form of a place. That is a name, a field selection, an index expression, or a call whose callee's type implements `Update`. Any other left side, such as a function call, is an error. Error: `invalid-assignment-target`.
4. r[expr.assign.compound.once] The receiver and index of the place are evaluated once, then the right-hand expression.
5. r[expr.assign.compound.meaning] For every type, `p op= e` means `p = p op e`: it reads the place, applies the operator to that value and `e`, and stores the result in the place.
6. r[expr.assign.compound.primitive] When the place and the right operand have primitive types, `p op= e` computes `p op e` by the built-in rules and stores the result in the place. The store follows the rules of `p = p op e`, so a name must be a reassignable local.
7. r[expr.assign.compound.operator] Otherwise the operator follows [Operator Traits](#operator-traits), so the place's type needs the operator's trait. Without a fitting implementation, the statement is an error. Error: `type-mismatch`.
8. r[expr.assign.compound.store-rules] The store follows the rules of assignment. So the place must be a reassignable local, a field selected through a mutable root, or an index place that accepts a store. A call place also qualifies.
9. r[expr.assign.compound.index-read-write] On an index place, the read is `r[k]` and the store is `r[k] = v`. `List` and `Map` use their built-in indexing, and another type needs both `Index` and `IndexSet`, as [Index Traits](#index-traits) defines.
10. r[expr.assign.compound.call-once] On a call place `v() op= e`, the callee `v` is evaluated once, then `e`.
11. r[expr.assign.compound.call-read-write] On a call place, the read is `v()` and the store is `v() = x`. So the callee's type needs both `Apply` and `Update`, as [Callable Values](#callable-values) defines.
12. r[expr.assign.compound.fresh-value] On a composite value, the store replaces the place's value with the operator's result. Other references to the old value keep the old value.
13. r[expr.assign.compound.no-assign-traits] `std.ops` declares no assign trait, and no operator changes a value in place.

```text
data Tally:
    count: i64

data Score:
    points: i64

fn current() -> i64: 1

fn invalid(tally: Tally, bonus: Score) -> void:
    let score = Score { points: 0 }
    tally.count += 1  # error: readonly-root
    score += bonus    # error: type-mismatch
    current() += 1    # error: invalid-assignment-target
```

```text
fn tally(counts: mut Map[string, i32], word: string) -> void:
    counts[word] += 1  # panics with index-out-of-bounds when `word` is absent
```

> **Note.** A diagnostic that rejects `p op= e` may show `p = p op e` as a
> hint. That hint is not normative.

> **Why.** One meaning for every type means an alias never observes `+=`:
> a `data` value in the place is replaced, never changed. Accumulators and
> builders change themselves through ordinary methods, such as
> `sb.push(x)`.

See also: [Operator Traits](#operator-traits),
[Callable Values](#callable-values),
[Mutation Checks](04-type-system.md#mutation-checks).

## Primary Expressions

This section defines names, literals, and parenthesized, tuple, collection,
and data expressions.

### Names

This section defines identifier, qualified, and contextual variant names.

1. r[expr.name.identifier] An identifier expression evaluates the declaration or local binding selected by lexical name resolution.
2. r[expr.name.qualified] A qualified name selects a declaration or enum variant through a module or type namespace.
3. r[expr.name.qualified.private] A module path to a declaration that its module declares without `pub` is an error, as a use of it would be. Error: `private-import`.
4. r[expr.name.qualified.missing] A module path to a declaration that its module does not declare is an error, as a use of it would be. Error: `unknown-import`.
5. r[expr.name.contextual] `.Variant` selects a variant only when the expression has an expected type that fixes one nominal enum.
6. r[expr.name.contextual.rules] `.Variant` has the same construction and argument rules as `Enum.Variant`; a payload-bearing variant still requires a call.
7. r[expr.name.contextual.no-search] The compiler does not search all visible enums for a matching variant name.
8. r[expr.name.contextual.no-type] Without a unique expected enum type, `.Variant` is a type error. Error: `missing-contextual-enum-type`.

```text
enum Status:
    Queued

fn invalid() -> void:
    status := .Queued  # error: missing-contextual-enum-type
    pass
```

```text
use pkg.words

fn shout(word: string) -> string:
    words.squash(word)                # error: private-import

fn headline(titles: List[string]) -> string:
    words.join_wordz(titles, " ")     # error: unknown-import
```

> **Note.** A `std` module is no exception. A module path to a `std`
> declaration without `pub` is `private-import` by
> `expr.name.qualified.private`, as for a module of the package.

See also: [Enum Declarations](08-data-and-enums.md#enum-declarations),
[Use Forms](10-modules.md#use-forms), [Use Roots](10-modules.md#use-roots).

### Literals

1. r[expr.literal.defined] Boolean, integer, floating-point, string, and character literals are defined lexically in [Lexical Structure](01-lexical-structure.md) and typed in [Type System](04-type-system.md).
2. r[expr.literal.call] A suffixed literal or prefixed string is a call, as [Literal Suffixes](#literal-suffixes) specifies.
4. r[expr.literal.pass] `pass` is the no-op expression. It has type `void` and performs no operation.

```hd
use std.time.s

fn demo() -> void:
    _ := 5s   # a suffixed literal is a call of the suffix function s
```

#### String Interpolation

An interpreted string may embed expressions, which are formatted through the
`Display` trait. The canonical signature is:

```text
trait Display:
    fn to_string(self) -> string
```

1. r[expr.interp.forms] Interpreted strings support `$name`, `$self`, and `${expression}` interpolation.
2. r[expr.interp.self] `"value: $self"` inside a method interpolates the receiver.
3. r[expr.interp.order] Embedded expressions are evaluated from left to right at the position of their segment.
4. r[expr.interp.display] Each embedded expression's type must implement the canonical `std.format.Display` trait.
5. r[expr.interp.append] The compiler appends the string returned by that implementation.
6. r[expr.interp.no-display] An embedded expression whose type does not implement `Display` is an error. Error: `unsatisfied-trait-bound`.
7. r[expr.interp.no-fallback] There is no fallback conversion through `Any`, runtime reflection, or debug output.
8. r[expr.interp.std] The standard library provides `Display` implementations for ordinary printable primitive types and `string`.
9. r[expr.interp.std.tuple.template] It also declares a tuple template for `Display`, so every tuple whose elements implement `Display` implements it, at every size. It writes the elements' texts inside parentheses, separated by `, `, as in `(1, a)`.
10. r[expr.interp.std.tuple.rest-inline] For a tuple with a rest element `List[T]...`, `T` must implement `Display`. The rest's items are written inline after the fixed elements, as in `(1, 2, 3, 4)`.
11. r[expr.interp.std.tuple.one] A tuple of one element writes a comma after it, as its source syntax does: `(1,)`. A rest tuple counts each of its rest's items as an element.
12. r[expr.interp.user] Optional and user-defined values are displayable only when the corresponding type implements `Display`.
13. r[expr.interp.prefixed] A prefixed string does not append its values: they become the values of a template, as [Prefixed Strings](#prefixed-strings) specifies.
14. r[expr.interp.constant] A string with no interpolation segments is an ordinary constant value and performs no `Display` calls.

> **Note.** The empty tuple writes `()`. A `void` value is that tuple, so
> `"${log()}"` writes `()` too.

```text
data Secret:
    value: i32

fn render(value: Secret) -> string:
    "secret=$value"  # error: unsatisfied-trait-bound
```

#### Literal Suffixes

A [suffixed literal](01-lexical-structure.md#literal-suffixes) or a
[prefixed string](01-lexical-structure.md#prefixed-strings) is a call of a
**literal function**. A literal function is marked `@num_suffix` for a
suffix, or
`@str_prefix` for a prefix. This section gives the rules both forms share,
then the suffix rules; [Prefixed Strings](#prefixed-strings) adds the
template.

```text
use std.ops.num_suffix

data Pixels:
    count: i32

@num_suffix
fn px(count: i32) -> Pixels:
    Pixels { count: count }

fn indent() -> Pixels:
    12px  # px(12)
```

`std.ops` declares the marker as a
[typed fact type](14-annotations.md#member-typed-facts):

```text
use std.annotation.annotate
use std.num.Num

@annotate::[F](.Fn)
pub data NumSuffix[F]: pass

pub fn num_suffix[N < Num, R, $Q]() -> NumSuffix[fn(N) -> R $ Q]:
    NumSuffix::[fn(N) -> R $ Q] {}
```

Rules for every literal function:

1. r[expr.literal-fn.marker] A **suffix function** carries a `std.ops.NumSuffix` value, written `@num_suffix`, and a **prefix function** carries a `std.ops.StrPrefix` value, written `@str_prefix`. The compiler recognizes both types, and `std.ops.Template`, by their qualified names.
2. r[expr.literal-fn.fn-only] `std.ops` declares `NumSuffix`, `num_suffix`, `StrPrefix`, `str_prefix`, and `Template`. Both marker types list only `.Fn` in their `@annotate`, so either marker before anything but a function is an error. Error: `decorator-target-kind`.
3. r[expr.literal-fn.not-marked] A suffix or prefix that resolves to anything but a literal function of its form is an error at the literal. Error: `invalid-literal-suffix` for a suffix, `invalid-string-prefix` for a prefix.
4. r[expr.literal-fn.no-marker-import] The call needs no import of the marker, its type, or `Template`: a `use` of the literal function alone makes the literal valid.
5. r[expr.literal-fn.ordinary-call] A literal is exactly the call of its literal function wherever it appears, and it is checked and evaluated as an ordinary call there. So argument errors, inferred type arguments, and the rules of a [fact](14-annotations.md#r-annot.fact.eval) or [shared enum data](08-data-and-enums.md#r-data.shared.compile-time) position all apply as for any call.
6. r[expr.literal-fn.row] A literal function may have a requirement row, and a literal of it needs that row where it appears, as any call does, by [`req.row.set.call`](11-requirements-and-suspension.md#r-req.row.set.call). Error: `missing-requirement`.

Each marker is a typed fact, checked by
[`annot.typed-fact.check`](14-annotations.md#r-annot.typed-fact.check):

| Function | `@num_suffix` checks like |
| --- | --- |
| `fn ms(n: i64) -> Millis` | `let f: NumSuffix[fn(i64) -> Millis] = num_suffix()` |
| `fn px(count: i32) -> Pixels $ Console` | `let f: NumSuffix[fn(i32) -> Pixels $ Console] = num_suffix()` |
| `fn k[N < Num](n: N) -> N` | `let f: NumSuffix[fn(Num) -> Num] = num_suffix()`, with one fixed `Num` type |

`num_suffix`'s signature then holds every shape constraint. A suffix
function takes one parameter of a `Num` type and never suspends. One that
does not fit is an error at its decorator. A generic one has its type
made monomorphic first, by
[`annot.typed-fact.monomorphic`](14-annotations.md#r-annot.typed-fact.monomorphic).

Its requirement row may be any row. `Q` is a
[row parameter](11-requirements-and-suspension.md#r-req.row.param.marked),
since it is declared `$Q`, and so a generic parameter
([`req.row.parameter`](11-requirements-and-suspension.md#r-req.row.parameter)).
The check infers it from the expected type, as it infers `N` and `R`, by
[`annot.typed-fact.check.inferred`](14-annotations.md#r-annot.typed-fact.check.inferred).
So for `fn px(count: i32) -> Pixels $ Console`, `Q` is `$ Console`.

Rules for suffixes:

7. r[expr.suffix.fn-call] A suffixed literal `Nx` is the call `x(N)` of the suffix function `x`, with the literal `N` as its one argument, so `250ms` means `ms(250)`.

```text
use std.ops.num_suffix

pub data Pixels:
    count: i32

fn pt(count: i32) -> Pixels:
    Pixels { count: count }

@num_suffix  # error: unsatisfied-trait-bound
fn em(label: string) -> Pixels:
    Pixels { count: 0 }

@num_suffix  # error: type-mismatch
fn later!(count: i64) -> i64:
    count

@num_suffix  # error: type-mismatch
fn kb(count: i64, unit: i64 = 1024) -> i64:
    count * unit

@num_suffix
fn px(count: i32) -> Pixels $ Console:
    Pixels { count: count }

@num_suffix  # error: unsatisfied-trait-bound
fn same[N](n: N) -> N:
    n

pub fn layout() -> Pixels:
    size := 12pt  # error: invalid-literal-suffix
    12px  # error: missing-requirement
```

> **Note.** The one parameter may have a default, since a default is not
> part of a [function type](07-functions.md#r-fn.type.no-names). A
> literal always passes its own value, so in
> `@num_suffix fn unit(count: i64 = 1)`, `5unit` passes `5`, and the
> ordinary call `unit()` uses the default. A second parameter does not
> fit, even with a default.

> **Note.** A suffix function may be generic, as in
> `@num_suffix fn k[N < Num](n: N) -> N`. A literal infers its type
> argument as any generic call does: `5k` takes `N` from the literal, or
> from the expected type, as in `let limit: i64 = 5k`. A unary minus is not part of the
> literal, so `-5s` is `-(5s)`.

A suffixed literal in shared enum data is a call there, as the position
rules say:

```text
use std.ops.num_suffix

data Millis:
    count: i64

@num_suffix
fn ms(count: i64) -> Millis:
    Millis { count: count }

enum Tier(limit: Millis):
    Fast -> Tier(limit=250ms)
    Slow -> Tier(limit=5_000ms)
```

> **Why.** A suffix is an ordinary function found through `use`, so
> libraries can add `12px` without new syntax. The literal's type is the
> function's result, so `12px` is a `Pixels`. A suspending function does
> not fit `num_suffix`'s result `NumSuffix[fn(N) -> R $ Q]`. hd marks
> every suspending call with `!`, and `5s` has no place to show it. A
> requirement row needs no mark at the call, so a literal function may
> have one. The marker is checked once, at the decorator, not at each
> literal.

> **Note.** A compiler may warn when a suffixed literal always overflows,
> as Rust's `unconditional_panic` lint does, but none is required.

> **Note.** The standard library's duration suffixes `ms`, `s`, `min`, and
> `h` of `std.time`, and its `Duration` type, are stdlib tier:
> [Time](../std/time.md).

See also: [Suffixed Literals](04-type-system.md#suffixed-literals),
[Literal Suffix Names](03-names-and-scopes.md#literal-suffix-names),
[Target Kinds](14-annotations.md#target-kinds).

#### Prefixed Strings

A [prefixed string](01-lexical-structure.md#prefixed-strings) is a call of
its prefix function, a function marked `@str_prefix`, with a template of its
text and values:

```text
use std.ops.{Template, str_prefix}

data Query:
    text: List[string]
    params: List[i64]

@str_prefix
fn sql(t: Template[i64]) -> Query:
    Query { text: t.raw_parts, params: t.values }

fn by_id(id: i64) -> Query:
    sql"select * from users where id = $id"
```

`std.ops` declares the marker and the template:

```text
use std.annotation.annotate

@annotate::[F](.Fn)
pub data StrPrefix[F]: pass

pub fn str_prefix[T, R, $Q]() -> StrPrefix[fn(Template[T]) -> R $ Q]:
    StrPrefix::[fn(Template[T]) -> R $ Q] {}

pub data Template[T]:
    pub raw_parts: List[string]
    pub values: List[T]
```

The [rules for every literal function](#literal-suffixes) apply to a
prefix function. `@str_prefix` before `fn sql(t: Template[i64]) -> Query`
checks like `let f: StrPrefix[fn(Template[i64]) -> Query] = str_prefix()`,
and its row parameter `Q` takes the empty row. The rules for prefixes:

1. r[expr.prefix.fn-call] A prefixed string `x"..."` is the call `x(t)` of the prefix function `x`, with a `std.ops.Template` value `t` as its one argument. The compiler never joins the pieces and never calls `Display`.
2. r[expr.prefix.template] `t.values` holds the `n` interpolated values in source order, and `t.raw_parts` holds the `n + 1` pieces of text around them.
3. r[expr.prefix.parts] The first piece is the text before the first interpolation, and the last piece is the text after the last one. A piece is `""` where two interpolations touch, or where one begins or ends the string. So `x"a $b c"` passes the pieces `["a ", " c"]` and the values `[b]`, and `x"text"` passes `["text"]` and `[]`.
4. r[expr.prefix.raw-parts] Each piece is the text exactly as written, with every backslash kept, as [`lex.prefix.raw-text`](01-lexical-structure.md#r-lex.prefix.raw-text) says.
5. r[expr.prefix.order] The interpolated expressions are evaluated from left to right before the call, as arguments are.

```text
use std.ops.{Template, str_prefix}

fn plain(t: Template[string]) -> string:
    "plain"

@str_prefix  # error: type-mismatch
fn count(n: i32) -> i32:
    n

@str_prefix  # error: type-mismatch
fn later!(t: Template[string]) -> string:
    "later"

@str_prefix  # error: type-mismatch
fn tagged(t: Template[string], tag: string = "x") -> string:
    tag

@str_prefix
fn logged(t: Template[string]) -> string $ Console:
    "logged"

pub fn render() -> string:
    first := plain"x"  # error: invalid-string-prefix
    logged"x"  # error: missing-requirement
```

> **Why.** A prefix is an ordinary function found through `use`, so a
> library adds `sql"..."` without new syntax. The template keeps text and
> values apart, so `sql` can send values as parameters instead of splicing
> them into the query text. As for a suffix, the string is plain call
> sugar and never suspends, because `sql"..."` has no place for `!`.

> **Note.** The standard library's one prefix, the raw-text `r` of
> `std.text`, is stdlib tier: [Raw Text Prefix](../std/text.md#raw-text-prefix).

See also: [Prefixed Strings](04-type-system.md#prefixed-strings),
[String Prefix Names](03-names-and-scopes.md#string-prefix-names),
[Prefixed Strings](01-lexical-structure.md#prefixed-strings).

### Parenthesized And Tuple Expressions

Parentheses group an expression, and a comma constructs a tuple:

```text
value := (a + b) * c
```

```text
empty := ()
single := (1,)
point := (10, 20)
```

1. r[expr.paren.group] Parentheses group one expression without changing its value.
2. r[expr.tuple.comma] A comma constructs a tuple.
3. r[expr.tuple.single] A one-element tuple requires a trailing comma.
4. r[expr.tuple.order] Tuple elements evaluate left to right.
5. r[expr.tuple.select-underscore] Tuple selection uses the member `_` followed by the zero-based element index, such as `point._0` and `point._1`.
6. r[expr.tuple.select-underscore.identifier] That member name is an ordinary identifier, so `point._0._1` selects through a nested tuple like any other member chain.

> **Why.** An identifier removes the floating-point ambiguity of `t.0.1`, and
> works wherever a member does, as in `"${point._0}"`.

See also: [Bang And Dot Tokens](02-grammar.md#bang-and-dot-tokens), which
rejects the former spelling `point.0`.

#### Tuple Rest Elements

A tuple expression fills a [rest element](04-type-system.md#rest-elements)
as a call fills a vararg: it collects separate elements, or it spreads a
list:

```text
fn total(values: (usize, usize, List[i32]...)) -> usize:
    values._0 + values._1 + values._2.len()

fn run(xs: List[i32]) -> usize:
    let t: (usize, usize, List[i32]...) = (1, 2, 3, 4)
    total(t) + total((1, 2)) + total((1, 2, xs...))
```

1. r[expr.tuple.rest.collect] Against an expected tuple type with a rest element `List[T]...`, a tuple expression's elements fill the fixed elements one each, in order. The elements after them are collected into the rest element, each checked against `T`. Too few elements is an error. Error: `type-mismatch`.
2. r[expr.tuple.rest.spread] A spread `xs...` that ends a tuple expression supplies the rest element. The elements before it are the fixed elements, and `xs` is the rest element's list. Against an expected type, both must match it. Error: `type-mismatch`.
3. r[expr.tuple.rest.spread.list] The spread operand must be a `List[T]`, which gives the rest element `List[T]...`. A tuple or any other operand is an error. Error: `type-mismatch`.
4. r[expr.tuple.rest.value] The resulting tuple's rest element is a `List[T]` that holds the collected elements in order, or the spread operand's list.

```text
fn short() -> (i32, i32, List[i32]...):
    (1,)  # error: type-mismatch

fn joined(pair: (i32, i32)) -> (i32, i32, i32):
    (1, pair...)  # error: type-mismatch
```

> **Why.** One collection rule serves calls and tuple expressions, so a
> value of a function's inputs tuple is written as its call's arguments
> are. A tuple is never spread into another: there is no tuple
> concatenation.

### List And Map Expressions

A list literal produces a list, and a map literal produces a map:

```text
names := ["Ada", "Grace"]
```

```text
scores := {"Ada": 10, "Grace": 12}
```

#### List Literals

1. r[expr.list.order] A list literal evaluates its elements left to right.
2. r[expr.list.type] A list literal produces `List[T]`, where every element is assignable to `T`.
3. r[expr.list.spread] A list element ending in `...` is a spread.
4. r[expr.list.spread.once] A spread's operand is evaluated once, in element order.
5. r[expr.list.spread.type] A spread's operand must have a list type `List[U]`. Error: `type-mismatch`.
6. r[expr.list.spread.insert] The spread's elements are inserted at its position, in order.
7. r[expr.list.spread.position] A literal may contain several spreads in any position, so `[0, xs...]` and `[xs..., ys...]` are valid.
8. r[expr.list.spread.expected] With an expected `List[T]`, `U` must be assignable to `T`.
9. r[expr.list.spread.inferred] Without an expected `List[T]`, a spread contributes `U` to the least common type of the elements.

```text
fn numbers(count: i32) -> List[i32]:
    [0, count...]  # error: type-mismatch
```

#### Map Literals

1. r[expr.map.order] A map literal evaluates each key and then its value, processing entries from left to right.
2. r[expr.map.duplicate] If two evaluated entries have equal keys, the later value replaces the earlier value without changing that key's first insertion position.
3. r[expr.map.iteration] Map iteration is in insertion order and is not part of map equality.
4. r[expr.map.keys] Key validity and equality/hash requirements are defined in [Type System](04-type-system.md#map-key-types).

```hd
fn demo() -> Map[string, i32]:
    {"a": +1, "b": +2}
```

#### Element Types

1. r[expr.collection.empty] Empty `[]` and `{}` literals require an expected collection type.
2. r[expr.collection.empty.error] Without an expected collection type, an empty literal is an error. Error: `cannot-infer-type`.
3. r[expr.collection.expected] When an expected `List[T]` or `Map[K, V]` type is available, each literal element is checked directly against the corresponding expected type.
4. r[expr.collection.inferred] Without an expected type, a list's element type is the [least common type](04-type-system.md#least-common-type) of its elements. A map's key type and value type are the least common types of its keys and of its values.
5. r[expr.collection.inferred.rows] Function values with different rows take the union of their rows first, as [Row Union In Literals](11-requirements-and-suspension.md#row-union-in-literals) states.

```text
fn main() -> void:
    values := []  # error: cannot-infer-type
    others := {}  # error: cannot-infer-type
```

> **Why.** An empty literal contains no values from which to infer type
> arguments.

### Data Expressions

A data expression names its type and provides every required field:

```text
user := User {
    id: "user_123",
    email: "ada@example.com",
}
```

1. r[expr.data.required] A data expression names its type and provides every required field.
2. r[expr.data.once] Every field may appear at most once.
3. r[expr.data.eval] Field initializers evaluate in source order, not declaration order.
4. r[expr.data.shorthand] A field written as its bare name, as in `Point { x, y }`, is field shorthand for `name: name`. Its initializer is a use of that value name, evaluated in its source position.

> **Note.** An embedded field is filled with a copy marker, as in
> `Timestamps: ...stamps`, which stores the copy-update
> `Timestamps { ...stamps }`, as
> [`data.part.construct`](08-data-and-enums.md#r-data.part.construct) states.

See also: [Data Literals](08-data-and-enums.md#data-literals) for field order and
unknown fields, and [Data Embedding](08-data-and-enums.md#data-embedding).

#### Copy-Update Expressions

Copy-update syntax begins with one spread followed by explicit replacements:

```text
renamed := User {
    ...user,
    display_name: "Ada",
}
```

1. r[expr.update.form] Copy-update syntax begins with one spread followed by explicit replacements.
2. r[expr.update.one-spread] A data expression permits at most one data spread, and it must precede every explicit field.
3. r[expr.update.spread-first] The spread expression is evaluated first.
4. r[expr.update.exact-type] The spread expression must have the exact data type being constructed.
5. r[expr.update.access-view] For each field not explicitly replaced, the copied value is type-checked as a read through the spread source's access view.
6. r[expr.update.readonly-source] A readonly source reads a direct `mut U` field as `U`.
7. r[expr.update.readonly-fill] That `U` value can fill the same field in a readonly result, but not in a `mut` result without an explicit `mut U` replacement.
8. r[expr.update.generic] Generic fields retain their substituted type, even when it is `mut U`.
9. r[expr.update.mutable-source] A mutable source retains direct fields' declared permissions.
10. r[expr.update.shallow] Copy-update is shallow: primitive fields are copied by value, while composite field references continue to refer to the same underlying objects.
11. r[expr.update.no-upgrade] A fresh mutable outer result does not upgrade copied child references.

> **Note.** Embedded parts are the exception to a shallow copy. Each part
> that is not replaced is copied as a copy-update of its own, as
> [`data.part.copy-update`](08-data-and-enums.md#r-data.part.copy-update)
> states.

See also: [Copy-Update Literals](08-data-and-enums.md#copy-update-literals),
[Mutable Edges](08-data-and-enums.md#mutable-edges).

## Postfix Expressions

1. r[expr.postfix.binding] Postfix operations bind more tightly than every infix operator.

```hd
fn demo(items: List[i32]) -> usize:
    items.len() + 1   # .len() binds tighter than +
```

### Member Access

`value.member` selects a member, and `tuple._0` selects a tuple element.

1. r[expr.member.select-identifier] `value.member` selects a member named by an identifier, and `tuple._0` selects a tuple element.
2. r[expr.member.field-read] Without an argument clause, `value.name` reads a field.
3. r[expr.member.lookup] [Member Resolution](03-names-and-scopes.md#member-resolution) defines field and method lookup, including promoted members and the declaration-time check `ambiguous-promoted-member`.
4. r[expr.member.enum-variant] `Enum.Variant` is not member access: it names an enum variant.
5. r[expr.member.variant-fn-value] A variant constructor with exactly one payload field is a function value.

```hd
data Point:
    x: i32
    y: i32

fn demo(p: Point) -> i32:
    p.x + p.y
```

See also: [Enum Declarations](08-data-and-enums.md#enum-declarations).

#### Method Calls

1. r[expr.member.method-call] When a member suffix is immediately followed by an argument clause, `value.name(arguments...)` is a method call. Member lookup selects a method, which is called with `value` as its receiver.
2. r[expr.member.no-field-call] A method call never reads a field.
3. r[expr.member.stored-fn] A function stored in a field is called by parenthesizing the field read, as in `(handler.callback)(event)`.
4. r[expr.member.stored-fn.error] `handler.callback(event)` looks for a method named `callback`, and is an error when there is none. Error: `unknown-method`.
5. r[expr.member.stored-fn.hint] When a field named `callback` exists, the `unknown-method` message should suggest `(handler.callback)(event)`.
6. r[expr.member.trait-available] A method call never finds a method of a trait that is not available at the call, even when the receiver's type implements it.
7. r[expr.member.trait-hint] When such a method is the only one with the name, the `unknown-method` message should suggest a use declaration for its trait.
8. r[expr.member.method-not-value] A method is not a value: `value.method` without an argument clause is a field read.
9. r[expr.member.method-references] A method is used as a value through a `::` [method reference](07-functions.md#method-references), `Type::name` or `value::name`.
10. r[expr.member.closure-adapt] Explicit closures can adapt method calls where a function value is needed.

```text
data Button:
    on_click: fn(i32) -> i32

fn invalid(button: Button) -> i32:
    button.on_click(41)  # error: unknown-method
```

See also: [Method References](07-functions.md#method-references).

#### Readonly Roots

1. r[expr.member.readonly-root] Member access through a readonly data root weakens a direct `mut U` field to `U` and an embedded field to readonly access.
2. r[expr.member.readonly-generic] Member access through a readonly data root does not weaken a generic field's substituted type.
3. r[expr.member.other-forms] Other member forms follow their own access rules in [Type System](04-type-system.md).

```hd
data Shelf:
    content: mut List[i32]

fn demo(shelf: Shelf) -> usize:
    shelf.content.len()   # readonly root: content reads as List[i32]
```

### Indexing

1. r[expr.index.order] `receiver[index]` evaluates the receiver, then the index, and invokes the receiver type's indexing behavior.
2. r[expr.index.expected] For a `List` or `string` receiver, the index is checked with `usize` as its expected type, so an integer literal index is a `usize`.
3. r[expr.index.expected.range] For such a receiver, an index that is a range expression gives each of its bounds `usize` as the expected type. So `items[1..3]` slices with a `Range[usize]`.
4. r[expr.index.negative-literal] A negated literal index or slice bound, as in `items[-1]` or `items[-1..]`, negates an unsigned value. It is an error. Error: `type-mismatch`.

```hd
fn demo(items: List[i32]) -> i32:
    items[0] + items[1]
```

#### List Indexing

1. r[expr.index.list.unsigned] For `List[T]`, an index must have an unsigned integer type, such as `usize`. A signed index is an error. Error: `type-mismatch`.
2. r[expr.index.list.range] A list index must be non-negative and less than the list length.
3. r[expr.index.list.panic] A failed check causes the standard checked runtime panic.
4. r[expr.index.list.read] Reading a list element yields its declared generic type `T`, including `mut U` when `T = mut U`, regardless of the list root's permission.
5. r[expr.index.list.assign] Assigning `items[index] = value` still requires a mutable list root and an in-range index.

```text
fn previous(items: List[i32], i: usize) -> i32:
    items[i - 1]  # in a debug or test build, panics with integer-overflow when i is 0

fn invalid(items: List[i32], i: i32) -> void:
    a := items[-1]  # error: type-mismatch
    b := items[i]   # error: type-mismatch
```

> **Why.** An index is unsigned, so no index counts from the end. With
> negative indices counting from the end, as in Python, `items[i - 1]` with
> `i == 0` would read the last element. Here the subtraction panics instead.

#### Map Indexing

1. r[expr.index.map.type] For `Map[K, V]`, the index must have type `K`.
2. r[expr.index.map.read-value] Reading `entries[key]` returns `V`. When the map holds no equal key, the read is a checked runtime panic. Panic: `index-out-of-bounds`.
3. r[expr.index.map.value-type] The read's type is the declared `V`, preserved through a readonly map, including `mut U` when `V = mut U`.
4. r[expr.index.map.get] The optional read is the built-in method `get`: `entries.get(key)` returns `V?`, and `.None` means no equal key exists.
5. r[expr.index.map.assign] Assigning `entries[key] = value` requires `mut Map[K, V]` and inserts or replaces the entry.
6. r[expr.index.map.library] Removal and entry APIs are standard-library methods rather than special syntax.

```text
fn score(scores: Map[string, i32], name: string) -> i32:
    match scores.get(name):
        .Some(value) => value
        .None => 0

fn strict(scores: Map[string, i32], name: string) -> i32:
    scores[name]  # panics with index-out-of-bounds when `name` is absent
```

> **Why.** A map read means one thing everywhere: `m[k]`,
> `m[k] += v`, and `Index::index` all read `V`, as a list index reads `T`.
> Rust and Python also panic or raise on a missing key and offer `get` for
> the optional read.

See also: [Built-In Methods](10-modules.md#built-in-methods).

#### String Indexing

A string index reads one byte:

```text
fn first(text: string) -> u8:
    text[0]

fn invalid(text: string, i: i32) -> void:
    text[0] = 65  # error: invalid-assignment-target
    b := text[i]  # error: type-mismatch
```

1. r[expr.index.string.byte] For `string`, `text[index]` reads the byte at that byte offset, as a `u8`, in constant time.
2. r[expr.index.string.offset] The index counts bytes, not scalar values, so `"é"[0]` is `0xC3`, the first byte of its encoding.
3. r[expr.index.string.unsigned] The index must have an unsigned integer type, such as `usize`. A signed index is an error. Error: `type-mismatch`.
4. r[expr.index.string.range] The index must be non-negative and less than the string's length in bytes.
5. r[expr.index.string.panic] A failed check causes the standard checked runtime panic, as a failed list index does.
6. r[expr.index.string.no-assign] A string is immutable, so `text[index]` is not a place. Assigning to it is an error. Error: `invalid-assignment-target`.

See also: [Strings](04-type-system.md#strings).

#### Slicing

An index of a range type selects a slice of a string or a list:

```text
fn parts(text: string, items: List[i32]) -> (string, string, List[i32], List[i32]):
    (text[0..3], text[2..], items[..2], items[1..=2])

fn whole(text: string, items: List[i32]) -> (string, List[i32], string):
    (text[..], items[..], text[..=1])
```

| Rule | Index | Selects the offsets |
| --- | --- | --- |
| r[expr.index.slice.half-open] Half-open | `a..b` | from `a` up to, not including, `b` |
| r[expr.index.slice.from] From | `a..` | from `a` up to the length |
| r[expr.index.slice.to] To | `..b` | from `0` up to, not including, `b` |
| r[expr.index.slice.inclusive] Inclusive | `a..=b` | from `a` through `b`, the same as `a..b + 1` computed without overflow |
| r[expr.index.slice.to-inclusive] To inclusive | `..=b` | from `0` through `b`, the same as `..b + 1` computed without overflow |
| r[expr.index.slice.full] Full | `..` | from `0` up to the length, so `text[..]` is the whole string and `items[..]` a copy of the whole list |

1. r[expr.index.slice.call] For a `List` or `string` receiver, an index of a range type is the call `Index::[K]::index(r, k)` of an implementation that [Built-In Implementations](#built-in-implementations) lists.
2. r[expr.index.slice.string] A string slice is the string of the bytes at the selected byte offsets.
3. r[expr.index.slice.string.shared] A string slice shares the original string's bytes rather than copying them, as [`module.string.slice.shared`](10-modules.md#r-module.string.slice.shared) states for `slice`.
4. r[expr.index.slice.string.range] A string slice whose start or end offset is greater than the length is a checked runtime panic. Panic: `index-out-of-bounds`.
5. r[expr.index.slice.string.reversed] A string slice whose start is greater than its end is a checked runtime panic. Panic: `index-out-of-bounds`.
6. r[expr.index.slice.string.boundary] A string slice whose start or end offset is not a [scalar boundary](04-type-system.md#r-types.string.boundary) is a checked runtime panic. Panic: `index-out-of-bounds`.
7. r[expr.index.slice.list] A list slice is a new list that holds the selected elements in order. It is not a view: later changes to either list do not change the other.
8. r[expr.index.slice.list.type] A list slice of a `List[T]` has type `mut List[T]`, the `Out` of its implementation, so the new list has mutable access. Its elements keep the type `T`, including `mut U` when `T = mut U`.
9. r[expr.index.slice.list.range] A list slice whose start or end offset is greater than the length is a checked runtime panic. Panic: `index-out-of-bounds`.
10. r[expr.index.slice.list.reversed] A list slice whose start is greater than its end is a checked runtime panic. Panic: `index-out-of-bounds`.
11. r[expr.index.slice.no-store] No range type has an `IndexSet` implementation for `List` or `string`, so assigning to a slice is an error. Error: `invalid-assignment-target`.
12. r[expr.index.slice.no-map] `Map` has no slicing: an index on a map is a key of type `K`, whatever its type.
13. r[expr.index.slice.unsigned] A slice's bounds must have an unsigned integer type. A range of a signed type is an error, as no implementation takes it. Error: `type-mismatch`.

```text
fn grow(items: List[i32]) -> List[i32]:
    let mut part = items[0..2]  # valid: the slice is a mut List[i32]
    part.push(9)
    part

fn invalid(items: mut List[i32]) -> void:
    items[0..2] = [7, 8]  # error: invalid-assignment-target
```

```text
fn panics(text: string, items: List[i32]) -> void:
    a := "héllo"[0..2]  # panics with index-out-of-bounds: offset 2 is inside é
    b := text[3..1]     # panics with index-out-of-bounds: start after end
    c := items[0..9]    # panics with index-out-of-bounds when items has fewer than 9
```

```text
fn invalid(items: List[i32], start: i32) -> void:
    a := items[-1..]      # error: type-mismatch
    b := items[start..]   # error: type-mismatch
```

> **Why.** A slice goes through `std.ops.Index`, as Rust's does, so generic
> code bounded by `Index[Range[usize]]` accepts strings and lists. A list
> slice is a copy, so later changes to the list never reach it. A
> string slice shares its bytes because strings are immutable. The copy's
> mutable access comes from the implementation's declared `Out`, so no
> extra freshness rule is needed.

> **Note.** A read-only window on a list with no copy is the `view` method
> of [Collections](../std/collections.md#views).

See also: [String Methods](10-modules.md#string-methods), [Range Expressions](#range-expressions).

#### Index Traits

Other types get `[]` by implementing the `std.ops` traits `Index` and
`IndexSet`:

```text
use std.ops.{Index, IndexSet}

data Ring:
    items: mut List[i32]

impl Index[usize] for Ring:
    type Out = i32
    fn index(self, key: usize) -> i32:
        self.items[key % self.items.len()]

impl IndexSet[usize, i32] for Ring:
    fn index_set(mut self, key: usize, value: i32) -> void:
        self.items[key % self.items.len()] = value

fn rotate(ring: mut Ring) -> i32:
    ring[5] = ring[0]
    ring[5]
```

`std.ops` declares them in this shape:

```text
pub trait Index[K]:
    type Out
    fn index(self, key: K) -> Self::Out

pub trait IndexSet[K, V]:
    fn index_set(mut self, key: K, value: V) -> void
```

1. r[expr.index.trait.std] `std.ops` declares `Index[K]`, with an associated type `Out`, and `IndexSet[K, V]`, as shown above.
2. r[expr.index.trait.read-other] For a receiver whose type is not `List`, `Map`, or `string`, a type parameter included, reading `r[k]` is the call `Index::[K]::index(r, k)`. Its type is that implementation's `Out`.
3. r[expr.index.trait.write] Assigning `r[k] = v` to such a receiver is the call `IndexSet::[K, V]::index_set(r, k, v)`.
4. r[expr.index.trait.choice] The candidates are chosen as for a binary operator, by the receiver's type, then by the key and, for a store, the value.
5. r[expr.index.trait.no-use] Neither call needs a `use` of the trait.
6. r[expr.index.trait.mut] `index_set` takes `mut self`, so a store needs mutable access to the receiver, as any `mut self` call does.
7. r[expr.index.trait.place] `r[k]` is a place only when the receiver's type implements `IndexSet`. Assigning to it otherwise is an error. Error: `invalid-assignment-target`.
8. r[expr.index.trait.no-read] Reading `r[k]` when the receiver's type has no fitting `Index` implementation is an error. Error: `type-mismatch`.
9. r[expr.index.trait.builtin-direct] A receiver whose type is `List`, `Map`, or `string` keeps the built-in indexing above, even though these types implement the traits, as [Built-In Implementations](#built-in-implementations) states.
10. r[expr.index.trait.independent] The two traits are independent: a type may implement either one alone.

```text
use std.ops.Index

data Row:
    cells: List[i32]

impl Index[usize] for Row:
    type Out = i32
    fn index(self, key: usize) -> i32:
        self.cells[key]

data Plain:
    value: i32

fn invalid(row: mut Row, plain: Plain) -> i32:
    row[0] = 1  # error: invalid-assignment-target
    plain[0]    # error: type-mismatch
```

> **Note.** `Index` has one `Out` for every reader, whatever the receiver's
> permission. An element's permission comes from `Out`, as a `List[mut U]`
> element's comes from `U`: an `Out = mut Cell` gives `mut Cell` even
> through a readonly receiver.

##### Built-In Implementations

The standard library implements the index traits for `List`, `Map`, and
`string`, so generic code bounded by them accepts these types:

```text
use std.ops.{Index, IndexSet}

fn first[C < Index[usize]](items: C) -> C::Out:
    items[0]

fn reset[C < mut IndexSet[usize, i32]](items: C) -> void:
    items[0] = 0

fn lead(counts: mut List[i32], text: string) -> u8:
    reset(counts)
    first(text)
```

| Rule | Type | Implementations |
| --- | --- | --- |
| r[expr.index.std.list.usize] List | `List[T]` | `Index[usize]` with `Out = T`, and `IndexSet[usize, T]` |
| r[expr.index.std.map] Map | `Map[K, V]` | `Index[K]` with `Out = V`, and `IndexSet[K, V]` |
| r[expr.index.std.string.usize] String | `string` | `Index[usize]` with `Out = u8`, and no `IndexSet` |
| r[expr.index.std.string.range.unsigned] String slice | `string` | `Index[R[I]]` with `Out = string`, for each range type `R` and each unsigned integer type `I` |
| r[expr.index.std.list.range.unsigned] List slice | `List[T]` | `Index[R[I]]` with `Out = mut List[T]`, for each range type `R` and each unsigned integer type `I` |
| r[expr.index.std.string.full] Whole string | `string` | `Index[RangeFull]` with `Out = string` |
| r[expr.index.std.list.full] Whole list | `List[T]` | `Index[RangeFull]` with `Out = mut List[T]` |

1. r[expr.index.std.intrinsic] The body of each implementation in the table is a compiler intrinsic. It behaves as the built-in indexing of its type, including the checks and their panics, so `Map`'s `index` panics when no equal key exists.
2. r[expr.index.std.map-store] `Map`'s `index_set` inserts or replaces the entry, as `entries[key] = value` does.
3. r[expr.index.std.string-no-index-set] `string` implements no `IndexSet`, so a bound such as `IndexSet[usize, u8]` rejects it. Error: `unsatisfied-trait-bound`.

```text
use std.ops.IndexSet

fn store[C < mut IndexSet[usize, u8]](items: C, byte: u8) -> void:
    items[0] = byte

fn invalid(text: string) -> void:
    store(text, 65)  # error: unsatisfied-trait-bound
```

> **Note.** Through a bound, `items[k]` has type `Out`, the declared
> element or value type, whatever the receiver's permission. Built-in
> indexing gives the same type, as
> [`types.path.collection`](04-type-system.md#r-types.path.collection)
> states: a `List[mut User]` yields `mut User`, and a `mut List[User]`
> yields `User`.

See also: [Operator Traits](#operator-traits),
[Compound Assignment](#compound-assignment).

### Calls

A call applies a callable to positional and named arguments:

```text
resize(640, height=480)
```

1. r[expr.call.order] A call evaluates the callable first, then arguments from left to right.
2. r[expr.call.callable] The callable may be any expression of function type, including a parenthesized field read such as `(handler.callback)(event)`.
3. r[expr.call.positional-first] Positional arguments must precede named arguments.

#### Named Arguments

1. r[expr.call.named] Each named argument identifies a parameter by its declared name.
2. r[expr.call.exactly-once] A parameter must receive exactly one argument after defaults are applied.
3. r[expr.call.duplicate] Supplying one parameter more than once, such as positionally and again by name, is an error. Error: `duplicate-argument`.
4. r[expr.call.source-order] Evaluation order follows source argument order, not parameter declaration order.

```text
fn resize(width: i32, height: i32) -> i32: width * height

fn area() -> i32:
    unknown := resize(640, depth=480)  # error: unknown-named-argument
    twice := resize(640, width=480)    # error: duplicate-argument
    unknown + twice
```

See also: [`fn.arg.unknown-name`](07-functions.md#r-fn.arg.unknown-name), which makes an unknown
named argument an error.

#### Positional Spreads

A **positional spread** `x...` passes the value `x` in place of separate
arguments. A tuple fills the callee's remaining inputs, and a vararg takes a
value of its own type:

```text
fn add(a: i32, b: i32) -> i32: a + b
fn count(values...: List[i32]) -> usize: values.len()
fn g(a: usize, b: usize, xs...: List[i32]) -> usize: a + b + xs.len()

fn spreads(pair: (i32, i32), items: List[i32], t: (usize, usize, List[i32]...)) -> (i32, usize):
    (add(pair...), count(items...) + count(1, 2) + g(t...))
```

1. r[expr.call.spread] An argument ending in `...` is a positional spread.
2. r[expr.call.spread.one] A call has at most one positional spread; it must be the final positional argument and therefore precedes every named argument. Error: `nonfinal-positional-spread`.
3. r[expr.call.spread.fills] A spread operand of type `X` is evaluated once and fills what `X` describes, as the table below states.

| Rule | Where the spread stands | `X` must be | It fills |
| --- | --- | --- | --- |
| r[expr.call.spread.at-vararg] At a vararg | the next positional parameter is a [vararg](07-functions.md#varargs) | assignable to the vararg's type | the vararg, as its collected value |
| r[expr.call.spread.tuple-vararg-tail] After a tuple vararg's arguments | it follows one or more separate arguments of a tuple-typed or `Tuple`-bounded vararg | what a tuple expression's final spread needs | the rest element, as [`fn.vararg.collect.tuple-expr`](07-functions.md#r-fn.vararg.collect.tuple-expr) states |
| r[expr.call.spread.inputs] Before fixed parameters | any other position | the same type as the tuple of the callee's remaining inputs, which keeps a [rest element](04-type-system.md#rest-elements) | each remaining parameter with one element, in order, and a `List[T]` vararg with the rest element's list |

4. r[expr.call.spread.mismatch] An operand that does not meet the table's requirement is an error. Error: `type-mismatch`.
5. r[expr.call.spread.list-needs-vararg] A `List[T]` operand where the next positional parameter is not a vararg is an error. Error: `positional-spread-needs-vararg`.
6. r[expr.call.vararg-by-name.exclusive] A call that passes a vararg by name must not also supply positional values for that vararg.

```text
fn add(a: i32, b: i32) -> i32: a + b
fn fixed(value: i32) -> i32: value

fn triple(values: (i32, i32, i32)) -> i32: add(values...)  # error: type-mismatch
fn total(values: List[i32]) -> i32: fixed(values...)       # error: positional-spread-needs-vararg
```

A function with a `List[T]` vararg takes a tuple whose rest element stands
for it. A function with a plain `List[T]` parameter takes a tuple
without one:

```text
fn g(a: usize, b: usize, xs...: List[i32]) -> usize: a + b + xs.len()
fn h(a: usize, b: usize, xs: List[i32]) -> usize: a + b + xs.len()

fn valid(t: (usize, usize, List[i32]...), u: (usize, usize, List[i32])) -> usize:
    g(t...) + h(u...)

fn crossed(t: (usize, usize, List[i32]...), u: (usize, usize, List[i32])) -> usize:
    g(u...)  # error: type-mismatch
    h(t...)  # error: type-mismatch
```

> **Note.** For a function value of type `Fn[Args, O, $ R]`, the remaining
> parameters are `Args`, so `f(args...)` with `args: Args` calls it. A
> suspending callee is written `f!(args...)`.

See also: [Varargs](07-functions.md#varargs).

#### Explicit Generic Arguments

Explicit generic arguments occur before the call argument list:

```text
first::[string](names)
```

1. r[expr.call.generic.position] Generic arguments, when explicit, occur before the call argument list.
2. r[expr.call.generic.trailing] An explicit list may omit trailing arguments, which are inferred or defaulted as [Explicit Type Arguments](07-functions.md#explicit-type-arguments) states.
3. r[expr.call.generic.targets] In hd-lang, explicit arguments may specialize a named module function or qualified function introduced by a use declaration.

See also: [Functions](07-functions.md), and
[Generic Methods And Qualified Calls](07-functions.md#generic-methods-and-qualified-calls)
for generic methods.

#### Callable Values

A **callable value** is a value whose type implements the `std.ops` trait
`Apply`: `v()` reads it. When the type also implements `Update`, `v()` is
a **call place**, and `v() = x` stores through it. This example declares a
live cell:

```text
use std.ops.{Apply, Update}

data Cell[T]:
    value: T

impl[T] Apply for Cell[T]:
    type Out = T
    fn apply(self) -> T: self.value

impl[T] Update[T] for Cell[T]:
    fn update(mut self, value: T) -> void:
        self.value = value

fn live[T](value: T) -> mut Cell[T]:
    Cell { value: value }

data Panel:
    count: mut Cell[i32]

fn parent(panel: mut Panel) -> string:
    let mut count = live(0)
    count() = 1
    count() += 1
    (panel.count)() += 1
    badge(count)

fn badge(count: Cell[i32]) -> string:
    "clicked ${count()}"
```

`std.ops` declares them in this shape:

```text
pub trait Apply:
    type Out
    fn apply(self) -> Self::Out

pub trait Update[V]:
    fn update(mut self, value: V) -> void
```

1. r[expr.call.apply.std] `std.ops` declares `Apply`, with an associated type `Out`, and `Update[V]`, as shown above.
2. r[expr.call.apply.zero-keys] Neither trait takes a key: a callable value is called with no arguments.
3. r[expr.call.apply.read] When the callee's type is not a function type and implements `Apply`, a type parameter included, the call `v()` is the call `Apply::apply(v)`.
4. r[expr.call.apply.read-type] The type of `v()` is that implementation's `Out`.
5. r[expr.call.apply.arguments] A call that passes a callable value any argument, positional or named, is an error. Error: `argument-count`.
6. r[expr.call.apply.write] Assigning `v() = x` is the call `Update::[V]::update(v, x)`.
7. r[expr.call.apply.choice] The `Update` implementation is chosen by the callee's type, then by the type of `x`, as an index store's is.
8. r[expr.call.apply.no-use] Neither call needs a `use` of the trait.
9. r[expr.call.apply.place] `v()` is a place only when the callee's type implements `Update`. Assigning to any other call, including a function call, is an error. Error: `invalid-assignment-target`.
10. r[expr.call.apply.mut] A store through `v()` mutates `v`, so `v` must have type `mut T`, as [Mutation Checks](04-type-system.md#mutation-checks) require of `e[i] = value`.
11. r[expr.call.apply.readonly] A store through a readonly callee is an error. Error: `readonly-root`.
12. r[expr.call.apply.not-callable] Calling a value whose type is neither a function type nor implements `Apply` is an error. Error: `not-callable`.
13. r[expr.call.apply.independent] The two traits are independent: a type may implement either one alone.
14. r[expr.call.apply.field] A callable value in a field is called by parenthesizing the field read, as in `(panel.count)()`, as [`expr.member.stored-fn`](#r-expr.member.stored-fn) states for a stored function.

A value with `Apply` alone reads but does not store, and a callable value
takes no keys:

```text
use std.ops.Apply

data Counter:
    hits: i32

impl Apply for Counter:
    type Out = i32
    fn apply(self) -> i32: self.hits

fn current() -> i32: 1

fn invalid(counter: mut Counter, tags: List[string]) -> void:
    counter() = 2  # error: invalid-assignment-target
    current() = 2  # error: invalid-assignment-target
    counter(1)     # error: argument-count
    tags(0)        # error: not-callable
```

A store needs a `mut` cell. With `Cell` and `live` from the first example,
a `:=` binding and a parameter typed `Cell[i32]` are readonly views:

```text
fn invalid(count: Cell[i32]) -> void:
    view := live(0)
    view() = 1   # error: readonly-root
    count() = 2  # error: readonly-root
```

> **Note.** `let mut count = live(0)` keeps the `mut Cell[i32]` that
> `live` returns, as [`types.bind.let-mut-infer`](04-type-system.md#r-types.bind.let-mut-infer)
> states. `view := live(0)` is readonly by
> [`types.bind.short`](04-type-system.md#r-types.bind.short): it reads
> `view()` but does not store. A cell reached through a readonly edge,
> `field: Cell[i32]`, gives `readonly-edge` instead, as
> [`types.path.store.readonly-edge`](04-type-system.md#r-types.path.store.readonly-edge) states.

> **Why.** Keyed reads have one syntax, `[]`, so `Apply` and `Update` take
> no keys, and a grid keeps `g[(0, 1)]`. `update` takes `mut self`, so a
> write through a cell shows in its holder's type, as every other
> mutation does.

See also: [Index Traits](#index-traits),
[Compound Assignment](#compound-assignment),
[Operator Traits](#operator-traits).

#### Suspension Calls

1. r[expr.call.suspension] Suspension calls with `!` construct and drive a child suspension as specified in [Requirements and Suspension](11-requirements-and-suspension.md).
2. r[expr.call.suspension.order] The callee and arguments are evaluated left to right before the child begins execution.

```hd
fn fetch!(key: string) -> string: "value of $key"

fn demo!() -> string:
    fetch!("a") + fetch!("b")   # left to right: "value of avalue of b"
```

### Propagation

Postfix `?` handles either an optional or a `Result` value:

| Rule | Operand | Value case | Early return |
| --- | --- | --- | --- |
| r[expr.try.option] Optional | `T?` | `.Some(value)` produces `value` as `T`. | `.None` immediately returns `.None` from the nearest function. |
| r[expr.try.result] Result | `Result[T, E]` | `.Ok(value)` produces the declared `T`, including a mutable type argument. | `.Err(error)` immediately returns `.Err` from the nearest function, holding the error converted as [Error Conversion](#error-conversion) describes. |

1. r[expr.try.once] The operand is evaluated once.
2. r[expr.try.no-panic] `?` does not catch runtime panics and does not interact with suspension by itself.
3. r[expr.try.expected] When `x?` has an expected type `T`, its operand gets an expected type as an inference hint.
4. r[expr.try.expected.result] The hint is `Result[T, E]` when the nearest function returns `Result[U, E]`.
5. r[expr.try.expected.option] The hint is `T?` when the nearest function returns an optional.
6. r[expr.try.expected.hint-only] The hint only solves what the operand leaves open, such as a call's type arguments; it never coerces the operand.

```text
fn empty_ok[T]() -> Result[List[T], string]:
    .Ok([])

fn no_ports() -> Result[List[i32], string]:
    let ports: List[i32] = empty_ok()?
    .Ok(ports)
```

In `no_ports`, the hint `Result[List[i32], string]` solves `empty_ok`'s
open type argument `T` as `i32`.

#### Error Conversion

1. r[expr.try.convert] Let the nearest enclosing named function or closure return `Result[U, F]`. The error of type `E` becomes the returned error by the first of these steps that applies.

The steps are:

1. r[expr.try.convert.assignable] **Assignability.** If `E` is assignable to `F` by one rule of [Assignability And Coercion](04-type-system.md#assignability-and-coercion), the returned error is the error converted by that rule.
2. r[expr.try.convert.from] **Conversion.** Otherwise, if `F` implements `From[E]`, the returned error is the result of that implementation's `from` applied to the error.
3. r[expr.try.convert.none] Otherwise the `?` is an error. Error: `invalid-result-propagation`.

```text
use std.convert.From

enum FsError:
    NotFound(path: string)

enum HttpError:
    Timeout

enum SyncError:
    Fs(error: FsError)
    Http(error: HttpError)

impl From[FsError] for SyncError:
    fn from(value: FsError) -> SyncError: SyncError.Fs(value)

impl From[HttpError] for SyncError:
    fn from(value: HttpError) -> SyncError: SyncError.Http(value)

fn read_config(path: string) -> Result[string, FsError]:
    .Err(FsError.NotFound(path))

fn fetch(url: string) -> Result[string, HttpError]:
    .Err(HttpError.Timeout)

fn sync(path: string) -> Result[string, SyncError]:
    url := read_config(path)?
    body := fetch(url)?
    .Ok(body)
```

These rules refine the steps:

1. r[expr.try.convert.assignable.covers] The assignability step covers an identical type, permission weakening, and variance. It also covers construction of a dynamic trait value such as the erased `dyn Error`, supertrait widening of a dynamic value, and optional injection.
2. r[expr.try.convert.strip-mut] In the conversion step, an outer `mut` on `E` is removed before the implementation is chosen.
3. r[expr.try.convert.no-import] The code using `?` does not need to import `From`.
4. r[expr.try.convert.message] The message of the third step's error should name `E` and `F`. It should suggest an implementation of `From[E]` for `F` or an explicit mapping of the error.
5. r[expr.try.convert.one-step] Exactly one step converts the error.
6. r[expr.try.convert.no-combine] `?` never combines an assignability rule with a conversion, and it never chains conversions.
7. r[expr.try.convert.no-chain] With `impl From[A] for B` and `impl From[B] for C`, a `?` on `Result[T, A]` in a function returning `Result[U, C]` is an error. Error: `invalid-result-propagation`.
8. r[expr.try.convert.no-injection] With the same implementations, a `?` on `Result[T, A]` in a function returning `Result[U, B?]` is also an error. The conversion to `B` would need an optional injection after it. Error: `invalid-result-propagation`.
9. r[expr.try.convert.single-rule] Assignability is itself one rule: an `A` that implements the trait `Tr` does not propagate into `Result[U, dyn Tr?]`.
10. r[expr.try.convert.only-try] Only `?` calls a conversion. `return .Err(error)` and every other `.Err` construction use ordinary assignability.
11. r[expr.try.convert.before-defer] The conversion is part of the propagated value, so it runs before any deferred cleanup.
12. r[expr.try.convert.pure] A conversion neither suspends nor uses a requirement, because `from` has neither.
13. r[expr.try.convert.inferred-closure] When the nearest function is a closure whose result type is neither written nor supplied by an expected function type, `?` performs no conversion. There, `E` itself contributes to the inferred result type.

See also: [Error Trait](09-traits.md#error-trait),
[Conversion Trait](09-traits.md#conversion-trait),
[Deferred Cleanup](06-control-flow.md#deferred-cleanup).

#### Propagation Targets

1. r[expr.try.target.nearest-function] Postfix `?` is invalid when its nearest enclosing named function or closure does not provide the required optional or `Result` return type.
2. r[expr.try.target.module-top-level] Module top-level statements do not provide an implicit propagation target.
3. r[expr.try.target.test-body] A test body is a closure, so it is the propagation target of its `?`, with the result type [Propagation In Test Blocks](#propagation-in-test-blocks) gives it.
4. r[expr.try.misuse] Every misuse of `?` is an error. Error: `invalid-result-propagation`.
5. r[expr.try.misuse.operand] An operand that is neither optional nor a `Result` is a misuse.
6. r[expr.try.misuse.target] A `?` whose enclosing function or closure does not return a compatible optional or a `Result` is a misuse.
7. r[expr.try.misuse.unconverted] A `Result` error that neither conversion step converts to the enclosing error type is a misuse.

```text
fn first(value: i32?) -> i32:
    value?  # error: invalid-result-propagation
```

#### Propagation In Test Blocks

A test body written as a trailing block has a fixed result type, and `?`
propagates to it by the ordinary rules:

```text
use std.error.Error
use std.testing.assert_equal

enum DigitError:
    NotDigit(text: string)

impl Display for DigitError:
    fn to_string(self) -> string:
        match self:
            DigitError.NotDigit(text) => "not a digit: " + text

impl Error for DigitError

fn parse_digit(text: string) -> Result[i32, DigitError]:
    if text == "7": .Ok(7) else: .Err(DigitError.NotDigit(text))

tests:
    it("parses a digit"):
        digit := parse_digit("7")?
        assert_equal(digit, 7, reason="the digit parses")
        .Ok(())

    it("a body without ? is void"):
        match parse_digit("x"):
            .Ok(_) => panic("x is not a digit")
            .Err(error) => assert_equal(error.to_string(), "not a digit: x", reason="the helper rejects x")
```

1. r[expr.try.test.fixed-result] When a trailing block is the body of an [`it` call](10-modules.md#test-cases), its result type is fixed rather than inferred.
2. r[expr.try.test.with-try] If the block contains a `?` outside any nested closure, its result type is `Result[void, dyn Error]`, where `dyn Error` is the erased `std.error.Error`.
3. r[expr.try.test.without-try] Otherwise its result type is `void`.
4. r[expr.try.test.converts] `?` in such a block converts by the ordinary rules, so an error type that implements `Error` propagates into the erased `dyn Error`. An error type that does not is an error. Error: `invalid-result-propagation`.
5. r[expr.try.test.final-value] The block's final value must be assignable to its result type, as for a function body. So a block that uses `?` usually ends in `.Ok(())`.
6. r[expr.try.test.explicit-closure] A body passed as an explicit closure keeps its written or inferred result type. That type must implement `std.process.Termination`, the bound on `it`. Error: `unsatisfied-trait-bound`.
7. r[expr.try.test.closure] Inside a closure nested in a test body, that closure is the nearest function, and these rules do not apply to it.

```text
use std.testing.assert

data Hidden: pass

fn hidden() -> Result[void, Hidden]:
    .Err(Hidden {})

fn maybe() -> i32?:
    .None

fn lookup(key: string) -> Result[i32, string]:
    .Err("missing: " + key)

tests:
    it("an error without Display", fn!() -> Result[void, Hidden]: hidden())  # error: unsatisfied-trait-bound
    it("an optional result", fn!() -> i32?: maybe())                         # error: unsatisfied-trait-bound

    it("a string error does not convert"):
        value := lookup("port")?  # error: invalid-result-propagation
        assert(value > 0, reason="a positive port")
        .Ok(())
```

> **Why.** A fixed `Result[void, dyn Error]` lets one test body use `?` on
> several error types, as Zig's inferred `anyerror!void` test bodies do.

> **Note.** The body closures of `it_each`, `it_prop`, and `it_prop_with`
> get their result types by rules 2 and 3, by
> [`module.testing.reg.body-closure-result`](10-modules.md#r-module.testing.reg.body-closure-result).

See also: [Error Trait](09-traits.md#error-trait),
[Standard Testing](10-modules.md#standard-testing).

## Unary And Binary Operators

This section defines operator precedence and the meaning of each operator.

### Precedence

1. r[expr.op.precedence] Operators are ordered from highest to lowest precedence as this table shows:

| Precedence | Operators | Associativity |
| --- | --- | --- |
| Postfix | `.`, `[]`, `()`, `!()`, postfix `?` | left |
| Power | `**` | right |
| Unary | `+`, `-`, `~`, prefix `!` | right |
| Multiplicative | `*`, `/`, `%` | left |
| Additive | `+`, `-` | left |
| Shift | `<<`, `>>` | left |
| Bitwise AND | `&` | left |
| Bitwise XOR | `^` | left |
| Bitwise OR | `\|` | left |
| Pipe | `\|>` | left |
| Comparison | `==`, `!=`, `<`, `<=`, `>`, `>=`, `is` | non-associative |
| Logical AND | `&&` | left, short-circuiting |
| Logical OR | `\|\|` | left, short-circuiting |
| Range | `..`, `..=` | non-associative |
| Control and closure | `if`, `match`, `for`, `while`, `fn` | structural |
| Binding | `:=` | right |

1. r[expr.op.suspension-call] Suspension-call postfix `!` has the same precedence as an ordinary call.
2. r[expr.op.not] Prefix `!` is logical negation.
3. r[expr.op.not.position] Prefix `!` is recognized only at the start of an operand. So it never conflicts with the suspension suffix `f!(args)` or the `fn!` type marker, which always follow a name or `fn`.
4. r[expr.op.not.combine] The two forms may combine: inside a suspending function, `!fetch!(id)` negates the `bool` result of the suspending call `fetch!(id)`.
5. r[expr.op.not-equal] By longest match, `f!=g` lexes as `f`, `!=`, `g` and is an inequality comparison, not a suspension call.
6. r[expr.op.complement] `~` remains bitwise complement on integers.
7. r[expr.op.not.grouping] `a && !b || c` groups as `(a && (!b)) || c`: prefix `!` binds tighter than every binary operator except `**`, and `&&` binds tighter than `||`.
8. r[expr.op.power.grouping] Exponentiation binds less tightly on its right than unary negation, following the grammar: `2 ** -3` parses as `2 ** (-3)`, while `-2 ** 2` means `-(2 ** 2)`.
9. r[expr.op.power.grouping.signed] With an integer base, `2 ** -3` is then rejected because its exponent is signed, as [Exponentiation](#exponentiation) describes.
10. r[expr.op.no-chain] Comparisons do not chain.

> **Note.** Write `low <= value && value < high` rather than
> `low <= value < high`.

```hd
fn demo() -> i32:
    2 + 3 * 4   # 14: * binds tighter than +
```

### Arithmetic Operators

1. r[expr.arith.primitive-numeric] Between primitive operands, arithmetic operators require compatible numeric operands. Other operands use [Operator Traits](#operator-traits).
2. r[expr.arith.non-numeric-no-impl] A binary `+`, `-`, `*`, `/`, `%`, or `**` is an error when an operand is a non-numeric primitive, as in `true + false`. It is also an error when no operator trait implementation fits, as in `[1] * [2]`. Error: `type-mismatch`.
3. r[expr.arith.string-primitive] `string + string` is the only arithmetic form on non-numeric primitives.
4. r[expr.arith.defined] Mixed-width result types, overflow, division, and shifts are defined in [Type System](04-type-system.md).
5. r[expr.arith.cast] Signed/unsigned and integer/floating mixing requires an explicit cast.
6. r[expr.arith.int.checked] For compatible integer operands, `+`, `-`, and `*` produce the common integer type. They are checked in a debug or test build and wrap in a release build, by [`types.arith.checked`](04-type-system.md#r-types.arith.checked).
7. r[expr.arith.int.divide] `/` truncates toward zero, `%` produces the corresponding remainder, and a zero divisor panics.
8. r[expr.arith.unary-plus] Unary `+` accepts all numeric types, preserves its operand's type and value, and evaluates the operand once. Any other operand is an error. Error: `type-mismatch`.
9. r[expr.arith.unary-minus] Unary `-` accepts signed integers and floating-point values, but not unsigned integers. Negating an unsigned operand is an error. Error: `type-mismatch`.

```text
fn sum(a: bool, b: bool) -> bool: a + b                     # error: type-mismatch
fn product(a: List[i32], b: List[i32]) -> List[i32]: a * b  # error: type-mismatch
```

### Bitwise Operators

1. r[expr.bit.primitive-integer] Between primitive operands, `~`, `&`, `|`, and `^` accept integer values only and produce the operand common type. Other operands use [Operator Traits](#operator-traits).
2. r[expr.bit.non-integer-no-impl] A binary `&`, `|`, or `^` with a primitive operand that is not an integer is an error. `bool`, floating-point, and `string` operands are examples. One with a non-primitive operand is an error too when no operator trait implementation fits. Error: `type-mismatch`.

```text
fn both(a: bool, b: bool) -> bool: a & b          # error: type-mismatch
fn either(a: f64, b: f64) -> f64: a | b           # error: type-mismatch
fn toggle(a: string, b: string) -> string: a ^ b  # error: type-mismatch
```

### Shifts

A shift moves the left operand's bits by an unsigned integer count:

```text
fn mask(x: i64, n: u32) -> i64:
    (x << n) | (x >> 3)

fn indexed_mask(x: i64, i: usize) -> i64:
    x << i
```

1. r[expr.shift.unification] Shifts are the exception to ordinary binary numeric unification.
2. r[expr.shift.left-any] The left operand may have any integer type, and the result has the left operand's type.
3. r[expr.shift.count-unsigned] The right operand, the shift count, must have an unsigned integer type. Its type may be `u8`, `u16`, `u32`, `u64`, or `usize`.
4. r[expr.shift.count-literal] An unsuffixed integer literal count has type `u32`, so `x << 3` needs no suffix.
5. r[expr.shift.count-invalid] A signed integer count or a count of any non-integer type is an error. A numeric count's fix-it converts it, as in `u32(n)`. Error: `type-mismatch`.
6. r[expr.shift.count] A negative count or a count at least as large as the left operand's bit width is an invalid shift, by [`types.arith.shift-count`](04-type-system.md#r-types.arith.shift-count).
7. r[expr.shift.fixed-width] The shift itself is a fixed-width bit operation; left-shifted high bits are discarded rather than reported as arithmetic overflow.

```text
fn scale(x: i64, n: i32) -> i64:
    x << n  # error: type-mismatch
```

> **Why.** Accepting every unsigned width lets an index or a compact count
> shift without a cast, while excluding negative counts statically. A literal
> still takes `u32`, the exponent type of `**` and the count type of Rust's
> `checked_shl`.

### Exponentiation

1. r[expr.power.int.exponent] For an integer base, `**` requires an exponent of an unsigned integer type, so a negative exponent cannot occur.
2. r[expr.power.int.literal] An unsuffixed integer literal in exponent position has type `u32`.
3. r[expr.power.int.signed] A signed integer exponent is an error. Error: `type-mismatch`.
4. r[expr.power.negated-literal] A negated literal is signed: in `2 ** -1` the literal `1` is the operand of unary `-`, not the exponent itself. So `-1` has a signed type, and the expression is a compile-time error. Error: `type-mismatch`.
5. r[expr.power.negated-literal.not-other] That error is not a runtime panic.
6. r[expr.power.checked] Integer exponentiation uses checked multiplication in the base's result type.
7. r[expr.power.float.same-type] For a floating-point base, the exponent must have the base's type, as for a [binary numeric operator](04-type-system.md#binary-numeric-operators). A floating-point literal exponent takes that type. Error: `type-mismatch`.
8. r[expr.power.float.pow] Floating `**` computes IEEE 754-2019 `pow` as specified in clause 9.2, including its special cases, and rounds the result correctly to the destination format.
9. r[expr.power.mixed] Integer and floating operands do not mix without an explicit cast; a mixed power expression is an error. Error: `type-mismatch`.

```text
fn power(exponent: i32) -> i32: 2 ** exponent  # error: type-mismatch
fn inverse() -> i32: 2 ** -1                   # error: type-mismatch
fn main() -> f64: 2 ** 2.0                     # error: type-mismatch
```

### Floating-Point Arithmetic

1. r[expr.float.basic] Floating `+`, `-`, `*`, and `/` use the corresponding required IEEE 754 basic operation, including infinities, signed zero, and NaN.
2. r[expr.float.power] Floating `**` uses the `pow` rule of [Exponentiation](#exponentiation).
3. r[expr.float.remainder-truncated] Floating `%` returns the remainder of division truncated toward zero, as C `fmod` and Rust `%` do. The result is exact and has the dividend's sign.
4. r[expr.float.remainder-special] A zero divisor, an infinite dividend, or a NaN operand gives NaN. A finite dividend with an infinite divisor gives the dividend.

```hd
fn demo() -> f64:
    5.5 % 2.0   # 1.5: truncated toward zero, with the dividend's sign
```

### Logical Operators

1. r[expr.logic.not] Prefix `!` requires `bool`.
2. r[expr.logic.bool] `&&` and `||` require `bool` operands and produce `bool`.
3. r[expr.logic.short-circuit] `&&` and `||` evaluate the right operand only when needed.

```hd
fn demo(a: bool, b: bool) -> bool:
    a && !b   # b is read only when a is true
```

### Equality

1. r[expr.eq.calls-eq] `==` calls `Eq.eq` and `!=` negates that result.
2. r[expr.eq.std] Standard-library implementations provide value equality for primitives, optional and result values, tuples, lists, and maps when their elements support equality.
3. r[expr.eq.map-order] Map equality is independent of entry order.
4. r[expr.eq.no-implicit] A user-defined data or enum type has no implicit `Eq` implementation, even if all its members are comparable. Its author must explicitly implement or request derivation of the trait.
5. r[expr.eq.no-identity-fallback] Equality never silently falls back to reference identity.
6. r[expr.eq.float] Floating-point equality follows IEEE 754, so NaN is unequal even to itself, although floating-point types implement `Eq`.
7. r[expr.eq.std.intrinsic] The `Eq` implementations of the number types, `char`, and `bool` are [intrinsic methods](09-traits.md#intrinsic-methods). The one for `string` is not.
8. r[expr.eq.functions] Function and closure values do not implement `Eq`; applying `==` or `!=` to them is an error. Error: `type-mismatch`.
9. r[expr.eq.contextual-operand] In `==` and `!=`, a contextual variant operand, such as `.None`, `.Ok(1)`, or `.Some(x)`, takes the other operand's type as its expected type. This holds on either side.
10. r[expr.eq.contextual-both] When both operands are contextual variants, neither has an expected type, so `.None == .None` is an error. Error: `missing-contextual-enum-type`.
11. r[expr.eq.readonly-view] `==` and `!=` with one `mut T` operand and one `T` operand compare at `T`, because the readonly view is enough. So `d == Date { year: 2026 }` with `d: Date` is valid, though the literal is a fresh `mut Date`.
12. r[expr.eq.enum-hint] `==` or `!=` on an enum that does not implement `Eq` is an error whose message suggests adding `@derive(Eq)` to the enum. Error: `type-mismatch`.
13. r[expr.eq.enum-hint.fix] When the enum is declared in the same file as the comparison, the error has a fix-it. The fix-it inserts `@derive(Eq)` on its own line before the enum's declaration.

```text
fn invalid(left: fn() -> void, right: fn() -> void) -> bool:
    left == right  # error: type-mismatch
```

```text
fn absent(found: i32?) -> bool:
    found == .None

fn first(found: i32?) -> bool:
    .Some(1) != found

fn neither() -> bool:
    .None == .None  # error: missing-contextual-enum-type
```

> **Why.** Equality stays opt-in, by
> [`expr.eq.no-implicit`](#r-expr.eq.no-implicit), so the error names the
> one-line opt-in rather than leaving the reader to find it.

### Ordering

1. r[expr.ord.partial-cmp] `<`, `<=`, `>`, and `>=` use `PartialOrd.partial_cmp`.
2. r[expr.ord.std] The standard library implements `PartialOrd.partial_cmp` for compatible numeric values, characters, strings, tuples, lists, and optionals.
3. r[expr.ord.std.char-scalar] Characters are ordered by Unicode scalar value.
4. r[expr.ord.std.string-bytes] Strings are ordered lexicographically by byte, which for valid UTF-8 is the order by scalar value.
5. r[expr.ord.std.sequences] Tuples and lists are ordered lexicographically.
6. r[expr.ord.std.optional] Optionals are ordered with `.None` before every present value.
7. r[expr.ord.composite] Composite ordering is available when the corresponding elements implement the comparison trait, and it stops at the first unequal or unordered element.
8. r[expr.ord.user] Users can implement comparison traits for their own types.
9. r[expr.ord.total] `Ord` is the total-order refinement; floating-point types have `PartialOrd` but not `Ord` because NaN is unordered.
10. r[expr.ord.unordered] An unordered comparison makes all four relational operators false.
11. r[expr.ord.std.intrinsic] The `PartialOrd` implementations of the number types and `char`, and the `Ord` implementations of the integer types and `char`, are [intrinsic methods](09-traits.md#intrinsic-methods). Those for `string` are not.

```hd
fn demo(names: List[string]) -> List[string]:
    names.sorted()   # byte order: "Zebra" before "apple"
```

#### Unsigned Comparisons With Zero

A comparison of an unsigned value with zero in one of four forms has a
result fixed by the type, so it gets a warning:

| Rule | Form | Result for an unsigned `t` |
| --- | --- | --- |
| r[expr.ord.unsigned-zero.at-least] At least zero | `t >= 0` | always true |
| r[expr.ord.unsigned-zero.at-most] Zero at most | `0 <= t` | always true |
| r[expr.ord.unsigned-zero.below] Below zero | `t < 0` | always false |
| r[expr.ord.unsigned-zero.above] Zero above | `0 > t` | always false |

1. r[expr.ord.unsigned-zero] A comparison in one of the forms in the table, where `t` has an unsigned integer type, gets a warning, and the program still compiles. Warning: `unsigned-comparison-always`.
2. r[expr.ord.unsigned-zero.literal] The `0` of a form is any unsuffixed integer literal whose value is zero, in any radix, such as `0` or `0x0`.
3. r[expr.ord.unsigned-zero.binding-type] The type of `t` is its type at the comparison. That is a declared unsigned type, or the [`usize` default](04-type-system.md#r-types.literal.local.default) of the literal its binding was initialized with.
5. r[expr.ord.unsigned-zero.message] The warning must name the operand and say whether the comparison is always true or always false. One phrasing is "`t` is unsigned, so `t >= 0` is always true". When `t` has the `usize` default, it should also suggest a signed literal such as `+10` or another condition.

```text
fn countdown() -> void:
    let t = 10
    while t >= 0:  # warning: unsigned-comparison-always
        t = t - 1

fn below(value: u32) -> bool:
    value < 0  # warning: unsigned-comparison-always
```

> **Why.** With the `usize` default, `while t >= 0` never ends normally:
> `t - 1` panics at zero instead. A countdown that means to reach `-1`
> writes `let t = +10`.

### Identity

`is` compares identity without invoking user code.

1. r[expr.is.no-user-code] `is` compares identity without invoking user code.

```hd
fn demo(a: List[i32], b: List[i32]) -> bool:
    a is b   # identity, not contents
```

#### Allocation Identity

1. r[expr.is.heap-values] Data values, lists, maps, and other heap composites have allocation identity; access permission (`mut`) does not change it.
2. r[expr.is.conversion] Converting such a value to a trait value or `Any` preserves the underlying identity.
3. r[expr.is.function-none] A function value has no identity. This covers named functions, generic instantiations, one-payload variant constructors, and closures.
4. r[expr.is.function-sharing] An implementation may share one function value between evaluations or allocate a new one at each evaluation. No program can observe the choice.
5. r[expr.is.box-values] A conversion of a primitive, string, tuple, enum, or function value to a dynamic trait value or `Any` may box the value. The box has no identity.
6. r[expr.is.box-unspecified] `is` between two trait or `Any` values has an unspecified result when either holds such a value at run time.
7. r[expr.is.no-wrapper] A direct conversion of a heap composite continues to preserve the composite's underlying identity and does not allocate an identity wrapper.
8. r[expr.is.canonical-data] A fieldless data value is canonical for its data type.
9. r[expr.is.canonical.same] Two occurrences of the same such value have the same identity, and constructing one allocates nothing.

```hd
fn demo() -> bool:
    let items: mut List[i32] = [+1]
    same := items
    items is same   # one allocation, one identity
```

#### Identity Operands

1. r[expr.is.tuple] Tuples have no identity, and using `is` with a tuple is an error even if it contains references. Error: `identity-requires-references`.
2. r[expr.is.value-operand] Primitive values, strings, enum values, optionals included, and function values likewise cannot be compared with `is`: an operand type must implement `AnyRef`, not `AnyVal`. Error: `identity-requires-references`.
3. r[expr.is.compatible] Both operands must otherwise have compatible composite reference types. Two such types are compatible when, after removing `mut` at every level, they are equal. They are also compatible when one is a trait value or `Any` type that the other converts to.
4. r[expr.is.permissions] Permissions never affect identity, so `List[User]` and `mut List[mut User]` are compatible.
5. r[expr.is.incompatible] Two composite reference operands that are not compatible, such as `List[User]` and `List[Order]`, are an error. Error: `incompatible-identity-operands`.
6. r[expr.is.function.operand] An `is` with an operand whose static type is a function type is an error, as for a value type. Error: `identity-requires-references`.
7. r[expr.is.function.bound] A function type does not implement `AnyRef`, so generic code over `T < AnyRef` never receives a function value. Error: `unsatisfied-trait-bound`.

```text
data User:
    name: string

data Order:
    total: i32

fn same(user: User, order: Order) -> bool:
    user is order  # error: incompatible-identity-operands

fn tuples(left: (i32, i32), right: (i32, i32)) -> bool:
    left is right  # error: identity-requires-references

enum Color:
    Red
    Green

fn colors(left: Color, right: Color) -> bool:
    left is right  # error: identity-requires-references
```

```text
fn aliases() -> bool:
    callback := fn() -> i32: 1
    alias := callback
    callback is alias  # error: identity-requires-references
```

> **Note.** Use `!(a is b)` for distinct identities. Code that must later
> remove a registered callback keeps a handle returned at registration
> instead of comparing function values.

> **Why.** Enum and function values have no identity, so an implementation
> may copy, share, or allocate them freely. A comparison would expose that
> choice. An enum never closes a reference cycle by itself, so cycle checks
> need no enum identity either.

See also: [Trait Values And `Any`](04-type-system.md#trait-values-and-any).

### Operator Traits

An **operator trait** is a `std.ops` trait that gives a type one operator. A
library type gets the operator by implementing the trait:

```text
use std.ops.{Add, Mul, Neg}

data Money:
    cents: i64

impl Add for Money:
    type Out = Money
    fn add(self, rhs: Money) -> Money:
        Money { cents: self.cents + rhs.cents }

impl Mul[i64] for Money:
    type Out = Money
    fn mul(self, rhs: i64) -> Money:
        Money { cents: self.cents * rhs }

impl Neg for Money:
    type Out = Money
    fn neg(self) -> Money:
        Money { cents: -self.cents }

fn net(price: Money, refund: Money) -> Money:
    price * 3 + -refund
```

`std.ops` declares each binary operator trait in this shape, and the two
unary ones without the argument:

```text
pub trait Add[Rhs = Self]:
    type Out
    fn add(self, rhs: Rhs) -> Self::Out

pub trait Neg:
    type Out
    fn neg(self) -> Self::Out
```

| Rule | Operator | Trait | Method |
| --- | --- | --- | --- |
| r[expr.op.trait.add] Add | `a + b` | `Add[Rhs]` | `add` |
| r[expr.op.trait.sub] Subtract | `a - b` | `Sub[Rhs]` | `sub` |
| r[expr.op.trait.mul] Multiply | `a * b` | `Mul[Rhs]` | `mul` |
| r[expr.op.trait.div] Divide | `a / b` | `Div[Rhs]` | `div` |
| r[expr.op.trait.rem] Remainder | `a % b` | `Rem[Rhs]` | `rem` |
| r[expr.op.trait.neg] Negate | `-a` | `Neg` | `neg` |
| r[expr.op.trait.bit-and] Bitwise AND | `a & b` | `BitAnd[Rhs]` | `bit_and` |
| r[expr.op.trait.bit-or] Bitwise OR | `a \| b` | `BitOr[Rhs]` | `bit_or` |
| r[expr.op.trait.bit-xor] Bitwise XOR | `a ^ b` | `BitXor[Rhs]` | `bit_xor` |
| r[expr.op.trait.not] Complement | `~a` | `Not` | `not` |
| r[expr.op.trait.shl] Shift left | `a << b` | `Shl[Rhs]` | `shl` |
| r[expr.op.trait.shr] Shift right | `a >> b` | `Shr[Rhs]` | `shr` |

1. r[expr.op.builtin] Arithmetic and bitwise operators are built in for the numeric types specified by this chapter and [Type System](04-type-system.md).
2. r[expr.op.concat] `string + string` concatenates strings.
3. r[expr.op.trait.std] `std.ops` declares the twelve operator traits in the table above.
4. r[expr.op.trait.shape-default] A binary operator trait takes the right operand's type as its one argument `Rhs`, whose [default](04-type-system.md#type-argument-defaults) is `Self`. It declares an associated type `Out` and one method `fn m(self, rhs: Rhs) -> Self::Out`.
5. r[expr.op.trait.rhs-self] So `impl Add for Money` implements `Add[Money]`, and the bound `T < Add[Out = T]` means `T < Add[T, Out = T]`.
6. r[expr.op.trait.rhs-explicit] The explicit form, such as `impl Add[Money] for Money`, stays valid and names the same trait.
7. r[expr.op.trait.unary-shape] `Neg` and `Not` take no argument. Each declares `Out` and one method `fn m(self) -> Self::Out`.
8. r[expr.op.primitive.types] When every operand is primitive after literal typing, the built-in rules of this chapter and [Type System](04-type-system.md) type the operator. They accept operands of one type or reject them, never converting an operand, and no trait is searched.
9. r[expr.op.primitive.call] The operator then calls the trait method that [`expr.op.desugar`](#r-expr.op.desugar) names for it, with the operands. So `a + b`, with two `i64` operands, calls `Add::[i64]::add(a, b)`.
10. r[expr.op.desugar] Otherwise `a op b` is the trait-qualified call `Op::[R]::m(a, b)` of the operator's trait, as in `Add::[R]::add(a, b)`. Likewise `-a` is `Neg::neg(a)` and `~a` is `Not::not(a)`.
11. r[expr.op.no-use] The call needs no `use` of the trait.
12. r[expr.op.left-dispatch] The left operand's type selects the implementation. Its instantiations of the trait are the candidates, and [Instantiations Of One Generic Trait](09-traits.md#instantiations-of-one-generic-trait) chooses among them by the right operand.
13. r[expr.op.left-dispatch.example] So `price * 3` checks `3` against `i64` in `Mul[i64]`.
14. r[expr.op.left-dispatch.exact-function] A left operand of function type selects implementations by its own type. [Row subsumption](11-requirements-and-suspension.md#row-subsumption) does not apply, so an implementation for a function type with a wider row does not fit. Error: `type-mismatch`.
15. r[expr.op.left-dispatch.exact-function.message] That error's message should suggest a binding typed with the implementation's function type, as in `let handler: fn(i32) -> i32 $ Db = get`.
16. r[expr.op.generic] When an operand's type is a type parameter, the candidates come from its bounds and their supertraits.
17. r[expr.op.out] The operator's result type is the chosen implementation's `Out`. Implementations are unique per trait instantiation and target, so `a + b` has one type.
18. r[expr.op.order] The left operand is evaluated, then the right one, and then the method is called.
19. r[expr.op.left-literal.join] A literal left operand with a non-primitive right operand takes its width by [`types.literal.local.class.meet`](04-type-system.md#r-types.literal.local.class.meet). An implementation alone never names a width, so with only `impl Mul[Money] for i64`, write `i64(3) * price`.
20. r[expr.op.left-literal.no-fit] With only `impl Mul[i64] for Money`, no width of `3` lets `3 * price` type-check, so it is an error; write `price * 3`. Error: `type-mismatch`.
23. r[expr.op.no-impl] An operator for which no implementation fits is an error, and its message should name the missing trait. Error: `type-mismatch`.
24. r[expr.op.newtype] A newtype has only the operators its author implements. It inherits none from its base type, and no derivation supplies an operator trait.
25. r[expr.op.fixed] Operator traits never change precedence or associativity, and they add no operator symbols.
26. r[expr.op.not-overloaded] `&&`, `||`, prefix `!`, unary `+`, `**`, `is`, `=`, `:=`, and postfix `?` have no trait and keep their built-in meaning.
27. r[expr.op.comparison] `==`, `!=`, and the relational operators call `Eq` and `PartialOrd`, as [Equality](#equality) and [Ordering](#ordering) define. `std.ops` declares no comparison trait.

```text
use std.ops.Mul

data Money:
    cents: i64

impl Mul[i64] for Money:
    type Out = Money
    fn mul(self, rhs: i64) -> Money:
        Money { cents: self.cents * rhs }

type Meters(f64)

fn triple(price: Money) -> Money:
    3 * price  # error: type-mismatch

fn total(a: Meters, b: Meters) -> Meters:
    a + b  # error: type-mismatch
```

#### Primitive Implementations

The standard library implements the operator traits for the primitive number
types, and `Add` for `string`. So generic code bounded by an operator trait
accepts them:

```text
use std.ops.Add

fn sum[T < Add[Out = T]](items: List[T], zero: T) -> T:
    let total = zero
    for item in items:
        total = total + item
    total

fn count(items: List[i32]) -> i32:
    sum(items, 0)
```

| Rule | Traits | Implementations, with `Out = T` |
| --- | --- | --- |
| r[expr.op.std.arith] Arithmetic | `Add`, `Sub`, `Mul`, `Div`, `Rem` | `impl Add for T` and the like, for every integer and floating-point type `T` |
| r[expr.op.std.neg] Negation | `Neg` | every signed integer and floating-point type |
| r[expr.op.std.bitwise] Bitwise | `BitAnd`, `BitOr`, `BitXor` | `impl BitAnd for T` and the like, for every integer type `T` |
| r[expr.op.std.not] Complement | `Not` | every integer type |
| r[expr.op.std.string-add] Concatenation | `Add` | `impl Add for string` |
| r[expr.op.std.shift-unsigned] Shifts | `Shl`, `Shr` | `impl Shl[C] for T` and `impl Shr[C] for T`, for every integer type `T` and every unsigned integer type `C` |

1. r[expr.op.std.intrinsic-method] Each method of the number types' implementations in the table is an [intrinsic method](09-traits.md#intrinsic-methods). It computes the operation that this chapter and [Type System](04-type-system.md) define for its type, including checked overflow and its panics.
2. r[expr.op.std.string-not-intrinsic] `impl Add for string` is an ordinary implementation, not an intrinsic method.
3. r[expr.op.std.one-type] The arithmetic and bitwise implementations are same-type only, as the built-in typing of primitive operands is, so neither generic nor primitive code widens an operand.
4. r[expr.op.std.string-generic] `impl Add for string` concatenates, as `string + string` does, so generic code such as `T < Add[Out = T]` accepts `string`.
5. r[expr.op.std.bool-char] The standard library declares no operator trait implementation for `bool` or `char`.

> **Note.** An implementation may compile an operator on primitive
> operands to inline code rather than a call, as
> [`trait.impl.intrinsic.inline`](09-traits.md#r-trait.impl.intrinsic.inline)
> allows. The result is the same.

> **Why.** This is Rust's shape. The right operand is a trait argument, so a
> type may scale by `i64` and add its own kind. As in Rust, the argument
> defaults to `Self`, so the common same-type case omits it. The output is an associated
> type that the operands fix, so `x := a + b` never becomes ambiguous when an
> implementation is added. Primitive operands skip trait search, so numeric
> code compiles as before. Type checking never searches across numeric
> types, the cost Swift pays for overloaded operators.

See also: [Compound Assignment](#compound-assignment),
[Index Traits](#index-traits),
[Numeric Traits](09-traits.md#numeric-traits).

## Pipe Expressions

A **pipe expression** `value |> step` passes a value to a step, so a chain
of calls reads left to right:

```text
fn tag(label: string, level: i32) -> string:
    "$label:$level"

fn clean(raw: string) -> string:
    "[$raw]"

fn label(raw: string) -> string:
    raw |> clean |> tag(_, 2)
```

1. r[expr.pipe.form] A pipe expression is a value, the `|>` token, and a step.
2. r[expr.pipe.left-assoc] `|>` is left-associative: `a |> f |> g` is `(a |> f) |> g`.
3. r[expr.pipe.precedence] `|>` binds more loosely than `|` and more tightly than comparison, so `n + 1 |> twice == 4` is `((n + 1) |> twice) == 4`.
4. r[expr.pipe.step-kinds] A step is either a **substitution step**, which contains `_`, or a **bare step**, which is a name or path without `_`.
5. r[expr.pipe.order] The piped value is evaluated first and exactly once, before any part of the step, including the step's callee.
6. r[expr.pipe.value] The pipe expression's type and value are those of its step.

### Substitution Steps

A substitution step marks the piped value's slot with `_`:

```text
data Point:
    x: usize
    y: usize

fn shift(point: Point, by: usize) -> usize:
    point.x + by

fn measure(raw: string, start: usize) -> usize:
    width := raw |> _.len()
    moved := start |> Point { x: _, y: 0 } |> shift(_, 3)
    width + moved |> _ * 2
```

1. r[expr.pipe.slot.one] A substitution step contains exactly one `_`, which stands for the piped value.
2. r[expr.pipe.slot.any] The step may be any expression at its precedence, not only a call, such as `_.len()`, `_ * 2`, or `Point { x: _, y: 0 }`.
3. r[expr.pipe.slot.meaning] The step is evaluated as if `_` were a name bound to the piped value, with that value's type and access.
4. r[expr.pipe.slot.suffix] Suffixes in the step apply inside it: in `x |> parse(_)?`, `?` propagates from `parse(x)`, and `x |> load!(_)` is a suspension call.
5. r[expr.pipe.slot.duplicate] A step with two or more `_` is an error. Error: `duplicate-pipe-placeholder`.
6. r[expr.pipe.slot.closure] A `_` inside a closure nested in the step is an error. Error: `pipe-placeholder-in-closure`.
7. r[expr.pipe.slot.nested] A `_` belongs to the innermost pipe step that contains it, and the rules above count only a step's own `_`. In `x |> f(_, y |> g(_))`, the second `_` belongs to `g(_)`, so the outer step has one `_`.

```text
fn add(left: i32, right: i32) -> i32: left + right

fn apply(value: i32, f: fn(i32) -> i32) -> i32: f(value)

fn doubled(n: i32) -> i32:
    n |> add(_, _)  # error: duplicate-pipe-placeholder

fn later(n: i32) -> i32:
    n |> apply(1, fn(v): v + _)  # error: pipe-placeholder-in-closure
```

> **Why.** `_` always shows where the value goes. A closure may run later,
> or many times, so a `_` inside one would not say which value it means.

### Bare Steps

A bare step names the function to call, so `x |> f` means `f(x)`:

```text
fn clean(raw: string) -> string:
    "[$raw]"

fn tidy(raw: string) -> string:
    raw |> clean
```

1. r[expr.pipe.bare.form] A bare step is an identifier, or identifiers joined by `.`, with no suffix after it.
2. r[expr.pipe.bare.call] `value |> path` evaluates as the call `path(value)`, except that `value` is evaluated first.
3. r[expr.pipe.bare.method] When the path's prefix names a value, the call is a method call on that value: `x |> user.greet` means `user.greet(x)`.
4. r[expr.pipe.bare.no-suspend] A bare step whose callee is a suspending function is an error. Write a substitution step, as in `x |> load!(_)`. Error: `suspending-pipe-step`.
5. r[expr.pipe.bare.needs-placeholder] A step without `_` that is not a bare step is an error. Error: `pipe-step-needs-placeholder`.
6. r[expr.pipe.bare.needs-placeholder.forms] That covers a call such as `x |> f(y)`, an index such as `x |> f[0]`, type arguments such as `x |> parse::[i32]`, and a suffix such as `x |> f?`.
7. r[expr.pipe.bare.method-reference] A [method reference](07-functions.md#method-references) without type arguments, such as `Config::parse`, is also a bare step: `raw |> Config::parse` means `Config::parse(raw)`.

```text
fn scale(value: i32, by: i32) -> i32: value * by

fn fetch!(id: i32) -> i32: id

fn tripled(n: i32) -> i32:
    n |> scale(3)  # error: pipe-step-needs-placeholder

fn loaded!(n: i32) -> i32:
    n |> fetch  # error: suspending-pipe-step
```

> **Why.** Elixir reads `x |> f(y)` as `f(x, y)`, and F# reads it as
> `f(y)(x)`. Rejecting the form removes both readings. A bare step is only
> a name or path, so an index or type arguments after it need `_` too. A
> suspending bare step would suspend with no visible `!`.

### Pipe Layout

1. r[expr.pipe.single-line] A step must not contain an indented suite, such as a multi-line `match`, `if`, or closure body. Error: `multi-line-pipe-step`.
2. r[expr.pipe.single-line.inline] A same-line closure or conditional inside a step is valid.
3. r[expr.pipe.no-trailing-block] A step takes no trailing block, because a trailing block call must be a complete statement or right-hand side, as [`fn.trailing.position`](07-functions.md#r-fn.trailing.position) says.
4. r[expr.pipe.lines] A chain may continue on lines that start with `|>`. A leading-dot line inside a chain is an error, as [Leading-Pipe Continuation](01-lexical-structure.md#leading-pipe-continuation) specifies.

```text
fn apply(value: i32, f: fn(i32) -> i32) -> i32: f(value)

fn bumped(n: i32) -> i32:
    n |> apply(_, fn(v):  # error: multi-line-pipe-step
        v + 1
    )
```

> **Why.** One line per step keeps a chain readable as a list of steps. For
> a longer step, bind a name first or extract a function.

### The Placeholder Outside Pipes

1. r[expr.pipe.placeholder-only] `_` has an expression meaning only in a pipe step.
2. r[expr.pipe.placeholder-outside] `_` as an expression anywhere else is an error. Error: `placeholder-outside-pipe`.
3. r[expr.pipe.no-partial] hd has no function placeholder: the partial application `f(_, a)` does not create a function.

```text
fn format(value: i32, width: i32) -> string:
    "$value/$width"

fn apply(value: i32, f: fn(i32) -> string) -> string: f(value)

fn run() -> string:
    apply(3, format(_, 8))  # error: placeholder-outside-pipe
```

> **Note.** Write the callback as a closure, such as
> `fn(value): format(value, 8)`, or as a
> [method reference](07-functions.md#method-references).

See also: [Precedence](#precedence),
[Leading-Pipe Continuation](01-lexical-structure.md#leading-pipe-continuation).

## Range Expressions

A range expression builds a range value from its bounds:

```text
use std.ops.{Range, RangeFrom}

fn halves(n: i32) -> (Range[i32], RangeFrom[i32]):
    middle := n / 2
    (0..middle, middle..)

fn wide(limit: i64) -> Range[i64]:
    0..limit

fn closed(limit: i64) -> Range[i64]:
    0..=limit
```

| Rule | Form | Type | Holds |
| --- | --- | --- | --- |
| r[expr.range.form.half-open] Half-open | `a..b` | `Range[T]` | the integers from `a` up to, not including, `b` |
| r[expr.range.form.from] From | `a..` | `RangeFrom[T]` | the integers from `a` up, with no end |
| r[expr.range.form.to] To | `..b` | `RangeTo[T]` | the integers below `b`, with no start |
| r[expr.range.form.through] Inclusive | `a..=b` | `Range[T]` | the integers from `a` through `b` |
| r[expr.range.form.to-through] To inclusive | `..=b` | `RangeTo[T]` | the integers up to and including `b`, with no start |
| r[expr.range.form.full] Full | `..` | `RangeFull` | every integer, with no start and no end |

`std.ops` declares the range types in this shape:

```text
pub data Range[T]:
    pub start: T
    pub end: T
    pub inclusive: bool

pub data RangeFrom[T]:
    pub start: T

pub data RangeTo[T]:
    pub end: T
    pub inclusive: bool

pub data RangeFull: pass
```

1. r[expr.range.declared.four] `std.ops` declares the four range types as data types whose fields are public, as shown above.
2. r[expr.range.value.fields] A range expression builds the range type of its form, with its bounds as the fields: `a..b` is `Range { start: a, end: b, inclusive: false }`.
3. r[expr.range.inclusive-field] The `inclusive` field of a `Range` or `RangeTo` is `true` for a `..=` form and `false` for a `..` form, so `..=b` is `RangeTo { end: b, inclusive: true }`.
4. r[expr.range.not-prelude] The range types are not prelude names. Code that names one in a type imports it, as in `use std.ops.Range`, but a range expression needs no use.
5. r[expr.range.order] A range expression evaluates its start bound, then its end bound.
6. r[expr.range.no-check] Building a range never compares its bounds, so `5..2` is a valid, empty range.
7. r[expr.range.bound.integer] Each bound must have an integer type. A bound of any other type is an error. Error: `type-mismatch`.
8. r[expr.range.bound.operands] The two bounds of `a..b` or `a..=b` are typed as the operands of a [binary numeric operator](04-type-system.md#binary-numeric-operators). A literal takes the other bound's type, and bounds of two types of one family are an error. Error: `type-mismatch`.
9. r[expr.range.bound.signedness] A signed and an unsigned bound are an error, as for a binary numeric operator. Error: `mixed-signedness`.
10. r[expr.range.element-type] The range's element type `T` is the bounds' common type, or the one bound's type for `a..`, `..b`, and `..=b`. `RangeFull` has no bound and no element type.
11. r[expr.range.expected] An expected range type gives each bound its element type as the bound's expected type, so `let r: Range[i64] = 0..10` has `i64` bounds.
12. r[expr.range.literal-bounds] With no expected type, literal bounds are one [literal class](04-type-system.md#r-types.literal.local.class): `0..3` is a `Range[usize]`, and `-3..3` is a `Range[i32]`.

```text
fn invalid(x: f64, count: u32, limit: i32, large: i64) -> void:
    a := 0.5..x        # error: type-mismatch
    b := count..limit  # error: mixed-signedness
    c := limit..large  # error: type-mismatch
```

> **Note.** `for` iterates `a..b`, `a..`, and `a..=b`, as
> [Range Iteration](06-control-flow.md#range-iteration) states. An index
> of a range type slices a string or a list with unsigned bounds, as
> [Slicing](#slicing) states. A `match` takes the range forms but `..b` and `..` as
> [range patterns](06-control-flow.md#range-patterns).

> **Why.** As in Rust, a range is an ordinary `std.ops` value. One syntax
> then serves loops, slicing, and any function that takes a range, with no
> separate `range` function.

> **Why.** `a..b` and `a..=b` share one type, as `..b` and `..=b` do, so a
> function that takes a `Range[i64]` accepts both forms. The types still
> keep a range with no start, which is not iterable, apart from one with a
> start.

See also: [Range Syntax](02-grammar.md#range-syntax),
[Binary Numeric Operators](04-type-system.md#binary-numeric-operators).

## Binding Expressions

`:=` binds names inside an expression:

```text
if (size := input.len()) > 0:
    println(size)
```

1. r[expr.bind.one-name] `:=` introduces one inferred, non-reassignable name and evaluates to the initializer's value.
2. r[expr.bind.precedence] `:=` has the lowest precedence.
3. r[expr.bind.parens] Parentheses are required when a binding appears as an operand of another expression, as above.
> **Note.** Destructuring is a `let` statement, as in
> `let (a, b) = pair`, never a `:=` binding, as
> [`grammar.stmt.short-binding.let-only`](02-grammar.md#r-grammar.stmt.short-binding.let-only)
> states.

See also: [Binding Expressions](03-names-and-scopes.md#binding-expressions),
which defines the binding's scope.

## Comprehensions

A list comprehension builds a list from nested iteration:

```text
pairs := [for x in xs for y in ys if x.id == y.owner_id => (x, y)]
```

1. r[expr.comp.list] A list comprehension evaluates clauses from left to right and appends one result for every path that reaches `=>`.
2. r[expr.comp.shape] A comprehension is equivalent in iteration shape to nested loops, with an `if` clause filtering at the exact point where it appears.
3. r[expr.comp.visibility] Names introduced by earlier clauses are visible to later clauses and the result.

### Map Comprehensions

A map comprehension generates keys and values:

```text
by_id := {for user in users if user.active => user.id: user}
```

1. r[expr.comp.map] A map comprehension evaluates the key and then the value for every successful path.
2. r[expr.comp.map.duplicate] Duplicate keys use the later generated value.

### Comprehension Restrictions

1. r[expr.comp.eager] Comprehensions are eager.
2. r[expr.comp.suspension] A comprehension follows the loops it abbreviates. So a bang call inside it is valid exactly where it would be valid in those loops: in a [driver context](11-requirements-and-suspension.md#r-req.bang.driver-contexts). Outside one it is an error. Error: `bang-call-outside-suspension`.
3. r[expr.comp.suspension.sequential] The bang calls of a comprehension run one at a time, in the order the loops reach them. Each call completes before the comprehension evaluates any later clause, guard, key, or element.
4. r[expr.comp.no-jumps] `return`, `break`, and `continue` are not valid inside a comprehension.
5. r[expr.comp.no-let] There is no comprehension `let` clause.

```text
fn fetch!(id: i32) -> string: "user"

fn names!(ids: List[i32]) -> List[string]:
    [for id in ids => fetch!(id)]

fn invalid(ids: List[i32]) -> List[string]:
    [for id in ids => fetch!(id)]  # error: bang-call-outside-suspension
```

> **Note.** Postfix `?` inside a comprehension returns from the nearest
> enclosing function or closure, as it would from the loop. The
> comprehension then stops: it evaluates no later clause, guard, or
> element.

In place of a `let` clause, use a parenthesized `:=` binding in a guard or
result expression:

```text
sizes := [for user in users
          if (size := user.name.len()) > 0
          => size]
```

## Closures And Control Expressions

1. r[expr.closure] Closures are expressions described in [Functions](07-functions.md).
2. r[expr.control] `if`, `match`, and loops are value-capable expressions described in [Control Flow](06-control-flow.md).

```hd
fn demo(flag: bool) -> i32:
    apply := fn(x: i32) -> i32: x * 2
    f := if flag: apply else: fn(x: i32) -> i32: x
    f(+21)
```

## Unsupported Expression Extensions

1. r[expr.unsupported.custom-operators] hd-lang has no user-defined operator symbols, and no overloading of the operators that [`expr.op.not-overloaded`](#r-expr.op.not-overloaded) lists.
2. r[expr.unsupported.chaining] hd-lang has no comparison chaining.
3. r[expr.unsupported.any-fallback] hd-lang has no fallback conversion of heterogeneous literals to `dyn Any`.
4. r[expr.unsupported.try-mapping] Postfix `?` has no mapping clause.
5. r[expr.unsupported.try-mapping.explicit] A site that needs a different error conversion maps the `Result` explicitly before `?`. One way is a function that takes a single-payload variant constructor as its mapper.

```text
fn inside(value: i32) -> bool:
    0 < value < 10   # error: comparison-chaining
```

See also: [Enum Declarations](08-data-and-enums.md#enum-declarations).
