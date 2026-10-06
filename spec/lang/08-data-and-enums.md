# Data Types and Enums

Status: language specification draft.

This chapter defines data types and enums.

1. r[data.kind.data] A **data type** is a nominal product type with reference semantics.
2. r[data.kind.enum] An **enum** is a nominal sum type.
3. r[data.kind.no-class] Neither is a class, and neither creates an inheritance hierarchy.

```hd
data Point:
    x: i32
    y: i32

enum Shape:
    Dot
    Circle(radius: f64)
```

> **Why.** The keyword `data` names the declaration form without suggesting
> value-type copying.

## Data Declarations

A `data` declaration defines a data type with named, typed fields.

```text
data User:
    id: string
    email: string
    display_name: string? = .None
```

1. r[data.decl.nominal] Data identity is nominal: two declarations with the same fields introduce different types.

### Fields

1. r[data.field.unique] Field names must be unique within the data type. Error: `duplicate-field`.
2. r[data.field.typed] Every field has an explicit type.
3. r[data.field.no-mut-modifier] Fields have no standalone `mut` modifier: `mut friend: User` is an error. Error: `mutable-field-modifier`.
4. r[data.field.mut-type] `friend: mut User` declares a field whose type grants mutable access through that reference.
5. r[data.field.embedded-no-mut] An embedded field is written without `mut`: `Base` embeds `Base`, and `mut Base` is an error. Error: `mutable-embedded-field`.

```text
data User:
    mut name: string    # error: mutable-field-modifier
    mut Base            # error: mutable-embedded-field
    friend: mut User    # valid: the type grants mutable access
```

> **Why.** Access to an embedded part already follows its container.

### Field Defaults

1. r[data.default.allowed] An ordinary named field may have a default expression.
2. r[data.default.type] The default must be assignable to the declared field type.
3. r[data.default.requirement-free] The default must obey the same requirement-free rule as a function-parameter default.
4. r[data.default.eval] A default is evaluated separately for each construction, not when the data type is declared.
5. r[data.default.scope] A default sees the declaration's lexical scope.
6. r[data.default.no-field-binding] A default does not implicitly bind other fields of the new value.
7. r[data.default.embedded] Embedded fields have no default syntax.

```hd
data Config:
    retries: i32 = 3
    name: string

fn demo() -> i32:
    Config { name: "shop" }.retries   # 3: the default fills the missing field
```

