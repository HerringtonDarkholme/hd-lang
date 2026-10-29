# Scope Guards

**Status:** design accepted 2026-10-28; being built (#962). The checked package
that modeled today's guard is in git history at 628a7f92e,
`docs/future-work/scope-guards/`.

## What The First Build Covers

The first build ships what current hd can express:

- `Disposable` becomes synchronous (`fn dispose(&mut self) -> void`), and the
  current suspending protocol becomes `AsyncDisposable`;
- `using` takes a `Disposable` in any function, and `using!` takes an
  `AsyncDisposable` in a suspending one; today's `using` sites become `using!`;
- `ScopeGuard` and `on_exit` in std, taking only an empty-row callback;
- the `unbound-disposable` lint at error level.

Later work: `ScopeGuard[$R]` after #961, and the tier-2 captures,
`AutoClosing`, `@auto_closing`, `defer(action)` and `ensure(action)` in #978.

## Accepted Design

Keep `defer:`. Add a synchronous disposal protocol and make the cleanup effect
visible in the binding:

```text
pub trait Disposable:
    fn dispose(&mut self) -> void

pub trait AsyncDisposable:
    fn dispose!(&mut self) -> void

let audit: *mut List[string] = &mut []
using guard = on_exit(fn() -> void:
    audit.push("closed")
)

using! socket = connect!()
```

`using` accepts `Disposable` and works in both ordinary and suspending
functions. `using!` accepts `AsyncDisposable` and needs a suspending function.
The latter spelling follows hd's existing bang convention rather than adding
C#'s second effect word, `await`. The current suspending `Disposable` becomes
`AsyncDisposable`; current `using` sites become `using!` sites.

An ordinary `let` or `:=` binding never disposes an ordinary `Disposable` or
`AsyncDisposable` implicitly. A reader can find every end-of-scope action by
looking for `defer:`, `using`, `using!`, or the accepted auto-closing call
convention below.

Both forms register on the existing lexical cleanup stack. `defer:` and
synchronous disposal run inline; asynchronous disposal is awaited. All three
run in reverse registration order and otherwise keep the accepted cleanup
semantics. This design changes neither panic cleanup nor cancellation.

The standard library can then provide an ordinary guard:

```text
pub struct ScopeGuard:
    action: fn() -> void

pub fn on_exit(action: fn() -> void) -> ScopeGuard:
    panic("todo")

impl Disposable for ScopeGuard:
    fn dispose(&mut self) -> void
```

`ScopeGuard` and `on_exit` are library code. The language knows only the two
protocols and bindings. The current form accepts only a callback with an empty
requirement row.

#961 adds row parameters to data types, so the guard can retain any callback
row instead of erasing it:

```text
# Depends on #961; this row-parameter form is not implemented today.
pub struct ScopeGuard[$R]:
    action: fn() -> void $ R

pub fn on_exit[$R](action: fn() -> void $ R) -> ScopeGuard[$R]:
    panic("todo")
```

### Auto-closing guard calls

**Deferred to tier 2 (#978).** The marker trait, decorator, and the library
`defer(action)` / `ensure(action)` helpers below are accepted design only; the
current #962 build does not implement them.

The matched working names are marker trait `AutoClosing` and decorator
`@auto_closing`. An `AutoClosing` type exists only to register cleanup; its
marker implementation refines its synchronous `Disposable` or suspending
`AsyncDisposable` protocol and identifies which cleanup entry to register.

`@auto_closing` is valid only on a function whose return type implements
`AutoClosing`. Any other return type is an error at the declaration. Calling an
auto-closing function as a statement registers its result on the same cleanup
stack as `defer:`, `using`, and `using!`. The entries keep one LIFO order. Using
the call as a value is an error.

A marker-typed result can never sit unregistered. Binding it with `:=` or
`let`, storing it, or returning it from a function without `@auto_closing` is a
compile error. Ordinary disposable values keep explicit `using` / `using!` and
the lint rule below; only the marker opts into this stricter convention.

The standard library can express `defer(action)` and `ensure(action)` as
ordinary functions with `@auto_closing` once the accepted dependencies are
implemented. The decorator and marker have the same `auto_closing` stem so the
registration rule is searchable at both the producer and call.

## The Three Gaps

### 1. Local references

Today this useful guard is rejected because a closure cannot capture a local
reference:

```text
fn record(log: &mut List[string]) -> void:
    do_work(log)
    # Target tier-2 syntax. Current hd reports fn.capture.no-local-reference.
    using guard = on_exit(fn once() ->{log} void:
        log.push("scope closed")
    )
```

The accepted tier-2 direction makes the lifetime visible in both the closure
and the guard. The guard may be returned only if `{log}` remains valid, may not
be stored in a global or longer-lived object, and must run before `log` dies. The action is
`fn once` and the guard is noncopyable so cleanup happens once.

The corresponding hypothetical library shape is:

```text
# Illustrative tier-2 spelling, not a proposed lifetime mechanism.
pub struct ScopeGuard[{l}] ~Copy:
    action: fn once() ->{l} void

pub fn on_exit[{l}](action: fn once() ->{l} void) -> ScopeGuard[{l}]
```

There is one important soundness boundary. A normal tier-2 closure that writes
through `&mut log` holds an exclusive borrow. The example above cannot both
keep that borrow in `guard` and call `do_work(log)` directly. A library guard
can instead own `&mut log` and expose access through itself, as Rust's
`scopeguard::ScopeGuard` exposes its protected value. Giving `defer:`-like late
capture to a stored closure would be a new tier-2 mechanism; this design does
not smuggle it in.

### 2. Synchronous code

The current workaround needs a bang function even when its action cannot
suspend:

```text
fn ordinary(log: *mut List[string]) -> void:
    # Target binding; the closure and counted reference are current hd.
    using guard = on_exit(fn() -> void:
        log.push("closed")
    )
```

`using` calls `Disposable.dispose` inline, so it works here and in `fn!`.
`using!` remains rejected here because `AsyncDisposable.dispose!` needs a
driver. No inference from the method body decides which form applies.

### 3. Visible end of scope

The same constructor is inert under an ordinary binding and active under a
cleanup binding:

```text
ignored := on_exit(fn(): audit("not registered"))
using registered = on_exit(fn(): audit("runs at scope exit"))
```

An ordinary constructor remains inert: `using` / `using!` registers its result.
An auto-closing function is the one exception, visible on its declaration, and
its statement call registers immediately rather than waiting for an ordinary
binding to die. Variable names such as `guard` are documentation, not semantics.

### Unbound disposable lint

`hd lint` reports `unbound-disposable` when a created `Disposable` or
`AsyncDisposable` value is used as a statement or bound by ordinary `:=` or
`let`. Its default level is error, and a package or site may allow it. The lint
is quiet when the value is bound by `using` / `using!`, passed on, returned, or
stored:

```text
ignored := on_exit(fn(): audit("not registered")) # lint: unbound-disposable
using registered = on_exit(fn(): audit("runs at scope exit"))
install(on_exit(fn(): audit("the callee owns this guard"))) # quiet
```

This remains an `hd lint` finding rather than a type error. It preserves
deliberate transfer while catching the spellings most likely to be mistaken for
a registered cleanup. `AutoClosing` values use the compile errors above instead
and cannot allow an unregistered state.

## How Scope Exit Works

1. Evaluate the initializer.
2. Bind the value read-only, as current `using` does.
3. Register one cleanup entry tagged synchronous or asynchronous.
4. On every accepted lexical exit, pop entries in reverse order.
5. Invoke a synchronous entry directly. Await an asynchronous entry under the
   existing cleanup bound and cancellation rules.

Mixed cleanup has one order:

```text
using first = sync_guard()
using! second = async_handle!()
defer: mark("third")
# exit: third, await second, first
```

A type may implement both protocols. The binding spelling selects one; the
compiler never guesses. A value transferred out of a cleanup binding follows
the current `using` transfer rules.

## Cost

This is one syntax extension (`using!`), one semantic split of the existing
`using` rule, one renamed plus one new protocol, one marker/decorator
convention, and one lint. It adds no intrinsic: guard storage, once-only
callbacks, `on_exit`, `defer(action)`, and `ensure(action)` remain library code.
The compiler already has lexical cleanup entries and asynchronous disposal; it
adds an inline synchronous entry and the auto-closing registration checks.

The source migration is mechanical but broad: current `Disposable` becomes
`AsyncDisposable`, and current `using` becomes `using!`. The benefit is that
ordinary code pays no driver requirement and every call site exposes whether
cleanup may suspend.

| Option | Soundness condition | Design cost |
| --- | --- | --- |
| Separate `using` / `using!` | selected protocol determines whether exit may suspend; tier-2 captures remain within their lifetimes | one syntax extension, one semantic split, one protocol rename, one protocol addition |
| One `using`, two protocols | an explicit priority or an error resolves types implementing both | one semantic exception and one protocol addition; effect stays hidden |
| Scoped combinator | callback is non-escaping and always invoked exactly once | core-library addition, but nested control flow and no stored guard |
| Automatic drop | compiler proves exactly-once destruction after captured locals remain live | semantic rule plus compiler-known destruction; cleanup is invisible |
| Keep only `defer:` | existing lexical capture rules remain the sole cleanup path | no change, but reusable guards remain weaker |

The accepted option has the most expensive change in the project's design
cost order: syntax. It wins despite that cost because the cheaper one-spelling
alternative needs an ambiguity rule and conceals suspension at scope exit.

## Appendix A: Stress Test

| Case | Result | Soundness condition |
| --- | --- | --- |
| Guard in an ordinary function | `using` works | `dispose` has no bang and cannot suspend |
| Guard in a suspending function | `using` works | cleanup runs inline in the shared stack |
| Network handle | `using!` works | the enclosing function has a driver |
| Shared local capture | tier-2 guard may hold `&T` | guard lifetime is no longer than the referent |
| Mutable local capture | tier-2 guard may hold `&mut T` | exclusive access lasts until guard disposal |
| Return a local guard | rejected unless all captured lifetimes outlive the result | capture set appears in the result type |
| Copy a guard | rejected | guard and `fn once` action are noncopyable |
| Type implements both protocols | caller chooses `using` or `using!` | no protocol inference |
| Ordinary statement/binding of a disposable | `unbound-disposable`, error level by default | allow is available; transfer, return and storage stay quiet |
| Auto-closing call statement | its marker-typed result registers cleanup | using the call as a value is an error |
| Unregistered marker-typed value | rejected | plain binding, storage and undecorated return are errors |
| Mixed `defer`, `using`, `using!` | one reverse-order stack | async entries are awaited in place |

The accepted design cannot express a stored closure that mutably captures a
local while unrelated code keeps accessing that local directly. That is the
deliberate anti-case: accepting it as an ordinary `&mut` capture would violate
tier 2's exclusivity rule. `defer:` remains the direct spelling for late
lexical access.

## Appendix B: Prior Art

### C#

C# separates [`IDisposable` with `using` from `IAsyncDisposable` with
`await using`](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/statements/using).
Both bindings visibly delimit cleanup and dispose in reverse order; the async
form makes suspension explicit. Hd follows this split, but writes the existing
effect marker as `using!` instead of introducing `await` syntax.

### Swift

Swift closure parameters are [non-escaping by default, with `@escaping`
required when a closure is stored](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/closures/#Escaping-Closures).
That supports a call-bounded cleanup combinator, but a scope guard necessarily
stores its action until scope exit. Hd therefore needs tier-2 capture lifetimes
rather than treating the callback as non-escaping.

### Rust

Rust [`Drop`](https://doc.rust-lang.org/core/ops/trait.Drop.html) runs
automatically when a value leaves scope, and
[`scopeguard::ScopeGuard`](https://docs.rs/scopeguard/latest/scopeguard/struct.ScopeGuard.html)
uses lifetimes to guard borrowed values. `ScopeGuard` is `#[must_use]`, so an
immediately dropped guard warns. Its protected value is accessed through the
guard, which preserves an exclusive borrow. Hd adopts the lifetime and warning
conditions, not automatic drop: `using` stays visible at the binding.

C#'s
[`CA2000`](https://learn.microsoft.com/en-us/dotnet/fundamentals/code-analysis/quality-rules/ca2000)
warns when a disposable object is not disposed before references leave scope.
Hd's lint is narrower and defaults to error level: it checks the visible
binding or transfer at creation, and remains allow-able like other lint rules.

### Python

Python [`contextlib.contextmanager`](https://docs.python.org/3/library/contextlib.html#contextlib.contextmanager)
turns `try` / `yield` / `finally` into a value used by a visible `with` block.
The visible block is a good fit; generator-driven dynamic cleanup and exception
suppression are not part of this design.

### Go And Zig

Go [`defer`](https://go.dev/ref/spec#Defer_statements) registers a call for
function return. Zig [`defer`](https://ziglang.org/documentation/master/#defer)
runs at lexical scope exit and in reverse order. Hd already keeps its more
capable lexical `defer:`. These languages support keeping that statement, but
do not solve the requested guard convention or synchronous/asynchronous
protocol split.

## Appendix C: Alternatives

### One `using`, two protocols

Let `using` choose synchronous or asynchronous disposal by implemented trait.
This avoids new syntax, but a type implementing both is ambiguous and a reader
inside `fn!` cannot tell whether scope exit suspends. A priority rule would add
a semantic exception. Rejected in favor of an explicit effect at the call
site.

### Keep Today's Names

Add `SyncDisposable` while leaving today's suspending protocol named
`Disposable`. This reduces migration, but makes the unqualified name mean the
more constrained asynchronous case forever. Rejected because the C# split is
clearer and the migration is mechanical.

### A Scoped Combinator

`with_guard(value, fn(guard): ...)` can use a non-escaping callback and needs
no stored local-reference closure. It adds nesting, cannot share the existing
cleanup order naturally with surrounding registrations, and makes early-exit
control flow harder. It is useful library code, not the sole convention.

### Automatic Drop

Make every guard run when an ordinary binding dies. This matches Rust and
needs no `using` syntax, but violates the requested visible end-of-scope
convention. Construction would hide later code execution. Rejected.

### Only `defer:`

Keep directing users to the statement. It already captures lexical locals and
works synchronously, but a library cannot package a reusable cleanup policy as
a guard. That is the under-built state this issue asks to address.

## Appendix D: Today

Today's `Disposable.dispose!` may suspend, so `using` is legal only under a
driver. Current closures cannot capture local references, and current ordinary
bindings never register cleanup. The package at 628a7f92e modeled a guard
with today's suspending protocol and marked all target syntax in comments.

## Appendix E: Details Deferred Elsewhere

- The accepted tier-2 lifetime and exclusivity direction is recorded in #978;
  it is not built in this slice.
- #961 supplies `ScopeGuard[$R]`, so `on_exit` retains the action's requirement
  row instead of accepting only an empty-row callback.
- The exact std module for `ScopeGuard` is an implementation decision.
- Panic and cancellation cleanup keep their accepted behavior and are not
  reopened here.

## Appendix F: Parse Log

All nine `text` blocks were checked with `scripts/hd-conformance.sh parse` on
2026-10-28. The five current-syntax blocks parse. The four blocks containing
target `using!`, `fn once`, or lifetime syntax reject as expected. Parsing
validates syntax only, not the proposed names or behavior.
