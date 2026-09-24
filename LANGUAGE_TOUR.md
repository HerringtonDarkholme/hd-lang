# hd-lang in Y Minutes

This is a draft language tour. Syntax examples are intended to be concrete enough to discuss, while still allowing us to revise unsettled choices as the design sharpens.

## Hello hd-lang

An hd-lang script can be written as top-level code:

```text
# hello.hd

println("hello, hd-lang")
```

Run it as a script:

```sh
hd run hello.hd
```

The language uses indentation for structure, so blocks are introduced by a header ending in `:` followed by either an indented body or a same-line body:

```text
fn greet(name: string) -> void:
    println("hello, " + name)

fn test() -> void: println("hi")

greet("Ada")
```

Comments use `#`:

```text
# This is a line comment.
println("comments should feel familiar to Python users")
```

Top-level statements make hd-lang useful as an interactive scripting language for AI agents. The same file can later grow into typed functions, tool definitions, tests, and deployable workflows without switching to a different language model.

Scripts execute top-level statements. Executable packages use the explicit `main`
entry point described later in the tour. The exact CLI and standard-output API
spellings are tooling decisions rather than language syntax.

## Values and Types

hd-lang is statically typed, but local code should stay light. There are two binding forms.

Use `:=` for short bindings. The type is inferred, and the binding cannot be reassigned:

```text
name := "Ada"
age := 36
active := true

println(name)
```

`:=` is also an expression. It evaluates to the value being bound, and the inferred immutable name is available in the nearest enclosing block:

```text
if (trimmed := input.trim()) != "":
    println(trimmed)
```

Use `let` for a local variable that may be reassigned. Type annotation is optional:

```text
let display_name: string = "Ada"
let nickname: string? = nil
let inferred = 1
let attempts: i32 = 0
let counter = 1

attempts = attempts + 1
counter = counter + 1
attempts = attempts + 1
```

For composite values, mutation permission is part of the type. `T` provides const access and `mut T` provides mutable access. Local declarations place `mut` in the type rather than before the binding name. A const composite reference cannot be upgraded to a mutable one:

```text
user := User {
    id: "user_123",
    email: "ada@example.com",
    display_name: "Ada"
}

let alias: mut User = user  # error: User cannot become mut User
```

A mutable reference may be viewed as const, and that const alias can observe later changes made through an existing mutable alias:

```text
let user: mut User = User {
    id: "user_123",
    email: "ada@example.com",
    display_name: "Ada"
}

readonly := user
user.display_name = "Ada Lovelace"

println(readonly.display_name)  # "Ada Lovelace"
```

This is shared reference permission, not ownership or deep immutability. Multiple mutable aliases may exist, but mutation authority cannot be created from a const reference.

An unannotated `let` infers the initializer's access type. Fresh composite construction may infer `mut T`, but assigning an existing `T` never upgrades it.

Types appear where they make interfaces between code clear: function parameters, return types, data fields, and public APIs.

```text
fn greeting(name: string) -> string:
    "hello, " + name
```

Primitive types include booleans, width-explicit numbers, strings, and chars:

```text
let ok: bool = true
let small: i8 = 1
let count: i32 = 42
let large: i64 = 9000
let octet: u8 = 255
let size: u64 = 1024
let ratio: f32 = 0.5
let precise: f64 = 0.5
large_float := 1e9
small_float := 1.5e-6
let name: string = "Ada"
let initial: char = 'A'
binary := 0b1010
mode := 0o755
color := 0xFF8800
million := 1_000_000
mask := 0x_FF_FF_00
```

Python-style raw strings preserve backslashes and escape-looking text. Their
multiline form uses three double quotes:

```text
pattern := r"\d+\s+\w+"
prompt := r"""Summarize the input.
Return one paragraph."""
```

Regular triple-quoted strings also span lines and continue interpreting normal
escapes. Multiline strings preserve their source indentation and line breaks:

```text
message := """Hello,\n
This remains on the next source line."""
```

Ordinary strings use Kotlin-style interpolation. A simple name follows `$`
directly; other expressions use `${...}`. Interpolated values must implement
`std.format.Display`:

```text
import std.format.Display

greeting := "Hello, $name"
summary := "User ${user.name} has ${posts.len()} posts"
price := "Cost: \$5"
literal := r"$name and ${user.name} are not expanded here"
```

Use `\$` for a literal dollar sign in an interpreted string. Raw strings treat
every dollar sign literally.

There are no convenience aliases such as `int`, `uint`, or `float`. Use explicit-width numeric types. `decimal` is a standard-library type, not a primitive.

Text literals distinguish chars and strings:

```text
letter := 'A'          # char
name := "Ada"          # string
```

Numeric values support ordinary arithmetic operators:

```text
sum := a + b
diff := a - b
product := a * b
quotient := a / b
remainder := a % b
power := a ** b
negative := -value
```

Integer `/` truncates toward zero. Integer overflow is always checked unless code uses explicit wrapping APIs. Shift counts can use any integer type, but must be non-negative and in range at runtime unless the compiler can prove that statically.

Integer values also support bitwise operators:

```text
masked := flags & mask
combined := read | write
toggled := flags ^ debug
inverted := ~mask
left := value << 2
right := value >> 1
```

Operator precedence follows a Python-like shape, from highest to lowest:

| Operators | Notes |
| --- | --- |
| `(expr)`, literals, list/map/data displays | atoms |
| `x.y`, `x[i]`, `x(args)`, `x!(args)` | field access, indexing, ordinary calls, suspension calls |
| postfix `?` | optional or error propagation |
| `**` | exponentiation, right-associative |
| `-x`, `~x`, `not x` | unary operators |
| `*`, `/`, `%` | multiplicative |
| `+`, `-` | additive |
| `<<`, `>>` | shifts |
| `&` | bitwise and |
| `^` | bitwise xor |
| `|` | bitwise or |
| `==`, `!=`, `<`, `<=`, `>`, `>=`, `is` | comparisons; no chaining |
| `and` | logical and |
| `or` | logical or |
| `if`, `match`, `for ... else`, `while ... else` | value-producing control flow |
| `fn(...) -> ...:` | closure expression |
| `:=` | binding expression, lowest precedence |

`==` uses `PartialEq`, while ordering uses `PartialOrd`; `Eq` and `Ord` express
the stronger total-equality and total-order contracts. Data and enums do not
gain equality automatically: implement the trait or request explicit
derivation. `is` checks whether two composite references point to the same
object, independently of their values. `not (a is b)` checks distinct identity.

```text
@derive(PartialEq, Eq, PartialOrd, Ord, Hash)
data User:
    id: i64
    name: string
```

`@derive` is a compiler intrinsic: it generates ordinary trait
implementations from the data or enum shape. Derived equality compares every
declared field, including embedded fields; enum equality also distinguishes
variants. It does not detect reference cycles, so comparison may exhaust the
stack when it repeatedly traverses one. It is not an annotation facet.
Derived ordering compares data fields in declaration order. Enum variants
compare by declaration order before their shared data and payload fields.
Derived `Hash` hashes every declared data field, or the enum variant identity
followed by its shared data and payload fields. Each such field needs `Hash`;
`Eq + Hash` lets a user-defined type serve as a map key.

Use parentheses when a binding expression appears inside a larger expression.

`void` is used for functions that return no useful value:

```text
fn log_start() -> void:
    println("start")
```

Tuples are built in for lightweight grouped values:

```text
point := (10, 20)                 # (i32, i32)
entry := ("Ada", 36, true)        # (string, i32, bool)
single := (1,)                    # (i32,)
empty := ()                       # empty tuple
```

Tuple fields use Rust-style numeric field access:

```text
x := point.0
y := point.1
```

Tuple destructuring works with both binding forms:

```text
x, y := point
let name, score = ("Ada", 10)
```

Use data types instead of named tuples when field names are part of the meaning.

Optional values use Swift-style `?`. A plain `string` must contain a string. A `string?` may be `nil`.

```text
data Profile:
    display_name: string
    nickname: string?
```

Optional values must be handled before they are used as non-optional values. Like Rust, `?` propagates absence from a function that also returns an optional:

```text
fn label(name: string?) -> string?:
    actual := name?
    "user: " + actual
```

`?` also propagates `Result` errors from a function that returns a compatible `Result[T, E]`.

