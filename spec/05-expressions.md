# Expressions

Status: language specification draft.

This chapter defines expressions, which compute values.

1. r[expr.static-type] Every expression has a static type.
2. r[expr.evaluated] Every expression is evaluated according to the order defined here and in the relevant feature chapter.

## Evaluation Order

This section defines the default order in which subexpressions are evaluated.

1. r[expr.order.left-to-right] Unless a construct states otherwise, subexpressions are evaluated from left to right and exactly once.
2. r[expr.order.applies] This rule applies to tuple and collection elements, data fields, call arguments, operands, and indexing expressions.
3. r[expr.order.conditional] `&&`, `||`, `if`, `match`, loops, optional or result propagation with `?`, and comprehension filters evaluate conditionally as described below.
4. r[expr.order.reorder] A compiler may reorder only when it can prove that the program's observable behavior is unchanged.

## Expression Categories

This section defines value expressions and place expressions.

1. r[expr.category.value] A **value expression** produces a value.
2. r[expr.category.place] A **place expression** identifies a storage location and may be read or, when permissions allow, assigned.

### Places

The core place expressions are locals, fields, and index operations:

1. r[expr.place.local] A reassignable local introduced by `let` is a core place expression.
2. r[expr.place.field] A field selected through a mutable composite root is a core place expression.
3. r[expr.place.index] An index operation whose receiver and indexing protocol expose mutable storage is a core place expression.
4. r[expr.place.receiver] The receiver of a field or index place may be any expression of mutable composite type, including a call that returns mutable access.
5. r[expr.place.not-places] `:=` bindings, literals, calls, arithmetic, and temporary values are not places.
6. r[expr.place.tuple-element] A tuple element selection such as `pair._0` is not a place: tuples are immutable, and a changed tuple is built as a new tuple value.
7. r[expr.place.enum-shared-field] A shared enum field such as `status.phrase` is not a place: enum values never change once built.

### Assignment

1. r[expr.assign.statement] Assignment is a statement and requires a mutable place on its left side.
2. r[expr.assign.target] Assigning to an expression that is not a place is an error. Error: `invalid-assignment-target`.
3. r[expr.assign.order] Assignment evaluates the place before the right-hand side, then performs one store.
4. r[expr.assign.order.local] A local place requires no subexpression evaluation.
5. r[expr.assign.order.field] A field assignment evaluates its receiver, then the right-hand expression.
6. r[expr.assign.order.index] An indexed assignment evaluates the receiver, the index, and the right-hand expression in that order.
7. r[expr.assign.abrupt] If any step completes abruptly, no store occurs.
8. r[expr.assign.embedded] An embedded field is assigned with the copy assignment `place ...= value`, which stores a copy of the value.

```text
fn invalid() -> (i32, i32):
    let pair: (i32, i32) = (0, 0)
    pair._0 = 1  # error: invalid-assignment-target
    pair
```

