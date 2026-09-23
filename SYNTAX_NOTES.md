# Syntax Notes

> Historical design log. This file preserves explored alternatives and may
> include superseded syntax inside sections that record earlier discussions.
> The maintained language surface is the [Language Tour](LANGUAGE_TOUR.md) and
> the normative [Formal Specification](spec/README.md).

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
2. `let`, with an optional type annotation, for named local declarations.

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

`let` introduces a local variable that may be reassigned. Type annotation is optional:

```text
let display_name: string = "Ada"
let nickname: string? = nil
let inferred = 1
let attempts: i32 = 0
let counter = 1

attempts = attempts + 1
counter = counter + 1
```

For composite values, mutation permission is part of the type. `T` provides const access and `mut T` provides mutable access. `mut` is always written in the type position, including local declarations. A const composite reference cannot be upgraded:

```text
user := User {
    id: "user_123",
    email: "ada@example.com",
    display_name: "Ada"
}

let alias: mut User = user  # error: User cannot become mut User
```

A mutable reference can be downgraded to a const view. The const view may observe changes made through an existing mutable alias:

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

This is deliberately different from value semantics, exclusive ownership, and global immutability. Multiple mutable aliases may exist, but mutable authority cannot be manufactured from a const reference.

Reasoning:

1. Most local code stays concise with `:=`.
2. `let` makes rebinding explicit without conflating it with reference permission.
3. Mutation through a composite value is review-relevant, so `mut` appears uniformly in its type.

An unannotated `let` infers the initializer's access type. A fresh composite initializer may infer `mut T`, but an existing `T` remains `T`; inference never upgrades const access.

A function returning mutable access must declare `-> mut T`. A return type of `T` exposes only const access, even if the function creates a fresh object internally. Callers use the declared return type; freshness does not propagate across a function boundary to recover mutation permission.

```text
fn new_user() -> User:
    User { name: "hi" }

fn new_mutable_user() -> mut User:
    User { name: "hi" }

fn borrow(u: User) -> User: u

fn change_name(u: mut User) -> void:
    u.name = "new"

immutable := new_user()
change_name(immutable)          # error: requires mut User
change_name(new_user())         # error: declared return type is User
change_name(borrow(immutable))  # error: declared return type is User
change_name(new_mutable_user()) # allowed: declared return type is mut User
```

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

Function generic parameters are erased at runtime by default:

```text
fn identity[T](value: T) -> T:
    value
```

Use `reified` when a function needs the concrete runtime type:

```text
fn runtime_shape[reified T]() -> TypeShape:
    shape(T)

shape := runtime_shape[User]()
```

The compiler implements a reified parameter by passing hidden runtime type metadata. The hidden descriptor is not part of the source-level argument list:

```text
# Conceptual lowering only; this is not source syntax.
fn runtime_shape[T](hidden type: Type[T]) -> TypeShape:
    shape(type)
```

Erased parameters cannot be used by runtime type operations or passed to reified parameters:

```text
fn resolve[reified T]() -> T $ TypeProvider:
    ...

fn resolved[reified T]() -> T $ TypeProvider:
    resolve[T]()

fn invalid_resolved[T]() -> T $ TypeProvider:
    resolve[T]()  # compile error: T is erased
```

The initial runtime-type operations requiring reification include:

1. `shape(T)` when `T` is a generic parameter.
2. Runtime annotation lookup for `T`.
3. Type-directed dependency injection such as `resolve[T]()`.
4. Runtime serialization or deserialization selected from `T`.
5. Runtime type tests or casts involving `T`, if those operations are added.

Concrete type expressions always have materializable descriptors, so `resolve[list[i32]]()` does not require the caller itself to be generic. A generic expression such as `resolve[list[T]]()` requires `T` to be reified.

Reified functions do not require an `inline` modifier. The WebAssembly backend can use descriptor passing, specialization, or both. It may erase an unused descriptor or specialize a concrete call only when observable reflection behavior remains unchanged.

In hd-lang, `reified` applies to function generic parameters. Reified parameters on generic struct, enum, trait, and type declarations remain a separate design question.

hd-lang does not support partial explicit generic arguments or placeholder generic arguments.

Variadic generics use ordered type and value packs. The language supports pack expansion in function types, vararg parameters, tuple types, call arguments, and type or expression patterns:

```text
fn call_with[Args..., R](f: fn(Args...) -> R, args: Args...) -> R:
    f(args...)
```

A pattern containing a pack can be repeated once for every pack element by placing `...` at its expansion position. This lets an ordinary library function preserve a pointwise relationship between heterogeneous inputs and outputs:

```text
fn all![Ts...](tasks: Suspend[Ts]...) -> (Ts...):
    ...
```

For `Ts... = User, i32, bool`, the parameter pattern expands to `Suspend[User], Suspend[i32], Suspend[bool]`, and the result type expands to `(User, i32, bool)`. The same rule applies to expression patterns in argument-list positions, such as `start(tasks)...`: the compiler repeats `start(task)` for each value in the `tasks` pack. Expansion is compile-time and does not turn the values into a runtime list.

If one repeated pattern references multiple packs, they expand positionally in lockstep and must have equal lengths. hd-lang does not support general pack mapping, filtering, indexing, splitting, or arithmetic. The scheduling and cancellation semantics of `all!` belong to the concurrency design; this example specifies only the variadic type relationship.

`all!` treats a child's `Err` as an ordinary completed value: it does not short-circuit or cancel siblings. It waits for every child to complete and returns their values, including any `Err` values. Runtime panics and cancellation are separate from this result-value rule.

`race!` returns the first completed child's value, including `Err`, and synchronously cancels the remaining children before returning. It does not wait for the first `Ok`. Tie-breaking between ready children remains unspecified.

Traits are explicit, not structural. A type does not implement a trait just because it has matching methods:

```text
trait Display:
    fn display(self) -> string

impl Display for User:
    fn display(self) -> string:
        self.email
```

Generic bounds such as `T: Display` use static dispatch. Trait value types such as `value: Display` use Go-style dynamic dispatch: the runtime value carries concrete data plus a method table. There is no `dyn` marker.

`Any` is the built-in universal empty trait, analogous to Go's `any`. Every non-optional value type satisfies it automatically:

```text
fn preserve[T: Any](value: T) -> T:
    value

fn keep_erased(value: Any) -> Any:
    value
```

`Any` is non-null. `nil` can only be stored in `Any?`, following the ordinary optional-type rule. An optional `T?` cannot erase to `Any`, but can erase to `Any?`. As with other traits, plain `Any` can be used as an erased dynamic trait value, while `T: Any` is a generic constraint that preserves the concrete type.

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
    pub id: UserId
    pub email: string
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

Import and re-export cycles are rejected.

Declarations are module-private by default, and `pub` makes them public. Enum variants inherit enum visibility; struct fields and inherent methods are private unless individually marked `pub`. There is no package-private visibility modifier.

## Program Entry Points And Wasm Exports

An executable package uses `pub fn main` as its conventional default entry point:

```text
import std.host.{Args, Console}

pub fn main!() -> Result[void, AppError] $ Args + Console:
    args, console := $.use(Args, Console)
    console.write_line!("starting " + args.program_name())?
```

`main` has no source-level parameters. Arguments, environment, I/O, and other host facilities are supplied as context requirements. It follows ordinary suspension naming: use `main!` only when its body can suspend. Its return type may be `void` or `Result[void, E]`; the generated host adapter maps `Err` to invocation failure.

`pub` only controls visibility between hd-lang modules. It does not export every public function through the Wasm component boundary. Tools, workflows, and library-facing functions require explicit registration, and that registration generates a typed host adapter. Their exact registration APIs are separate library/tooling designs.

An exported signature is checked recursively for boundary-safe structural types. The initial allowed forms are primitive scalars, `string`, tuples, `list[T]`, `map[K, V]`, structs, enums, `T?`, and `Result[T, E]`; every nested type argument, field, variant payload, success value, and error value must itself be boundary-safe. A map key must also satisfy the ordinary map-key rules. Mutable types, trait values, closures, and live runtime handles are rejected anywhere in the boundary shape. `$` requirements are bound by the host adapter and are not serialized parameters.

`map[K, V]` is unordered by default. Insertion and iteration order are not part of map equality or boundary semantics, even if a particular host encoding represents entries as a sequence.

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

There are no convenience aliases such as `int`, `uint`, or `float`. Use explicit-width numeric types. `decimal` is a standard-library type, not a primitive.

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
| `==`, `!=`, `<`, `<=`, `>`, `>=` | comparisons; no chaining  |
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

Named tuples are not supported. Use structs when field names are part of the meaning.

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

Composite fields may carry mutable reference permission in their type:

```text
struct Profile:
    display_name: string

struct Account:
    profile: mut Profile

let profile: mut Profile = Profile {
    display_name: "Ada"
}

account := Account { profile: profile }
account.profile.display_name = "Ada Lovelace"  # error: const root

let editable: mut Account = Account { profile: profile }
editable.profile.display_name = "Ada Lovelace"  # mutable root + mutable edge
```

Mutation through a composite path requires both:

1. A mutable root whose type is `mut T`, including a local, parameter, field projection, return value, or `mut self`.
2. `mut` permission on every composite field, list element, or map value edge crossed by the path.

An ordinary `field: T` is a const edge. A mutable outer root may replace that field slot but cannot mutate the referenced child through it. Initializing `field: mut T` requires a `mut T` value; `T` cannot be upgraded.

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

The qualifier composes in stored and callable types:

```text
friend: mut User
users: list[mut User]
users_by_id: map[string, mut User]
fn current_user() -> mut User
fn apply(user: mut User, operation: fn(mut User) -> void) -> void
```

`mut T` can be used where `T` is expected; `T` cannot be used where `mut T` is required. Generic declarations state variance with `+` and `-`: `+T` is covariant, `-T` is contravariant, and an unmarked `T` is invariant. Variance applies to read-only outer views. Every `mut Generic[...]` view is invariant in its generic arguments because mutation could otherwise store a value that violates the original type.

```text
struct Producer[+T]:
    produce: fn() -> T

struct Consumer[-T]:
    consume: fn(T) -> void

struct Cell[T]:
    value: T
```

Given the permission weakening `mut User` to `User`, the corresponding read-only conversions are:

```text
Producer[mut User] -> Producer[User]       # covariance
Consumer[User] -> Consumer[mut User]       # contravariance
Cell[mut User] -> Cell[User]               # invalid: invariant
```

The compiler checks declared variance against the read-only fields and methods. Return positions are positive, parameter positions are negative, and entering a function parameter reverses polarity. A parameter used in both directions must be invariant. Members requiring `mut self` do not participate in read-only variance, because mutable outer views never receive variance conversions.

