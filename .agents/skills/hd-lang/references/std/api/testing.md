# `std.testing`

Generated from `hd doc std.testing`. Run `hd doc std.testing.NAME` in a package for one item's full text.

## `TestRunner`

```text
pub trait TestRunner
```

The host capability of the test runner: it reports the run and answers the checks of a test case.

## `PropertyCase`

```text
pub struct PropertyCase
```

What a property runner's `start` returns for one case.

- `pub example: usize?`: The index of the example that the case runs, or `.None` for a generated case.
- `pub seed: i64`: The seed of the case's fresh draws.
- `pub size: i32`: How far the case's fresh draws reach, which grows from case to case.
- `pub replay: List[i64]`: The recorded draws that the case replays or shrinks, in order; empty for a fresh case.

## `PropertyRunner`

```text
pub trait PropertyRunner
```

The host capability of a property runner: it records and shrinks each case's draws.

## `Choices`

```text
pub struct Choices
```

The source of one case's drawn values, which the runner records so it can replay and shrink the case.

- `pub fn int[N < Integer](&mut self, range: Range[N]) -> N`: Draws an integer of `range`.
- `pub fn float[F < Float](&mut self, range: Range[F]) -> F`: Draws a finite float of `range`.
- `pub fn bool(&mut self) -> bool`: Draws a `bool`.
- `pub fn choose[T](&mut self, items: List[T]) -> T`: Draws one of `items`; earlier items shrink first.
- `pub fn list[T](&mut self, max: usize, item: fn(&mut Choices) -> T) -> List[T]`: Draws at most `max` items, each drawn by `item`.
- `pub fn map[K < Eq & Hash, V]( &mut self, max: usize, key: fn(&mut Choices) -> K, value: fn(&mut Choices) -> V, ) -> Map[K, V]`: Draws at most `max` entries, each key drawn by `key` and its value by `value`.
- `pub fn string(&mut self, max_chars: usize) -> string`: Draws a string of at most `max_chars` chars.
- `pub fn assume(&mut self, ok: bool) -> void`: Discards the case when `ok` is false.
- `pub fn draw[T < Arbitrary](&mut self) -> T`: Draws `T::arbitrary(self)`, the type's default generator.

## `Arbitrary`

```text
pub trait Arbitrary
```

A type's default generator, which `it_prop` and `Choices.draw` use.

## `it_each`

```text
pub fn it_each[A, T < Termination, $ R](
```

Registers one test case for each element of `rows`, named `name[i]`, which runs `body` with that element.

## `it_prop`

```text
pub fn it_prop[T < Arbitrary & Debug, R < Termination, $ Q](
```

Registers one property test case, whose inputs `T`'s `Arbitrary` draws.

## `it_prop_with`

```text
pub fn it_prop_with[T < Debug, R < Termination, $ Q](
```

Registers one property test case, whose inputs `gen` draws.

## `assert`

```text
pub fn assert(condition: bool, reason: string = "") -> void
```

A false `condition` is a failed assertion: a checked runtime panic that shows a nonempty `reason`.

## `assert_equal`

```text
pub fn assert_equal[T < Eq & Debug](actual: T, expected: T, reason: string = "") -> void
```

Compares two values and fails the assertion when they are unequal.

## `snapshot`

```text
pub fn snapshot(text: string, expect: string = "") -> void
```

Compares `text` with `expect`, the expected text written in the source; a difference is a failed assertion.

## `snapshot_file`

```text
pub fn snapshot_file(text: string) -> void $ TestRunner
```

Compares `text` with the snapshot file the runner names from the running test case; a mismatch or a missing file is a failed assertion.

## `hd_run`

```text
pub fn hd_run!(command: Command) -> ProcessOutput $ Process
```

Runs `command` and returns the program's output and exit status.

## `temp_dir`

```text
pub fn temp_dir() -> Path $ TestRunner
```

Returns the absolute path of a fresh, empty directory that belongs to the running test case alone.

## `commands`

```text
pub fn commands[M, C](
```

Draws a bounded command history while advancing a pure model.