Collections are typed:

```text
names := ["Ada", "Grace", "Linus"]       # list[string]
scores := {"Ada": 10, "Grace": 12}       # map[string, i32]
```

List comprehensions build lists from iterables:

```text
long_names := [for name in names if name.len() > 3 => name]
name_lengths := [for name in names => name.len()]
pairs := [for x in xs for y in ys => (x, y)]
labels := [for user in users if (label := user.name.trim().lower()) != "" => label]
```

Comprehensions put generator and filter clauses before `=>`, and the produced value after `=>`. Multiple `for` clauses run left to right. Use the `:=` binding expression to name intermediate values inside guards or result expressions; there is no separate `let` clause in comprehensions.

A later clause can use names introduced by earlier clauses. An `if` filters at the point where it appears:

```text
early_filter := [for x in xs if x.active for item in x.items => item]
pair_filter := [for x in xs for y in ys if x.id == y.owner_id => (x, y)]
```

The first form filters `x` before entering the inner `item` loop. The second form filters after both `x` and `y` exist. Later clauses are not visible to earlier clauses.

Map comprehensions build maps from key/value expressions:

```text
scores_by_name := {for user in users => user.name: user.score}
active_by_id := {for user in users if user.active => user.id: user}
```

If a map comprehension produces the same key more than once, the later value wins.

Comprehensions cannot contain suspension points. Use an explicit loop when the body needs a `!` call.

Use `:=` for ordinary const local values and `let` for variables that may be reassigned. Composite mutation permission is written in the type as `mut T`; explicit types remain important at boundaries that humans, tools, and AI agents need to review.

## Control Flow and Expressions

hd-lang makes value-producing code easy to read. Literals, calls, field access, arithmetic, closures, `if`, `match`, and `:=` bindings are expressions. `let` declarations and assignment are statements.

Block headers such as `fn`, `if`, `else if`, `else`, `for`, `while`, and `match` end with `:`. Blocks can evaluate to their last expression. This is why functions can return without an explicit `return`:

```text
fn score_label(score: i32) -> string:
    if score >= 90:
        "excellent"
    else if score >= 70:
        "passing"
    else:
        "needs work"
```

When `if` is used as an expression, every branch must produce the same type, and an `else` branch is required:

```text
label := if active:
    "active"
else:
    "inactive"
```

`else if` is one direct conditional-chain form, not a nested `else:` block containing a separate `if`.

Use `return` for early exits:

```text
fn find_name(names: list[string], prefix: string) -> string?:
    for name in names:
        if name.starts_with(prefix):
            return name
    nil
```

Loops can be used for control flow. `break` exits a loop, and `continue` skips to the next iteration:

```text
let total: i32 = 0

for value in values:
    if value < 0:
        continue
    if value > 100:
        break
    total = total + value
```

`for` separates an ordinary iterable source from the iterator that tracks one traversal. An ordinary `Iterable` contains no per-traversal cursor or progress. Each call to its `iter()` creates an independent mutable `Iterator`, whose `next()` operation advances only that iterator. This is why nested traversal over the same source has independent progress:

```text
for left in values:
    for right in values:
        println((left, right))
```

The iterable may still contain data and may itself refer to mutable data; "stateless" here means only that traversal progress is not stored in an ordinary iterable source. Every mutable `Iterator` also implements `Iterable` by returning itself from `iter()`. This does not clone or reset it: iteration continues from the cursor's current position and leaves it exhausted when completed. A const iterator cannot advance. Built-in list and map iterators are invalidated by insertion, removal, clearing, or another shape change, and their next advance panics. Replacing an existing element or value without changing collection shape does not invalidate the iterator.

Use `while` when the loop condition is not just iterating a collection:

```text
let index: i32 = 0

while index < names.len():
    println(names[index])
    index = index + 1
```

`for` and `while` can also have `else` blocks. When a loop has an `else`, the loop becomes a value-producing expression: `break value` produces the loop value when the loop exits early, and `else value` produces the loop value when the loop finishes normally.

```text
first_large := for value in values:
    if value > 100:
        break value
else:
    nil
```

The same rule applies to `while`:

```text
found := while index < names.len():
    if names[index].starts_with(prefix):
        break names[index]
    index = index + 1
else:
    nil
```

Without an `else` block, a loop evaluates to `void`, even if it contains plain `break`. `break value` is only valid in a value-producing loop with an `else`; use plain `break` in statement-only loops.

`match` is also an expression. It is exhaustive unless an explicit fallback arm is used:

```text
message := match status:
    JobStatus.Queued => "waiting"
    JobStatus.Running => "working"
    JobStatus.Succeeded => "done"
    JobStatus.Failed => "failed"
    _ => "unknown"
```

`pass` is the no-op expression and evaluates to `void`. It is useful when syntax requires a body but no operation is needed. In `annotate Validation for User: pass`, it means default derivation with no overrides.

## Data Types

hd-lang has data types, not classes. The `data` declaration defines a nominal
product type with named, typed fields. Its composite values use shared references,
not value-type copies:

```text
data User:
    id: string
    email: string
    display_name: string? = nil
```

Construct a data value with a typed literal:

```text
user := User {
    id: "user_123",
    email: "ada@example.com"
}
```

Omitted fields use their declared defaults. Defaults must be pure and are
evaluated for each construction after explicit field expressions; a
copy-update spread supplies every field and skips defaults.

Field access uses dot syntax:

```text
println(user.email)
```

Data types can also be matched by field. Unlisted fields are ignored;
`field=pattern` can rename a binding or test a nested value:

```text
fn email_of(user: User) -> string:
    match user:
        User { email=address } => address
```

Composite fields may store either const or mutable references. Mutation through a path requires a mutable root and `mut` on every composite reference edge crossed by that path:

```text
data Profile:
    display_name: string

data Account:
    profile: mut Profile

let profile: mut Profile = Profile {
    display_name: "Ada"
}

account := Account { profile: profile }
account.profile.display_name = "Ada Lovelace"  # error: const root

let editable: mut Account = Account { profile: profile }
editable.profile.display_name = "Ada Lovelace"  # mutable root + mutable edge
```

An ordinary composite field is a const edge. A `mut` outer object may replace
any field, even a const-typed one, but cannot mutate a referenced child through
a const edge. A readonly outer view cannot replace fields. It reads a direct
`friend: mut User` field as `User`, so it cannot mutate that child or call its
`mut self` methods. A readonly outer `User` may be constructed with a readonly
value in that direct field; constructing `mut User` requires a mutable value.
A generic field `value: T` instead retains its substituted type, including
`mut Child` when `T = mut Child`. Other mutable aliases may still change the
underlying object.
`mut` belongs in a named field's type (`friend: mut User`), never before its
name (`mut friend: User`). Embedded members also cannot carry `mut`: use
`Base`, not `mut Base`; use a named field if a mutable edge is needed.

Mutable parameter permission is written in the type position:

```text
fn inspect(user: User) -> string:
    user.display_name

fn invalid(user: User) -> void:
    user.display_name = "new"  # error: User is const

fn normalize_user(user: mut User) -> User:
    user.email = user.email.trim().lower()
    user
```

The same rule composes through containers:

```text
fn edit_users(users: mut list[mut User]) -> void:
    users[0].display_name = "new"
```

Here the list is a mutable root and its element references are mutable edges.
`mut list[User]` can replace list elements but cannot mutate the referenced
users; `list[mut User]` cannot replace list elements but can mutate its
referenced users, because indexing retains the generic element type.

Container and element permissions are independent, so all four forms are meaningful: `list[User]`, `list[mut User]`, `mut list[User]`, and `mut list[mut User]`.

A read-only list view may weaken element permission because `list` declares its element parameter as covariant, conceptually `list[+T]`: `list[mut User]` can be used as `list[User]`. Mutable list views are invariant, so `mut list[mut User]` cannot become `mut list[User]`; that mutable view could insert a const `User` into storage requiring `mut User`.

Maps follow the same separation: a readonly `map[K, mut User]` can yield
`mut User` from lookup or iteration, but replacing an entry requires a
`mut map[K, mut User]`. Lookup returns `mut User?`; matching the present case
or propagating `?` yields `mut User`.
The same generic-content rule applies to `Result[mut User, E]`: propagating a
successful result with `?` yields `mut User`, not a weakened reference.

