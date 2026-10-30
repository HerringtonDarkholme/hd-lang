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
