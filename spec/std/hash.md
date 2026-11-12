# Hash

Status: standard library specification draft.

This chapter defines the part of `std.hash` that `lib/std` writes in
ordinary hd over the language tier:

- what a derived `Hash` hashes, through the trait's template;
- how tuples hash, through the trait's tuple template;
- how lists and results hash;
- the bytes each standard implementation writes;
- the standard hasher `DefaultHasher`, and `hash_of`;
- the stable hasher `Fnv1a`.

The language tier keeps `Hash` and `Hasher`, which map keys require
([Hashing](../lang/09-traits.md#hashing)), and the `@derive` checks.

## Derived Hashing

`@derive(Hash)` hashes a value member by member:

```text
@derive(Eq, Hash)
enum Status:
    Ready(code: i32)
    Failed(code: i32, reason: string)

fn lookup_status(items: Map[Status, i32], status: Status) -> i32?:
    items.get(status)
```

1. r[std-hash.derive.hash.template] `std.hash` declares the [template](../lang/14-annotations.md#templates) of `Hash`, which `@derive(Hash)` instantiates.
2. r[std-hash.derive.hash.support] `@derive(Hash)` supports structs and enums.
3. r[std-hash.derive.hash.data] It hashes every declared struct field in declaration order, including embedded fields.
4. r[std-hash.derive.hash.enum] For an enum, it hashes the variant, then shared enum data in declaration order, then that variant's payload fields in declaration order.
5. r[std-hash.derive.hash.fields] Every hashed field must implement `Hash`; no field is implicitly excluded.
6. r[std-hash.derive.hash.cycles] Like derived equality, derived hashing does not detect cycles, so hashing a cyclic graph may exhaust the execution stack.
7. r[std-hash.derive.hash.references] A reference field hashes as the value it refers to, by [Reference Hashing](#reference-hashing).

## Tuple Hashing

```text
fn index(counts: Map[(i32, string), i32]) -> i32:
    counts[(1, "a")]
```

1. r[std-hash.tuple.template] `std.hash` declares a [tuple template](../lang/14-annotations.md#tuple-templates) for `Hash`. A tuple implements `Hash` when every element does, at every size.
2. r[std-hash.tuple.rest-list] A tuple's rest element hashes as its `List[T]`. So a rest tuple implements `Hash` when its fixed elements do and `T < Hash`, and it may be a map key.

## List Hashing

```text
fn count(counts: Map[List[i32], i32], key: List[i32]) -> i32:
    match counts.get(key):
        .Some(n) => n
        .None => 0
```

1. r[std-hash.list.impl] `std.hash` implements `Hash` for `List[T]` when `T < Hash`.
2. r[std-hash.list.not-element] A list whose item type does not implement `Hash`, such as `List[f64]`, does not implement `Hash`. Using it where `Hash` is required is an error. Error: `unsatisfied-trait-bound`.

```text
fn lookup(counts: Map[List[f64], i32]) -> i32:  # error: unsatisfied-trait-bound
    0
```

## Result Hashing

```text
fn failures(counts: Map[Result[i32, string], i32], key: Result[i32, string]) -> i32:
    match counts.get(key):
        .Some(n) => n
        .None => 0
```

1. r[std-hash.result.impl] `std.hash` implements `Hash` for `Result[T, E]` when `T < Hash` and `E < Hash`, so a `Result` of hashable types may be a map key.

## Bytes Written

Each standard `Hash` implementation writes a fixed sequence of bytes to
its `Hasher`:

```text
use std.hash.Fnv1a

fn digest() -> u64:
    let key: (u8, string) = (7, "a")
    let mut hasher = Fnv1a::new()
    key.hash(&mut hasher)
    hasher.finish()  # 12920292002639229810, from the bytes 7, 1, 0, 0, 0, 0, 0, 0, 0, 97
```

| Rule | Value | Bytes written, in order |
| --- | --- | --- |
| r[std-hash.bytes.integer] Integer | an `i8` to `i64`, a `u8` to `u64`, or a `usize` | its two's-complement bytes at its own width, least significant first |
| r[std-hash.bytes.bool] `bool` | `false` or `true` | one byte, `0` or `1` |
| r[std-hash.bytes.char] `char` | a Unicode scalar value | the scalar value, written as a `u32` writes it |
| r[std-hash.bytes.string] `string` | a UTF-8 text | the number of its UTF-8 bytes as a `u64`, then those bytes |
| r[std-hash.bytes.list] `List[T]` | a list | its length as a `u64`, then each item's bytes in order |
| r[std-hash.bytes.tuple] Tuple | a tuple | each element's bytes in order, with no length; a rest element writes as its `List` |
| r[std-hash.bytes.optional] `T?` | `.None` or `.Some(value)` | the tag byte `0` for `.None`; the tag byte `1`, then the bytes of `value`, for `.Some` |
| r[std-hash.bytes.reference] Reference | `&T` or `&mut T` | the bytes of the value it refers to |
| r[std-hash.bytes.result] `Result[T, E]` | `.Ok(value)` or `.Err(error)` | the tag byte `0`, then the bytes of `value`, for `.Ok`; the tag byte `1`, then the bytes of `error`, for `.Err` |

1. r[std-hash.bytes.exact] Each standard implementation of `Hash` writes exactly the bytes of the table above. So a value's `Fnv1a` digest is the same in every implementation.
2. r[std-hash.bytes.typed-methods] A standard `Hash` implementation may write integer bytes or a string's bytes through the corresponding [typed `Hasher` method](../lang/09-traits.md#r-trait.hash.typed-methods), since it writes the same byte sequence as `write`.
3. r[std-hash.bytes.derive-enum] A derived `Hash` on an enum writes the variant's zero-based index as a `u32`, before the members that [`std-hash.derive.hash.enum`](#r-std-hash.derive.hash.enum) lists.
4. r[std-hash.bytes.derive-data] A derived `Hash` on a struct writes no tag, only the bytes of its members in the order of [`std-hash.derive.hash.data`](#r-std-hash.derive.hash.data).

> **Why.** A string writes its length first, as a list does, so one rule
> covers every sequence, and `("ab", "c")` writes other bytes than
> `("a", "bc")`. Rust instead ends a string with the byte `0xff`, which
> UTF-8 never contains. That is shorter, but it is a second scheme. A
> tuple writes no length because its type fixes its arity.

> **Note.** These rules fix the bytes in order, not how an implementation
> splits them into `write` calls. A `DefaultHasher` gives the same result
> for any split, by [`std-hash.default.write`](#r-std-hash.default.write),
> and so does an `Fnv1a`, by [`std-hash.fnv1a.write`](#r-std-hash.fnv1a.write).

## Default Hasher

`DefaultHasher` is the standard `Hasher`. For one hd toolchain, its digest
of the same bytes never changes; its algorithm is not specified:

```text
use std.hash.{DefaultHasher, Hasher, hash_of}

fn same_digest() -> bool:
    let mut hasher = DefaultHasher::new()
    "hd".hash(&mut hasher)
    hasher.finish() == hash_of("hd")  # true, in every run of every program

fn bucket(key: string) -> u64:
    hash_of(key)
```

1. r[std-hash.default.declarations] `std.hash` declares `DefaultHasher` with private fields and the function `hash_of`, imported with `use std.hash.{DefaultHasher, hash_of}`.
2. r[std-hash.default.per-toolchain] For one hd toolchain, `DefaultHasher` gives the same digest of the same bytes in every run, replay, program, and platform.
3. r[std-hash.default.unspecified] The algorithm of `DefaultHasher` is not specified, and it may change between toolchain versions. A program must not persist its digests or compare them with another toolchain's.
4. r[std-hash.default.new-empty] `DefaultHasher::new() -> DefaultHasher` returns a hasher value with no bytes written to it.
5. r[std-hash.default.write] `DefaultHasher` implements `Hasher`. Its `write(bytes)` processes each byte in order, so two writes give the state of one write of both byte lists joined.
6. r[std-hash.default.finish-digest] `finish(self) -> u64` returns the digest of the bytes written so far and does not change the hasher.
7. r[std-hash.default.typed-writes] `DefaultHasher` overrides every [typed `Hasher` method](../lang/09-traits.md#r-trait.hash.typed-methods) to process the specified bytes without building a list.
8. r[std-hash.hash-of-local] `hash_of[T < Hash](value: T) -> u64` hashes through a mutable borrow of a local `DefaultHasher`, then returns its `finish()`.
9. r[std-hash.map.foldhash-fast] `Map` and `UniqMap` bucket keys by the low 32 bits of foldhash 0.2's `fast` hash.
10. r[std-hash.map.seed-word] Each map stores one 64-bit seed.
11. r[std-hash.map.seed-new] A new map takes its seed by mixing the run seed with the next value of a per-run counter.
12. r[std-hash.map.seed-empty] Whenever a nonempty map becomes empty, it takes a fresh seed.
13. r[std-hash.map.seed-copy] A copy-on-write copy of a `Map` preserves its source's seed.
14. r[std-hash.map.seed-replay] Replaying a run's recorded seed and map operations reproduces its map seeds.
15. r[std-hash.map.order-independent] `Map` and `UniqMap` iteration order does not depend on bucket hashes or seeds.

> **Why.** A general hash that must stay the same forever locks the
> language into its algorithm, as `String.hashCode` locked Java. Go, Python
> and Swift instead choose a random seed per process, which breaks replay.
> Like [Rust's `DefaultHasher`](https://doc.rust-lang.org/std/hash/struct.DefaultHasher.html),
> hd fixes the seed but promises nothing across versions. So a digest
> depends on no run state, replay stays deterministic, and a later
> toolchain may adopt a faster algorithm. `DefaultHasher` uses foldhash's
> `fast` algorithm, the one maps use, with one constant seed.

> **Note.** A digest is reproducible within one toolchain, so a test may
> compare two digests. A test that asserts a literal digest, or a program
> that stores one, uses [`Fnv1a`](#fnv-1a-hasher). With a fixed seed, keys
> chosen to collide stay colliding. So `Map` and `UniqMap` do not bucket keys
> by `DefaultHasher`, but give each map its own seed.

> **Why.** A fixed bucket hash lets one chosen collision pattern apply to
> every map. [Foldhash's `fast` algorithm](https://github.com/orlp/foldhash/blob/8f878c636fda9c9e93384824ea45e06d03f009f5/src/fast.rs) hashes integer and string keys
> efficiently, while a separate seed gives each map its own bucket hash.
> Refreshing the seed when a map becomes empty follows
> [Go's map seed rotation](https://github.com/golang/go/issues/25237).
> The journal records the run seed, so rebuilding reproduces map seeds.
> Bucket changes preserve
> [deterministic iteration](../lang/04-type-system.md#r-types.map.order.deterministic).

> **Note.** The [`Hasher` contract](../lang/09-traits.md#r-trait.hash.override-equivalent)
> requires equivalent typed and raw writes. Generic key bytes use fast's
> sponge as repeated single-byte writes. The implementation batches words
> with constant state and one `Hash` call.

See also: [Derived Tuple Implementations](../lang/09-traits.md#derived-tuple-implementations),
[Lookup And Order](../lang/04-type-system.md#lookup-and-order),
[Cmp](cmp.md).

## FNV-1a Hasher

`Fnv1a` is a `Hasher` whose digests never change, for a program that
persists them:

```text
use std.hash.{Fnv1a, Hasher}

fn digest() -> u64:
    let mut hasher = Fnv1a::new()
    hasher.write([97])
    hasher.finish()  # 12638187200555641996
```

| Step | Effect on the 64-bit state |
| --- | --- |
| start | the FNV offset basis, `0xcbf29ce484222325` |
| each byte `b`, in order | XOR `b` into the state, then multiply by the FNV prime `0x100000001b3`, modulo 2^64 |
| `finish` | returns the state |

1. r[std-hash.fnv1a.declarations] `std.hash` declares `Fnv1a` with private fields, imported with `use std.hash.Fnv1a`.
2. r[std-hash.fnv1a.algorithm] `Fnv1a` computes 64-bit FNV-1a over the bytes written to it, by the table above.
3. r[std-hash.fnv1a.new-value] `Fnv1a::new() -> Fnv1a` returns a hasher value whose state is the offset basis.
4. r[std-hash.fnv1a.write] `Fnv1a` implements `Hasher`. Its `write(bytes)` processes each byte in order, so two writes give the state of one write of both byte lists joined.
5. r[std-hash.fnv1a.finish] `finish(self) -> u64` returns the state and does not change it.
6. r[std-hash.fnv1a.typed-writes] `Fnv1a` overrides every [typed `Hasher` method](../lang/09-traits.md#r-trait.hash.typed-methods) to process the specified bytes without building a list.
7. r[std-hash.fnv1a.fixed] The algorithm has no seed and no key. So the same bytes give the same digest in every run, program, implementation, and toolchain version, and a program may persist it.
8. r[std-hash.fnv1a.vectors] `Fnv1a` gives these digests:

| Bytes written | Digest |
| --- | --- |
| none | `14695981039346656037` (`0xcbf29ce484222325`) |
| `97` | `12638187200555641996` (`0xaf63dc4c8601ec8c`) |
| `97`, `98` | `620445648566982762` (`0x089c4407b545986a`) |
| the UTF-8 bytes of `foobar` | `9625390261332436968` (`0x85944171f73967e8`) |
| `7`, `1`, seven `0`s, `97`: the `Hash` bytes of `(7, "a")` as `(u8, string)` | `12920292002639229810` |

> **Why.** FNV-1a is a few lines of hd that need only XOR and one
> wrapping multiplication. Its published digests let any program check
> its own. SipHash-1-3 mixes better, but its strength is a secret
> key. With a fixed key it gives no defense against chosen keys, so its
> larger code buys little here.

## Reference Hashing

`std.hash` implements `Hash` for references by the values they refer to:

```text
use std.hash.hash_of

fn same_hash(a: &string, b: &string) -> bool:
    hash_of(a) == hash_of(b)  # true when *a == *b
```

1. r[std-hash.reference.noncopy] `std.hash` implements `Hash` for each reference type, `*T`, `*mut T`, `&T`, and `&mut T`, when `T < ?Copy & Hash`. A reference writes the bytes of the value it refers to, so a reference and its value hash alike.
2. r[std-hash.reference.derived] A derived `Hash` reaches this implementation for a reference field, as [`std-hash.derive.hash.references`](#r-std-hash.derive.hash.references) states.
