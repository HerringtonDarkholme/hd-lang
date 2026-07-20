# Syntax Notes

## Current Direction

The language should read like Python, but behave more like a compact Rust/Go-inspired static language.

Block-introducing headers end with `:`. The body can either follow on the same line for simple definitions, or continue as an indented block:

```text
fn test() -> void: println("hi")

fn greet(name: string) -> void:
    println("hello, " + name)
```

Design priorities:

1. Expose intent clearly for AI-generated code.
2. Keep common code short.
3. Make effects, contracts, schemas, and generated tests visible enough for tooling.
4. Avoid Python's dynamic object model.

## Bindings

There should be only two local binding forms:

1. `:=` for short bindings.
2. `let`, with optional `mut`, for named local declarations.

`:=` introduces an inferred, non-reassignable reference:

```text
name := "Ada"
age := 36
```

`:=` is also an expression. It evaluates to the value being bound, and the inferred immutable name is available in the nearest enclosing block:

```text
if (trimmed := input.trim()) != "":
    println(trimmed)
```

The binding cannot be assigned again:

```text
name := "Ada"
name = "Grace"   # invalid: `name` was introduced with `:=`
```

Plain `let` introduces an immutable local when an explicit declaration or type annotation is useful:

```text
let display_name: string = "Ada"
let nickname: string? = nil
let inferred = 1
```

`let mut` introduces a variable that can be reassigned or mutated in place. Type annotation is optional:

```text
let mut attempts: i32 = 0
let mut counter = 1

attempts = attempts + 1
counter = counter + 1
```

Reasoning:

1. Most local code stays concise with `:=`.
2. Mutation is review-relevant, so it should be visually explicit with `mut`.
3. Type annotations remain available for important mutable state, but inference is allowed when the type is obvious.

## Type System Direction

The type system should be static, with local inference and explicit public boundaries. The compiler should know the type of every expression before code runs, but ordinary local code can avoid redundant annotations.

Function parameters, return types, struct fields, enum payloads, and trait methods should carry type annotations:

```text
fn find_user(id: UserId) -> Result[User?, DbError]:
    ...

struct User:
    id: UserId
    email: string
```

Structs and enums are nominal types. Matching field shape is not enough for assignment or calls:

```text
struct UserId:
    value: string

struct PostId:
    value: string

fn load_user(id: UserId) -> User?:
    ...

post_id := PostId { value: "post_123" }
load_user(post_id)        # invalid
```

Tuples are structural:

```text
let point: (i32, i32) = (10, 20)
let offset: (i32, i32) = point
```

Transparent aliases are the default alias form:

```text
type UserName = string

let raw: string = "Ada"
let name: UserName = raw      # ok
```

Nominal single-field newtypes use a distinct spelling. Candidate:

```text
type Mile(i32)
type Kilometer(i32)

miles := Mile(10)
km := Kilometer(16)

fn drive(distance: Mile) -> void:
    ...

drive(miles)             # ok
drive(km)                # invalid
drive(10)                # invalid

raw_distance := i32(miles)
```

Integer widening is implicit from lower precision to higher precision. Narrowing requires an explicit cast:

```text
let small: i16 = 42
let large: i64 = small

let huge: i64 = 9000
let smaller: i16 = huge          # invalid
smaller_ok := i16(huge)
```

Integer literals use a default concrete type when there is no expected type. The default integer type is always `i32`. When there is an expected numeric type, the literal is checked against that type's range:

```text
x := 1                 # i32 by default
let small_ok: i8 = 1   # ok
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

Generic arguments are inferred at call sites when the type is unambiguous. Callers can also provide the full generic argument list explicitly:

```text
names := ["Ada", "Grace"]

a := first(names)          # T inferred as string
b := first[string](names)  # explicit generic argument
```

v1 does not support partial explicit generic arguments or placeholder generic arguments.

Variadic generics use type packs. Minimal v1 supports packs only in function types, vararg parameters, and spread calls:

```text
fn call_with[Args..., R](f: fn(Args...) -> R, args: Args...) -> R:
    f(args...)
```

v1 does not support pack mapping, filtering, splitting, or arithmetic.

Traits are explicit, not structural. A type does not implement a trait just because it has matching methods:

```text
trait Display:
    fn display(self) -> string

impl Display for User:
    fn display(self) -> string:
        self.email
```

Generic bounds such as `T: Display` use static dispatch. Trait value types such as `value: Display` use Go-style dynamic dispatch: the runtime value carries concrete data plus a method table. There is no `dyn` or `any` marker.

Nullability is explicit. `T` and `T?` are different types, and `nil` only belongs to optional values.

Struct embedding is composition, not inheritance. Embedded fields and methods can be promoted, but the outer struct is not automatically a subtype of the embedded struct.

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

show(Service { Logger: Logger { name: "api" } })    # ok
```

If multiple embedded fields promote conflicting methods, the outer type does not satisfy the trait automatically. The user must qualify calls or write an explicit impl to resolve the conflict.

## Modules, Packages, And Imports

Modules are path-inferred. There is no required `module` or `package` declaration in source files. The module path comes from the file path under the package source root.

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

Example public types:

```text
# src/user/types.hd

pub type UserId(string)

pub struct User:
    id: UserId
    email: string
```

Import selected names with grouped import syntax:

```text
# src/post/service.hd

import pkg.user.types.{User, UserId}

fn author_id(user: User) -> UserId:
    user.id
```

Import a module namespace when qualification is useful:

```text
import pkg.user.types

fn author_id(user: pkg.user.types.User) -> pkg.user.types.UserId:
    user.id
```

Use aliases for long module paths or conflicting names:

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

Visibility is explicit with `pub`. Declarations without `pub` are private to their module:

```text
pub fn load_user(id: UserId) -> Result[User?, DbError]:
    ...

fn normalize_email(email: string) -> string:
    email.trim().lower()
```

Submodule access goes through imports; a parent module does not automatically import child modules, and a child module does not automatically import parent declarations.

Import and re-export cycles are rejected in v1.

v1 keeps visibility simple: declarations are module-private by default, and `pub` makes them public. There is no package-private visibility modifier.

## Primitive Types

Primitive types include booleans, width-explicit numbers, strings, and chars.

```text
let ok: bool = true
let small: i8 = 1
let short: i16 = 1
let count: i32 = 42
let large: i64 = 9000
let octet: u8 = 255
let ushort: u16 = 65535
let size: u32 = 1024
let huge: u64 = 9000
let ratio: f32 = 0.5
let precise: f64 = 0.5
let name: string = "Ada"
let initial: char = 'A'
```