Local declarations use `let name: mut T`; the former `let mut name: T` spelling does not exist. Receiver syntax remains `mut self`, shorthand for `self: mut Self`.

Lists and maps apply the root-and-edge rule uniformly:

```text
fn replace_users(users: mut list[User], replacement: User) -> void:
    users[0] = replacement       # allowed: mutable list slot
    users[0].display_name = "x"  # error: const User edge

fn edit_users(users: mut list[mut User]) -> void:
    users[0].display_name = "x"  # allowed: mutable root + mutable edge
```

The container root and element edge are independent:

```text
list[User]           # const container, const elements
list[mut User]       # const container, latent mutable element edges
mut list[User]       # mutable container, const elements
mut list[mut User]   # mutable container, mutable elements
```

The built-in `list` type declares a covariant element parameter, conceptually `list[+T]`. Therefore a read-only list view may weaken element permission:

```text
let stored: list[mut User] = ...
let visible: list[User] = stored            # allowed

let editable: mut list[mut User] = ...
let invalid: mut list[User] = editable      # invalid: mutable containers are invariant
```

The covariant conversion creates a read-only view, not an immutable snapshot. Changes made through another mutable alias remain observable. The `list` API must preserve its `+T` declaration; an operation that consumes an element through a read-only list view needs a separately type-safe signature rather than placing `T` directly in a negative position.

An ordinary generic parameter denotes a complete type, including any access modifier. Therefore an unconstrained generic declaration cannot apply `mut` again:

```text
struct Box[T]:
    value: T

Box[User]       # value: User
Box[mut User]   # value: mut User

struct InvalidBox[T]:
    value: mut T  # invalid: T may already be `mut U`
```

Mutable generic constraints qualify a trait bound. `T: mut Any` accepts any mutable root type; `T: mut Trait` accepts a mutable root whose underlying type implements `Trait`:

```text
trait Reset:
    fn reset(mut self) -> void

fn reset_value[T: mut Reset](value: T) -> void:
    value.reset()
```

The `mut` qualifier proves that `value` can call methods requiring `mut self`. `T` remains the complete inferred type, including its access permission. For example, `mut list[User]` satisfies `mut Any`, but `list[mut User]` does not because its root is const.

Trait values follow the same rule. `mut Trait` is a mutable dynamic trait view, and `mut Any` is an erased mutable composite reference. The qualifier preserves permission but does not invent operations: `mut Any` can only use universal runtime operations until checked as a concrete mutable type or passed somewhere with a stronger trait requirement.

Mutable map keys are disallowed because changing a structurally hashed key could invalidate map invariants. The formal type specification defines the closed set of supported map-key types; a stable user-defined equality/hash protocol remains backlog work.

### Alternative: Shallow Const Bindings And Implicit Const Borrows

An earlier, simpler design treated `:=` and plain `let` as shallow const bindings whose reference permission was not preserved when stored elsewhere. A value could be deliberately placed behind a mutable binding or container and then changed through that alias:

```text
user := User { ... }
let users: mut list[User] = [user]
users[0].display_name = "new"  # allowed in the alternative
```

Composite function parameters behaved like implicit C++ references, with `user: User` analogous to `const User&` and mutable permission written before the parameter name:

```text
fn rename(mut user: User, name: string) -> void:
    user.display_name = name
```

Fields had no `mut T` reference-permission type. This kept the surface smaller but could not precisely express mutation authority for nested fields, collection elements, map values, function values, or mutable returns. It also made local shallow constness and call-boundary constness follow different propagation rules. This model is retained as an alternative, not the current direction.

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

Payload variants are called like functions. Positional arguments come first,
followed by named arguments:

```text
error := ToolError.NotFound(resource="user_123")
```

Match arms use `pattern => expression`. Enum variant patterns may use `.Variant` when the matched value fixes the enum type; otherwise use the qualified enum name. Payload patterns use call-style parentheses:

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

### Iteration Protocol Direction

Iteration has two distinct trait roles:

- An ordinary `Iterable` source carries no per-traversal progress. Calling `iter()` creates a new traversal.
- `Iterator` owns the mutable cursor/progress for one traversal. Calling `next(mut self)` advances only that iterator.

Two iterators created from the same ordinary iterable therefore have independent progress, enabling repeated and nested traversal. An iterable may still contain data or refer to mutable data; it is only traversal-state-free, not necessarily an immutable or fieldless value.

Every iterator also implements `Iterable`. Its `iter()` returns the same mutable iterator rather than creating a fresh cursor. It neither clones nor resets traversal state, so a `for` loop over a partially consumed iterator continues from its current position and leaves that iterator exhausted when the loop completes.

Mutation during traversal, iterator invalidation, and possible fail-fast behavior are deferred. No semantics are selected yet.

Conceptually, the protocols have this shape:

```text
trait Iterator:
    type Item

    fn next(mut self) -> Self.Item?

trait Iterable:
    type Item
    type Iter: Iterator

    fn iter(self) -> mut Self.Iter
```

This sketch records the two-role model, not final associated-type constraint syntax. `for` loops and comprehensions consume these ordinary protocols rather than supporting only compiler-known collection types. Concurrent mutation of an underlying source during traversal remains unspecified.

Mutable collection access is written in the collection type:

```text
let attempts: mut list[i32] = []

attempts.append(1)
attempts.append(2)
```

Open syntax issues:

1. Which collection operations are methods, functions, or trait-provided behavior.
2. Exact associated-type syntax connecting `Iterable.Item`, `Iterable.Iter`, and `Iterator.Item`.
3. Whether and how an iterator value itself can be passed directly to `for` without a separate wrapper.
4. How source mutation during iteration is detected or specified.

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

### Trailing Blocks

A call may pass an indented block as its final argument when the final parameter is a zero-argument function. Ordinary positional and named arguments remain inside parentheses:

```text
result := when(a, b):
    compute_result()

transaction:
    save_user()
    write_audit_log()
```

The second form has no ordinary arguments, so it omits empty `()`. The compiler constructs the zero-argument closure and checks the block against the final parameter's expected function type, including its inferred return type, capture mutability, requirements, and suspension behavior.

Illustrative desugaring:

```text
result := when(a, b, fn():
    compute_result()
)
```

This sugar is restricted to zero-argument callbacks. A callback with parameters uses the ordinary explicit closure syntax:

```text
names := users.map(fn(user: User) -> string:
    user.name
)
```

Only one trailing block is allowed, and it always supplies the final function parameter.

Control flow is local to the generated callback. In particular, `return` exits that callback and supplies its return value; it does not return from the enclosing function:

```text
fn load(cached: User, use_cache: bool) -> User:
    user := transaction:
        if use_cache:
            return cached
        fetch_user()

    audit(user)
    user
```

The `return cached` above returns from the callback passed to `transaction`. Execution then continues with `audit(user)` in `load`. Non-local return through a trailing block is not supported.

Varargs accept zero or more positional arguments:

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

There is no separate short closure syntax. Use `fn(...) -> ...:` for closures. Same-line closure bodies are allowed when the body is a single expression:

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

Contextual typing also keeps multiple multiline callbacks inline. Parentheses delimit each closure expression, and the comma after `)` separates call arguments:

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

Here `aa` is inferred as `i32`, `bb` as `string`, and both callbacks return `void`. Without an expected function type, the closure must state its parameter and return types explicitly.

Shorthand argument closures such as `$0 + $1` are not supported; closures use named parameters.

Closures capture values from lexical scope. Closures that mutate captured locals have a mutable function type, written `mut fn(...) -> ...`. Calling a mutable closure requires the closure value itself to be mutable:

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

Closures that capture dependencies or capabilities carry those requirements in their function type. Serializable closures are a separate deferred design area; their capture and execution semantics have not been decided.

Overloads are not supported. Each function name resolves to one declaration in a scope.

Function with suspension and a context requirement:

```text
fn load_user!(id: UserId) -> Result[User?, DbError] $ Database:
    ...
```

This keeps suspension and required dependencies explicit.

Open questions:

1. Exact precedence and formatting rules for requirement-row expressions using `+`, `-`, and parentheses.

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

Receiver spelling is `self` or `mut self`; there is no reference receiver spelling. Primitive parameters are passed by value. Composite parameter permissions appear in the type position: `value: T` is const and `value: mut T` is mutable. `mut self` is shorthand for `self: mut Self`. Structs, tuples, lists, and maps are composite types.

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

Status: backlog material excluded from the language tour. The
examples in this contract section preserve earlier exploration, including
obsolete `$ require + ensure` notation, and are not accepted hd-lang syntax.

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
let low: i32 = 0
let high: i32 = items.len()

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

## Requirements And Suspension

Dependencies should be modeled as signature requirements, but they do not need a separate dependency declaration syntax. A dependency can be an ordinary trait or capability that appears in a function's `$` requirement row.

Normal error handling is not modeled as an effect. Errors use `Result[T, E]`.

### Decomposed Effect Model

hd-lang pragmatically decomposes concerns commonly handled by algebraic effects into three mechanisms. They are related and intentionally not fully orthogonal:

1. **One-shot suspension:** `fn!`, `Suspend[T]`, bang calls, polling, and cancellation describe where execution may suspend and how a runtime drives it. This is not a general continuation system and does not provide multi-shot resumption.
2. **Requirement checking:** `$` requirement rows let the compiler verify that every dependency a function may use is available. Requirements are not primarily an authority or host-capability model.
3. **Dependency injection:** contexts, providers, `$.use(...)`, and `$.with(...)` select concrete implementations that satisfy those requirements.

At explicit dependency and suspension boundaries, the same source can execute with different providers or runtime drivers, such as production, mock, sandbox, and replay implementations. Ordinary code outside those boundaries is not reinterpreted. Requirements and suspension points remain visible and compiler-checked across these executions.

This decomposition replaces a single catch-all effect operation or handler construct. The mechanisms cooperate, but none implies the others: dependency lookup does not suspend, suspension does not represent normal errors, and a dependency may be required without being authority-bearing. Ordinary traits define dependency interfaces; they are the existing foundation of this model, not a fourth effect mechanism.

The signature can summarize the required `$` row:

```text
fn load_user!(id: UserId) -> Result[User?, DbError] $ Database
```

Their distinct roles are expressed in the body, not by writing `use` or `raise` as kinds inside the function signature.

```text
db := $.use(Database)
user := db.get_user!(id)?
```

Body-level intent:

1. `$.use(Database)` retrieves the `Database` provider from the current context.
2. `db.get_user!(id)` is a possible suspension point.
3. Normal errors are returned as `Result[T, E]`, not receiverless control effects.

