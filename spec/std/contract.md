# Contract

Status: standard library specification draft.

This chapter defines the development precondition helper in `std.contract`.
It is an ordinary hd function over the language-tier `panic` function.

## Require

`require` checks a caller-supplied condition before work continues:

```text
use std.contract.require

fn debit(balance: i64, amount: i64) -> i64:
    require:
        amount > 0
    require("balance must cover the amount"):
        balance >= amount
    balance - amount
```

1. r[std-contract.require.decl] `std.contract` declares `pub fn require[F < Fn() -> bool](message: string = "requirement failed", condition: F) -> void`, with `Fn` from `std.function`.
2. r[std-contract.require.debug-once] In a debug or test build, a `require` call evaluates `condition` exactly once.
3. r[std-contract.require.debug-pass] In a debug or test build, if `condition` returns true, `require` returns without panicking.
4. r[std-contract.require.debug-fail] In a debug or test build, if `condition` returns false, `require` calls the prelude `panic` with `message`. Panic: `explicit-panic`.
5. r[std-contract.require.read-only] `condition` is an `Fn` argument, so by [`fn.trait.read-only`](../lang/07-functions.md#r-fn.trait.read-only) it does not change a capture. A closure that does is an error. Error: `not-writable`.
6. r[std-contract.require.read-refs] By [`ref.generic.outlives.argument`](../lang/13-values-and-references.md#r-ref.generic.outlives.argument), a closure written as the `condition` argument may read local references, such as `self` in a `&mut self` method, for the call.
7. r[std-contract.require.release] In a release build, `require` returns without calling `condition`. Its arguments are evaluated as for any call, and release optimization removes the ones nothing uses.

> **Why.** Precondition failures are development checks. The paired declarations
> remain checked in every profile, while a release build pays no runtime cost.

## Ensure

`ensure` registers a postcondition for the normal exits of its calling scope.

1. r[std-contract.ensure.guard] `std.contract` declares `pub struct EnsureGuard[F < Fn() -> bool & 'a, 'a]`, which holds a private message and condition.
2. r[std-contract.ensure.protocol] `EnsureGuard[F, 'a]` implements `AutoClosing` and `Disposable`; its `dispose` evaluates the condition exactly once.
3. r[std-contract.ensure.decl] `std.contract` declares `@auto_closing pub fn ensure[F < Fn() -> bool & 'a, 'a](message: string, condition: F) -> EnsureGuard[F, 'a]`.
4. r[std-contract.ensure.pass] At a normal exit, a true condition completes silently.
5. r[std-contract.ensure.fail] At a normal exit, a false condition calls the prelude `panic` with `message`. Panic: `explicit-panic`.
6. r[std-contract.ensure.read-only] The condition is an `Fn` argument, so it may read captured local references but does not change a capture. Error: `not-writable`.
7. r[std-contract.ensure.result] The condition receives no result argument; a postcondition about the result stores it in a local that the condition captures.
8. r[std-contract.ensure.order] The guard is registered by the statement call and shares the cleanup stack and last-in, first-out order specified by [`flow.auto-closing.statement`](../lang/06-control-flow.md#r-flow.auto-closing.statement) and [`flow.auto-closing.order`](../lang/06-control-flow.md#r-flow.auto-closing.order).

> **Why.** A postcondition belongs at scope exit, next to the local state it checks, while `Fn` prevents the check itself from changing that state.