Text literals distinguish chars and strings:

```text
letter := 'A'          # char
name := "Ada"          # string
```

Signed integer types:

```text
i8
i16
i32
i64
```

Unsigned integer types:

```text
u8
u16
u32
u64
```

Floating-point types:

```text
f32
f64
```

`void` is used for functions that return no useful value:

```text
fn log_start() -> void:
    println("start")
```

There are no convenience aliases such as `int`, `uint`, or `float` in v1. Use explicit-width numeric types. `decimal` is a standard-library type, not a primitive.

## Operators

Arithmetic operators:

```text
a + b
a - b
a * b
a / b
a % b
a ** b
-a
```

Integer `/` truncates toward zero. Integer overflow is always checked unless code uses explicit wrapping APIs.

Bitwise operators apply to integer values:

```text
a & b
a | b
a ^ b
~a
a << n
a >> n
```

Shift counts can use any integer type, but must be non-negative and in range at runtime unless statically known.

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

## Tuples

Tuples are built in for lightweight grouped values:

```text
point := (10, 20)                 # (i32, i32)
entry := ("Ada", 36, true)        # (string, i32, bool)
single := (1,)                    # (i32,)
empty := ()                       # empty tuple
```

Tuple type spelling:

```text
(i32, i32)
(string, i32, bool)
(i32,)
()
```

Tuple fields use Rust-style numeric field access:

```text
x := point.0
y := point.1
```

Tuple destructuring works with both binding forms:

```text
x, y := point
let name, score = entry
```

Named tuples are not in v1. Use structs when field names are part of the meaning.

## Struct Literals

Struct literals use the type name followed by braces:

```text
user := User {
    id: "user_123",
    email: "ada@example.com",
    display_name: "Ada"
}
```

This keeps construction visually distinct from function calls.

Struct fields do not carry their own mutability marker. Mutation is controlled by the binding or parameter that owns the value:

```text
let mut editable = User {
    id: "user_123",
    email: "ada@example.com",
    display_name: "Ada"
}

editable.display_name = "Ada Lovelace"

fn normalize_user(mut user: User) -> User:
    user.email = user.email.trim().lower()
    user
```

Use copy-update syntax when creating a modified value from an existing struct:

```text
renamed := User {
    ...user,
    display_name: "Ada Lovelace"
}
```

## Struct Embedding

Struct embedding uses a bare type-name line inside the struct body. There is no `embed` keyword.

```text
struct Timestamps:
    created_at: i64
    updated_at: i64

struct Post:
    Timestamps
    id: string
    title: string
```

Embedded structs are initialized with the embedded type name as the field key:

```text
post := Post {
    Timestamps: Timestamps {
        created_at: 1700000000,
        updated_at: 1700000000
    },
    id: "post_123",
    title: "Hello"
}
```

The embedded struct's fields are promoted for ordinary field access:

```text
post.created_at
```

Embedded-field name conflicts follow Go-style promotion rules. Direct promoted access is allowed only when the field name is unambiguous. If multiple embedded structs promote the same field name, direct access is invalid and the user must qualify through the embedded field:

```text
struct CreatedBySystem:
    id: string

struct CreatedByUser:
    id: string

struct AuditRecord:
    CreatedBySystem
    CreatedByUser

record.id                    # invalid: ambiguous promoted field
record.CreatedByUser.id      # ok
```

## Enums

Enums model sum types. Simple variants have no payload:

```text
enum JobStatus:
    Queued
    Running
    Succeeded
    Failed
```

Payload variants use compact `Variant(field: type)` declarations. Use a separate struct payload when the data is large enough to need a full field block:

```text
enum ToolError:
    NotFound(resource: string)
    Unauthorized(reason: string)
    RateLimited(retry_after_ms: i32)
    Internal(message: string)
```

Payload variants are constructed with enum-qualified names and brace literals:

```text
error := ToolError.NotFound {
    resource: "user_123"
}
```

Match arms use `pattern => expression`. Enum variant patterns must be qualified with the enum name, and payload patterns use call-style parentheses:

```text
fn error_message(error: ToolError) -> string:
    match error:
        ToolError.NotFound(resource) => "not found: " + resource
        ToolError.Unauthorized(reason) => "unauthorized: " + reason
        ToolError.RateLimited(retry_after_ms) => "rate limited, retry after " + retry_after_ms.to_string() + "ms"
        ToolError.Internal(message) => message
        _ => "unknown"
```

Enums can declare constructor parameters shared by every variant. Enum constructor definitions and calls follow the same parameter conventions as functions: unnamed positional parameters/arguments first, then named parameters/arguments.

```text
enum StatusCode(i32):
    Ok -> StatusCode(200)
    NotFound -> StatusCode(404)

enum HttpStatus(code: i32, phrase: string):
    Ok -> HttpStatus(200, phrase="OK")
    NotFound -> HttpStatus(404, phrase="Not Found")
```

Enums support generic ADTs, recursive enums, and GADT-style variants with explicit result types:

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

Pattern matching on a GADT-style variant refines the enum type parameter inside that arm:

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

`Result` values use capitalized helper constructors:

```text
return Ok(user)
return Err(db_error)
```

## Absence

Follow Swift's design:

```text
let name: string = "Ada"      # required, cannot be nil
let nickname: string? = nil   # optional, may be nil
```

Use `T?` for optional values. Non-optional values cannot be `nil`.

Rust-style `?` propagates absence from functions returning optional values:

```text
fn label(name: string?) -> string?:
    actual := name?
    "user: " + actual
```

The same `?` operator propagates errors from `Result[T, E]` in functions returning a compatible `Result`.

## Built-In Collections

hd-lang should have built-in list and map collection types and literals, not only library-defined container types. Collections are part of the language surface because validation, generated test data, serialization, tool schemas, and interop all need to understand them.

Candidate collection types:

```text
list[string]
map[string, i32]
```

Collection literals should infer their collection type when possible:

```text
names := ["Ada", "Grace", "Linus"]       # list[string]
scores := {"Ada": 10, "Grace": 12}       # map[string, i32]
```

List comprehensions produce `list[T]` values:

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

Map comprehensions produce `map[K, V]` values:

```text
scores_by_name := {for user in users => user.name: user.score}
active_by_id := {for user in users if user.active => user.id: user}
```

If a map comprehension produces the same key more than once, the later value wins.

Comprehensions cannot contain suspension points. Use an explicit loop when the body needs a `!` call.

