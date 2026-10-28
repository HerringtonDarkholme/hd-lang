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
| r[std-serde.model.map] Map | entries in order, each with a `string` key | `begin_map(len)`, then per entry `key(k)` and one value, then `end_map()` |
| r[std-serde.model.variant] Variant | a struct value, or an enum value | `begin_variant(facts, v)`, then per member `member(m)` and one value, then `end_variant()` |

1. r[std-serde.model.one-value] A `serialize` call writes exactly one value of the table: one scalar call, or one begin call, its contents, and the matching end call.
2. r[std-serde.model.len] The `len` of `begin_list` or `begin_map` is the number of items or entries that follow.
3. r[std-serde.model.variant-args.local-ref] `begin_variant(facts: Facts, v: &VariantInfo)` receives the type-level facts and a local reference to the variant's metadata.
4. r[std-serde.model.data-variant] A struct value has one variant, whose `of_data` is true.
5. r[std-serde.model.member-args.local-ref] `member(m: &Member)` receives a local reference to the metadata of the member whose value follows.
6. r[std-serde.model.member-facts] The member's facts are those visible to its derivation, as [`annot.structure.per-derivation`](../lang/14-annotations.md#r-annot.structure.per-derivation) specifies.
7. r[std-serde.model.errors] A writer's first `.Err` ends the write. Every standard implementation returns it unchanged, and calls nothing after it.
8. r[std-serde.model.ref-receivers] Every method of `Serializer` and `Deserializer` takes `&mut self`, and `serialize` and `deserialize` take the writer or reader as `&mut W` or `&mut R`. So a format's writer or reader is a struct that changes in place, and its caller reads the struct's fields after the call.

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
| r[std-serde.read.absent] `is_absent` | `is_absent()` | `true` when the member that `member(m)` just positioned has no value at all, which reads nothing; the trait's default body gives `false` |

1. r[std-serde.read.one-value] A `deserialize` call reads exactly one value.
2. r[std-serde.read.expected] Each scalar call's `expected` names the type being read, as `"u8"`. A format's error for a value of another kind may report it.
3. r[std-serde.read.member.local-ref] A `member(m: &Member)` call positions the next value of that member, which the format may report as absent.
4. r[std-serde.read.choices] `choices` holds the `VariantInfo` of every variant of the type, in declaration order. A struct offers its one variant.

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
| `Session { user: "ada", token: "t", expires_at: 9 }` | `begin_variant`, `member` for `user`, `text("ada")`, `member` for `token`, `text("t")`, `member` for `expires_at`, `int(9)`, `end_variant()` |

1. r[std-serde.derive.serialize.template] `std.serde` declares the template of `Serialize`, which `@derive(Serialize)` instantiates.
2. r[std-serde.derive.serialize.included-order] Derived `serialize` calls `begin_variant`, then `member(m)` and `serialize` for each included walked member in declaration order, then `end_variant()`.
3. r[std-serde.derive.deserialize.template] `std.serde` declares the template of `Deserialize`, which `@derive(Deserialize)` instantiates.
4. r[std-serde.derive.deserialize.unskipped-order] Derived `deserialize` offers every variant to `begin_variant`, then calls `member(m)` and `deserialize` for each unskipped built member in declaration order, then `end_variant()`.
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
12. r[std-serde.rename-all.map-keys] A `RenameAll` style does not change string map keys.

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

### Absent Optional Members

The typed `SkipNone` fact omits an optional member only when it is absent.
For example, `@skip_none()` on `note: string?` omits the key for `.None`.