See also: [Default Values](07-functions.md#default-values).

### Field Visibility

1. r[data.vis.private] Named fields are module-private unless individually marked `pub`.
2. r[data.vis.type-not-fields] A public data type does not make its unmarked fields public.
3. r[data.vis.embedded-public] An embedded field takes no marker and is always public: it is visible wherever its outer type is.
4. r[data.vis.literal] In another module, a data literal may construct the type only when all its fields are public.
5. r[data.vis.private-fields] In another module, private fields cannot be named, initialized, or carried through a copy-update literal.
6. r[data.vis.factory] A public factory function can construct a value with private fields inside the defining module.

Because an embedded field is public, embedding a module-private data type in
a public data type leaks it, as
[`module.vis.signature.coverage`](10-modules.md#r-module.vis.signature.coverage)
states:

```text
data Timestamps:
    created_at: i32

pub data Post:
    Timestamps  # error: private-type-leak
    title: string
```

> **Why.** An embedded field is always public: it is visible wherever its
> outer type is.

See also: [Member Resolution](03-names-and-scopes.md#member-resolution),
[Public Uses And Visibility](10-modules.md#public-uses-and-visibility).

### Fieldless Data Types

A data type with no fields is written with `pass`:

```text
data Unit: pass

unit := Unit {}
```

1. r[data.empty.pass] An empty nominal data type uses `pass` as its body.
2. r[data.empty.canonical] A value of a fieldless data type is canonical: every `Unit {}` is the same value with the same identity.
3. r[data.empty.no-allocation] Constructing a value of a fieldless data type allocates nothing.

See also: [Unary And Binary Operators](05-expressions.md#unary-and-binary-operators).

### Recursive Data Types

1. r[data.recursive.direct] Data declarations may be directly recursive.
2. r[data.recursive.mutual] Module-level data types may also be mutually recursive.
3. r[data.recursive.local] Local data types follow declaration-point visibility and cannot refer to a later local declaration.
4. r[data.recursive.required-fields] Recursion does not imply optionality: a program must still provide a value for every required field during construction.

```hd
data Chain:
    value: i32
    next: Chain?
```

> **Note.** A recursive graph therefore normally includes an optional, enum,
> list, or another finite base case.

> **Why.** Data declarations may be directly recursive because composite
> fields use managed references.

### Reference Semantics

1. r[data.ref.shared] Passing a data value to a function passes a shared reference, not a copy of its fields.
2. r[data.ref.views] The `T` and `mut T` views control mutation through that reference.

```hd
data Log:
    entries: mut List[string]

fn add(log: mut Log, entry: string) -> void:
    log.entries.push(entry)

fn demo() -> usize:
    let mut log = Log { entries: [] }
    add(log, "a")
    log.entries.len()   # 1: add saw the same Log
```

See also: [Composite Values And Access Permission](04-type-system.md#composite-values-and-access-permission).

### Obsolete `struct` Declarations

1. r[data.decl.no-struct] `struct` is not a declaration keyword, and `struct Name:` must be diagnosed. Error: `old-struct-declaration`.

```text
struct Legacy: pass  # error: old-struct-declaration
```

> **Why.** The dedicated diagnostic keeps obsolete source from masquerading
> as a sequence of identifiers.

## Construction And Access

A typed brace literal constructs a data value, and `value.field` accesses a
field.

```text
user := User {
    id: "user_123",
    email: "ada@example.com",
    display_name: "Ada",
}
```

### Data Literals

1. r[data.literal.form] Data values use typed brace literals.
2. r[data.literal.required] Each field without a default must be initialized exactly once. Omitting one is an error. Error: `missing-required-field`.
3. r[data.literal.defaulted] A field with a default may be omitted or supplied explicitly.
4. r[data.literal.unknown] An unknown field is a compile-time error. Error: `unknown-data-field`.
5. r[data.literal.duplicate] A duplicate field is a compile-time error. Error: `duplicate-field`.
6. r[data.literal.any-order] Fields may appear in any order.
7. r[data.literal.eval-explicit] Explicit field expressions are evaluated in source order.
8. r[data.literal.eval-defaults] Then defaults for omitted fields are evaluated in field declaration order.

```text
missing := User { id: "user_123" }                       # error: missing-required-field
unknown := User { id: "u", email: "e", nickname: "Ada" } # error: unknown-data-field
twice := User { id: "u", id: "v", email: "e" }           # error: duplicate-field
```

#### Field Shorthand

1. r[data.literal.shorthand] A field may be written as its bare name when a binding of that name is in scope: `Point { x, y }` means `Point { x: x, y: y }`.
2. r[data.literal.shorthand.mixed] Shorthand fields and `field: value` fields may be mixed in one literal, as in `Point { x, y: 0 }`.
3. r[data.literal.shorthand.rules] A shorthand field follows every rule of the `field: value` it stands for, so it counts toward `data.literal.required` and `data.literal.duplicate`.
4. r[data.literal.shorthand.unknown] A shorthand name that resolves to no binding in scope is an error, as for any unknown value name. Error: `unknown-name`.

```text
data Point:
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
2. r[data.update.same-type] The spread has the same data type.
3. r[data.update.supplies-all] The spread supplies every field.
4. r[data.update.replace] Explicit fields replace copied values.
5. r[data.update.new-value] A copy-update literal creates a data value; it does not mutate the spread source.
6. r[data.update.no-defaults] Because the spread supplies every field, copy-update does not evaluate defaults for unlisted fields.
7. r[data.update.access-view] Each copied field is checked through the spread source's access view.
8. r[data.update.readonly-source] A readonly source can supply its effective `U` value for a direct `mut U` field when the result is also readonly `T`.
9. r[data.update.mutable-result] From such a readonly source, producing `mut T` requires a `mut U` replacement.
10. r[data.update.generic] Generic fields retain their substituted type in both views.
11. r[data.update.shorthand] An explicit replacement may use field shorthand, as in `Point { ...origin, x }`, which means `Point { ...origin, x: x }`.

> **Why.** A copy of a readonly value has mutable access only when nothing
> mutable is read through a readonly view to make it.

> **Note.** Embedded parts are copied, not shared, as
> [`data.part.copy-update`](#r-data.part.copy-update) states.

See also: [Data Embedding](#data-embedding),
[Data Expressions](05-expressions.md#data-expressions).

### Field Access

1. r[data.access.syntax] Field access uses `value.field`.
2. r[data.access.mut-root] A `mut T` root may reassign any of its fields, regardless of the field's declared mutability.
3. r[data.access.readonly-root] A readonly `T` value cannot reassign any field.
4. r[data.access.nested] Nested mutation or a `mut self` call through a field requires the field read to have a `mut` access type.
5. r[data.access.readonly-mut-field] Reading a `field: mut U` through a readonly value yields only `U`.
6. r[data.access.construct-mut-field] A readonly data value may be constructed with `U` in that direct field, while a mutable data value requires `mut U`.
7. r[data.access.generic] A generic field declared `field: P` retains its substituted type: `P = mut U` requires and exposes `mut U` even in a readonly outer value.
8. r[data.access.assignment] Direct assignment to a visible field enforces its declared type, not arbitrary validation or cross-field invariants.
9. r[data.access.graph] The language permits shared mutable children and does not guarantee invariants over the entire reachable object graph.
10. r[data.access.other-module] In another module, field access reaches only public fields; member lookup skips the others.

```hd
data User:
    pub name: string
    email: string

fn rename(user: mut User, name: string) -> void:
    user.name = name   # a mut root may reassign a field
```

> **Note.** Keep fields private and expose controlled methods when writes to
> those fields must preserve such invariants. This does not control other
> mutable aliases to an object stored in a private field.

See also: [Mutable Paths](04-type-system.md#mutable-paths),
[Data Embedding](#data-embedding),
[Member Resolution](03-names-and-scopes.md#member-resolution).

### Data Patterns

1. r[data.pattern.form] Data values may be destructured in `match` patterns using the same `DataName { ... }` form.
2. r[data.pattern.subset] The pattern may mention any subset of visible fields; omitted fields are not tested.
3. r[data.pattern.fields] Within the braces, `field` binds the field value and `field: pattern` applies a nested pattern.

```hd
data Point:
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

A bare type-name member is an **embedded field**, which embeds another data
type.

```text
data Post:
    Timestamps
    id: string
    title: string
```

A data literal fills the embedded field with a copy, written `...`:

```text
post := Post {
    Timestamps: ...Timestamps {
        created_at: 1700000000,
        updated_at: 1700000000,
    },
    id: "post_123",
    title: "Hello",
}
```

### Embedded Field Names

1. r[data.embed.member] A bare type-name member embeds another data type, possibly with type arguments. The embedded field's name, and its key in a data literal, is the type's final name without type arguments: `Box[T]` is the field `Box`.
2. r[data.embed.data-only] An embedded field must name a data type, a generic data type such as `Box[T]`, or a transparent alias that resolves to one. Embedding any other type is an error. That covers an enum, a newtype, a trait value type, `Any`, a builtin or collection type, a function type, and a type parameter. Error: `embedded-non-data`.
3. r[data.embed.unique] The name must be unique among the outer data type's fields. A duplicate name that involves an embedded field is an error. Error: `duplicate-embedded-field`.

```text
data Storage:
    Box[i32]
    Box[string]  # error: duplicate-embedded-field
```

```text
data Holder[T]:
    JobStatus  # error: embedded-non-data
    List[i32]  # error: embedded-non-data
    T          # error: embedded-non-data
```

### Embedding Limits

Embedding is limited in width and depth, and a data type never embeds
itself.

| Rule | Limit | Error | Reported on |
| --- | --- | --- | --- |
| r[data.embed.width] Width | A data type may declare at most three embedded fields. | `too-many-embedded-fields` | the fourth embedded field |
| r[data.embed.depth] Depth | Embedding chains may be at most three levels deep: `C` may embed `P1`, which embeds `P2`, which embeds `P3`. | `embedding-too-deep` | the embedded field that begins the first chain that reaches depth 4 |
| r[data.embed.depth.self] Cycle | A data type that embeds itself, directly or through other types, is an error, in place of `embedding-too-deep`. | `embedding-cycle` | the embedded field that begins the cycle, in the cycle's first declared type |

```text
data Post:
    Created
    Updated
    Owned
    Tagged  # error: too-many-embedded-fields
    title: string
```

Here `Site` reaches `Stamp` at depth 4, one level too deep:

```text
data Audit:
    Stamp
    by: string

data Record:
    Audit
    id: string

data Page:
    Record
    title: string

data Site:
    Page  # error: embedding-too-deep
    host: string
```

```text
data Node:
    Edge  # error: embedding-cycle
    id: string

data Edge:
    Node
    weight: i32
```

> **Note.** For the error revamp: `embedding-too-deep` is reported at the
> declaration of every data type that reaches a part at depth 4. Its
> message shows the chain, as in `C > P1 > P2 > P3 > P4`. The
> `embedding-cycle` message shows the cycle path, as in `Node > Edge > Node`.

> **Note.** Together the two limits bound a data type's embedding tree to at
> most 3 + 9 + 27 = 39 parts.[^miku]

[^miku]: 39 reads as "mi-ku" in Japanese (san-kyuu, 3-9), a nod to Hatsune Miku. The bound was not chosen for this reason, but it is a happy coincidence.

### Parts And Copies

An embedded field holds a **part** of the outer value: a value of the
embedded type that the outer value receives as its own copy.

1. r[data.part.construct] A part copy is a copy-update. `E: ...e` in a data literal, and `x.E ...= e` on a `mut` value, store the copy-update `T { ...e }`. Here `E` is the embedded field's name and `T` its type. The `...` applies to the whole field expression.
2. r[data.part.copy-update] A copy-update literal, as in `Post { ...post, title: "t" }`, copies each embedded part `p` of its spread source that it does not replace, as `T { ...p }`. This is the one exception to a shallow copy-update: a copy never shares a part with its original.

```text
fn build(ts: Timestamps) -> Post:
    let mut post = Post { Timestamps: ...ts, id: "p", title: "t" }  # stores Timestamps { ...ts }
    post.Timestamps ...= ts                                          # stores Timestamps { ...ts }
    post
```

> **Note.** A part copy is therefore a new value: a later change to `post`'s
> part is not seen through `ts`. Composite values that the part's ordinary
> fields reference are still shared, as in any copy-update.

See also: [Copy-Update Literals](#copy-update-literals).

#### Copy Markers

1. r[data.part.marker-required] The copy marker is required exactly where a part is filled or stored, even from a fresh literal, as in `Timestamps: ...Timestamps { created_at: 1, updated_at: 1 }`. An embedded field filled as `Post { Timestamps: ts }` or assigned as `post.Timestamps = ts` is an error. Error: `embedded-copy-required`. A `...` after any other field's label, or `...=` on any other place, is an error. Error: `copy-into-ordinary-field`.

```text
fn invalid(post: mut Post, ts: Timestamps, account: mut Account, user: User) -> void:
    fresh := Post { Timestamps: ts, id: "p" }  # error: embedded-copy-required
    post.Timestamps = ts                       # error: embedded-copy-required
    other := Account { owner: ...user }        # error: copy-into-ordinary-field
    account.owner ...= user                    # error: copy-into-ordinary-field
```

> **Note.** For the error revamp: the `embedded-copy-required` message
> suggests the `...` form.

A prefix `...` always copies, and a suffix `...` always spreads, as
[`grammar.primary.prefix-copies`](02-grammar.md#r-grammar.primary.prefix-copies)
states:

| Form | Example | Meaning |
| --- | --- | --- |
| Leading spread | `User { ...user }` | copies the source's fields into the new value |
| Embedded field label | `Label: ...value` | copies `value`'s fields into the part |
| Suffix `...` | `f(xs...)`, `[0, xs...]`, `$.with(ctx...)` | spreads elements or entries |

See also: [Primary Expressions](02-grammar.md#primary-expressions).

#### When Copies Are Made

1. r[data.part.copy-time] A part is copied as soon as the value that fills it is evaluated. In a literal, that is at its field expression, before any later field expression runs. For parts that a spread supplies, it is when the spread is evaluated, before every explicit field expression.

```hd
data Base:
    id: i32

data Outer:
    Base
    tag: string

fn demo(b: Base) -> Outer:
    Outer { Base: ...b, tag: "t" }   # b is copied into the part now
```

> **Note.** Only a `...` copies: passing, returning, binding, or matching
> the outer value, or reading its part, never copies.

### Access Through Parts

1. r[data.part.access] A read of an embedded field, including a derivation's member value for it, yields the part itself, not a copy. It has its container's access: if `post` has type `mut Post`, `post.Timestamps` has type `mut Timestamps`; if `post` has type `Post`, it has type `Timestamps`. This is the embedded-field step of [Mutable Paths](04-type-system.md#mutable-paths).

> **Note.** A promoted member is its explicit path, as
> [`names.promoted.path`](03-names-and-scopes.md#r-names.promoted.path)
> states, so it has the access the receiver grants. A binding of the part
> is an alias whose view follows the binding rules. `let mut stamps =
> post.Timestamps` on a `mut Post` binds a `mut Timestamps`, and a mutation
> through either name is observed through the other.

The following example assumes the `Post` and `Timestamps` declarations
above:

```text
impl Timestamps:
    pub fn touch(mut self, at: i64) -> void:
        self.updated_at = at

fn edit(post: mut Post, stamps: Timestamps) -> void:
    post.touch(1700000100)            # promoted mut self method
    post.updated_at = 1700000200      # promoted field through a mut root
    let mut alias = post.Timestamps   # mut Timestamps, the same part
    alias.created_at = 1700000000     # observed as post.created_at
    let mut copy = Post { Timestamps: ...stamps, id: "p", title: "t" }
    copy.touch(1700000300)            # changes copy's part, never stamps
    post.Timestamps ...= stamps       # copy assignment
```

```text
fn invalid(post: Post) -> void:
    post.touch(5)  # error: mutable-receiver-required
```

#### Part Ownership

1. r[data.part.aliases-untracked] A part is owned by its outer value only in one sense: a part is copied whenever it is filled. So no two outer values receive the same part. The language does not track or prevent later aliases. A read of the part, or a `mut self` method of the embedded type that stores `self` elsewhere, keeps a reference. Changes through it are observed in the outer value.
2. r[data.part.unobservable] An implementation may lay a part out inline or as a separate object referenced only by its outer value. It may also omit the copy of a value that nothing else can reference, such as a fresh literal. Neither choice is observable.

```hd
data Inner:
    value: i32

data Outer:
    Inner

fn read(o: Outer) -> i32:
    o.Inner.value   # the part itself
```

### Mutable Edges

A part copy from a readonly value is readonly when a mutable edge lies
anywhere beneath it, because it is a copy-update.

```text
data Stamp:
    at: i32
    owner: mut Owner  # a mutable edge

data Post:
    Stamp
    id: string

fn invalid(stamp: Stamp) -> void:
    let mut post = Post { Stamp: ...stamp, id: "p" }  # error: mutable-upgrade
    kept := Post { Stamp: ...stamp, id: "q" }         # valid: the binding is readonly
```

Here `Stamp { ...stamp }` is readonly by
[`data.update.mutable-result`](#r-data.update.mutable-result), so the
literal is readonly by
[`types.fresh.mut-literal`](04-type-system.md#r-types.fresh.mut-literal),
and `let mut` rejects it by
[`types.fresh.expected-mut`](04-type-system.md#r-types.fresh.expected-mut).

> **Note.** A data type has **mutable edges** when it, or a type it embeds,
> declares a direct `field: mut U`: a mutable edge counts at any depth. A
> part copy of a readonly value of such a type is readonly. Adding a direct
> `mut U` field to a data type is therefore a breaking change for every type
> that embeds it, at any depth. A literal elsewhere that fills the part from
> a readonly value and is used as `mut` becomes a `mutable-upgrade` error.

See also: [Bindings And Fresh Values](04-type-system.md#bindings-and-fresh-values).

### Embedded Field Variance

For variance, an embedded field's type counts as beneath `mut`, so it is an
invariant position, as [`types.polarity.mut`](04-type-system.md#r-types.polarity.mut)
states.

```text
data Box[+T]:
    value: T

data Holder[+T]:
    Box[T]  # error: invalid-variance
```

> **Why.** Access through an embedded field follows the container.

See also: [Variance](04-type-system.md#variance).

### Member Promotion

1. r[data.promote.members] Embedding promotes the `pub` fields and `pub` inherent methods of an embedded type for convenient access, as [Member Resolution](03-names-and-scopes.md#member-resolution) defines.

Each fact about promotion is stated once, in chapter 03 or chapter 09:

| Fact | Rule |
| --- | --- |
| Only the `pub` fields and `pub` inherent methods of a part, at any depth, are promoted; trait methods never are. | [`names.promote.member`](03-names-and-scopes.md#r-names.promote.member) |
| For each name, the shallowest member hides deeper ones, so a `pub` own member hides a promoted one. | [`names.hide.depth`](03-names-and-scopes.md#r-names.hide.depth) |
| A private own member with a promoted member's name is `ambiguous-promoted-member`. | [`names.conflict.private-shadow`](03-names-and-scopes.md#r-names.conflict.private-shadow) |
| Two members with one name at the smallest depth are `ambiguous-promoted-member` at the declaration. | [`names.conflict.error`](03-names-and-scopes.md#r-names.conflict.error) |
| A promoted member means its explicit path, with the part as receiver: there is no overriding. | [`names.promoted.path`](03-names-and-scopes.md#r-names.promoted.path) |
| A trait method of the receiver beside a promoted method is `ambiguous-method`. | [`names.method-lookup.ambiguous`](03-names-and-scopes.md#r-names.method-lookup.ambiguous) |
| Embedding grants no trait conformance. | [`trait.embed.no-conformance`](09-traits.md#r-trait.embed.no-conformance) |
| A promoted method never fills a trait method. | [`trait.impl.fill.never`](09-traits.md#r-trait.impl.fill.never) |

```text
data Left:
    pub id: string

data Right:
    pub id: string

data LeftBox:
    Left

data RightBox:
    Right

data Record:
    LeftBox
    RightBox  # error: ambiguous-promoted-member

data Base:
    pub id: string

data Entry:
    Base
    id: string  # error: ambiguous-promoted-member
```

See also: [Member Resolution](03-names-and-scopes.md#member-resolution),
which defines which field `x.name` or method `x.name(args)` selects.

### Composition, Not Subtyping

Embedding is composition, not subtyping. The outer data type is a different
nominal type, so it is not assignable to the embedded type. A promoted
method runs as the embedded type's own method, and embedding grants no
trait conformance.

> **Note.** Conformance through a part is written explicitly, with
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
4. r[data.enum.payload.large] Large payloads should use a separate data type rather than a nested field block.
5. r[data.enum.payload.no-field-blocks] Variant field blocks are not part of the language.
6. r[data.enum.immutable] An enum value never changes once built: its variant and its payload values are fixed at construction.
7. r[data.enum.immutable.shallow] The rule is shallow. A payload declared `mut U` still refers to a mutable object, which may change through that reference.

### Variant Construction

A variant is constructed with call syntax:

```text
error := ToolError.NotFound(resource="user_123")
```

1. r[data.enum.construct.call] Variant construction follows function-call conventions: positional arguments come before named arguments.
2. r[data.enum.construct.unknown] A named argument that names no payload field of the variant is an error. Error: `unknown-data-field`.
3. r[data.enum.construct.call-codes] Other argument errors use the call codes in [Calls](05-expressions.md#calls), such as `duplicate-argument`.

```text
enum Pair:
    Value(first: i32, second: i32)

fn make() -> Pair: Pair.Value(missing=1, first=2)  # error: unknown-data-field
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
8. r[data.enum.fn-value.shorthand] The contextual shorthand `.Variant` still needs an expected enum type, so it is never a function value.
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

See also: [Function Types And Values](07-functions.md#function-types-and-values),
[Generalized Algebraic Data Types](13-gadts.md).

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
3. r[data.shared.constructor] Each variant with shared enum data must provide its enum constructor expression after `->`.
4. r[data.shared.arguments] The constructor call follows ordinary positional/named argument ordering and must initialize each shared parameter without a default.
5. r[data.shared.per-variant] Shared constructor data belongs to the variant, not to each value: every value of one variant has the same shared data.
6. r[data.shared.compile-time] Each variant's constructor expression is evaluated once, at compile time, as a [fact expression](14-annotations.md#r-annot.fact.eval) is, and it must be requirement-free.
7. r[data.shared.no-payload] The variant's payload parameters are not in scope in its constructor expression.
8. r[data.shared.not-stored] Shared data is stored once per variant and never in an enum value, so it adds nothing to a value's size or identity.

> **Why.** Shared data describes a variant, like a Java or Kotlin enum
> constant's constructor arguments. Data that differs from value to value
> belongs in each variant's payload or in a wrapper data type.

### Shared Parameter Defaults

1. r[data.shared.default] A shared parameter may declare a default expression.
2. r[data.shared.default.order] After the first defaulted parameter, every following shared parameter must also have a default, as with function parameters. A later parameter without one is an error. Error: `default-order`.
3. r[data.shared.default.requirement-free] The default must satisfy the same requirement-free rule as a function-parameter or data-field default.
4. r[data.shared.default.eval-once] When omitted, the default is evaluated once for the variant, with the variant's constructor expression.
5. r[data.shared.default.eval-order] Explicit argument expressions are evaluated first, then omitted defaults in parameter declaration order.
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
4. r[data.shared.permissions] Shared fields follow ordinary composite access permissions.
5. r[data.shared.readonly] Reading through a readonly enum yields a readonly viewpoint.
6. r[data.shared.read-only] A shared field is read-only: assigning one is an error, even through a mutable enum root. Error: `invalid-assignment-target`.
7. r[data.shared.payload-fields] Variant payload fields remain available through pattern matching rather than direct field access, because they do not exist on every variant.
8. r[data.shared.payload-names] Within one variant, a named payload parameter must not duplicate a named shared constructor parameter.
9. r[data.shared.payload-defaults] Variant payload parameters do not have defaults.

```text
enum HttpStatus(code: i32, phrase: string):
    Ok -> HttpStatus(200, phrase="OK")
    Moved(target: string) -> HttpStatus(301, phrase=target)  # error: unknown-name

fn rename(status: mut HttpStatus) -> void:
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
    Branch(left: Tree[T], right: Tree[T])
```

1. r[data.enum.generic] Enums may be generic and recursive.
2. r[data.enum.generic.result] Without an explicit GADT result clause, every variant constructs the enclosing enum instantiated with the declaration's type arguments.

See also: [Generalized Algebraic Data Types](13-gadts.md), which defines
explicit refined results and variant-local generic parameters.

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

1. r[data.repr.backend] Data and enum representation is chosen by the Wasm GC backend subject to observable language semantics.
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
[Open Issues](../../future-work/OPEN_ISSUES.md).

## Generalized Algebraic Data Types

1. r[data.gadt.declare] Variants may declare explicit refined result types and variant-local generic parameters.
2. r[data.gadt.refine] Matching such a variant refines the subject type within that arm.

```hd
enum Expr[T]:
    Int(value: i64) -> Expr[i64]

fn eval(e: Expr[i64]) -> i64:
    match e:
        Expr.Int(value) => value
```

See also: [Generalized Algebraic Data Types](13-gadts.md).

## Typed Derivation Of Data And Enums

Data types and enums derive library traits with `@derive` or a derivation
block, as [Typed Derivation](14-annotations.md#typed-derivation) defines.

1. r[data.derive.members] For typed derivation, a data type's members are its fields in declaration order, embedded fields included. An enum's members are each variant's payload parameters.
2. r[data.derive.payload-names] An unnamed payload parameter is the member `_0`, `_1`, and so on, by position.
3. r[data.derive.shared] Shared constructor data is not a member. A derivation reads it from the variant's information, and `build` never reads it.
4. r[data.derive.gadt] A GADT enum cannot be derived through a template. Error: `gadt-derivation`.
5. r[data.derive.newtype] A newtype derives through its base type, as [Derived Newtypes](09-traits.md#derived-newtypes) defines.

```hd
@derive(Eq, Debug)
data Point:
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
> private enum in a data type with a private field. It then exposes accessor
> methods.

```hd
enum Kind:
    Timeout
    Refused

data ApiError:
    kind: Kind

impl ApiError:
    fn is_timeout(self) -> bool:
        self.kind is Kind.Timeout
```

See also: [Variant Payloads](#variant-payloads), where
`data.enum.payload.no-field-blocks` rules out variant field blocks.