Explicit mutable collection variables use `let mut`, like any other mutable binding:

```text
let mut attempts: list[i32] = []

attempts.append(1)
attempts.append(2)
```

Open syntax issues:

1. Which collection operations are methods, functions, or trait-provided behavior.

## Function Shape

Basic function shape:

```text
fn add(a: i32, b: i32) -> i32:
    a + b
```

Explicit `return` exists for early exits:

```text
fn first_name(name: string) -> string:
    if name == "":
        return "anonymous"
    name.split(" ")[0]
```

Named arguments are supported. Calls can mix positional and named arguments, but positional arguments must come first:

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
```

Default values can use pure expressions and pure function calls. They cannot require effects, dependencies, or suspension:

```text
fn default_port() -> i32:
    443

fn connect(host: string, port: i32 = default_port()) -> Connection:
    ...

fn bad_connect(host: string, token: string = read_secret!("TOKEN")) -> Connection:
    ...      # invalid: default value suspends and requires a capability
```

Varargs accept zero or more positional arguments:

```text
fn sum(values: i32...) -> i32:
    let mut total: i32 = 0
    for value in values:
        total = total + value
    total

sum(1, 2, 3)

nums := [1, 2, 3]
sum(nums...)
```

Varargs must be the final positional parameter. Spread syntax is positional only. A named vararg accepts a list:

```text
fn tagged_sum(tag: string, values: i32...) -> i32:
    ...

tagged_sum("score", 1, 2, 3)
tagged_sum("score", nums...)
tagged_sum(tag="score", values=nums)
tagged_sum(tag="score", nums...)      # invalid: positional spread after named arg
```

Generic functions put generic arguments after the function name:

```text
fn first[T](items: list[T]) -> T?:
    if items.len() == 0:
        nil
    else:
        items[0]
```

Closures use anonymous `fn(...) -> ...` syntax:

```text
slugify := fn(text: string) -> string:
    text.trim().lower().replace(" ", "-")
```

There is no separate short closure syntax in v1. Use `fn(...) -> ...:` for closures. Same-line closure bodies are allowed when the body is a single expression:

```text
inc := fn(x: i32) -> i32: x + 1
add := fn(x: i32, y: i32) -> i32: x + y
make_id := fn() -> string: "id_123"
```

Closure parameter and return types can be inferred when there is an expected function type:

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

Shorthand argument closures such as `$0 + $1` are deferred; v1 requires named parameters in the closure parameter list.

Closures capture values from lexical scope. Closures that mutate captured locals have a mutable function type, written `mut fn(...) -> ...`. Calling a mutable closure requires the closure value itself to be mutable:

```text
let mut count: i32 = 0

let mut next = mut fn() -> i32:
    count = count + 1
    count

next()
```

Plain `fn(...) -> T` closures cannot mutate captured locals. Use `mut fn(...) -> T` when mutation is part of the callable's behavior:

```text
fn repeat(times: i32, mut f: mut fn() -> void) -> void:
    let mut i: i32 = 0
    while i < times:
        f()
        i = i + 1
```

Closures that capture dependencies or capabilities carry those requirements in their function type. Serializable closures are stricter: they can only capture serializable values, and cannot capture live handles or capabilities unless a runtime feature explicitly supports that capture.

Overloads are not supported. Each function name resolves to one declaration in a scope.

Effectful function candidate:

```text
fn load_user!(id: UserId) -> Result[User?, DbError] $ Database:
    ...
```

This keeps the function signature compact while making effects explicit.

Open questions:

1. Should effect names be plain identifiers, trait/capability names, or typed values?
2. Exact precedence and formatting rules for effect expressions using `+`, `-`, and parentheses.

Examples:

```text
fn load_user!(id: UserId) -> Result[User?, DbError] $ Database
```

## Traits And Methods

Traits describe shared behavior without classes or inheritance.

Candidate:

```text
trait Display:
    fn display(self) -> string

impl Display for User:
    fn display(self) -> string:
        self.email

impl User:
    fn domain(self) -> string:
        self.email.split("@")[1]

fn show[T: Display](value: T) -> string:
    value.display()
```

Traits can provide default implementations:

```text
trait Named:
    fn name(self) -> string

    fn label(self) -> string:
        "name: " + self.name()
```

Generic traits:

```text
trait Repository[T]:
    fn get!(self, id: string) -> T? $ Database
    fn save!(self, value: T) -> void $ Database
```

Trait bounds compose like Rust:

```text
fn audit_label[T: Display + Named](value: T) -> string:
    value.display() + " / " + value.name()
```

Receiver spelling is `self` or `mut self`; there is no reference receiver spelling. Semantically, primitive types are passed by value and composite types are passed by reference. Structs, tuples, lists, and maps are composite types.

Trait implementation is explicit. A type does not implement a trait just because it has matching methods.

Struct embedding interacts with traits through promoted methods in the same spirit as promoted fields: embedded methods can be called when unambiguous, and unambiguous promoted methods can satisfy trait requirements for the outer struct. Ambiguous promoted methods must be qualified through the embedded field or resolved with an explicit impl.

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
```

This is different from generic static dispatch, where the compiler specializes the function for a concrete type:

```text
fn show_static[T: Display](value: T) -> string:
    value.display()
```

## Contract Syntax Direction

Contracts need to be readable, toolable, and easy for AI to generate correctly. The preferred direction is direct `require` and `ensure` sections inspired by NimContracts, without a wrapping `contract` block:

```text
fn isqrt(x: i32) -> i32 $ require + ensure:
    require:
        x >= 0
    ensure:
        result >= 0
        result * result <= x
        (result + 1) * (result + 1) > x
    floor(sqrt(x))
```

This keeps contracts visibly attached to the function while avoiding Python-like decorators and an extra wrapper block. Contracts are language-level intent, not runtime metadata.

Contract expressions should be typechecked, but the compiler should not try to prove them. Failed contract checks should use their own effects. The exact effect names are still open; `require` and `ensure` are placeholders.

Candidate effect names:

```text
fn divide(a: i32, b: i32) -> i32 $ require + ensure
fn divide(a: i32, b: i32) -> i32 $ precondition + postcondition
fn divide(a: i32, b: i32) -> i32 $ caller_contract + callee_contract
```

## NimContracts-Inspired Ideas

NimContracts uses contract sections for preconditions, postconditions, invariants, and bodies. The useful ideas to adapt are:

1. `require` for preconditions.
2. `ensure` for postconditions.
3. `invariant` for loop or type invariants.
4. A notion of previous values for postconditions and invariants.
5. Contract-specific diagnostics instead of generic assertion failures.
6. Contract sections as documentation, not just runtime checks.

## Contract Section Decision

```text
fn withdraw(account: Account, amount: Money) -> Account $ require + ensure:
    require:
        amount > 0
        account.balance >= amount
    ensure:
        result.balance >= 0
    account.withdraw(amount)
```

Use:

1. `require` for caller obligations.
2. `ensure` for callee obligations.
3. `invariant` for loops and structs.
4. Dedent back to the function or loop body after contract sections.

Rejected alternatives:

1. `requires` / `ensures`: more natural English, but longer and less aligned with NimContracts.
2. `pre` / `post`: compact, but less self-explanatory.
3. A wrapping `contract` block: clear grouping, but adds an unnecessary extra level.
4. An explicit `body` section: clear for parsers, but visually too heavy.

## Function Body Decision

Do not use an explicit `body` section after contract sections. The executable body starts when indentation returns to the function body level:

```text
fn divide(a: i32, b: i32) -> i32 $ require + ensure:
    require:
        a >= 0
        b > 0
    ensure:
        result * b <= a
        (result + 1) * b > a
    a / b
```

Note: an earlier sketch used `ensure result * b == a`, which is false for any division with a remainder (7 / 2 = 3, but 3 * 2 != 7). It survived review because it looked plausible — motivation for property-testing every contract predicate as part of the standard toolchain.

Reasons:

1. It keeps function code compact.
2. It preserves the Python-like indentation feel.
3. It keeps contract predicates visually nested under `require` and `ensure`, while the real body returns to the normal function indentation level.

## Loop Invariants

Loops should also support invariants:

```text
let mut low: i32 = 0
let mut high: i32 = items.len()

while low < high:
    invariant:
        if key in items:
            key in items[low..high]
        high - low < old(high - low)
    mid := (low + high) / 2
    if items[mid] < key:
        low = mid + 1
    else:
        high = mid
```

## Type Invariants

Structs should be able to declare invariants:

```text
struct Account:
    balance: Money
    currency: Currency

    invariant:
        balance >= 0
```

Open question: should type invariants be checked after construction, after public mutation, or at all function boundaries?

## Dependency Effects

Dependencies should be modeled as signature requirements, but they do not need a separate dependency declaration syntax. A dependency can be an ordinary trait or capability that appears in a function's `$` requirement row.

Normal error handling is not modeled as an effect. Errors use `Result[T, E]`.

### Effect Body Operations

An effect is doing two related but distinct jobs:

1. Resolving or injecting a dependency/capability.
2. Suspending execution flow into a handler.

The signature can summarize the required `$` row:

```text
fn load_user!(id: UserId) -> Result[User?, DbError] $ Database
```

But the distinction should be expressed in the body, not by writing `use` or `raise` as kinds inside the function signature.

```text
db := $.use(Database)
user := db.get_user!(id)?
```

Body-level intent:

1. `$.use(Database)` retrieves the `Database` provider from the current context.
2. `db.get_user!(id)` is the suspension point where control can enter the handler.
3. Normal errors are returned as `Result[T, E]`, not receiverless control effects.

A suspending function must declare the `!` suffix in its function name, and callers use the same suffix at the suspension point:

```text
fn get_user!(id: UserId) -> Result[User?, DbError]:
    ...

user := db.get_user!(id)?
```

This is closer to Effect's service model, where the type tracks required services but service access happens in the program body, and to Kotlin-style functional effects where effectful operations are explicit suspension points.

Candidate:

```text
trait Database:
    fn get_user!(id: UserId) -> Result[User?, DbError]

trait Cache:
    fn get_user(id: UserId) -> User?

fn load_user!(id: UserId) -> Result[User?, DbError] $ Database + Cache:
    db, cache := $.use(Database, Cache)
    cached := cache.get_user(id)
    if cached != nil:
        return Ok(cached)
    user := db.get_user!(id)?
    user

handler mock_db for Database:
    fn get_user!(id: UserId) -> Result[User?, DbError]:
        Ok(User {
            id: id,
            name: "Test User"
        })

$.with(Database=mock_db, Cache=memory_cache):
    user := load_user!(UserId { value: "user_123" })
```

Reusable contexts are provider-map values typed by a requirement row:

```text
fn prod_context() -> $.Context[Metrics + Cache]:
    $.context(Metrics=metrics, Cache=cache)

$.with(Database=mock_db, Logger=console_logger, ...prod_context()):
    db, logger, cache := $.use(Database, Logger, Cache)
    user := load_user!(UserId { value: "user_123" })
```

`$.Context[Metrics + Cache]` is not a variadic generic. The `Metrics + Cache` part is an unordered requirement row, using the same composition shape as function `$` requirements. `$.context(Metrics=metrics, Cache=cache)` creates a reusable context value, `$.with(Database=mock_db, ...prod_context())` opens a lexical provider scope and spreads reusable providers, and `$.use(Database, Logger, Cache)` retrieves providers in the requested return order.

Requirement names in `$.context`, `$.with`, and `$.use` are requirement keys, usually trait or capability names, not ordinary named-argument labels.

Duplicate providers for the same requirement cannot coexist; during context construction or spread, later bindings win and the resulting context has one entry per key. If a required provider does not exist for a call, that is a compile-time error. The `$` namespace is special context syntax, not an ordinary value namespace.

Open syntax issues:

1. Exact provider declaration syntax for production, tests, and package/app boundaries.
2. `Result[T, E]` ergonomics beyond `?` propagation and `Ok(value)` / `Err(error)` construction, including pattern matching.

## Effect Polymorphism

Higher-order functions need a way to propagate the effects of function-typed arguments. Without it, `map` either forbids effectful callbacks or needs one copy per effect combination:

```text
fn map(items: list[T], f: fn(T) -> U) -> list[U]   # what effects does map have?
```

### Candidate A: Explicit effect-row variables (Koka-style)

```text
fn map[T, U, e](items: list[T], f: fn(T) -> U $ e) -> list[U] $ e
```

1. Fully explicit; aligns with effects being visible in signatures.
2. General: supports several independent variables, stored function fields, and returned closures.
3. Verbose; AI and reviewers must thread `$ e` correctly, and variables must be declared so a typo of an effect name cannot silently become a fresh variable.

Explicit effect variables become more useful if handlers can remove effects from the variable. This may be full row polymorphism, or a smaller operation that only supports effect removal by handlers.