Use copy-update syntax when creating a modified value from an existing data value:

```text
renamed := User {
    ...user,
    display_name: "Ada Lovelace"
}
```

Copy-update is shallow. It creates a new outer data value, copies primitive fields
by value, and reuses composite field references unless an explicit replacement
provides a different value. Copied fields are read through the source view:
a readonly source exposes a direct `mut`-typed child as readonly. It can fill
that field in a readonly copy; a mutable copy requires an explicit mutable
replacement. Generic fields retain their substituted types in either view.

Data types can contain other data types:

```text
data Profile:
    avatar_url: string?
    bio: string?

data Account:
    user: User
    profile: Profile
```

Data embedding supports Go-style composition without inheritance. A bare type-name line embeds that data:

```text
data Timestamps:
    created_at: i64
    updated_at: i64

data Post:
    Timestamps
    id: string
    title: string
    author_id: string
```

Embedded data types are initialized with the embedded type name as the field key. Their fields are still promoted for ordinary access:

```text
post := Post {
    Timestamps: Timestamps {
        created_at: 1700000000,
        updated_at: 1700000000
    },
    id: "post_123",
    title: "Hello",
    author_id: "user_123"
}

println(post.created_at)
```

Generic data types can be embedded with type arguments. The construction key
is the type's final name, without arguments:

```text
data Box[T]:
    value: T

data Shipment[T]:
    Box[T]
    id: string

shipment := Shipment[i32] {
    Box: Box[i32] { value: 5 },
    id: "shipment_1",
}
shipment.value
```

Two embedded types with the same final name are rejected, even when their
type arguments differ.

Embedded-field name conflicts follow Go-style promotion rules. A promoted field can be accessed directly only when it is unambiguous. If two embedded data types promote the same field name, direct access is ambiguous and the code must qualify through the embedded field:

```text
data CreatedBySystem:
    id: string

data CreatedByUser:
    id: string

data AuditRecord:
    CreatedBySystem
    CreatedByUser

record := AuditRecord {
    CreatedBySystem: CreatedBySystem {
        id: "system"
    },
    CreatedByUser: CreatedByUser {
        id: "user_123"
    }
}

record.id                    # invalid: ambiguous promoted field
record.CreatedByUser.id      # ok
```

## Enums

Data types model product types: one value contains all listed fields. Enums model sum types: one value is exactly one of several variants.

Use `enum` for closed sets of variants:

```text
enum JobStatus:
    Queued
    Running
    Succeeded
    Failed
```

Construct a variant with the enum name and variant name:

```text
status := JobStatus.Queued
```

Variants can carry named payload fields:

```text
enum ToolError:
    NotFound(resource: string)
    Unauthorized(reason: string)
    RateLimited(retry_after_ms: i32)
    Internal(message: string)
```

Payload variant declarations use the compact `Variant(field: type)` form. Use a separate data type when the payload is large enough to need a full field block.

Payload variants are called like functions. Positional arguments come first, and
named arguments follow them:

```text
error := ToolError.NotFound(resource="user_123")
```

Use `match` to inspect an enum. Matches are exhaustive unless an explicit fallback arm is present:

```text
fn status_label(status: JobStatus) -> string:
    match status:
        JobStatus.Queued => "queued"
        JobStatus.Running => "running"
        JobStatus.Succeeded => "succeeded"
        JobStatus.Failed => "failed"
```

An arm may use `if` to guard a pattern. Pattern bindings are visible in the guard;
when it is false, matching continues. Guarded arms do not count toward
exhaustiveness, so a later arm must cover the remaining values:

```text
fn queue_label(status: JobStatus, urgent: bool) -> string:
    match status:
        JobStatus.Queued if urgent => "urgent"
        JobStatus.Queued => "queued"
        JobStatus.Running => "running"
        JobStatus.Succeeded => "succeeded"
        JobStatus.Failed => "failed"
```

Enum variants can use `.Variant` where the enum type is known from context, including a typed binding, a return type, or a `match` subject. Without that context, use the qualified form such as `JobStatus.Queued`; the compiler does not guess an enum from a variant name. Enum variants cannot be imported directly.

Payload fields can be bound in a match arm:

```text
fn error_message(error: ToolError) -> string:
    match error:
        ToolError.NotFound(resource) => "not found: " + resource
        ToolError.Unauthorized(reason) => "unauthorized: " + reason
        ToolError.RateLimited(retry_after_ms) => "rate limited, retry after " + retry_after_ms.to_string() + "ms"
        ToolError.Internal(message) => message
```

Enum variants can also be used for domain states that should be impossible to confuse:

```text
enum PaymentState:
    Draft
    Authorized(id: string)
    Captured(id: string, amount: i64)
    Refunded(id: string, amount: i64)
```

An enum can declare constructor parameters for data shared by every variant. Variants can call the enum constructor in their result expression:

```text
enum StatusCode(i32):
    Ok -> StatusCode(200)
    NotFound -> StatusCode(404)
    InternalError -> StatusCode(500)
```

Enum constructor definitions and calls follow the same parameter conventions as functions. In definitions, unnamed positional parameters come before named parameters. In calls, positional arguments come first, and named arguments must come after positional arguments.

```text
enum HttpStatus(code: i32, phrase: string, retryable: bool = false):
    Ok -> HttpStatus(200, phrase="OK")
    NotFound -> HttpStatus(404, phrase="Not Found")
    ServiceUnavailable -> HttpStatus(503, phrase="Service Unavailable", retryable=true)
```

Shared enum constructor parameters may have pure defaults. Defaults follow
function-parameter ordering and run when omitted for each construction, after
explicit arguments. Variant payload parameters remain required.

Shared named constructor data is available on every enum value as a field, such
as `HttpStatus.NotFound.phrase`. Unnamed shared data uses a zero-based numeric
member, such as `StatusCode.NotFound.0`. Variant-specific payloads remain
available through pattern matching.

Enums can be generic algebraic data types:

```text
enum Maybe[T]:
    Some(value: T)
    None
```

Enums can also be recursive:

```text
enum Tree[T]:
    Leaf(value: T)
    Branch(left: Tree[T], right: Tree[T])
```

For more precise modeling, a GADT variant can declare an explicit result type so pattern matching can recover more specific type information:

```text
enum Expr[T]:
    IntLit(value: i64) -> Expr[i64]
    BoolLit(value: bool) -> Expr[bool]
    Add(left: Expr[i64], right: Expr[i64]) -> Expr[i64]
    Sub(left: Expr[i64], right: Expr[i64]) -> Expr[i64]
    Scale(value: Expr[i64], factor: i64) -> Expr[i64]
    If[T](cond: Expr[bool], then_value: Expr[T], else_value: Expr[T]) -> Expr[T]
```

A GADT-style variant can also refine the enum type while passing data to an enum-level constructor:

```text
enum Box[T](contents: T):
    IntBox(n: i64) -> Box[i64](n)
    BoolBox(b: bool) -> Box[bool](b)
```

When a variant omits an explicit result type, it returns the enclosing enum with the enum's type arguments. When matching a GADT-style enum, the matched variant refines the enum type parameter inside that arm:

```text
fn eval[T](expr: Expr[T]) -> T:
    match expr:
        Expr.IntLit(value) => value
        Expr.BoolLit(value) => value
        Expr.Add(left, right) => eval(left) + eval(right)
        Expr.Sub(left, right) => eval(left) - eval(right)
        Expr.Scale(value, factor) => eval(value) * factor
        Expr.If(cond, then_value, else_value) =>
            if eval(cond):
                eval(then_value)
            else:
                eval(else_value)
```

In the `Expr.IntLit` arm, `T` is known to be `i64`, so returning `value: i64` is valid. In the `Expr.BoolLit` arm, `T` is known to be `bool`. The compiler uses those refinements for arm-local type checking and still checks that the whole `match` returns the function's declared `T`.

Enum payload patterns follow the same positional/named convention as function calls and enum constructor calls. Positional patterns come first. Only `field=pattern` counts as a named pattern, and named patterns come after positional patterns. Bare identifiers bind new names, while literals match exact values:

```text
fn eval_i64(expr: Expr[i64]) -> i64:
    match expr:
        Expr.Add(l, r) => eval_i64(l) + eval_i64(r)
        Expr.Sub(left=l, right=r) => eval_i64(l) - eval_i64(r)
        Expr.Scale(value, factor=2) => eval_i64(value) * 2
        Expr.IntLit(value) => value
```

In `Expr.Add(l, r)`, `l` and `r` are positional patterns that bind new names; they do not need to match the payload field names `left` and `right`. In `Expr.Sub(left=l, right=r)`, `left=` and `right=` select payload fields by name, while `l` and `r` are still new binding patterns. `Expr.Scale(value, factor=2)` matches only a scale expression whose `factor` payload equals `2`. A positional pattern cannot appear after a named pattern. If the payload itself is an expression, match the nested variant explicitly, such as `right=Expr.IntLit(2)`.

Because the compiler knows every variant, it checks that callers handle every state. This matters for AI-generated code: missing cases become compiler diagnostics instead of latent production behavior.

`Option` and `Result` are standard enum-like types, even though hd-lang gives them special syntax:

```text
let name: string? = nil                 # Option[string]
fn loaded() -> Result[User, DbError]:
    ...
```

`nil` is the empty optional case, and `?` propagates `nil` or `Result` errors from the current function.
A plain `User` can be assigned to `User?` without writing a wrapper. In a
`match`, `user?` matches a present optional and binds `user` as `User`; `nil`
matches absence. A bare `user` pattern would bind the entire optional.

Construct `Result` values with capitalized helper constructors:

```text
return Ok(user)
return Err(db_error)
```

## Functions

Functions use `fn`, typed parameters, and an explicit return type:

```text
fn add(a: i32, b: i32) -> i32:
    a + b
```

The function body is an indented block. The last expression is the return value:

```text
fn normalized_email(user: User) -> string:
    user.email.trim().lower()
```

Explicit `return` exists for early exits:

```text
fn first_name(name: string) -> string:
    if name == "":
        return "anonymous"
    name.split(" ")[0]
```

Use `void` when a function does not return a useful value:

```text
fn print_user(user: User) -> void:
    println(user.email)
```

Functions are ordinary values, so small scripts can grow into reusable tools without switching styles:

```text
fn normalize_email(email: string) -> string:
    email.trim().lower()

clean := normalize_email("Ada@Example.COM ")
```

Parameter names are immutable bindings. A `mut T` parameter permits mutation
through the reference but cannot itself be rebound. Use a `let` local when the
algorithm needs reassignment:

```text
fn normalize(value: i32) -> i32:
    let current = value
    current = current.abs()
    current
```

Calls can mix positional and named arguments. Positional arguments must come first; no positional argument can appear after a named argument:

```text
fn resize(width: i32, height: i32) -> (i32, i32):
    (width, height)

size := resize(640, height=480)
bad := resize(width=640, 480)        # invalid: positional after named
```

Parameters can have default values:

```text
fn connect(host: string, port: i32 = 443, tls: bool = true) -> Connection:
    ...

secure := connect(host="api.example.com")
local := connect("localhost", port=8080, tls=false)
```

Default values can use pure expressions and pure function calls. They cannot require context requirements or suspension:

```text
fn default_port() -> i32:
    443

fn connect(host: string, port: i32 = default_port()) -> Connection:
    ...

fn bad_connect(host: string, token: string = read_secret!("TOKEN")) -> Connection:
    ...      # invalid: default value suspends and requires a capability
```

Use varargs when a function accepts zero or more positional arguments of the same type:

```text
fn sum(values: i32...) -> i32:
    let total: i32 = 0
    for value in values:
        total = total + value
    total

sum(1, 2, 3)

nums := [1, 2, 3]
sum(nums...)
```

Varargs must be the final positional parameter. `nums...` spreads a list into positional arguments at the call site, and spread syntax is positional only. If a vararg is passed by name, it accepts a list:

```text
fn tagged_sum(tag: string, values: i32...) -> i32:
    ...

tagged_sum("score", 1, 2, 3)
tagged_sum("score", nums...)
tagged_sum(tag="score", values=nums)
tagged_sum(tag="score", nums...)      # invalid: positional spread after named arg
```

Function types use `fn(...) -> ...`:

```text
fn apply(value: string, transform: fn(string) -> string) -> string:
    transform(value)
```

When a function's final parameter is a zero-argument callback, an indented trailing block can supply it without writing `fn()` or its return type:

```text
result := when(a, b):
    compute_result()

transaction:
    save_user()
    write_audit_log()
```

Ordinary arguments stay inside `()`. Calls with no ordinary arguments omit empty parentheses. The callback's return type and behavior are checked against the final parameter type. Callbacks with parameters continue to use explicit `fn(...)` syntax; trailing-block sugar is zero-argument only.

`return` inside a trailing block exits the generated callback, not the enclosing function:

```text
fn load(cached: User, use_cache: bool) -> User:
    user := transaction:
        if use_cache:
            return cached
        fetch_user()

    audit(user)
    user
```

Here `return cached` supplies the callback's result, after which `load` continues with `audit(user)`. Trailing blocks do not support non-local return.

Closures use anonymous `fn(...) -> ...` syntax. They are expressions, so they can be bound to names, passed to functions, or returned from functions:

```text
slugify := fn(text: string) -> string:
    text.trim().lower().replace(" ", "-")

slug := slugify("Hello hd-lang")
```

There is no separate short closure syntax. Use `fn(...) -> ...:` for closures. Same-line closure bodies are allowed when the body is a single expression:

```text
inc := fn(x: i32) -> i32: x + 1
add := fn(x: i32, y: i32) -> i32: x + y
make_id := fn() -> string: "id_123"
```

Inline closures may omit parameter and return types when the surrounding call provides an expected function type. Parenthesize each closure expression to pass multiple multiline callbacks inline:

```text
fn choice(
    first: fn(i32) -> void,
    second: fn(string) -> void,
) -> void:
    first(1)
    second("two")

choice(
    (
        fn(aa):
            println(aa)
    ),
    (
        fn(bb):
            println(bb)
    ),
)
```

The expected types infer `aa: i32`, `bb: string`, and a `void` return from both callbacks. A standalone nonrecursive closure needs explicit parameter types but may infer its return type from its body.

A directly bound local closure may call itself when it writes an explicit
return type. Parameter types can still come from an expected function type:

```text
let sum: fn(i32) -> i32 = fn(n) -> i32:
    if n == 0: 0
    else: n + sum(n - 1)
```

`sum := fn(n: i32): ...` cannot recurse because its return type is inferred.

Closures capture values from the surrounding lexical scope:

```text
prefix := "user:"

label_user := fn(id: string) -> string:
    prefix + id

label := label_user("123")
```

Closures that mutate captured locals have a mutable function type, written `mut fn(...) -> ...`. Calling a mutable closure requires the closure value itself to be mutable:

```text
let count: i32 = 0

let next: mut fn() -> i32 = mut fn() -> i32:
    count = count + 1
    count

next()
```

Plain `fn(...) -> T` closures cannot mutate captured locals. Use `mut fn(...) -> T` when mutation is part of the callable's behavior:

```text
fn repeat(times: i32, f: mut fn() -> void) -> void:
    let i: i32 = 0
    while i < times:
        f()
        i = i + 1
```

Closures that capture dependencies or capabilities carry those requirements in their function type. Serializable closures are a separate deferred design area; the language has not yet defined their capture, identity, compatibility, or execution semantics.

When a closure is passed where a function type is already expected, parameter and return types can usually be inferred:

```text
fn map_names(names: list[string], f: fn(string) -> string) -> list[string]:
    ...

lower_names := map_names(names, fn(name):
    name.lower()
)
```

The same call can use a same-line closure:

```text
lower_names := map_names(names, fn(name): name.lower())
```

Shorthand argument closures such as `$0 + $1` are not supported; closures use named parameters.

Generic functions put generic arguments after the function name:

```text
fn first[T](items: list[T]) -> T?:
    if items.len() == 0:
        nil
    else:
        items[0]
```

Generic arguments are inferred at call sites when the type is unambiguous. Callers can also provide the full generic argument list explicitly:

```text
names := ["Ada", "Grace"]

a := first(names)          # T inferred as string
b := first[string](names)  # explicit generic argument
```

An explicit generic argument list must be complete. A call cannot provide only
a left-to-right prefix and infer the remaining generic arguments. Use `_` to
infer an individual slot while preserving the complete list:

```text
fn convert[From, To](value: From) -> To:
    ...

user := convert[_, User](payload)
```

Every `_` must be determined by the call arguments, expected result type, or
generic constraints. It is not a type and cannot be used in `list[_]`.

Function generic parameters are erased at runtime by default. Mark a parameter `reified` when the function needs its concrete runtime type:

```text
fn runtime_shape[reified T]() -> TypeShape:
    shape(T)
```

`reified` is written on the generic parameter, not on the function. The compiler passes a hidden runtime type descriptor at each call:

```text
string_shape := runtime_shape[string]()
```

An erased generic parameter cannot be used where runtime type information is required:

```text
fn invalid_shape[T]() -> TypeShape:
    shape(T)  # compile error: T is erased
```

Reification propagates through generic calls. A function passing its own type parameter to a reified parameter must also declare that parameter as reified:

```text
fn resolve[reified T]() -> T $ TypeProvider:
    ...

fn resolved[reified T]() -> T $ TypeProvider:
    resolve[T]()

fn invalid_resolved[T]() -> T $ TypeProvider:
    resolve[T]()  # compile error: resolve requires runtime type information for T
```

Unlike Kotlin's JVM implementation, hd-lang does not require a reified function to be `inline`. Backends may specialize calls and remove descriptors when doing so cannot change observable reflection behavior.

hd-lang does not support partial explicit generic argument lists. An explicit
list remains complete even when one or more slots use `_` for inference.

Functions cannot be overloaded. Each function name resolves to one declaration in a scope.

Functions are the primary unit of behavior. Methods, tools, workflows, tests, and generated adapters attach to normal functions instead of requiring a separate object model.

## Traits and Methods

Traits describe shared behavior without classes or inheritance:

```text
trait Display:
    fn display(self) -> string
```

Traits can provide default implementations:

```text
trait Named:
    fn name(self) -> string

    fn label(self) -> string:
        "name: " + self.name()
```

Implement a trait for a data type with an `impl` block:

```text
impl Display for User:
    fn display(self) -> string:
        self.email
```

Trait methods can be called with dot syntax:

```text
label := user.display()
```

When two implemented traits leave a dot call ambiguous, qualify the trait explicitly with `Trait::method(receiver, ...)`, such as `Display::display(user)`. Qualification selects that trait implementation directly; it does not perform inherent or embedded-method lookup.

Data types can also have inherent methods with `impl TypeName`:

```text
impl User:
    fn domain(self) -> string:
        self.email.split("@")[1]

    fn tagged[T](self, value: T) -> T:
        value

domain := user.domain()
label := user.tagged[string]("admin")
```

Generic methods infer all arguments when brackets are omitted. Their explicit
lists must be complete and may use `_` in individual inferred slots, just like
module functions.

Traits can require multiple methods:

```text
trait Repository[T]:
    fn get!(self, id: string) -> T? $ Database
    fn save!(self, value: T) -> void $ Database
```

Functions can use trait constraints on generic parameters:

```text
fn show[T: Display](value: T) -> string:
    value.display()
```

Trait bounds compose like Rust:

```text
fn audit_label[T: Display + Named](value: T) -> string:
    value.display() + " / " + value.name()
```

Receivers are either `self` or `mut self`; there is no reference receiver spelling. Primitive parameters are passed by value. Composite parameters use reference permissions in the type position: `value: T` is const and `value: mut T` is mutable. `mut self` is receiver shorthand for `self: mut Self`. Data types, tuples, lists, and maps are composite types.

Data types do not own behavior in the class sense. Behavior lives in inherent
`impl TypeName` blocks or trait implementations:

```text
data Money:
    amount: i64
    currency: string

trait Add[T]:
    fn add(self, other: T) -> T

impl Add[Money] for Money:
    fn add(self, other: Money) -> Money:
        Money {
            amount: self.amount + other.amount,
            currency: self.currency
        }
```

Trait implementation is explicit. A type does not implement a trait just because it has matching methods.

Data embedding interacts with traits through promoted methods in the same spirit as promoted fields: embedded methods can be called when unambiguous, and unambiguous promoted methods can satisfy trait requirements for the outer data. Ambiguous promoted methods must be qualified through the embedded field or resolved with an explicit impl.

When promoted methods conflict during trait checking, diagnostics should show the missing trait requirement, list the ambiguous promoted methods, and suggest an explicit impl:

```text
C does not satisfy Display

required method:
  fn display(self) -> string

ambiguous promoted methods:
  A.display(self) -> string
  B.display(self) -> string

fix:
  implement Display for C explicitly
  or call the embedded method through c.A.display() / c.B.display()
```

Dynamic dispatch follows the Go interface style: use the trait name as a value type, and method calls through that trait value dispatch to the concrete implementation at runtime.

```text
fn print_display(value: Display) -> void:
    println(value.display())

print_display(user)
```

This is different from generic static dispatch, where the compiler specializes the function for a concrete type:

```text
fn show_static[T: Display](value: T) -> string:
    value.display()
```

## Type System

hd-lang is statically typed. The compiler knows the type of every expression before code runs, but local code can rely on inference when the type is obvious:

```text
name := "Ada"          # inferred string
count := 3             # inferred i32
```

Public boundaries stay explicit. Function parameters, return types, data fields, enum payloads, and trait methods carry type annotations so humans and AI agents can review interfaces without chasing implementation details:

```text
fn find_user(id: UserId) -> Result[User?, DbError]:
    ...

data User:
    id: UserId
    email: string
```

User-defined data types and enums are nominal types. Two types with the same fields are still different types:

```text
data UserRecord:
    value: string

data PostRecord:
    value: string

fn load_user(record: UserRecord) -> User?:
    ...

post := PostRecord { value: "post_123" }
load_user(post)        # invalid: PostRecord is not UserRecord
```

Tuples are structural. A value of type `(i32, i32)` is compatible with another `(i32, i32)` because tuple identity comes from its element types:

```text
let point: (i32, i32) = (10, 20)
let offset: (i32, i32) = point
```

Type aliases are transparent by default:

```text
type UserName = string

fn greet(name: UserName) -> void:
    println("hello, " + name)

let raw: string = "Ada"
greet(raw)              # ok: UserName is an alias for string
```

Use a nominal newtype when the compiler must prevent accidental mixing:

```text
type Mile(i32)
type Kilometer(i32)

fn drive(distance: Mile) -> void:
    ...

miles := Mile(10)
km := Kilometer(16)

drive(miles)            # ok
drive(km)               # invalid: Kilometer is not Mile
drive(10)               # invalid: i32 is not Mile

raw_distance := i32(miles)     # explicit cast back to base type
```

Lower-precision integers can widen into higher-precision integers without an explicit cast. Narrowing requires an explicit cast:

```text
let small: i16 = 42
let large: i64 = small          # ok: widening

let huge: i64 = 9000
let smaller: i16 = huge         # invalid: narrowing
smaller_ok := i16(huge)
```

Integer literals use a default concrete type when there is no expected type. The default integer type is always `i32`. When there is an expected numeric type, the literal is checked against that type's range:

```text
x := 1                 # i32 by default
let small_ok: i8 = 1   # ok: 1 fits in i8
let bad: u8 = 300      # invalid: 300 is out of range for u8
```

Range diagnostics include the invalid literal, the target type range, and a repair hint:

```text
integer literal 300 does not fit in u8
valid range for u8 is 0..255
suggestion: use u16 if the value is intentional
```

Narrowing diagnostics point to an explicit cast:

```text
cannot assign i64 to i16 without an explicit cast
suggestion: use i16(huge) if range checking is intended
```

Generic types and functions use square brackets:

```text
let names: list[string] = ["Ada", "Grace"]
let scores: map[string, i32] = {"Ada": 10, "Grace": 12}

fn first[T](items: list[T]) -> T?:
    if items.len() == 0:
        nil
    else:
        items[0]
```

Generic type declarations use `+T` for covariance, `-T` for contravariance, and unmarked `T` for invariance:

```text
data Producer[+T]:
    produce: fn() -> T

data Consumer[-T]:
    consume: fn(T) -> void

data Cell[T]:
    value: T
```

