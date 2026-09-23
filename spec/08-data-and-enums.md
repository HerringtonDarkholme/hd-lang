# Data Types and Enums

Status: language specification draft.

Data types are nominal product types with reference semantics. Enums are nominal
sum types. Neither is a class, and neither creates an inheritance hierarchy.
`data` names the declaration form without suggesting value-type copying.

## Data Declarations

A `data` declaration defines named, typed fields:

```text
data User:
    id: string
    email: string
    display_name: string?
```

Field names must be unique within the data type. Every field has an explicit type.
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

Every field without a future default mechanism must be initialized exactly
once. Unknown and duplicate fields are compile-time errors. Fields may appear
in any order.

Field access uses `value.field`. Assignment to a field requires a mutable root.
Nested mutation also requires every traversed composite field edge to have a
`mut` type, as specified in [Type System](04-type-system.md).
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

Unambiguous promoted methods may contribute to trait satisfaction. Ambiguous
promoted methods never satisfy a trait requirement automatically.

Embedded shorthand accepts a named data type with no generic arguments. Generic
and explicitly mutable embedded-field shorthand is not supported; an
ordinary named field expresses those relationships.

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
`JobStatus.Queued`; `.Queued` is also valid where a contextual expected type
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

enum HttpStatus(code: i32, phrase: string):
    Ok -> HttpStatus(200, phrase="OK")
    NotFound -> HttpStatus(404, phrase="Not Found")
```

Each variant with shared enum data must provide its enum constructor expression
after `->`. The constructor call follows ordinary positional/named argument
ordering and must initialize the declared shared data.

Shared constructor data is part of every enum value. A named constructor
parameter is available as a field on the enum value; an unnamed parameter uses
zero-based tuple-style numeric access:

```text
code := StatusCode.NotFound.0
phrase := HttpStatus.NotFound.phrase
```

Shared fields follow ordinary composite access permissions. Reading through a
const enum yields a const viewpoint; assignment requires a mutable enum root
and the required mutable edges. Variant payload fields remain available through
pattern matching rather than direct field access, because they do not exist on
every variant.

Within one variant, a named payload parameter must not duplicate a named shared
constructor parameter. Every shared constructor parameter must be initialized
by the variant result expression. Shared constructor data has no defaults.

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
not make ordinary enum variants directly importable.

Core optional patterns include `nil`, `_`, and a bare catch-all binding. A
dedicated present-value destructuring pattern is not part of the language; ordinary code
uses postfix `?` or optional library operations to extract the contained value.

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
language, user-visible finalizers, or weak references. Resource cleanup is
separate from memory reclamation and remains a runtime-design backlog item.

## Generalized Algebraic Data Types

Variants may declare explicit refined result types and variant-local generic
parameters. Matching such a variant refines the subject type within that arm as
specified in [Generalized Algebraic Data Types](13-gadts.md).

## Unsupported Aggregate Extensions

hd-lang has no data-field defaults, shared enum-constructor defaults, variant field
blocks, generic embedded-field shorthand, or explicitly mutable embedded-field
shorthand. Use explicit named fields when those relationships are required.
Stable object layout and component-model representation are ABI concerns and
are not observable core-language semantics.