A suspending function must declare the `!` suffix in its function name, and callers use the same suffix at the suspension point:

```text
fn get_user!(id: UserId) -> Result[User?, DbError]:
    ...

user := db.get_user!(id)?
```

The declaration also introduces a cold computation constructor. Given:

```text
fn load_user!(id: UserId) -> Result[User, DbError]:
    user := fetch_user!(id)?
    Ok(user)
```

the two call forms differ deliberately:

```text
pending := load_user(id)   # Suspend[Result[User, DbError]]; body has not started
result := load_user!(id)   # construct and immediately drive the suspension
```

`fn load_user!(...) -> T` is source-level sugar for a function that constructs `Suspend[T]`. The compiler evaluates and captures ordinary call arguments when constructing the suspension, then lowers the function body into a resumable state machine. It does not lower to an eagerly executed ordinary function body that merely happens to return `Suspend[T]`. Inside that state machine, `fetch_user!(id)` drives the child suspension until it completes or suspends; if it suspends, the enclosing `load_user` state is saved and resumed later.

Conceptually, but not as normative source syntax:

```text
fn load_user(id: UserId) -> Suspend[Result[User, DbError]]:
    runtime.suspend_state_machine(...lowered body...)

load_user!(id)
# approximately: runtime.drive!(load_user(id))
```

`Suspend[T]` is the trait implemented directly by compiler-generated suspension frames. The driver polls a frame, receiving `Pending` or `Ready(T)`. `Poll[T]` describes the outcome of one poll, while `Suspend[T]` represents the computation that retains state between polls. The polling context provides a waker that requests another poll when progress is possible; the waker does not carry the result. No separate public `Continuation[T]` is required.

```text
enum Poll[T]:
    Pending
    Ready(T)

trait Suspend[T]:
    fn poll(mut self, context: PollContext) -> Poll[T]
    fn cancel(mut self) -> void
```

Suspension values use ordinary dynamic trait dispatch. The driver must obtain mutable access under the runtime exclusive-driving guard; this is not permission to upgrade arbitrary const references. The exact compiler/runtime access mechanism remains to be specified.

Each suspension represents one execution. Exclusive driving is checked at runtime, not through ownership or affine types: competing drivers and reentrant polling panic. Successive polls by the same driver while pending are valid. Starting another execution or polling a completed or canceled suspension panics. The suspension does not restart or cache a result for repeated driving.

Dependency providers are captured at construction. The caller must satisfy the suspending function's `$` requirements through its own signature or a local provider scope, even for an unbanged call that only constructs the suspension. Arguments and providers are captured without running the body; a later driving context cannot replace the captured providers.

```text
fn prepare_user(id: UserId) -> Suspend[Result[User, DbError]] $ Database:
    load_user(id)  # captures this caller's Database provider
```

This example assumes `load_user!` declares `$ Database`. Construction alone does not suspend, so `prepare_user` has no bang suffix. Durable replay reconstructs execution with rebound host providers rather than serializing provider handles.