Candidate:

```text
fn handle_log[e](callback: fn(string) -> void $ e) -> void $ (e - Logger):
    $.with(Logger=logger):
        callback("str")
```

Meaning:

1. `callback` may require any effects in `e`.
2. `handle_log` installs a provider/handler for `Logger`.
3. The remaining requirement row is `e - Logger`.
4. If `callback` only requires `Logger`, the result is pure after `Logger` is handled.
5. If `callback` requires `Logger` and `Database`, the result effect is `Database`.

The important idea is effect-variable transformation: a function can propagate "all callback effects except the ones I handle." This is useful even if the language does not expose full row polymorphism in v1.

### Candidate B: Parameter-linked effects (generalized Swift `rethrows`)

```text
fn map(items: list[T], f: fn(T) -> U) -> list[U] $ f
```

`$ f` in the requirement row means "whatever requirements the argument bound to `f` has".

1. Reads naturally; no type-level machinery in the common case.
2. Typo-resistant: `$ f` must name a function-typed parameter or the program does not compile.
3. Multiple function parameters union: `$ f + g`.
4. Does not cover function values stored in structs or returned closures; those need row variables or monomorphization.

### Candidate C: Inferred propagation

Unhandled requirements of function-typed parameters propagate automatically; the signature stays clean and tooling displays the resolved requirement row.

1. Zero annotation burden; nothing for AI to get wrong.
2. Conflicts with the decision that effects are explicit in signatures; a reviewer reading source sees `map` as pure.

### Open questions regardless of candidate

1. Effect subtraction/removal syntax: should handled effects be written explicitly as `$ (e - Logger)` or inferred from provider scopes such as `$.with(Logger=logger):`?
2. Function-typed struct fields and returned closures: row variables, monomorphization, or disallowed in v1?
3. Do polymorphic rows range over contract effects (`require`, `ensure`) as well as user effects?
4. How do handlers installed at a call site interact with a polymorphic row?
5. Is the underlying model full row polymorphism, or a smaller effect-variable system that only supports union and removal?

## Branded Validated Types

Validation annotations do not refine static types by default (unbranded values keep base type identity), but opt-in branded newtypes tie validation to type identity. If a type is not branded, it does not refine.

Candidate spelling, reusing ordinary validator values:

```text
brand Email = string.email().max_len(320)

struct User:
    id: UserId
    email: Email
```

Rules:

1. `Email` is a distinct static type. A `string` is not assignable to `Email` without going through validation.
2. Construction is the enforcement boundary: `Email.parse(s)` returns `Email` or fails with a validation effect ("parse, don't validate").
3. Once constructed, interior code can trust the brand; no re-validation at internal boundaries.
4. Unbranded annotated fields keep the existing semantics: metadata for tooling and runtime checks only, no refinement.

Open questions:

1. Spelling: `brand Email = ...` versus `type Email = brand string ...` versus an annotation on a type alias.
2. Whether brand-to-base coercion is implicit (usable anywhere a `string` is) or explicit (`.value`).
3. Whether inline `@` annotations may appear on a branded field, and how conflicts with the brand's validators are handled. Candidate: field annotations on branded fields are a compile error; validators live on the brand.
4. How brands appear in generated JSON Schema and TypeScript output. Candidate: TypeScript branded types.

## Schema And Validation Direction

Validation should not create distinct static subtypes by default. A field like `string.max_len(50)` and `string.max_len(100)` should still have the same base static type, `string`; validation metadata is used for runtime checks, generated schemas, generated data, docs, and tooling.

## Generic Representation And Annotation Derivation

Annotation design should be split into four separate concerns before finalizing syntax.

### Common Representation

The compiler should expose a typed common representation for declarations. This is similar in spirit to Python's runtime introspection model (`__annotations__`, docstrings, signatures, defaults) and Scala's compile-time generic representation, but hd-lang should make it compile-time-first, typed, and compiler-verifiable.

The common representation should cover at least:

1. Structs: name, fields, embedded structs, field types, field defaults, docs, visibility, and attached metadata.
2. Enums: name, variants, constructor arguments, GADT result types, docs, and attached metadata.
3. Functions: name, parameters, return type, dependency requirements, suspension marker, docs, defaults, and attached metadata.

Provisional shape vocabulary:

```text
shape(User)       # StructShape
shape(JobStatus)  # EnumShape
shape(get_user)   # FnShape
```

This representation is the foundation for AI tooling: the compiler and tools can inspect source-level intent without falling back to string parsing or ad hoc reflection.

### Generic Derivation

The common path should be structural derivation from a shape. A facet such as JSON, UI, Tool, DatabaseSchema, Retention, or Observability defines how to derive an artifact from an appropriate shape.

Conceptually:

```text
Json.derive(shape(User))
UI.derive(shape(User))
Tool.derive(shape(get_user))
Retention.derive(shape(Post))
```

The exact user-facing spelling is still open. Candidate directions include `derive Facet for Target`, facet-led blocks, or another syntax that keeps the derived facet and target obvious.

### Overriding Derivation

Derived behavior needs local specialization. There should be several override paths:

1. Override annotation: patch only specific fields, variants, parameters, or function-level settings while keeping the generic derivation.
2. Reusable override profile: name a set of overrides so the same derivation policy can be reused.
3. Whole derivation rewrite: replace the generic derivation with custom code when a facet cannot be derived structurally.

The design goal is Serde-like local override ergonomics without making each ecosystem invent unrelated attribute syntax.

Provisional examples, not settled syntax:

```text
derive UI for User:
    avatar_url = AvatarImage(size=48)
    display_name = TextInput(label="Display name")

derive Json for User:
    rename_all = camelCase
    avatar_url = omitWhenNil()
```

### Runtime Use

Compile-time derivation and runtime metadata export should be separate. Some derivations only need generated code; others need runtime values for tool registries, UI renderers, schemas, workflow engines, or observability systems.

Runtime export should be explicit when requested:

```text
derive Json for User
derive UI for User export UserView
derive Tool for get_user export GetUserTool
```

The exact syntax is open, but the principle is settled: runtime metadata should be produced deliberately, not as an accidental consequence of attaching metadata to a declaration.

### Provisional Annotation Protocol

A promising direction is to model annotations as uniformly typed derivation protocols implemented by ordinary types. This is not final syntax; it records the current idea for further design.

Shape values should be usable as runtime values:

```text
shape(User)        # StructShape
shape(User.id)     # FieldShape
shape(JobStatus)   # EnumShape
shape(get_user)    # FnShape
```