See also: [Data Embedding](08-data-and-enums.md#data-embedding).

## Primary Expressions

This section defines names, literals, and parenthesized, tuple, collection,
and data expressions.

### Names

This section defines identifier, qualified, and contextual variant names.

1. r[expr.name.identifier] An identifier expression evaluates the declaration or local binding selected by lexical name resolution.
2. r[expr.name.qualified] A qualified name selects a declaration or enum variant through a module or type namespace.
3. r[expr.name.contextual] `.Variant` selects a variant only when the expression has an expected type that fixes one nominal enum.
4. r[expr.name.contextual.rules] `.Variant` has the same construction and argument rules as `Enum.Variant`; a payload-bearing variant still requires a call.
5. r[expr.name.contextual.no-search] The compiler does not search all visible enums for a matching variant name.
6. r[expr.name.contextual.no-type] Without a unique expected enum type, `.Variant` is a type error.

```text
enum Status:
    Queued

fn invalid() -> void:
    status := .Queued  # error
    pass
```

See also: [Enum Declarations](08-data-and-enums.md#enum-declarations).

### Literals

1. r[expr.literal.defined] Boolean, integer, floating-point, string, and character literals are defined lexically in [Lexical Structure](01-lexical-structure.md) and typed in [Type System](04-type-system.md).
2. r[expr.literal.suffixed] A suffixed literal is a call, as [Literal Suffixes](#literal-suffixes) specifies.
3. r[expr.literal.prefixed] A prefixed string is a call, as [Prefixed Strings](#prefixed-strings) specifies.
4. r[expr.literal.pass] `pass` is the no-op expression. It has type `void` and performs no operation.

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
9. r[expr.interp.user] Optional and user-defined values are displayable only when the corresponding type implements `Display`.
10. r[expr.interp.prefixed] A prefixed string does not append its values: they become the values of a template, as [Prefixed Strings](#prefixed-strings) specifies.
11. r[expr.interp.constant] A string with no interpolation segments is an ordinary constant value and performs no `Display` calls.

```text
data Secret:
    value: i32

fn render(value: Secret) -> string:
    "secret=$value"  # error: unsatisfied-trait-bound
```

#### Literal Suffixes

A [suffixed literal](01-lexical-structure.md#literal-suffixes) is a call of
its suffix function, a function marked `@num_suffix`:

```text
use std.ops.num_suffix

data Pixels:
    count: i32

@num_suffix
fn px(count: i32) -> Pixels:
    Pixels { count: count }

fn indent() -> Pixels:
    -12px  # px(-12)
```

`std.ops` declares the marker:

```text
use std.annotation.annotate

@annotate(.Fn)
pub data NumSuffix: pass

pub fn num_suffix() -> NumSuffix:
    NumSuffix {}
```

1. r[expr.suffix.fn-call] A suffixed literal `Nx` is the call `x(N)` of the suffix function `x`, with the literal `N` as its one argument.
2. r[expr.suffix.fn-call.example] So `250ms` means `ms(250)`, and `-5s` means `s(-5)`.
3. r[expr.suffix.marker] A **suffix function** is a function that carries a `std.ops.NumSuffix` value, written `@num_suffix`. The compiler recognizes `std.ops.NumSuffix` by its qualified name.
4. r[expr.suffix.marker.module] `std.ops` declares `NumSuffix` and `num_suffix`. `NumSuffix` carries `@annotate(.Fn)`, so `@num_suffix` before anything but a function is an error. Error: `decorator-not-annotator`.
5. r[expr.suffix.not-marked] A suffix that resolves to anything other than a suffix function is an error, reported at the literal. Error: `invalid-literal-suffix`.
6. r[expr.suffix.no-marker-import] The call needs no import of `num_suffix` or `NumSuffix`: a `use` of the suffix function alone makes the literal valid.
7. r[expr.suffix.call-errors] The call is checked as an ordinary call. An argument that the function cannot accept is the ordinary call error at the literal, such as `type-mismatch` or `argument-count`.
8. r[expr.suffix.fn-shape-one] A suffix function declares exactly one parameter, of a primitive integer or floating-point type, and never suspends. A second parameter breaks this shape even when it has a default.
9. r[expr.suffix.fn-shape.definition] The compiler checks this shape at the definition that carries `@num_suffix`, not at each literal. A marked function that breaks it is an error at that definition. Error: `type-mismatch`.
10. r[expr.suffix.ordinary-rules] Otherwise the call follows the ordinary rules. A generic suffix function's type arguments are inferred at the literal, and its requirement row joins the row of the code that contains the literal, as any call's does.
11. r[expr.suffix.exact-call] A suffixed literal is exactly that call wherever it appears, and it has no evaluation rule of its own.
12. r[expr.suffix.position-rules] In a [fact](14-annotations.md#r-annot.fact.eval), in [shared enum data](08-data-and-enums.md#r-data.shared.compile-time), or in any other position, the call follows the rules for any call there, including what a panic in the suffix function does. So a compile-time position that must be requirement-free rejects a suffix function that needs providers, as it rejects any such call.

```text
use std.ops.num_suffix

pub data Pixels:
    count: i32

fn pt(count: i32) -> Pixels:
    Pixels { count: count }

@num_suffix
fn em(label: string) -> Pixels:  # error: type-mismatch
    Pixels { count: 0 }

@num_suffix
fn later!(count: i64) -> i64:  # error: type-mismatch
    count

@num_suffix
fn kb(count: i64, unit: i64 = 1024) -> i64:  # error: type-mismatch
    count * unit

@num_suffix
fn px(count: i32) -> Pixels $ Console:
    Pixels { count: count }

pub fn layout() -> Pixels:
    size := 12pt  # error: invalid-literal-suffix
    12px  # error: missing-requirement
```

The standard library declares these suffixes in `std.time`:

| Rule | Suffix | Meaning |
| --- | --- | --- |
| r[expr.suffix.std.ms] Milliseconds | `ms` | `Duration` of that many milliseconds |
| r[expr.suffix.std.s] Seconds | `s` | `Duration` of that many seconds |
| r[expr.suffix.std.min] Minutes | `min` | `Duration` of that many minutes |
| r[expr.suffix.std.h] Hours | `h` | `Duration` of that many hours |

1. r[expr.suffix.std.fn] Each is a suffix function that takes one `i64` and returns the standard `std.time.Duration`, as in `@num_suffix pub fn ms(count: i64) -> Duration`.
2. r[expr.suffix.std.duration] A `std.time.Duration` is a whole number of milliseconds, held in an `i64`.
3. r[expr.suffix.std.only-four] These four are the only standard suffixes. `std` declares no `ns`, `us`, `m`, `d`, byte-size, or string suffix.
4. r[expr.suffix.std.import] None is a prelude name; code imports them, as in `use std.time.{ms, s}`.
5. r[expr.suffix.std.duration-api] The public API of `Duration` is `Duration::milliseconds(n: i64)`, `Duration::seconds(n: i64)`, and `d.as_milliseconds() -> i64`.
6. r[expr.suffix.std.overflow] A standard suffix call whose result does not fit in `i64` milliseconds, as in `10_000_000_000_000_000h`, panics at run time, as checked `i64` arithmetic does. Panic: `integer-overflow`.

```text
use std.time.{Duration, ms, s}

enum Tier(limit: Duration):
    Fast -> Tier(limit=250ms)
    Slow -> Tier(limit=5s)
```

> **Why.** A suffix is an ordinary function found through `use`, so
> libraries can add `12px` without new syntax. The literal's type is the
> function's result, so `5s` and `250ms` are both `Duration` and mix
> freely. The literal is plain call sugar, so the one rule kept is that it
> never suspends: hd marks every suspending call with `!`, and `5s` has no
> place to show it. The compiler reads the marker, so it checks the
> signature once, at the marked definition.

> **Note.** A compiler may warn when a suffixed literal always overflows,
> as Rust's `unconditional_panic` lint does, but none is required.

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

@annotate(.Fn)
pub data StrPrefix: pass

pub fn str_prefix() -> StrPrefix:
    StrPrefix {}

pub data Template[T]:
    pub raw_parts: List[string]
    pub values: List[T]
```

1. r[expr.prefix.fn-call] A prefixed string `x"..."` is the call `x(t)` of the prefix function `x`, with a `std.ops.Template` value `t` as its one argument.
2. r[expr.prefix.template] `t.values` holds the `n` interpolated values in source order, and `t.raw_parts` holds the `n + 1` pieces of text around them.
3. r[expr.prefix.parts] The first piece is the text before the first interpolation, and the last piece is the text after the last one. A piece is `""` where two interpolations touch, or where one begins or ends the string.
4. r[expr.prefix.parts.example] So `x"a $b c"` passes the pieces `["a ", " c"]` and the values `[b]`, and `x"text"` passes `["text"]` and `[]`.
5. r[expr.prefix.raw-parts] Each piece is the text exactly as written, with every backslash kept, as [`lex.prefix.raw-text`](01-lexical-structure.md#r-lex.prefix.raw-text) says.
6. r[expr.prefix.no-join] The compiler never joins the pieces and never calls `Display`. The prefix function decides what the string means.
7. r[expr.prefix.order] The interpolated expressions are evaluated from left to right before the call, as arguments are.
8. r[expr.prefix.marker] A **prefix function** is a function that carries a `std.ops.StrPrefix` value, written `@str_prefix`. The compiler recognizes `std.ops.StrPrefix` and `std.ops.Template` by their qualified names.
9. r[expr.prefix.marker.module] `std.ops` declares `StrPrefix`, `str_prefix`, and `Template`. `StrPrefix` carries `@annotate(.Fn)`, so `@str_prefix` before anything but a function is an error. Error: `decorator-not-annotator`.
10. r[expr.prefix.not-marked] A prefix that resolves to anything other than a prefix function is an error, reported at the string. Error: `invalid-string-prefix`.
11. r[expr.prefix.no-marker-import] The call needs no import of `str_prefix`, `StrPrefix`, or `Template`: a `use` of the prefix function alone makes the string valid.
12. r[expr.prefix.fn-shape-one] A prefix function declares exactly one parameter, of type `std.ops.Template[T]` for some type `T`, and never suspends. A second parameter breaks this shape even when it has a default.
13. r[expr.prefix.fn-shape.definition] The compiler checks this shape at the definition that carries `@str_prefix`, not at each string. A marked function that breaks it is an error at that definition. Error: `type-mismatch`.
14. r[expr.prefix.ordinary-rules] Otherwise the call is checked as an ordinary call. A generic prefix function's type arguments are inferred at the string, and its requirement row joins the row of the code that contains the string, as any call's does.
15. r[expr.prefix.exact-call] A prefixed string is exactly that call wherever it appears, and it has no evaluation rule of its own. In a fact or any other compile-time position, it follows the rules for any call there.

```text
use std.ops.{Template, str_prefix}

fn plain(t: Template[string]) -> string:
    "plain"

@str_prefix
fn count(n: i32) -> i32:  # error: type-mismatch
    n

@str_prefix
fn later!(t: Template[string]) -> string:  # error: type-mismatch
    "later"

@str_prefix
fn tagged(t: Template[string], tag: string = "x") -> string:  # error: type-mismatch
    tag

@str_prefix
fn logged(t: Template[string]) -> string $ Console:
    "logged"

pub fn render() -> string:
    first := plain"x"  # error: invalid-string-prefix
    logged"x"  # error: missing-requirement
```

The standard library declares one prefix, in `std.text`:

| Rule | Declaration | Meaning |
| --- | --- | --- |
| r[expr.prefix.std.r] Raw text | `@str_prefix pub fn r(t: Template[Display]) -> string` | the pieces joined with the values' `Display` text, with no escape processed |

1. r[expr.prefix.std.r-meaning] So `r"\d+ $n"` is the text `\d+ ` followed by the `Display` text of `n`, and `r"a\"b"` keeps its backslash.
2. r[expr.prefix.std.only-r] `r` is the only standard prefix. `std` declares no `b`, so `b"..."` names nothing until a bytes type exists.
3. r[expr.prefix.std.import-text] `std.text` declares `r`. It is not a prelude name; code imports it, as in `use std.text.r`.

```text
use std.text.r

fn digits(count: i32) -> string:
    r"\d{$count}"  # the text \d{ then count, then }
```

> **Why.** A prefix is an ordinary function found through `use`, so a
> library adds `sql"..."` without new syntax. The template keeps text and
> values apart, so `sql` can send values as parameters instead of splicing
> them into the query text. As for a suffix, the string is plain call
> sugar and never suspends, because `sql"..."` has no place for `!`.

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
5. r[expr.list.spread.type] A spread's operand must have a list type `List[U]`.
6. r[expr.list.spread.insert] The spread's elements are inserted at its position, in order.
7. r[expr.list.spread.position] A literal may contain several spreads in any position, so `[0, xs...]` and `[xs..., ys...]` are valid.
8. r[expr.list.spread.expected] With an expected `List[T]`, `U` must be assignable to `T`.
9. r[expr.list.spread.inferred] Without an expected `List[T]`, a spread contributes `U` to the least common type of the elements.

```text
fn numbers(count: i32) -> List[i32]:
    [0, count...]  # error
```

#### Map Literals

1. r[expr.map.order] A map literal evaluates each key and then its value, processing entries from left to right.
2. r[expr.map.duplicate] If two evaluated entries have equal keys, the later value replaces the earlier value without changing that key's first insertion position.
3. r[expr.map.iteration] Map iteration is in insertion order and is not part of map equality.
4. r[expr.map.keys] Key validity and equality/hash requirements are defined in [Type System](04-type-system.md#map-key-types).

#### Element Types

1. r[expr.collection.empty] Empty `[]` and `{}` literals require an expected collection type.
2. r[expr.collection.empty.error] Without an expected collection type, an empty literal is an error. Error: `unresolved-generic-placeholder`.
3. r[expr.collection.expected] When an expected `List[T]` or `Map[K, V]` type is available, each literal element is checked directly against the corresponding expected type.
4. r[expr.collection.inferred] Without an expected type, a list's element type is the [least common type](04-type-system.md#least-common-type) of its elements. A map's key type and value type are the least common types of its keys and of its values.
5. r[expr.collection.inferred.rows] Function values with different rows take the union of their rows first, as [Row Union In Literals](11-requirements-and-suspension.md#row-union-in-literals) states.

```text
fn main() -> void:
    values := []  # error: unresolved-generic-placeholder
    others := {}  # error: unresolved-generic-placeholder
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
2. r[expr.data.any-order] Fields may appear in any order.
3. r[expr.data.once] Every field may appear at most once.
4. r[expr.data.unknown] An unknown field is a compile-time error.
5. r[expr.data.eval] Field initializers evaluate in source order, not declaration order.
6. r[expr.data.embedded] Embedded fields are initialized with their embedded type name as the field key and a copy marker, as in `Timestamps: ...stamps`.
7. r[expr.data.embedded.copy] Each embedded field receives a copy of its value.

See also: [Data Literals](08-data-and-enums.md#data-literals),
[Data Embedding](08-data-and-enums.md#data-embedding).

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
10. r[expr.update.embedded] Each embedded part that is not replaced is copied, as construction copies it.
11. r[expr.update.embedded.readonly] The copy of a part read through a readonly source has mutable access only when the part's type has no mutable edges.
12. r[expr.update.shallow] Copy-update is shallow: primitive fields are copied by value, while composite field references continue to refer to the same underlying objects.
13. r[expr.update.shallow.embedded] Embedded parts are the exception: the copy receives copies of them and never shares a part with its source.
14. r[expr.update.no-upgrade] A fresh mutable outer result does not upgrade copied child references.

See also: [Copy-Update Literals](08-data-and-enums.md#copy-update-literals),
[Mutable Edges](08-data-and-enums.md#mutable-edges).

## Postfix Expressions

1. r[expr.postfix.binding] Postfix operations bind more tightly than every infix operator.

### Member Access

`value.member` selects a member, and `tuple._0` selects a tuple element.

1. r[expr.member.select-identifier] `value.member` selects a member named by an identifier, and `tuple._0` selects a tuple element.
2. r[expr.member.field-read] Without an argument clause, `value.name` reads a field.
3. r[expr.member.lookup] [Member Resolution](03-names-and-scopes.md#member-resolution) defines field and method lookup, including promoted members and the declaration-time check `ambiguous-promoted-member`.
4. r[expr.member.enum-variant] `Enum.Variant` is not member access: it names an enum variant.
5. r[expr.member.variant-fn-value] A variant constructor with exactly one payload field is a function value.

See also: [Enum Declarations](08-data-and-enums.md#enum-declarations).

#### Method Calls

1. r[expr.member.method-call] When a member suffix is immediately followed by an argument clause, `value.name(arguments...)` is a method call: member lookup selects a method, which is called with `value` as its receiver.
2. r[expr.member.no-field-call] A method call never reads a field.
3. r[expr.member.stored-fn] A function stored in a field is called by parenthesizing the field read, as in `(handler.callback)(event)`.
4. r[expr.member.stored-fn.error] `handler.callback(event)` looks for a method named `callback`, and is an error when there is none. Error: `unknown-method`.
5. r[expr.member.stored-fn.hint] When a field named `callback` exists, the `unknown-method` message should suggest `(handler.callback)(event)`.
6. r[expr.member.trait-available] A method call never finds a method of a trait that is not available at the call, even when the receiver's type implements it.
7. r[expr.member.trait-hint] When such a method is the only one with the name, the `unknown-method` message should suggest a use declaration for its trait.
8. r[expr.member.embedded-trait] A trait method of an embedded type is never found through the outer value; it is called through the embedded field, as in `page.Label.to_string()`.
9. r[expr.member.method-not-value] A method is not a value: `value.method` without an argument clause is a field read.
10. r[expr.member.method-values] Method values are deferred, and their future spellings `Type::name` and `value::name` are reserved and diagnosed.
11. r[expr.member.closure-adapt] Explicit closures can adapt method calls where a function value is needed.

```text
data Button:
    on_click: fn(i32) -> i32

fn invalid(button: Button) -> i32:
    button.on_click(41)  # error: unknown-method
```

See also: [Unsupported Function Extensions](07-functions.md#unsupported-function-extensions).

#### Readonly Roots

1. r[expr.member.readonly-root] Member access through a readonly data root weakens a direct `mut U` field to `U` and an embedded field to readonly access.
2. r[expr.member.readonly-generic] Member access through a readonly data root does not weaken a generic field's substituted type.
3. r[expr.member.other-forms] Other member forms follow their own access rules in [Type System](04-type-system.md).

### Indexing

1. r[expr.index.order] `receiver[index]` evaluates the receiver, then the index, and invokes the receiver type's indexing behavior.

#### List Indexing

1. r[expr.index.list.type] For `List[T]`, an index may have any integer type.
2. r[expr.index.list.range] A list index must be non-negative and less than the list length.
3. r[expr.index.list.panic] A failed check causes the standard checked runtime panic.
4. r[expr.index.list.read] Reading a list element yields its declared generic type `T`, including `mut U` when `T = mut U`, regardless of the list root's permission.
5. r[expr.index.list.assign] Assigning `items[index] = value` still requires a mutable list root and an in-range index.

#### Map Indexing

1. r[expr.index.map.type] For `Map[K, V]`, the index must have type `K`.
2. r[expr.index.map.read] Reading `entries[key]` returns `V?`: `.None` means no equal key exists.
3. r[expr.index.map.generic] The generic `V` is preserved through a readonly map, including `mut U` when `V = mut U`; unwrapping the optional returns `V`.
4. r[expr.index.map.assign] Assigning `entries[key] = value` requires `mut Map[K, V]` and inserts or replaces the entry.
5. r[expr.index.map.library] Removal and entry APIs are standard-library methods rather than special syntax.

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
2. r[expr.call.named.unknown] A named argument that names no parameter is an error. Error: `unknown-named-argument`.
3. r[expr.call.exactly-once] A parameter must receive exactly one argument after defaults are applied.
4. r[expr.call.duplicate] Supplying one parameter more than once, such as positionally and again by name, is an error. Error: `duplicate-argument`.
5. r[expr.call.source-order] Evaluation order follows source argument order, not parameter declaration order.

```text
fn resize(width: i32, height: i32) -> i32: width * height

fn area() -> i32:
    unknown := resize(640, depth=480)  # error: unknown-named-argument
    twice := resize(640, width=480)    # error: duplicate-argument
    unknown + twice
```

#### Positional Spreads

1. r[expr.call.spread] An argument ending in `...` is a positional spread.
2. r[expr.call.spread.type] A positional spread is evaluated once, must have `List[T]` compatible with the callee's final `T...` parameter, and supplies that vararg's remaining positional elements.
3. r[expr.call.spread.fixed] A positional spread cannot fill fixed parameters.
4. r[expr.call.spread.needs-vararg] A positional spread in a call whose callee has no vararg parameter is an error. Error: `positional-spread-needs-vararg`.
5. r[expr.call.spread.one] A call has at most one positional spread; it must be the final positional argument and therefore precedes every named argument.
6. r[expr.call.vararg-by-name] Passing a vararg by name uses one ordinary list value without `...`.
7. r[expr.call.vararg-by-name.exclusive] A call that passes a vararg by name must not also supply positional values for that vararg.

```text
fn fixed(value: i32) -> i32: value
fn total(values: List[i32]) -> i32: fixed(values...)  # error: positional-spread-needs-vararg
```

#### Explicit Generic Arguments

Explicit generic arguments occur before the call argument list:

```text
first[string](names)
```

1. r[expr.call.generic.position] Generic arguments, when explicit, occur before the call argument list.
2. r[expr.call.generic.complete] The complete generic argument list must be supplied; partial explicit lists are not supported.
3. r[expr.call.generic.targets] In hd-lang, explicit arguments may specialize a named module function or qualified function introduced by a use declaration.
4. r[expr.call.generic.methods] The same explicit-list rules apply to generic methods.

See also: [Functions](07-functions.md).

#### Suspension Calls

1. r[expr.call.suspension] Suspension calls with `!` construct and drive a child suspension as specified in [Requirements and Suspension](11-requirements-and-suspension.md).
2. r[expr.call.suspension.order] The callee and arguments are evaluated left to right before the child begins execution.

### Propagation

Postfix `?` handles either an optional or a `Result` value:

| Rule | Operand | Value case | Early return |
| --- | --- | --- | --- |
| r[expr.try.option] Optional | `T?` | `.Some(value)` produces `value` as `T`. | `.None` immediately returns `.None` from the nearest function. |
| r[expr.try.result] Result | `Result[T, E]` | `.Ok(value)` produces the declared `T`, including a mutable type argument. | `.Err(error)` immediately returns `.Err` from the nearest function, holding the error converted as [Error Conversion](#error-conversion) describes. |

1. r[expr.try.once] The operand is evaluated once.
2. r[expr.try.no-panic] `?` does not catch runtime panics and does not interact with suspension by itself.

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

1. r[expr.try.convert.assignable.covers] The assignability step covers an identical type, numeric widening, permission weakening, and variance. It also covers construction of a dynamic trait value such as the erased `Error`, supertrait widening of a dynamic value, and optional injection.
2. r[expr.try.convert.strip-mut] In the conversion step, an outer `mut` on `E` is removed before the implementation is chosen.
3. r[expr.try.convert.no-import] The code using `?` does not need to import `From`.
4. r[expr.try.convert.message] The message of the third step's error should name `E` and `F`, and suggest an implementation of `From[E]` for `F` or an explicit mapping of the error.
5. r[expr.try.convert.one-step] Exactly one step converts the error.
6. r[expr.try.convert.no-combine] `?` never combines an assignability rule with a conversion, and it never chains conversions.
7. r[expr.try.convert.no-chain] With `impl From[A] for B` and `impl From[B] for C`, a `?` on `Result[T, A]` in a function returning `Result[U, C]` is an error. Error: `invalid-result-propagation`.
8. r[expr.try.convert.no-injection] With the same implementations, a `?` on `Result[T, A]` in a function returning `Result[U, B?]` is also an error. The conversion to `B` would need an optional injection after it. Error: `invalid-result-propagation`.
9. r[expr.try.convert.single-rule] Assignability is itself one rule: an `A` that implements the dynamically safe trait `Tr` does not propagate into `Result[U, Tr?]`.
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
        .Ok()

    it("a body without ? is void"):
        match parse_digit("x"):
            .Ok(_) => panic("x is not a digit")
            .Err(error) => assert_equal(error.to_string(), "not a digit: x", reason="the helper rejects x")
```

1. r[expr.try.test.fixed-result] When a trailing block is the body of an [`it` call](10-modules.md#test-cases), its result type is fixed rather than inferred.
2. r[expr.try.test.with-try] If the block contains a `?` outside any nested closure, its result type is `Result[void, Error]`, where `Error` is the erased `std.error.Error`.
3. r[expr.try.test.without-try] Otherwise its result type is `void`.
4. r[expr.try.test.converts] `?` in such a block converts by the ordinary rules, so an error type that implements `Error` propagates into the erased `Error`. An error type that does not is an error. Error: `invalid-result-propagation`.
5. r[expr.try.test.final-value] The block's final value must be assignable to its result type, as for a function body, so a block that uses `?` usually ends in `.Ok()`.
6. r[expr.try.test.explicit-closure] A body passed as an explicit closure keeps its written or inferred result type. That type must implement `std.process.Termination`, the bound on `it`. Error: `unsatisfied-trait-bound`.
7. r[expr.try.test.closure] Inside a closure nested in a test body, that closure is the nearest function, and these rules do not apply to it.
8. r[expr.try.test.row-body] The body closure of an [`it_each`](10-modules.md#table-tests), `it_prop`, or `it_prop_with` call that writes no result type gets its result type by rules 2 and 3, as a trailing block given to `it` does.

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
        .Ok()
```

> **Why.** A fixed `Result[void, Error]` lets one test body use `?` on
> several error types, as Zig's inferred `anyerror!void` test bodies do.

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
| Comparison | `==`, `!=`, `<`, `<=`, `>`, `>=`, `is` | non-associative |
| Logical AND | `&&` | left, short-circuiting |
| Logical OR | `\|\|` | left, short-circuiting |
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

### Arithmetic Operators

1. r[expr.arith.numeric] Arithmetic operators require compatible numeric operands.
2. r[expr.arith.non-numeric] A binary `+`, `-`, `*`, `/`, `%`, or `**` with an operand of a non-numeric type, such as `true + false` or `[1] * [2]`, is an error. Error: `type-mismatch`.
3. r[expr.arith.string] `string + string` is the only non-numeric arithmetic form.
4. r[expr.arith.defined] Mixed-width result types, overflow, division, and shifts are defined in [Type System](04-type-system.md).
5. r[expr.arith.cast] Signed/unsigned and integer/floating mixing requires an explicit cast.
6. r[expr.arith.int.checked] For compatible integer operands, `+`, `-`, and `*` produce the common integer type and use checked arithmetic.
7. r[expr.arith.int.divide] `/` truncates toward zero, `%` produces the corresponding remainder, and a zero divisor panics.
8. r[expr.arith.unary-plus] Unary `+` accepts all numeric types, preserves its operand's type and value, and evaluates the operand once.
9. r[expr.arith.unary-minus] Unary `-` accepts signed integers and floating-point values, but not unsigned integers.

```text
fn sum(a: bool, b: bool) -> bool: a + b                     # error: type-mismatch
fn product(a: List[i32], b: List[i32]) -> List[i32]: a * b  # error: type-mismatch
```

### Bitwise Operators

1. r[expr.bit.integer] `~`, `&`, `|`, and `^` accept integer values only and produce the operand common type.
2. r[expr.bit.non-integer] A binary `&`, `|`, or `^` with an operand that is not an integer, such as a `bool`, floating-point, or `string` operand, is an error. Error: `type-mismatch`.

```text
fn both(a: bool, b: bool) -> bool: a & b          # error: type-mismatch
fn either(a: f64, b: f64) -> f64: a | b           # error: type-mismatch
fn toggle(a: string, b: string) -> string: a ^ b  # error: type-mismatch
```

### Shifts

1. r[expr.shift.unification] Shifts are the exception to ordinary binary numeric unification.
2. r[expr.shift.operands] The left operand may have any integer type, the right operand may have any integer type, and the result preserves the left operand type.
3. r[expr.shift.count] A negative count or a count at least as large as the left operand's bit width panics.
4. r[expr.shift.fixed-width] The shift itself is a fixed-width bit operation; left-shifted high bits are discarded rather than reported as arithmetic overflow.

### Exponentiation

1. r[expr.power.int.exponent] For an integer base, `**` requires an exponent of an unsigned integer type, so a negative exponent cannot occur.
2. r[expr.power.int.literal] An unsuffixed integer literal in exponent position has type `u32`.
3. r[expr.power.int.signed] A signed integer exponent is an error. Error: `type-mismatch`.
4. r[expr.power.negated-literal] A negated literal is signed: in `2 ** -1` the literal `1` is the operand of unary `-`, not the exponent itself. So `-1` has a signed type, and the expression is a compile-time error. Error: `type-mismatch`.
5. r[expr.power.negated-literal.not-other] That error is not `unsigned-negation` and not a runtime panic.
6. r[expr.power.checked] Exponentiation uses checked multiplication in the base's result type.
7. r[expr.power.float.exponent] For a floating-point base, the exponent must be floating point after ordinary floating widening.
8. r[expr.power.float.pow] Floating `**` computes IEEE 754-2019 `pow` as specified in clause 9.2, including its special cases, and rounds the result correctly to the destination format.
9. r[expr.power.mixed] Integer and floating operands do not mix without an explicit cast; a mixed power expression is an error. Error: `mixed-numeric-types`.

```text
fn power(exponent: i32) -> i32: 2 ** exponent  # error: type-mismatch
fn inverse() -> i32: 2 ** -1                   # error: type-mismatch
fn main() -> f64: 2 ** 2.0                     # error: mixed-numeric-types
```

### Floating-Point Arithmetic

1. r[expr.float.basic] Floating `+`, `-`, `*`, and `/` use the corresponding required IEEE 754 basic operation, including infinities, signed zero, and NaN.
2. r[expr.float.power] Floating `**` uses the `pow` rule of [Exponentiation](#exponentiation).
3. r[expr.float.remainder] `%` is integer-only.

### Logical Operators

1. r[expr.logic.not] Prefix `!` requires `bool`.
2. r[expr.logic.bool] `&&` and `||` require `bool` operands and produce `bool`.
3. r[expr.logic.short-circuit] `&&` and `||` evaluate the right operand only when needed.

### Equality

1. r[expr.eq.calls-eq] `==` calls `Eq.eq` and `!=` negates that result.
2. r[expr.eq.std] Standard-library implementations provide value equality for primitives, optional and result values, tuples, lists, and maps when their elements support equality.
3. r[expr.eq.map-order] Map equality is independent of entry order.
4. r[expr.eq.no-implicit] A user-defined data or enum type has no implicit `Eq` implementation, even if all its members are comparable. Its author must explicitly implement or request derivation of the trait.
5. r[expr.eq.no-identity-fallback] Equality never silently falls back to reference identity.
6. r[expr.eq.float] Floating-point equality follows IEEE 754, so NaN is unequal even to itself, although floating-point types implement `Eq`.
7. r[expr.eq.functions] Function and closure values do not implement `Eq`; applying `==` or `!=` to them is an error. Error: `unsupported-equality`.

```text
fn invalid(left: fn() -> void, right: fn() -> void) -> bool:
    left == right  # error: unsupported-equality
```

### Ordering

1. r[expr.ord.partial-cmp] `<`, `<=`, `>`, and `>=` use `PartialOrd.partial_cmp`.
2. r[expr.ord.std] The standard library implements `PartialOrd.partial_cmp` for compatible numeric values, characters, strings, tuples, lists, and optionals.
3. r[expr.ord.std.text] Characters are ordered by Unicode scalar value, and strings lexicographically by scalar value.
4. r[expr.ord.std.sequences] Tuples and lists are ordered lexicographically.
5. r[expr.ord.std.optional] Optionals are ordered with `.None` before every present value.
6. r[expr.ord.composite] Composite ordering is available when the corresponding elements implement the comparison trait, and it stops at the first unequal or unordered element.
7. r[expr.ord.user] Users can implement comparison traits for their own types.
8. r[expr.ord.total] `Ord` is the total-order refinement; floating-point types have `PartialOrd` but not `Ord` because NaN is unordered.
9. r[expr.ord.unordered] An unordered comparison makes all four relational operators false.

### Identity

`is` compares identity without invoking user code.

1. r[expr.is.no-user-code] `is` compares identity without invoking user code.

#### Allocation Identity

1. r[expr.is.heap] Data values, stored enum payloads, lists, maps, and other heap composites have allocation identity; access permission (`mut`) does not change it.
2. r[expr.is.conversion] Converting such a value to a trait value or `Any` preserves the underlying identity.
3. r[expr.is.function-unspecified] Function values implement `AnyRef`, but the identity of each one is unspecified. This covers named functions, generic instantiations, one-payload variant constructors, and closures.
4. r[expr.is.function-sharing] An implementation may share one function value between evaluations or allocate a new one at each evaluation.
5. r[expr.is.box] A conversion of a primitive or tuple value to a dynamic trait value or `Any` allocates one fresh immutable box.
6. r[expr.is.box.identity] The resulting trait or `Any` value has that box's identity, and aliases of the converted value share it.
7. r[expr.is.box.distinct] Repeating the conversion allocates a distinct box even when the source values compare equal.
8. r[expr.is.no-wrapper] A direct conversion of a heap composite continues to preserve the composite's underlying identity and does not allocate an identity wrapper.
9. r[expr.is.canonical] A payload-free enum value is canonical for its variant, and a fieldless data value is canonical for its data type.
10. r[expr.is.canonical.same] Two occurrences of the same such value have the same identity, and constructing one allocates nothing.
11. r[expr.is.shared-data-canonical] Shared constructor data is not stored in enum values, so a variant without a payload is canonical even when its enum declares shared data.
12. r[expr.is.none] Optionals follow the same enum rules: `.None` is payload-free and canonical, so every `.None` of one optional type is the same value.
13. r[expr.is.some] Each construction of `.Some(value)`, including the implicit wrap of a `T` where `T?` is expected, has its own identity, distinct from its payload's.

#### Identity Operands

1. r[expr.is.tuple] Tuples have no identity, and using `is` with a tuple is rejected even if it contains references.
2. r[expr.is.primitive] Primitive values likewise cannot be compared with `is`: an operand type must implement `AnyRef`, not `AnyVal`.
3. r[expr.is.compatible] Both operands must otherwise have compatible composite reference types. Two such types are compatible when, after removing `mut` at every level, they are equal, or one is a trait value or `Any` type that the other converts to.
4. r[expr.is.permissions] Permissions never affect identity, so `List[User]` and `mut List[mut User]` are compatible.
5. r[expr.is.incompatible] Two composite reference operands that are not compatible, such as `List[User]` and `List[Order]`, are an error. Error: `incompatible-identity-operands`.
6. r[expr.is.function] A direct `is` with an operand whose static type is a function type is an error. Error: `unsupported-function-identity`.
7. r[expr.is.function.generic] Generic code over `T < AnyRef` may still compare function values with `is`, and the result is unspecified.

```text
data User:
    name: string

data Order:
    total: i32

fn same(user: User, order: Order) -> bool:
    user is order  # error: incompatible-identity-operands

fn tuples(left: (i32, i32), right: (i32, i32)) -> bool:
    left is right  # error
```

```text
fn aliases() -> bool:
    callback := fn() -> i32: 1
    alias := callback
    callback is alias  # error: unsupported-function-identity
```

> **Note.** Use `!(a is b)` for distinct identities. Code that must later
> remove a registered callback keeps a handle returned at registration
> instead of comparing function values.

> **Why.** Function identity is unspecified so that an implementation may
> share or allocate function values freely. A direct comparison would expose
> that choice.

See also: [Trait Values And `Any`](04-type-system.md#trait-values-and-any).

### Operator Traits

1. r[expr.op.builtin] Arithmetic and bitwise operators are built in for the numeric types specified by this chapter and [Type System](04-type-system.md).
2. r[expr.op.concat] `string + string` concatenates strings.
3. r[expr.op.traits] Comparison traits are the only operator traits; other user-defined operator overloading is not part of the language.

## Binding Expressions

`:=` binds names inside an expression:

```text
if (trimmed := input.trim()) != "":
    println(trimmed)
```

1. r[expr.bind.value] `:=` introduces one or more inferred, non-reassignable names and evaluates to the initializer's value.
2. r[expr.bind.precedence] `:=` has the lowest precedence.
3. r[expr.bind.parens] Parentheses are required when a binding appears as an operand of another expression, as above.
4. r[expr.bind.tuple] For tuple binding, the right side must be a tuple of the same arity; any other value is an error. Error: `type-mismatch`.
5. r[expr.bind.tuple.value] The value of the whole binding expression is the original tuple value.

```text
fn run() -> i32:
    first, second := (1, 2, 3)  # error: type-mismatch
    first
```

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
2. r[expr.comp.no-suspension] Comprehensions cannot contain suspension calls.
3. r[expr.comp.no-jumps] `return`, `break`, and `continue` are not valid inside a comprehension.
4. r[expr.comp.no-let] There is no comprehension `let` clause.

```text
fn ready!() -> i32: 1

fn main!() -> List[i32]:
    [for value in [1] => ready!()]  # error
```

In place of a `let` clause, use a parenthesized `:=` binding in a guard or
result expression:

```text
labels := [for user in users
           if (label := user.name.trim().lower()) != ""
           => label]
```

## Closures And Control Expressions

1. r[expr.closure] Closures are expressions described in [Functions](07-functions.md).
2. r[expr.control] `if`, `match`, and loops are value-capable expressions described in [Control Flow](06-control-flow.md).

## Unsupported Expression Extensions

1. r[expr.unsupported.overloading] hd-lang has no user-defined arithmetic or bitwise operator overloading.
2. r[expr.unsupported.chaining] hd-lang has no comparison chaining.
3. r[expr.unsupported.any-fallback] hd-lang has no fallback conversion of heterogeneous literals to `Any`.
4. r[expr.unsupported.try-mapping] Postfix `?` has no mapping clause.
5. r[expr.unsupported.try-mapping.explicit] A site that needs a different error conversion maps the `Result` explicitly before `?`, for example with a function that takes a single-payload variant constructor as its mapper.

See also: [Enum Declarations](08-data-and-enums.md#enum-declarations).
