# Scope Guards

**Status:** proposal for owner review. Nothing here is accepted language or
library behavior. The checked package beside this document models only what
current hd can express; comments marked `Target` are hypothetical.

## Proposal

Keep `defer:`. Add a synchronous disposal protocol and make the cleanup effect
visible in the binding:

```text
pub trait Disposable:
    fn dispose(&mut self) -> void

pub trait AsyncDisposable:
    fn dispose!(&mut self) -> void

using guard = on_exit(fn once() ->{audit} void:
    audit.push("closed")
)

using! socket = connect!()
```

`using` accepts `Disposable` and works in both ordinary and suspending
functions. `using!` accepts `AsyncDisposable` and needs a suspending function.
The latter spelling follows hd's existing bang convention rather than adding
C#'s second effect word, `await`. The current suspending `Disposable` becomes
`AsyncDisposable`; current `using` sites become `using!` sites.

An ordinary `let` or `:=` binding never disposes a value implicitly. A reader
can find every end-of-scope action by looking for `defer:`, `using`, and
`using!`. A constructor name such as `on_exit` is not the convention: its
result runs only when the caller chooses one of the two visible bindings.

Both forms register on the existing lexical cleanup stack. `defer:` and
synchronous disposal run inline; asynchronous disposal is awaited. All three
run in reverse registration order and otherwise keep the accepted cleanup
semantics. This proposal changes neither panic cleanup nor cancellation.

The standard library can then provide an ordinary guard:

```text
pub struct ScopeGuard[{l}] ~Copy:
    # Illustrative tier-2 spelling, not a proposed lifetime mechanism.
    action: fn once() ->{l} void

pub fn on_exit[{l}](action: fn once() ->{l} void) -> ScopeGuard[{l}]

impl[{l}] Disposable for ScopeGuard[{l}]:
    fn dispose(&mut self) -> void
```

`ScopeGuard` and `on_exit` are library code. The language knows only the two
protocols and bindings. Requirement-row polymorphism is separate work in #961.

## Questions For The Owner

1. **Should synchronous and suspending cleanup have separate protocols and
   binding spellings?** **Recommendation: yes: `Disposable` / `using` and
   `AsyncDisposable` / `using!`.** The call site says whether scope exit may
   suspend, ordinary functions need no driver, and a type implementing both is
   unambiguous. The cost is a breaking rename of today's protocol and sites.

2. **Should an ordinary binding ever invoke disposal?** **Recommendation: no.**
   Requiring `using` or `using!` is the single visible convention. Automatic
   `Drop` would make guard construction shorter, but would hide scope-exit code
   behind an ordinary call.

3. **What must tier 2 guarantee for a guard over locals?**
   **Recommendation: a guard carries the capture lifetime of its action, cannot
   outlive any captured local, and is destroyed before those locals.** A
   mutable capture is exclusive for the guard's lifetime; code that still
   needs the value must access it through a guard designed to expose it. Making
   a deferred mutable capture coexist with direct later access would need an
   additional tier-2 rule and is deliberately not proposed here.

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

Tier 2 must make the lifetime visible in both the closure and the guard. The
guard may be returned only if `{log}` remains valid, may not be stored in a
global or longer-lived object, and must run before `log` dies. The action is
`fn once` and the guard is noncopyable so cleanup happens once.

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
fn ordinary(log: &mut List[string]) -> void:
    # Target. The cleanup protocol and this function are both synchronous.
    using guard = on_exit(fn once() ->{log} void:
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

The type alone does not cause action at a distance. Code search for `using`,
`using!`, or `defer:` finds every registration point. The variable need not
follow a naming convention; names such as `guard` are documentation, not
semantics.

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
`using` rule, and one renamed plus one new protocol. It adds no intrinsic:
guard storage, once-only callbacks, and `on_exit` remain library code. The
compiler already has lexical cleanup entries and asynchronous disposal; it
adds an inline synchronous entry.

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

The recommended option has the most expensive change in the project's design
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
| Ordinary binding of a disposable | no cleanup | only visible cleanup bindings register |
| Mixed `defer`, `using`, `using!` | one reverse-order stack | async entries are awaited in place |

The proposal cannot express a stored closure that mutably captures a local
while unrelated code keeps accessing that local directly. That is the
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
uses lifetimes to guard borrowed values. Its protected value is accessed
through the guard, which preserves an exclusive borrow. Hd adopts that
lifetime condition, not automatic drop: `using` stays visible at the binding.

### Python

Python [`contextlib.contextmanager`](https://docs.python.org/3/library/contextlib.html#contextlib.contextmanager)
turns `try` / `yield` / `finally` into a value used by a visible `with` block.
The visible block is a good fit; generator-driven dynamic cleanup and exception
suppression are not part of this proposal.

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
bindings never register cleanup. The checked package beside this document
models a guard with today's suspending protocol and marks all target syntax in
comments.

## Appendix E: Details Deferred Elsewhere

- Tier-2 lifetime syntax and capture checking remain in #163 and #833.
- Requirement-row polymorphism for generic guards remains in #961.
- The exact std module for `ScopeGuard` is an implementation decision after
  the language design is accepted.
- Panic and cancellation cleanup keep their accepted behavior and are not
  reopened here.