Cancellation is synchronous, provisionally expressed as `cancel(mut self) -> void` on the polling protocol. The driver serializes cancellation with polling. Cancellation terminates execution, cancels owned active child operations, unregisters waits, and performs synchronous cleanup. Repeated cancellation is harmless; late wakes cannot restart the suspension. Source-level cleanup registration and resource lifetime rules remain in [Deferred Resource Cleanup And Scope Exit](#deferred-resource-cleanup-and-scope-exit). Cancellation does not introduce a cleanup keyword or asynchronous cleanup.

Cancelling `all!` or `race!` synchronously cancels every unfinished child. Once cancellation returns, those children cannot resume execution. Cancellation propagates through providers to actively abort underlying external operations, such as an HTTP request; merely ignoring their eventual results is insufficient. The synchronous cancellation path invokes the host/provider abort mechanism without waiting for remote acknowledgement. Aborting an operation does not undo effects already performed by a remote system.

The direct syntax for driving a stored suspension remains open. A separate `Task[T]` wrapper and its API are backlog work. Scheduling, structured scope representation, and runtime-panic policies for concurrency combinators also remain open.

### Compiling A Suspending Function

The compilation strategy is a stackless state machine. Split the function at possible suspension points, store locals that remain live across those points in a managed frame, and generate polling and synchronous cancellation paths. The unbanged constructor captures arguments and the required providers; the first poll enters the body. A child returning `Ready` permits the parent to continue within the same poll. A child returning `Pending` causes the parent to preserve its state and return `Pending` using the same waker.

The following lowering is non-normative and illustrative. Generated names, polling trait spelling, and `runtime` helpers are explanatory placeholders, not additional accepted source APIs. The compiler performs this transformation; it preserves ordinary runtime behavior rather than evaluating the body at compile time.

Source:

```text
trait Counter:
    fn next!(self) -> i32

fn adjusted!(base: i32) -> i32 $ Counter:
    counter := $.use(Counter)
    offset := base + 1
    value := counter.next!()
    value + offset
```

Illustrative generated frame and constructor:

```text
enum AdjustedState:
    New(base: i32, counter: Counter)
    Waiting(child: mut Suspend[i32], offset: i32)
    Done
    Cancelled

struct AdjustedFrame:
    state: AdjustedState

fn adjusted(base: i32) -> Suspend[i32] $ Counter:
    counter := $.use(Counter)
    AdjustedFrame {
        state: AdjustedState.New(base, counter)
    }
```

The generated frame implements `Suspend[i32]` directly and is returned through ordinary dynamic trait dispatch, without a separate suspension wrapper. Driver access and exclusive-driving checks are supplied by compiler/runtime machinery, whose exact representation remains open. Capturing `counter` here fixes the provider at construction time. Neither `base + 1` nor `counter.next()` runs until the first poll.

Illustrative generated implementation:

```text
impl Suspend[i32] for AdjustedFrame:
    fn poll(mut self, context: PollContext) -> Poll[i32]:
        while true:
            match self.state:
                AdjustedState.New(base, counter) =>
                    offset := base + 1
                    child := runtime.claim_child(counter.next())
                    self.state = AdjustedState.Waiting(child, offset)

                AdjustedState.Waiting(child, offset) =>
                    match runtime.poll_child(child, context):
                        Poll.Pending => return Poll.Pending
                        Poll.Ready(value) =>
                            self.state = AdjustedState.Done
                            return Poll.Ready(value + offset)

                AdjustedState.Done => panic("suspension already completed")
                AdjustedState.Cancelled => panic("suspension was cancelled")

    fn cancel(mut self) -> void:
        match self.state:
            AdjustedState.New(_, _) =>
                self.state = AdjustedState.Cancelled
            AdjustedState.Waiting(child, _) =>
                self.state = AdjustedState.Cancelled
                runtime.cancel_child(child)
            AdjustedState.Done => pass
            AdjustedState.Cancelled => pass
```

The child helpers preserve the same exclusive driver identity through nested polling. They do not create a task or imply that `Task[T]` has been accepted. Mutable access to the child payload above represents generated access through the mutable frame; the exact source-level pattern rules are not specified by this lowering.

The runtime guard around the frame rejects competing drivers and reentrant entry before calling generated code. Terminal-state checks prevent repeated execution. Cancelling before the first poll never starts the body; cancelling while waiting synchronously cancels the active child. The child provider unregisters its pending waits, and stale wakes cannot re-enter the cancelled execution. This example has no user-owned resources or cleanup declarations; those remain in [Deferred Resource Cleanup And Scope Exit](#deferred-resource-cleanup-and-scope-exit).

For example, polling `adjusted(10)` constructs its child once and saves `offset = 11`. If the child is pending, a later wake triggers another poll of that same child. When it returns `7`, the parent returns `Ready(18)`. Each further suspension point adds the necessary frame state and live locals. Loops reuse states, and a `Result` propagated by `?` completes with `Ready(Err(error))`; an ordinary error result is not cancellation.

For the Wasm GC backend, frames and captured language values use managed storage. A frame contains the state discriminator and the values needed for resumption; the compiler may optimize their layout. Durable workflow recovery reconstructs these frames by deterministic replay rather than serializing the frame or waker.

This is closer to Effect's service model, where the type tracks required services but service access happens in the program body, and to Kotlin-style functional effects where effectful operations are explicit suspension points.

Example:

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

struct MockDatabase:
    user: User

impl Database for MockDatabase:
    fn get_user!(self, id: UserId) -> Result[User?, DbError]:
        Ok(self.user)

mock_db := MockDatabase {
    user: User {
        id: UserId("user_123"),
        name: "Test User"
    }
}

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

`$.Context[Metrics + Cache]` is not a variadic generic. The `Metrics + Cache` part is an unordered requirement row, using the same composition shape as function `$` requirements. `$.context(Metrics=metrics, Cache=cache)` creates a reusable context value, `$.with(Database=mock_db, ...prod_context())` opens a lexical provider scope and spreads reusable providers, and `$.use(Database, Logger, Cache)` retrieves providers in the requested return order.

Requirement names in `$.context`, `$.with`, and `$.use` are requirement keys, usually trait or capability names, not ordinary named-argument labels.

Duplicate providers for the same requirement cannot coexist; during context construction or spread, later bindings win and the resulting context has one entry per key. If a required provider does not exist for a call, that is a compile-time error. The `$` namespace is special context syntax, not an ordinary value namespace.

Capabilities are ordinary dependencies. They use normal traits, `$` requirement rows, and context operations rather than a separate `capability` declaration or signature form:

```text
trait FileRead:
    fn read!(path: string) -> Result[string, FileError]

fn load_config!(path: string) -> Result[string, FileError] $ FileRead:
    files := $.use(FileRead)
    files.read!(path)
```

hd-lang compiles to WebAssembly using Wasm GC for managed language values, with WASI as the host boundary. Every authority-bearing capability provider originates at that boundary. A Wasm module cannot manufacture ambient external authority. A fake implementation may satisfy `FileRead` using in-memory data, while an implementation that accesses the host filesystem must receive that authority from its host-injected context. User code may wrap or narrow an injected provider. Security is enforced at the runtime/provider boundary, not by making capability requirements a distinct type-system concept.

An entry point's transitive `$` requirement row is the authoritative provider list:

```text
pub fn main!() -> void $ FileRead + Network:
    ...
```

The compiler derives and verifies requirements from the call graph. Manifests do not repeat a separate capability list; host configuration only grants and binds concrete, scoped providers to the derived keys. Every host-backed standard-library service is a trait requirement obtained through the context system; there are no ambient global service APIs. The official hd runtime implements every standard capability and injects only those granted to the invocation. Alternate hosts may implement a subset. Missing entry-point providers are reported before execution. Pure standard-library operations remain ordinary functions and require no context.

Open syntax issues:

1. Exact provider declaration syntax for production, tests, and package/app boundaries.
2. `Result[T, E]` ergonomics beyond `?` propagation and `Ok(value)` / `Err(error)` construction, including pattern matching.

Standard capability granularity is deferred until the standard library is implemented. Broad service traits and narrower least-authority traits should be compared against concrete APIs rather than selected as a standalone language rule.

## Requirement Polymorphism

Higher-order functions need a way to propagate the requirements of function-typed arguments. Without it, `map` either forbids callbacks with requirements or needs one copy per requirement combination:

```text
fn map(items: list[T], f: fn(T) -> U) -> list[U]   # what requirements does map have?
```

### Candidate A: Explicit requirement-row variables

```text
fn map[T, U, r](items: list[T], f: fn(T) -> U $ r) -> list[U] $ r
```

1. Fully explicit; aligns with requirements being visible in signatures.
2. General: supports several independent variables, stored function fields, and returned closures.
3. Verbose; AI and reviewers must thread `$ r` correctly, and variables must be declared so a typo of a requirement name cannot silently become a fresh variable.

Explicit requirement variables become more useful if a provider scope can satisfy and remove one requirement from the variable. This may use full row polymorphism or a smaller subtraction operation.

Candidate:

```text
fn provide_logger[r](callback: fn(string) -> void $ r) -> void $ (r - Logger):
    $.with(Logger=logger):
        callback("str")
```

Meaning:

1. `callback` may have any requirements in `r`.
2. `provide_logger` installs a provider for `Logger`.
3. The remaining requirement row is `r - Logger`.
4. If `callback` only requires `Logger`, the wrapper has an empty requirement row.
5. If `callback` requires `Logger` and `Database`, the wrapper requires `Database`.

The important idea is requirement-row transformation: a function can propagate all callback requirements except the ones it provides locally. This is useful even if the language does not expose full row polymorphism.

### Candidate B: Parameter-linked requirements

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
2. Conflicts with the decision that requirements are explicit in signatures; a reviewer reading source cannot see the callback requirements.

### Open questions regardless of candidate

1. Requirement subtraction/removal syntax: should provided requirements be written explicitly as `$ (r - Logger)` or inferred from provider scopes such as `$.with(Logger=logger):`?
2. Function-typed struct fields and returned closures: row variables, monomorphization, or disallowed ?
3. How do provider scopes installed at a call site interact with a polymorphic row?
4. Is the underlying model full row polymorphism, or a smaller requirement-variable system that only supports union and removal?

## Nominal Types With Validation Metadata

Validation metadata does not refine an existing static type. Reusable domain
identity uses the ordinary nominal newtype syntax together with an exact
annotation case; there is no separate `brand` declaration:

```text
type Email(string)

annotate Validation for Email:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.String(StringRules {
            min_len: 3,
            max_len: 320,
            contains: "@",
        })

struct User:
    id: UserId
    email: Email
```

`Email` is nominal because of `type Email(string)`, not because it has a
`Validation` annotation. The annotation supplies reusable metadata wherever
`Email` appears. Whether a validation library exposes only checked construction
or also permits direct `Email(value)` construction remains a library/API design
question; the core annotation mechanism does not silently change construction.

## Schema And Validation Direction

Validation should not create distinct static subtypes by default. A field like `string.max_len(50)` and `string.max_len(100)` should still have the same base static type, `string`; validation metadata is used for runtime checks, generated schemas, generated data, docs, and tooling.

## Generic Representation And Annotation Derivation

Annotation design should be split into four separate concerns before finalizing syntax.

### Common Representation

The compiler should expose a typed runtime representation for declarations. This is similar in spirit to Python's runtime introspection model (`__annotations__`, docstrings, signatures, defaults) and Scala's compile-time generic representation, while remaining compiler-generated and compiler-verifiable.

The common representation should cover at least:

1. Structs: name, fields, embedded structs, field types, docs, visibility, and attached metadata.
2. Enums: name, variants, constructor arguments, GADT result types, docs, and attached metadata.
3. Functions: name, parameters, return type, dependency requirements, suspension marker, docs, and parameter defaults.

Shape vocabulary:

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

Examples of the accepted syntax:

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

### Annotation Protocol

A promising direction is to model annotations as uniformly typed derivation protocols implemented by ordinary types. This is not final syntax; it records the current idea for further design.

Shape values should be usable as runtime values:

```text
shape(User)        # StructShape
shape(User.id)     # FieldShape
shape(JobStatus)   # EnumShape
shape(get_user)    # FnShape
```

Every annotation kind first implements the common annotation protocol. The protocol associates it with one uniform information type. Targets expose availability through an ordinary generic trait:

```text
trait Annotation:
    type Info

trait Annotate[A: Annotation]:
    fn info() -> A::Info
```

Ready and deferred annotation values use the same compiler/runtime-provided representation:

```text
opaque type AnnotationRef[T]

impl[T] AnnotationRef[T]:
    fn key(self) -> AnnotationKey
    fn is_ready(self) -> bool
    fn get(self) -> T
```

Conceptually, `AnnotationRef[T]` has `Ready(T)` and `Deferred(AnnotationKey, fn() -> T)` states, but those constructors are not part of normal source code. `get()` memoizes deferred resolution through the package annotation registry. Public `Facet::annotation(Target)` returns the facet's completed ordinary `Info`; structural derivation passes `AnnotationRef[Info]` internally so recursive edges can remain deferred.

Local metadata protocols correspond directly to member shapes. They are open traits so unrelated concrete values can coexist in one homogeneous dynamic-trait list:

```text
trait FieldMetadata[T]
trait VariantMetadata

trait StructAnnotator: Annotation:
    type FieldTarget

    fn map_field(
        self,
        field: FieldShape,
        type_metadata: AnnotationRef[Self::Info],
    ) -> Self::FieldTarget

    fn build(
        self,
        shape: StructShape,
        fields: Dict[string, Self::FieldTarget],
    ) -> Self::Info
```

For a field of type `T`, `annotate Target` expects `list[FieldMetadata[T]]`. Concrete values are coerced to that Go-style dynamic trait value type using ordinary trait conformance. `VariantMetadata` serves enum variants. Parameters do not have local metadata assignments; `FuncAnnotator` reads their shapes. `StructAnnotator`, `EnumAnnotator`, and `FuncAnnotator` remain the aggregate derivation protocols and perform child mapping before `build`.

The supported type set is open because new exact cases can be added without changing an existing annotator:

```text
annotate Validation for i32:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.I32

annotate Validation for string:
    fn build(self, shape: TypeShape) -> Validator:
        ...
```

There is no wildcard `annotate Facet for type` fallback. Each block contributes one exact type target. Generic annotation-target syntax such as a reusable case for every `list[T]` remains undesigned; until then, examples can annotate concrete instantiations such as `list[Entry]`.

### Annotation Lowering Model

Annotations do not introduce a separate semantic constraint system. Their semantics reduce to traits, implementations, ordinary values, and declaration shapes.

`annotate A for T` is structural syntax for the same conformance as `impl Annotate[A] for T`. It adds compiler-supported field or variant overrides while deriving all unmentioned members through the applicable struct or enum annotator:

```text
annotate UI for User:
    avatar = profile_image(size=40)

# Same coherence slot; full manual implementation:
impl Annotate[UI] for User:
    fn info() -> UI::Info:
        ...
```

The two forms cannot coexist for the same annotation/target pair. A direct `impl` constructs the complete information value; `annotate` is the ergonomic form for structural derivation with member-level overrides.

`annotate Target` is member-shape metadata sugar. It evaluates ordinary metadata values and attaches them to existing fields or variants:

```text
annotate User:
    email = [max_len(320), contains('@')]

annotate Entry:
    File = [variant_doc("A stored file")]
```

For `User.email: string`, the first right-hand side is contextually typed as `list[FieldMetadata[string]]`. `MaxLen` and `Contains` may be different concrete types while both satisfy that homogeneous dynamic trait type. The compiler verifies member names and metadata trait conformance, then stores each value on the corresponding `FieldShape` or `VariantShape`. The bracketed expression is an ordinary homogeneous list, not a special heterogeneous annotation bundle.

There is no decorator syntax. `annotate A for T: pass` requests default aggregate derivation with no result overrides; `annotate T` attaches member metadata that any aggregate annotator may inspect.

`pass` is the general no-op expression and evaluates to `void`. Its use in a no-override `annotate` body introduces no special runtime behavior.

Decorator syntax may be added later as optional locality sugar. A future field form such as `@max_len(320)` must lower exactly to the corresponding value in `annotate User`, use the same `FieldMetadata[T]` trait checking, and introduce no new annotation semantics. Decorators are not part of the current language design.

Generic code can use normal trait bounds to require annotation availability:

```text
# Generic constraints use ordinary generic impl and where-clause syntax.
fn validate[T: Annotate[Validation]](value: T) -> Result[T, ValidationError]:
    validator := Validation::annotation(T)
    validator.validate(value)
```

A stronger model in which arbitrary traits directly inspect an implementation target's structure remains an exploratory idea only. It is not required by the selected lowering model because `annotate` retains the structural override sugar.

### Bottom-Up Annotation Composition

Annotation derivation follows one strict composition direction:

1. An exact type annotation produces the type's metadata.
2. A field annotation combines that type metadata with annotations attached to the field, producing field metadata.
3. An enum variant combines its payload-field metadata with annotations attached to the variant, producing variant metadata.
4. A struct combines its field metadata with annotations attached to the struct, producing struct metadata.
5. An enum combines its variant metadata with annotations attached to the enum, producing enum metadata.

Conceptually:

```text
type_metadata := Facet::annotation_ref(field.type)
field_metadata := Facet.map_field(field, type_metadata)
variant_metadata := Facet.map_variant(variant, payload_fields)
struct_metadata := Facet.build(struct_shape, fields)
enum_metadata := Facet.build(enum_shape, variants)
```

Evaluation is strictly bottom-up. An enclosing annotator can inspect child metadata and its own attached annotation values, but a child annotator cannot depend on metadata from its enclosing struct, variant, or enum. The compiler should pass already-derived type metadata into `map_field`; the field mapper should not secretly restart type resolution.

The source relationship is:

```text
struct A:
    field: i32
    variant: Enum

enum Enum:
    Foo
    Bar

annotate A:
    field = [FieldDec]
    variant = [FieldDec]

annotate Enum:
    Foo = [VariantDec]

annotate StructDec for A: pass
annotate EnumDec for Enum: pass
```

The compiler requires `StructDec: StructAnnotator`, each field value to implement `FieldMetadata[T]` for that field's declared type, `EnumDec: EnumAnnotator`, and each variant value to implement `VariantMetadata`.

For UI, every field maps to a `ReactComponent`:

```text
impl Annotation for UI:
    type Info = list[ReactComponent]

impl StructAnnotator for UI:
    type FieldTarget = ReactComponent

    fn map_field(
        self,
        field: FieldShape,
        type_metadata: AnnotationRef[list[ReactComponent]],
    ) -> ReactComponent:
        if field.type == string:
            TextInput(field.name)
        else if field.type == bool:
            Checkbox(field.name)
        else:
            DefaultInput(field.name)

    fn build(self, shape: StructShape, fields: Dict[string, ReactComponent]) -> list[ReactComponent]:
        [for field in shape.fields => fields[field.name]]
```

`annotate` is the chosen special syntax for customizing a facet for a target:

```text
annotate UI for User:
    userId = ReactUserId

    fn build(self, shape: StructShape, fields: Dict[string, ReactComponent]) -> list[ReactComponent]:
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

Annotation facets are open across target types: `annotate Validation for i32`, `annotate Validation for string`, and `annotate Validation for Email` are independent exact cases. There is still at most one block for each exact facet/target pair.

Annotation blocks are global within a package. Any module in the package can request the materialized annotation value:

```text
schema := DatabaseSchema::annotation(User)
```

Libraries do not provide implicit annotation blocks for downstream applications. If a library explicitly provides `annotate Facet for ConcreteTarget`, that exact annotation is authoritative and cannot be overridden by downstream packages. If that exact facet/target pair is unclaimed, an application package can add it. A library's cases for other target types do not close the facet.

The right-hand side of a field override must typecheck as that annotation's uniform field target:

```text
let user_id_component: UI::FieldTarget = ReactUserId
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

impl Annotation for DatabaseSchema:
    type Info = TableSchema

impl StructAnnotator for DatabaseSchema:
    type FieldTarget = DatabaseColumn

    fn map_field(
        self,
        field: FieldShape,
        type_metadata: AnnotationRef[TableSchema],
    ) -> DatabaseColumn:
        max_len := field.metadata(MaxLen).map(
            fn(annotation: MaxLen) -> i32: annotation.value
        )

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

    fn build(self, shape: StructShape, fields: Dict[string, DatabaseColumn]) -> TableSchema:
        TableSchema {
            name: shape.name,
            columns: [for field in shape.fields => fields[field.name]],
        }
```

Overrides typecheck against the uniform target:

```text
struct User:
    id: UserId
    email: string
    display_name: string
    active: bool

annotate User:
    email = [max_len(320)]
    display_name = [max_len(80)]

annotate DatabaseSchema for User:
    # Optional result override. Without it, `map_field` reads MaxLen metadata.
    email = DatabaseColumn.TextColumn(name="email", max_len=320)
    active = DatabaseColumn.BoolColumn(name="is_active")
```

The compiler checks only that each right-hand side is a `DatabaseColumn`. It does not statically prove that a `string` field received a text column or that an `i32` field received an integer column. Annotation facets can still enforce stricter domain rules in their own constructors, `map_field`, validation hooks, or generated diagnostics, but the core annotation protocol stays uniform.

This is an intentional simplification. TypeScript mapped types can model shape-preserving field transforms, but in a stricter nominal language that likely requires HKT-like type functions, dependent record construction, or special compiler-generated HList/labelled-record machinery. hd-lang should avoid that in the initial annotation design.

Enums need the same idea for variants:

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
```

Function annotations use the same shape: a parameter mapping step plus `build`. In hd-lang, function parameter assignment overrides inside `annotate` are not supported; parameter customization should come from parameter annotations, parameter docs, or a whole-function `build` override.

```text
trait FuncAnnotator: Annotation:
    type ParamTarget

    fn map_param(self, param: ParamShape) -> Self::ParamTarget

    fn build(
        self,
        shape: FnShape,
        params: Dict[string, Self::ParamTarget],
    ) -> Self::Info
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

impl Annotation for Tool:
    type Info = ToolSpec

impl FuncAnnotator for Tool:
    type ParamTarget = ToolParam

    fn map_param(self, param: ParamShape) -> ToolParam:
        ToolParam {
            name: param.name,
            schema: JsonSchema::from_type(param.type),
            description: param.annotation(Description).map(
                fn(annotation: Description) -> string: annotation.text
            ),
        }

    fn build(self, shape: FnShape, params: Dict[string, ToolParam]) -> ToolSpec:
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
    fn build(self, shape: FnShape, params: Dict[string, ToolParam]) -> ToolSpec:
        spec := Tool::build(shape, params)
        ToolSpec {
            ...spec,
            name: "get_user",
        }
```

This is not valid:

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

The spelling `Facet::annotation(Target)` is the language syntax. Semantically, it means:

```text
shape_value := shape(Target)
fields := Dict.from_entries(
    [for field in shape_value.fields => (
        field.name,
        Facet.map_field(field, Facet::annotation_ref(field.type)),
    )]
)
result := Facet.build(shape_value, fields)
```

For functions and enums, the same pattern applies with parameters or variants:

```text
fn_shape := shape(get_user)
param_map := Dict.from_entries(
    [for param in fn_shape.params => (param.name, Tool.map_param(param))]
)
tool_spec := Tool.build(fn_shape, param_map)

enum_shape := shape(ToolError)
variant_map := Dict.from_entries(
    [for variant in enum_shape.variants => (
        variant.name,
        ErrorDoc.map_variant(
            variant,
            [for field in variant.fields => ErrorDoc.map_field(
                field,
                ErrorDoc::annotation_ref(field.type),
            )],
        ),
    )]
)
error_schema := ErrorDoc.build(enum_shape, variant_map)
```

If a package-local annotation block exists, the compiler/runtime applies its field overrides between generic mapping and whole-generation construction:

```text
user_shape := shape(User)
default_fields := Dict.from_entries(
    [for field in user_shape.fields => (
        field.name,
        DatabaseSchema.map_field(
            field,
            DatabaseSchema::annotation_ref(field.type),
        ),
    )]
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

### Complete Validation Derivation Example

The standalone source version is [`validation.hd`](validation.hd). The following example combines primitive and generic type mapping, struct fields, enum payload fields, nominal defaults, field metadata, and automatic recursion. `Validator` does not need a facet-specific `Ref` variant because references are represented uniformly by `AnnotationRef[Validator]`:

```text
struct StringRules:
    min_len: i32?
    max_len: i32?
    contains: string?

struct FieldRules:
    required: bool
    min_len: i32?
    max_len: i32?
    contains: string?

struct FieldValidator:
    position: i32
    name: string?
    target: AnnotationRef[Validator]
    rules: FieldRules

struct VariantValidator:
    name: string
    description: string?
    fields: list[FieldValidator]

enum Validator:
    Bool
    I32
    String(rules: StringRules)
    Optional(inner: AnnotationRef[Validator])
    List(item: AnnotationRef[Validator])
    Struct(name: string, fields: Dict[string, FieldValidator])
    Enum(name: string, variants: Dict[string, VariantValidator])
```

The field and variant annotations used below are ordinary values. Lowercase helpers construct them using functional annotation syntax:

```text
struct MinLen:
    value: i32

struct MaxLen:
    value: i32

struct Contains:
    value: string

struct VariantDoc:
    text: string

fn min_len(value: i32) -> MinLen: MinLen { value: value }
fn max_len(value: i32) -> MaxLen: MaxLen { value: value }
fn contains(value: string) -> Contains: Contains { value: value }
fn variant_doc(text: string) -> VariantDoc: VariantDoc { text: text }

impl FieldMetadata[string] for MinLen
impl FieldMetadata[string] for MaxLen
impl FieldMetadata[string] for Contains
impl VariantMetadata for VariantDoc
```

`Validation` is a zero-configuration annotation value. It implements the enclosing annotators for structs and enums, while field and variant annotations are independent ordinary values implementing their corresponding target-specific annotator traits:

```text
impl Annotation for Validation:
    type Info = Validator

annotate Validation for bool:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.Bool

annotate Validation for i32:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.I32

annotate Validation for string:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.String(StringRules {
            min_len: nil,
            max_len: nil,
            contains: nil,
        })

annotate Validation for string?:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.Optional(Validation::annotation_ref(string))

fn validation_field(
    field: FieldShape,
    type_metadata: AnnotationRef[Validator],
) -> FieldValidator:
    FieldValidator {
        position: field.position,
        name: field.name,
        target: type_metadata,
        rules: FieldRules {
            required: not field.type.is_optional(),
            min_len: field.metadata(MinLen).map(
                fn(annotation: MinLen) -> i32: annotation.value
            ),
            max_len: field.metadata(MaxLen).map(
                fn(annotation: MaxLen) -> i32: annotation.value
            ),
            contains: field.metadata(Contains).map(
                fn(annotation: Contains) -> string: annotation.value
            ),
        },
    }

impl StructAnnotator for Validation:
    type FieldTarget = FieldValidator

    fn map_field(
        self,
        field: FieldShape,
        type_metadata: AnnotationRef[Validator],
    ) -> FieldValidator:
        validation_field(field, type_metadata)

    fn build(
        self,
        shape: StructShape,
        fields: Dict[string, FieldValidator],
    ) -> Validator:
        Validator.Struct(name=shape.name, fields=fields)

impl EnumAnnotator for Validation:
    type FieldTarget = FieldValidator
    type VariantTarget = VariantValidator

    fn map_field(
        self,
        field: FieldShape,
        type_metadata: AnnotationRef[Validator],
    ) -> FieldValidator:
        validation_field(field, type_metadata)

    fn map_variant(
        self,
        variant: VariantShape,
        fields: list[FieldValidator],
    ) -> VariantValidator:
        VariantValidator {
            name: variant.name,
            description: variant.metadata(VariantDoc).map(
                fn(annotation: VariantDoc) -> string: annotation.text
            ),
            fields: fields,
        }

    fn build(
        self,
        shape: EnumShape,
        variants: Dict[string, VariantValidator],
    ) -> Validator:
        Validator.Enum(name=shape.name, variants=variants)
```

The Validation facet is open because each exact type annotation is independent. `Validation::annotation_ref(type)` enters the same cycle-aware resolver and returns an `AnnotationRef[Validator]`. Conceptually, resolution dispatches as follows:

```text
resolve(Validation, target):
    if has_concrete_annotation(Validation, target):
        return build_concrete_annotation(Validation, target)

    target_shape := shape(target)
    match target_shape:
        StructShape if target_shape.has_annotation(Validation) =>
            derive_struct(Validation, target_shape)
        EnumShape if target_shape.has_annotation(Validation) =>
            derive_enum(Validation, target_shape)
        _ => annotation_not_found(Validation, target)

resolve(Validation, string)
# calls the exact `annotate Validation for string` build

resolve(Validation, list[Entry])
# calls the exact `annotate Validation for list[Entry]` build
```

This resolver is conceptual compiler behavior, not a user-callable overloaded function.

Field and variant metadata is attached to shapes before their enclosing struct or enum annotator runs. Consequently, `validation_field` can read `MinLen`, `MaxLen`, and `Contains` values from `FieldShape`, and `Validation.map_variant` can inspect `VariantMetadata` values through `VariantShape` when needed.

A nominal type can provide a reusable explicit default without placing annotation syntax on a type expression:

```text
type Email(string)

annotate Validation for Email:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.String(StringRules {
            min_len: 3,
            max_len: 320,
            contains: "@",
        })
```

Every `Email` field therefore resolves through `Validation::annotation(Email)` unless the enclosing target's `annotate Validation for ...` block overrides that field result.

The same annotation handles recursion across structs and enums:

```text
struct Folder:
    name: string
    path: string
    note: string?
    owner: Email
    entries: list[Entry]

enum Entry:
    File(name: string, size: i32)
    Directory(folder: Folder)

annotate Folder:
    name = [min_len(1), max_len(120)]
    path = [contains("/")]

annotate Entry:
    File = [variant_doc("A stored file")]

annotate Validation for Folder: pass
annotate Validation for Entry: pass

annotate Validation for list[Entry]:
    fn build(self, shape: TypeShape) -> Validator:
        Validator.List(Validation::annotation_ref(Entry))
```

This follows the same target-specific annotation model throughout:

1. `annotate Validation for Folder: pass` derives `Annotate[Validation]` through `Validation: StructAnnotator`.
2. `Folder.name` expects `list[FieldMetadata[string]]`, so both `MinLen` and `MaxLen` must implement `FieldMetadata[string]`.
3. `annotate Validation for Entry: pass` derives `Annotate[Validation]` through `Validation: EnumAnnotator`.
4. `Entry.File` expects `list[VariantMetadata]`, so `VariantDoc` must implement `VariantMetadata`.
5. `annotate Validation for list[Entry]` adds the exact collection type needed by `Folder.entries`; it explicitly references the `Entry` validator.

No `Validation` metadata is placed directly on `Folder.entries` or `Entry.Directory.folder`; the enclosing annotators derive those payloads from their declared types through `Validation::annotation_ref`. Putting `Validation` into a field metadata list is rejected because it does not implement `FieldMetadata[string]`:

```text
struct Invalid:
    value: string

annotate Invalid:
    value = [Validation]  # compile error: Validation does not implement FieldMetadata[string]
```

When deriving `Validation::annotation(Folder)`, the resolver marks `(Validation, Folder)` active. It derives `string`, `Email`, and `list[Entry]`, then derives the `Entry` variants. At `Entry.Directory.folder`, resolving `Folder` re-enters the active key, so that field receives a deferred `AnnotationRef[Validator]`. All primitive, nominal, list, and non-recursive edges receive ready references. Once the outer `Folder` validator is built, the deferred reference resolves through the completed registry entry.

The attached annotations register the derived targets, and runtime code retrieves ordinary validator values explicitly:

```text
folder_validator := Validation::annotation(Folder)
entry_validator := Validation::annotation(Entry)
```

This mechanism is universal across annotation facets:

1. Resolution is keyed by `(facet, concrete target)`.
2. A completed key produces `AnnotationRef.Ready(target)` conceptually.
3. Re-entering an active key produces `AnnotationRef.Deferred(key, resolver)` conceptually.
4. Completed targets are memoized per package.
5. `Facet::annotation(Target)` returns the completed ordinary target; only the internal structural graph carries `AnnotationRef` values.

There is no `Annotation.reference` hook and no `Validator.Ref` case. An annotation opts into recursive structure by placing `AnnotationRef[Info]` wherever its own output graph can contain another information value. Struct and enum validation use the same `AnnotationRef[Validator]` representation.

Annotation resolution otherwise follows this order:

1. Use an exact `annotate Facet for ConcreteTarget` implementation when one exists.
2. Otherwise use an attached `StructAnnotator` or `EnumAnnotator` for aggregate declarations.
3. If neither applies, behavior remains unresolved. A statically visible missing-annotation policy is the current candidate below.

### Candidate Missing Annotation Policy

A missing child type annotation crosses the static/runtime boundary. `map_field` runs at runtime, so an error returned by `map_field` cannot become a compiler error. If a facet can choose whether missing child metadata is rejected during compilation or ignored, that choice must be visible statically before runtime derivation.

Candidate sketch, not a decision:

```text
trait MissingAnnotationPolicy

struct Require
struct Ignore

impl MissingAnnotationPolicy for Require
impl MissingAnnotationPolicy for Ignore

trait StructAnnotator: Annotation:
    type FieldTarget
    type MissingTypePolicy: MissingAnnotationPolicy

    fn map_field(
        self,
        field: FieldShape,
        type_metadata: AnnotationRef[Self::Info],
    ) -> Self::FieldTarget
```

Under this proposal, `Require` makes a missing child type annotation a compiler error. `Ignore` excludes that child before runtime `map_field` and `build` execute. `map_field` therefore keeps a non-optional `AnnotationRef[Info]` and never decides whether compilation succeeds.

One possible way to make the policy selectable is to carry it in the annotator's static type:

```text
struct Validation[Missing: MissingAnnotationPolicy]:
    ...
```

The names, marker representation, selection syntax, default policy, policy granularity, and applicability to structs, enums, fields, and variants are all unresolved. In particular, this does not adopt `Require` or `Ignore` as a default.

### Manual Deferral

An optional `lazy` field-metadata value remains a candidate for explicit deferral even when no cycle is detected:

```text
struct Document:
    related: list[Document]

annotate Document:
    related = [lazy]
```

`lazy` would be an ordinary `FieldMetadata[list[Document]]` value consumed only by annotations that support deferred references. It would not make the stored field lazy and would not change normal field access or type semantics. Automatic cycle detection remains the preferred default; the exact consumer scope remains unsettled.

Open concerns:

1. The exact aggregate method names are not settled: `map_field`, `map_variant`, `map_param`, and `build` still need naming review.
2. Reusable generic target syntax for cases such as every `list[T]` remains open. The current example uses the exact target `list[Entry]` rather than inventing generic annotation syntax.
3. Field and variant result types are uniform in the current model. This gives up static proof of field-type-specific override correctness in exchange for a much simpler type system.
4. `build` receives dictionaries keyed by field, variant, or parameter name. If output ordering matters, `build` should use the original `shape` ordering.
5. Function parameter assignment overrides are deferred; parameter customization uses parameter annotations/docs or whole-function `build`.
6. Runtime annotation materialization uses `Facet::annotation(Target)`.
7. The internal representation and lifecycle of `AnnotationRef[Info]` remain compiler/runtime details.
8. Which compatible aggregate annotators consume manual `lazy` metadata remains open.
9. Whether a statically visible `MissingAnnotationPolicy` should select compile-time rejection or omission for missing child type annotations remains open, including its default and granularity. Runtime `map_field` cannot make this choice.
10. Decorator locality sugar is not part of the language.

Support two `annotate` forms:

1. `annotate Target` attaches metadata values to existing member shapes.
2. `annotate Annotation for Target` generates `Annotate[Annotation]` with optional structural result overrides.

### Annotation Metadata

`annotate Target` member metadata is distinct from `annotate Annotation for Target` derived information, but both reduce to ordinary values, traits, implementations, and shapes.

Annotation principles:

1. Metadata does not alter behavior, declaration names, underlying types, or signatures.
2. `annotate Target` can assign metadata only to existing fields or variants.
3. A field of type `T` expects `list[FieldMetadata[T]]`; variants expect `list[VariantMetadata]`. Parameters have no local metadata assignment syntax.
4. These are homogeneous collections of Go-style dynamic trait values. Their concrete elements may have unrelated types.
5. Metadata expressions create ordinary runtime values evaluated in a restricted metadata phase.
6. Constructor calls, helper function calls, named values, and reusable lists are equivalent when their values implement the required metadata trait.
7. Metadata attaches to declaration shapes, not to type expressions. There is no annotated type syntax.
8. `annotate Annotation for Target` produces the same conformance as `impl Annotate[Annotation] for Target`; `pass` means no structural result overrides.

Annotation values are not normal application side effects and not compiler magic. The language should have a metadata evaluation phase:

```text
source code -> typed shapes -> metadata evaluation -> derived artifacts -> runtime program
```

Metadata evaluation can construct ordinary hd-lang values such as `MaxLen { value: 320 }`, `max_len(320)`, named metadata values, reusable metadata lists, and annotation information values. The phase must be deterministic, sandboxed, and capability-limited so tooling can run it without running the application. Shape values expose their attached dynamic metadata values directly.

In hd-lang, metadata evaluation is intentionally strict:

1. It can construct structs/enums, call pure functions, compose annotation values, inspect shape values, and use constants.
2. It cannot use `$` context requirements.
3. It cannot call suspending functions marked with `!`.
4. It cannot perform IO, network access, database access, time reads, random generation, or other nondeterministic work.
5. It cannot rely on mutation that escapes the metadata evaluation.

Metadata constructors, helper functions, and aggregate annotator methods must therefore be pure, non-suspending, and dependency-free.

Metadata evaluation follows a strict bottom-up order. Child declarations attach metadata before their enclosing declaration attaches metadata:

```text
field metadata -> struct annotator -> Annotate information
parameter shapes -> function annotator -> Annotate information
variant-field metadata -> variant metadata -> enum annotator -> Annotate information
```

For structs:

1. Type-check the struct and field shapes.
2. Contextually type each field's assigned values as `list[FieldMetadata[T]]`.
3. Evaluate those values and store them on `FieldShape`.
4. Run the selected `StructAnnotator`, which can inspect field metadata.
5. Build the annotation's `Info` and expose `Annotate[A]` for the struct.

For functions, parameter shapes are assembled before a `FuncAnnotator` maps parameters and builds function information.

For enums, payload-field metadata is attached before variant metadata, and both are available before an `EnumAnnotator` maps variants and builds enum information.

Parent annotators cannot retroactively change child metadata values. Defaults and conventions belong in the aggregate mapping/build process after child metadata is available. This keeps evaluation acyclic and predictable.

#### Illustrative Desugaring

The compiler does perform annotation lowering/desugaring, but this desugared form is not normative. The exact generated helper names, memoization implementation, storage strategy, and internal APIs are compiler/runtime details. The purpose of this sketch is only to explain the semantics in hd-lang-like syntax.

The source-level model remains:

1. `annotate Target` evaluates ordinary member metadata values in the restricted metadata phase.
2. The compiler contextually checks each collection against `FieldMetadata[T]` or `VariantMetadata` for the selected member.
3. The compiler lowers those values into bottom-up shape metadata construction.
4. `annotate Annotation for Target` is type-checked against the target shape and generates `impl Annotate[Annotation] for Target`.
5. `pass` means default aggregate derivation with no result overrides.

Source:

```text
struct User:
    id: UserId
    email: string

annotate User:
    email = [max_len(320), description("Company email")]

annotate DatabaseSchema for User:
    email = DatabaseColumn.Text(name="email", max_len=320)
```

Illustrative compiler-generated metadata-phase code:

```text
fn __meta_User_id() -> Result[FieldShape, AnnotationError]:
    base := FieldShape.base(
        parent="User",
        name="id",
        type=type(UserId),
    )

    Ok(FieldShape.with_metadata(base, metadata=[]))

fn __meta_User_email() -> Result[FieldShape, AnnotationError]:
    base := FieldShape.base(
        parent="User",
        name="email",
        type=type(string),
    )

    Ok(FieldShape.with_metadata(
        base,
        metadata=[max_len(320), description("Company email")],
    ))

fn __meta_User() -> Result[StructShape, AnnotationError]:
    id_field := __meta_User_id()?
    email_field := __meta_User_email()?

    StructShape.base(
        name="User",
        fields=FieldShapes.of(id_field, email_field),
    )
```

Illustrative lowering for facet materialization:

```text
fn __annotation_DatabaseSchema_User() -> Result[TableSchema, AnnotationError]:
    shape := __meta_User()?

    let fields: mut Dict[string, DatabaseColumn] = Dict[string, DatabaseColumn].empty()
    for field in shape.fields:
        type_metadata := DatabaseSchema::annotation_ref(field.type)
        fields[field.name] = DatabaseSchema.map_field(field, type_metadata)

    # From `annotate DatabaseSchema for User`.
    # The compiler has checked that `email` is an existing field of `User`,
    # and that the right-hand side is a `DatabaseColumn`.
    fields["email"] = DatabaseColumn.Text(name="email", max_len=320)

    Ok(DatabaseSchema.build(shape, fields))

# A source request such as `DatabaseSchema::annotation(User)` uses this generated path.
fn __materialize_User_table() -> Result[TableSchema, AnnotationError]:
    __annotation_DatabaseSchema_User()
```

Function and enum annotations lower the same way: parameter shapes are built before function information, and variant-field metadata is built before variant metadata and enum information. The compiler-generated form is equivalent to the source annotation semantics; it does not give user code extra effects or let metadata evaluation escape its restricted phase.

```text
fn load_policy!() -> Policy $ Database:
    ...

struct User:
    email: string

annotate User:
    email = [Policy(load_policy!())]  # compile error: metadata evaluation cannot suspend or use Database
```

Example:

```text
struct MaxLen:
    value: i32

impl FieldMetadata[string] for MaxLen
impl[T] FieldMetadata[list[T]] for MaxLen

struct User:
    id: UserId
    email: string
    tags: list[string]
    age: i32

annotate User:
    email = [max_len(320)]
    tags = [max_len(5)]
    age = [max_len(12)]  # compile error: MaxLen does not implement FieldMetadata[i32]
```

When metadata is assigned to a field of type `T`, the compiler expects `list[FieldMetadata[T]]`. This keeps metadata reusable and type-checked without letting it refine or change the field's underlying type.

Reusable metadata groups are ordinary homogeneous lists of dynamic trait values:

```text
let email_metadata: list[FieldMetadata[string]] = [
    max_len(320),
    min_len(3),
]

struct User:
    email: string

annotate User:
    email = email_metadata
```

`T` is selected from the annotated field and supplies the list's expected dynamic trait type. Multiple entries with the same concrete metadata type remain banned.

An annotation facet can read that metadata:

```text
fn map_field(
    field: FieldShape,
    type_metadata: AnnotationRef[TableSchema],
) -> DatabaseColumn:
    max_len := field.metadata(MaxLen).map(
        fn(annotation: MaxLen) -> i32: annotation.value
    )
    ...
```

Runtime/tooling can also inspect the attached values directly:

```text
field := shape(User.email)
metadata := field.metadata
```

Custom validators should be normal functions and can be referenced from annotation metadata:

```text
fn company_email(value: string) -> bool:
    value.ends_with("@company.com")

struct Employee:
    id: UserId
    email: string

annotate Employee:
    email = [email(), max_len(320), refine(company_email)]
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

The language should be function-first. System metadata should be attached to normal declarations through `annotate` blocks.

Candidate:

```text
"""Fetch a user by ID."""
fn get_user!(id: UserId) -> Result[User, ToolError] $ Database + access:
    access:
        require auth.can("user:read")
    db := $.use(Database)
    user := db.get_user!(id)?
    user

annotate Tool for get_user: pass

tool_registry.register(Tool::annotation(get_user))
```

This keeps the implementation as a normal function while giving `FuncAnnotator` enough shape information to produce a `ToolSpec`. Registration remains an explicit user operation; annotation does not discover or register the function.

Annotation applicability is independent of declaration visibility. `annotate Tool for get_user` may target a module-private or `pub` function, `FuncAnnotator` imposes no visibility requirement, and annotation does not make the function public.

Decisions:

1. A no-override function annotation uses a same-line `pass`, such as `annotate Tool for get_user: pass`.
2. Registration annotations do not alter or constrain normal declaration visibility.
3. Registration remains explicit after annotation information is retrieved.

Open syntax issues:

1. How function member metadata is assigned to parameters through `annotate function`.
2. How tool information composes with contracts and effects.
3. How explicit registries select tools for external exposure.

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

## Observability Runtime Model

Automatic instrumentation is limited to semantic boundaries rather than every function call. Compiler- or library-generated adapters for entry points, tools, RPC, workflows, cells, suspending calls, and host-provider boundaries explicitly require `Observability`. The wrapped business function does not acquire that requirement unless its own body emits telemetry; the generated adapter is the registered/deployed executable, so `Observability` remains visible in the transitive requirement graph.

The runtime keeps execution-local observability context containing the current span, scoped log fields, trace links, and propagated trace context. Child tasks fork this context, lexical scopes restore it, and suspension/resumption preserves it. Boundary lowering follows this conceptual sequence:

1. Resolve the explicit `Observability` provider.
2. Create a span using the current span as parent plus compiler-generated operation metadata.
3. Install the new span in the execution-local context.
4. Execute the wrapped operation and retain its complete exit.
5. Restore the previous context and end the span from that exit.

Explicit logs are enriched from the execution-local context and are also recorded as events on the current span. Span lifecycle replaces duplicate boundary start/end logs. Unhandled errors, defects, retries, cancellation, and failed suspensions produce automatic runtime events. Exact trait methods, custom-span source syntax, metric instruments, privacy, sampling, replay deduplication, and exporter configuration remain open.

The initial provider surface is:

```text
trait Observability:
    fn sample(self, candidate: SpanCandidate) -> bool
    fn emit(self, event: Observation) -> void
```

`Observation` is a normalized enum with `SpanStarted`, `SpanEnded`, `Log`, `Metric`, and `Runtime` cases. The runtime owns trace/span IDs, parent selection, current-span installation, complete-exit mapping, context restoration, enrichment, and replay suppression. Providers implement policies and sinks such as console formatting, in-memory test recording, fan-out, filtering/redaction, sampling, or buffered OpenTelemetry export.

`emit` is non-suspending and best-effort. Production exporters enqueue locally and export in a runtime-managed background worker; observability failure cannot alter application results. Reliable audit delivery is a separate dependency with normal `Result` and suspension behavior.

Each observation has a stable identity derived from execution ID, boundary ID, attempt, and event kind. Replaying an already completed history event does not emit its observations again. Exact custom-span syntax, metric instruments, observation scalar representation, privacy rules, and exporter configuration remain open. The `sample + emit` interface is an initial draft and may be optimized.

## Deferred Resource Cleanup And Scope Exit

This entire topic is backlog material. The language has not selected `Drop`, `using`, `with`, `defer`, or `errdefer` syntax.

The central distinction is where cleanup belongs:

- RAII/`Drop` and `using` attach cleanup to a resource value and its lifetime. This gives the type system, generic constraints, analyzers, and tooling a protocol to inspect and enforce.
- `defer` attaches an arbitrary action to exit from a lexical control-flow scope. The action can capture local mutable state and need not correspond to a resource type.
- Python-style `with` gives a manager control over entry and exit around a block, including the opportunity to transform the entered value and inspect exceptional completion.

There is an intentional asymmetry. A statement-form `defer` can invoke any cleanup protocol directly. A protocol can represent arbitrary scope-exit work only by wrapping a closure in a general guard or stack, which recreates defer as a library abstraction. The two approaches may be computationally equivalent with such adapters, but they are not equally direct or equally enforceable.

Concrete cases where direct `defer` avoids protocol ceremony include:

```text
# Read local state after it has changed.
started := clock.now()
defer:
    metrics.record(clock.now() - started, attempt, cache_hit)
```

```text
# Restore a mutation rather than release a resource.
old := config.verbose
config.verbose = true
defer:
    config.verbose = old
```

Other cases are conditional or runtime-sized cleanup registration, outcome-dependent commit or rollback, staged construction that transfers completed resources on success, and several independent cleanups interleaved with ordinary logic. Protocol-based systems can express these with scope guards, context managers, exit stacks, disposable stacks, or explicit ownership-transfer operations, but each requires a reusable abstraction or closure-backed adapter.

Most desirable uses still pair setup with teardown; the setup is simply not always resource construction. Unpaired patterns such as panic recovery, modifying a pending return value, or implicitly rewriting an error are separate control-flow features and are not accepted motivations for hd-lang.

A candidate hybrid for later discussion is protocol-owned RAII/`Drop` as the foundation for real resources, plus Zig-style lexical `defer` and possibly `errdefer` as escape hatches for ad-hoc restoration and rollback. Go-style function-scoped defer is disfavored because registration inside a loop delays cleanup until the whole function exits. This candidate is recorded for comparison, not accepted.

Any later design must specify interaction with `Result` and `?`, cleanup failures, mutable captures, alias escape, suspending `!` cleanup, cancellation, and deterministic replay. A live non-serializable resource may also need to be prohibited from crossing a durable suspension boundary.

### Resource Escape And Use After Disposal

The current type system does not cover resource leakage through aliases. `mut` expresses write permission only; it does not express ownership, lexical lifetime, open/closed state, or responsibility for disposal. Consequently, either a future `defer` or `using` design could allow an alias to outlive the resource scope:

```text
let global_file: File? = nil

fn publish_file() -> void:
    file := File.open("data.txt")
    defer:
        file.close()

    global_file = file

# In another function returning an optional value, after `publish_file`
# has closed the file:
file := global_file?
file.read()  # should be rejected or fail explicitly as already disposed
```

The same problem exists with protocol-owned cleanup:

```text
fn publish_file() -> void:
    using file = File.open("data.txt")
    global_file = file
```

Both snippets are illustrative backlog syntax, not accepted hd-lang programs. The optional global is explicitly unwrapped before the later read, so the intended issue is use after disposal rather than optional-value handling. Lexical cleanup guarantees that one cleanup action runs; by itself it does not invalidate or find aliases stored in globals, fields, containers, returned values, or closures. Garbage collection also cannot provide prompt deterministic release.

Potential solution families remain open:

1. Resource-only affine ownership, where a scoped resource cannot be copied and an explicit transfer moves cleanup responsibility elsewhere.
2. Region or scoped-lifetime types that prevent a resource reference from escaping its lexical region.
3. Typestate such as `OpenFile` and `ClosedFile`, combined with alias restrictions strong enough to update every usable reference.
4. A shared runtime handle whose operations detect disposal and return a typed `Result` error. This catches use after disposal at runtime but not at compile time.
5. Scoped callback APIs whose resource type cannot escape the callback, requiring some form of higher-ranked or region-polymorphic typing.

This problem is independent of choosing `defer`, `using`, or `with`: all three need an ownership, lifetime, or runtime-state policy if hd-lang intends `global_file.read()` to be a compile-time error. It also raises the question of whether a narrow ownership discipline should apply only to external resources even though general hd-lang values retain shared-reference semantics.

## Serializable Closures And Incremental Computation

Serializable closures and incremental computation are deferred design areas rather than active syntax work. Serializable closures are intended to make captured computation storable or movable, but "a function reference plus its captured environment" is only a motivation, not an accepted semantic definition. Incremental computation should track dependencies so derived results can be reused and only affected computations are recomputed.

### Serializable Closure Semantics Backlog

The semantics must be fixed before choosing inference, a distinct function type, an annotation, a modifier, or a wrapper such as `Serializable[fn(...)]`. No such API or syntax has been accepted.

The backlog must define:

1. Whether capture serialization takes a value snapshot, preserves identity and aliasing, or rejects captures where the distinction is observable.
2. Whether mutable captures are forbidden, frozen, independently copied, or restored as shared mutable state.
3. Representation of nested graphs, repeated references, cycles, trait values, and nested closures.
4. Stable code identity and compatibility across source changes, compiler versions, deployments, and WebAssembly runtime versions.
5. Treatment of erased and `reified` generic arguments in stored identity and execution.
6. Whether requirements, capabilities, authorization, and provider state are captured, rebound, or prohibited. Live runtime resources need an explicit rule rather than an assumed exception.
7. The boundary between compile-time capture rejection and runtime serialization failure for dynamically typed or abstract values.
8. Delivery, replay, cancellation, expiry, idempotency, and result-compatibility guarantees.
9. Sandbox validation and trust of stored code identities and captured data.
10. Schema evolution and migration for captures and results.

Durable workflow replay already has its own semantics and does not depend on serializing closures or the execution stack. Incremental computation and cross-run caching likewise remain distinct concerns.

### Research Findings

Incremental computation has a large enough semantic and runtime surface that it should not be conflated with either durable replay or a generic function cache:

1. An incremental query produces derived data that may be invalidated and recomputed when one of its tracked inputs changes.
2. A cross-run cache reuses a result only while its complete code, argument, capture, provider, and external-dependency identity remains valid.
3. Durable replay restores a historical result belonging to one logical execution. That result remains authoritative for the run even if the code or external data has since changed.

A suspending `!` call only means that execution may suspend. It does not say whether the operation is deterministic, read-only, idempotent, or cacheable. Similarly, `$ Database` says that a `Database` provider must be available; it does not identify the provider implementation, authorization scope, database snapshot, or rows read.

The strongest prior-art direction combines Salsa/DICE-style dynamic query dependencies and equality cutoff, Jane Street Incremental-style transactional stabilization, Bazel/Nix-style explicit external action identity, and Shake-style dependency diagnostics. Verse's planned live variables provide another useful lesson: actual reads can determine dynamic dependencies, but automatically repeated computation must be sharply restricted from writes and suspension.

### Initial Library Boundary

Incremental computation should initially be a native-feeling `std.incremental` library rather than a new language construct or `incremental` keyword. The library owns inputs, computation nodes, observation, update transactions, stabilization, equality policies, storage strategies, and graph queries. Runtime library support maintains the active dependency recorder, while existing function shapes and tooling expose code identity and source metadata.

The computation callback uses ordinary hd-lang write-purity rules. It must be a plain, non-suspending `fn`, not a `mut fn` or `fn!`, and it must have no `$` requirements, mutable parameters, or mutable captures. Calls made by the callback must satisfy the same constraints transitively. This is normal function-type checking rather than an incremental-specific compiler rule. Local mutation of newly created, non-escaping values remains allowed because it has no externally observable effect.

Write purity alone does not make a callback referentially stable. A const reference may observe an object changed through an existing mutable alias between evaluations. Incremental computations must therefore read changing shared state through tracked inputs or require a separately defined stable/immutable input; that exact library constraint remains open.

Illustrative library usage, not accepted final API:

```text
let price = incremental.input(100)
let quantity = incremental.input(2)

total := incremental.compute(fn() -> i32:
    price.get() * quantity.get()
)

view := incremental.observe(total)

incremental.update(mut fn() -> void:
    price.set(120)
    quantity.set(3)
)

incremental.stabilize()
println(view.get())
```

Inputs and computation regions are explicit. Calls to tracked `get` operations inside a computation record the dependencies actually read, including transitive reads through ordinary pure helper functions. When control flow changes, successful recomputation atomically replaces the node's previous dependency set.

### Initial Runtime Direction

The initial model should investigate:

1. Transactional source updates so observers never see a mixture of old and new upstream values.
2. Lazy recomputation of demanded nodes in topological order.
3. Equality-based propagation cutoff: if recomputation produces an equal value, unaffected downstream nodes remain valid.
4. A DAG by default, with cycles rejected as soon as graph construction or evaluation discovers them and diagnostics showing the complete cycle path.
5. Explicit fixed-point or relational computation as a separate future API rather than implicit cyclic evaluation.
6. Whole-value tracking initially, with keyed list/map granularity considered later.
7. Observation-driven lifetime so unobserved graph regions can eventually be collected.

External data cannot silently participate in a correct persistent cache. A tracked file, database query, HTTP response, clock, environment value, or other external input must provide a stable revision, content digest, ETag, snapshot, logical timestamp, or equivalent dependency token. An opaque operation is volatile and prevents persistent reuse of the enclosing result. Freshness mechanisms such as TTL, manual invalidation tags, and stale-while-revalidate are policies layered above dependency correctness, not substitutes for it.

Workflow replay remains independent. Replaying a completed `!` event must not consult incremental cache freshness, and invalidating an incremental node must never automatically repeat an external workflow action.

### Inspection Requirements

The runtime should expose a stable machine-readable graph containing node and code identity, source location, value type, observed dependencies and dependents, external input identities and versions, clean/dirty state, revision, cache/storage state, last duration, and invalidation cause. Human and AI tooling should be able to answer:

- Why was this node recomputed?
- Why was it not recomputed?
- Which input or code revision invalidated it?
- Which conditional branch changed its dependency set?
- What keeps this node alive?
- What contributes to its cache key?

These findings establish an architectural boundary, not a complete design. Exact API spelling, identity and fingerprint protocols, storage tiers, persistence and distribution, collection granularity, node lifetime, cycle handling, and observability integration remain open.

Earlier syntax sketches, retained only as non-normative historical examples:

```text
fn make_followup(query: string) -> fn(Response) -> Prompt:
    prefix := "Answer using these facts:"
    fn(response: Response) -> Prompt:
        Prompt(prefix, query, response)

annotate Serializable for make_followup: pass
```

```text
fn answer_question!(query: string) -> Answer $ llm + search:
    draft := llm.generate!(query)
    facts := search.web!(draft)
    llm.refine!(draft, facts)

annotate Workflow for answer_question: pass
```

Related runtime constraints already identified, but not yet a complete serializable-closure model:

1. Captured values must be serializable or rejected by tooling.
2. Captured effects and capabilities must remain visible.
3. Code identity and captured values can contribute to cache keys.
4. Durable workflow resumption initially uses deterministic replay from the entry function over an append-only event history; it does not serialize the machine stack.
5. A completed event matching the next suspending `!` call returns its recorded result. A new suspension appends a command and pauses execution until a completion event is available.
6. Code between suspension points must be deterministic. Time, randomness, and external inputs must cross recorded suspending dependencies.
7. Workflow runs pin compatible code identity, and suspension sites have stable compiler-generated identities for replay checks.
8. Capability providers and live handles are rebound rather than serialized.
9. External commands receive idempotency keys because worker execution and completion recording cannot generally be atomic.
10. There is no `checkpoint` keyword in the language.
11. Incremental queries, cross-run caching, and durable replay use separate identities and reuse rules.
12. Cache hits, invalidations, and recomputations should be observable and distinguishable from history replay.

Interactive execution uses the same recorded-suspension foundation but has notebook semantics. A live kernel retains the current namespace for fast reconnects. Successful cells atomically commit records containing cell/source/code identity, parent state, suspension events, state delta, and output. Recovery restores a serializable namespace snapshot and deterministically replays later committed runs in actual execution order. Rerunning an earlier or edited cell creates a new history branch and marks previous descendants stale.

Open issues after the semantic backlog:

1. The exact `std.incremental` API; no dedicated keyword or `annotate Cache` design is currently proposed.
2. Whether workflows use `annotate Workflow for ...` or standard-library effects only.
3. How closure capture restrictions are displayed to reviewers after their semantics are decided.
4. How tracked external inputs expose versions and how incremental dependencies are inspected.

## Open Design Questions

The maintained cross-cutting question list is in [Design Questions](DESIGN_QUESTIONS.md). This notes file records detailed alternatives near the relevant syntax instead of maintaining a second, easily outdated priority queue.
