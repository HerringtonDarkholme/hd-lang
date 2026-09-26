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
    display_name: string? = nil
```

Field names must be unique within the data type. Every field has an explicit type.
Fields have no standalone `mut` modifier. `friend: mut User` declares a field
whose type grants mutable access through that reference; `mut friend: User`
is invalid. The same rule applies to embedded fields: `Base` embeds `Base`,
but `mut Base` is invalid. Embedded fields cannot declare a mutable edge.
An ordinary named field may have a default expression. It must be assignable to
the declared field type and obey the same requirement-free rule as a
function-parameter default in [Functions](07-functions.md#default-values). A
default is evaluated separately for each
construction, not when the data type is declared. It sees the declaration's
lexical scope but does not implicitly bind other fields of the new value.
Embedded fields have no default syntax.
Fields, including embedded fields, are module-private unless individually
marked `pub`. A public data type does not make its unmarked fields public. In
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
replacement. Generic fields retain their substituted type in both views. See
[Data Expressions](05-expressions.md#data-expressions).

Field access uses `value.field`. A `mut T` root may reassign any of its fields,
regardless of the field's declared mutability; a readonly `T` value cannot
reassign any field. Nested mutation or a `mut self` call through a field
requires the field read to have a `mut` access type, as specified in
[Mutable Paths](04-type-system.md#mutable-paths). Reading a
`field: mut U` through a readonly value yields only `U`. A readonly data value
may be constructed with `U` in that direct field, while a mutable data value
requires `mut U`. A generic field declared `field: P` retains its substituted
type: `P = mut U` requires and exposes `mut U` even in a readonly outer value.
Direct assignment to a visible field enforces its declared type, not arbitrary
validation or cross-field invariants. Keep fields private and expose controlled
methods when writes to those fields must preserve such invariants. This does
not control other mutable aliases to an object stored in a private field; the
language permits shared mutable children and does not guarantee invariants
over the entire reachable object graph.
Cross-module field access additionally requires the field to be public.
Data values may be destructured in `match` patterns using the same
`DataName { ... }` form. The pattern may mention any subset of visible
fields; omitted fields are not tested. Within the braces, `field` binds the
field value and `field=pattern` applies a nested pattern. See
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
name as its key:

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

Embedding promotes fields and methods for convenient access but does not make
the outer data type a subtype of the embedded type. Promotion follows Go-style
shortest-path resolution. A direct member hides promoted members. Multiple
equally short promoted members are ambiguous and require qualification through
the embedded field.
Cross-module access through an embedded field requires that field and the
promoted member to be public.

Embedding never grants trait conformance. Inside an explicit trait `impl`, an
unambiguous promoted method may supply a required method. Ambiguous promoted
methods require an explicit method body and a qualified embedded-field call.

Embedded shorthand accepts a named data type, including one with generic
arguments. For `Box[T]`, the embedded field's name and construction key are
`Box`; type arguments are not part of the key. The name must be unique among
the outer data type's fields, so embedding both `Box[i32]` and `Box[string]`
is a duplicate-field error. Explicitly mutable embedded-field shorthand is
not supported; use an ordinary named field with a `mut` type for a mutable
edge, without promotion.

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
function parameters. The default must satisfy the same requirement-free rule as a
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

Optional `T?` and `Result[T, E]` behave as standard enum-like types, with
language support for `nil` and postfix `?`. `Ok(value)` and `Err(error)` are the
construction spellings for `Result`. In patterns, `Ok(pattern)` and
`Err(pattern)` are the corresponding unqualified built-in spellings; they do
not make ordinary enum variants directly nameable through `use`.

Optional patterns include `nil`, `value?` for the present case, `_`, and a bare
catch-all binding. The pattern `value?` binds the contained value; it is not a
`Some` constructor. A plain `T` value constructs a present `T?` wherever that
optional type is expected.

Whether these are literally user-definable standard-library enums or compiler
intrinsics with equivalent semantics is an ABI decision, not a source-language
difference.

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

hd-lang has no variant field blocks or explicitly mutable embedded-field
shorthand. Use an explicit named field with a `mut` type when a mutable
reference edge is required.
Stable object layout and component-model representation are ABI concerns and
are not observable core-language semantics.