An annotation facet for structs can implement a protocol similar to:

```text
trait StructAnnotation:
    type FieldTarget
    type Target

    fn map_field(field: FieldShape) -> Self::FieldTarget

    fn build(
        shape: StructShape,
        fields: Dict[string, Self::FieldTarget],
    ) -> Self::Target
```

`map_field` derives one field-level artifact at a time. It receives `FieldShape`, so the derivation can inspect the source field's runtime type metadata. The field result type is uniform for the annotation facet. `build` combines the field artifacts into one ordinary target value. Struct field artifacts are passed as `Dict[string, FieldTarget]`, keyed by field name.

For UI, every field maps to a `ReactComponent`:

```text
impl StructAnnotation for UI:
    type FieldTarget = ReactComponent
    type Target = list[ReactComponent]

    fn map_field(field: FieldShape) -> ReactComponent:
        if field.type == string:
            TextInput(field.name)
        else if field.type == bool:
            Checkbox(field.name)
        else:
            DefaultInput(field.name)

    fn build(shape: StructShape, fields: Dict[string, ReactComponent]) -> list[ReactComponent]:
        shape.fields.map(fn(field): fields[field.name])
```

`annotate` is the chosen special syntax for customizing a facet for a target:

```text
annotate UI for User:
    userId = ReactUserId

    fn build(shape: StructShape, fields: Dict[string, ReactComponent]) -> list[ReactComponent]:
        [
            fields["userId"],
            fields["displayName"],
            fields["avatar"],
        ]
```

Inside `annotate UI for User`, assignments such as `userId = ...` may only target existing fields of `User`. They do not create new fields. The right-hand side replaces the mapped field result for that field while keeping the generic derivation for all other fields. The block may also override the whole-generation hook, `build`.

For one facet/target pair, only one `annotate` block is allowed in a package:

```text
annotate DatabaseSchema for User:
    email = DatabaseColumn.TextColumn(name="email", max_len=320)
```

A second `annotate DatabaseSchema for User` block in the same package is a compile-time error. Therefore field-level merge rules and duplicate `build` rules are not needed for same-package annotations.

Annotation blocks are global within a package. Any module in the package can request the materialized annotation value:

```text
schema := DatabaseSchema::annotation(User)
```

Libraries do not provide implicit default annotation blocks for downstream applications. If a library explicitly provides `annotate Facet for Target`, that annotation is authoritative and cannot be overridden by downstream packages. If the imported target has no library-provided annotation for that facet, an application package can define its own package-local annotation for the imported target. Since only one annotation block can apply for a facet/target pair in a package, no merge rule is needed; attempting to define a downstream annotation where an upstream explicit annotation already exists is a compile-time error.

The right-hand side of a field override must typecheck as that annotation's uniform field target:

```text
ReactUserId: UI::FieldTarget
```

For UI, that means `ReactComponent`.

For database schema, the field type is still available to `map_field`, but the result is uniformly typed:

```text
enum DatabaseColumn:
    I32Column(name: string)
    I64Column(name: string)
    TextColumn(name: string, max_len: i32?)
    BoolColumn(name: string)
    JsonColumn(name: string)

struct TableSchema:
    name: string
    columns: list[DatabaseColumn]

impl StructAnnotation for DatabaseSchema:
    type FieldTarget = DatabaseColumn
    type Target = TableSchema

    fn map_field(field: FieldShape) -> DatabaseColumn:
        max_len := field.annotation(MaxLen)?.value

        if field.type == i32:
            DatabaseColumn.I32Column(field.name)
        else if field.type == i64:
            DatabaseColumn.I64Column(field.name)
        else if field.type == string:
            DatabaseColumn.TextColumn(field.name, max_len=max_len)
        else if field.type == bool:
            DatabaseColumn.BoolColumn(field.name)
        else:
            DatabaseColumn.JsonColumn(field.name)

    fn build(shape: StructShape, fields: Dict[string, DatabaseColumn]) -> TableSchema:
        TableSchema {
            name: shape.name,
            columns: shape.fields.map(fn(field): fields[field.name]),
        }
```

Overrides typecheck against the uniform target:

```text
struct User:
    id: UserId
    @max_len(320)
    email: string
    @max_len(80)
    display_name: string
    active: bool

annotate DatabaseSchema for User:
    # Optional override. Without this, `map_field` reads @max_len(320).
    email = DatabaseColumn.TextColumn(name="email", max_len=320)
    active = DatabaseColumn.BoolColumn(name="is_active")
```

The compiler checks only that each right-hand side is a `DatabaseColumn`. It does not statically prove that a `string` field received a text column or that an `i32` field received an integer column. Annotation facets can still enforce stricter domain rules in their own constructors, `map_field`, validation hooks, or generated diagnostics, but the core annotation protocol stays uniform.

This is an intentional simplification. TypeScript mapped types can model shape-preserving field transforms, but in a stricter nominal language that likely requires HKT-like type functions, dependent record construction, or special compiler-generated HList/labelled-record machinery. hd-lang should avoid that in the initial annotation design.

Enums need the same idea for variants:

```text
trait EnumAnnotation:
    type VariantTarget
    type Target

    fn map_variant(variant: VariantShape) -> Self::VariantTarget

    fn build(
        shape: EnumShape,
        variants: Dict[string, Self::VariantTarget],
    ) -> Self::Target
```

Function annotations use the same shape: a parameter mapping step plus `build`. In v1, function parameter assignment overrides inside `annotate` are not supported; parameter customization should come from parameter annotations, parameter docs, or a whole-function `build` override.

```text
trait FuncAnnotation:
    type ParamTarget
    type Target

    fn map_param(param: ParamShape) -> Self::ParamTarget

    fn build(
        shape: FnShape,
        params: Dict[string, Self::ParamTarget],
    ) -> Self::Target
```

Example tool annotation:

```text
struct ToolParam:
    name: string
    schema: JsonSchema
    description: string?

struct ToolSpec:
    name: string
    description: string
    params: Dict[string, ToolParam]
    result: JsonSchema
    requirements: list[string]

impl FuncAnnotation for Tool:
    type ParamTarget = ToolParam
    type Target = ToolSpec

    fn map_param(param: ParamShape) -> ToolParam:
        ToolParam {
            name: param.name,
            schema: JsonSchema::from_type(param.type),
            description: param.annotation(Description)?.text,
        }

    fn build(shape: FnShape, params: Dict[string, ToolParam]) -> ToolSpec:
        ToolSpec {
            name: shape.name,
            description: shape.doc,
            params: params,
            result: JsonSchema::from_type(shape.return_type),
            requirements: shape.requirements.names(),
        }
```

