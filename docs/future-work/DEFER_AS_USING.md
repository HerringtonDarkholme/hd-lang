# `defer` As A Library Guard

Status: exploration for [#879](https://github.com/HerringtonDarkholme/hd-lang/issues/879).
Nothing here is accepted language or library behavior.

This report reviews [Deferred Cleanup](../../spec/lang/06-control-flow.md#deferred-cleanup),
[Using Bindings](../../spec/lang/06-control-flow.md#using-bindings), and the parked
[Object Model, Tier 2](OBJECT_MODEL_TIER2.md).
It does not change the specification, compiler, or standard library.

## Contents

- [Domain](#domain)
- [Problem And Current Behavior](#problem-and-current-behavior)
- [Cross-Cutting Constraints](#cross-cutting-constraints)
- [Survey](#survey)
- [Option 1: Keep `defer` And `using`](#option-1-keep-defer-and-using)
- [Option 2: A Plain Guard Over Current `using`](#option-2-a-plain-guard-over-current-using)
- [Option 3: Split Synchronous And Asynchronous Disposal](#option-3-split-synchronous-and-asynchronous-disposal)
- [Option 4: A Compiler-Known Function](#option-4-a-compiler-known-function)
- [Option 5: Explicit Cleanup Only](#option-5-explicit-cleanup-only)
- [Comparison](#comparison)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

## Domain

Scope cleanup must connect a registration point to every later exit from that scope.
The difficult cases are early `return`, `break`, `continue`, propagated `?`, and task cancellation.

There are two distinct jobs.
Arbitrary cleanup runs a lexical action, while resource disposal invokes a protocol on an owned value.

These jobs differ in what they may retain.
A lexical suite can use local references, while an ordinary closure value must be safe to store or return.

They also differ in suspension.
Synchronous cleanup completes while control leaves a plain function, while asynchronous disposal needs a driver and may yield.

The representative use case below mutates a borrowed log and exits early.
It requires cleanup to observe the same local storage without making the function suspending.

```text
fn audit(log: &mut List[string], stop: bool) -> bool:
    defer:
        log.push("closed")
    log.push("opened")
    if stop:
        return false
    true
```

## Problem And Current Behavior

`defer:` is a language statement with synchronous suite semantics.
It registers on the innermost cleanup scope and runs in last-in, first-out order.

The suite runs after result operands are saved.
It observes captured lexical storage at cleanup time and may use a local reference.

The suite cannot suspend, propagate `?`, or leave through `return`, `break`, or `continue`.
A panic skips pending cleanup because hd has no panic unwinding.

`using name = expression` instead binds a resource value implementing `std.ops.Disposable`.
Its registered `dispose!` call shares `defer`'s ordering and exit rules.

`dispose!` may suspend, so every `using` appears in a suspending function.
Cancellation waits for its shielded disposal calls.

The proposed library guard appears smaller because it removes one statement form.
On today's model, it changes three observable properties instead of only changing spelling.

1. A plain function becomes suspending, and that effect propagates to callers.
2. A closure cannot capture `&mut log`, because the guard stores the closure past its creating call.
3. The guard needs a readable but unused binding, because `using _ =` is not grammar.

The existing guard also creates a closure value and a resource value.
Current `defer:` can lower directly into the enclosing cleanup frame without either source-level object.

The exact release-size and runtime difference remains a measurement question.
Issue [#858](https://github.com/HerringtonDarkholme/hd-lang/issues/858) can change that result by frame-allocating nonescaping objects.

## Cross-Cutting Constraints

**Traits.** The library guard naturally implements `Disposable`, but that trait commits it to a bang method.
A second synchronous trait would split protocol implementations and resource-binding rules.

**Requirement rows.** A `defer:` suite uses the enclosing function's providers directly.
An `OnExit` field of type `fn() -> void` stores only an empty-row callback.

Hd structs cannot take row parameters, so `OnExit[$R]` cannot preserve an arbitrary callback row.
A general guard therefore also needs row-polymorphic storage or compiler erasure.

**Suspension.** Current `using` makes the enclosing callable suspending even when a particular `dispose!` body never yields.
A synchronous protocol avoids that effect but creates a second resource protocol.

**Derivation.** No existing derivation can generate a guard or prove its capture lifetime.
Deriving traits for the wrapper does not address the closure's row, escape, or suspension effects.

**Reference counting.** A stored closure may allocate counted environment storage and retain every counted capture.
A direct `defer:` suite can instead use the enclosing frame's places.

These costs might disappear after escape analysis or #858.
They cannot disappear for a guard that must genuinely outlive its creating call or carry counted captures.

## Survey

### Go, Zig, And Swift: Lexical Statements

Go evaluates a deferred call's function and arguments at registration, then invokes saved calls in reverse order.
The calls run when the surrounding function returns, including a panic return path.

Zig runs `defer` expressions in reverse order at lexical scope exit.
It separately offers `errdefer`, and it rejects `return` from a deferred expression.

Swift executes a `defer` suite before control leaves its lexical scope.
Its documentation presents manual resource management and thrown errors as primary uses.

These languages keep arbitrary cleanup in syntax.
The direct form makes scope, order, and control restrictions visible without a guard name.

### Rust: Type Destruction Plus Library Guards

Rust runs a type's `Drop::drop` when its value leaves a drop scope.
Local variables drop in reverse declaration order, and nested scopes leave from inside outward.

Arbitrary callback guards are library types rather than Rust standard-library primitives.
The `scopeguard` crate provides both a guard constructor and a `defer!` macro over that guard.

Rust can safely retain borrowed closure captures because its lifetime system types the guard's lifetime.
Hd's current local references deliberately cannot be captured or stored.

### C#, Java, And TypeScript: Resource Bindings

C# `using` disposes an `IDisposable` when control leaves its block.
Its separate `await using` form consumes `IAsyncDisposable` and makes suspension visible.

Java try-with-resources closes declared resources in reverse initialization order.
The language specification defines it by translation to nested `try` and `finally` statements.

TypeScript likewise separates `using` with `Symbol.dispose` from `await using` with `Symbol.asyncDispose`.
Both dispose on early exits and use first-in, last-out order.

TypeScript also exposes `DisposableStack.defer(callback)` for arbitrary cleanup.
That callback lives inside a named stack value whose `using` binding controls the scope.

These designs separate synchronous and asynchronous disposal at the call site.
They also treat resource binding as more structured than an arbitrary cleanup statement.

### Python And Kotlin: Scoped Callback APIs

Python's `ExitStack` is a context manager containing a reverse-order callback stack.
Its `callback` method accepts arbitrary cleanup, while the surrounding `with` supplies lexical scope.

Kotlin's inline `use` function takes a block and closes its receiver after that block completes.
The block boundary makes the resource's extent explicit without storing an escaping callback.

Both library approaches require an enclosing block or context value.
Neither is only a function call whose result silently changes the surrounding scope.

### Takeaways

1. Statement-style `defer` is established prior art for arbitrary lexical cleanup.
2. Resource bindings commonly distinguish synchronous and asynchronous disposal.
3. Callback libraries retain an explicit scope carrier: a guard, stack, context, or block.
4. Library guards need lifetime support when callbacks borrow stack-local storage.
5. A std-looking call with compiler registration semantics is an intrinsic, not an ordinary library function.

## Option 1: Keep `defer` And `using`

This option keeps the representative program exactly as it is today.

```text
fn audit(log: &mut List[string], stop: bool) -> bool:
    defer:
        log.push("closed")
    log.push("opened")
    if stop:
        return false
    true
```

`defer:` remains the arbitrary synchronous action.
`using` remains the typed resource protocol whose `dispose!` may suspend.

No rule changes.
The language tier retains `flow.defer.*`, `flow.using.*`, and the `defer_statement` grammar.

This option preserves local-reference capture, latest-storage observation, and plain-function use.
It also preserves one shared last-in, first-out cleanup stack.

The cost is two user-facing mechanisms.
Agents must learn when an action belongs in `defer:` and when a resource belongs in `using`.

The anti case against Go, Zig, and Swift is typed asynchronous resources.
Those statement-only models do not cover hd's cancellation-aware `Disposable` protocol.

## Option 2: A Plain Guard Over Current `using`

This option removes `defer:` and adds an ordinary std guard constructor.
The current type model can implement it without compiler knowledge.

```text
use std.ops.Disposable

struct OnExit:
    run: fn() -> void

impl Disposable for OnExit:
    fn dispose!(&mut self) -> void:
        (self.run)()

fn on_exit(run: fn() -> void) -> OnExit:
    OnExit { run: run }

fn audit!(log: *mut List[string], stop: bool) -> bool:
    using cleanup = on_exit(fn(): log.push("closed"))
    log.push("opened")
    if stop:
        return false
    true
```

The stdlib tier adds `OnExit` and `on_exit`.
The language tier removes `flow.defer.*` and the `defer_statement` grammar.

The example cannot retain the original `&mut List[string]` parameter.
It changes that parameter to a counted `*mut` reference and changes `audit` to `audit!`.

These are semantic regressions, not only ergonomic costs.
They add counts or heap storage and propagate suspension through every caller.

The shown `OnExit` also accepts only an empty requirement row.
An arbitrary requirement-bearing cleanup cannot fit its closure field without another language mechanism.

Multiple guards retain current `using` order.
Mixed resource and callback cleanup also retains one last-in, first-out stack.

The unused name is visible noise.
Adding `using _ =` would add grammar immediately after this option removed grammar.

Diagnostics would point at closure escape or suspension, not at cleanup intent.
An agent following familiar `defer` prior art would first write syntax that no longer exists.

This is the cheapest implementation because it is only a core-library addition.
It does not preserve the current feature, so it is not a behavioral replacement.

## Option 3: Split Synchronous And Asynchronous Disposal

This option first makes resource effects explicit, following C# and TypeScript.
`Disposable.dispose` is synchronous, while `AsyncDisposable.dispose!` may suspend.

```text
fn audit(log: &mut List[string], stop: bool) -> bool:
    using cleanup = on_exit(fn(): log.push("closed")) # hypothetical scoped capture
    log.push("opened")
    if stop:
        return false
    true

fn transfer!() -> void:
    using! connection = connect!() # hypothetical async-using syntax
```

The stdlib tier adds a synchronous guard and splits the disposal protocols.
The language tier adds selection and ordering rules for synchronous and asynchronous bindings.

The shown spelling adds `using!`, though `await using` or two distinct statements are alternatives.
Every spelling adds syntax after deleting `defer:` syntax.

Preserving `&mut log` additionally needs a scoped closure type.
The parked Tier 2 model can express such captures, but that core design is not accepted.

Until that core exists, the guard still fails the representative use case.
This option therefore waits on lifetime and NonEscapable decisions.

Row-polymorphic guard storage is a separate missing core feature.
Scoped lifetimes alone do not let a struct field abstract over every callback requirement row.

With those decisions, the guard can remain in the frame and avoid counted capture storage.
It still needs measurement against direct cleanup lowering after #858.

The soundness burden is larger than today's `defer:` suite.
The checker must prove the guard cannot escape, move, or outlive each captured local reference.

Diagnostics can name synchronous versus asynchronous cleanup directly.
They must also explain lifetime failures introduced solely by a cleanup registration.

## Option 4: A Compiler-Known Function

This option removes statement grammar but keeps current semantics behind a recognized call.
The compiler treats `on_exit` as registration, not as an ordinary function call.

```text
fn audit(log: &mut List[string], stop: bool) -> bool:
    on_exit(fn(): log.push("closed")) # hypothetical intrinsic semantics
    log.push("opened")
    if stop:
        return false
    true
```

The language tier removes `defer_statement` but adds an intrinsic and new registration rules.
The library tier can expose the intrinsic's signature, but ordinary hd cannot implement its behavior.

This option can preserve local-reference access and plain functions.
It can lower to the current cleanup frame without a runtime closure or guard object.

The surface call is misleading.
Moving it into a helper, storing its result, or shadowing its name cannot follow ordinary call rules.

The diagnostic must teach that one apparent std call changes the caller's control flow.
That conflicts with hd's no-action-at-a-distance direction.

This option removes syntax but not a mechanism.
It replaces a visible language construct with a compiler intrinsic and a semantic exception.

## Option 5: Explicit Cleanup Only

This radical option removes arbitrary automatic cleanup and adds no replacement.
`using` remains only for types that already implement `Disposable`.

```text
fn audit(log: &mut List[string], stop: bool) -> bool:
    log.push("opened")
    if stop:
        log.push("closed")
        return false
    log.push("closed")
    true
```

The language tier removes `flow.defer.*` and `defer_statement`.
No stdlib or compiler mechanism is added.

This has the smallest mechanism count and no closure cost.
It duplicates cleanup at each exit and makes a missed path a program bug.

Refactoring introduces risk because every new `return`, `break`, `continue`, or `?` needs review.
Nested scopes and result-producing blocks amplify that risk.

Agents can write this syntax immediately.
They are less likely to maintain it correctly because cleanup intent is not mechanically registered.

This option gives up the primary safety property of `defer`.
It is suitable only if arbitrary cleanup is judged unnecessary in hd programs.

## Comparison

| Criterion | 1. Keep split | 2. Current guard | 3. Split disposal | 4. Intrinsic call | 5. Explicit only |
| --- | --- | --- | --- | --- | --- |
| Plain `fn` cleanup | Yes | No | Yes, for sync guard | Yes | Yes |
| Captures `&mut` local reference | Yes | No | Only after scoped closures | Yes, specially | Not retained |
| Latest local storage | Yes | Counted alias only | Yes after lifetime work | Yes | Manually |
| Suspending resource release | `using` | Every guard appears suspending | Separate visible form | Separate `using` | `using` |
| LIFO on all structured exits | Yes | Yes | Yes | Yes | Manual duplication |
| Rules added | None | Two std items | Two traits, scoped guards, async-binding rules | Intrinsic and semantic rules | None |
| Rules removed | None | `flow.defer.*`, grammar | `flow.defer.*`, grammar; current disposal rules rewritten | Grammar only; semantics remain | `flow.defer.*`, grammar |
| Soundness burden | Current suite checks | Ordinary closure escape checks | Lifetime, move, and effect checks | Compiler special case | Missed cleanup paths |
| Requirement-bearing cleanup | Uses enclosing row | Not generic over rows | Needs row-polymorphic storage | Can preserve enclosing row | Direct calls use enclosing row |
| Agent writability | Familiar keyword and typed resource | Unused name, `!`, counted captures | More forms and lifetime errors | Looks ordinary but is not | Easy first draft, fragile edits |
| Human readability | Cleanup intent is explicit | Intent split across constructor and binding | Effects explicit but verbose | Hidden control effect | Cleanup duplicated far from setup |
| Implementation cost | None | Core library only | Language, checker, std, lowering | Compiler intrinsic and lowering | Deletion and migration |
| Runtime cost | Direct cleanup frame | Closure plus guard unless optimized | Frame guard if proven nonescaping | Same ceiling as current lowering | Direct calls, duplicated code |
| Evolution | Stable split | Blocks local-reference-first style | Depends on parked Tier 2 | Permanent intrinsic contract | Hard to restore safety without a new mechanism |

By the repository's design-cost order, Option 2 has the cheapest added mechanism.
It fails required behavior, so its low implementation cost is not a complete comparison.

Option 4 costs a compiler intrinsic but preserves behavior.
Option 3 has the highest design cost because it changes effects, lifetimes, syntax, and std protocols.

## Recommendation

Keep Option 1: retain `defer:` for arbitrary synchronous lexical cleanup and `using` for typed disposal.
It is the only option that preserves current behavior without adding a more expensive mechanism.

The prior-art default supports this split.
Go, Zig, and Swift use visible statements, while C#, Java, and TypeScript use resource bindings.

The recommendation gives up removing one keyword and one grammar production.
That cost buys local-reference access, plain-function cleanup, and visible control-flow semantics.

The next-best option is Option 2 as an additive convenience, not as a replacement.
`on_exit` could serve counted callbacks in suspending code without changing `defer:`.

That additive API should still wait for a measured use case.
Without one, it duplicates existing syntax and teaches a weaker cleanup path.

## Questions For The Owner

1. **Should #879 close with `defer:` retained?** Candidates are Option 1, or Option 2 as a semantic reduction. Recommendation: Option 1.

```text
fn work(log: &mut List[string]) -> void:
    defer: log.push("closed")
```

2. **If one mechanism is mandatory, which behavior may change?** Candidates are suspension propagation, local-reference capture, or neither. Recommendation: neither.

```text
fn work(log: &mut List[string]) -> void:
    on_exit(fn(): log.push("closed")) # hypothetical replacement semantics
```

3. **Should synchronous and asynchronous disposal become visibly distinct?** Candidates are keep one `using`, add `using!`, or adopt `await using`. Recommendation: keep one until an independent resource-design need appears.

```text
fn transfer!() -> void:
    using! connection = connect!() # hypothetical async-using syntax
```

4. **Should an additive callback guard be explored after #858?** Candidates are measure `on_exit`, park it, or reject it. Recommendation: park until real code needs it.

```text
fn work!(log: *mut List[string]) -> void:
    using cleanup = on_exit(fn(): log.push("closed"))
```

## Sources

- [Go specification: Defer statements](https://go.dev/ref/spec#Defer_statements)
- [Zig language reference: `defer`](https://ziglang.org/documentation/master/#defer)
- [Zig language reference: `errdefer`](https://ziglang.org/documentation/master/#errdefer)
- [Swift language reference: Defer Statement](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/statements/#Defer-Statement)
- [Rust Reference: Destructors](https://doc.rust-lang.org/reference/destructors.html)
- [Rust `scopeguard` crate](https://docs.rs/scopeguard/latest/scopeguard/)
- [C# reference: `using` statement](https://learn.microsoft.com/dotnet/csharp/language-reference/statements/using)
- [Java Language Specification: try-with-resources](https://docs.oracle.com/javase/specs/jls/se21/html/jls-14.html#jls-14.20.3)
- [TypeScript 5.2: Explicit Resource Management](https://www.typescriptlang.org/docs/handbook/release-notes/typescript-5-2.html#using-declarations-and-explicit-resource-management)
- [Python `contextlib.ExitStack`](https://docs.python.org/3/library/contextlib.html#contextlib.ExitStack)
- [Kotlin `use`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.io/use.html)

## Parse Log

Parsing checks syntax only, not names, types, effects, or the proposed semantics.
Each `text` block was passed twice to `scripts/hd-conformance.sh parse`.

| Block | Result | Note |
| --- | --- | --- |
| Domain baseline | Parsed | Current syntax. |
| Option 1 | Parsed | Current syntax. |
| Option 2 | Parsed | Current syntax; intended to expose current semantic catches. |
| Option 3 | Rejected as expected | Line 9: `syntax-error: expected a line ending, found '!'`; `using!` is hypothetical. |
| Option 4 | Parsed | Existing call syntax with hypothetical intrinsic semantics. |
| Option 5 | Parsed | Current syntax. |
| Owner question 1 | Parsed | Current syntax. |
| Owner question 2 | Parsed | Existing call syntax with hypothetical semantics. |
| Owner question 3 | Rejected as expected | Line 2: `syntax-error: expected a line ending, found '!'`; `using!` is hypothetical. |
| Owner question 4 | Parsed | Current syntax. |
