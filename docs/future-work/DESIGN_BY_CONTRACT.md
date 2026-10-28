# Design By Contract

Status: design exploration, 2026-10-28; nothing here is decided or in the
specification.

This record explores [issue #880](https://github.com/HerringtonDarkholme/hd-lang/issues/880).
It reviews [Functions](../../spec/lang/07-functions.md),
[Deferred Cleanup](../../spec/lang/06-control-flow.md#deferred-cleanup),
[Runtime Panics](../../spec/lang/06-control-flow.md#runtime-panics), and
[Prefix Decorators](../../spec/lang/14-annotations.md#prefix-decorators).

## Contents

1. [The domain](#1-the-domain)
2. [The problem and hd today](#2-the-problem-and-hd-today)
3. [Survey](#3-survey)
4. [Plain library checks](#4-plain-library-checks)
5. [A library boundary combinator](#5-a-library-boundary-combinator)
6. [Compiler-known contract decorators](#6-compiler-known-contract-decorators)
7. [Native contract clauses](#7-native-contract-clauses)
8. [Comparison](#8-comparison)
9. [Recommendation](#9-recommendation)
10. [Questions for the owner](#10-questions-for-the-owner)
11. [Sources and parse log](#11-sources-and-parse-log)

## 1. The Domain

A contract states what one side of a call may assume and what the other side
must provide. A precondition is the caller's obligation. A postcondition is the
callee's guarantee after a normal return.

A type invariant describes valid observable states of a value. It may be false
temporarily while a method mutates private state. The hard question is which
program points count as observable boundaries.

Contracts differ from input validation. Bad user input is expected and normally
returns a `Result`. A contract failure means the program is wrong and cannot
safely continue.

Postconditions add two mechanisms beyond an ordinary assertion:

- the completed function result must have a name;
- an `old(expression)` reads the expression's entry value.

`old` is not a deep snapshot. For an hd value, evaluation copies that value.
For a counted reference, it copies an alias unless the expression selects a
value field.

The worked example is a withdrawal which changes an account balance:

```text
struct Account:
    balance: i64

fn withdraw(account: &mut Account, amount: i64) -> i64:
    if amount < 0:
        panic("amount must be non-negative")
    before := account.balance
    account.balance = account.balance - amount
    result := account.balance
    if result != before - amount:
        panic("balance did not decrease by amount")
    result
```

This body can write both checks today. It cannot expose either condition as part
of the function's interface. Each early return must also repeat the second
check.

Five details make automatic contracts difficult:

1. `return`, fallthrough and postfix `?` all complete a function.
2. `defer` and `using` run after the result is saved, before the caller sees it.
3. A panic skips pending source cleanup and poisons the program instance.
4. An ordinary closure cannot capture a local reference such as `&mut account`.
5. A struct can be copied or changed through a public field without calling a method.

## 2. The Problem And hd Today

The question is which contract support belongs in ordinary `std`, and which
parts require compiler-known behavior. The answer must preserve hd's preference
for library mechanisms over semantic exceptions and syntax.

The fixed use cases are:

1. reject a bad function argument before the body runs;
2. check a returned value on every normal completion path;
3. compare post-state with an entry value;
4. check a type after construction and state-changing methods;
5. handle plain and suspending functions without losing cleanup ordering; and
6. let tests and `hd lint` find and exercise declared contracts.

Hd currently provides these pieces:

- `panic(message)` always raises `explicit-panic` and source cannot catch it.
- `std.testing.assert(condition, reason)` raises `assertion-failed`, even
  outside a test, through a compiler intrinsic hidden inside `std.testing`.
- `defer` saves a result before cleanup, then runs on every structured exit.
- A panic does not run pending `defer` suites or `using` disposals.
- Decorators attach ordinary values to declarations. They do not wrap calls or
  transform a function body.
- Typed derivation traverses struct and enum members. It cannot enumerate or
  rewrite a type's inherent methods.
- `&T` and `&mut T` are local references. A closure cannot capture them.

A plain `require(bool, reason)` therefore fits in std immediately. It can call
`panic`, but it cannot select a new stable panic category without an intrinsic.

Automatic `ensure` cannot be an ordinary after-the-body call. The function may
return earlier, propagate with `?`, or suspend. A callback guard also cannot see
the saved result and inherits the local-reference and suspension catches from
[#879](https://github.com/HerringtonDarkholme/hd-lang/issues/879).

Automatic invariants have a separate boundary problem. Checking only
`&mut self` methods misses construction, direct field writes, and mutation
through aliases. Checking every write changes unrelated code and may observe a
temporarily invalid state.

No option should use contracts for recoverable input errors. A contract
predicate must not suspend. If checks can disappear, it must also have no
required side effect, which current hd cannot prove generally.

## 3. Survey

| System | Preconditions and postconditions | Invariants and old values | Release behavior and lesson for hd |
| --- | --- | --- | --- |
| Eiffel | `require` and `ensure` are clauses of every routine. Preconditions are caller obligations; postconditions are supplier guarantees. | A class invariant holds after creation and around exported calls. `old expression` in a postcondition evaluates the entry value. | Assertion monitoring has selectable levels. Contracts also remain interface documentation when checks are disabled. Native support gives the complete model, but it spends syntax and inheritance rules. [Eiffel contracts](https://www.eiffel.org/doc/eiffelstudio/I2E-_Design_by_Contract_and_Assertions), [checking levels](https://www.eiffel.org/doc/eiffel/ET-_Design_by_Contract_%28tm%29%2C_Assertions_and_Exceptions). |
| D | `in` and `out(result)` blocks run before and after a function. A failed assertion reports a contract failure. | Struct and class invariant functions run around public methods and construction. D has no general `old` expression, so code snapshots values explicitly. | Release builds remove `in`, `out`, and invariants. D warns that contracts are not user-input validation. This makes side effects inside predicates unsafe. [D contracts](https://dlang.org/spec/contracts.html), [D structs and classes](https://dlang.org/book/invariant.html). |
| Ada 2022 | `Pre` and `Post` are language-defined aspects on callable entities. Class-wide forms compose across dispatching operations. | `Type_Invariant` is an aspect on private types. Postconditions can use the `Old` attribute for entry values. | `Assertion_Policy` independently checks, ignores, or implementation-defines assertion kinds. Ada's aspects show a decorator-like surface still needs language semantics. [Pre and Post](https://www.adaic.org/resources/add_content/standards/22aarm/html/AA-6-1-1.html), [Type invariants](https://www.adaic.org/resources/add_content/standards/22aarm/html/AA-7-3-2.html), [assertion policy](https://www.adaic.org/resources/add_content/standards/12rm/html/RM-11-4-2.html). |
| Kotlin | `require` is an inline std function which throws `IllegalArgumentException`. `check` similarly reports invalid state with `IllegalStateException`. | Kotlin std supplies no automatic postcondition, old-value, or class-invariant hook. Callers write explicit checks. | Both functions are ordinary always-executed library calls. This is the cheapest useful subset and cleanly separates argument from state failures. [`require`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin/require.html), [`check`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin/check.html). |
| Rust | `assert!` is always checked; `debug_assert!` is normally absent from optimized builds. The `contracts` crate adds `requires`, `ensures`, and `invariant` procedural attributes. | The crate exposes the result as `ret` and supports an `old()` pseudo-function in postconditions. It provides always, debug, and test-only modes. | Rust std chooses calls, while a third-party macro performs body rewriting. Hd has no macro system, so the same surface would be compiler-known. [`assert!`](https://doc.rust-lang.org/std/macro.assert.html), [`debug_assert!`](https://doc.rust-lang.org/std/macro.debug_assert.html), [`contracts`](https://docs.rs/contracts/latest/contracts/). |
| Swift | `precondition` is a std function for conditions needed in production. `assert` is development-only. Neither is recoverable. | Swift std supplies no automatic postcondition or type-invariant construct. Programs write explicit checks. | The split exposes an important policy choice: production preconditions and debug assumptions are different APIs. [Assertions and Preconditions](https://docs.swift.org/swift-book/LanguageGuide/TheBasics.html#Assertions-and-Preconditions), [`precondition`](https://developer.apple.com/documentation/swift/precondition%28_%3A_%3Afile%3Aline%3A%29). |
| Racket | Function contracts wrap a boundary, check arguments and results, and assign blame to the client or provider. Contracts commonly live at module exports. | Stateful and higher-order contracts can monitor values across later uses rather than only one return. | Boundary wrappers are powerful but can add repeated checks and unexpected blame. Hd panics poison an instance and has no recovery, so blame is less useful than a precise category and frame. [Contract boundaries](https://docs.racket-lang.org/guide/contract-boundaries.html), [function contracts](https://docs.racket-lang.org/guide/contract-func.html). |

Three takeaways follow.

1. Kotlin and Swift validate that `require` can be only a library call.
2. Every complete postcondition system rewrites the function boundary through
   syntax, aspects, attributes, macros, or wrappers.
3. Configurable checks require side-effect discipline. Always-on checks avoid
   that extra semantic rule.

The strongest prior-art default is therefore a split. Start with ordinary
always-on checks, then add compiler-known boundary contracts only if automatic
postconditions and invariants justify them.

## 4. Plain Library Checks

This radical simplification adds ordinary `std.contract` functions and no
automatic mechanism:

```text
fn require(condition: bool, reason: string) -> void:
    if !condition:
        panic("precondition failed: $reason")

fn ensure(condition: bool, reason: string) -> void:
    if !condition:
        panic("postcondition failed: $reason")

struct Account:
    balance: i64

fn withdraw(account: &mut Account, amount: i64) -> i64:
    require(amount >= 0, reason="amount is non-negative")
    before := account.balance
    account.balance = account.balance - amount
    result := account.balance
    ensure(result == before - amount, reason="balance decreases by amount")
    result
```

The stdlib tier adds `require` and perhaps `ensure` or `check`. Each is an
ordinary function implemented with `panic`. No compiler rule changes.

Both checks run in every build. Their conditions may call ordinary code and
have side effects because hd already evaluates function arguments normally.

The failure category is `explicit-panic`. A distinct `contract-violation`
category would turn this into a compiler intrinsic addition, like the hidden
panic helper behind `std.testing.assert`.

This option handles cheap preconditions well. It handles a postcondition only
when the body reaches one explicit exit. It offers no automatic invariant or
`old` mechanism.

Early `return` and `?` can bypass `ensure`. A refactor can silently stop checking
a guarantee. `hd lint` could warn about an `ensure` before some returns, but it
cannot prove that the call expresses every intended postcondition.

Agents already know this pattern from Kotlin and Swift. The explicit calls are
readable, and diagnostics point directly to the failed check. The missing
all-path guarantee is the intentional simplification.

Traits, requirement rows, suspension, derivation and RC do not change. The
functions accept evaluated booleans, so no closure or reference escapes.

Design-cost kinds: one core-library addition, kind 4. Adding a stable category
also needs one compiler intrinsic, kind 3.

## 5. A Library Boundary Combinator

This option makes a body closure the boundary. An ordinary generic function
runs it, receives the result, and then checks a predicate:

```text
fn contract[T](
    body: fn() -> T,
    post: fn(T) -> bool,
    reason: string,
) -> T:
    result := body()
    if !post(result):
        panic("postcondition failed: $reason")
    result

struct Account:
    balance: i64

fn withdraw(account: *mut Account, amount: i64) -> i64:
    require(amount >= 0, reason="amount is non-negative")
    before := account.balance
    body := fn() -> i64:
        account.balance = account.balance - amount
        account.balance
    post := fn(result: i64) -> bool: result == before - amount
    contract(
        body,
        post,
        reason="balance decreases by amount",
    )
```

The stdlib tier adds `contract` and `require`. A second `contract!` would be
needed for a suspending body, and both signatures must preserve the body's
requirement row.

The result reaches the postcondition without compiler rewriting. Entry values
are ordinary explicit bindings such as `before`.

The original `&mut Account` signature cannot be used. Both closures would
capture a local reference, which is `local-reference-escapes` today. The example
uses a counted `*mut Account`, changing ownership and RC behavior.

`return` inside the body returns from the generated closure, not from
`withdraw`. This is current hd behavior. It makes a wrapper refactor change the
target of every return and complicates nested `?` propagation.

The combinator's postcondition runs before the outer function's `defer` and
`using` cleanup. A true function postcondition should normally observe state
after that cleanup, so this ordering also differs.

A combinator can expose invariants only through explicit wrappers. It cannot
intercept construction, direct field writes, or unrelated method calls.

This mechanism is ordinary library code and useful for counted callbacks. It
does not preserve the fixed local-reference, cleanup and control-flow cases.

Traits and derivation do not change. Suspension needs a parallel API. RC may
allocate closure environments and count captured references.

Design-cost kinds: core-library additions only, kind 4. Preserving local
references would additionally need the unsettled scoped-closure model.

## 6. Compiler-Known Contract Decorators

This option uses existing decorator syntax and gives three qualified std names
compiler semantics. The compiler rewrites a decorated function boundary:

```text
@require(fn(account: &Account, amount: i64) -> bool: amount >= 0)
@ensure(fn(result: i64, account: &Account, amount: i64) -> bool:
    result == old(account.balance) - amount
)
fn withdraw(account: &mut Account, amount: i64) -> i64:
    account.balance = account.balance - amount
    account.balance
```

The decorator expressions are current syntax. Their behavior is not current
behavior: an ordinary decorator only attaches a value.

The compiler evaluates `require` when execution enters the body. It evaluates
each `old(expression)` once at that point, saves every normal result, runs
registered cleanup, and then evaluates `ensure` before returning.

`result` denotes the saved result. A `Result.Err` propagated by `?` is still a
normal result, so a predicate decides whether it is acceptable. A panic and
cancellation do not promise a postcondition.

A suspending function checks its precondition when its driven body starts, not
when its cold suspension value is created. Its postcondition runs after normal
completion and shielded cleanup.

Predicates must return `bool`, must not suspend, and have an empty requirement
row. They may read their parameters and copied entry values. Always-on
evaluation avoids a purity rule that hd cannot currently prove.

`old` evaluates the written expression. `old(account.balance)` saves an `i64`.
`old(account)` would copy the local reference or counted reference, not clone
the account.

Type invariants remain the difficult part. One possible decorator is:

```text
@invariant(fn(account: &Account) -> bool: account.balance >= 0)
struct Account:
    balance: i64
```

The compiler can check it after complete construction and around exported
methods. However, direct public field writes and same-module helpers can bypass
those boundaries.

Requiring invariant-bearing structs to hide their fields makes the automatic
rule coherent. Checking after every field write instead observes temporary
states and creates action at a distance.

Decorators on trait methods raise composition questions. A replacement must
not strengthen a precondition or weaken a postcondition if callers know only
the trait. Eiffel and Ada solve this with explicit inheritance rules.

The language tier adds compiler-known decorators, predicate typing, `result`,
`old`, ordering, trait composition, and panic behavior. The stdlib tier declares
the decorator names and any fact values used by tooling.

The stable failure category can be `contract-violation`, with a frame naming
precondition, postcondition, or invariant and showing `reason`. Source still
cannot catch it, and the failed instance remains poisoned.

`hd lint` can read normalized contract metadata from checked functions. Property
tests can generate arguments satisfying preconditions and assert postconditions,
but rejecting too many generated values needs the existing discard limits.

This option preserves local references because predicates are compiler-bound
checks, not stored closure values. It adds no grammar, but apparent ordinary
decorators now change control flow.

Design-cost kinds: compiler intrinsics, kind 3, plus boundary semantic rules,
kind 2. It avoids new syntax, kind 1.

## 7. Native Contract Clauses

This option follows Eiffel and D with clauses in the declaration:

```text
fn withdraw(account: &mut Account, amount: i64) -> i64  # hypothetical syntax
    require amount >= 0 reason="amount is non-negative"  # hypothetical syntax
    ensure result: result == old(account.balance) - amount  # hypothetical syntax
:
    account.balance = account.balance - amount
    account.balance
```

A type-level clause can place the invariant beside its fields:

```text
struct Account:
    balance: i64
    invariant balance >= 0 reason="balance is non-negative"  # hypothetical syntax
```

The semantic model can match the decorator option. Preconditions run at body
entry. Postconditions run after result-saving and cleanup. Panics and
cancellation provide no postcondition.

The syntax makes contracts visible in generated documentation and reserves
`result` and `old` only inside a contract clause. It cannot be mistaken for
ordinary attached metadata.

The language tier adds grammar, typing, lowering, boundary rules, trait
composition, invariant timing, a panic category, and tool metadata. No stdlib
implementation provides the core behavior.

The invariant boundary problem remains. Syntax does not decide what a value
constructor is, whether public fields are allowed, or which method calls expose
a stable state.

This surface is familiar to Eiffel, D and specification languages. It is also
the costliest option under the repository's design order because it adds new
syntax before exhausting an existing decorator form.

Agents can distinguish clauses from body statements. They must learn another
function-header region, `old` lifetime rules, and invariant boundary rules.

Traits, requirement rows, suspension, derivation and RC interact as in the
decorator option. The clauses become part of callable API compatibility.

Design-cost kinds: new syntax, kind 1, semantic boundary rules, kind 2, and
compiler implementation, kind 3.

## 8. Comparison

| Criterion | Plain library checks | Library combinator | Contract decorators | Native clauses |
| --- | --- | --- | --- | --- |
| Bad argument | Explicit `require` call | Explicit `require` call | Automatic before body | Automatic before body |
| Every normal result | No; each exit must call | Yes inside closure only | Yes, including fallthrough, `return`, and `?` | Yes, including fallthrough, `return`, and `?` |
| Entry values | Explicit local copy | Explicit local copy | `old(expression)` compiler snapshot | `old(expression)` compiler snapshot |
| `&mut` local receiver | Preserved | Cannot be captured today | Preserved by compiler lowering | Preserved by compiler lowering |
| Cleanup ordering | Author chooses; easy to bypass | Post runs before outer cleanup | Post runs after saved-result cleanup | Post runs after saved-result cleanup |
| Suspending function | Ordinary calls work | Needs `contract!` and row-preserving callbacks | Same decorators; checks run while driven | Same clauses; checks run while driven |
| Type invariant | Explicit calls only | Explicit wrapper only | Possible automatic hook, but boundary rules required | Possible automatic hook, but boundary rules required |
| Direct public field write | Only checked where author calls | Not intercepted | Must reject, instrument, or exclude | Must reject, instrument, or exclude |
| Trait composition | No contract inheritance | No contract inheritance | Needs weakening/strengthening rules | Needs weakening/strengthening rules |
| Release policy | Always checked | Always checked | Recommended always checked | Recommended always checked |
| Failure category | `explicit-panic`, unless intrinsic added | `explicit-panic`, unless intrinsic added | New stable `contract-violation` | New stable `contract-violation` |
| Tooling | Lint sees calls | Lint sees wrappers | Structured metadata for lint, docs and tests | Structured syntax for lint, docs and tests |
| Rules added | Two std functions | Std functions and wrappers | Compiler decorators, predicate and boundary semantics | Grammar, predicate and boundary semantics |
| Rules removed | None | None | None | None |
| Soundness condition | No automatic claim | Closures cannot outlive captures | Correct all-exit rewrite and invariant boundary | Correct all-exit rewrite and invariant boundary |
| Agent writability | Easiest, but exits are forgettable | Unfamiliar wrapper and changed `return` target | Familiar annotations; errors can name predicates | Clearest intent; most new syntax |
| Human readability | Checks mixed into body | Body nested inside a call | Contract sits above declaration | Contract is part of declaration |
| Implementation cost | Smallest | Small std addition; weak semantics | Checker, lowering, metadata and std declarations | Parser, checker, lowering, metadata and docs |
| Evolution | Add automation later | Hard to migrate borrowed bodies | Can grow one decorator at a time | Commits to a permanent grammar region |
| Costliest change kind | core library (4) | core library (4) | semantic rule (2) | syntax (1) |

The library options are cheaper because they make weaker guarantees. Their
cost ranking does not imply that they implement automatic postconditions.

The decorator and clause options have the same hard semantic questions. The
surface does not solve invariant boundaries, trait composition, cleanup order,
or `old` aliasing.

## 9. Recommendation

**Recommendation: adopt plain `require` first, and prefer compiler-known
decorators if automatic postconditions later prove necessary.**

1. Add an always-on `std.contract.require(condition, reason)` as an ordinary
   function. Initially keep `explicit-panic`; do not spend an intrinsic only
   for a category.
2. Keep `ensure` explicit in tests and bodies until real code demonstrates
   repeated early-exit mistakes. A function named `ensure` without all-path
   semantics is likely to overpromise.
3. If automatic postconditions are approved, use `@require` and `@ensure`.
   Existing decorator grammar avoids a new header language and follows Ada
   aspects and Rust contract attributes.
4. Keep checks always on. Add a separate debug assertion API for expensive
   assumptions instead of making contract behavior build-dependent.
5. Do not approve automatic type invariants until the owner chooses stable
   boundaries for construction, field writes and same-module mutation.

This recommendation gives up first-class postcondition documentation in the
initial slice. It also gives up release removal of expensive checks.

The next-best option is native clauses if contracts become central enough to
justify syntax. Clauses are more honest than compiler-special decorators about
their control-flow behavior.

The combinator is not recommended as the general answer. It changes reference,
cleanup, requirement-row and `return` behavior merely to avoid compiler support.

## 10. Questions For The Owner

1. **Which initial scope should hd adopt?**
   - **Recommended:** an ordinary always-on `require(condition, reason)` only.
   - Add plain `require` and explicit `ensure` calls.
   - Start directly with automatic preconditions and postconditions.

```text
fn withdraw(account: &mut Account, amount: i64) -> i64:
    require(amount >= 0, reason="amount is non-negative")
    account.balance = account.balance - amount
    account.balance
```

2. **If automatic postconditions are wanted, which surface should carry them?**
   - **Recommended:** compiler-known `@require` and `@ensure` decorators.
   - Native function clauses.
   - A library `contract(body, post)` wrapper.

```text
@ensure(fn(result: i64, amount: i64) -> bool: result >= -amount)
fn change(amount: i64) -> i64:
    0 - amount
```

3. **When should approved contracts run?**
   - **Recommended:** every build; add a separate debug assertion for expensive checks.
   - Development and tests only.
   - A package or build-profile policy chooses each contract kind.

```text
fn reserve(capacity: usize) -> void:
    require(capacity > 0, reason="capacity is positive")
```

4. **What failure identity should a plain library `require` use?**
   - **Recommended:** existing `explicit-panic` for the first library-only slice.
   - Add `contract-violation` immediately through a compiler panic intrinsic.
   - Reuse `assertion-failed` from `std.testing`.

```text
fn open(port: u16) -> void:
    require(port > 0, reason="port zero is reserved")
```

5. **What does `old` copy?**
   - **Recommended:** evaluate the written expression once with ordinary hd copy semantics.
   - Deep-copy every reachable object.
   - Permit only values, not reference-typed expressions.

```text
@ensure(fn(result: i64, account: &Account) -> bool:
    result == old(account.balance)
)
fn balance(account: &Account) -> i64:
    account.balance
```

6. **Which values may declare an automatic invariant?**
   - **Recommended:** postpone invariants until construction and mutation boundaries are designed.
   - Only structs with no public fields, checked after construction and exported methods.
   - Every struct, checked after each complete construction and each field write.

```text
@invariant(fn(account: &Account) -> bool: account.balance >= 0)
struct Account:
    balance: i64
```

7. **How should trait contracts compose?**
   - **Recommended:** a replacement may weaken preconditions and strengthen postconditions.
   - Require exact contract identity across trait and implementation.
   - Apply only the contract visible at the static call site.

```text
trait Store:
    @ensure(fn(result: bool) -> bool: result)
    fn save(&mut self, value: string) -> bool
```

## 11. Sources And Parse Log

Sources were read on 2026-10-28. The survey links the primary source for each
claim. Repository sources are the spec sections linked in the status and
problem sections, plus [issue #879](https://github.com/HerringtonDarkholme/hd-lang/issues/879).

Every `text` block was checked with `scripts/hd-conformance.sh parse` from this
branch. Parsing checks syntax only; it does not type-check proposed names or
compiler behavior.

| Block | Result | Note |
| --- | --- | --- |
| Domain baseline | Parsed | Current syntax. |
| Plain library checks | Parsed | Current syntax. |
| Library boundary combinator | Parsed | Current syntax; typing would expose the documented ownership change. |
| Contract decorators | Parsed | Existing decorator syntax with hypothetical compiler semantics. |
| Decorated invariant | Parsed | Existing decorator syntax with hypothetical compiler semantics. |
| Native function clauses | Rejected as expected | Lines 2 and 4 reject the hypothetical header clauses. |
| Native type invariant | Rejected as expected | Line 3 rejects the hypothetical invariant member. |
| Owner question 1 | Parsed | Current syntax. |
| Owner question 2 | Parsed | Existing decorator syntax with hypothetical compiler semantics. |
| Owner question 3 | Parsed | Current syntax. |
| Owner question 4 | Parsed | Current syntax. |
| Owner question 5 | Parsed | Existing decorator syntax with hypothetical compiler semantics. |
| Owner question 6 | Parsed | Existing decorator syntax with hypothetical compiler semantics. |
| Owner question 7 | Parsed | Existing decorator syntax with hypothetical compiler semantics. |
