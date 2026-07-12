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

Open surface choices from this section:

1. Whether the standard output function should be `println`, `print`, or something else.
2. Whether the command should be `hd run`, `hd-lang run`, or another CLI shape.
3. Whether executable files should use top-level statements, an explicit `main`, or both.

## Values and Types

hd-lang is statically typed, but local code should stay light. There are two binding forms.

Use `:=` for short bindings. The type is inferred, and the reference cannot be reassigned:

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

Use `let` when the variable will be reassigned. Type annotation is optional:

```text
let attempts: i32 = 0
let inferred = 1

attempts = attempts + 1
inferred = inferred + 1
attempts = attempts + 1
```

Types appear where they make interfaces between code clear: function parameters, return types, struct fields, and public APIs.

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
let name: string = "Ada"
let initial: char = 'A'
```

There are no convenience aliases such as `int`, `uint`, or `float` in v1. Use explicit-width numeric types. `decimal` is a standard-library type, not a primitive.

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
| `(expr)`, literals, list/map/struct displays | atoms |
| `x.y`, `x[i]`, `x(args)` | field access, indexing, calls |
| postfix `?`, postfix `!` call marker | propagation, suspension call marker |
| `**` | exponentiation, right-associative |
| `-x`, `~x`, `not x` | unary operators |
| `*`, `/`, `%` | multiplicative |
| `+`, `-` | additive |
| `<<`, `>>` | shifts |
| `&` | bitwise and |
| `^` | bitwise xor |
| `|` | bitwise or |
| `==`, `!=`, `<`, `<=`, `>`, `>=` | comparisons; no chaining in v1 |
| `and` | logical and |
| `or` | logical or |
| `if`, `match`, `for ... else`, `while ... else` | value-producing control flow |
| `fn(...) -> ...:` | closure expression |
| `:=` | binding expression, lowest precedence |

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

Use structs instead of named tuples when field names are part of the meaning.

Optional values use Swift-style `?`. A plain `string` must contain a string. A `string?` may be `nil`.

```text
struct Profile:
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
long_names := [name for name in names if name.len() > 3]
name_lengths := [name.len() for name in names]
```

Map comprehensions build maps from key/value expressions:

```text
scores_by_name := {user.name: user.score for user in users}
active_by_id := {user.id: user for user in users if user.active}
```

The tour should prefer `:=` for ordinary local values, `let` only for variables that must change, and explicit types for boundaries that humans, tools, and AI agents need to review.

Open surface choices from this section:

1. Whether comprehensions should support multiple `for` clauses and `let` bindings.

## Control Flow and Expressions

hd-lang should make value-producing code easy to read. Literals, calls, field access, arithmetic, closures, `if`, `match`, and `:=` bindings are expressions. `let` declarations and assignment are statements.

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

Without an `else` block, a loop evaluates to `void`, even if it contains plain `break`.

`match` is also an expression. It should be exhaustive unless an explicit fallback arm is used:

```text
message := match status:
    JobStatus.Queued:
        "waiting"
    JobStatus.Running:
        "working"
    JobStatus.Succeeded:
        "done"
    JobStatus.Failed:
        "failed"
```

Open surface choices from this section:

1. Whether `break value` should be allowed only when the loop has an `else`, or whether it should be allowed in any loop and ignored for `void` loop contexts.
2. Exact fallback arm spelling for `match`, such as `_` or `else`.
3. Whether `else if` is one syntax form or parsed as nested `else` plus `if`.

## Structs

hd-lang has structs, not classes. Structs describe data with named, typed fields:

```text
struct User:
    id: string
    email: string
    display_name: string?
```

Construct a struct with a typed literal:

```text
user := User {
    id: "user_123",
    email: "ada@example.com",
    display_name: "Ada"
}
```

Field access uses dot syntax:

```text
println(user.email)
```

Structs can contain other structs:

```text
struct Profile:
    avatar_url: string?
    bio: string?

struct Account:
    user: User
    profile: Profile
```

Struct embedding supports Go-style composition without inheritance. A bare type-name line embeds that struct:

```text
struct Timestamps:
    created_at: i64
    updated_at: i64

struct Post:
    Timestamps
    id: string
    title: string
    author_id: string
```

Embedded structs are initialized with the embedded type name as the field key. Their fields are still promoted for ordinary access:

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

Embedded-field name conflicts follow Go-style promotion rules. A promoted field can be accessed directly only when it is unambiguous. If two embedded structs promote the same field name, direct access is ambiguous and the code must qualify through the embedded field:

```text
struct CreatedBySystem:
    id: string

struct CreatedByUser:
    id: string

struct AuditRecord:
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

Open surface choices from this section:

1. Whether struct fields are immutable by default, and what update syntax should look like.

## Enums

