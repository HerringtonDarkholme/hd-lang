# Standard Library Specification

This directory holds the stdlib tier of the specification. The numbered
chapters in [`lang/`](../README.md#contents) are the language tier.
Both tiers are one specification, written to one
[Specification Style](../STYLE.md), with one conformance suite.

## Tiers

| Tier | Holds | Where |
| --- | --- | --- |
| language | syntax, static and dynamic semantics, intrinsics, and every std item the compiler must know by name | the [numbered chapters](../README.md#contents) |
| stdlib | std APIs that `lib/std` writes in ordinary hd over the language tier | this directory |
| CLI | how the `hd` command finds and runs a package or a single file | [Command Line](../cli/command-line.md) |

The language tier names a std item only when the compiler must know it:

- a lang item, such as `Iterable` for `for`, `std.process.Termination`, or
  a `std.ops` operator trait;
- an intrinsic, such as `List.append`, `string.len`, or `facts_of(f)`;
- a prelude name and its signature;
- the test registration functions `it`, `it_each`, `it_prop`, and
  `it_prop_with`, with their position rules and diagnostics;
- the conformance harness: `it`, `assert`, `assert_equal`, and `println`.

This tier keeps what those registration functions do when a test runs:
table rows, property generation and shrinking, the runner capabilities,
reporting, and snapshots ([Testing](testing.md)).

Every other decided std API belongs in this directory.

### The Tier Test

> Could `lib/std` implement the item in ordinary hd, over language-tier
> items only, with no compiler knowledge of its name, and keep the same
> observable behavior?

Yes puts the item in the stdlib tier. A diagnostic code about the item, a
language rule that names it, or a position rule that lists it each mean
no.

Only `std` may add inherent methods to built-in types
([`trait.own.inherent.std`](../lang/09-traits.md#r-trait.own.inherent.std)).
That alone does not make a method language-tier.

## Standard Library Primitives

`lib/std` is ordinary hd over the language tier, plus the primitives
below. A primitive is a function or a method whose body the
implementation supplies. A primitive function is private to the std
module that declares it, so no program calls it. The specification has no
syntax for declaring one; the reference implementation writes
`@intrinsic("name")` before it. A primitive operation is an
[intrinsic method](../lang/09-traits.md#intrinsic-methods), which any
program may call.

Any primitive not in these tables needs the owner's approval.

**Representation.** These read or build the run-time layout of a string,
a `char`, or a list.

| Primitive | Signature | Why it is a primitive |
| --- | --- | --- |
| `bytes_len` | `(s: string) -> i32` | reads the byte length of a string's representation |
| `bytes_at` | `(s: string, i: i32) -> u8` | reads one byte of a string's representation |
| `bytes_slice` | `(s: string, start: i32, end: i32) -> string` | shares the bytes in constant time, as [`module.string.slice.shared`](../lang/10-modules.md#r-module.string.slice.shared) requires |
| `bytes_concat` | `(a: string, b: string) -> string` | allocates a string's representation; hd has no byte buffer |
| `string_from_bytes` | `(bytes: List[u8]) -> string` | builds a string from bytes that the caller has checked are UTF-8 |
| `char_scalar`, `char_from_scalar` | `(c: char) -> i32`, `(point: i32) -> char` | a `char` is its scalar value at run time, and hd has no unchecked conversion |
| `list_version` | `[T](items: List[T]) -> i32` | reads the structural version that [`flow.for.version`](../lang/06-control-flow.md#r-flow.for.version) counts, which `ListView` checks |

**Panic.** One primitive raises every checked runtime panic that `lib/std`
raises with a category other than `explicit-panic`.

| Primitive | Signature | Why it is a primitive |
| --- | --- | --- |
| `panic` | `(category: string, message: string) -> void` | the prelude `panic` always has the category `explicit-panic`; this one names a [stable category](../lang/06-control-flow.md#r-flow.panic.stable-categories), and an empty message shows none |

**Compiler-level.** The compiler lowers these at each use.

| Primitive | Why it is a primitive |
| --- | --- |
| `facts_of` | the compiler builds each call's facts value ([Function Facts](../lang/14-annotations.md#function-facts)) |
| `task_race_frame`, `task_all_frame` | the polling frames of `race!` and `all!`, which are compiler intrinsics ([`req.combinator.intrinsic`](../lang/11-requirements-and-suspension.md#r-req.combinator.intrinsic)) |

**Host.** The host supplies these, for now.

| Primitive | Signature | Why it is a primitive |
| --- | --- | --- |
| `format_f64`, `format_f32` | `(value: f64) -> string`, `(value: f32) -> string` | the shortest round-trip decimal text of a float ([Numeric Display](../lang/04-type-system.md#numeric-display)) |
| `string_lower`, `string_upper` | `(text: string) -> string` | Unicode case mapping, which needs the Unicode tables |

**Test-runner hooks.** `std.testing` reaches the test runner through the
host capabilities `TestRunner` and `PropertyRunner`
([Runner Capabilities](testing.md#runner-capabilities)), so table rows,
timeouts, snapshot files, and property draws need no primitive.

**Operations.** One intrinsic method per primitive operation. `lib/std`
writes each once, mostly in a numeric-family implementation such as
`impl[N < Num] Add for N`
([`trait.target.numeric-family`](../lang/09-traits.md#r-trait.target.numeric-family)),
and the compiler specializes it for each type.

| Intrinsic method | Implemented for | Why it is a primitive |
| --- | --- | --- |
| `Add.add` | every number type | machine addition, with the checked integer overflow panic |
| `Sub.sub` | every number type | machine subtraction, with the checked integer overflow panic |
| `Mul.mul` | every number type | machine multiplication, with the checked integer overflow panic |
| `Div.div` | every number type | truncating integer division with its panics, or IEEE 754 division |
| `Rem.rem` | every number type | the truncated remainder, with its panic, or the floating remainder |
| `Neg.neg` | the signed integer and floating-point types | machine negation; negating the minimum integer panics |
| `BitAnd.bit_and` | every integer type | one machine instruction on the bits |
| `BitOr.bit_or` | every integer type | one machine instruction on the bits |
| `BitXor.bit_xor` | every integer type | one machine instruction on the bits |
| `Not.not` | every integer type | one machine instruction on the bits |
| `Shl.shl` | every integer type, with a `u32` count | a fixed-width shift, with the shift count panic |
| `Shr.shr` | every integer type, with a `u32` count | a fixed-width shift, arithmetic for a signed value, with the shift count panic |
| `Eq.eq` | every number type, `char`, and `bool` | compares the machine values; floats follow IEEE 754 |
| `PartialOrd.partial_cmp` | every number type and `char` | compares the machine values; a NaN operand is unordered |
| `Ord.cmp` | every integer type and `char` | compares the machine values |

`string`'s `Add`, `Eq`, `PartialOrd`, and `Ord` are not intrinsic: `lib/std`
writes them in hd.

> **Why.** Each primitive is a promise that every implementation must
> keep, and code that the compiler supplies cannot be read or tested as
> hd. A short explicit list keeps the rest of `lib/std` portable.

## Chapters

Each chapter is one unnumbered file named after its std module. The
Spec Tiers migration plan
added the files below, and it is complete. A new decided std module gets a
file of its own.

| File | Module | Rule ID prefix | Scope |
| --- | --- | --- | --- |
| [`testing.md`](testing.md) | `std.testing` | `std-testing` | what `it_each`, `it_prop`, and `it_prop_with` cases do when they run, property tests, the draw budget, derived `Arbitrary`, table-test rows, snapshot files, the `timeout` option, `hd_run!`, the runner capabilities |
| [`iter.md`](iter.md) | `std.iter` | `std-iter` | iterator adapters, collect targets, `FromIterator` and its impls, `map` on a list or an optional |
| [`text.md`](text.md) | `std.text` | `std-text` | string methods above the intrinsics, including `lines` and `repeat`; the `r` prefix |
| [`format.md`](format.md) | `std.format` | `std-format` | the text `debug` returns, `Debug` builders and layout, derived builder calls |
| [`time.md`](time.md) | `std.time` | `std-time` | `Duration` and its suffixes; the host trait `Clock`, `Timestamp`, `Instant`, `now`, and `sleep!`; the provider `ManualClock` |
| [`task.md`](task.md) | `std.task` | `std-task` | the `retry!` combinator; `Backoff` and `retry_with!` |
| [`ops.md`](ops.md) | `std.ops` | `std-ops` | the `Default` trait and its standard implementations |
| [`cmp.md`](cmp.md) | `std.cmp` | `std-cmp` | what derived `Eq`, `PartialOrd`, and `Ord` compare; tuple comparison; `clamp` |
| [`hash.md`](hash.md) | `std.hash` | `std-hash` | what derived `Hash` hashes; tuple hashing; `DefaultHasher` and `hash_of` |
| [`collections.md`](collections.md) | `std.collections` | `std-collections` | the `List` methods `view` and `chunks`, the `ListView` type, and the `Map` methods `contains_key`, `keys`, and `values` |
| [`console.md`](console.md) | `std.console` | `std-console` | `eprintln`, the host trait `ConsoleInput`, and `read_line!` |
| [`host.md`](host.md) | `std.host` | `std-host` | the host traits `Args` and `Env`, the helpers `args` and `env`, and the providers `MapArgs` and `MapEnv` |
| [`fs.md`](fs.md) | `std.fs` | `std-fs` | the host traits `FsRead` and `FsWrite`, `FsError`, `Entry`, the helpers `read_text!` and `write_text!`, and the provider `MemoryFs` |
| [`path.md`](path.md) | `std.path` | `std-path` | the `Path` newtype |
| [`random.md`](random.md) | `std.random` | `std-random` | the host trait `Random` and the provider `SeededRandom` |
| [`option.md`](option.md) | `std.option` | `std-option` | the methods `and_then`, `unwrap_or`, `ok_or`, `is_some`, `is_none`, and `expect` of `T?` |
| [`result.md`](result.md) | `std.result` | `std-result` | the methods `and_then`, `map_err`, `unwrap_or`, `map`, `ok`, `err`, `is_ok`, `is_err`, and `expect` of `Result[T, E]` |
| [`num.md`](num.md) | `std.num` | `std-num` | checked, wrapping, and saturating integer methods, `abs_diff`, and the bit counts on every integer type; `is_nan` and `is_finite`; integer parsing with `ParseNumberError` |

## Glossary

This glossary lists the terms of the stdlib chapters. Each entry links to
the rule, or the section, that defines the term. The language terms are in
the [language glossary](../README.md#glossary).

| Term | Definition |
| --- | --- |
| **collect target** | The collection that `collect` builds, named by the expected type. See [Collect Targets](iter.md#collect-targets). |
| **Debug builders** | The `DebugWriter` methods that describe a value as a struct, tuple, list, or map. See [Debug Builders](format.md#debug-builders). |
| **draw budget** | The per-case limit on draws from `Choices`; once it is spent, every draw returns its simplest value. See [`std-testing.budget`](testing.md#r-std-testing.budget). |
| **iterator adapters** | Methods of the prelude `Iterator[T]` that wrap an iterator in a new one, or drain it. See [Iterator Adapters](iter.md#iterator-adapters). |
| **property test** | A test case whose body runs on inputs drawn from a `Choices` source. See [Property Tests](testing.md#property-tests). |

## Rule IDs

1. A rule ID here starts with its file's prefix: `std-` and the module
   name.
2. A rule that moves here from a numbered chapter is retired there and
   keeps its tail under the new prefix. `module.testing.prop.debug`
   becomes `std-testing.prop.debug`.
3. The retired ID is deleted, as [Stability](../STYLE.md#stability)
   requires.

## Links Between Tiers

- A stdlib chapter may cite any language rule.
- A language chapter links here only from a Note or a See also, never from
  a numbered rule. No language rule depends on stdlib behavior.
- A moved heading is deleted, and every link to it is fixed when it moves.

## Conformance And Diagnostics

- A case's tier is the tier of its `specification` column in
  [`cases.tsv`](../conformance/cases.tsv). A path under `std/` is stdlib.
- Every diagnostic code stays in the [Diagnostics](../README.md#diagnostics)
  table, and every panic category stays in
  [Control Flow](../lang/06-control-flow.md). Each is a compiler check.