1. r[std-serde.skip-none.fact] `std.serde` exports the member-typed field fact `SkipNone[F]` for the field type `F`.
2. r[std-serde.skip-none.constructor] `skip_none[T]() -> SkipNone[T?]` constructs the fact for an optional field, checked by [`annot.typed-fact.check`](../lang/14-annotations.md#r-annot.typed-fact.check).
3. r[std-serde.skip-none.omit] Derived serialization omits a member marked by `skip_none()` when its value is `.None`, without calling its serializer or the format's `member`.
4. r[std-serde.skip-none.present] A marked `.Some(value)` is written normally, including `.Some(.None)` as a present member whose value is null.
5. r[std-serde.skip-none.read] `SkipNone` does not change deserialization, so absent keys follow the ordinary optional and declared-default rules.
6. r[std-serde.skip-none.ordinary] An unmarked optional member still writes `.None` as null.

> **Why.** The fact's optional predicate runs before the format sees the
> member, so every serializer receives the same view without confusing a
> present value that serializes as null with an absent optional.

### Skipped Members

The `Skip` field fact omits a member from its serde view.
For example, `@skip()` on `cached: usize = 0` leaves the cache out of documents.

1. r[std-serde.skip.fact] `std.serde` exports the field fact `Skip` with no fields.
2. r[std-serde.skip.constructor] `skip() -> Skip` constructs the fact.
3. r[std-serde.skip.write] Derived serialization omits a member with `Skip` without calling its serializer or the format's `member` method for it.
4. r[std-serde.skip.read] Derived deserialization fills a skipped member from its declared default without positioning or decoding its input, even when a key is present.
5. r[std-serde.skip.impl-fact] The walker of derived serialization and the source of derived deserialization carry `skips::[Skip]()`. So [`annot.impl-fact.skips`](../lang/14-annotations.md#r-annot.impl-fact.skips) omits a member whose known facts hold `Skip`.
6. r[std-serde.skip.no-bound] A skipped field's type need not implement `Serialize` or `Deserialize`.
7. r[std-serde.skip.no-default] Under a derived `Deserialize`, a skipped field without a declared default is an error at the opt-in. Error: `omitted-member-without-default`.
8. r[std-serde.skip.unknown-fact] A `Skip` that only a named list or a spread attaches is read when the program runs. The field keeps both bounds, and reading it without a declared default returns a deserializer error naming the field.

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
| r[std-serde.std.result] Result | `Result[T, E]`, where `T` and `E` implement the trait | an `Ok` variant with member `value`, or an `Err` variant with member `error` | the corresponding variant and member form |
| r[std-serde.std.list] List | `List[T]`, where `T` implements the trait | a list of the items, in order | a list, item by item |
| r[std-serde.std.map-entries] Map | `Map[string, V]`, where `V` implements the trait | a map of the entries, in the map's [iteration order](../lang/04-type-system.md#r-types.map.order.deterministic) | a map, entry by entry, in its order |
| r[std-serde.std.set] Set | `Set[T]`, where `T` implements the trait | a list of the elements, in insertion order | a list, element by element; a repeated element keeps one entry |
| r[std-serde.std.tuple] Tuple | a tuple type whose members implement the trait | a list of the members, in order | a list with one item per member, in order |
| r[std-serde.std.reference] Reference | `&T` and `&mut T`, where `T` implements the trait | what `T` writes for the value the reference refers to | a new object that holds what `T` reads |

1. r[std-serde.std.range] A value that its scalar call reads but the type cannot hold is the reader's `invalid(expected)` error. That is an integer outside the type's range, a `char` text of another length, or an `f32` past its finite range. The range of `usize` is the target's, by [`types.usize.width`](../lang/04-type-system.md#r-types.usize.width), so on the Wasm32 target it is that of `u32`.
2. r[std-serde.std.float-read] An `f32` reads the nearest `f32` to the `f64` it reads.
3. r[std-serde.std.tuple-length] A tuple reads exactly one list item per member. A list with fewer items, or with items left after the last member, is the reader's `invalid(expected)` error, so JSON reports it at the path of the list or of the first extra item.
4. r[std-serde.std.tuple-rest] A rest element is one member of type `List[T]`, by [`annot.walk.rest.default`](../lang/14-annotations.md#r-annot.walk.rest.default). So `(i32, List[i32]...)` writes `[1, [2, 3]]`, a list whose last item is the rest list.
5. r[std-serde.std.map-keys] Only a `Map` whose key type is `string` implements the traits. Using a map with any other key type where they are required is an error. Error: `unsatisfied-trait-bound`.

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
