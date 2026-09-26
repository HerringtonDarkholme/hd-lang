# Data Types and Enums

Status: language specification draft.

`struct` is not a declaration keyword; diagnose `struct Name:` as
`old-struct-declaration` so obsolete source does not masquerade as a sequence
of identifiers.

Data types are nominal product types with reference semantics. Enums are nominal
sum types. Neither is a class, and neither creates an inheritance hierarchy.
`data` names the declaration form without suggesting value-type copying.

## Data Declarations

A `data` declaration defines named, typed fields:

```text
data User:
    id: string
    email: string
    display_name: string? = .None
```

Field names must be unique within the data type. Every field has an explicit type.
Fields have no standalone `mut` modifier. `friend: mut User` declares a field
whose type grants mutable access through that reference; `mut friend: User`
is invalid. An embedded field is written without `mut`: `Base` embeds `Base`,
and `mut Base` is a `mutable-embedded-field` error, because access to an
embedded part already follows its container
([Data Embedding](#data-embedding)).
An ordinary named field may have a default expression. It must be assignable to
the declared field type and obey the same requirement-free rule as a
function-parameter default in [Functions](07-functions.md#default-values). A
default is evaluated separately for each
construction, not when the data type is declared. It sees the declaration's
lexical scope but does not implicitly bind other fields of the new value.
Embedded fields have no default syntax.
Named fields are module-private unless individually marked `pub`. A public
data type does not make its unmarked fields public. An embedded field takes
no marker and is always public: it is visible wherever its outer type is, so
promoted members need only their own visibility
([Member Resolution](03-names-and-scopes.md#member-resolution)). Embedding a
module-private data type in a public data type is therefore a
`private-type-leak` error
([Public Uses And Visibility](10-modules.md#public-uses-and-visibility)). In
another module, a data literal may construct the type only when all its
fields are public; private fields cannot be named, initialized, or carried
through a copy-update literal there. A public factory function can construct
a value with private fields inside the defining module.

Data identity is nominal. Two declarations with the same fields introduce
different types.

An empty nominal data type uses `pass`:

```text
data Validation: pass

facet := Validation {}
```

A value of a fieldless data type is canonical: every `Validation {}` is the
same value with the same identity, and constructing it allocates nothing
([Expressions](05-expressions.md#unary-and-binary-operators)).

Data declarations may be directly recursive because composite fields use
managed references. Module-level data types may also be mutually recursive;
local data types follow declaration-point visibility and cannot refer to a later
local declaration. Recursion does not imply optionality: a program must still
provide a value for every required field during construction, so a recursive
graph normally includes an optional, enum, list, or another finite base case.
Passing a data value to a function passes a shared reference, not a copy of its
fields. The `T` and `mut T` views control mutation through that reference as
specified in [Type System](04-type-system.md#composite-values-and-access-permission).

## Construction And Access

Data values use typed brace literals:

```text
user := User {
    id: "user_123",
    email: "ada@example.com",
    display_name: "Ada",
}
```

Each field without a default must be initialized exactly once. A field with a
default may be omitted or supplied explicitly. Unknown and duplicate fields
are compile-time errors. Fields may appear in any order. Explicit field
expressions are evaluated in source order, then defaults for omitted fields
are evaluated in field declaration order. A copy-update spread supplies every
field, so it does not evaluate defaults for unlisted fields.
Each copied field is checked through the spread source's access view. A
readonly source can supply its effective `U` value for a direct `mut U` field
when the result is also readonly `T`; producing `mut T` requires a `mut U`
replacement. Embedded parts are copied rather than shared
([Data Embedding](#data-embedding)). Generic fields retain their substituted type in both views. See
[Data Expressions](05-expressions.md#data-expressions).

Field access uses `value.field`. A `mut T` root may reassign any of its fields,
regardless of the field's declared mutability; a readonly `T` value cannot
reassign any field. Nested mutation or a `mut self` call through a field
requires the field read to have a `mut` access type, as specified in
[Mutable Paths](04-type-system.md#mutable-paths). Reading a
`field: mut U` through a readonly value yields only `U`. A readonly data value
may be constructed with `U` in that direct field, while a mutable data value
requires `mut U`. An embedded field instead receives a copy
([Data Embedding](#data-embedding)). A generic field declared `field: P` retains its substituted
type: `P = mut U` requires and exposes `mut U` even in a readonly outer value.
Direct assignment to a visible field enforces its declared type, not arbitrary
validation or cross-field invariants. Keep fields private and expose controlled
methods when writes to those fields must preserve such invariants. This does
not control other mutable aliases to an object stored in a private field; the
language permits shared mutable children and does not guarantee invariants
over the entire reachable object graph.
In another module, field access reaches only public fields; member lookup
skips the others ([Member Resolution](03-names-and-scopes.md#member-resolution)).
Data values may be destructured in `match` patterns using the same
`DataName { ... }` form. The pattern may mention any subset of visible
fields; omitted fields are not tested. Within the braces, `field` binds the
field value and `field: pattern` applies a nested pattern. See
[Match Expressions](06-control-flow.md#match-expressions).

Copy-update construction uses one leading spread:

```text
renamed := User {
    ...user,
    display_name: "Ada Lovelace",
}
```

The spread has the same data type and supplies every field. Explicit fields
replace copied values. This creates a data value; it does not mutate the
spread source.

## Data Embedding

A bare type-name member embeds another data type:

```text
data Post:
    Timestamps
    id: string
    title: string
```

The embedded field's name is the embedded type name. Construction uses that
name as its key, followed by `...`, because the field receives a copy of the
value (see below):

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

Embedded shorthand accepts a named data type, including one with generic
arguments. For `Box[T]`, the embedded field's name and construction key are
`Box`; type arguments are not part of the key. The name must be unique among
the outer data type's fields: a duplicate name that involves an embedded
field, such as embedding both `Box[i32]` and `Box[string]`, is a
`duplicate-embedded-field` error.

An embedded field holds a **part** of the outer value: a value of the embedded
type that the outer value receives as its own copy.

- **Construction copies.** Filling an embedded field stores a copy of the
  supplied value, never the value itself. The copy is written: the field
  label is followed by `...`, as in `Post { Timestamps: ...ts, id: "p" }`.
  The `...` applies to the whole field expression and is required even for
  a fresh literal, as in `Timestamps: ...Timestamps { created_at: 1,
  updated_at: 1 }`. The copy of a value `e` of type `E`
  is a new `E` whose ordinary fields hold the values of `e`'s fields,
  copied shallowly as copy-update copies them, and whose embedded fields hold
  copies of `e`'s parts, made by the same rule. `Post { Timestamps: ...ts }`
  therefore never shares a part with `ts`: a later change to `post`'s part
  is not seen through `ts`, and a change through `ts` is not seen in
  `post`. Composite values that the part's ordinary fields reference are
  still shared. The supplied value may be readonly.
- **Copy-update copies.** A copy-update literal, written as before with a
  leading spread, as in `Post { ...post, title: "t" }`, copies each embedded
  part of its spread source that it does not replace, by the same rule, so a
  copy never shares a part with its original.
- **Stores copy.** An embedded field of a `mut` value is assigned with the
  copy assignment `...=`, as in `post.Timestamps ...= stamps`, which stores a
  copy of `stamps` by the same rule.
- **Access follows the container.** Reading an embedded field through a
  `mut` outer value yields `mut` access to the part, and reading it through a
  readonly outer value yields readonly access. If `post` has type
  `mut Post`, `post.Timestamps` has type `mut Timestamps`; if `post` has type
  `Post`, it has type `Timestamps`. This is the embedded-field step of
  [Mutable Paths](04-type-system.md#mutable-paths). Promoted members are reached
  through the same step, so through a `mut Post` a promoted field may be
  assigned and a promoted `mut self` method called, while through a readonly
  `Post` the field is readonly and the call is `mutable-receiver-required`.
- **Reading a part out aliases it.** A read of an embedded field yields the
  part itself, not a copy. `let stamps = post.Timestamps` on a `mut Post`
  binds a `mut Timestamps` alias, and a mutation through either name is
  observed through the other; on a readonly `Post` the alias is readonly. A
  `:=` binding exposes a readonly view, as it does for every composite value.

The copy marker is required exactly where a copy is made. An embedded field
initialized without it, as in `Post { Timestamps: ts }`, or assigned with
plain `=`, as in `post.Timestamps = ts`, is an `embedded-copy-required`
error, whose message suggests the `...` form. A `...` after the label of any
other field, or `...=` on any other place, is a `copy-into-ordinary-field`
error. A prefix `...` therefore always means "copy the named members of this
value": a leading spread copies the source's fields into the new value, and
`Label: ...value` copies `value`'s fields into the part. A suffix `...`
always spreads elements or entries, as in `f(xs...)`, `[0, xs...]`, and
`$.with(ctx...)`, and never copies
([Primary Expressions](02-grammar.md#primary-expressions)).

Copies are made only by construction, copy-update, and stores into an
embedded field. Passing, returning, binding, or matching the outer value, or
reading its part, never copies. A part is copied as soon as the value that
fills it is evaluated: at the position of its field expression in a literal,
before any later field expression runs, and for parts supplied by a spread,
when the spread is evaluated, before every explicit field expression. A side
effect of a later field expression on the source is therefore not seen in
the copy.

A copy of a readonly value has mutable access only when nothing mutable is
read through a readonly view to make it. A data type has **mutable edges**
when it declares a direct `field: mut U`, or embeds a type that has mutable
edges. The copy of `e` has type `mut E` when `e` has type `mut E`, or when
`E` has no mutable edges; otherwise it has readonly type `E`, because the
copy reads each direct `mut U` field of the readonly `e` as `U`, as a
readonly copy-update does. A literal with a readonly copy is readonly, and so
is the stored value, as
[Bindings And Fresh Values](04-type-system.md#bindings-and-fresh-values) states:
where `mut Post` is required, such a literal is a `mutable-upgrade` error, and
so is a store of such a copy. A copy's generic fields keep their substituted
types, as generic fields always do.

Adding a direct `mut U` field to a data type is therefore a breaking change
for every type that embeds it, at any depth: the embedded type gains a
mutable edge, a copy of a readonly value of it becomes readonly, and a literal
elsewhere that fills the part from a readonly value and is used as `mut`
becomes a `mutable-upgrade` error.

The part is owned by the outer value only in this sense: the language copies
it whenever a part is filled, so no two outer values receive the same part.
It does not track or prevent later aliases. A read of the part, as above,
and a `mut self` method of the embedded type that stores `self` elsewhere
both keep a reference to the part, and changes through that reference are
observed through the outer value. An implementation may lay a part out
inline or as a separate object referenced only by its outer value, and may
omit the copy of a value that nothing else can reference, such as a fresh
literal; neither choice is observable.

```text
impl Timestamps:
    fn touch(mut self, at: i64) -> void:
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

For [Variance](04-type-system.md#variance), an embedded field is an invariant
position, because access through it follows the container: a covariant or
contravariant parameter used in an embedded field's type is an
`invalid-variance` error.

Embedding promotes the embedded type's fields and inherent methods for
convenient access. Which field `x.name` or method `x.name(args)` selects is
defined once in [Member Resolution](03-names-and-scopes.md#member-resolution):
the fields and inherent methods of every part, at any depth, are promoted;
for each name the shallowest member hides deeper ones, so the receiver's own
members come first and each embedded type decides its own names. Two members
with one name at the same smallest depth, such as the embedded field name
of a type embedded twice at one depth, are an `ambiguous-promoted-member`
error at the outer type's declaration, never at a use. Members not visible
from the calling module do not take part (a private member of an embedded
type is never reported). An embedded type's trait methods are never promoted
and have no effect on lookup; such a method is called through the embedded
field, as in `x.Label.to_string()`. A trait method of the receiver's type
counts only where its trait is available, and a promoted method beside it is
`ambiguous-method`.

Embedding is composition, not subtyping. The outer data type is not
assignable to the embedded type. Embedding has no overriding: a promoted
method runs as the embedded type's own method, with the embedded value as its
receiver, so inside `Base`'s methods `self.m()` is always `Base`'s `m`, even
when a type that embeds `Base` declares its own `m`. Embedding never grants
trait conformance, and a promoted method never fills a method of a trait
implementation; see
[Embedding And Trait Satisfaction](09-traits.md#embedding-and-trait-satisfaction).
Conformance through a part is written explicitly: `impl Describe for Service
by Logger` implements `Describe` for `Service` by forwarding every method to
its embedded `Logger` ([Trait Delegation](09-traits.md#trait-delegation)).

An embedded field accepts the same prefix metadata decorators as a named field.
The metadata is attached to the embedded field itself, whose name is the final
type name and whose declared type includes any generic arguments. It is not
copied to fields or methods promoted from the embedded value.

## Enum Declarations

An enum defines a closed set of variants:

```text
enum JobStatus:
    Queued
    Running
    Succeeded
    Failed
```

Variant names must be unique within the enum. The qualified form is
`JobStatus.Queued`; `.Queued` is also valid where an expected type
fixes `JobStatus`. A matching name in another enum does not create ambiguity
when that expected type is known, and a unique name across the program does
not make the shorthand valid without an expected enum type.

A variant may carry payload parameters:

```text
enum ToolError:
    NotFound(resource: string)
    Unauthorized(reason: string)
    RateLimited(retry_after_ms: i32)
```

Payload parameters follow function definition conventions: unnamed positional
parameters first, followed by named parameters. Each payload type is explicit.
Large payloads should use a separate data type rather than a nested field block;
variant field blocks are not part of the language.

Variant construction follows function-call conventions. Positional arguments
come before named arguments:

```text
error := ToolError.NotFound(resource="user_123")
```

A named argument that names no payload field of the variant is an
`unknown-data-field` error. Other argument errors use the call codes in
[Expressions](05-expressions.md#calls), such as `duplicate-argument`.

A payload-bearing variant constructor is not itself a first-class function
value  and must be called. Use an explicit closure to pass construction as
a function value. A payload-free variant, including one whose declaration
initializes shared enum data, is an enum value and is selected without `()`.

## Shared Enum Constructor Data

An enum may declare data shared by all variants. Constructor parameters may be
unnamed or named:

```text
enum StatusCode(i32):
    Ok -> StatusCode(200)
    NotFound -> StatusCode(404)

enum HttpStatus(code: i32, phrase: string, retryable: bool = false):
    Ok -> HttpStatus(200, phrase="OK")
    NotFound -> HttpStatus(404, phrase="Not Found")
    ServiceUnavailable -> HttpStatus(503, phrase="Service Unavailable", retryable=true)
```

Each variant with shared enum data must provide its enum constructor expression
after `->`. The constructor call follows ordinary positional/named argument
ordering and must initialize each shared parameter without a default. A shared
parameter may declare a default expression. After the first defaulted
parameter, every following shared parameter must also have a default, as with
function parameters; a later parameter without one is a `default-order`
error. The default must satisfy the same requirement-free rule as a
function-parameter or data-field default and is evaluated for each construction
when omitted. Explicit argument expressions are evaluated first, then omitted
defaults in parameter declaration order. A default may refer to earlier named
shared parameters but not later ones.

Shared constructor data is part of every enum value. A named constructor
parameter is available as a field on the enum value; an unnamed parameter uses
zero-based tuple-style numeric access:

```text
code := StatusCode.NotFound.0
phrase := HttpStatus.NotFound.phrase
```

Shared fields follow ordinary composite access permissions. Reading through a
readonly enum yields a readonly viewpoint; assignment requires a mutable enum root
and the required mutable edges. Variant payload fields remain available through
pattern matching rather than direct field access, because they do not exist on
every variant.

Within one variant, a named payload parameter must not duplicate a named shared
constructor parameter. Variant payload parameters do not have defaults.

## Generic And Recursive Enums

Enums may be generic and recursive:

```text
enum Maybe[T]:
    Some(value: T)
    None

enum Tree[T]:
    Leaf(value: T)
    Branch(left: Tree[T], right: Tree[T])
```

Without an explicit GADT result clause, every variant constructs the enclosing
enum instantiated with the declaration's type arguments. Explicit refined
results and variant-local generic parameters are defined in
[Generalized Algebraic Data Types](13-gadts.md).

## Matching Enums

Enum values are inspected with exhaustive `match`. Variant patterns may be
qualified, or use `.Variant` when the matched value fixes their enum:

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

The shorthand also works for payload construction and nested variant patterns
when their expected enum type is fixed. It is not a way to infer an enum from
the variant spelling alone: `status := .Queued` has no contextual enum type and
is rejected.

Payload patterns use parentheses, not braces. Positional patterns may bind names
different from declared field names. Named patterns use `field=pattern`, and no
positional pattern may follow a named one:

```text
Expr.Add(l, r)
Expr.Sub(left=l, right=r)
Expr.Scale(value, factor=2)
```

Literals and nested variant patterns may constrain payload values. Pattern
scope and exhaustiveness are defined in [Control Flow](06-control-flow.md).

## Option And Result

Optional `T?` is sugar for the prelude enum
`enum Option[T]: Some(value: T); None`
([Optional Types](04-type-system.md#optional-types)). Its values are built
with `.Some(value)`, `.None`, `Option.Some(value)`, and `Option.None`, and
matched with the same spellings as patterns. The only language support beyond
an ordinary enum is the `T?` spelling, the one-layer implicit wrap of a plain
`T` value where `T?` is expected, and postfix `?`.

`Result[T, E]` behaves as a standard enum-like type with language support for
postfix `?`. `Ok(value)` and `Err(error)` are the construction spellings for
`Result`. In patterns, `Ok(pattern)` and `Err(pattern)` are the corresponding
unqualified built-in spellings; they do not make ordinary enum variants
directly nameable through `use`.

The representation of either type is an ABI decision, not a source-language
difference. An implementation may, for example, represent `.None` as a null
reference.

## Representation And Garbage Collection

Data and enum representation is chosen by the Wasm GC backend subject to
observable language semantics. Programs must not depend on field offsets,
variant tags, object addresses, or representation identity unless a future
interop facility exposes them explicitly.

Reachable composite values are garbage collected, and unreachable reference
cycles are reclaimable. The language does not expose manual deallocation in the
language, user-visible finalizers, or weak references. Weak references and
finalizers are deferred rather than permanently ruled out. Resource cleanup is
separate from memory reclamation. Block-scoped `defer` provides explicit
synchronous cleanup on ordinary control-flow exits and cancellation; it is not
an ownership or garbage-collection mechanism. Ownership, alias-escape
prevention, automatic finalization, and asynchronous or fallible cleanup policy
remain deferred in [Open Issues](../future-work/OPEN_ISSUES.md).

## Generalized Algebraic Data Types

Variants may declare explicit refined result types and variant-local generic
parameters. Matching such a variant refines the subject type within that arm as
specified in [Generalized Algebraic Data Types](13-gadts.md).

## Unsupported Aggregate Extensions

hd-lang has no variant field blocks. It has no `mut` embedded-field
shorthand either, because access to an embedded part already follows its
container.
Stable object layout and component-model representation are ABI concerns and
are not observable core-language semantics.
