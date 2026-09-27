# Data Types and Enums

Status: language specification draft.

This chapter defines data types and enums.

1. r[data.kind.data] A **data type** is a nominal product type with reference semantics.
2. r[data.kind.enum] An **enum** is a nominal sum type.
3. r[data.kind.no-class] Neither is a class, and neither creates an inheritance hierarchy.

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

See also: [Default Values](07-functions.md#default-values).

### Field Visibility

1. r[data.vis.private] Named fields are module-private unless individually marked `pub`.
2. r[data.vis.type-not-fields] A public data type does not make its unmarked fields public.
3. r[data.vis.embedded-public] An embedded field takes no marker and is always public: it is visible wherever its outer type is.
4. r[data.vis.promotion] Only `pub` fields and `pub` inherent methods of an embedded type are promoted.
5. r[data.vis.private-embed] Embedding a module-private data type in a public data type is an error. Error: `private-type-leak`.
6. r[data.vis.literal] In another module, a data literal may construct the type only when all its fields are public.
7. r[data.vis.private-fields] In another module, private fields cannot be named, initialized, or carried through a copy-update literal.
8. r[data.vis.factory] A public factory function can construct a value with private fields inside the defining module.

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
data Validation: pass

facet := Validation {}
```

1. r[data.empty.pass] An empty nominal data type uses `pass` as its body.
2. r[data.empty.canonical] A value of a fieldless data type is canonical: every `Validation {}` is the same value with the same identity.
3. r[data.empty.no-allocation] Constructing a value of a fieldless data type allocates nothing.

See also: [Unary And Binary Operators](05-expressions.md#unary-and-binary-operators).

### Recursive Data Types

1. r[data.recursive.direct] Data declarations may be directly recursive.
2. r[data.recursive.mutual] Module-level data types may also be mutually recursive.
3. r[data.recursive.local] Local data types follow declaration-point visibility and cannot refer to a later local declaration.
4. r[data.recursive.required-fields] Recursion does not imply optionality: a program must still provide a value for every required field during construction.

> **Note.** A recursive graph therefore normally includes an optional, enum,
> list, or another finite base case.

> **Why.** Data declarations may be directly recursive because composite
> fields use managed references.

### Reference Semantics

1. r[data.ref.shared] Passing a data value to a function passes a shared reference, not a copy of its fields.
2. r[data.ref.views] The `T` and `mut T` views control mutation through that reference.

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
10. r[data.update.embedded] Embedded parts are copied rather than shared.
11. r[data.update.generic] Generic fields retain their substituted type in both views.

See also: [Data Embedding](#data-embedding),
[Data Expressions](05-expressions.md#data-expressions).

### Field Access

1. r[data.access.syntax] Field access uses `value.field`.
2. r[data.access.mut-root] A `mut T` root may reassign any of its fields, regardless of the field's declared mutability.
3. r[data.access.readonly-root] A readonly `T` value cannot reassign any field.
4. r[data.access.nested] Nested mutation or a `mut self` call through a field requires the field read to have a `mut` access type.
5. r[data.access.readonly-mut-field] Reading a `field: mut U` through a readonly value yields only `U`.
6. r[data.access.construct-mut-field] A readonly data value may be constructed with `U` in that direct field, while a mutable data value requires `mut U`.
7. r[data.access.embedded-copy] An embedded field instead receives a copy.
8. r[data.access.generic] A generic field declared `field: P` retains its substituted type: `P = mut U` requires and exposes `mut U` even in a readonly outer value.
9. r[data.access.assignment] Direct assignment to a visible field enforces its declared type, not arbitrary validation or cross-field invariants.
10. r[data.access.graph] The language permits shared mutable children and does not guarantee invariants over the entire reachable object graph.
11. r[data.access.other-module] In another module, field access reaches only public fields; member lookup skips the others.

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

1. r[data.embed.member] A bare type-name member embeds another data type.
2. r[data.embed.name] The embedded field's name is the embedded type name.
3. r[data.embed.key] Construction uses that name as its key, followed by `...`, because the field receives a copy of the value.
4. r[data.embed.named-type] Embedded shorthand accepts a named data type, including one with generic arguments.
5. r[data.embed.data-only] An embedded field must name a data type, a generic data type such as `Box[T]`, or a transparent alias that resolves to one. Embedding any other type is an error. Error: `embedded-non-data`.
6. r[data.embed.non-data] Such other types include enums, newtypes, trait value types, `Any`, builtin and collection types, function types, and type parameters.
7. r[data.embed.generic-name] For `Box[T]`, the embedded field's name and construction key are `Box`; type arguments are not part of the key.
8. r[data.embed.unique] The name must be unique among the outer data type's fields. A duplicate name that involves an embedded field is an error. Error: `duplicate-embedded-field`.

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

Embedding is limited in width and depth.

| Rule | Limit | Error | Reported on |
| --- | --- | --- | --- |
| r[data.embed.width] Width | A data type may declare at most three embedded fields. | `too-many-embedded-fields` | the fourth embedded field |
| r[data.embed.depth] Depth | Embedding chains may be at most three levels deep. | `embedding-too-deep` | the embedded field that begins the first chain that reaches depth 4 |

1. r[data.embed.depth.chain] `C` may embed `P1`, which embeds `P2`, which embeds `P3`. A part at depth 4, as when `P3` embeds `P4`, is an error.
2. r[data.embed.depth.every-type] The `embedding-too-deep` error occurs at the declaration of every data type that reaches such a part.
3. r[data.embed.depth.message] Its message shows the chain, as in `C > P1 > P2 > P3 > P4`.
4. r[data.embed.depth.generic] Depth counts embedded fields of generic data types like any other, with their type arguments substituted.
5. r[data.embed.depth.self] A type that embeds itself, directly or through other types, always exceeds the limit.

```text
data Post:
    Created
    Updated
    Owned
    Tagged  # error: too-many-embedded-fields
    title: string
```

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

> **Note.** Together the two limits bound a data type's embedding tree to at
> most 3 + 9 + 27 = 39 parts.[^miku]

[^miku]: 39 reads as "mi-ku" in Japanese (san-kyuu, 3-9), a nod to Hatsune Miku. The bound was not chosen for this reason, but it is a happy coincidence.

### Parts And Copies

An embedded field holds a **part** of the outer value: a value of the
embedded type that the outer value receives as its own copy.

1. r[data.part.construct] Filling an embedded field stores a copy of the supplied value, never the value itself.
2. r[data.part.marker] The copy is written: the field label is followed by `...`, as in `Post { Timestamps: ...ts, id: "p" }`.
3. r[data.part.marker-scope] The `...` applies to the whole field expression.
4. r[data.part.marker-fresh] The `...` is required even for a fresh literal, as in `Timestamps: ...Timestamps { created_at: 1, updated_at: 1 }`.
5. r[data.part.copy] The copy of a value `e` of type `E` is a new `E`. Its ordinary fields hold the values of `e`'s fields, copied shallowly as copy-update copies them. Its embedded fields hold copies of `e`'s parts, made by the same rule.
6. r[data.part.not-shared] `Post { Timestamps: ...ts }` therefore never shares a part with `ts`. A later change to `post`'s part is not seen through `ts`, and a change through `ts` is not seen in `post`.
7. r[data.part.shallow] Composite values that the part's ordinary fields reference are still shared.
8. r[data.part.readonly-source] The supplied value may be readonly.
9. r[data.part.copy-update] A copy-update literal, as in `Post { ...post, title: "t" }`, copies each embedded part of its spread source that it does not replace, by the same rule. A copy therefore never shares a part with its original.
10. r[data.part.store] An embedded field of a `mut` value is assigned with the copy assignment `...=`, as in `post.Timestamps ...= stamps`. It stores a copy of `stamps` by the same rule.

#### Copy Markers

1. r[data.part.marker-required] The copy marker is required exactly where a copy is made.
2. r[data.part.missing-marker] An embedded field initialized without it, as in `Post { Timestamps: ts }`, is an error. Error: `embedded-copy-required`.
3. r[data.part.plain-assignment] An embedded field assigned with plain `=`, as in `post.Timestamps = ts`, is an error. Error: `embedded-copy-required`.
4. r[data.part.suggestion] The `embedded-copy-required` message suggests the `...` form.
5. r[data.part.ordinary-label] A `...` after the label of any other field is an error. Error: `copy-into-ordinary-field`.
6. r[data.part.ordinary-store] `...=` on any other place is an error. Error: `copy-into-ordinary-field`.

```text
fn invalid(post: mut Post, ts: Timestamps, account: mut Account, user: User) -> void:
    fresh := Post { Timestamps: ts, id: "p" }  # error: embedded-copy-required
    post.Timestamps = ts                       # error: embedded-copy-required
    other := Account { owner: ...user }        # error: copy-into-ordinary-field
    account.owner ...= user                    # error: copy-into-ordinary-field
```

1. r[data.part.prefix] A prefix `...` therefore always means "copy the named members of this value".
2. r[data.part.suffix] A suffix `...` always spreads elements or entries, and never copies.

| Form | Example | Meaning |
| --- | --- | --- |
| Leading spread | `User { ...user }` | copies the source's fields into the new value |
| Embedded field label | `Label: ...value` | copies `value`'s fields into the part |
| Suffix `...` | `f(xs...)`, `[0, xs...]`, `$.with(ctx...)` | spreads elements or entries |

See also: [Primary Expressions](02-grammar.md#primary-expressions).

#### When Copies Are Made

1. r[data.part.copy-sites] Copies are made only by construction, copy-update, and stores into an embedded field.
2. r[data.part.no-implicit-copy] Passing, returning, binding, or matching the outer value, or reading its part, never copies.
3. r[data.part.copy-time] A part is copied as soon as the value that fills it is evaluated.
4. r[data.part.copy-time.field] In a literal, that is at the position of its field expression, before any later field expression runs.
5. r[data.part.copy-time.spread] For parts supplied by a spread, that is when the spread is evaluated, before every explicit field expression.
6. r[data.part.copy-time.effects] A side effect of a later field expression on the source is therefore not seen in the copy.

### Access Through Parts

1. r[data.part.access] Reading an embedded field through a `mut` outer value yields `mut` access to the part. Reading it through a readonly outer value yields readonly access. For example, if `post` has type `mut Post`, `post.Timestamps` has type `mut Timestamps`; if `post` has type `Post`, it has type `Timestamps`.
2. r[data.part.access.step] This is the embedded-field step of [Mutable Paths](04-type-system.md#mutable-paths).
3. r[data.part.access.promoted] Promoted members are reached through the same step. Through a `mut Post`, a promoted field may be assigned and a promoted `mut self` method called.
4. r[data.part.access.readonly-promoted] Through a readonly `Post`, the promoted field is readonly and the promoted `mut self` call is an error. Error: `mutable-receiver-required`.
5. r[data.part.alias] A read of an embedded field yields the part itself, not a copy.
6. r[data.part.alias.mut] `let stamps = post.Timestamps` on a `mut Post` binds a `mut Timestamps` alias, and a mutation through either name is observed through the other.
7. r[data.part.alias.readonly] On a readonly `Post` the alias is readonly.
8. r[data.part.alias.binding] A `:=` binding exposes a readonly view, as it does for every composite value.

The following example assumes the `Post` and `Timestamps` declarations
above:

```text
impl Timestamps:
    pub fn touch(mut self, at: i64) -> void:
        self.updated_at = at

fn edit(post: mut Post, stamps: Timestamps) -> void:
    post.touch(1700000100)            # promoted mut self method
    post.updated_at = 1700000200      # promoted field through a mut root
    let alias = post.Timestamps       # mut Timestamps, the same part
    alias.created_at = 1700000000     # observed as post.created_at
    let copy: mut Post = Post { Timestamps: ...stamps, id: "p", title: "t" }
    copy.touch(1700000300)            # changes copy's part, never stamps
    post.Timestamps ...= stamps       # copy assignment
```

```text
fn invalid(post: Post) -> void:
    post.touch(5)  # error: mutable-receiver-required
```

#### Part Ownership

1. r[data.part.owned] The part is owned by the outer value only in this sense: the language copies it whenever a part is filled. No two outer values therefore receive the same part.
2. r[data.part.aliases-untracked] The language does not track or prevent later aliases.
3. r[data.part.kept-references] A read of the part and a `mut self` method of the embedded type that stores `self` elsewhere both keep a reference to the part. Changes through that reference are observed through the outer value.
4. r[data.part.layout] An implementation may lay a part out inline or as a separate object referenced only by its outer value.
5. r[data.part.elided-copy] An implementation may omit the copy of a value that nothing else can reference, such as a fresh literal.
6. r[data.part.unobservable] Neither choice is observable.

### Mutable Edges

This section defines when a copy of a readonly value has mutable access.

1. r[data.edge.principle] A copy of a readonly value has mutable access only when nothing mutable is read through a readonly view to make it.
2. r[data.edge.definition] A data type has **mutable edges** when it declares a direct `field: mut U`, or embeds a type that has mutable edges.
3. r[data.edge.copy-type] The copy of `e` has type `mut E` when `e` has type `mut E`, or when `E` has no mutable edges; otherwise it has readonly type `E`.
4. r[data.edge.readonly-literal] A literal with a readonly copy is readonly, and so is the stored value.
5. r[data.edge.upgrade-literal] Where `mut Post` is required, such a literal is an error. Error: `mutable-upgrade`.
6. r[data.edge.upgrade-store] Storing a readonly copy is an error only where the store's target requires a mutable part: a `mut` field, parameter, or binding. Error: `mutable-upgrade`.
7. r[data.edge.generic] A copy's generic fields keep their substituted types, as generic fields always do.

```text
data Stamp:
    at: i32
    owner: mut Owner  # a mutable edge

data Post:
    Stamp
    id: string

fn invalid(stamp: Stamp) -> void:
    let post: mut Post = Post { Stamp: ...stamp, id: "p" }  # error: mutable-upgrade
    kept := Post { Stamp: ...stamp, id: "q" }               # valid: the binding is readonly
```

> **Why.** The copy reads each direct `mut U` field of the readonly `e` as
> `U`, as a readonly copy-update does.

> **Note.** Adding a direct `mut U` field to a data type is therefore a
> breaking change for every type that embeds it, at any depth. The embedded
> type gains a mutable edge, and a copy of a readonly value of it becomes
> readonly. A literal elsewhere that fills the part from a readonly value
> and is used as `mut` then becomes a `mutable-upgrade` error.

See also: [Bindings And Fresh Values](04-type-system.md#bindings-and-fresh-values).

### Embedded Field Variance

1. r[data.embed.variance] For variance, an embedded field is an invariant position: a covariant or contravariant parameter used in an embedded field's type is an error. Error: `invalid-variance`.

```text
data Box[+T]:
    value: T

data Holder[+T]:
    Box[T]  # error: invalid-variance
```

> **Why.** Access through an embedded field follows the container.

See also: [Variance](04-type-system.md#variance).

### Member Promotion

1. r[data.promote.members] Embedding promotes the embedded type's `pub` fields and `pub` inherent methods for convenient access.
2. r[data.promote.depth] The `pub` fields and `pub` inherent methods of every part, at any depth, are promoted.
3. r[data.promote.shallowest] For each name the shallowest member hides deeper ones, so the receiver's `pub` own members come first and each embedded type decides its own names.
4. r[data.promote.private] A private member of a part is never promoted, even in the module that declares it. It is reached through the explicit path, as in `post.Timestamps.secret`.
5. r[data.promote.same-depth] Two members with one name at the same smallest depth are an error. An example is the embedded field name of a type embedded twice at one depth. Error: `ambiguous-promoted-member`.
6. r[data.promote.private-own] A private own member with the name of a promoted member is also an error: a private member never shadows a promoted one. Error: `ambiguous-promoted-member`.
7. r[data.promote.at-declaration] An `ambiguous-promoted-member` error is reported at the outer type's declaration, never at a use.
8. r[data.promote.uniform] Every module therefore sees the same members of a type.
9. r[data.promote.no-trait-methods] An embedded type's trait methods are never promoted and have no effect on lookup. Such a method is called through the embedded field, as in `x.Label.to_string()`.
10. r[data.promote.receiver-trait] A trait method of the receiver's type counts only where its trait is available, and a promoted method beside it is an error. Error: `ambiguous-method`.

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

1. r[data.embed.not-subtype] Embedding is composition, not subtyping: the outer data type is not assignable to the embedded type.
2. r[data.embed.no-override] Embedding has no overriding: a promoted method runs as the embedded type's own method, with the embedded value as its receiver.
3. r[data.embed.self-call] Inside `Base`'s methods, `self.m()` is always `Base`'s `m`, even when a type that embeds `Base` declares its own `m`.
4. r[data.embed.no-conformance] Embedding never grants trait conformance, and a promoted method never fills a method of a trait implementation.
5. r[data.embed.delegation] Conformance through a part is written explicitly: `impl Describe for Service by Logger` implements `Describe` for `Service` by forwarding every method to its embedded `Logger`.

See also: [Embedding And Trait Satisfaction](09-traits.md#embedding-and-trait-satisfaction),
[Trait Delegation](09-traits.md#trait-delegation).

### Embedded Field Metadata

1. r[data.embed.metadata] An embedded field accepts the same prefix metadata decorators as a named field.
2. r[data.embed.metadata.target] The metadata is attached to the embedded field itself, whose name is the final type name and whose declared type includes any generic arguments.
3. r[data.embed.metadata.not-promoted] The metadata is not copied to fields or methods promoted from the embedded value.

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

### Variant Payloads

Variants may carry payloads:

```text
enum ToolError:
    NotFound(resource: string)
    Unauthorized(reason: string)
    RateLimited(retry_after_ms: i32)
```

1. r[data.enum.payload] A variant may carry payload parameters.
2. r[data.enum.payload.order] Payload parameters follow function definition conventions: unnamed positional parameters first, followed by named parameters.
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
5. r[data.enum.fn-value.generic] For a generic enum, or a variant with its own generic parameters, the value follows the rule for generic function values. That rule requires an expected monomorphic function type to instantiate every generic parameter.
6. r[data.enum.fn-value.shorthand] The contextual shorthand `.Variant` still needs an expected enum type, so it is never a function value.
7. r[data.enum.fn-value.call] Calling the value constructs the variant, exactly as calling the constructor does.
8. r[data.enum.fn-value.multiple] A variant constructor with two or more payload fields is not a function value and must be called.
9. r[data.enum.fn-value.unsaturated] Using one without an argument clause is an error. Error: `unsaturated-enum-constructor`.
10. r[data.enum.fn-value.closure] An explicit closure passes its construction as a function value.
11. r[data.enum.fn-value.payload-free] A payload-free variant, including one whose declaration initializes shared enum data, is an enum value and is selected without `()`.

```text
fn apply(make: fn(i32, FsError) -> SyncError) -> SyncError:
    make(1, FsError.NotFound("a.txt"))

fn retry() -> SyncError:
    apply(SyncError.Retry)  # error: unsaturated-enum-constructor
```

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
6. r[data.shared.compile-time] Each variant's constructor expression is evaluated once, at compile time, by the evaluator that annotation values use, and it must be requirement-free.
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

See also: [Optional Types](04-type-system.md#optional-types),
[Result Types](04-type-system.md#result-types).

## Representation And Garbage Collection

1. r[data.repr.backend] Data and enum representation is chosen by the Wasm GC backend subject to observable language semantics.
2. r[data.repr.no-dependence] Programs must not depend on field offsets, variant tags, object addresses, or representation identity unless a future interop facility exposes them explicitly.
3. r[data.repr.gc] Reachable composite values are garbage collected, and unreachable reference cycles are reclaimable.
4. r[data.repr.no-manual] The language does not expose manual deallocation, user-visible finalizers, or weak references.
5. r[data.repr.deferred] Weak references and finalizers are deferred rather than permanently ruled out.
6. r[data.repr.cleanup] Resource cleanup is separate from memory reclamation.
7. r[data.repr.defer] Block-scoped `defer` provides explicit synchronous cleanup on ordinary control-flow exits and cancellation; it is not an ownership or garbage-collection mechanism.

Ownership, alias-escape prevention, automatic finalization, and asynchronous
or fallible cleanup policy remain deferred in
[Open Issues](../future-work/OPEN_ISSUES.md).

## Generalized Algebraic Data Types

1. r[data.gadt.declare] Variants may declare explicit refined result types and variant-local generic parameters.
2. r[data.gadt.refine] Matching such a variant refines the subject type within that arm.

See also: [Generalized Algebraic Data Types](13-gadts.md).

## Unsupported Aggregate Extensions

1. r[data.unsupported.mut-embedded] hd-lang has no `mut` embedded-field shorthand, because access to an embedded part already follows its container.
2. r[data.unsupported.layout] Stable object layout and component-model representation are ABI concerns and are not observable core-language semantics.

See also: [Variant Payloads](#variant-payloads), where
`data.enum.payload.no-field-blocks` rules out variant field blocks.
