# Random

Status: standard library specification draft.

This chapter defines `std.random`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability trait `Random`;
- `SeededRandom`, its deterministic provider;
- `Rng`, a seeded generator with the draws that scripts need, and `rng`.

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
> with written numbers. An `Rng` fixes its generator, as
> [Rng Generator](#rng-generator) states.

See also: [Mutable Providers](../lang/11-requirements-and-suspension.md#mutable-providers).

## Rng

`Rng` is a generator value with the draws that scripts need. A seed starts
it, or `rng` seeds it from the `Random` provider:

```text
use std.random.{Random, Rng, rng}

fn roll() -> i64 $ Random:
    let mut dice = rng()
    dice.int(1..=6)

fn deal(seed: u64) -> List[string]:
    let mut deck = Rng::from_seed(seed)
    let cards: mut List[string] = ["A", "K", "Q", "J"]
    deck.shuffle(cards)
    cards
```

| Rule | Method | Returns |
| --- | --- | --- |
| r[std-random.rng.next] `next_u64` | `pub fn next_u64(mut self) -> u64` | the next 64-bit draw |
| r[std-random.rng.int] `int` | `pub fn int(mut self, range: Range[i64]) -> i64` | an integer of `range`, each with equal probability |
| r[std-random.rng.float] `float` | `pub fn float(mut self) -> f64` | an `f64` from 0.0 up to, not including, 1.0 |
| r[std-random.rng.bool] `bool` | `pub fn bool(mut self) -> bool` | `true` or `false`, each with equal probability |
| r[std-random.rng.choose] `choose` | `pub fn choose[T](mut self, items: List[T]) -> T?` | one item, each position with equal probability |
| r[std-random.rng.shuffle] `shuffle` | `pub fn shuffle[T](mut self, items: mut List[T]) -> void` | nothing: it reorders `items`, each order with equal probability |
| r[std-random.rng.sample] `sample` | `pub fn sample[T](mut self, items: List[T], count: usize) -> List[T]` | `count` items from distinct positions, in random order |

1. r[std-random.rng.decl] `std.random` declares the data type `Rng`, with private fields, and the methods above. Code imports it, as in `use std.random.Rng`.
2. r[std-random.rng.from-seed] `Rng::from_seed(seed: u64) -> mut Rng` returns a generator that `seed` starts.
3. r[std-random.rng.from-random] `std.random` declares `pub fn rng() -> mut Rng $ Random`. It draws one `next_u64` from the `Random` provider that covers the call, and returns `Rng::from_seed` of that draw.
4. r[std-random.rng.plain] Every `Rng` method is a plain call with the empty requirement row. An `Rng` reads nothing from the host.
5. r[std-random.rng.mut] Every method takes `mut self`, since each draw advances the generator.

> **Why.** A script that draws many values pays for one host draw, and a
> seeded `Rng` needs no provider at all. Go's `math/rand/v2` pairs a
> seeded generator value with a host-seeded default the same way.

### Rng Generator

An `Rng`'s generator is fixed, so a seed gives the same draws in every
implementation:

| Call | Draws |
| --- | --- |
| `Rng::from_seed(42)`, `next_u64` three times | `12186209167316039401`, `2060537544740691614`, `1459241681827062570` |
| `Rng::from_seed(0)`, `next_u64` twice | `16359567918072254692`, `230122937566968419` |
| `Rng::from_seed(7)`, `int(1..=6)` ten times | `6`, `5`, `2`, `5`, `5`, `5`, `4`, `4`, `1`, `6` |
| `Rng::from_seed(7)`, `float` | `0.23382771772151634` |

1. r[std-random.rng.generator] An `Rng`'s state is four 32-bit words, and each 32-bit draw is one step of xoshiro128** 1.0, as the reference code below gives.
2. r[std-random.rng.seeding] `from_seed` folds the seed to the 32-bit `z = (seed ^ (seed >> 32)) mod 2^32`. Then for each state word in order, `z` advances by `0x9E3779B9` modulo 2^32, and the word is `mix32(z)`.
3. r[std-random.rng.mix] `mix32` is the 32-bit finalizer of MurmurHash3, as the reference code gives.
4. r[std-random.rng.next-u64] `next_u64` takes two 32-bit draws. The first is the high half of the result, and the second the low half.

```c
uint32_t mix32(uint32_t z) {
    z = (z ^ (z >> 16)) * 0x85EBCA6B;
    z = (z ^ (z >> 13)) * 0xC2B2AE35;
    return z ^ (z >> 16);
}

uint32_t rotl(uint32_t x, int k) { return (x << k) | (x >> (32 - k)); }

uint32_t next32(uint32_t s[4]) {
    uint32_t result = rotl(s[1] * 5, 7) * 9;
    uint32_t t = s[1] << 9;
    s[2] ^= s[0]; s[3] ^= s[1]; s[1] ^= s[2]; s[0] ^= s[3];
    s[2] ^= t;
    s[3] = rotl(s[3], 11);
    return result;
}
```

> **Why.** A seeded `Rng` serves output that must repeat, such as a level
> that a seed names or a fixture's sample. Its draws therefore cannot
> depend on the implementation. xoshiro128** needs only 32-bit steps, so
> no implementation needs a wide multiplication.

> **Note.** `lib/std`'s `SeededRandom` uses the same generator, but
> [Seeded Random](#seeded-random) does not require it.

### Integer Ranges

`int` takes either form of `Range`, so `0..10` draws from 0 through 9 and
`1..=6` from 1 through 6:

1. r[std-random.rng.int.count] The count `n` of `range` is `end - start` for a `..` range and `end - start + 1` for a `..=` range, computed exactly.
2. r[std-random.rng.int.empty] A range with no integer, where `n` is 0 or less, panics. Panic: `explicit-panic`.
3. r[std-random.rng.int.offset] `int` returns `start + below(n)`.
4. r[std-random.rng.below] `below(n)` draws `next_u64` values until one, `x`, is at least `2^64 mod n`. It returns `x mod n`.
5. r[std-random.rng.below.full] When `n` is 2^64, the whole `i64` range, `below(n)` is one `next_u64` draw.

> **Why.** Rejection keeps the result unbiased. The accepted draws, from
> `2^64 mod n` up, number a multiple of `n`, so each remainder has the
> same share. Fewer than two draws are needed on average, as in OpenBSD's
> `arc4random_uniform`.

> **Note.** `below` is not a public method. The rules name it so that the
> list methods can use it.

### Floats And Booleans

1. r[std-random.rng.float.bits] `float` is the top 53 bits of one `next_u64` draw, times 2^-53. So it is a multiple of 2^-53, and below 1.0.
2. r[std-random.rng.bool.bit] `bool` is `true` exactly when the top bit of one `next_u64` draw is set.

### Lists

1. r[std-random.rng.choose.index] For a list that is not empty, `choose` returns `.Some(items[below(len)])`.
2. r[std-random.rng.choose.empty] For an empty list, `choose` returns `.None` and draws nothing.
3. r[std-random.rng.shuffle.order] `shuffle` is the Fisher–Yates shuffle from the back. For each `i` from `len - 1` down to 1, it swaps the items at `i` and at `below(i + 1)`.
4. r[std-random.rng.shuffle.permutation] So `shuffle` keeps every item and changes only their order.
5. r[std-random.rng.shuffle.short] A list of fewer than two items is unchanged, and `shuffle` draws nothing.
6. r[std-random.rng.sample.order] `sample` shuffles a copy of `items` from the front, for `count` steps. Step `i`, from 0, swaps the copy's items at `i` and at `i + below(len - i)`.
7. r[std-random.rng.sample.result] `sample` returns the copy's first `count` items.
8. r[std-random.rng.sample.input] `items` itself is unchanged.
9. r[std-random.rng.sample.count] A `count` above the list's length panics. Panic: `explicit-panic`.

| Call on `Rng::from_seed(7)` | Result |
| --- | --- |
| `shuffle` of `[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]` | `[10, 3, 7, 4, 6, 5, 1, 9, 2, 8]` |
| `sample([1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 3)` | `[8, 3, 4]` |
| `choose(["a", "b", "c"])` | `.Some("c")` |

> **Why.** A count past the length is a bug in the caller, so it panics,
> as `chunks(0)` does. Python's `random.sample` rejects it too.

See also: [Range Expressions](../lang/05-expressions.md#range-expressions).
