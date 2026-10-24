# Data Types and Enums

Status: language specification draft.

This chapter defines structs and enums.

1. r[data.kind.struct] A **struct** is a nominal product type with value semantics.
2. r[data.kind.enum] An **enum** is a nominal sum type.
3. r[data.kind.no-class] Neither is a class, and neither creates an inheritance hierarchy.

```hd
struct Point:
    x: i32
    y: i32

enum Shape:
    Dot
    Circle(radius: f64)
```

## Struct Declarations

A `struct` declaration defines a struct with named, typed fields. A struct
value is copied, never shared.

```hd
struct Vec2:
    x: f64
    y: f64 = 0.0

fn demo() -> f64:
    let mut a = Vec2 { x: 1.0 }
    b := a      # b is a copy of a
    a.x = 5.0   # changes a only
    b.x         # 1.0
```

1. r[data.struct.decl] `struct Name:` declares a struct. Its body holds named fields, defaults, visibility, decorators, and generic parameters.
2. r[data.decl.nominal] Struct identity is nominal: two declarations with the same fields introduce different types.
3. r[data.struct.value] A struct is a value type, as [Value Types](04-type-system.md#value-types) defines.
4. r[data.struct.receiver-forms] A method of a struct takes `self` or `mut self`, its own read-only or changeable copy of the receiver, or `&self` or `&mut self`, a [reference](07-functions.md#reference-receivers) to the caller's value.
5. r[data.struct.mut-self] A struct method's `mut self` changes its copy only. The caller's value is unchanged unless the method returns the copy and the caller keeps it.
6. r[data.struct.embed] A struct may embed a struct, by the rules of [Data Embedding](#data-embedding) for names, limits, and member promotion. An embedded field is a value field, held inline.
7. r[data.struct.embed.plain] A struct literal fills an embedded field as it fills a named field, as in `Post { Timestamps: ts }`, and `post.Timestamps = ts` assigns it. Both copy the value, as every struct copy does.
8. r[data.struct.no-recursion] A struct that contains itself by value is an error. Error: `recursive-value-type`.
9. r[data.struct.contains] A value contains the values of its struct fields, enum payloads, tuple elements, optional payload, and newtype base, and what those contain.
10. r[data.struct.contains.ends] A collection, a trait value, or a function value holds a reference, so containment ends there.
11. r[data.struct.contains.reference] A [reference](13-values-and-references.md#reference-types), `&T` or `&mut T`, also ends containment.
12. r[data.struct.no-recursion.fix] The `recursive-value-type` error of a struct has a fix-it that writes `&` before the type of each field that contains the struct.

```text
struct Vec2:
    x: f64
    y: f64

enum Unit:
    Metre
    Foot

struct Pair:
    Vec2          # valid: an embedded struct
    Unit          # error: embedded-non-data
    weight: f64

struct Chain:
    next: Chain?  # error: recursive-value-type
```

```hd
impl Vec2:
    fn moved(mut self, dx: f64) -> Vec2:
        self.x += dx
        self
```

> **Why.** A struct holds its fields inline, so one that contained itself
> would have no finite size. Recursion goes through a reference or a
> collection, as `next: &Chain?` and `next: List[Chain]` do.

See also: [Value Types](04-type-system.md#value-types),
[Writable Places](04-type-system.md#writable-places).

### Fields

A struct declares named, typed fields:

```text
struct User:
    id: string
    email: string
    display_name: string? = .None
```

1. r[data.field.unique] Field names must be unique within the struct. Error: `duplicate-field`. The named parameters of an enum header, and the named payload parameters of one variant, are unique in the same way.
2. r[data.field.typed] Every field has an explicit type.
3. r[data.field.no-mut-modifier] Fields have no standalone `mut` modifier: `mut friend: User` is an error. Error: `mutable-field-modifier`.
4. r[data.field.mut-type] `friend: &mut User` declares a field whose type grants mutable access through that reference.
5. r[data.field.embedded-no-mut] An embedded field is written without `mut`: `Base` embeds `Base`, and `mut Base` is an error. Error: `mutable-field-modifier`.

```text
struct User:
    mut name: string    # error: mutable-field-modifier
    mut Base            # error: mutable-field-modifier
    friend: &mut User   # valid: the type grants mutable access
```

> **Why.** Access to an embedded field already follows its container.

### Field Defaults

1. r[data.default.allowed] An ordinary named field may have a default expression.
2. r[data.default.type] The default must be assignable to the declared field type.
3. r[data.default.requirement-free] The default must obey the same requirement-free rule as a function-parameter default.
4. r[data.default.eval] A default is evaluated separately for each construction, not when the struct is declared.
5. r[data.default.scope] A default sees the declaration's lexical scope.
6. r[data.default.no-field-binding] A default does not implicitly bind other fields of the new value.
7. r[data.default.embedded] Embedded fields have no default syntax.

```hd
struct Config:
    retries: i32 = 3
    name: string

fn demo() -> i32:
    Config { name: "shop" }.retries   # 3: the default fills the missing field
```

See also: [Default Values](07-functions.md#default-values).

### Field Visibility

1. r[data.vis.private] Named fields are module-private unless individually marked `pub`.
2. r[data.vis.type-not-fields] A public struct does not make its unmarked fields public.
3. r[data.vis.embedded-public] An embedded field takes no marker and is always public: it is visible wherever its outer type is.
4. r[data.vis.literal] In another module, a struct literal may construct the type only when all its fields are public. Any other such literal is an error. Error: `private-member`.
5. r[data.vis.private-fields] In another module, private fields cannot be named, initialized, or carried through a copy-update literal. Doing so is an error. Error: `private-member`.
6. r[data.vis.factory] A public factory function can construct a value with private fields inside the defining module.

Because an embedded field is public, embedding a module-private struct in a
public struct leaks it, as
[`module.vis.signature.coverage`](10-modules.md#r-module.vis.signature.coverage)
states:

```text
struct Timestamps:
    created_at: i32

pub struct Post:
    Timestamps  # error: private-type-leak
    title: string
```

> **Why.** An embedded field is always public: it is visible wherever its
> outer type is.

See also: [Member Resolution](03-names-and-scopes.md#member-resolution),
[Public Uses And Visibility](10-modules.md#public-uses-and-visibility).

### Fieldless Data Types

A struct with no fields is written with `pass`:

```text
struct Unit: pass

unit := Unit {}
```

1. r[data.empty.pass] An empty struct uses `pass` as its body.
2. r[data.empty.no-allocation] Constructing a value of a fieldless struct allocates nothing.

### Recursive Data Types

1. r[data.recursive.through-reference] A struct may refer to itself through a reference, an optional reference, or a collection, as `next: &Chain?` and `replies: List[Comment]` do.
2. r[data.recursive.mutual] Module-level structs may also be mutually recursive through such links.
3. r[data.recursive.local] Local structs follow declaration-point visibility and cannot refer to a later local declaration.
4. r[data.recursive.required-fields] Recursion does not imply optionality: a program must still provide a value for every required field during construction.

```hd
struct Chain:
    value: i32
    next: &Chain?
```

> **Note.** A recursive graph therefore normally includes an optional, enum,
> list, or another finite base case.

See also: [`data.struct.no-recursion`](#r-data.struct.no-recursion),
[Generic And Recursive Enums](#generic-and-recursive-enums).

## Construction And Access

A typed brace literal constructs a struct value, and `value.field` accesses
a field.

```text
user := User {
    id: "user_123",
    email: "ada@example.com",
    display_name: "Ada",
}
```

### Data Literals

1. r[data.literal.form] Struct values use typed brace literals.
2. r[data.literal.required] Each field without a default must be initialized exactly once. Omitting one is an error. Error: `missing-required-field`.
3. r[data.literal.defaulted] A field with a default may be omitted or supplied explicitly.
4. r[data.literal.unknown] An unknown field is a compile-time error. Error: `unknown-field`.
5. r[data.literal.duplicate] A duplicate field is a compile-time error. Error: `duplicate-field`.
6. r[data.literal.any-order] Fields may appear in any order.
7. r[data.literal.eval-explicit] Explicit field expressions are evaluated in source order.
8. r[data.literal.eval-defaults] Then defaults for omitted fields are evaluated in field declaration order.

```text
missing := User { id: "user_123" }                       # error: missing-required-field
unknown := User { id: "u", email: "e", nickname: "Ada" } # error: unknown-field
twice := User { id: "u", id: "v", email: "e" }           # error: duplicate-field
```

#### Field Shorthand

1. r[data.literal.shorthand] A field may be written as its bare name when a binding of that name is in scope: `Point { x, y }` means `Point { x: x, y: y }`.
2. r[data.literal.shorthand.mixed] Shorthand fields and `field: value` fields may be mixed in one literal, as in `Point { x, y: 0 }`.
3. r[data.literal.shorthand.rules] A shorthand field follows every rule of the `field: value` it stands for, so it counts toward `data.literal.required` and `data.literal.duplicate`.
4. r[data.literal.shorthand.unknown] A shorthand name that resolves to no binding in scope is an error, as for any unknown value name. Error: `unknown-name`.

```text
struct Point:
    x: i32
    y: i32

fn place(x: i32, y: i32) -> (Point, Point):
    (Point { x, y }, Point { x, y: 0 })

fn broken(x: i32) -> Point:
    Point { x, y }  # error: unknown-name
```

> **Why.** A data pattern already binds `Point { x, y }` field by field, so
> the literal builds with the same spelling it destructures with.

See also: [Data Expressions](05-expressions.md#data-expressions).

### Copy-Update Literals

A **copy-update literal** builds a new value from an existing one:

```text
renamed := User {
    ...user,
    display_name: "Ada Lovelace",
}
```

1. r[data.update.spread] Copy-update construction uses one leading spread.
2. r[data.update.same-type] The spread has the same struct type.
3. r[data.update.supplies-all] The spread supplies every field.
4. r[data.update.replace] Explicit fields replace copied values.
5. r[data.update.new-value] A copy-update literal creates a new struct value; it does not mutate the spread source.
6. r[data.update.no-defaults] Because the spread supplies every field, copy-update does not evaluate defaults for unlisted fields.
7. r[data.update.shorthand] An explicit replacement may use field shorthand, as in `Point { ...origin, x }`, which means `Point { ...origin, x: x }`.

See also: [Data Embedding](#data-embedding),
[Data Expressions](05-expressions.md#data-expressions).

### Field Access

1. r[data.access.syntax] Field access uses `value.field`.
2. r[data.access.writable-place] Assigning a field, or changing its value in place, requires a writable place, as [Writable Places](04-type-system.md#writable-places) states.
3. r[data.access.assignment] Direct assignment to a visible field enforces its declared type, not arbitrary validation or cross-field invariants.
4. r[data.access.graph] The language permits shared mutable children through `&mut` references, and does not guarantee invariants over the entire reachable object graph.
5. r[data.access.other-module] In another module, field access reaches only public fields; member lookup skips the others.

```hd
struct User:
    pub name: string
    email: string

fn rename(user: &mut User, name: string) -> void:
    user.name = name   # a write through a &mut reference
```

> **Note.** Keep fields private and expose controlled methods when writes to
> those fields must preserve such invariants. This does not control other
> mutable references to a value that a private field refers to.

See also: [Writable Places](04-type-system.md#writable-places),
[Data Embedding](#data-embedding),
[Member Resolution](03-names-and-scopes.md#member-resolution).

### Data Patterns

1. r[data.pattern.form] Struct values may be destructured in `match` patterns using the same `StructName { ... }` form.
2. r[data.pattern.subset] The pattern may mention any subset of visible fields; omitted fields are not tested.
3. r[data.pattern.fields] Within the braces, `field` binds the field value and `field: pattern` applies a nested pattern.

```hd
struct Point:
    x: i32
    y: i32

fn on_axis(p: Point) -> bool:
    match p:
        Point { x: 0 } => true
        Point { y: 0 } => true
        _ => false
```

See also: [Match Expressions](06-control-flow.md#match-expressions).

## Data Embedding

A bare type-name member is an **embedded field**, which embeds another
struct inline.

```text
struct Post:
    Timestamps
    id: string
    title: string
```

A struct literal fills the embedded field as it fills a named field:

```text
post := Post {
    Timestamps: Timestamps {
        created_at: 1700000000,
        updated_at: 1700000000,
    },
    id: "post_123",
    title: "Hello",
}
```

### Embedded Field Names

1. r[data.embed.member] A bare type-name member embeds another struct, possibly with type arguments. The embedded field's name, and its key in a struct literal, is the type's final name without type arguments: `Box[T]` is the field `Box`.
2. r[data.struct.embed.struct-only] A struct's embedded field must name a struct, a generic struct such as `Box[T]`, or a transparent alias that resolves to one. Embedding any other type is an error. That covers an enum, a newtype, a trait value type, `Any`, a builtin or collection type, a function type, and a type parameter. Error: `embedded-non-data`.
3. r[data.embed.unique] The name must be unique among the outer struct's fields. A duplicate name that involves an embedded field is an error. Error: `duplicate-embedded-field`.

```text
struct Storage:
    Box[i32]
    Box[string]  # error: duplicate-embedded-field
```

```text
struct Holder[T]:
    JobStatus  # error: embedded-non-data
    List[i32]  # error: embedded-non-data
    T          # error: embedded-non-data
```

### Embedding Limits

Embedding is limited in width and depth, and a struct never embeds
itself.

| Rule | Limit | Error | Reported on |
| --- | --- | --- | --- |
| r[data.embed.width] Width | A struct may declare at most three embedded fields. | `too-many-embedded-fields` | the fourth embedded field |
| r[data.embed.depth] Depth | Embedding chains may be at most three levels deep: `C` may embed `P1`, which embeds `P2`, which embeds `P3`. A type that reaches a cycle without being part of it gets no `embedding-cycle`; its depth counts along chains that never repeat a type. | `embedding-too-deep` | the embedded field that begins the first chain that reaches depth 4 |
| r[data.embed.depth.self] Cycle | A struct that embeds itself, directly or through other types, is an error, in place of `embedding-too-deep`. | `embedding-cycle` | the embedded field that begins the cycle, in the cycle's first declared type |

```text
struct Post:
    Created
    Updated
    Owned
    Tagged  # error: too-many-embedded-fields
    title: string
```

Here `Site` reaches `Stamp` at depth 4, one level too deep:

```text
struct Audit:
    Stamp
    by: string

struct Record:
    Audit
    id: string

struct Page:
    Record
    title: string

struct Site:
    Page  # error: embedding-too-deep
    host: string
```

```text
struct Node:
    Edge  # error: embedding-cycle
    id: string

struct Edge:
    Node
    weight: i32
```

> **Note.** For the error revamp: `embedding-too-deep` is reported at the
> declaration of every struct that reaches an embedded struct at depth 4. Its
> message shows the chain, as in `C > P1 > P2 > P3 > P4`. The
> `embedding-cycle` message shows the cycle path, as in `Node > Edge > Node`.

> **Note.** Together the two limits bound a struct's embedding tree to at
> most 3 + 9 + 27 = 39 embedded structs.[^miku]

[^miku]: 39 reads as "mi-ku" in Japanese (san-kyuu, 3-9), a nod to Hatsune Miku. The bound was not chosen for this reason, but it is a happy coincidence.

### Embedded Field Variance

For variance, an embedded field's type is an invariant position, as
[Variance](04-type-system.md#variance) states.

```text
struct Box[+T]:
    value: T

struct Holder[+T]:
    Box[T]  # error: invalid-variance
```

> **Why.** Access through an embedded field follows the container.

### Member Promotion

1. r[data.promote.members] Embedding promotes the `pub` fields and `pub` inherent methods of an embedded type for convenient access, as [Member Resolution](03-names-and-scopes.md#member-resolution) defines.

Each fact about promotion is stated once, in chapter 03 or chapter 09:

| Fact | Rule |
| --- | --- |
| Only the `pub` fields and `pub` inherent methods of an embedded struct, at any depth, are promoted; trait methods never are. | [`names.promote.member`](03-names-and-scopes.md#r-names.promote.member) |
| An embedded struct's own embedded field is also promoted; it takes no `pub` marker but is always public. | [`names.promote.embedded`](03-names-and-scopes.md#r-names.promote.embedded) |
| For each name, the shallowest member hides deeper ones, so a `pub` own member hides a promoted one. | [`names.hide.depth`](03-names-and-scopes.md#r-names.hide.depth) |
| A private own member with a promoted member's name is `ambiguous-promoted-member`. | [`names.conflict.private-shadow`](03-names-and-scopes.md#r-names.conflict.private-shadow) |
| Two members with one name at the smallest depth are `ambiguous-promoted-member` at the declaration. | [`names.conflict.error`](03-names-and-scopes.md#r-names.conflict.error) |
| A promoted member means its explicit path, with the embedded field as receiver: there is no overriding. | [`names.promoted.path`](03-names-and-scopes.md#r-names.promoted.path) |
| A trait method of the receiver beside a promoted method is `ambiguous-method`. | [`names.method-lookup.ambiguous`](03-names-and-scopes.md#r-names.method-lookup.ambiguous) |
| Embedding grants no trait conformance. | [`trait.embed.no-conformance`](09-traits.md#r-trait.embed.no-conformance) |
| A promoted method never fills a trait method. | [`trait.impl.fill.never`](09-traits.md#r-trait.impl.fill.never) |

```text
struct Left:
    pub id: string

struct Right:
    pub id: string

struct LeftBox:
    Left

struct RightBox:
    Right

struct Record:
    LeftBox
    RightBox  # error: ambiguous-promoted-member

struct Base:
    pub id: string

struct Entry:
    Base
    id: string  # error: ambiguous-promoted-member
```

See also: [Member Resolution](03-names-and-scopes.md#member-resolution),
which defines which field `x.name` or method `x.name(args)` selects.

### Composition, Not Subtyping

Embedding is composition, not subtyping. The outer struct is a different
nominal type, so it is not assignable to the embedded type. A promoted
method runs as the embedded type's own method, and embedding grants no
trait conformance.

> **Note.** Conformance through an embedded field is written explicitly, with
> [`by`](09-traits.md#trait-delegation), as in
> `impl Describe for Service by Logger`.

See also: [Embedding And Trait Satisfaction](09-traits.md#embedding-and-trait-satisfaction),
[Trait Delegation](09-traits.md#trait-delegation).

### Embedded Field Metadata

An embedded field is a field, so it accepts the same prefix metadata
decorators as a named field, as
[`annot.target.kind.field`](14-annotations.md#r-annot.target.kind.field)
states. Its name is the final type name, and the metadata stays on the
field: promoted members do not carry it.

## Enum Declarations

An `enum` declaration lists its variants:

```text
enum JobStatus:
    Queued
    Running
    Succeeded
    Failed
```

1. r[data.enum.closed] An enum defines a closed set of variants.
2. r[data.enum.unique] Variant names must be unique within the enum. Error: `duplicate-variant`.
3. r[data.enum.qualified] The qualified form is `JobStatus.Queued`.
4. r[data.enum.shorthand] `.Queued` is also valid where an expected type fixes `JobStatus`.
5. r[data.enum.shorthand.other-enum] A matching name in another enum does not create ambiguity when that expected type is known.
6. r[data.enum.shorthand.needs-type] A unique name across the program does not make the shorthand valid without an expected enum type.
7. r[data.enum.unknown-variant] A qualified or shorthand variant name that names no variant of the enum is an error. Error: `unknown-variant`.

### Variant Payloads

Variants may carry payloads:

```text
enum ToolError:
    NotFound(resource: string)
    Unauthorized(reason: string)
    RateLimited(retry_after_ms: i32)
```

1. r[data.enum.payload] A variant may carry payload parameters.
2. r[data.enum.payload.order] In a variant constructor call and in a variant pattern, positional arguments and sub-patterns come before `name=` ones. Function calls follow the same rule, [`fn.arg.positional-first`](07-functions.md#r-fn.arg.positional-first).
3. r[data.enum.payload.typed] Each payload type is explicit.
4. r[data.enum.payload.large] Large payloads should use a separate struct rather than a nested field block.
5. r[data.enum.payload.no-field-blocks] Variant field blocks are not part of the language.
6. r[data.enum.immutable] An enum value never changes once built: its variant and its payload values are fixed at construction.
7. r[data.enum.immutable.shallow] The rule is shallow. A payload of type `&mut U` still refers to a place, which may change through that reference.
8. r[data.enum.no-identity] An enum value has no identity, so every enum type implements `AnyVal`, as [`types.sealed.anyval-members`](04-type-system.md#r-types.sealed.anyval-members) states.
9. r[data.enum.no-identity.is] `is` with an enum operand is an error. Error: `identity-requires-references`.

### Variant Construction

A variant is constructed with call syntax:

```text
error := ToolError.NotFound(resource="user_123")
```

1. r[data.enum.construct.call] Variant construction follows function-call conventions: positional arguments come before named arguments.
2. r[data.enum.construct.unknown] A named argument that names no payload field of the variant is an error. Error: `unknown-field`.
3. r[data.enum.construct.call-codes] Other argument errors use the call codes in [Calls](05-expressions.md#calls), such as `duplicate-argument`.

```text
enum Pair:
    Value(first: i32, second: i32)

fn make() -> Pair: Pair.Value(missing=1, first=2)  # error: unknown-field
fn again() -> Pair: Pair.Value(1, first=2)         # error: duplicate-argument
```

### Variant Constructors As Function Values

Some variant constructors are function values:

```text
enum FsError:
    NotFound(path: string)

enum SyncError:
    Fs(error: FsError)
    Retry(attempt: i32, error: FsError)

fn wrap_all(errors: List[FsError], wrap: fn(FsError) -> SyncError) -> List[SyncError]:
    [for error in errors => wrap(error)]

wrapped := wrap_all([FsError.NotFound("a.txt")], SyncError.Fs)
retry := fn(error: FsError) -> SyncError: SyncError.Retry(1, error)
```

1. r[data.enum.fn-value] A variant constructor with exactly one payload field is a function value when it is written without an argument clause.
2. r[data.enum.fn-value.type] `ToolError.NotFound` has type `fn(string) -> ToolError`: one positional parameter of the payload type, the enum as result, no suspension, and the empty requirement row.
3. r[data.enum.fn-value.use] It may be passed wherever that function type is expected, such as to a standard-library `map_err` in `result.map_err(SyncError.Fs)`.
4. r[data.enum.fn-value.positional] The payload's field name and positional or named declaration do not matter, because function values take positional arguments only.
5. r[data.enum.fn-value.generic-rule] For a generic enum, or a variant with its own generic parameters, the value follows the rule for [generic function values](07-functions.md#generic-function-values).
6. r[data.enum.fn-value.generic-argument] Passed as a call argument, it takes its type arguments from the call. So `result.map_err(TaskError.Failed)` on a `Result[T, FsError]` gives `TaskError[FsError]`.
7. r[data.enum.fn-value.generic-unsolved] A generic parameter that remains unsolved is an error. Error: `cannot-infer-type`.
8. r[data.enum.fn-value.shorthand] The contextual shorthand `.Variant` still needs an expected enum type, so it is never a function value. Using it as one is an error. Error: `missing-contextual-enum-type`.
9. r[data.enum.fn-value.call] Calling the value constructs the variant, exactly as calling the constructor does.
10. r[data.enum.fn-value.multiple] A variant constructor with two or more payload fields is not a function value and must be called.
11. r[data.enum.fn-value.unsaturated] Using one without an argument clause is an error. Error: `unsaturated-enum-constructor`.
12. r[data.enum.fn-value.closure] An explicit closure passes its construction as a function value.
13. r[data.enum.fn-value.payload-free] A payload-free variant, including one whose declaration initializes shared enum data, is an enum value and is selected without `()`.

```text
fn apply(make: fn(i32, FsError) -> SyncError) -> SyncError:
    make(1, FsError.NotFound("a.txt"))

fn retry() -> SyncError:
    apply(SyncError.Retry)  # error: unsaturated-enum-constructor
```

A generic variant constructor takes its type arguments from the call:

```text
enum FsError:
    NotFound(path: string)

enum TaskError[E]:
    Failed(error: E)
    Cancelled

fn wrap_error[T, E, F](result: Result[T, E], wrap: fn(E) -> F) -> Result[T, F]:
    match result:
        .Ok(value) => .Ok(value)
        .Err(error) => .Err(wrap(error))

fn read(path: string) -> Result[string, FsError]:
    .Err(FsError.NotFound(path))

wrapped := wrap_error(read("a.txt"), TaskError.Failed)
```

```text
enum Either[L, R]:
    Left(value: L)
    Right(value: R)

fn wrap_all[T, F](values: List[T], wrap: fn(T) -> F) -> List[F]:
    [for value in values => wrap(value)]

fn main() -> void:
    lefts := wrap_all([1, 2], Either.Left)  # error: cannot-infer-type
```

In the second example, nothing determines `R`. An expected type such as
`let lefts: List[Either[i32, string]]` would solve it.

See also: [Function Types And Values](07-functions.md#function-types-and-values).

## Shared Enum Constructor Data

Enums can declare constructor data that every variant shares:

```text
enum StatusCode(i32):
    Ok -> StatusCode(200)
    NotFound -> StatusCode(404)

enum HttpStatus(code: i32, phrase: string, retryable: bool = false):
    Ok -> HttpStatus(200, phrase="OK")
    NotFound -> HttpStatus(404, phrase="Not Found")
    ServiceUnavailable -> HttpStatus(503, phrase="Service Unavailable", retryable=true)
```

1. r[data.shared.declare] An enum may declare data shared by all variants.
2. r[data.shared.parameters] Constructor parameters may be unnamed or named.
3. r[data.shared.constructor] Each variant with shared enum data must provide its enum constructor expression after `->`. A variant without one is an error. Error: `missing-required-field`.
4. r[data.shared.owner] The name after `->` must be the enclosing enum. Any other name is an error. Error: `variant-result-owner`.
5. r[data.shared.arguments] The constructor call follows ordinary positional/named argument ordering and must initialize each shared parameter without a default. Error: `missing-required-field`.
6. r[data.shared.per-variant] Shared constructor data belongs to the variant, not to each value: every value of one variant has the same shared data.
7. r[data.shared.module-init] Each variant's constructor expression is evaluated once at run time during its declaring module's initialization.
8. r[data.shared.module-init.order] These evaluations join that module's top-level initialization under [`module.init.cycle`](10-modules.md#r-module.init.cycle) and [Order Inside A Cycle](10-modules.md#order-inside-a-cycle).
9. r[data.shared.module-init.requirement-free] The expression must satisfy [`module.init.requirement-free`](10-modules.md#r-module.init.requirement-free), and it may call any requirement-free function.
10. r[data.shared.module-init.effects] Its writes to module state occur during initialization.
11. r[data.shared.module-init.not-lazy] The expression is neither lazy nor compile-time evaluation.
12. r[data.shared.no-payload] The variant's payload parameters are not in scope in its constructor expression.
13. r[data.shared.not-stored] Shared data is stored once per variant and never in an enum value, so it adds nothing to a value's size.
14. r[data.shared.generic-copy] A generic enum's shared data has one copy per variant of the declaration, not one per instantiation: `Tagged[i32].A` and `Tagged[string].A` read the same copy.
15. r[data.shared.type-parameter] A shared parameter whose type names a type parameter of the enum is an error, because one copy has no single type. Error: `shared-data-type-parameter`.

> **Why.** Shared data describes a variant, like a Java or Kotlin enum
> constant's constructor arguments. Data that differs from value to value
> belongs in each variant's payload or in a wrapper struct.

### Shared Parameter Defaults

1. r[data.shared.default] A shared parameter may declare a default expression.
2. r[data.shared.default.order] After the first defaulted parameter, every following shared parameter must also have a default, as with function parameters. A later parameter without one is an error. Error: `default-order`.
3. r[data.shared.default.requirement-free] The default must satisfy [`data.shared.module-init.requirement-free`](#r-data.shared.module-init.requirement-free), as a function-parameter or struct-field default does.
4. r[data.shared.default.eval-once] When omitted, the default is evaluated once for the variant during the variant's constructor call at module initialization.
5. r[data.shared.default.eval-order] Within one variant's constructor call, explicit argument expressions are evaluated first, then omitted defaults in parameter declaration order.
6. r[data.shared.default.scope] A default may refer to earlier named shared parameters but not later ones.

```text
enum Status(code: i32 = 0, phrase: string):  # error: default-order
    Unknown -> Status(phrase="unknown")
```

### Shared Fields

Shared data is read as fields of the enum value:

```text
code := StatusCode.NotFound._0
phrase := HttpStatus.NotFound.phrase
```

1. r[data.shared.value-read] Every enum value exposes its variant's shared data as fields.
2. r[data.shared.named-field] A named constructor parameter is available as a field on the enum value.
3. r[data.shared.underscore-field] An unnamed parameter uses zero-based tuple-style access, spelled `_0`, `_1`, and so on, as in `StatusCode.NotFound._0`.
4. r[data.shared.read-only] A shared field is read-only: assigning one is an error, even through a `&mut` reference or a `let mut` binding. Error: `invalid-assignment-target`.
5. r[data.shared.no-place] A shared field is no place, so a change to its value in place, through a method call or an element store, is an error. Error: `not-writable`.
6. r[data.shared.payload-fields] Variant payload fields remain available through pattern matching rather than direct field access, because they do not exist on every variant. A direct access is an error. Error: `unknown-field`.
7. r[data.shared.payload-names] Within one variant, a named payload parameter must not duplicate a named shared constructor parameter, nor another named payload parameter of that variant. Error: `duplicate-field`.
8. r[data.shared.payload-defaults] Variant payload parameters do not have defaults; writing one is an error. Error: `syntax-error`.

```text
enum HttpStatus(code: i32, phrase: string):
    Ok -> HttpStatus(200, phrase="OK")
    Moved(target: string) -> HttpStatus(301, phrase=target)  # error: unknown-name

fn rename(status: &mut HttpStatus) -> void:
    status.phrase = "Fine"  # error: invalid-assignment-target
```

## Generic And Recursive Enums

This section defines generic and recursive enums.

```text
enum Maybe[T]:
    Some(value: T)
    None

enum Tree[T]:
    Leaf(value: T)
    Branch(left: &Tree[T], right: &Tree[T])
```

1. r[data.enum.generic.params] An enum may declare generic parameters.
2. r[data.enum.generic.instance] Every variant constructs the enclosing enum instantiated with the declaration's type arguments.
3. r[data.enum.recursive] An enum may refer to itself through a reference, an optional reference, or a collection, as `Branch(left: &Tree[T], right: &Tree[T])` does.
4. r[data.enum.no-recursion] An enum that contains itself by value, as [`data.struct.contains`](#r-data.struct.contains) defines containment, is an error. Error: `recursive-value-type`.
5. r[data.enum.no-recursion.fix] The error has a fix-it that writes `&` before the type of each payload field or shared field that contains the enum.
6. r[data.enum.no-recursion.group] Structs and enums that contain one another, each through the next, are one `recursive-value-type` error, reported at one of them.
7. r[data.enum.no-boxing] No value is boxed implicitly: each `&` in a recursive enum is written, and `&` on a new value allocates its object.

```text
enum Tree:
    Leaf(value: i64)
    Node(left: Tree, right: Tree)  # error: recursive-value-type

enum Chain:
    Link(next: Chain?)  # error: recursive-value-type

enum Branch:
    Fork(forest: Forest)  # error: recursive-value-type

struct Forest:
    branch: Branch
```

> **Why.** An enum holds its payloads inline, as a struct holds its fields,
> so one that contained itself would have no finite size. Writing the `&`
> keeps every allocation visible, as Go's pointers and Rust's `Box` do.

> **Why.** hd has no generalized algebraic data types. A typed request and
> its response pair through a trait with an associated type, and a typed
> interpreter uses a runtime value enum or a trait.

See also: [Variant result types](02-grammar.md#r-grammar.enum.no-result-type).

## Matching Enums

A `match` inspects an enum value:

```text
match error:
    ToolError.NotFound(resource) => resource
    ToolError.Unauthorized(reason) => reason
    ToolError.RateLimited(retry_after_ms) => retry_after_ms.to_string()
```

```text
fn queued() -> JobStatus: .Queued

fn label(status: JobStatus) -> string:
    match status:
        .Queued => "queued"
        .Running => "running"
        .Succeeded => "succeeded"
        .Failed => "failed"
```

1. r[data.match.exhaustive] Enum values are inspected with exhaustive `match`.
2. r[data.match.variant-patterns] Variant patterns may be qualified, or use `.Variant` when the matched value fixes their enum.
3. r[data.match.shorthand] The shorthand also works for payload construction and nested variant patterns when their expected enum type is fixed.
4. r[data.match.no-inference] The shorthand is not a way to infer an enum from the variant spelling alone: `status := .Queued` has no contextual enum type and is an error. Error: `missing-contextual-enum-type`.

```text
status := .Queued  # error: missing-contextual-enum-type
```

### Payload Patterns

Payload patterns destructure a variant's payload:

```text
Expr.Add(l, r)
Expr.Sub(left=l, right=r)
Expr.Scale(value, factor=2)
```

1. r[data.match.payload.parentheses] Payload patterns use parentheses, not braces.
2. r[data.match.payload.names] Positional patterns may bind names different from declared field names.
3. r[data.match.payload.named] Named patterns use `field=pattern`, and no positional pattern may follow a named one.
4. r[data.match.payload.constrain] Literals and nested variant patterns may constrain payload values.

See also: [Control Flow](06-control-flow.md), which defines pattern scope
and exhaustiveness.

## Option And Result

`Option[T]` and `Result[T, E]` are prelude enums:

| | `Option[T]` | `Result[T, E]` |
| --- | --- | --- |
| Declaration | `enum Option[T]: Some(value: T); None` | `enum Result[T, E]: Ok(value: T); Err(error: E)` |
| Values and patterns | `.Some(value)`, `.None`, `Option.Some(value)`, `Option.None` | `.Ok(value)`, `.Err(error)`, `Result.Ok(value)`, `Result.Err(error)` |
| Language support beyond an ordinary enum | the `T?` spelling; the one-layer implicit wrap of a plain `T` value where `T?` is expected; postfix `?` | postfix `?`; the `Result[void, E]` entry-point result |

1. r[data.prelude.option] Optional `T?` is sugar for the prelude enum `enum Option[T]: Some(value: T); None`.
2. r[data.prelude.result] `Result[T, E]` is likewise the prelude enum `enum Result[T, E]: Ok(value: T); Err(error: E)`.
3. r[data.prelude.values] Values of either type are built with the spellings in the table, and matched with the same spellings as patterns.
4. r[data.prelude.support] The only language support beyond an ordinary enum is what the table lists for each type.
5. r[data.prelude.abi] The representation of either type is an ABI decision, not a source-language difference.
6. r[data.prelude.null] An implementation may, for example, represent `.None` as a null reference.

```hd
fn head(items: List[i32]) -> i32?:
    if items.len() == 0: .None
    else: .Some(items[0])
```

See also: [Optional Types](04-type-system.md#optional-types),
[Result Types](04-type-system.md#result-types).

## Representation And Garbage Collection

1. r[data.repr.implementation] Struct and enum representation is chosen by the implementation subject to observable language semantics.
2. r[data.repr.no-dependence] Programs must not depend on field offsets, variant tags, object addresses, or representation identity unless a future interop facility exposes them explicitly.
3. r[data.repr.gc] Reachable composite values are garbage collected, and unreachable reference cycles are reclaimable.
4. r[data.repr.no-manual] The language does not expose manual deallocation, user-visible finalizers, or weak references.
5. r[data.repr.runtime-only] Weak references and finalizers may exist only inside the standard runtime, and user code cannot observe when one clears or runs.
6. r[data.repr.cleanup] Resource cleanup is separate from memory reclamation.
7. r[data.repr.defer] Block-scoped `defer` provides explicit synchronous cleanup on ordinary control-flow exits and cancellation; it is not an ownership or garbage-collection mechanism.

```hd
fn work() -> void: pass

fn done() -> void: pass

fn demo() -> void:
    defer: done()
    work()
```

Ownership, alias-escape prevention, automatic finalization, and asynchronous
or fallible cleanup policy remain deferred in
[the open-issues umbrella](https://github.com/HerringtonDarkholme/hd-lang/issues/833).

## Typed Derivation Of Data And Enums

Structs and enums derive library traits with `@derive` or a derivation
block, as [Typed Derivation](14-annotations.md#typed-derivation) defines.

1. r[data.derive.members] For typed derivation, a struct's members are its fields in declaration order, embedded fields included. An enum's members are each variant's payload parameters.
2. r[data.derive.payload-names] An unnamed payload parameter is the member `_0`, `_1`, and so on, by position.
3. r[data.derive.shared] Shared constructor data is not a member. A derivation reads it from the variant's information, and `build` never reads it.
4. r[data.derive.newtype] A newtype derives through its base type, as [Derived Newtypes](09-traits.md#derived-newtypes) defines.

```hd
@derive(Eq, Debug)
struct Point:
    x: i32
    y: i32

fn same(a: Point, b: Point) -> bool:
    a == b
```

See also: [Members And Variants](14-annotations.md#members-and-variants).

## Unsupported Aggregate Extensions

1. r[data.unsupported.layout] Stable object layout and component-model representation are ABI concerns and are not observable core-language semantics.
2. r[data.unsupported.non-exhaustive] Enums have no non-exhaustive form, so adding a variant to a public enum is a breaking change for its users.

> **Note.** A library that needs to grow a set of error kinds can wrap a
> private enum in a struct with a private field. It then exposes accessor
> methods.

```hd
@derive(Eq)
enum Kind:
    Timeout
    Refused

struct ApiError:
    kind: Kind

impl ApiError:
    fn is_timeout(self) -> bool:
        self.kind == Kind.Timeout
```

See also: [Variant Payloads](#variant-payloads), where
`data.enum.payload.no-field-blocks` rules out variant field blocks.
