# `std.random`

Generated from `hd doc std.random`. Run `hd doc std.random.NAME` in a package for one item's full text.

## `Random`

```text
pub trait Random
```

The host capability to fill bytes from the system random source.

## `fill`

```text
pub fn fill(count: usize) -> List[u8] $ Random
```

`count` random bytes of the covering `Random` provider.

## `SeededRandom`

```text
pub struct SeededRandom
```

The deterministic `Random` provider: a seeded generator for tests, with no host access.

- `pub fn new(seed: u64) -> *mut SeededRandom`: Returns the provider of `seed`; two providers of one seed answer the same.

## `Rng`

```text
pub struct Rng
```

A random generator: seeded, so its draws repeat for one seed.

- `pub fn new(seed: u64) -> *mut Rng`: Returns the generator of `seed`.
- `pub fn next_u64(&mut self) -> u64`: The next random `u64`.
- `pub fn int[N < Integer](&mut self, range: Range[N]) -> N`: A random integer of `range`, each with the same chance.
- `pub fn float(&mut self) -> f64`: A random float from 0 up to, not including, 1.
- `pub fn bool(&mut self) -> bool`: A random `bool`, each value with the same chance.
- `pub fn choose[T](&mut self, items: List[T]) -> T?`: One of `items`, each with the same chance, or `.None` for an empty list.
- `pub fn shuffle[T](&mut self, items: &mut List[T]) -> void`: Reorders `items` at random, each permutation with the same chance.
- `pub fn sample[T](&mut self, items: List[T], count: usize) -> List[T]`: `count` of `items`, drawn without replacement in random order.

## `rng`

```text
pub fn rng() -> *mut Rng $ Random
```

Returns a generator seeded from the `Random` provider that covers the call.