Structs model product types: one value contains all listed fields. Enums model sum types: one value is exactly one of several variants.

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

Payload variants use the same brace literal style as structs:

```text
error := ToolError.NotFound {
    resource: "user_123"
}
```

Use `match` to inspect an enum. Matches should be exhaustive unless an explicit fallback arm is present:

```text
fn status_label(status: JobStatus) -> string:
    match status:
        JobStatus.Queued:
            "queued"
        JobStatus.Running:
            "running"
        JobStatus.Succeeded:
            "succeeded"
        JobStatus.Failed:
            "failed"
```

Payload fields can be bound in a match arm:

```text
fn error_message(error: ToolError) -> string:
    match error:
        ToolError.NotFound { resource }:
            "not found: " + resource
        ToolError.Unauthorized { reason }:
            "unauthorized: " + reason
        ToolError.RateLimited { retry_after_ms }:
            "rate limited, retry after " + retry_after_ms.to_string() + "ms"
        ToolError.Internal { message }:
            message
```

Enum variants can also be used for domain states that should be impossible to confuse:

```text
enum PaymentState:
    Draft
    Authorized(id: string)
    Captured(id: string, amount: i64)
    Refunded(id: string, amount: i64)
```

Because the compiler knows every variant, it can check that callers handle every state. This matters for AI-generated code: missing cases should become compiler diagnostics instead of latent production behavior.

`Option` and `Result` are standard enum-like types, even though hd-lang gives them special syntax:

```text
let name: string? = nil                 # Option[string]
fn loaded() -> Result[User, DbError]:
    ...
```

`nil` is the empty optional case, and `?` propagates `nil` or `Result` errors from the current function.

Open surface choices from this section:

1. Whether payload variant declarations should use `Variant(field: type)` or an indented field block.
2. Whether match arms must always qualify variants with the enum name.
3. Exact fallback spelling, such as `_` or `else`.
4. Exact construction names for `Result`, such as `ok(value)` and `err(error)` versus `Result.Ok { value }`.

## Functions

Functions use `fn`, typed parameters, and an explicit return type:

```text
fn add(a: i32, b: i32) -> i32:
    a + b
```

The function body is an indented block. The last expression is the return value:

```text
fn display_name(user: User) -> string:
    if user.display_name == nil:
        user.email
    else:
        user.display_name!
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

Closures use anonymous `fn(...) -> ...` syntax. They are expressions, so they can be bound to names, passed to functions, or returned from functions:

```text
slugify := fn(text: string) -> string:
    text.trim().lower().replace(" ", "-")

slug := slugify("Hello hd-lang")
```

Closures capture values from the surrounding lexical scope:

```text
prefix := "user:"

label_user := fn(id: string) -> string:
    prefix + id

label := label_user("123")
```

When a closure is passed where a function type is already expected, parameter and return types can usually be inferred:

```text
fn map_names(names: list[string], f: fn(string) -> string) -> list[string]:
    ...

lower_names := map_names(names, fn(name):
    name.lower()
)
```

Generic functions put generic arguments after the function name:

```text
fn first[T](items: list[T]) -> T?:
    if items.len() == 0:
        nil
    else:
        items[0]
```

Functions cannot be overloaded. Each function name resolves to one declaration in a scope.

Functions are the primary unit of behavior. Methods, tools, workflows, tests, and handlers should attach to normal functions instead of requiring a separate object model.

Open surface choices from this section:

1. Whether default values must be compile-time constants.
2. Whether generic arguments are inferred at call sites, explicit at call sites, or both.
3. Whether hd-lang also needs a shorter one-line closure form for simple expressions.
4. Exact capture rules for mutable locals, serializable closures, and capability-carrying closures.

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

Implement a trait for a struct with an `impl` block:

```text
impl Display for User:
    fn display(self) -> string:
        self.email
```

Trait methods can be called with dot syntax:

```text
label := user.display()
```

Structs can also have inherent methods with `impl Struct`:

```text
impl User:
    fn domain(self) -> string:
        self.email.split("@")[1]

domain := user.domain()
```

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

Receivers are either `self` or `mut self`; there is no reference receiver spelling. Semantically, primitive types are passed by value and composite types are passed by reference. Structs, tuples, lists, and maps are composite types.

Structs do not own behavior in the class sense. Behavior lives in `impl Struct` blocks or trait implementations:

```text
struct Money:
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

Struct embedding interacts with traits through promoted methods in the same spirit as promoted fields: embedded methods can be called when unambiguous, and unambiguous promoted methods can satisfy trait requirements for the outer struct. Ambiguous promoted methods must be qualified through the embedded field or resolved with an explicit impl.

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

Open surface choices from this section:

1. Exact diagnostics when embedded methods conflict while checking trait satisfaction.

## Type System

