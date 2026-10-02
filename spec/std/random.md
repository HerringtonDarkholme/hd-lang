# Random

Status: standard library specification draft.

This chapter defines `std.random`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability trait `Random`.

Which provider a command binds is CLI tier
([Host Capabilities](../cli/command-line.md#host-capabilities)).

## Random Source

`Random` draws values from a source of randomness:

```text
pub trait Random:
    fn next_u64(mut self) -> u64
    fn fill(mut self, count: i32) -> List[u8]
```

1. r[std-random.decl] `std.random` declares the host capability trait `Random` with the methods above. Code imports it, as in `use std.random.Random`.
2. r[std-random.next] `next_u64` returns the next value of the provider's source.
3. r[std-random.fill] `fill(count)` returns `count` values of that source, as bytes.
4. r[std-random.plain] Both methods are plain calls, not bang calls.
5. r[std-random.mut] Both take `mut self`, so `Random` is a mutable requirement trait and a seeded provider may advance its own state.

```text
use std.random.Random

fn roll() -> u64 $ Random:
    $.use(Random).next_u64() % 6 + 1
```

> **Why.** A random draw returns a value from the host, as a clock read
> does, so it needs no driver. A test installs a seeded provider, and
> replay records each draw at the boundary.

See also: [Mutable Providers](../lang/11-requirements-and-suspension.md#mutable-providers).