Function annotation overrides can replace the whole build:

```text
annotate Tool for get_user:
    fn build(shape: FnShape, params: Dict[string, ToolParam]) -> ToolSpec:
        spec := Tool::build(shape, params)
        ToolSpec {
            ...spec,
            name: "get_user",
        }
```

This is not valid in v1:

```text
annotate Tool for get_user:
    id = ToolParam { ... }  # invalid: function parameter assignment overrides are deferred
```

Runtime use should be explicit. Applying an annotation facet can produce a runtime value that tools, registries, UI renderers, schema generators, or deployment systems can consume:

```text
UserTable := DatabaseSchema::annotation(User)     # TableSchema
UserForm := UI::annotation(User)                  # list[ReactComponent]
ToolSpec := Tool::annotation(get_user)            # ToolSpec
ErrorSchema := ErrorDoc::annotation(ToolError)    # ErrorSchema
```

The spelling `Facet::annotation(Target)` is the current preferred provisional syntax. Semantically, it means:

```text
shape_value := shape(Target)
fields := Dict.from_entries(
    shape_value.fields.map(fn(field): (field.name, Facet.map_field(field)))
)
result := Facet.build(shape_value, fields)
```

For functions and enums, the same pattern applies with parameters or variants:

```text
fn_shape := shape(get_user)
param_map := Dict.from_entries(
    fn_shape.params.map(fn(param): (param.name, Tool.map_param(param)))
)
tool_spec := Tool.build(fn_shape, param_map)

enum_shape := shape(ToolError)
variant_map := Dict.from_entries(
    enum_shape.variants.map(fn(variant): (variant.name, ErrorDoc.map_variant(variant)))
)
error_schema := ErrorDoc.build(enum_shape, variant_map)
```

If a package-local annotation block exists, the compiler/runtime applies its field overrides between generic mapping and whole-generation construction:

```text
user_shape := shape(User)
default_fields := Dict.from_entries(
    user_shape.fields.map(fn(field): (field.name, DatabaseSchema.map_field(field)))
)
overridden_fields := apply_overrides(DatabaseSchema, User, default_fields)
table := DatabaseSchema.build(user_shape, overridden_fields)
```

Runtime annotation values should be ordinary typed values. They can be assigned, exported, registered, passed to functions, or generated into external artifacts:

```text
tool_registry.register(Tool::annotation(get_user))
db.sync(DatabaseSchema::annotation(User))
render_form(UI::annotation(User), user)
openapi.add_tool(Tool::annotation(get_user))
```

Whether annotation values are computed at compile time, generated into code, cached by the runtime, or materialized lazily is a compiler/runtime decision. The source-level model should be that annotations are requested explicitly and produce ordinary values.

Open concerns:

1. The exact protocol names are not settled: `StructAnnotation`, `EnumAnnotation`, `FuncAnnotation`, `map_field`, `map_variant`, `map_param`, and `build` all need naming review.
2. Field and variant result types are uniform in the current model. This gives up static proof of field-type-specific override correctness in exchange for a much simpler type system.
3. `build` receives dictionaries keyed by field, variant, or parameter name. If output ordering matters, `build` should use the original `shape` ordering.
4. Function parameter assignment overrides are deferred in v1; parameter customization uses parameter annotations/docs or whole-function `build`.
5. Runtime annotation materialization syntax is provisional. The current preferred sketch uses `Facet::annotation(Target)`, but the exact spelling is still open.

Support two styles:

1. Lightweight annotations for minimal inline constraints.
2. External derivation/override blocks for richer or extendable metadata. The exact block syntax is unresolved.

### Decorator Metadata

Decorators are distinct from `annotate Facet for Target` blocks.

Decorator principles:

1. A decorator does not alter the behavior or type of the declaration it is attached to.
2. A decorator cannot change a field name, parameter name, variant name, function name, underlying type, or signature.
3. A decorator attaches static metadata to the target shape.
4. Decorator metadata is compile-time computed and exposed through shape values such as `FieldShape`, `ParamShape`, `VariantShape`, `StructShape`, `EnumShape`, and `FnShape`.
5. Field decorator applicability is checked through trait resolution on the decorated field type.

Example:

```text
struct MaxLen:
    value: i32

trait FieldDecorator[T]:
    fn attach(decorator: Self, field: FieldShape) -> Result[FieldMetadata, DecoratorError]

impl FieldDecorator[string] for MaxLen:
    fn attach(decorator: Self, field: FieldShape) -> Result[FieldMetadata, DecoratorError]:
        Ok(FieldMetadata.max_len(decorator.value))

impl[T] FieldDecorator[list[T]] for MaxLen:
    fn attach(decorator: Self, field: FieldShape) -> Result[FieldMetadata, DecoratorError]:
        Ok(FieldMetadata.max_len(decorator.value))

struct User:
    id: UserId
    @MaxLen(320)
    email: string

    @MaxLen(5)
    tags: list[string]

    @MaxLen(12)
    age: i32       # compile error: MaxLen does not implement FieldDecorator[i32]
```

When a decorator is attached to a field of type `T`, the compiler checks that the decorator value implements `FieldDecorator[T]`. The `attach` function converts the decorator into static field metadata. This keeps decorators reusable and type-checked without letting them refine or change the field's underlying type.

An annotation facet can read that metadata:

```text
fn map_field(field: FieldShape) -> DatabaseColumn:
    max_len := field.annotation(MaxLen)?.value
    ...
```

Open decorator questions:

1. What is the exact constructor shorthand for decorators: `@MaxLen(320)`, `@MaxLen(value=320)`, or a lowercase helper such as `@max_len(320)`?
2. Should equivalent protocols exist for parameter, function, struct, enum, and variant decorators, and what should their trait names be?
3. How should multiple decorators of the same metadata type compose or conflict?

Candidate annotation style:

```text
struct User:
    id: UserId
    @email()
    @max_len(320)
    email: string
    @range(0..150)
    age: i32
```

Custom validators should be normal functions and can be referenced from annotation metadata:

```text
fn company_email(value: string) -> bool:
    value.ends_with("@company.com")

struct Employee:
    id: UserId
    @email()
    @max_len(320)
    @refine(company_email)
    email: string
```

Conceptual external validation override, not final syntax:

```text
Validation.derive(shape(Employee), overrides={
    email: string.email().max_len(320).refine(company_email),
    age: i32.range(18..150),
})
```

