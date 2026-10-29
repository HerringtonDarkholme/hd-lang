# Contract

Status: standard library specification draft.

This chapter defines the always-checked precondition helper in `std.contract`.
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

1. r[std-contract.require.decl] `std.contract` declares `pub fn require(message: string = "requirement failed", condition: fn() -> bool) -> void`.
2. r[std-contract.require.once] A `require` call evaluates `condition` exactly once.
3. r[std-contract.require.pass] If `condition` returns true, `require` returns without panicking.
4. r[std-contract.require.fail] If `condition` returns false, `require` calls the prelude `panic` with `message`. Panic: `explicit-panic`.
5. r[std-contract.require.profiles] `require` has the same behavior in every build profile.

> **Why.** Precondition failures are programming errors. Checking every profile
> avoids profile-dependent correctness, and ordinary panic keeps the mechanism
> in std.
