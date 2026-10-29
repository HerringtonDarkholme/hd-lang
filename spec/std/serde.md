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

fn write_pair[W < Serializer](out: &mut W, name: string, count: i64) -> Result[void, W::Error]:
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
| r[std-serde.model.map-scalar-keys] Map | entries in order, each with a `MapKeyValue` key | `begin_map(len)`, then per entry `map_key(k)` and one value, then `end_map()` |
| r[std-serde.model.data-record] Data record | a struct value | `begin_data(facts, v)`, then per member `member(m)` and one value, then `end_data()` |
| r[std-serde.model.enum-record] Enum record | a unit, named, or mixed enum variant | `begin_variant(facts, v)`, then per member `member(m)` and one value, then `end_variant()` |
| r[std-serde.model.required-newtype-variant] Newtype variant | an enum variant with one direct payload | `begin_newtype_variant(facts, v)`, then the payload value, then `end_variant()`; the hook is required, so each format must support the shape or return its own error |
| r[std-serde.model.required-tuple-variant] Tuple variant | an enum variant with several direct payloads | `begin_tuple_variant(facts, v, len)`, then the payload values in order, then `end_variant()`; the hook is required, so each format must support the shape or return its own error |

1. r[std-serde.model.one-value] A `serialize` call writes exactly one value of the table: one scalar call, or one begin call, its contents, and the matching end call.
2. r[std-serde.model.container-len] The `len` of `begin_list`, `begin_map`, or `begin_tuple_variant` is the number of items, entries, or direct payload values that follow.
3. r[std-serde.model.variant-args.local-ref] `begin_variant(facts: Facts, v: &VariantInfo)` receives the type-level facts and a local reference to the variant's metadata.
4. r[std-serde.model.data-args.local-ref] `begin_data(facts: Facts, v: &VariantInfo)` receives the type-level facts and a local reference to the struct variant's metadata. Its default calls `begin_variant(facts, v)`, and `end_data`'s default calls `end_variant`.
5. r[std-serde.model.data-variant] A struct value has one variant, whose `of_data` is true.
6. r[std-serde.model.member-args.local-ref] `member(m: &Member)` receives a local reference to the metadata of the member whose value follows.
7. r[std-serde.model.member-facts] The member's facts are those visible to its derivation, as [`annot.structure.per-derivation`](../lang/14-annotations.md#r-annot.structure.per-derivation) specifies.
8. r[std-serde.model.errors] A writer's first `.Err` ends the write. Every standard implementation returns it unchanged, and calls nothing after it.
9. r[std-serde.model.ref-receivers] Every method of `Serializer` and `Deserializer` takes `&mut self`, and `serialize` and `deserialize` take the writer or reader as `&mut W` or `&mut R`. So a format's writer or reader is a struct that changes in place, and its caller reads the struct's fields after the call.

> **Why.** Sixteen calls cover JSON's kinds, and the variant calls keep
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
| r[std-serde.read.map-scalar-keys] Map | `begin_map()`, then `next_map_key()` before each entry | `next_map_key` is the entry's `MapKeyValue`, or `.None` after the last |
| r[std-serde.read.data-record] Data record | `begin_data(facts, choices)`, then the calls for its members, then `end_data()` | `begin_data` is the index of the one struct variant; its default calls `begin_variant`, and `end_data`'s default calls `end_variant` |
| r[std-serde.read.enum-record] Enum record | `begin_variant(facts, choices)`, then the calls for its payload, then `end_variant()` | `begin_variant` is the index in `choices` of the variant read |
| r[std-serde.read.required-newtype-variant] Newtype variant | `begin_variant(facts, choices)`, `begin_newtype_variant()`, then the direct payload value, then `end_variant()` | `begin_variant` selects the variant; `begin_newtype_variant` is required, so each format must position the reader at the direct payload or return its own error |
| r[std-serde.read.tuple-variant] Tuple variant | `begin_variant(facts, choices)`, `begin_tuple_variant(len)`, then `next_item()` before each direct payload value, then `end_variant()` | `begin_variant` selects the variant; `begin_tuple_variant` positions a payload of exactly `len` items |
| r[std-serde.read.invalid] `invalid` | `invalid(expected)` | the format's error for the value just read, which does not fit `expected` |
| r[std-serde.read.absent] `is_absent` | `is_absent()` | `true` when the member that `member(m)` just positioned has no value at all, which reads nothing; the trait's default body gives `false` |