Variance conversions apply to read-only outer views. A `mut Producer[T]`, `mut Consumer[T]`, or other mutable generic view remains invariant because its fields can be replaced.

Function generic parameters are erased by default. Use `reified` only when runtime behavior needs the concrete type, such as shape inspection, annotation lookup, serialization, or type-directed dependency injection:

```text
fn resolve[reified T]() -> T $ TypeProvider:
    ...

items := resolve[list[i32]]()
```

At the language level, a reified call behaves as if it passes a hidden `Type[T]` descriptor. This descriptor is not an ordinary source-level argument and cannot be supplied with a named argument. Reification is part of a function's public type and ABI.

Variadic generics use ordered type and value packs. Pack expansion is supported in function types, vararg parameters, tuple types, call arguments, and type or expression patterns:

```text
fn call_with[Args..., R](f: fn(Args...) -> R, args: Args...) -> R:
    f(args...)
```

This lets the compiler preserve the exact argument types of higher-order functions instead of collapsing them into `list[Any]` or a weak tuple type.

A pattern containing a pack can be expanded once per pack element. This is especially useful for a heterogeneous concurrency combinator:

```text
fn all![Ts...](tasks: mut Suspend[Ts]...) -> (Ts...):
    ...
```

For `Ts... = User, i32, bool`, `mut Suspend[Ts]...` expands to three parameter types, `mut Suspend[User], mut Suspend[i32], mut Suspend[bool]`, while `(Ts...)` becomes the result tuple `(User, i32, bool)`. Expression patterns can expand in argument-list positions too: `start(tasks)...` repeats `start(task)` for every value in the `tasks` pack. Pattern expansion happens at compile time and does not allocate a runtime collection.

Multiple packs in one repeated pattern expand positionally in lockstep and must have equal lengths. A library `all!` driver can use `pack.map((tasks...), make_slot)` to turn the heterogeneous task pack into a tuple of typed slots, `pack.map_list(slots, poll_slot, context)` to gather homogeneous readiness flags, and `pack.map(slots, take_ready)` to recover the result tuple. Each mapper is a named generic function instantiated for each tuple element; mapping is compiler-supported, while scheduling and cancellation remain library behavior. Filtering, indexing, splitting, and pack arithmetic remain unsupported.

Traits describe behavior, but trait implementation is explicit. A type does not satisfy a trait just because it has matching methods:

```text
trait Display:
    fn display(self) -> string

impl Display for User:
    fn display(self) -> string:
        self.email
```

Generic trait bounds use static dispatch:

```text
fn label[T: Display](value: T) -> string:
    value.display()
```

Using a trait name as a value type creates a Go-style trait value: a pair of concrete value plus method table, dispatched at runtime. There is no `dyn` marker:

```text
fn print_display(value: Display) -> void:
    println(value.display())
```

`Any` is the built-in universal empty trait, analogous to Go's `any`. Every non-optional value type satisfies it automatically. Use `Any` for an erased dynamic value and `T: Any` when generic code must preserve the concrete type:

```text
fn keep_erased(value: Any) -> Any:
    value

fn preserve[T: Any](value: T) -> T:
    value
```

Mutable bounds combine access permission with trait conformance:

```text
trait Clear:
    fn clear(mut self) -> void

fn clear_value[T: mut Clear](value: T) -> void:
    value.clear()

fn accept_mutable[T: mut Any](value: T) -> void:
    keep_erased(value)
```

`mut Trait` is likewise a mutable dynamic trait view. `mut Any` preserves mutable access to an erased composite value, but provides no type-specific operation by itself. `mut list[User]` satisfies `mut Any`; `list[mut User]` does not, because its root is const.

There is no implicit nullability. `T` and `T?` are different types, and `nil` only belongs to optional values:

```text
let name: string = "Ada"
let nickname: string? = nil
let value: Any = nil         # invalid
let maybe_value: Any? = nil
```

Likewise, an optional `T?` can erase to `Any?`, but not to `Any`.

Data embedding is composition, not inheritance. It promotes fields and methods for convenience, but it does not make the outer data a subtype of the embedded data.

Embedded promoted methods count for trait satisfaction when they are unambiguous:

```text
data Logger:
    name: string

impl Display for Logger:
    fn display(self) -> string:
        self.name

data Service:
    Logger

fn show(value: Display) -> void:
    println(value.display())

service := Service {
    Logger: Logger { name: "api" }
}

show(service)       # ok: Service satisfies Display through Logger
```

If multiple embedded fields promote conflicting methods, the outer type does not satisfy the trait automatically. The user must qualify calls or write an explicit impl to resolve the conflict.

## Modules, Packages, and Imports

Modules are inferred from file paths under the package source root. There is no required `module` or `package` declaration:

```text
src/user/types.hd      # module user.types
src/user/service.hd    # module user.service
src/post/service.hd    # module post.service
```

Module path components follow the same NFC Unicode identifier rules as source names. Module identities are case-sensitive, but a package is rejected when two paths collide after Unicode case folding and NFC normalization.

Packages use `hd.toml`. The default source root is `src`:

```toml
[package]
name = "my_app"
version = "0.1.0"

[source]
root = "src"

[dependencies]
billing = "1.2.0"
```

Directories define submodule namespaces only when they contain a `mod.hd` file. `mod.hd` is required for every directory module and acts as the public index:

```text
src/user/mod.hd        # module user
src/user/types.hd      # module user.types
src/user/service.hd    # module user.service
```

Export public API with `pub`:

```text
# src/user/types.hd

pub type UserId(string)

pub data User:
    pub id: UserId
    pub email: string
```

Import selected names with grouped imports:

```text
# src/post/service.hd

import pkg.user.types.{User, UserId}

fn author_id(user: User) -> UserId:
    user.id
```

Import the module namespace when qualification is clearer:

```text
import pkg.user.types

fn author_id(user: pkg.user.types.User) -> pkg.user.types.UserId:
    user.id
```

Use aliases for long paths or name conflicts:

```text
import pkg.user.types as user_types
import dep.billing.types.{UserId as BillingUserId}
import std.time.{Duration}
```

`pkg` means the current package root. `std` means the standard library. `dep.<name>` means an external dependency from `hd.toml`.

Use `self` and `super` for relative imports:

```text
# src/user/service.hd

import self.types.{User, UserId}
import super.shared.{Email}
```

Use `mod.hd` to define the directory module and re-export a clean package-facing API:

```text
# src/user/mod.hd

export pkg.user.types.{User, UserId}
export pkg.user.service.{load_user, save_user}
```

Submodules are not imported automatically. Parent modules and child modules both use explicit imports.

Import and re-export cycles are rejected.

Declarations are module-private by default, and `pub` makes them public. Enum variants inherit the enum's visibility. Data fields and inherent methods remain private unless individually marked `pub`, even on a public data. A public signature cannot leak a module-private type. There is no package-private visibility modifier.

## Program Entry Points

`pub fn main` is the conventional default entry point for an executable package. It takes no source-level parameters. Process arguments, environment, console I/O, and every other host service are explicit context requirements:

```text
import std.host.{Args, Console, ConsoleError}

pub fn main!() -> Result[void, ConsoleError] $ Args + Console:
    args, console := $.use(Args, Console)
    console.write_line!("starting " + args.program_name())
```

The ordinary function rules still apply. Use the `!` suffix only when `main` can suspend. A non-suspending entry point is named `main`. It may return `void` or `Result[void, E]`; the generated host adapter maps an `Err` to a failed invocation.

`pub` controls hd-lang module visibility, not Wasm export visibility. Other public functions are not automatically exported from the compiled component. Tools, workflows, and library-facing Wasm functions become host-visible only through explicit registration, which generates the required boundary adapter. The exact registration API is designed separately for each integration.

Registered Wasm boundaries accept only recursively boundary-safe structural values. The initial boundary-safe forms are primitive scalars, `string`, tuples, `list[T]`, `map[K, V]`, data types, enums, `T?`, and `Result[T, E]`, provided every contained type is also boundary-safe:

```text
pub data LookupRequest:
    pub ids: list[UserId]
    pub filters: map[string, string]

pub enum LookupError:
    InvalidId(id: string)
    Unavailable(message: string)

fn lookup_users!(request: LookupRequest) -> Result[list[User], LookupError] $ Database:
    ...
```

