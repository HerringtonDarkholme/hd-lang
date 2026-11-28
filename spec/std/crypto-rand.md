# Secure random bytes

Status: standard library specification draft.

`std.crypto.rand` exposes bytes from the existing random provider.
Default hosts use an operating-system cryptographic random source.

```text
use std.crypto.rand.fill
use std.random.Random

fn nonce() -> List[u8] $ Random:
    fill(12)
```

1. r[std-crypto-rand.import] `std.crypto.rand` re-exports `std.random.fill`; neither the module nor `fill` is a prelude name.
2. r[std-crypto-rand.fill] `fill(count: usize) -> List[u8] $ Random` returns the covering `Random` provider's `fill(count)` result unchanged.
3. r[std-crypto-rand.delegation] Each call invokes the provider exactly once, including a zero-byte request, without drawing integers or supplying a seeded fallback.
4. r[std-crypto-rand.default] The default host provider supplies the requested number of bytes from a cryptographically secure system source.
5. r[std-crypto-rand.override] An explicit `Random` provider override supplies its own semantics, including deterministic providers for tests.

> **Why.** Re-exporting the existing operation preserves requirement
> overrides and adds neither a second provider nor a host import.
> Deterministic generators remain available through `std.random`.

> **Note.** A deterministic override does not supply secure entropy.

See also: [Random](random.md), [Crypto](crypto.md).
