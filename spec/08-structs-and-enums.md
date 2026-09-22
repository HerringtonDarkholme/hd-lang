# Structs and Enums

Status: core specification draft.

Structs are nominal product types. Enums are nominal sum types. Neither is a
class, and neither creates an inheritance hierarchy.

## Struct Declarations

A struct declares named, typed fields:

```text
struct User:
    id: string
    email: string
    display_name: string?
```

Field names must be unique within the struct. Every field has an explicit type.
Field-level visibility is not supported in v1; visibility applies to the struct
declaration as a whole.

Struct identity is nominal. Two declarations with the same fields introduce
different types.

Struct declarations may be directly or mutually recursive because composite
fields use managed references. Recursion does not imply optionality: a program
must still provide a value for every required field during construction, so a
recursive graph normally includes an optional, enum, list, or another finite
base case.

## Construction And Access

Struct values use typed brace literals:

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

Copy-update construction uses one leading spread:

```text
renamed := User {
    ...user,
    display_name: "Ada Lovelace",
}
```

The spread has the same struct type and supplies every field. Explicit fields
replace copied values. This creates a struct value; it does not mutate the
spread source.

## Struct Embedding

A bare type-name member embeds another struct:

```text
struct Post:
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
the outer struct a subtype of the embedded type. Promotion follows Go-style
shortest-path resolution. A direct member hides promoted members. Multiple
equally short promoted members are ambiguous and require qualification through
the embedded field.

Unambiguous promoted methods may contribute to trait satisfaction. Ambiguous
promoted methods never satisfy a trait requirement automatically.

The current core embeds a named struct type with no generic arguments. Generic
and explicitly mutable embedded-field shorthand is not supported in v1; an
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

Variant names must be unique within the enum and are accessed through the enum
name, such as `JobStatus.Queued`.

A variant may carry payload parameters:

```text
enum ToolError:
    NotFound(resource: string)
    Unauthorized(reason: string)
    RateLimited(retry_after_ms: i32)
```

Payload parameters follow function definition conventions: unnamed positional
parameters first, followed by named parameters. Each payload type is explicit.
Large payloads should use a separate struct rather than a nested field block;
variant field blocks are not part of v1.

Variant construction follows function-call conventions. Positional arguments
come before named arguments:

```text
error := ToolError.NotFound(resource="user_123")
```

A payload-bearing variant constructor is not itself a first-class function
value in v1 and must be called. Use an explicit closure to pass construction as
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
by the variant result expression. Shared constructor data has no defaults in
v1.

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

Without a provisional GADT result clause, every variant constructs the
enclosing enum instantiated with the declaration's type arguments.

## Matching Enums

Enum values are inspected with exhaustive `match`. Variant patterns are always
qualified:

```text
match error:
    ToolError.NotFound(resource) => resource
    ToolError.Unauthorized(reason) => reason
    ToolError.RateLimited(retry_after_ms) => retry_after_ms.to_string()
```

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
`Err(pattern)` are the corresponding unqualified built-in variant spellings and
are the exception to the ordinary enum-qualification rule.

Core optional patterns include `nil`, `_`, and a bare catch-all binding. A
dedicated present-value destructuring pattern is not part of v1; ordinary code
uses postfix `?` or optional library operations to extract the contained value.

Whether these are literally user-definable standard-library enums or compiler
intrinsics with equivalent semantics is an ABI decision, not a source-language
difference.

## Representation And Garbage Collection

Struct and enum representation is chosen by the Wasm GC backend subject to
observable language semantics. Programs must not depend on field offsets,
variant tags, object addresses, or representation identity unless a future
interop facility exposes them explicitly.

Reachable composite values are garbage collected, and unreachable reference
cycles are reclaimable. The language does not expose manual deallocation in the
current core, user-visible finalizers, or weak references. Resource cleanup is
separate from memory reclamation and remains a runtime-design backlog item.

## Provisional GADTs

Variants with explicit refined result types, variant-local generic parameters,
and pattern-driven type refinement are specified only in
[Generalized Algebraic Data Types](provisional/gadts.md).

## Unsupported Aggregate Extensions

v1 has no struct-field defaults, shared enum-constructor defaults, variant field
blocks, generic embedded-field shorthand, or explicitly mutable embedded-field
shorthand. Use explicit named fields when those relationships are required.
Stable object layout and component-model representation are ABI concerns and
are not observable core-language semantics.