1. r[std-serde.read.one-value] A `deserialize` call reads exactly one value.
2. r[std-serde.read.expected] Each scalar call's `expected` names the type being read, as `"u8"`. A format's error for a value of another kind may report it.
3. r[std-serde.read.member.local-ref] A `member(m: &Member)` call positions the next value of that member, which the format may report as absent.
4. r[std-serde.read.choice-metadata] `choices` holds the `VariantInfo` of every variant offered to `begin_variant`, in declaration order. `begin_data` receives a list containing the struct's one variant.
5. r[std-serde.read.tuple-variant-default] `Deserializer`'s default `begin_tuple_variant` rejects the value through `invalid("tuple variant")`, so a format must opt in to tuple variants.

```text
use std.serde.Deserializer

fn read_text_or_empty[R < Deserializer](input: &mut R) -> Result[string, R::Error]:
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
| `Session { user: "ada", token: "t", expires_at: 9 }` | `begin_data`, `member` for `user`, `text("ada")`, `member` for `token`, `text("t")`, `member` for `expires_at`, `int(9)`, `end_data()` |

1. r[std-serde.derive.serialize.template] `std.serde` declares the template of `Serialize`, which `@derive(Serialize)` instantiates.
2. r[std-serde.derive.serialize.data-and-enum-forms] Derived `serialize` writes every included walked member in declaration order. A struct uses `begin_data`, `member(m)` before each value, and `end_data`. A unit, named, or mixed positional-and-named enum variant uses `begin_variant`, `member(m)` before each value, and `end_variant`. A pure positional variant uses this derivation's included positional-member count. Zero uses the unit record form, one uses `begin_newtype_variant`, and more uses `begin_tuple_variant` with that count. Every enum form ends with `end_variant`.
3. r[std-serde.derive.deserialize.template] `std.serde` declares the template of `Deserialize`, which `@derive(Deserialize)` instantiates.
4. r[std-serde.derive.deserialize.data-and-enum-forms] Derived `deserialize` offers a struct's one variant to `begin_data`, reads every unskipped built member in declaration order through `member(m)`, and calls `end_data`. For an enum it offers every variant to `begin_variant`, selects the same included-member form as serialization, reads every unskipped built member in declaration order, and calls `end_variant`. Enum record forms call `member(m)` before each value, a newtype form reads its direct value, and a tuple form calls `next_item()` before each direct value.
5. r[std-serde.derive.deserialize.unskipped-default] After `member(m)`, derived deserialization asks `is_absent()` for an unskipped defaulted member, using its declared default instead of `deserialize` when absent.
6. r[std-serde.derive.omitted] A member omitted with `= pass` is not written. Reading fills it with its declared default, as [`annot.omit.default`](../lang/14-annotations.md#r-annot.omit.default) states.
7. r[std-serde.derive.shared] Shared constructor data is not a member, so it is neither written nor read.

```text
use std.serde.{Serialize, Deserialize}
use std.structure.Structure

struct Cache:
    hits: i64