The same mechanism should work for other tooling facets:

```text
DatabaseSchema.derive(shape(User), overrides={
    userId: varchar(36).primary_key(),
    email: varchar(320).unique(),
    createdAt: timestamp(),
})

UI.derive(shape(User), overrides={
    userId: text,
    avatar: ProfileImage.rounded(size=40),
    email: link.mailto(),
})
```

Reusable validation pieces can be ordinary values/functions, not new type-level entities:

```text
CompanyEmail := string.email().max_len(320).refine(company_email)

Validation.derive(shape(Employee), overrides={
    email: CompanyEmail,
})
```

Tooling should generate runtime validators, JSON Schema, TypeScript types, valid test data generators, serializers/deserializers, and documentation from validated data types. Invalid data generation is not needed initially.

Open syntax issues:

1. Standard inline validation annotation set.
2. Exact external derivation/override block syntax.
3. Whether validator composition uses method chaining, pipes, nested calls, or blocks.
4. How annotation facets import and reuse validators from other files.
5. How generators handle custom validators.
6. Whether annotation facets are open-ended user-defined names or declared interfaces.

## Registration Annotation Direction

The language should be function-first. System registration should be attached to normal declarations through annotations or decorator-like metadata.

Candidate:

```text
@tool
@description "Fetch a user by ID."
@context AuthContext, TenantContext
fn get_user!(id: UserId) -> Result[User, ToolError] $ Database + access:
    access:
        require auth.can("user:read")
    db := $.use(Database)
    user := db.get_user!(id)?
    user
```

Metadata-heavy registrations can use an indented annotation block:

```text
@tool
    description "Fetch a user by ID."
    context AuthContext, TenantContext
    generate openapi, json_schema, mcp
fn get_user!(id: UserId) -> Result[User, ToolError] $ Database + access:
    access:
        require auth.can("user:read")
    db := $.use(Database)
    user := db.get_user!(id)?
    user
```

This keeps the implementation as a normal function while still giving the compiler enough metadata to generate tool specs, runtime registration, observability links, and access-control wiring.

Decisions:

1. Compact annotations should use one-line forms like `@tool`.
2. Metadata-heavy annotations should allow indented blocks.

Open syntax issues:

1. Which annotations are compile-time only versus runtime-visible.
2. How annotations compose with contracts, examples, and effects.
3. How registered functions are discovered across files and packages.

## Data Retention Direction

Struct data should be able to express retention, deletion, and cascade requirements declaratively. The behavior should not be hardcoded into the language as a specific policy; the language should provide syntax for expressing the requirement.

Retention should use the general derivation/override facet model, not a standalone retention syntax. Its payload should look like facet-specific field annotations, similar to validation annotations.

Conceptual model, not final syntax:

```text
struct User:
    id: UserId

struct Post:
    id: PostId
    userId: UserId

Retention.derive(shape(Post), overrides={
    userId: ownerId,
    policy: deleteWhen(User.deleted),
})
```

Here, `ownerId` and `deleteWhen` are not new language keywords. They are annotation terms provided by the `Retention` facet, similar to how validation might provide `email`, `max_len`, or `range`.

```text
Validation.derive(shape(User), overrides={
    email: email.max_len(320),
})

Retention.derive(shape(Post), overrides={
    userId: ownerId,
    policy: deleteWhen(User.deleted),
})
```

Open syntax issues:

1. Exact retention facet derivation/override syntax.
2. Whether facet annotation terms like `ownerId` and `deleteWhen` must be declared by the facet.
3. Which retention policy terms are needed, such as `deleteWhen`, `retainFor`, `archiveWhen`, or `anonymizeWhen`.
4. Exact policy expression syntax, such as `deleteWhen(User.deleted)`.
5. How retention annotations refer to related objects.
6. How retention rules compile to storage backends, workflows, or generated checks.
7. How retention rules interact with auth, audit logs, and observability.
8. Whether retention annotations can reference relationship annotations.

## Serializable Closures And Incremental Computation

Serializable closures should package a function reference with its captured environment so computation can be stored, moved, cached, or resumed. Incremental computation should track dependencies so cached results are reused and only affected computations are recomputed.

Possible annotation-style sketches:

```text
@serializable
fn make_followup(query: string) -> fn(Response) -> Prompt:
    prefix := "Answer using these facts:"
    fn(response: Response) -> Prompt:
        Prompt(prefix, query, response)
```

```text
@workflow
fn answer_question!(query: string) -> Answer $ llm + search:
    draft := llm.generate!(query)
    facts := search.web!(draft)
    llm.refine!(draft, facts)
```

```text
@cache
fn expensive_summary!(doc: Document) -> Summary $ model:
    model.summarize!(doc)
```

Runtime requirements:

1. Captured values must be serializable or rejected by tooling.
2. Captured effects and capabilities must remain visible.
3. Code identity and captured values can contribute to cache keys.
4. Workflow resumption can persist continuations at suspension points.
5. Non-deterministic effects must be handled for replay.
6. Incremental computation tracks dependencies between code, inputs, captures, data reads, and outputs.
7. Cache hits, invalidations, and recomputations should be observable.

Open syntax issues:

1. Whether serializable closures are inferred, annotated, or a distinct function type.
2. Whether caching uses `@cache`, an `annotate Cache for ...` facet, or a standard-library wrapper.
3. Whether workflows use `@workflow`, an annotation facet, or standard-library effects only.
4. How closure capture restrictions are displayed to reviewers.
5. How incremental dependencies are declared, inferred, or inspected.
6. How code identity is represented across JS and WASM targets.

## Syntax Questions To Decide Next

1. Should postconditions refer to the return value as `result`, `return`, or a named return variable?
2. Should old values use `old(x)`, `before(x)`, backticks like NimContracts, or another syntax?
3. What should the failed `require` and failed `ensure` effects be named?
4. Should every function with `require` or `ensure` explicitly list the contract effects, or should the effects be inferred from the sections?
5. What should user-defined effect and handler syntax look like?
6. What should inline validation annotation and external derivation/override syntax look like?
7. What should declarative data-retention and cascade syntax look like?
8. What should registration annotation/decorator syntax look like?
9. Which effect-polymorphism candidate should higher-order functions use, and what is deferred to later versions?
10. What is the exact `brand` spelling, and is brand-to-base coercion implicit or explicit?
11. What should serializable closure syntax and capture restrictions look like?
12. What should incremental computation syntax and cache/dependency tooling look like?