Mutable types, trait values, closures, and live runtime handles cannot appear anywhere in an exported parameter or result. Context requirements such as `Database` are host bindings and do not cross as serialized function arguments. Export registration checks the complete signature and generates the boundary conversion.

Maps are unordered by default. Their insertion or iteration order is not part of the value or boundary semantics, and boundary consumers must not infer meaning from the order used by a particular encoding.

## Requirements and Suspension

Requirements, provider contexts, and suspension are language features specified
separately from ordinary functions and `Result` error handling.

hd-lang separates three concerns often grouped under algebraic effects:

1. `$` rows statically check which dependencies a function requires.
2. Context scopes inject concrete providers for those requirements.
3. `fn!`, `Suspend[T]`, and bang calls provide one-shot suspension.

These mechanisms cooperate, but none implies the others. Dependency lookup does
not suspend, suspension does not represent normal errors, and an ordinary
dependency does not have to be a host capability. Normal error handling uses
`Result[T, E]`:

```text
data ParseError:
    message: string

fn parse_user(input: string) -> Result[User, ParseError]:
    ...
```

Dependencies are ordinary traits or capabilities that can appear in the `$` requirement row. This makes dependency injection explicit without a separate dependency declaration syntax:

```text
trait Database:
    fn get_user!(self, id: UserId) -> Result[User?, DbError]

trait Cache:
    fn get_user(self, id: UserId) -> User?

fn load_user!(id: UserId) -> Result[User?, DbError] $ Database + Cache:
    db, cache := $.use(Database, Cache)
    cached := cache.get_user(id)
    if cached != nil:
        return Ok(cached)
    db.get_user!(id)
```

A suspending declaration also creates a cold computation constructor:

```text
let pending: mut Suspend[Result[User?, DbError]] = load_user(id)
result := load_user!(id)   # Result[User?, DbError]; drive and suspend if necessary
```

`fn load_user!(...) -> T` lowers conceptually to a function constructing `mut Suspend[T]`. Arguments are evaluated when the cold suspension is constructed, while the body is compiled into a resumable state machine and begins only when driven. `:=` weakens a stored suspension to readonly `Suspend[T]`; use `let pending: mut Suspend[T]` when it must later be polled or cancelled. Each nested bang call is a possible suspension point: the compiler saves the enclosing state, drives the child computation, and resumes with its result.

`Suspend[T]` is a single-execution, pollable state machine. Its driver polls for `Pending` or `Ready(T)` and uses a waker to arrange further progress. Exclusive driving is enforced at runtime: competing drivers, reentrant polling, and driving after completion or cancellation panic. Repeated polling while pending is normal; executing again requires constructing a new suspension.

The caller must satisfy the function's dependency requirements when constructing the suspension. The selected providers are captured then, even though the body has not started, and are not replaced by a later driving context. Cancellation is synchronous and cleanup cannot suspend. Source-level cleanup remains [backlog work](DESIGN_QUESTIONS.md#deferred-resource-cleanup-and-scope-exit). Stored-suspension driving syntax remains open, and a separate `Task[T]` API is deferred.

Here `$.use(Database, Cache)` retrieves multiple providers from the current context in order. The `!` on `db.get_user!(id)` marks a possible suspension point. It does not mean that the call raises an error or performs dependency lookup.

Providers are ordinary values whose types implement the required trait:

```text
data MockDatabase:
    user: User

impl Database for MockDatabase:
    fn get_user!(self, id: UserId) -> Result[User?, DbError]:
        Ok(self.user)

mock_db := MockDatabase {
    user: User {
        id: UserId("user_123"),
        email: "test@example.com",
        display_name: "Test User"
    }
}
```

Call sites provide requirements through context scopes. `$.with(Requirement=provider)` binds a requirement key to a provider value for the indented body:

```text
$.with(Database=mock_db, Cache=memory_cache):
    result := load_user!(UserId("user_123"))
```

Reusable contexts are provider-map values typed by a requirement row:

```text
fn prod_context() -> $.Context[Metrics + Cache]:
    $.context(Metrics=metrics, Cache=cache)

$.with(Database=mock_db, Logger=console_logger, ...prod_context()):
    db, logger, cache := $.use(Database, Logger, Cache)
    result := load_user!(UserId("user_123"))
```

`$.Context[Metrics + Cache]` is not a variadic generic. The `Metrics + Cache` part is an unordered requirement row, using the same composition shape as function `$` requirements. `$.context(Metrics=metrics, Cache=cache)` creates a reusable context value, `...prod_context()` spreads reusable providers into a lexical context scope, and `$.use(Database, Logger, Cache)` retrieves providers in the requested return order. Requirement names in `$.context`, `$.with`, and `$.use` are requirement keys, usually trait or capability names, not ordinary named-argument labels. Duplicate providers for the same requirement cannot coexist; during context construction or spread, later bindings win and the resulting context has one entry per key. If a required provider does not exist for a call, that is a compile-time error. The `$` namespace is special context syntax, not an ordinary value namespace.

Requirement polymorphism for higher-order functions preserves callback
requirements rather than erasing them:

```text
fn map[T, U, r](items: list[T], f: fn(T) -> U $ r) -> list[U] $ r:
    ...
```

A provider scope removes a locally supplied requirement from a row variable:

```text
fn provide_logger[r](callback: fn(string) -> void $ r) -> void $ (r - Logger):
    $.with(Logger=logger):
        callback("str")
```

Here `callback` may require `Logger` plus other requirements. The local provider
satisfies `Logger`, so callers see only the remaining row. The helper itself is
not named `provide_logger!` because its body has no suspension point.

Providers come from an enclosing `$.with` scope or an entry point's host
configuration; there are no implicit provider defaults. Requirement variables
are specialized row parameters with union and subtraction. Additional
`Result[T, E]` convenience APIs belong to the standard library.

## Using Annotations

Annotations provide typed metadata and structural derivation. Shape APIs,
materialization, exact-target derivation, and recursive references are part of
the language. An unoverridden field uses its type's facet annotation; an exact
field result override can supply the facet result instead. Missing information
is a compile-time error, never an implicit omission.

Annotations attach typed metadata to declaration shapes and derive typed information for complete targets. They do not change a declaration's type, behavior, name, or visibility.

Prefix decorators put a no-override facet next to a declaration and member
metadata next to a field, variant, or function parameter:

```text
@Validation
data User:
    @max_len(80)
    display_name: string

@Tool
fn get_user(
    @description("User identifier")
    id: UserId,
) -> User:
    ...
```

Here `@Validation` expands to `annotate Validation for User: pass`, then to
`impl Annotate[Validation] for User`. The field decorator expands to
`annotate User: display_name = [max_len(80)]` and attaches metadata to its
field shape. An enum can use `@Validation` and decorators on its variants. A
function can use a facet decorator such as `@Tool`; its parameter decorators
expand to metadata in `annotate get_user` and are attached to `ParamShape`
before `map_param`. These are compile-time checked attachments, not runtime
wrappers.

Decorators and `annotate` blocks are module-level. Local declarations cannot
carry annotation facets or member metadata.

Declaration facets may be configured with ordinary values:

```text
@tool(strict=true)
fn search(query: string) -> list[Result]:
    ...
```

If `tool(strict=true)` returns `Tool`, the compiler uses `Tool` as the facet
type and retains that value as `self` for `map_param` and `build`. The
configuration runs once during restricted annotation initialization. It does
not create a second annotation namespace: `@Tool` and `@tool(...)` occupy the
same `(Tool, search)` coherence slot.

Use `annotate Target` to attach metadata to existing members:

```text
data User:
    display_name: string
    active: bool

annotate User:
    display_name = [min_len(1), max_len(80)]
```

For `display_name: string`, the assignment is contextually typed as `list[FieldMetadata[string]]`. `MinLen` and `MaxLen` are different concrete values implementing the same dynamic trait, so the collection is homogeneous.

Metadata values and reusable metadata lists are ordinary values:

```text
let display_name_metadata: list[FieldMetadata[string]] = [
    min_len(1),
    max_len(80),
]

data User:
    display_name: string

annotate User:
    display_name = display_name_metadata
```

Metadata values execute in a restricted metadata phase. They must be pure, deterministic, non-suspending, and dependency-free. Multiple entries with the same concrete metadata type on one member are rejected.

