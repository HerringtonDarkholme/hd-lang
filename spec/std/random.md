# Random

Status: standard library specification draft.

This chapter defines `std.random`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability trait `Random`;
- `SeededRandom`, its deterministic provider.

Which provider a command binds is CLI tier
([Host Capabilities](../cli/command-line.md#host-capabilities)).

## Random Source

`Random` draws values from a source of randomness:

```text
pub trait Random:
    fn next_u64(mut self) -> u64
    fn fill(mut self, count: usize) -> List[u8]
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

> **Note.** A property test's random draws also come from a `Random`
> provider, one that `std.testing` seeds from the test runner
> ([`std-testing.runner.random`](testing.md#r-std-testing.runner.random)).

## Seeded Random

`SeededRandom` is the deterministic `Random` provider. A test supplies it
with `$.with`, and each seed gives one fixed sequence of draws:

```text
use std.random.{Random, SeededRandom}
use std.testing.assert_equal

fn roll() -> u64 $ Random:
    $.use(Random).next_u64() % 6 + 1

tests:
    it("one seed, one sequence"):
        let mut first = SeededRandom::new(7)
        let mut again = SeededRandom::new(7)
        let rolled: u64 = 0
        $.with(Random=first):
            rolled = roll()
        $.with(Random=again):
            assert_equal(roll(), rolled, reason="the same seed rolls the same")
```

1. r[std-random.seeded.decl] `std.random` declares `SeededRandom`, which implements `Random`, with private fields. Code imports it, as in `use std.random.SeededRandom`.
2. r[std-random.seeded.new] `SeededRandom::new(seed: u64) -> mut SeededRandom` returns a provider that `seed` starts.
3. r[std-random.seeded.same-seed] Two providers that one seed starts return the same values for the same sequence of `next_u64` and `fill` calls.
4. r[std-random.seeded.no-host] A `SeededRandom` draws from its own state only. It reads nothing from the host.

> **Note.** These rules fix no generator, so a seed's values may differ
> between implementations. A test compares draws with each other, not
> with written numbers.

See also: [Mutable Providers](../lang/11-requirements-and-suspension.md#mutable-providers).