struct Session:
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
> [`derivation-line-drift`](../lang/14-annotations.md#r-annot.line.drift.lint)
> warning.

### Member Names

A member's **wire name** is its name in a document format.
For example, `@rename("total")` on `total_cents: i64` uses the key `total`.

1. r[std-serde.rename.fact] `std.serde` exports the field fact `Rename` with the public field `name: string`.
2. r[std-serde.rename.constructor] `rename(name: string) -> Rename` constructs a fact with that name.
3. r[std-serde.member-name.precedence] A member's wire name uses its `Rename.name` first, its container's `RenameAll` style second, and its source name otherwise.
4. r[std-serde.rename.metadata] `Member.name` retains the source member name, and document formats read `Rename` from `Member.facts`.

5. r[std-serde.member-name.helper.local-ref] `member_name(member: &Member, container: Facts) -> string` returns that member's wire name.
6. r[std-serde.member-name.formats] Standard serializers and deserializers use `member_name` for document or capture-group names.
7. r[std-serde.rename-all.styles] `RenameCase` is an `Eq` and `Debug` enum with the eight style variants in the table.
8. r[std-serde.rename-all.fact] `RenameAll` is a data-type and enum fact with the public field `style: RenameCase`.
9. r[std-serde.rename-all.constructor] `rename_all(style: RenameCase) -> RenameAll` constructs the fact.
10. r[std-serde.rename-all.conversion] Style conversion follows the table, applying ASCII case changes to a member's written name.
11. r[std-serde.rename-all.container] A standard format's container facts are the current record's or enum's per-derivation facts supplied to `begin_variant`.
12. r[std-serde.rename-all.map-key-values] A `RenameAll` style does not change a map key's scalar value or text.

| `RenameCase` variant | Style | Conversion | `order_id` becomes |
| --- | --- | --- | --- |
| `Lowercase` | `lowercase` | Keep the name | `order_id` |
| `Uppercase` | `UPPERCASE` | Uppercase ASCII letters | `ORDER_ID` |
| `PascalCase` | `PascalCase` | Remove underscores and uppercase each word's first character in ASCII | `OrderId` |
| `CamelCase` | `camelCase` | PascalCase, then lowercase its first character in ASCII | `orderId` |
| `SnakeCase` | `snake_case` | Keep the name | `order_id` |
| `ScreamingSnakeCase` | `SCREAMING_SNAKE_CASE` | Uppercase ASCII letters | `ORDER_ID` |
| `KebabCase` | `kebab-case` | Replace underscores with hyphens | `order-id` |
| `ScreamingKebabCase` | `SCREAMING-KEBAB-CASE` | Uppercase ASCII letters and replace underscores with hyphens | `ORDER-ID` |

For example, `@rename_all(.CamelCase)` on `OrderPaid` gives the member
`order_id` the wire name `orderId`; a field's explicit `@rename` wins.

> **Why.** Document names belong to the serialization view; preserving the
> source name lets other adapters, such as environment readers, name fields
> independently.

## Map Keys

`MapKey` converts a map key to and from a format-neutral scalar:

```text
use std.serde.{MapKey, MapKeyValue}

struct Language:
    code: string

impl MapKey for Language:
    fn to_key(self) -> MapKeyValue: MapKeyValue.Text(self.code)
    fn from_key(value: MapKeyValue) -> Language?:
        match value:
            .Text(code) => .Some(Language { code: code })
            _ => .None
```

1. r[std-serde.map-key.value] `MapKeyValue` is the public enum `Bool(bool)`, `Int(i64)`, `Uint(u64)`, and `Text(string)`.
2. r[std-serde.map-key.trait] `MapKey` declares `to_key(self) -> MapKeyValue` and `from_key(value: MapKeyValue) -> Self?`.
3. r[std-serde.map-key.format-neutral] A serializer receives the scalar from `to_key`, and a deserializer gives `from_key` the scalar it read.
4. r[std-serde.map-key.writer-default] `Serializer.map_key` defaults to `key` with the scalar's canonical text.
5. r[std-serde.map-key.reader-default] `Deserializer.next_map_key` defaults to wrapping `next_key`'s text in `MapKeyValue.Text`.
6. r[std-serde.map-key.builtins] `string`, every integer type, `char`, and `bool` implement `MapKey` with their matching scalar kind.
7. r[std-serde.map-key.user-text] A user implementation uses `Text` for its public text representation and accepts that representation from `Text`.
8. r[std-serde.map-key.derive-template] `std.serde` declares the template of `MapKey`, which `@derive(MapKey)` instantiates.
9. r[std-serde.map-key.derive-shapes] Derived `MapKey` accepts a payload-free enum or an empty struct and uses the selected variant or struct name as `Text`.
10. r[std-serde.map-key.derive-payload-reject] Derived `MapKey` rejects an enum variant or struct that has a member.
11. r[std-serde.map-key.no-standard-impl] Floats, lists, and other user data types have no standard `MapKey` implementation.

> **Why.** The scalar enum gives a binary format native integer and boolean
> keys without adding one key-writer protocol per format.

### Absent Optional Members

The typed `SkipNone` fact omits an optional member only when it is absent.
For example, `@skip_none()` on `note: string?` omits the key for `.None`.

1. r[std-serde.skip-none.fact] `std.serde` exports the member-typed field fact `SkipNone[F]` for the field type `F`.
2. r[std-serde.skip-none.constructor] `skip_none[T]() -> SkipNone[T?]` constructs the fact for an optional field, checked by [`annot.typed-fact.check`](../lang/14-annotations.md#r-annot.typed-fact.check).
3. r[std-serde.skip-none.named-omit] Derived serialization omits a named member marked by `skip_none()` when its value is `.None`, without calling its serializer or the format's `member`.
4. r[std-serde.skip-none.present] A marked `.Some(value)` is written normally, including `.Some(.None)` as a present member whose value is null.
5. r[std-serde.skip-none.read] `SkipNone` does not change deserialization, so absent keys follow the ordinary optional and declared-default rules.
6. r[std-serde.skip-none.ordinary] An unmarked optional member still writes `.None` as null.
7. r[std-serde.skip-none.positional-error] A positional enum payload member with a known `SkipNone` fact is an error under a derived `Serialize`. Error: `member-not-derivable`.

> **Why.** The fact's optional predicate runs before the format sees the
> member, so every serializer receives the same view without confusing a
> present value that serializes as null with an absent optional.

> **Why.** A named member's key identifies which optional value is absent.
> A positional payload has no key, so omitting a value would make every
> later position ambiguous to a reader.

### Skipped Members

The `Skip` field fact omits a member from its serde view.
For example, `@skip()` on `cached: usize = 0` leaves the cache out of documents.

1. r[std-serde.skip.fact] `std.serde` exports the field fact `Skip` with no fields.
2. r[std-serde.skip.constructor] `skip() -> Skip` constructs the fact.
3. r[std-serde.skip.write] Derived serialization omits a member with `Skip` without calling its serializer or the format's `member` method for it.
4. r[std-serde.skip.named-read-default] Derived deserialization fills a skipped named member from its declared default without positioning or decoding its input, even when a key is present.
5. r[std-serde.skip.positional-read-default] Derived deserialization fills a skipped positional payload member with `T::default()`, where `T` is that member's type, without positioning or decoding its input.
6. r[std-serde.skip.impl-fact] The walker of derived serialization and the source of derived deserialization carry `skips::[Skip]()`. So [`annot.impl-fact.skips`](../lang/14-annotations.md#r-annot.impl-fact.skips) omits a member whose known facts hold `Skip`.
7. r[std-serde.skip.no-bound] A skipped field's type need not implement `Serialize` or `Deserialize`.
8. r[std-serde.skip.named-no-default] Under a derived `Deserialize`, a skipped named member without a declared default is an error at the opt-in. Error: `omitted-member-without-default`.
9. r[std-serde.skip.positional-no-default] Under a derived `Deserialize`, a skipped positional payload member whose type does not implement `Default` is an error at the opt-in. The diagnostic hint names the required `Default` implementation. Error: `omitted-member-without-default`.
10. r[std-serde.skip.named-unknown-fact] A `Skip` that only an opaque fact list or a spread attaches to a named member is read when the program runs. The member keeps both bounds, and reading it without a declared default returns a deserializer error naming the member.
11. r[std-serde.skip.positional-opaque-error] A positional payload member with an opaque fact list or spread is an error under derived `Serialize` or `Deserialize`, because the compiler cannot know whether it contains a shape-changing `Skip` or `SkipNone` fact. Error: `member-not-derivable`.
12. r[std-serde.skip.positional-form] A known `Skip` removes a positional payload member from the count that selects the derived unit, newtype, or tuple form and from a tuple form's `len`.

> **Why.** A skipped member is never written or read, so its type owes
> serde nothing. Saying so on the walker and source keeps the rule in
> library code.

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
| r[std-serde.std.result] Result | `Result[T, E]`, where `T` and `E` implement the trait | a newtype variant named `Ok` with the `T` payload, or one named `Err` with the `E` payload | the corresponding newtype variant and direct payload |
| r[std-serde.std.list] List | `List[T]`, where `T` implements the trait | a list of the items, in order | a list, item by item |
| r[std-serde.std.map-key-entries] Map | `Map[K, V]`, where `K` implements `Eq`, `Hash`, and `MapKey`, and `V` implements the trait | a map of the scalar-keyed entries, in the map's [iteration order](../lang/04-type-system.md#r-types.map.order.deterministic) | a map, entry by entry, in its order |
| r[std-serde.std.set] Set | `Set[T]`, where `T` implements the trait | a list of the elements, in insertion order | a list, element by element; a repeated element keeps one entry |
| r[std-serde.std.tuple] Tuple | a tuple type whose members implement the trait | a list of the members, in order | a list with one item per member, in order |
| r[std-serde.std.reference] Reference | `&T` and `&mut T`, where `T` implements the trait | what `T` writes for the value the reference refers to | a new object that holds what `T` reads |

1. r[std-serde.std.range] A value that its scalar call reads but the type cannot hold is the reader's `invalid(expected)` error. That is an integer outside the type's range, a `char` text of another length, or an `f32` past its finite range. The range of `usize` is the target's, by [`types.usize.width`](../lang/04-type-system.md#r-types.usize.width), so on the Wasm32 target it is that of `u32`.
2. r[std-serde.std.float-read] An `f32` reads the nearest `f32` to the `f64` it reads.
3. r[std-serde.std.tuple-length] A tuple reads exactly one list item per member. A list with fewer items, or with items left after the last member, is the reader's `invalid(expected)` error, so JSON reports it at the path of the list or of the first extra item.
4. r[std-serde.std.tuple-rest] A rest element is one member of type `List[T]`, by [`annot.walk.rest.default`](../lang/14-annotations.md#r-annot.walk.rest.default). So `(i32, List[i32]...)` writes `[1, [2, 3]]`, a list whose last item is the rest list.
5. r[std-serde.std.map-key-bound] A `Map[K, V]` implements a serde trait only when `K` implements `Eq`, `Hash`, and `MapKey` and `V` implements that serde trait.
6. r[std-serde.std.map-key-missing] Using a map key without `MapKey` where a serde trait is required is an error. Error: `unsatisfied-trait-bound`.

```text
use std.serde.Serialize

@derive(Eq, Hash)
struct Point:
    x: i32
    y: i32

fn send[T < Serialize](value: T) -> void:
    pass

fn send_points(points: Map[Point, i32]) -> void:
    send(points)  # error: unsatisfied-trait-bound
```

> **Note.** `T??` loses a layer: `.Some(.None)` writes `null`, which reads
> back as `.None`.

> **Why.** A format decides whether a key scalar is native or text. The
> key type owns the conversion, so every format sees the same value.

See also: [Serialization](../lang/14-annotations.md#serialization), [Typed JSON](json.md#typed-json), [Time](time.md#serialization), [Boundary Encoding](../lang/10-modules.md#boundary-encoding).