hd-lang is statically typed. The compiler should know the type of every expression before code runs, but local code can rely on inference when the type is obvious:

```text
name := "Ada"          # inferred string
count := 3             # inferred i32
```

Public boundaries should stay explicit. Function parameters, return types, struct fields, enum payloads, and trait methods carry type annotations so humans and AI agents can review interfaces without chasing implementation details:

```text
fn find_user(id: UserId) -> Result[User?, DbError]:
    ...

struct User:
    id: UserId
    email: string
```

User-defined structs and enums are nominal types. Two types with the same fields are still different types:

```text
struct UserId:
    value: string

struct PostId:
    value: string

fn load_user(id: UserId) -> User?:
    ...

post_id := PostId { value: "post_123" }
load_user(post_id)        # invalid: PostId is not UserId
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

Use a nominal newtype when the compiler should prevent accidental mixing. Candidate spelling:

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

Range diagnostics should include the invalid literal, the target type range, and a repair hint:

```text
integer literal 300 does not fit in u8
valid range for u8 is 0..255
suggestion: use u16 if the value is intentional
```

Narrowing diagnostics should point to an explicit cast:

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

Variadic generics use type parameter packs. Minimal v1 supports packs only in function types, vararg parameters, and spread calls:

```text
fn call_with[Args..., R](f: fn(Args...) -> R, args: Args...) -> R:
    f(args...)
```

This lets the compiler preserve the exact argument types of higher-order functions instead of collapsing them into `list[any]` or a weak tuple type. v1 does not support pack mapping, filtering, splitting, or arithmetic.

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

Using a trait name as a value type creates a Go-style trait value: a pair of concrete value plus method table, dispatched at runtime. There is no `dyn` or `any` marker:

```text
fn print_display(value: Display) -> void:
    println(value.display())
```

There is no implicit nullability. `T` and `T?` are different types, and `nil` only belongs to optional values:

```text
let name: string = "Ada"
let nickname: string? = nil
```

Struct embedding is composition, not inheritance. It promotes fields and methods for convenience, but it does not make the outer struct a subtype of the embedded struct.

Embedded promoted methods count for trait satisfaction when they are unambiguous:

```text
struct Logger:
    name: string

impl Display for Logger:
    fn display(self) -> string:
        self.name

struct Service:
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

pub struct User:
    id: UserId
    email: string
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

Import and re-export cycles are rejected in v1.

v1 keeps visibility simple: declarations are module-private by default, and `pub` makes them public. There is no package-private visibility modifier.

## Effects

Effects make handler-mediated behavior visible in function signatures. A function that accesses a dependency, logs, calls a model, uses a capability, or can suspend into a handler should say so in its type.

This section is less settled than the previous ones. An effect needs to express two things: resolving or injecting a dependency/capability, and suspending execution flow into a handler. That intent should appear in the function body, not as `use` or `raise` keywords inside the signature.

Normal error handling uses `Result[T, E]`, not effects:

```text
struct ParseError:
    message: string

fn parse_user(input: string) -> Result[User, ParseError]:
    ...
```

Dependencies are ordinary traits or capabilities that can appear in the `$` requirement row. This makes dependency injection explicit without a separate dependency declaration syntax:

```text
trait Database:
    fn get_user!(id: UserId) -> Result[User?, DbError]

fn load_user!(id: UserId) -> Result[User?, DbError] $ Database:
    db := use(Database)
    user := db.get_user!(id)?
    user
```

Here `use(Database)` resolves the dependency from the current handler/context. The `!` on `db.get_user!(id)` marks the call as a suspension point where execution can enter the handler.

Handlers provide implementations for effects:

```text
handler mock_db for Database:
    fn get_user!(id: UserId) -> Result[User?, DbError]:
        ok(User {
            id: id,
            email: "test@example.com",
            display_name: "Test User"
        })
```

A call site can run code with a handler:

```text
with mock_db:
    user := load_user!(UserId { value: "user_123" })
```

Higher-order functions need effect polymorphism so callback effects are not erased:

```text
fn map[T, U, e](items: list[T], f: fn(T) -> U $ e) -> list[U] $ e:
    ...
```

Handlers can remove effects from an effect variable. This is useful when a function handles one callback effect but must still expose the remaining effects:

```text
fn handle_log[e](callback: fn(string) -> void $ e) -> void $ (e - log):
    with logger:
        callback("str")
```

Here `callback` may require `log` plus other effects. `handle_log` handles `log`, so callers only see the remaining effects. The helper itself is not named `handle_log!` unless its own body contains a suspending call such as `some_call!()`.

Open surface choices from this section:

1. Handler selection rules for tests, production, nested scopes, and defaults.
2. Whether effect polymorphism uses full row polymorphism or a smaller effect-variable model.
3. Exact `Result[T, E]` ergonomics beyond `?` propagation.
