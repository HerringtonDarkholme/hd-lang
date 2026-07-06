# Syntax Notes

## Current Direction

The language should read like Python, but behave more like a compact Rust/Go-inspired static language.

Design priorities:

1. Expose intent clearly for AI-generated code.
2. Keep common code short.
3. Make effects, contracts, schemas, and generated tests visible enough for tooling.
4. Avoid Python's dynamic object model.

## Absence

Follow Swift's design:

```text
name: String        # required, cannot be nil
nickname: String?   # optional, may be nil
```

Use `T?` for optional values. Non-optional values cannot be `nil`.

## Function Shape

Candidate:

```text
fn parse_user(input: String) -> User !parse
    ...
```

This keeps the function signature compact while making effects explicit.

Open questions:

1. Should effects come after the return type?
2. Should multiple effects be space-separated, comma-separated, or grouped?
3. Should effect names be plain identifiers or typed values?

Examples:

```text
fn load_user(id: UserId) -> User !db !not_found
fn load_user(id: UserId) -> User ![db, not_found]
fn load_user(id: UserId) -> User throws NotFound uses Db
```

## Contract Syntax Direction

Contracts need to be readable, toolable, and easy for AI to generate correctly. The preferred direction is direct `require` and `ensure` sections inspired by NimContracts, without a wrapping `contract` block:

```text
fn isqrt(x: Int) -> Int !require !ensure
    require
        x >= 0
    ensure
        result >= 0
        result * result <= x
        (result + 1) * (result + 1) > x
    floor(sqrt(x))
```

This keeps contracts visibly attached to the function while avoiding Python-like decorators and an extra wrapper block. Contracts are language-level intent, not runtime metadata.

Contract expressions should be typechecked, but the compiler should not try to prove them. Failed contract checks should use their own effects. The exact effect names are still open; `!require` and `!ensure` are placeholders.

Candidate effect names:

```text
fn divide(a: Int, b: Int) -> Int !require !ensure
fn divide(a: Int, b: Int) -> Int !precondition !postcondition
fn divide(a: Int, b: Int) -> Int !caller_contract !callee_contract
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
fn withdraw(account: Account, amount: Money) -> Account !require !ensure
    require
        amount > 0
        account.balance >= amount
    ensure
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
fn divide(a: Int, b: Int) -> Int !require !ensure
    require
        a >= 0
        b > 0
    ensure
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
while low < high
    invariant
        if key in items
            key in items[low..high]
        high - low < old(high - low)
    mid := (low + high) / 2
    if items[mid] < key
        low = mid + 1
    else
        high = mid
```

## Type Invariants

Structs should be able to declare invariants:

```text
struct Account
    balance: Money
    currency: Currency

    invariant
        balance >= 0
```

Open question: should type invariants be checked after construction, after public mutation, or at all function boundaries?

## Dependency Effects

Dependencies should be modeled as effects. Users should be able to define custom effects and handlers.

Candidate:

```text
effect db
    fn get_user(id: UserId) -> User?

fn load_user(id: UserId) -> User !db !require
    require
        id.value != ""
    user := perform db.get_user(id)
    user!

handler mock_db for db
    fn get_user(id: UserId) -> User?
        User(id: id, name: "Test User")
```

Open syntax issues:

1. How effects are declared.
2. How operations inside an effect are called.
3. How handlers are selected in tests, production, and nested scopes.

## Effect Polymorphism

Higher-order functions need a way to propagate the effects of function-typed arguments. Without it, `map` either forbids effectful callbacks or needs one copy per effect combination:

```text
fn map(items: List[T], f: fn(T) -> U) -> List[U]   # what effects does map have?
```

### Candidate A: Explicit effect-row variables (Koka-style)

```text
fn map[T, U, e](items: List[T], f: fn(T) -> U !e) -> List[U] !e
```

1. Fully explicit; aligns with effects being visible in signatures.
2. General: supports several independent variables, stored function fields, and returned closures.
3. Verbose; AI and reviewers must thread `!e` correctly, and variables must be declared so a typo of an effect name cannot silently become a fresh variable.

Explicit effect variables become more useful if handlers can remove effects from the variable. This may be full row polymorphism, or a smaller operation that only supports effect removal by handlers.

Candidate:

```text
fn handle_log[e](callback: fn(String) -> Void !e) -> Void !(e - log)
    with logger
        callback("str")
```

Meaning:

1. `callback` may perform any effects in `e`.
2. `handle_log` installs a handler for `log`.
3. The remaining effect row is `e - log`.
4. If `callback` only performs `log`, the result is pure.
5. If `callback` performs `log` and `db`, the result effect is `db`.

The important idea is effect-variable transformation: a function can propagate "all callback effects except the ones I handle." This is useful even if the language does not expose full row polymorphism in v1.

### Candidate B: Parameter-linked effects (generalized Swift `rethrows`)

```text
fn map(items: List[T], f: fn(T) -> U) -> List[U] !f
```

`!f` in the effect row means "whatever effects the argument bound to `f` has".

