# `std.contract`

Generated from `hd doc std.contract`. Run `hd doc std.contract.NAME` in a package for one item's full text.

## `EnsureGuard`

```text
pub struct EnsureGuard[F < Fn() -> bool & 'a, 'a]
```

A postcondition guard registered by `ensure`.

## `ensure`

```text
pub fn ensure[F < Fn() -> bool & 'a, 'a](message: string, condition: F) -> EnsureGuard[F, 'a]
```

Registers a postcondition checked at every normal exit of the calling scope.

## `require`

```text
pub fn require[F < Fn() -> bool](message: string = "requirement failed", condition: F) -> void
```

Panics unless `condition` returns true.

## `require`

```text
pub fn require[F < Fn() -> bool](message: string = "requirement failed", condition: F) -> void
```

Reference: [std.contract](spec/std/contract.md)
