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
2. `let` for mutated variables.

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

`let` introduces a variable that can be reassigned. Type annotation is optional:

```text
let attempts: i32 = 0
let inferred = 1

attempts = attempts + 1
inferred = inferred + 1
```

Reasoning:

1. Most local code stays concise with `:=`.
2. Mutation is review-relevant, so it should be visually explicit.
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
long_names := [name for name in names if name.len() > 3]
name_lengths := [name.len() for name in names]
```

Map comprehensions produce `map[K, V]` values:

```text
scores_by_name := {user.name: user.score for user in users}
active_by_id := {user.id: user for user in users if user.active}
```

Explicit mutable collection variables use `let`, like any other mutable binding:

```text
let attempts: list[i32] = []

attempts.append(1)
attempts.append(2)
```

Open syntax issues:

1. Which collection operations are methods, functions, or trait-provided behavior.
2. Whether comprehensions support multiple `for` clauses.
3. Whether comprehensions support local bindings inside the comprehension.

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

Closure parameter and return types can be inferred when there is an expected function type:

```text
fn map_names(names: list[string], f: fn(string) -> string) -> list[string]:
    ...

lower_names := map_names(names, fn(name):
    name.lower()
)
```

Closures capture values from lexical scope. Exact capture rules for mutable locals, serializable closures, and capability-carrying closures are still open.

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
3. Whether default values must be compile-time constants.
4. Whether generic arguments are inferred at call sites, explicit at call sites, or both.
5. Whether a shorter one-line closure form is needed.
6. Exact closure capture rules.

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

Open syntax issues:

1. Exact diagnostics when embedded methods conflict while checking trait satisfaction.

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
db := use(Database)
user := db.get_user!(id)?
```

Body-level intent:

1. `use(Database)` resolves or injects the `Database` dependency from the current handler/context.
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

fn load_user!(id: UserId) -> Result[User?, DbError] $ Database:
    require:
        id.value != ""
    db := use(Database)
    user := db.get_user!(id)?
    user

handler mock_db for Database:
    fn get_user!(id: UserId) -> Result[User?, DbError]:
        ok(User {
            id: id,
            name: "Test User"
        })
```

Open syntax issues:

1. How operations inside an effect are called.
2. How handlers are selected in tests, production, and nested scopes.
3. `Result[T, E]` ergonomics beyond `?` propagation, including construction and pattern matching.

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
fn handle_log[e](callback: fn(string) -> void $ e) -> void $ (e - log):
    with logger:
        callback("str")
```

Meaning:

1. `callback` may require any effects in `e`.
2. `handle_log` installs a handler for `log`.
3. The remaining requirement row is `e - log`.
4. If `callback` only requires `log`, the result is pure after `log` is handled.
5. If `callback` requires `log` and `Database`, the result effect is `Database`.

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

1. Effect subtraction/removal syntax: should handled effects be written explicitly as `$ (e - log)` or inferred from `with logger`?
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

Support two styles:

1. Lightweight annotations for minimal inline constraints.
2. General external `annotate <Facet> for <Target>` blocks for richer or extendable metadata.

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

Candidate external validation annotation block:

```text
annotate Validation for Employee:
    email:
        string.email().max_len(320).refine(company_email)
    age:
        i32.range(18..150)
```

The same mechanism should work for other tooling facets:

```text
annotate DatabaseSchema for User:
    userId: varchar(36).primary_key()
    email: varchar(320).unique()
    createdAt: timestamp()

annotate UI for User:
    userId: text
    avatar: ProfileImage.rounded(size=40)
    email: link.mailto()
```

Reusable validation pieces can be ordinary values/functions, not new type-level entities:

```text
CompanyEmail := string.email().max_len(320).refine(company_email)

annotate Validation for Employee:
    email: CompanyEmail
```

Tooling should generate runtime validators, JSON Schema, TypeScript types, valid test data generators, serializers/deserializers, and documentation from validated data types. Invalid data generation is not needed initially.

Open syntax issues:

1. Standard inline validation annotation set.
2. Exact `annotate <Facet> for <Target>` syntax.
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
    db := use(Database)
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
    db := use(Database)
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

Retention should use the general `annotate <Facet> for <Target>` model. Its payload should look like facet-specific field annotations, similar to validation annotations.

Candidate:

```text
struct User:
    id: UserId

struct Post:
    id: PostId
    userId: UserId

annotate Retention for Post:
    userId: ownerId
    policy: deleteWhen(User.deleted)
```

Here, `ownerId` and `deleteWhen` are not new language keywords. They are annotation terms provided by the `Retention` facet, similar to how validation might provide `email`, `max_len`, or `range`.

```text
annotate Validation for User:
    email: email.max_len(320)

annotate Retention for Post:
    userId: ownerId
    policy: deleteWhen(User.deleted)
```

Open syntax issues:

1. Exact `annotate Retention for ...` syntax.
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
6. What should inline validation annotation and external `annotate <Facet> for <Target>` syntax look like?
7. What should declarative data-retention and cascade syntax look like?
8. What should registration annotation/decorator syntax look like?
9. Which effect-polymorphism candidate should higher-order functions use, and what is deferred to later versions?
10. What is the exact `brand` spelling, and is brand-to-base coercion implicit or explicit?
11. What should serializable closure syntax and capture restrictions look like?
12. What should incremental computation syntax and cache/dependency tooling look like?