1. Reads naturally; no type-level machinery in the common case.
2. Typo-resistant: `!f` must name a function-typed parameter or the program does not compile.
3. Multiple function parameters union: `!f !g`.
4. Does not cover function values stored in structs or returned closures; those need row variables or monomorphization.

### Candidate C: Inferred propagation

Unhandled effects of function-typed parameters propagate automatically; the signature stays clean and tooling displays the resolved effect row.

1. Zero annotation burden; nothing for AI to get wrong.
2. Conflicts with the decision that effects are explicit in signatures; a reviewer reading source sees `map` as pure.

### Open questions regardless of candidate

1. Effect subtraction/removal syntax: should handled effects be written as `!(e - log)`, `!e - log`, `!e without log`, or inferred from `with logger`?
2. Function-typed struct fields and returned closures: row variables, monomorphization, or disallowed in v1?
3. Do polymorphic rows range over contract effects (`!require`, `!ensure`) as well as user effects?
4. How do handlers installed at a call site interact with a polymorphic row?
5. Is the underlying model full row polymorphism, or a smaller effect-variable system that only supports union and removal?

## Branded Validated Types

Validation annotations do not refine static types by default (unbranded values keep base type identity), but opt-in branded newtypes tie validation to type identity. If a type is not branded, it does not refine.

Candidate spelling, reusing ordinary validator values:

```text
brand Email = String.email().max_len(320)

struct User
    id: UserId
    email: Email
```

Rules:

1. `Email` is a distinct static type. A `String` is not assignable to `Email` without going through validation.
2. Construction is the enforcement boundary: `Email.parse(s)` returns `Email` or fails with a validation effect ("parse, don't validate").
3. Once constructed, interior code can trust the brand; no re-validation at internal boundaries.
4. Unbranded annotated fields keep the existing semantics: metadata for tooling and runtime checks only, no refinement.

Open questions:

1. Spelling: `brand Email = ...` versus `type Email = brand String ...` versus an annotation on a type alias.
2. Whether brand-to-base coercion is implicit (usable anywhere a `String` is) or explicit (`.value`).
3. Whether inline `@` annotations may appear on a branded field, and how conflicts with the brand's validators are handled. Candidate: field annotations on branded fields are a compile error; validators live on the brand.
4. How brands appear in generated JSON Schema and TypeScript output. Candidate: TypeScript branded types.

## Schema And Validation Direction

Validation should not create distinct static subtypes by default. A field like `String.max_len(50)` and `String.max_len(100)` should still have the same base static type, `String`; validation metadata is used for runtime checks, generated schemas, generated data, docs, and tooling.

Support two styles:

1. Lightweight annotations for minimal inline constraints.
2. General external `annotate <Facet> for <Target>` blocks for richer or extendable metadata.

Candidate annotation style:

```text
struct User
    id: UserId
    @email
    @max_len 320
    email: String
    @range 0..150
    age: Int
```

Custom validators should be normal functions and can be referenced from annotation metadata:

```text
fn company_email(value: String) -> Bool
    value.ends_with("@company.com")

struct Employee
    id: UserId
    @email
    @max_len 320
    @refine company_email
    email: String
```

Candidate external validation annotation block:

```text
annotate Validation for Employee
    email:
        String.email().max_len(320).refine(company_email)
    age:
        Int.range(18..150)
```

The same mechanism should work for other tooling facets:

```text
annotate DatabaseSchema for User
    userId: varchar(36).primary_key()
    email: varchar(320).unique()
    createdAt: timestamp()

annotate UI for User
    userId: text
    avatar: ProfileImage.rounded(size: 40)
    email: link.mailto()
```

Reusable validation pieces can be ordinary values/functions, not new type-level entities:

```text
let CompanyEmail = String.email().max_len(320).refine(company_email)

annotate Validation for Employee
    email: CompanyEmail
```

Tooling should generate runtime validators, JSON Schema, TypeScript types, valid test data generators, serializers/deserializers, and documentation from validated data types. Invalid data generation is not needed initially.

Open syntax issues:

1. Exact annotation spelling for inline constraints.
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
fn get_user(id: UserId) -> User !db !not_found !access
    access
        require auth.can("user:read")
    user := perform db.get_user(id)
    user?
```

Metadata-heavy registrations can use an indented annotation block:

```text
@tool
    description "Fetch a user by ID."
    context AuthContext, TenantContext
    generate openapi, json_schema, mcp
fn get_user(id: UserId) -> User !db !not_found !access
    access
        require auth.can("user:read")
    user := perform db.get_user(id)
    user?
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

Candidate separate policy block:

```text
struct User
    id: UserId

struct Post
    id: PostId
    author: UserId

retention
    when User deleted
        delete Post where author == User.id
```

Candidate relationship-local rule:

```text
struct Post
    id: PostId
    author: UserId
        references User.id
        on User deleted
            delete self
```

Open syntax issues:

1. Whether lifecycle rules live in separate policy blocks or near relationships.
2. How retention rules refer to related objects.
3. How retention rules compile to storage backends, workflows, or generated checks.
4. How retention rules interact with auth, audit logs, and observability.

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