Use `annotate Annotation for Target` to derive information for a complete target. `pass` requests default derivation with no structural result overrides:

```text
annotate Validation for User: pass
```

Metadata is composed from the bottom up. For `User`, hd-lang first resolves validation information for each field's declared type, reads the field's `FieldMetadata[string]` values, and then uses `DataAnnotator` to build validation information for the complete data.

Generated metadata is retrieved explicitly as an ordinary runtime value:

```text
user_validator := Validation::annotation(User)

input := read_json()
result := user_validator.parse(input)
```

The same declaration can provide unrelated annotation information:

```text
data User:
    id: UserId
    display_name: string
    avatar: string?

annotate Validation for User: pass
annotate DatabaseSchema for User: pass
annotate UI for User: pass
```

Each facet is retrieved independently:

```text
validator := Validation::annotation(User)
table := DatabaseSchema::annotation(User)
form := UI::annotation(User)
```

Add structural result overrides when default derivation is insufficient:

```text
annotate UI for User:
    avatar = profile_image(size=40)
```

Assignments in an `annotate` block can override existing fields or variants only; they cannot invent members that are absent from the target declaration. A package can have at most one block for an exact facet/target pair.

Function annotations follow the same model:

```text
fn get_user!(id: UserId) -> Result[User?, ToolError] $ Database:
    db := $.use(Database)
    db.get_user!(id)

annotate Tool for get_user: pass
```

This produces tool information but does not discover or register the function, and it does not make the function public. Registration is explicit:

```text
tool_registry.register(Tool::annotation(get_user))
```

`Facet::annotation(Target)` is the runtime retrieval spelling. A local `build`
inside the facet block replaces aggregate assembly. A member type without the
requested facet needs an exact field or variant result override; otherwise
derivation is a compile-time error. Decorator syntax is an optional locality
form for the corresponding `annotate` blocks.

## Implementing Annotators

Annotators are ordinary types that transform declaration shapes into typed metadata. The compiler exposes shapes for the declarations an annotator can inspect:

```text
shape(User)          # DataShape
shape(User.email)    # FieldShape
shape(JobStatus)     # EnumShape
shape(get_user)      # FnShape
```

`FnShape` includes ordered parameter shapes, the result type, default presence,
the suspension marker, and the normalized unordered requirement row. Parameter
shapes do not have local metadata assignments.

Every annotation value implements `Annotation` and chooses one uniform information type. `Annotate[A]` records that a concrete target provides information for annotation `A`:

```text
trait Annotation:
    type Info

trait Annotate[A: Annotation]:
    fn info() -> A::Info
```

Local member metadata uses open traits. A field metadata trait is generic over the field's declared type:

```text
trait FieldMetadata[T]
trait VariantMetadata
trait ParamMetadata[T]
```

For example, `MaxLen` applies to `string` fields but not `i32` fields:

```text
data MaxLen:
    value: i32

fn max_len(value: i32) -> MaxLen:
    MaxLen { value: value }

impl FieldMetadata[string] for MaxLen
```

The compiler accepts this metadata based on ordinary trait checking:

```text
data User:
    display_name: string

annotate User:
    display_name = [max_len(80)]
```

It rejects the same value on an incompatible field because `MaxLen` does not implement `FieldMetadata[i32]`:

```text
data Invalid:
    retry_count: i32

annotate Invalid:
    retry_count = [max_len(80)]  # compile error
```

An annotation maps complete types to uniform information. This small validation annotation uses one recursive `Validator` type for primitives and data types:

```text
type Dict[K, V] = map[K, V]

data Validation: pass

data FieldValidator:
    name: string
    target: AnnotationRef[Validator]
    max_len: i32?

enum Validator:
    I32
    String
    Data(name: string, fields: Dict[string, FieldValidator])

impl Annotation for Validation:
    type Info = Validator
```

Exact `annotate` blocks extend the facet for individual types:

```text
annotate Validation for i32:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.I32

annotate Validation for string:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.String
```

Generic target families use the same binders and bounds as generic
implementations:

```text
annotate[T: Annotate[Validation]] Validation for list[T]:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.List(Validation::annotation_ref(T))
```

This occupies the same coherence region as the corresponding generic
`impl Annotate[Validation] for list[T]`. There is no unconstrained wildcard
`annotate Validation for type` fallback.

Ordinary decorators expand to `annotate` blocks, which lower to `Annotate[Facet]` implementations or shape metadata. `annotate Validation for T` generates the same conformance as `impl Annotate[Validation] for T`. The `annotate` form additionally understands the target's structure so it can express field or variant overrides. Both forms occupy the same trait-coherence slot. `@derive` is the compiler-intrinsic exception.

A local `build` in `annotate Facet for Target` replaces only aggregate assembly;
child resolution and member mapping still happen first. To replace the entire
derivation pipeline, implement the conformance directly:

```text
impl Annotate[Validation] for User:
    fn info() -> Validator:
        Validator.Custom(...)
```

The direct implementation and structural `annotate` form cannot coexist for
the same facet and target.

An annotator for a data type maps each field and then builds one result for the complete type:

```text
trait DataAnnotator: Annotation:
    type FieldTarget

    fn map_field(
        self,
        field: FieldShape,
        type_metadata: AnnotationRef[Self::Info],
    ) -> Self::FieldTarget

    fn build(
        self,
        shape: DataShape,
        fields: Dict[string, Self::FieldTarget],
    ) -> Self::Info
```

`Validation` combines each field's already-derived type validator with metadata attached directly to that field:

```text
impl DataAnnotator for Validation:
    type FieldTarget = FieldValidator

    fn map_field(
        self,
        field: FieldShape,
        type_metadata: AnnotationRef[Validator],
    ) -> FieldValidator:
        FieldValidator {
            name: field.name,
            target: type_metadata,
            max_len: field.metadata(MaxLen).map(
                fn(annotation: MaxLen) -> i32: annotation.value
            ),
        }

    fn build(
        self,
        shape: DataShape,
        fields: Dict[string, FieldValidator],
    ) -> Validator:
        Validator.Data(name=shape.name, fields=fields)
```

The compiler supplies `type_metadata`; `map_field` does not restart annotation resolution. This enforces the bottom-up order:

```text
type metadata -> field metadata -> data metadata
```

`AnnotationRef[T]` is provided by the annotation runtime. It can hold an already-built target or a deferred reference to one, allowing the same annotator to support recursive data types and enums without adding a facet-specific `Ref` variant.

Enums and functions follow the same mapping-then-building pattern:

```text
trait EnumAnnotator: Annotation:
    type FieldTarget
    type VariantTarget

    fn map_field(
        self,
        field: FieldShape,
        type_metadata: AnnotationRef[Self::Info],
    ) -> Self::FieldTarget

    fn map_variant(
        self,
        variant: VariantShape,
        fields: list[Self::FieldTarget],
    ) -> Self::VariantTarget

    fn build(
        self,
        shape: EnumShape,
        variants: Dict[string, Self::VariantTarget],
    ) -> Self::Info

trait FuncAnnotator: Annotation:
    type ParamTarget

    fn map_param(self, param: ParamShape) -> Self::ParamTarget

    fn build(
        self,
        shape: FnShape,
        params: Dict[string, Self::ParamTarget],
    ) -> Self::Info
```

`VariantMetadata` provides the corresponding homogeneous dynamic-trait
collection for variants. A parameter of type `T` uses
`list[ParamMetadata[T]]`; function annotators can read it together with each
`ParamShape`'s name, type, default presence, and documentation. This metadata
customizes `map_param` but does not directly replace its `ParamTarget` result.
`DataAnnotator`, `EnumAnnotator`, and `FuncAnnotator` remain responsible for
aggregate mapping and building.

Shape values and annotator methods follow the definitions in the annotation
chapter. Generic families may use generic `annotate` declarations or ordinary
generic `impl Annotate[A] for Target`; both occupy the same coherence region. Type and aggregate
information share `Annotation::Info`. A child with no facet implementation
requires an exact field or variant result override; otherwise derivation fails
at compile time.

## Runtime and Library Features

Testing, sandbox enforcement, persistence, replay, and observability build on the language features introduced above but are primarily standard-library, tooling, or runtime concerns. They are documented separately in [Runtime and Library Design](RUNTIME_AND_LIBRARY.md).
