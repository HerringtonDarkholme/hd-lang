# Serde

Status: standard library specification draft.

This chapter defines the part of `std.serde` that `lib/std` writes in
ordinary hd over the language tier:

- the data model: what each `Serializer` call writes, and what each
  `Deserializer` call reads;
- what a derived `Serialize` or `Deserialize` calls, through the traits'
  templates;
- the standard implementations of both traits.

The language tier keeps the module's declarations and what opting in means
([Serialization](../lang/14-annotations.md#serialization)), since the host
boundary reads the implementation's output, not the field tree
([`module.boundary.out`](../lang/10-modules.md#r-module.boundary.out)).

## Data Model

A format sees one value as a tree of calls on its writer:

```text
use std.serde.{Serialize, Serializer}

fn write_pair[W < Serializer](out: mut W, name: string, count: i64) -> Result[void, W::Error]:
    out.begin_list(2)?
    out.text(name)?
    out.int(count)?
    out.end_list()
```

| Rule | Value | `Serializer` calls |
| --- | --- | --- |
| r[std-serde.model.null] Null | an absent value | `null()` |
| r[std-serde.model.bool] Boolean | `false` or `true` | `bool(value)` |
| r[std-serde.model.int] Signed | an integer that fits `i64` | `int(value)` |
| r[std-serde.model.uint] Unsigned | an integer of an unsigned type | `uint(value)` |
| r[std-serde.model.float] Float | any `f64`, NaN and infinities included | `float(value)` |
| r[std-serde.model.text] Text | a `string` | `text(value)` |
| r[std-serde.model.list] List | items in order | `begin_list(len)`, one value per item, `end_list()` |
| r[std-serde.model.map] Map | entries in order, each with a `string` key | `begin_map(len)`, then per entry `key(k)` and one value, then `end_map()` |
| r[std-serde.model.variant] Variant | a data value, or an enum value | `begin_variant(facts, v)`, then per member `member(m)` and one value, then `end_variant()` |

1. r[std-serde.model.one-value] A `serialize` call writes exactly one value of the table: one scalar call, or one begin call, its contents, and the matching end call.
2. r[std-serde.model.len] The `len` of `begin_list` or `begin_map` is the number of items or entries that follow.
3. r[std-serde.model.variant-args] `facts` is the type-level facts of the value's type, and `v` the `VariantInfo` of its variant. A data value is its type's one variant, with `v.of_data` true.
4. r[std-serde.model.member-args] `m` is the `Member` of the member whose value follows. Its facts are the ones its derivation sees, as [`annot.structure.per-derivation`](../lang/14-annotations.md#r-annot.structure.per-derivation) states.
5. r[std-serde.model.errors] A writer's first `.Err` ends the write. Every standard implementation returns it unchanged, and calls nothing after it.

> **Why.** Fourteen calls cover JSON's kinds, and the variant calls keep
> names and facts, which a format needs to choose a key or a tag. Fewer
> calls than serde's 29 keep a format small, and a binary format can
> still tell `int` from `uint`.

### Reading

A `Deserializer` reads the same tree, one call per value:

| Rule | Call | Returns |
| --- | --- | --- |
| r[std-serde.read.peek] `peek` | `peek()` | the `ValueKind` of the next value, without reading it; an absent value is `Null` |
| r[std-serde.read.is-null] `is_null` | `is_null()` | `true` when the next value is null or absent, which reads it; `false` otherwise, which leaves it |
| r[std-serde.read.scalar] Scalars | `bool`, `int`, `uint`, `float`, `text` | the next value, when it is of that kind and fits the result type |
| r[std-serde.read.list] List | `begin_list()`, then `next_item()` before each item | `next_item` is `true` when an item follows, and `false` after the last |
| r[std-serde.read.map] Map | `begin_map()`, then `next_key()` before each entry | `next_key` is the entry's key, or `.None` after the last |
| r[std-serde.read.variant] Variant | `begin_variant(facts, choices)`, then per member `member(m)` and one value, then `end_variant()` | `begin_variant` is the index in `choices` of the variant read |
| r[std-serde.read.invalid] `invalid` | `invalid(expected)` | the format's error for the value just read, which does not fit `expected` |

1. r[std-serde.read.one-value] A `deserialize` call reads exactly one value.
2. r[std-serde.read.expected] Each scalar call's `expected` names the type being read, as `"u8"`. A format's error for a value of another kind may report it.
3. r[std-serde.read.member] `member(m)` makes the value of member `m` the next value. The format decides which value that is, and it may be absent.
4. r[std-serde.read.choices] `choices` holds the `VariantInfo` of every variant of the type, in declaration order. A data type offers its one variant.

```text
use std.serde.Deserializer

fn read_text_or_empty[R < Deserializer](input: mut R) -> Result[string, R::Error]:
    match input.peek()?:
        .Text => input.text("string")
        _ => .Ok("")
```

> **Note.** `peek` lets a value that holds any kind, such as a `Json`,
> read itself. A format that is not self-describing may return an error
> from `peek`.

## Derived Implementations

The [templates](../lang/14-annotations.md#templates) drive a format's
writer or reader with a value's members. For the `Session` of
[Serialization](../lang/14-annotations.md#serialization):

| Value | `Serializer` calls of derived `serialize` |
| --- | --- |
| `Session { user: "ada", token: "t", expires_at: 9 }` | `begin_variant`, `member` for `user`, `text("ada")`, `member` for `token`, `text("t")`, `member` for `expires_at`, `int(9)`, `end_variant()` |

1. r[std-serde.derive.serialize.template] `std.serde` declares the template of `Serialize`, which `@derive(Serialize)` instantiates.
2. r[std-serde.derive.serialize.order] Derived `serialize` calls `begin_variant` for the value's variant first. Then, for each walked member in declaration order, it calls `member(m)` and the member's own `serialize`. Last, it calls `end_variant()`.
3. r[std-serde.derive.deserialize.template] `std.serde` declares the template of `Deserialize`, which `@derive(Deserialize)` instantiates.
4. r[std-serde.derive.deserialize.order] Derived `deserialize` calls `begin_variant` with every variant first. Then, for each built member in declaration order, it calls `member(m)` and the member type's `deserialize`. Last, it calls `end_variant()`.
5. r[std-serde.derive.omitted] A member omitted with `= pass` is not written. Reading fills it with its declared default, as [`annot.omit.default`](../lang/14-annotations.md#r-annot.omit.default) states.
6. r[std-serde.derive.shared] Shared constructor data is not a member, so it is neither written nor read.

```text
use std.serde.{Serialize, Deserialize}
use std.structure.Structure

data Cache:
    hits: i64

data Session:
    pub user: string
    token: string
    cache: Cache = Cache { hits: 0 }

impl Serialize for Session by Structure:
    cache = pass

impl Deserialize for Session by Structure:
    cache = pass
```

> **Note.** The two blocks above leave `cache` out of every format. Their
> member lines agree, so they get no
> [`derivation-line-drift`](../lang/14-annotations.md#r-annot.line.drift)
> warning.

## Standard Implementations

`std.serde` implements both traits for these types:

| Rule | Type | `serialize` writes | `deserialize` reads, with `expected` |
| --- | --- | --- | --- |
| r[std-serde.std.bool] Boolean | `bool` | `bool` | `bool`, `"bool"` |
| r[std-serde.std.text] String | `string` | `text` | `text`, `"string"` |
| r[std-serde.std.char] Character | `char` | `text` of that one character | `text` of exactly one character, `"char"` |
| r[std-serde.std.signed] Signed integers | `i8`, `i16`, `i32`, and `i64` | `int` | `int` within the type's range, the type's name |
| r[std-serde.std.unsigned] Unsigned integers | `u8`, `u16`, `u32`, `u64`, and `usize` | `uint` | `uint` within the type's range, the type's name |
| r[std-serde.std.float] Floats | `f32` and `f64` | `float` of the value as an `f64` | `float`, the type's name |
| r[std-serde.std.optional] Optional | `T?`, where `T` implements the trait | `null` for `.None`, and the payload for `.Some` | `.None` when `is_null` is `true`, and `.Some` of what `T` reads otherwise |
| r[std-serde.std.list] List | `List[T]`, where `T` implements the trait | a list of the items, in order | a list, item by item |
| r[std-serde.std.map-entries] Map | `Map[string, V]`, where `V` implements the trait | a map of the entries, in the map's [iteration order](../lang/04-type-system.md#r-types.map.order.deterministic) | a map, entry by entry, in its order |

1. r[std-serde.std.range] A value that its scalar call reads but the type cannot hold is the reader's `invalid(expected)` error. That is an integer outside the type's range, a `char` text of another length, or an `f32` past its finite range. The range of `usize` is the target's, by [`types.usize.width`](../lang/04-type-system.md#r-types.usize.width), so on the Wasm32 target it is that of `u32`.
2. r[std-serde.std.float-read] An `f32` reads the nearest `f32` to the `f64` it reads.
3. r[std-serde.std.map-keys] Only a `Map` whose key type is `string` implements the traits. Using a map with any other key type where they are required is an error. Error: `unsatisfied-trait-bound`.

```text
use std.serde.Serialize

fn send[T < Serialize](value: T) -> void:
    pass

fn send_counts(counts: Map[i32, i32]) -> void:
    send(counts)  # error: unsatisfied-trait-bound
```

> **Note.** `T??` loses a layer: `.Some(.None)` writes `null`, which reads
> back as `.None`.

> **Why.** A map key is text in JSON, YAML, and TOML. A map with other
> keys converts them first, so the program chooses their text.

See also: [Serialization](../lang/14-annotations.md#serialization), [Typed JSON](json.md#typed-json), [Time](time.md#serialization), [Boundary Encoding](../lang/10-modules.md#boundary-encoding).
