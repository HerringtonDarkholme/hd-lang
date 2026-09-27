# Typed Derivation: Stress Test Round 3 (M1-M19)

Status: design review, 2026-09-27; changes no decision, design record, spec
text, or prototype code. Nothing here is accepted language behavior. Every
design choice below is a question for the owner.

The design under test is [Typed Derivation](TYPED_DERIVATION.md), owner
decisions M1-M19 in its [Owner Decisions](TYPED_DERIVATION.md#owner-decisions),
read with M19's value-driven walk in place of the M14 protocol that the
[Current Design example](TYPED_DERIVATION.md#current-design-full-example-m1-m14)
still shows. Also in force: [Enum Semantics decision 4](ENUM_SEMANTICS.md#owner-decisions)
(shared variant data are per-variant constants) and the `@error` intrinsic
of [Error Conversion](ERROR_CONVERSION.md#owner-decisions), which is only
tested where it meets derivation. Spec sections relied on:
[Mutable Paths](../spec/04-type-system.md#mutable-paths),
[Variance](../spec/04-type-system.md#variance),
[Shapes and Generic Code](../spec/04-type-system.md#shapes-and-generic-code),
[Newtypes](../spec/04-type-system.md#newtypes),
[Data Embedding](../spec/08-data-and-enums.md#data-embedding),
[Shared Enum Constructor Data](../spec/08-data-and-enums.md#shared-enum-constructor-data),
[Comparison Traits](../spec/09-traits.md#comparison-traits), and
[Associated Function Calls](../spec/09-traits.md#associated-function-calls).

[Round 1](DERIVATION_STRESS_TEST.md) used ids P1-P20 and
[round 2](DERIVATION_STRESS_TEST_2.md) used R1-R20. New problems here are
R3-1 to R3-12.

## Contents

1. [Surface Being Tested](#surface-being-tested)
2. [Method](#method)
3. [Summary](#summary)
4. [Cases](#cases) (1-14)
5. [Round-2 Problems Under M15-M19](#round-2-problems-under-m15-m19)
6. [Comparison With Other Languages](#comparison-with-other-languages)
7. [Problems, Ranked](#problems-ranked)
8. [Questions For The Owner](#questions-for-the-owner)
9. [Parse Log](#parse-log)

## Surface Being Tested

The record fixes `Walker` and `Describer` (M19), the handles (M14, M17,
P11g), and the generated `walk`, `describe`, and `build`. It does not write
the `Structure` signatures under M19, the `Member` and `VariantInfo`
types, or the `Source` trait for input-driven `build` (M18 P7). This
report assumes the smallest declarations that the record's text implies:

```text
# std.structure, as this report assumes it (M14, M17, M18, M19)
pub trait Structure:                     # sealed; only inside `by Structure` templates
    fn facts() -> Facts
    fn walk[W < Walker[Self]](self, w: mut W) -> Result[void, W::Error]
    fn describe[D < Describer[Self]](d: mut D) -> Result[void, D::Error]
    fn build[S < Source[Self]](s: mut S) -> Result[mut Self, S::Error]

pub data Field[S, F]:                    # one constant per member
    pub info: Member

impl[S, F] Field[S, F]:
    pub fn get(self, s: S) -> F: pass    # M17: typed like `s.field`
    pub fn has_default(self) -> bool: pass
    pub fn default(self) -> F?: pass

pub data Variant[S]:                     # one constant per variant
    pub info: VariantInfo

impl[S] Variant[S]:
    pub fn holds(self, s: S) -> bool: pass

pub data Member:                         # P11g
    pub name: string
    pub position: i32
    pub facts: Facts
    pub doc: string?
    pub embedded: bool

pub data VariantInfo:                    # P11g and Enum Semantics decision 4
    pub name: string
    pub index: i32
    pub facts: Facts
    pub shared: List[Any]                # assumed: the variant's constants, in declaration order

pub trait Walker[S]:                     # M19: a walk over one value
    type Error
    fn variant(mut self, v: Variant[S]) -> Result[void, Self::Error]
    fn member[F](mut self, h: Field[S, F], value: F) -> Result[void, Self::Error]

pub trait Describer[S]:                  # M19: a walk over the type only
    type Error
    fn variant(mut self, v: Variant[S]) -> Result[void, Self::Error]
    fn member[F](mut self, h: Field[S, F]) -> Result[void, Self::Error]
```

Four assumptions go beyond the record, and each case says when it uses one:

| Id | Assumption | Why |
| --- | --- | --- |
| A1 | `walk` takes `self` readonly. A template calls it as `Structure::walk(self, w)` (09 `Trait::method(receiver, ...)`). | Trait methods that encode or compare take `self`; the qualified form avoids a clash with a trait method named `walk`. |
| A2 | `VariantInfo.shared` holds the variant's shared constants as `List[Any]`. | Decision 4 says derivation reads them from `v.info` but gives no type. |
| A3 | M19's "a data type is an enum with one variant" means `walk`, `describe`, and `build` call `variant` for data types too. | That is the plain reading, and R3-2 tests it. |
| A4 | The `Source` trait below. | The record does not write it (R3-3 asks the owner to decide it). |

The proposed `Source`, used consistently in every case:

```text
# Proposed by this report (R3-3). Not in the design record.
pub data Members[S]:                     # the chosen variant's members: a compiler constant
    pub infos: List[Member]

impl[S] Members[S]:
    pub fn end(self) -> Key[S]: pass     # "input has no more members"
    pub fn at(self, position: i32) -> Key[S]: pass                 # end() when out of range
    pub fn find(self, matches: fn(Member) -> bool) -> Key[S]: pass # end() when none matches

pub data Key[S]:                         # names one member of the chosen variant
    pub info: Member
    pub is_end: bool

pub trait Source[S]:
    type Error
    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], Self::Error]
    fn next(mut self, members: Members[S]) -> Result[Key[S], Self::Error]
    fn member[F](mut self, h: Field[S, F], previous: F?) -> Result[F, Self::Error]
    fn missing[F](mut self, h: Field[S, F]) -> Result[F, Self::Error]
```

Generated `build` calls `variant` once, then `next` until it returns the end
key. For each key it calls `member`, passing the value already read for
that member, if any, as `previous` (a duplicate). After the loop it calls
`missing` once for each member that `next` never named. Generated code
never creates an error itself: it cannot, because it knows nothing about
`Self::Error`. M9's strengthened bound applies to `member` and `missing`.

```text
# Generated for `@derive(json.Decode)` on case 1's Post (illustrative)
fn build(s: mut json.FieldSource[Post]) -> Result[mut Post, json.DecodeError]:
    _ := s.variant(POST_VARIANTS)?               # A3: a data type has one variant
    let id: i64? = .None                         # generated code may keep flag bits instead
    let email: string? = .None
    while true:
        key := s.next(POST_MEMBERS)?
        if key.is_end:
            break
        match key.info.position:
            1 =>
                id = .Some(s.member[i64](h_id, id)?)
            2 =>
                email = .Some(s.member[string](h_email, email)?)
            _ =>
                panic("a key from another type or variant")
    stamps := s.missing[Timestamps](h_timestamps)?   # never named: see case 1
    id_value := match id:
        .Some(value) => value
        .None => s.missing[i64](h_id)?
    email_value := match email:
        .Some(value) => value
        .None => s.missing[string](h_email)?
    .Ok(Post { Timestamps: ...stamps, id: id_value, email: email_value })
```

This is the shape Kotlin's serialization plugin generates:
`decodeElementIndex` returns the next element's index or `DECODE_DONE`,
and generated code keeps one local per element
([kotlinx `decodeElementIndex`](https://kotlinlang.org/api/kotlinx.serialization/kotlinx-serialization-core/kotlinx.serialization.encoding/-composite-decoder/decode-element-index.html)).

## Method

Cases approximate the public API of well-known libraries: serde and
serde_json, prost, schemars, sqlx and diesel, clap, zeroize, figment and the
`merge` crate, proptest and quickcheck, and LLM tool schemas. Library shapes
are approximations written from their documentation, not their source. Each
case gives the original in its own language, the hd library side, the user
side, what the compiler generates where it matters, and a verdict.

Verdicts: *works* (the design expresses it and it behaves like the
original); *friction* (it works with a workaround, a visible behavior
difference, or a cost the original does not pay); *breaks* (the design
cannot express it, or it is unsound).

Code follows spec syntax: `=` for named arguments, brace data literals,
`T::f()` inside templates, `Structure::walk(self, w)` (A1), `.Ok()` for
`Result[void, E]`, no `mut` at argument sites, and `while true:` (there is
no `loop`). Library code reuses round 2's helper names (`key_for`,
`apply_case`, `style_of`, `find_variant`) without repeating them. M3 member
lines end in `# hypothetical syntax`.

Parsing checks syntax only. Nothing here was type-checked, the prototype
implements none of the surface, and a block's comments state the errors a
checker would report. Counting rules, where a case counts: a "block" is one
`impl ... by Structure` declaration; "per member" means per member per
value walked or built.

## Summary

| # | Case | Library | Verdict | Design element at fault |
| --- | --- | --- | --- | --- |
| 1 | JSON encode and decode | serde_json | Friction | Data type versus one-variant enum (R3-2). Flattening needs the part's cooperation and buffering (R3-8). Keys recomputed per member per call (R3-4). |
| 2 | Binary format with field numbers | prost | Friction | Duplicates must merge (R3-3 `previous`). Enum numbers are untyped shared constants (R3-7). Oneof tags repeated on the parent (R3-4). Tag checks at run time (R8). |
| 3 | JSON Schema and OpenAPI through `describe` | schemars | Friction | Schema and encoding drift across tier-2 blocks (R3-6). No schema name (R3-2). |
| 4 | SQL rows and DDL | sqlx, diesel | Works | Payload-free-only enums are checked by a bound trick (R8). |
| 5 | CLI parsing and help | clap | Friction | No type doc or variant doc (R3-2). Payload members cannot carry facts (R3-9). |
| 6 | `Eq`, `Hash` intrinsic beside templated `Debug` | std | Works | Type names (R3-2). Redaction keeps the member's obligation (P9). |
| 7 | Clone, diff, patch, in-place | std, `similar`, zeroize | Clone and diff work; `mut` members **break** (unsound); in-place **breaks** | Generic `h.get` upgrades access (R3-1). No place-based traversal (R3-10). |
| 8 | Config merging | figment, `merge` | Friction | Two-value sources can still panic (R3-11). In-place merge impossible (R3-10). |
| 9 | Generators and shrinking | proptest, quickcheck | Friction | Per-type facts recomputed on every call (R3-4). |
| 10 | Tool and LLM function schemas | schemars for tools | Friction | No type doc for the tool description (R3-2). Function targets wait for FN_TYPE. |
| 11 | Enum shapes | serde_repr, serde | Friction | Untyped shared constants (R3-7). Unnamed payloads (R3-9). One-variant enum (R3-2). |
| 12 | Generic, recursive, newtype | serde, GHC `newtype deriving` | Generic and recursive work; newtypes **break** for most traits | Deriving through the base needs `Self` rewrapping (R3-5). |
| 13 | Mirror types, cross-package use | serde `remote` | Friction | A foreign member type still needs a local wrapper type. |
| 14 | Performance and diagnostics | all | Walk holds; build and plans cost more than serde | R3-4. Drift and panics have no diagnostic (R3-6, R3-11). |

M19 fixes what round 2 ranked second: an enum walk is one `match`, a
walker no longer answers "enter this variant?", and `variant` can report
an error. The value-driven walk made encoders, hashers, and debug printers
short. Two decided fixes do not hold as written. M17's "get is typed like a
field read" has no typing inside generic walker and source code, so a
source can upgrade a `mut` member's access (R3-1). M18's "a newtype derives
through its base" needs the `Self` forwarding that M7 rejected for the
wrapper (R3-5). M19's one-variant reading of data types erases the
data-or-enum distinction that every tagged format needs (R3-2). The
remaining cost is the one serde avoids at macro time: per-type tables
(renamed keys, key lookup, tag sets) are rebuilt on every call (R3-4).

## Cases

### 1. JSON Encode And Decode (serde_json)

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Post {
    #[serde(flatten)]
    stamps: Timestamps,
    id: u64,
    full_name: String,
    #[serde(rename = "mail")]
    email: String,
    #[serde(default)]
    nickname: String,
}
```

User side. The embedded part is one member (M16); flattening it is json's
policy, read from `h.info.embedded`.

```text
use dep.json

@derive(json.Encode, json.Decode)
pub data Timestamps:
    pub created_at: i64
    pub updated_at: i64

@derive(json.Encode, json.Decode)
@json.style(case=.Camel)
pub data Post:
    Timestamps
    pub id: i64
    pub full_name: string
    @json.rename("mail")
    pub email: string
    pub nickname: string = ""
```

Library side. Flattening needs a second method on `Encode` that writes a
value's members without braces, so the part's own impl must cooperate:

```text
pub trait Encode:
    fn encode(self, out: mut Writer) -> Result[void, EncodeError]
    fn encode_members(self, out: mut Writer) -> Result[void, EncodeError]:
        .Err(EncodeError.NotAnObject)            # scalars and hand-written impls

pub trait Decode:
    fn decode(p: mut Parser) -> Result[Self, DecodeError]
    fn decode_value(v: Value) -> Result[Self, DecodeError]
    fn absent(key: string) -> Result[Self, DecodeError]:
        .Err(DecodeError.Missing(key))           # the impl for T? returns .Ok(.None)

impl[T] Encode for T by Structure:
    fn encode(self, out: mut Writer) -> Result[void, EncodeError]:
        out.begin_object()
        self.encode_members(out)?
        out.end_object()
        .Ok()

    fn encode_members(self, out: mut Writer) -> Result[void, EncodeError]:
        let w: mut Encoder[T] = Encoder { out: out, style: style_of(T::facts()) }
        Structure::walk(self, w)

data Encoder[S]:
    out: mut Writer
    style: Style

impl[S] Walker[S] for Encoder[S]:
    type Error = EncodeError

    fn variant(mut self, v: Variant[S]) -> Result[void, EncodeError]:
        # Called for Post too (A3). Nothing in v says whether S is a data type
        # (write no tag) or an enum with one variant (write the tag): R3-2.
        if !is_data(v):                          # assumed helper; no API provides it
            self.out.key(self.style.tag)
            self.out.string(apply_case(self.style.case, v.info.name))
        .Ok()

    fn member[F < Encode](mut self, h: Field[S, F], value: F) -> Result[void, EncodeError]:
        if h.info.embedded:
            return value.encode_members(self.out)    # flatten: F's members into this object
        self.out.key(key_for(self.style, h.info))    # recomputed per member per call (R3-4)
        value.encode(self.out)
```

The decoder is a source. A flattened part's keys arrive interleaved with
the outer members, so the source buffers every key it cannot place, and
`missing` builds the part from the buffer (the part's own key,
`Timestamps`, never appears in the input):

```text
impl[T] Decode for T by Structure:
    fn decode(p: mut Parser) -> Result[Self, DecodeError]:
        p.begin_object()?
        let s: mut FieldSource[T] = FieldSource { parser: p, style: style_of(T::facts()), rest: [] }
        value := T::build(s)?
        p.end_object()?
        .Ok(value)

    fn decode_value(v: Value) -> Result[Self, DecodeError]:
        T::decode(Parser::over(v))

data FieldSource[S]:
    parser: mut Parser
    style: Style
    rest: mut List[(string, Value)]              # keys no member claimed

impl[S] Source[S] for FieldSource[S]:
    type Error = DecodeError

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], DecodeError]:
        if choices.length() == 1:                # a data type, or an enum with one variant (R3-2)
            return .Ok(choices[0])
        name := self.parser.find_tag(self.style.tag)?   # may scan ahead and buffer
        find_variant(choices, name, self.style.case)

    fn next(mut self, members: Members[S]) -> Result[Key[S], DecodeError]:
        while self.parser.more_keys()?:
            name := self.parser.key()?
            key := members.find(fn(m: Member) -> bool: key_for(self.style, m) == name)
            if !key.is_end:
                return .Ok(key)
            self.rest.append((name, self.parser.value()?))   # unknown, or a flattened part's key
        .Ok(members.end())

    fn member[F < Decode](mut self, h: Field[S, F], previous: F?) -> Result[F, DecodeError]:
        if previous.is_some():
            return .Err(DecodeError.Duplicate(key_for(self.style, h.info)))
        F::decode(self.parser)

    fn missing[F < Decode](mut self, h: Field[S, F]) -> Result[F, DecodeError]:
        if h.info.embedded:
            return F::decode_value(Value.object(self.rest))
        match h.default():
            .Some(value) => .Ok(value)
            .None => F::absent(key_for(self.style, h.info))
```

Behavior against serde:

| Input | serde | hd with the proposed `Source` |
| --- | --- | --- |
| Keys in any order | accepted | accepted: `next` follows the input |
| A key twice | `duplicate field` error | `Duplicate`: the source sees `previous` |
| An unknown key | ignored, or an error with `deny_unknown_fields` | buffered; rejecting it is impossible while a part is flattened |
| `nickname` absent | `""` (`#[serde(default)]`) | `""`: the declared default |
| `filter: Filter?` absent | `None` | `.None` through `F::absent` |
| Flattened part's casing | the part's own `rename_all` | the part's own style: `created_at` beside `fullName` |

The last two rows match serde, including its documented limit that
`deny_unknown_fields` does not work with `flatten`
([serde container attributes](https://serde.rs/container-attrs.html#deny_unknown_fields)).

Skipping a member of a type without json impls still takes one tier-2
block per trait (M11), with the line repeated:

```text
impl json.Encode for Session by Structure:
    cache = pass                                 # hypothetical syntax
impl json.Decode for Session by Structure:
    cache = pass                                 # hypothetical syntax
```

**Verdict: friction.** The encoder cannot tell `Post` from a one-variant
enum (R3-2). Flattening needs `encode_members` on every part type and a
buffered decode, as serde's does (R3-8). `find` calls `key_for`, which
converts case and allocates, for every member on every key (R3-4).

### 2. A Binary Format With Field Numbers (prost)

```rust
#[derive(Message)]
struct SearchRequest {
    #[prost(string, tag = "1")] query: String,
    #[prost(enumeration = "Corpus", tag = "3")] corpus: i32,
    #[prost(string, repeated, tag = "4")] tags: Vec<String>,
    #[prost(oneof = "Filter", tags = "5, 6")] filter: Option<Filter>,
}
```

Prost repeats the oneof's tags on the parent field, as the generated
`google.protobuf.Value` shows: `#[prost(oneof = "value::Kind", tags = "1,
2, 3, 4, 5, 6")]`
([prost-types source](https://github.com/tokio-rs/prost/blob/master/prost-types/src/protobuf.rs)).

```text
use dep.pb

@derive(pb.Enum)
pub enum Corpus(number: i32):
    Universal -> Corpus(0)
    Web -> Corpus(1)
    Images -> Corpus(2)

@derive(pb.Oneof)
pub enum Filter:
    @pb.tag(5)
    Author(name: string)
    @pb.tag(6)
    Year(value: i32)

@derive(pb.Message)
pub data SearchRequest:
    @pb.tag(1)
    query: string = ""
    @pb.tag(3)
    corpus: Corpus = .Universal
    @pb.tag(4)
    tags: List[string] = []
    @pb.oneof(5, 6)                              # repeats Filter's tags, as prost does
    filter: Filter? = .None
    @pb.unknown()
    unknown: pb.UnknownFields = pb.UnknownFields {}
```

Decoding must merge a repeated occurrence of one field: "for numeric types
and strings ... the parser accepts the last value", embedded messages are
merged, and repeated fields are concatenated
([protobuf encoding, last one wins](https://protobuf.dev/programming-guides/encoding/#last-one-wins)).
The `previous` argument makes that possible; a source without it would lose
the first chunk of `tags`:

```text
impl[S] Source[S] for WireSource[S]:
    type Error = WireError

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], WireError]:
        .Ok(choices[0])                          # messages are data types (A3)

    fn next(mut self, members: Members[S]) -> Result[Key[S], WireError]:
        while !self.input.at_end():
            tag := self.input.peek_tag()?
            key := members.find(fn(m: Member) -> bool: owns_tag(m.facts, tag))
            if !key.is_end:
                return .Ok(key)
            self.unknown.append(self.input.raw_field()?)   # kept for the @pb.unknown member
        .Ok(members.end())

    fn member[F < pb.Field](mut self, h: Field[S, F], previous: F?) -> Result[F, WireError]:
        value := F::read(self.input)?
        match previous:
            .Some(earlier) => .Ok(earlier.merge(value))    # append, merge, or last wins: F decides
            .None => .Ok(value)

    fn missing[F < pb.Field](mut self, h: Field[S, F]) -> Result[F, WireError]:
        if is_unknown_sink(h.info.facts):
            return F::from_unknown(self.unknown)
        match h.default():
            .Some(value) => .Ok(value)
            .None => .Ok(F::zero())              # proto3: absent means the zero value
```

`Corpus` should encode as its `number`. Under decision 4 that constant
lives in `v.info`, which has no type for it (A2), so the template downcasts:

```text
impl[S] Walker[S] for EnumNumber[S]:
    type Error = WireError

    fn variant(mut self, v: Variant[S]) -> Result[void, WireError]:
        match v.info.shared[0].downcast[i32]():  # untyped: a run-time check (R3-7)
            .Some(number) =>
                self.out.varint(number)
                .Ok()
            .None => .Err(WireError.NotAnEnumNumber(v.info.name))

    fn member[F < NoPayload](mut self, h: Field[S, F], value: F) -> Result[void, WireError]:
        .Ok()                                    # NoPayload has no impls: payloads are rejected at the opt-in
```

What a checker would catch, and when:

| Mistake | Caught |
| --- | --- |
| `@pb.tag("one")` | compile time: the annotation function's argument type |
| A member without `@pb.tag` | run time, first encode (cross-member: R8) |
| Two members with tag 3 | run time, or never (R8) |
| A variant added to `Filter` without updating `@pb.oneof(5, 6)` | never: its field is buffered as unknown (R8, R3-4) |
| Reordering members | nothing changes: tags are explicit (P20 is only documented) |

**Verdict: friction.** Merging needs `previous` (R3-3). Enum numbers need a
downcast (R3-7). Oneof tags are repeated by hand because `next` is not
generic and cannot ask `Filter` for its tags; a per-type plan would (R3-4).
Tag checks wait for a cross-member check hook (R8).

### 3. JSON Schema And OpenAPI Through `describe` (schemars)

```text
impl[T] Schema for T by Structure:
    fn schema(defs: mut Defs) -> Node:
        key := schema_name[T]()                  # from where? (R3-2, R9)
        if defs.reserve(key):                    # false when known or in progress: recursion
            let d: mut SchemaDescriber[T] = SchemaDescriber { defs: defs, style: style_of(T::facts()), variants: [] }
            _ := T::describe(d)
            defs.define(key, finish(d.variants))    # one object, or oneOf; payload-free variants become const
        Node.Ref(key)

impl[S] Describer[S] for SchemaDescriber[S]:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        self.variants.append(VariantSchema { info: v.info, properties: [], required: [], all_of: [] })
        .Ok()

    fn member[F < Schema](mut self, h: Field[S, F]) -> Result[void, never]:
        node := F::schema(self.defs).described(h.info.doc)
        current := self.variants.last_mut()
        if h.info.embedded:
            current.all_of.append(node)          # a flattened part: allOf
            return .Ok()
        key := key_for(self.style, h.info)
        current.properties.append((key, node))
        if !h.has_default() && !F::nullable():
            current.required.append(key)
        .Ok()
```

Within one block, `describe` and `walk` are generated from the same member
list and the same member lines, so they agree on which members exist, and
the schema's `required` set matches the decoder's use of `h.default()`.
That consistency does not cross blocks:

```text
@derive(json.Schema, json.Decode)
@json.style(case=.Camel)
pub data Invoice:
    pub id: i64
    pub total_cents: i64

impl json.Encode for Invoice by Structure:
    total_cents = [json.rename("total")]         # hypothetical syntax
```

This compiles. The encoder writes `"total"`, while the schema and the
decoder say `"totalCents"`, so the service's own output fails its published
schema and its own decoder. No diagnostic points anywhere (R3-6). schemars
avoids this by reading serde's own attributes
([schemars attributes](https://graham.cool/schemars/deriving/attributes/)).

**Verdict: friction.** Drift across blocks (R3-6). The `$defs` key needs a
type name (R3-2). A payload-free variant is recognized only after the next
`variant` call or the end, because `VariantInfo` has no member count.

### 4. SQL Rows And DDL (sqlx, diesel)

```text
use dep.db

@derive(db.Row, db.Table)
@db.table("users")
pub data UserRow:
    @db.primary_key()
    pub id: i64
    pub email: string
    pub status: Status = .Active
    pub bio: string? = .None

@derive(db.Column)
pub enum Status:
    Active
    Banned
```

A row arrives in the query's column order, which is exactly what
input-driven `build` is for:

```text
impl[S] Source[S] for RowSource[S]:
    type Error = DbError

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], DbError]:
        if choices.length() != 1:
            return .Err(DbError.NotARow)
        .Ok(choices[0])

    fn next(mut self, members: Members[S]) -> Result[Key[S], DbError]:
        while self.column < self.row.width():
            name := self.row.name(self.column)
            key := members.find(fn(m: Member) -> bool: column_name(m) == name)
            if !key.is_end:
                return .Ok(key)
            self.column = self.column + 1        # a column the type does not map
        .Ok(members.end())

    fn member[F < Column](mut self, h: Field[S, F], previous: F?) -> Result[F, DbError]:
        value := F::from_sql(self.row.get(self.column))?
        self.column = self.column + 1
        .Ok(value)

    fn missing[F < Column](mut self, h: Field[S, F]) -> Result[F, DbError]:
        match h.default():
            .Some(value) => .Ok(value)
            .None => .Err(DbError.MissingColumn(column_name(h.info)))
```

DDL is a describer. Nullability comes from `F::nullable()`, and `DEFAULT`
from `h.default()` rendered by `F::sql_literal`. `db.Column` for `Status`
writes `v.info.name` and uses a bound no type implements to reject
payloads at the opt-in:

```text
@derive(db.Column)
pub enum Bad:
    Ok
    Failed(reason: string)
# error at @derive(db.Column): member `reason`: string does not implement db.NoPayload
```

The error names the member, but its wording is a side effect of the trick.
A type-level check (R8) would say "db.Column needs payload-free variants".

**Verdict: works.**

### 5. CLI Parsing And Help (clap)

```rust
/// A tiny file tool
#[derive(Parser)]
struct Cli {
    /// Print more
    #[arg(short, long)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Add a file
    Add { path: String },
    /// Remove by id
    Remove { id: u64, #[arg(long)] force: bool },
}
```

clap turns the struct's doc comment into `about` and each variant's doc
comment into its subcommand help
([clap derive reference](https://docs.rs/clap/latest/clap/_derive/index.html)).

```text
use dep.cli

## A tiny file tool
@derive(cli.Parse)
pub data Cli:
    ## Print more
    @cli.short("v")
    verbose: bool = false
    @cli.subcommand()
    command: Command

@derive(cli.Parse)
pub enum Command:
    ## Add a file
    Add(path: string)
    ## Remove by id
    Remove(id: i64, force: bool)
```

The source reads `argv`. `next` maps `--verbose` or `-v` to a member by
facts, and a bare word to the `@cli.subcommand()` member. It needs that
fact because `next` is not generic and cannot ask a member's type whether
it is a subcommand. `member` asks `F` whether it takes a value, and
`missing` falls back to the default or to `F::absent`, which is `false` for
`bool` and "required" otherwise. The template renders help with `describe`
before `build` and hands the text to the source, because a source has no
`Structure` bound and cannot describe `S` itself.

```text
impl[S] Source[S] for ArgSource[S]:
    type Error = CliError

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], CliError]:
        if choices.length() == 1:
            return .Ok(choices[0])
        word := self.args.take_word()?
        for v in choices:
            if kebab(v.info.name) == word:
                return .Ok(v)
        .Err(CliError.UnknownCommand(word, self.help))

    fn next(mut self, members: Members[S]) -> Result[Key[S], CliError]:
        match self.args.peek():
            .None => .Ok(members.end())
            .Some(token) =>
                if token == "--help" || token == "-h":
                    return .Err(CliError.Help(self.help))
                key := members.find(fn(m: Member) -> bool: matches_flag(m, token))
                if key.is_end:
                    return .Err(CliError.UnknownFlag(token, suggest(members, token)))
                .Ok(key)

    fn member[F < cli.Arg](mut self, h: Field[S, F], previous: F?) -> Result[F, CliError]:
        if previous.is_some() && !F::repeatable():
            return .Err(CliError.Repeated(flag_of(h.info)))
        F::parse_arg(self.args, previous)

    fn missing[F < cli.Arg](mut self, h: Field[S, F]) -> Result[F, CliError]:
        match h.default():
            .Some(value) => .Ok(value)
            .None => F::absent(flag_of(h.info))
```

What does not carry over:

- **No `about` text.** "A tiny file tool" documents the type. P11g gives
  docs to members only, and `Structure` has no type-level doc (R3-2, R9).
- **No subcommand help.** "Add a file" documents a variant, and
  `VariantInfo` has no `doc` (R3-2).
- **No per-payload flags.** `force` should be `--force`, but
  `Remove(id: i64, @cli.long() force: bool)` is a `syntax-error`: payload
  parameters take no decorators until Error Conversion decision 12's
  grammar is applied (R3-9).

**Verdict: friction** (R3-2, R3-9).

### 6. `Eq` And `Hash` Intrinsic Beside A Templated `Debug`

```text
use dep.fmt

@derive(Eq, Hash, fmt.Debug)
pub data Session:
    user: string
    @fmt.redact()
    token: string

data DebugWalker[S]:
    out: mut fmt.Formatter
    first: bool

impl[S] Walker[S] for DebugWalker[S]:
    type Error = fmt.Error

    fn variant(mut self, v: Variant[S]) -> Result[void, fmt.Error]:
        self.out.write(v.info.name)              # "Session", if a data type's variant is named after it (R3-2)
        self.out.write(" { ")
        .Ok()

    fn member[F < fmt.Debug](mut self, h: Field[S, F], value: F) -> Result[void, fmt.Error]:
        if !self.first:
            self.out.write(", ")
        self.first = false
        self.out.write(h.info.name + ": ")
        if h.info.facts.find[fmt.Redact]().is_some():
            return self.out.write("***")
        value.debug(self.out)
```

M18 allows intrinsic and templated traits in one `@derive` list, so this
is one line, as in Rust. `Eq` and `Hash` see every member, with no member
lines (M18 R3), so the law-partner drift of round 2 cannot happen. Output:
`Session { user: "ada", token: *** }`.

Two gaps. The walker needs the type name, and the only candidate is
`v.info.name` under A3 (R3-2). Redaction keeps the obligation
`string < fmt.Debug`, so a member whose type has no `Debug` cannot be shown
as `***`; `= pass` drops it from the output instead (P9, unchanged).

**Verdict: works.**

### 7. Clone, Diff, Patch, And In-Place Traversal

**Clone** is a source over the old value. It names members in declaration
order:

```text
pub trait Clone:
    fn clone(self) -> mut Self

impl[S] Source[S] for CloneSource[S]:
    type Error = never

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], never]:
        for v in choices:
            if v.holds(self.old):
                return .Ok(v)
        panic("a value holds exactly one variant")

    fn next(mut self, members: Members[S]) -> Result[Key[S], never]:
        key := members.at(self.position)
        self.position = self.position + 1
        .Ok(key)

    fn member[F < Clone](mut self, h: Field[S, F], previous: F?) -> Result[F, never]:
        .Ok(h.get(self.old).clone())

    fn missing[F < Clone](mut self, h: Field[S, F]) -> Result[F, never]:
        panic("next names every member")
```

This works for ordinary members. A `mut` member exposes a hole in M17.
`h.get(s)` is "typed like `s.field`", but inside a generic source `F` is a
type parameter, and 04's rule for a generic field is "the substituted type,
unchanged". So `h.get(readonly_s)` has type `F`. `build` instantiates `F`
with the declared type (M17), which for `hits: mut Cell` is `mut Cell`:

```text
data Cell:
    count: i64

@derive(Alias)
data Counter:
    name: string
    hits: mut Cell

pub trait Alias:
    fn alias(self) -> mut Self

impl[T] Alias for T by Structure:
    fn alias(self) -> mut Self:
        let s: mut ShareSource[T] = ShareSource { old: self, position: 0 }
        match T::build(s):
            .Ok(copy) => copy
            .Err(e) => e

impl[S] Source[S] for ShareSource[S]:
    type Error = never

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], never]:
        .Ok(choices[0])

    fn next(mut self, members: Members[S]) -> Result[Key[S], never]:
        key := members.at(self.position)
        self.position = self.position + 1
        .Ok(key)

    fn member[F](mut self, h: Field[S, F], previous: F?) -> Result[F, never]:
        .Ok(h.get(self.old))                     # F = mut Cell, and self.old is readonly

    fn missing[F](mut self, h: Field[S, F]) -> Result[F, never]:
        panic("next names every member")

fn leak(c: Counter) -> void:
    let copy: mut Counter = c.alias()
    copy.hits.count = 99                         # mutates the Cell that `c` reached only readonly
```

This violates 08's rule that "a copy of a readonly value has mutable
access only when nothing mutable is read through a readonly view to make
it" (`data.edge.principle`). The same typing makes M19's walk inconsistent:
generated `walk` reads `value.hits` through readonly `self` (A1), which is
`Cell`, but `member[F](h: Field[S, F], value: F)` has one `F` for both, and
the handle constant is `Field[Counter, mut Cell]` (R3-1).

**Diff** is a two-value walker, and M19's error channel gives it a clean
exit when the variants differ:

```text
data VariantChanged: pass

impl[S] Walker[S] for DiffWalker[S]:
    type Error = VariantChanged

    fn variant(mut self, v: Variant[S]) -> Result[void, VariantChanged]:
        if !v.holds(self.old):
            return .Err(VariantChanged {})       # the template records a whole-value change
        .Ok()

    fn member[F < Diff](mut self, h: Field[S, F], value: F) -> Result[void, VariantChanged]:
        h.get(self.old).diff_into(value, self.path + "." + h.info.name, self.changes)
        .Ok()
```

A walker whose error type is `never` has no way to stop at a variant
mismatch. If it forgets the `holds` check, the first `h.get(self.old)`
panics (R3-11). Patch is round 2's `PatchSource` with `next` added, and
works the same way.

**In place** (zeroize): Rust's `#[derive(Zeroize)]` writes code that
overwrites each field through `&mut self`
([zeroize](https://docs.rs/zeroize/latest/zeroize/)). hd's walk hands out
values, not places; `build` makes a new value; and a `mut self` method
cannot replace `self`:

```text
pub trait Zeroize:
    fn zeroize(mut self) -> void

impl[T] Zeroize for T by Structure:
    fn zeroize(mut self) -> void:
        # Nothing can store into self's `password: string`: the walk passes a
        # string value, and no handle has a `set` (R3-10). Not expressible.
        pass
```

**Verdict:** clone and diff work; `mut` members **break** (unsound, R3-1);
in-place traversal **breaks** (R3-10).

### 8. Config Merging (figment, the `merge` crate)

Layered configuration reads the first layer that has a key. As a source
this is typed and short:

```text
impl[S] Source[S] for LayerSource[S]:
    type Error = ConfigError

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], ConfigError]:
        if choices.length() == 1:
            return .Ok(choices[0])
        find_variant(choices, self.layers.first_tag()?, .Snake)

    fn next(mut self, members: Members[S]) -> Result[Key[S], ConfigError]:
        key := members.at(self.position)
        self.position = self.position + 1
        .Ok(key)

    fn member[F < Decode](mut self, h: Field[S, F], previous: F?) -> Result[F, ConfigError]:
        match self.layers.first_with(key_for(self.style, h.info)):
            .Some(value) => F::decode_value(value).map_err(ConfigError.Decode)
            .None => self.missing(h)

    fn missing[F < Decode](mut self, h: Field[S, F]) -> Result[F, ConfigError]:
        match h.default():
            .Some(value) => .Ok(value)
            .None => .Err(ConfigError.Missing(h.info.name))
```

`self.missing(h)` inside `member` is a call through a concrete source, so
M9 rule 2 allows it. Merging two typed values is a source over both:

```text
impl[S] Source[S] for MergeSource[S]:
    type Error = never

    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], never]:
        for v in choices:
            if v.holds(self.overlay):
                self.same = v.holds(self.base)   # forget this, and member panics (R3-11)
                return .Ok(v)
        panic("a value holds exactly one variant")

    fn next(mut self, members: Members[S]) -> Result[Key[S], never]:
        key := members.at(self.position)
        self.position = self.position + 1
        .Ok(key)

    fn member[F < Merge](mut self, h: Field[S, F], previous: F?) -> Result[F, never]:
        if !self.same:
            return .Ok(h.get(self.overlay))
        .Ok(h.get(self.base).merge(h.get(self.overlay)))

    fn missing[F < Merge](mut self, h: Field[S, F]) -> Result[F, never]:
        panic("next names every member")
```

The `merge` crate's derive is in place, `fn merge(&mut self, other: Self)`
([merge](https://docs.rs/merge/latest/merge/)); hd can only return a new
value (R3-10). "Unset in the overlay" needs a partial type with every
member optional, which derivation does not generate (P10, decided out of
scope).

**Verdict: friction** (R3-11, R3-10).

### 9. Generators And Shrinking (proptest, quickcheck)

quickcheck's `Gen` carries a size that recursive generators shrink as they
descend ([quickcheck `Gen`](https://docs.rs/quickcheck/latest/quickcheck/struct.Gen.html)).
At size zero a recursive enum must pick a variant that does not recurse.
The only way to learn which is `describe`, and nothing keeps its answer
between calls:

```text
impl[T] Arbitrary for T by Structure:
    fn arbitrary(g: mut Gen) -> mut Self:
        let sizes: mut VariantSizes[T] = VariantSizes { counts: [] }
        _ := T::describe(sizes)                  # on every call, at every level (R3-4)
        let s: mut RandomSource[T] = RandomSource { gen: g, smallest: sizes.smallest(), position: 0 }
        match T::build(s):
            .Ok(value) => value
            .Err(e) => e

impl[S] Describer[S] for VariantSizes[S]:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        self.counts.append(0)
        .Ok()

    fn member[F](mut self, h: Field[S, F]) -> Result[void, never]:
        self.counts.set_last(self.counts.last() + 1)
        .Ok()
```

`RandomSource.variant` picks `choices[self.smallest]` at size zero and a
random choice otherwise; `member` calls `F::arbitrary(self.gen.smaller())`.
Shrinking is round 2's one-candidate-per-build loop; `previous` is unused.

**Verdict: friction.** The describe pass runs once per generated value per
nesting level (R3-4). Member count stands in for "does not recurse", which
is wrong for `Leaf(value: T)` beside `Nil`, and a real answer needs
`TypeId` comparisons under an `Inspectable` bound.

### 10. Tool And LLM Function Schemas

```text
use dep.json

## Search the catalog by keyword
@derive(json.Decode, json.Schema)
@json.strict()
data SearchArgs:
    ## Words to search for
    query: string
    ## At most this many results
    limit: i64 = 20
    order: Order = .Relevance

@derive(json.Decode, json.Schema)
enum Order:
    Relevance
    Newest
```

Case 3's describer produces the parameter schema, and `@json.strict()`
switches it to "every property required, optional ones nullable", as
OpenAI's strict mode requires. `Order` becomes `{"enum": [...]}` once the
describer has seen that no variant has members. The tool description,
"Search the catalog by keyword", is the type's doc, which `Structure` does
not expose (R3-2). Tools declared as functions still wait for
[FN_TYPE](FN_TYPE.md).

**Verdict: friction** (R3-2).

### 11. Enum Shapes

**Shared constants.** serde_repr encodes a C-like enum as its integer
([serde_repr](https://docs.rs/serde_repr/latest/serde_repr/)), and Swift
encodes a raw-value enum as its raw value. In hd the integer is a shared
constant:

```text
@derive(json.Encode, json.Decode)
@json.repr("code")                               # json's fact: encode as the shared constant `code`
pub enum HttpStatus(code: i32, phrase: string):
    Ok -> HttpStatus(200, phrase="OK")
    NotFound -> HttpStatus(404, phrase="Not Found")
```

Encoding reads `v.info.shared`, which is untyped under A2; the walker must
find `code` by position and downcast it. Decoding compares `404` against
each variant's downcast constant. A typo in `@json.repr("cod")` is a run-time
error (R3-7).

**Unnamed payloads.** `Num(f64)` is a legal variant (08 Variant Payloads).
serde writes a newtype variant as `{"Num": 1.5}`. The walker sees a member
whose `name` is presumably `_0` (the spelling decided for `@error` messages
in Error Conversion gap 3), and has no flag saying the member is
positional (R3-9):

```text
@derive(json.Encode)
pub enum Token:
    Num(f64)
    Ident(string)
    Pair(left: string, right: string)
# Token.Num walks as variant(v_num), then member(h_num_0, 1.5): h_num_0.info.name == "_0"?
```

**Facts on payload members.** P11d says declaration facts on payload
parameters still apply, but no grammar accepts them yet:

```text
pub enum Event:
    Logout(user: i64, @json.rename("why") reason: string)   # hypothetical syntax
```

**Data type or one-variant enum** (A3):

```text
@derive(json.Encode)
pub data Point:
    x: i64

@derive(json.Encode)
pub enum Shape:
    Point(x: i64)
# Point { x: 1 } walks as: variant(v) with v.info.name == "Point", then member(h_x, 1)
# Shape.Point(x=1) walks as: variant(v) with v.info.name == "Point", then member(h_x, 1)
# json should write {"x":1} and {"type":"Point","x":1}; the walker cannot tell which
```

aeson has the same ambiguity on purpose: Haskell has no separate record
declaration, and aeson's default `tagSingleConstructors = False` encodes
every one-constructor type untagged
([aeson `Options`](https://hackage.haskell.org/package/aeson/docs/Data-Aeson-Types.html#t:Options)).
hd declares the difference, so losing it is new (R3-2).

**Verdict: friction** (R3-7, R3-9, R3-2).

### 12. Generic, Recursive, And Newtype Targets

Generic and recursive types behave as round 2 found, now with value-driven
walks:

```text
@derive(json.Encode, json.Decode)
pub data Page[T]:
    pub items: List[T]
    pub next: string? = .None
# derived: impl[T < json.Encode] json.Encode for Page[T], and the same for Decode (M12)

@derive(json.Encode, json.Decode, json.Schema)
pub enum Json:
    Null
    Bool(bool)
    Num(f64)
    Str(string)
    Arr(List[Json])
    Obj(Map[string, Json])
# coinductive: Arr's List[Json] may assume Json's own impls; Schema recurses through defs.reserve
```

**Newtypes** derive through their base (M18 P11b, 09 Derived Newtypes).
09's rule works for `Eq` and `Hash` because `Self` appears only as the
receiver and as a plain parameter. Templated traits put `Self` elsewhere:

```text
@derive(json.Encode, json.Decode, prop.Shrink)
type UserId(i64)

# What "through the base" has to generate:
impl json.Encode for UserId:
    fn encode(self, out: mut json.Writer) -> Result[void, json.EncodeError]:
        i64(self).encode(out)                    # receiver: unwrap. Fine.

impl json.Decode for UserId:
    fn decode(p: mut json.Parser) -> Result[Self, json.DecodeError]:
        match i64::decode(p):                    # Self inside Result: rewrap
            .Ok(raw) => .Ok(UserId(raw))
            .Err(e) => .Err(e)

impl prop.Shrink for UserId:
    fn shrink(self) -> List[Self]:
        [for raw in i64(self).shrink() => UserId(raw)]   # Self inside List: a new list
```

Each position needs its own rewrapping code: `Result[Self, E]`, `Self?`,
`List[Self]`, `Map[string, Self]`, `fn(Self) -> bool`. That is the
`Self` forwarding M7 rejected for the `Derive[T]` wrapper, because it needs
coercions with GHC-style roles
([GHC roles](https://downloads.haskell.org/ghc/latest/docs/users_guide/exts/roles.html)).
A trait with `fn index(values: List[Self]) -> Map[Self, i64]` has `Self` as
a map key, where rewrapping is not even meaning-preserving if the newtype's
`Hash` differs from the base's (R3-5).

**Verdict:** generic and recursive targets **work**; newtypes **break**
for any trait with `Self` outside the receiver and plain positions (R3-5).

### 13. Mirror Types And Cross-Package Use

serde's `remote` pairs a mirror with `#[serde(with = "PointDef")]` on the
field, so the domain type keeps `geo::Point`. hd has the mirror (M18 P15)
but not the per-field codec (M6), so the member's obligation still fails:

```text
use dep.geo
use dep.json

@derive(json.Encode, json.Decode)
data PointDef:
    lat: f64
    lng: f64

@derive(json.Encode)
pub data Place:
    name: string
    at: geo.Point
# error at @derive(json.Encode): member `at`: geo.Point does not implement json.Encode
```

The workaround changes the domain type to a local newtype with hand-written
impls that convert through the mirror. Deriving on the newtype would go
through its base, which has no impl:

```text
type JsonPoint(geo.Point)

impl json.Encode for JsonPoint:
    fn encode(self, out: mut json.Writer) -> Result[void, json.EncodeError]:
        p := geo.Point(self)
        PointDef { lat: p.lat, lng: p.lng }.encode(out)

@derive(json.Encode)
pub data Place2:
    name: string
    at: JsonPoint                                # every use site now wraps and unwraps
```

If `geo.Point` later adds a public field with a default, the conversion
compiles and silently drops it. serde's `remote` catches field drift because
its generated code names every field of the remote type.

**Across packages** everything needed is in interfaces (M18 P19):
template bodies, the walkers and sources they name, annotation functions.
One consequence: a walker's strengthened bound is part of the library's
public API. If json 1.1 changes `Encoder.member` to `F < Encode + Sized`,
every downstream `@derive(json.Encode)` is rechecked, and the error appears
in whichever dependency declared the type, not in the app. That matches
Rust's behavior when serde tightens a bound, and needs only a note in the
packaging guide.

**Verdict: friction.** P15's mirror covers top-level values; members of
foreign types need a wrapper type (M6, accepted).

### 14. Performance And Diagnostics

Generated code for a data type and an enum, for json's encoder:

```text
fn walk(value: Post, w: mut json.Encoder[Post]) -> Result[void, json.EncodeError]:
    w.variant(v_post)?                           # A3
    w.member[Timestamps](h_timestamps, value.Timestamps)?
    w.member[i64](h_id, value.id)?
    w.member[string](h_full_name, value.full_name)?
    w.member[string](h_email, value.email)?
    w.member[string](h_nickname, value.nickname)?
    .Ok()

fn walk(value: Event, w: mut json.Encoder[Event]) -> Result[void, json.EncodeError]:
    match value:                                 # M19: one match, one variant call
        Event.Login(user) =>
            w.variant(v_login)?
            w.member[i64](h_login_user, user)?
        Event.Logout(user, reason) =>
            w.variant(v_logout)?
            w.member[i64](h_logout_user, user)?
            w.member[string](h_logout_reason, reason)?
    .Ok()
```

Cost per member, under 04's per-shape compilation:

| Step | M14 (round 2) | M19 walk | Proposed build |
| --- | --- | --- | --- |
| Variant selection per value | `O(variants)` `holds` calls | one `match` | one `variant` call |
| Call into the walker or source | 1 direct, shape-specialized | 1 direct | `next` plus `member`: 2 |
| Read the member | `h.get`, unstated cost | a local from the `match`: free | not needed |
| Call through `F`'s dictionary | 1 | 1 | 1 |
| Library work (`key_for`) | 1 case conversion, allocates | 1, allocates | up to `n` per key in `find`: `O(n²)` per object |
| Allocation | none | none, if `.Ok()` of `Result[void, E]` is canonical | `.Some` per local unless flags; `.Some` per `h.default()` (R15) |

Two claims need a sentence in 04 or the record. M9 rule 4's "no
per-member allocation" assumes `.Ok()` of `Result[void, E]` allocates
nothing; Enum Semantics decision 1 makes payload variants GC structs and
does not say whether a `void` payload counts. And the generated `build`'s
locals should be flags, not `F?`, or each present member allocates a
`.Some`.

Code size is one `walk`, `describe`, or `build` per (type, template), as
M18 P19 says. Walker and source bodies are generic in `F`, so each is
compiled at most once per shape in its own package. That is smaller than
serde, which expands a full visitor per type.

Where errors point:

| Mistake | Reported | Quality |
| --- | --- | --- |
| A member type lacks the walker's bound | the `@derive` line, naming the member | good; the bound shown is the walker's, not the trait's (R14) |
| A member line names no member | the line | good |
| `= pass` on a member with no default, in a building template | the line | good |
| `@derive(X)` plus a tier-2 block for `X` | `overlapping-impl` | good |
| A configuration fact no derived template reads | lint at the fact (M18) | good |
| Schema and encoder disagree across blocks | nowhere | R3-6 |
| A walker reads another variant's payload | run-time panic in library code | R3-11 |
| A `Key` from another variant reaches generated `build` | run-time panic in generated code | R3-3 |
| A newtype derives a trait with `Self` in `List[Self]` | not specified | R3-5 |
| Missing or duplicate `@pb.tag` | run time, or never | R8 |

**Verdict:** the walk holds its cost claims. `build` and every library that
needs per-type tables pay per call what serde pays at macro time (R3-4).

## Round-2 Problems Under M15-M19

| Id | Round-2 problem | Status | Where |
| --- | --- | --- | --- |
| R1 | A `mut` member's handle has no single member type | **Not fixed.** M17 types `get` like a field read, which has no meaning where `F` is a type parameter; a source can upgrade access (R3-1). | Case 7 |
| R2 | The enum protocol | **Fixed** by M19: one `match`, `variant` returns `Result`, no answers to get wrong. Residue: two-value code still calls `h.get` on the other value (R3-11). New: data types look like one-variant enums (R3-2). | Cases 1, 7, 11 |
| R3 | `= pass` and law partners | **Fixed** for comparison traits by M18 (intrinsic, no member lines). The same drift remains between json's `Schema`, `Encode`, and `Decode` (R3-6). | Case 3 |
| R5 | Handles escape the walk | **Accepted** by M18. | — |
| P13 | Tier-1 template selection | **Fixed** by M18 (`@derive` lists traits; facts only attach). One skipped member still costs one block per trait (M11, accepted). | Case 1 |
| R6, R7, R10-R12, R16-R20, R4 | `@derive(Error)` problems | **Moved** to `@error` (Error Conversion decisions 10 and 12). Decision 12's payload-parameter decorators are not yet in the grammar, which P11d also relies on (R3-9). | Case 11 |
| R8 | Fact check hook scope | **Remains.** Tags, duplicate tags, oneof sets, and payload-free-only enums need cross-member or type-level checks. | Cases 2, 4 |
| R9 | No type-level information beyond `facts()` | **Remains, and sharper** under A3 (R3-2). | Cases 3, 5, 6, 10 |
| R13 | `Structure`'s names collide | **Remains, wider:** M19 adds `describe`, a common method name (09's own examples declare `fn describe(self)`). 09's `Trait::f(args)` form (`trait.assoc-call.trait`) may already resolve `Structure::build(s)` when the expected type fixes `Self`. | — |
| R14 | The bound names the trait, not the walker's bound | **Remains.** | Case 14 |
| R15 | `default()` allocates | **Remains.** | Case 14 |
| P7 | Declaration-order decode, per-call recomputation | **Order fixed** by M18 (input-driven). **Recomputation remains**, and input-driven lookup makes it `O(n²)` per object (R3-4). | Cases 1, 2, 9 |
| P8 | No requirement row in traversal | **Closed** by M18 (pure). | — |
| P10 | No derived types or methods | **Closed** by M18 (out of scope). | Case 8 |
| P11 | Member model | a: **closed** (decision 4), untyped access (R3-7). b: **closed**, unsound for most traits (R3-5). d: **closed**, needs grammar (R3-9). e, f, g: **closed**. Embedding (M16): holds, with R3-8. | Cases 1, 2, 11, 12 |
| P15 | Foreign types | **Closed** by M18 (mirror). Members of foreign types need a wrapper (case 13). | Case 13 |
| P16 | Templates cannot be extended or composed | **Remains.** A path-tracking wrapper walker would call `inner.member(h, value)` through a generic walker, which M9 rule 2 forbids. | — |
| P19, P20 | Interfaces; wire stability | **Closed** by M18 (carried; documented). | Cases 2, 13 |
| `T -> U` | Mapping between two types | **Open** in M14 and not revisited. | — |

## Comparison With Other Languages

| Need | Rust (serde, schemars) | Go `encoding/json` | Swift `Codable` | Kotlin serialization | Scala 3 `Mirror` | GHC.Generics, aeson | MoonBit | hd M19 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Tell a record from a one-case sum | yes: struct or enum | no sums | yes | descriptor `kind` | `Mirror.ProductOf` or `SumOf` | no: untagged by default | yes | **no** (R3-2) |
| Decode in input order | field visitor `match` | per-type field cache | keyed container | `decodeElementIndex` | library code | object lookup | built in | input-driven; `Source` unwritten (R3-3) |
| Per-type tables | at macro expansion | cached per type | synthesized `CodingKeys` | static descriptor | inline givens | compile time | compiler | **per call** (R3-4) |
| Newtype codec | `#[serde(transparent)]` | named types | `RawRepresentable` | value classes | library code | `newtype deriving`, roles | not stated | through base; `Self` rewrapping unstated (R3-5) |
| Enum as its constant | `serde_repr` | — | raw values | custom serializer | library code | custom | not stated | untyped `v.info` (R3-7) |
| In-place field writes | derive macros (`zeroize`, `merge`) | `reflect.Value.Set` | no | no | no | no | no | **no** (R3-10) |
| Schema matches encoding | schemars reads serde attributes | — | — | one descriptor for both | library code | same `Options` | one derive | per-block lines may drift (R3-6) |

Sources: [serde attributes](https://serde.rs/attributes.html);
[Go `encoding/json`](https://pkg.go.dev/encoding/json) (embedded struct
fields "are usually marshaled as if their inner exported fields were fields
in the outer struct");
[Swift `Codable`](https://developer.apple.com/documentation/swift/encoding-decoding-and-serialization);
[kotlinx `decodeElementIndex`](https://kotlinlang.org/api/kotlinx.serialization/kotlinx-serialization-core/kotlinx.serialization.encoding/-composite-decoder/decode-element-index.html);
[Scala 3 `Mirror`](https://docs.scala-lang.org/scala3/reference/contextual/derivation.html);
[aeson `Options`](https://hackage.haskell.org/package/aeson/docs/Data-Aeson-Types.html#t:Options);
[MoonBit derive](https://docs.moonbitlang.com/en/latest/language/derive.html)
(a closed compiler list: `Eq`, `Compare`, `Debug`, `Default`, `Hash`,
`Arbitrary`, `Shrink`, `ToJson`, `FromJson`, with "coarse-grained"
renaming options). Cells marked "not stated" are features the cited
documentation does not describe.

MoonBit is the nearest neighbor in scope: a closed built-in list with a few
json options, and hand-written impls for everything else. hd's templates
aim wider, which is why per-type tables (R3-4) matter here and not there.

## Problems, Ranked

Rank follows how many cases a problem hits and how silent its failure is.
None of the candidates reopens an alternative the record rejected, except
where a candidate says which new evidence it rests on.

| Rank | Id | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | R3-1 | Generic `h.get` upgrades a `mut` member; walk and build need different `F` | Critical | 7 |
| 2 | R3-2 | Data types walk like one-variant enums; no type name or doc, no variant doc | High | 1, 3, 5, 6, 10, 11 |
| 3 | R3-3 | The `Source` protocol for input-driven `build` is unwritten | High | 1, 2, 4, 5, 7, 8, 9 |
| 4 | R3-4 | Per-type tables are rebuilt on every call | High | 1, 2, 5, 9, 14 |
| 5 | R3-5 | A newtype through its base needs `Self` rewrapping | Medium | 12, 13 |
| 6 | R3-6 | Describing and walking traits drift across tier-2 blocks | Medium | 1, 3 |
| 7 | R3-7 | Shared variant constants are untyped | Medium | 2, 11 |
| 8 | R3-8 | Flattening needs the part type's cooperation | Medium | 1, 3 |
| 9 | R3-9 | Payload members: no facts yet, no positional flag | Medium | 5, 11 |
| 10 | R3-10 | No in-place traversal | Low | 7, 8 |
| 11 | R3-11 | Two-value code can still panic on another variant | Low | 7, 8 |
| 12 | R3-12 | The record is out of date in five places | Low | — |

### R3-1. Generic `get` Upgrades A `mut` Member

**Severity: critical.** A derived `build` can return a value with mutable
access to state its readonly input exposed only readonly.

**Effect.** M17 says `h.get(s)` is typed like `s.field`. Inside a walker or
source, the member type is the parameter `F`, and 04 types a generic field
as "the substituted type, unchanged". `build` instantiates `F` with the
declared type (`mut Cell`), so `h.get(readonly_old)` returns `mut Cell`.
Case 7's `ShareSource` compiles and breaks `data.edge.principle`. Walk has
the mirror problem: the value read through readonly `self` is `Cell`, but
`member[F](h: Field[S, F], value: F)` needs the handle's `F` to match.

**Candidates.**

- **A. Two member types per handle.** Walk and describe pass
  `Field[S, R]` with `R` the read type; build passes the declared type.
  Declaring `Field[-S, +F]` lets a declared handle weaken to its read type,
  since `mut U -> U` preserves representation (04 Variance). `get` on a
  build handle must then require `s: mut S`. *Q:* are two handle views
  acceptable? This reopens round 2's R1-A, on new evidence: M17's rule has
  no typing where `F` is a parameter.
- **B. No derived `build` for types with a `mut` member.** Opting in to a
  template that calls `build` is an error naming the member, unless a
  tier-2 line says `= pass` and the member has a default. Every handle's
  `F` is then the read type, and `get` is sound. *Q:* is losing derived
  decode and clone for such types acceptable?
- **C. Accept the upgrade and document it.** *Q:* is a derived exception
  to `data.edge.principle` acceptable?

**Recommendation:** B. It is the smallest sound rule, the error names the
member, and cloning a type that holds shared mutable state is ambiguous
anyway (share the `Cell` or copy it?).

Prior art: Pony, whose viewpoint adaptation hd's field rule resembles,
types a generic read with an arrow type: reading a field of type `A`
through the receiver gives `this->A`
([Pony arrow types](https://tutorial.ponylang.io/reference-capabilities/arrow-types.html)).
hd has no such type operator, which is why M17's rule cannot type
`h.get(s)` when `F` is a parameter. Rust avoids the question: a derived
`Clone` calls `clone` on each field, and `&` versus `&mut` belongs to the
borrow, not the field.

### R3-2. Data Types Walk Like One-Variant Enums

**Severity: high.** Every tagged format and every schema needs to know
whether a type is a record or a sum, and needs its name.

**Effect.** Under A3, `data Point: x: i64` and `enum Shape: Point(x: i64)`
walk, describe, and build identically (case 11). json cannot choose between
`{"x":1}` and `{"type":"Point","x":1}` (case 1); a source cannot tell
whether to read a tag. Related gaps: no type name for `$defs` or `Debug`
(cases 3, 6), no type doc for CLI `about` and tool descriptions (cases 5,
10), no variant doc for subcommand help (case 5), and no member count to
spot payload-free variants early (case 3).

**Candidates.**

- **A. A kind flag plus docs on `VariantInfo`.** `VariantInfo` gains
  `of_data: bool` and `doc: string?`. For a data type, the one variant's
  `name` and `doc` are the type's. *Q:* is a data type's variant carrying
  the type's name and doc an acceptable way to deliver type-level
  information, closing R9 as well?
- **B. Data types skip `variant`.** Generated `walk`, `describe`, and
  `build` call `variant` only for enums, and `Structure` gains
  `info() -> TypeInfo` (name, doc, kind). *Q:* is a second information
  type worth keeping M19's callback for enums only?
- **C. Keep A3; libraries use facts.** A one-variant enum carries
  `@json.tagged()`. *Q:* is a silent default for one-variant enums
  acceptable?

**Recommendation:** A. It is two fields, keeps M19's uniform protocol, and
answers R9 for names and docs. Scala 3 separates `Mirror.ProductOf` from
`Mirror.SumOf` and provides `MirroredLabel`; Kotlin descriptors carry a
`kind` and `serialName`; aeson shows the cost of not knowing
(`tagSingleConstructors`).

### R3-3. The `Source` Protocol Is Unwritten

**Severity: high.** Every decoder, row mapper, CLI parser, clone, merge,
and generator is a source, and M18 P7 decided only its direction.

**Effect.** Four constraints shape it. Generated code cannot create a
`Self::Error`, so every failure it detects (missing member, duplicate)
must be the source's call. The source must name a member without knowing
its type, so it returns an untyped key and generated code dispatches.
Protobuf needs duplicates merged, JSON needs them rejected, so the source
needs the earlier value (case 2). A key from another variant's `Members`
reaches generated code only through escape (R5) and should panic.

**Candidates.**

- **A. The four-method shape** (`variant`, `next`, `member(h, previous)`,
  `missing`) with compiler-made `Members[S]` and `Key[S]`, as used in
  every case here. *Q:* is this the protocol?
- **B. Three methods.** Drop `missing`; after the input ends, generated
  code calls `member(h, .None)` for each unnamed member, and the source
  tracks what it named. *Q:* is a smaller trait worth duplicate
  bookkeeping in every source?
- **C. A without `previous`.** Generated code keeps the last value of a
  repeated member and never tells the source. *Q:* is losing protobuf's
  merge and JSON's duplicate error acceptable?

**Recommendation:** A. It mirrors Kotlin's `decodeElementIndex` loop, and
every case in this report was written against it without friction from
the protocol itself.

### R3-4. Per-Type Tables Are Rebuilt On Every Call

**Severity: high** for performance; nothing is wrong, only slow.

**Effect.** Everything a template knows at compile time (facts, member
names, handles) is available only at run time, per call. json renames
every key on every encode (`key_for` allocates). The proposed `next` scans
members and renames each on every input key: `O(n²)` allocations per
object (case 1). Protobuf cannot compute a oneof's tag set from `Filter`
and repeats it by hand (case 2). Generators describe the type at every
nesting level (case 9). serde computes keys at macro expansion; Go caches
field lists per type; Kotlin keeps one static descriptor per class.

**Candidates.**

- **A. Compile-time plans.** A template may bind a constant evaluated once
  per opt-in by the annotation evaluator, from `T::facts()` and
  `T::describe` with a pure describer (M18 P8 made traversal pure). *Q:* may
  the compile-time evaluator run library describers, and may a template
  declare such a constant?
- **B. A lazy per-(template, type) cache in std.** `std.structure` builds a
  plan on first use, as superseded decision 10's cache did. *Q:* is
  program-global mutable state in std acceptable here?
- **C. Accept it.** *Q:* is `O(n²)` key matching acceptable for derived
  decoders?

**Recommendation:** A, if the evaluator can run describers; otherwise B.
Both keep templates as ordinary code.

### R3-5. A Newtype Through Its Base Needs `Self` Rewrapping

**Severity: medium.** Every decoding, generating, or shrinking trait puts
`Self` inside a type constructor.

**Effect.** 09's newtype rule forwards a method to the base type's method.
`Eq` and `Hash` have `Self` only as the receiver and a plain parameter.
`Decode` returns `Result[Self, E]`, `Shrink` returns `List[Self]`, and a
trait may take `Map[Self, V]` (case 12). Forwarding needs one rewrapping
per position, which is M7's rejected coercion problem. Rewrapping a map key
is not meaning-preserving when the newtype's `Hash` differs from the base's.

**Candidates.**

- **A. A closed list of positions.** Receiver, plain parameter, plain
  result, `Self?`, and `Result[Self, E]` are forwarded; any other position
  is an error naming the trait method. *Q:* is that list enough?
- **B. Only receiver and plain positions,** as 09 allows for comparison.
  *Q:* is losing derived `Decode` for newtypes acceptable?
- **C. Newtypes get `Structure`** as a one-member data type with a
  `newtype` flag, and libraries encode them transparently by policy. *Q:*
  does this evidence justify revisiting P11b?

**Recommendation:** C. It needs no forwarding rule at all, and json, pb,
and clap already treat a one-member wrapper as transparent by policy
(serde's `transparent`). A is the fallback that keeps P11b.

### R3-6. Describing And Walking Traits Drift Across Blocks

**Severity: medium.** The failure is silent and ships in public schemas.

**Effect.** A tier-2 line applies to its block only (M10). `Encode` with
`total_cents = [json.rename("total")]` beside a derived `Schema` compiles,
and the service's output fails its own schema (case 3). M18 fixed this for
comparison traits by keeping them intrinsic; json's three traits remain.

**Candidates.**

- **A. A lint.** Tier-2 blocks of traits from one package on one type, or a
  tier-2 block beside a `@derive` of a same-package trait, with different
  member lines, get a warning at the second block. *Q:* is a lint enough,
  given that M11 allows per-direction differences on purpose?
- **B. A declared partner.** A trait may name the trait it describes
  (`Schema` describes `Encode`), and a derived describer copies the
  partner's block lines. *Q:* is a new trait-level declaration worth it?

**Recommendation:** A. It is silent-by-default drift that a lint catches,
and M11's per-direction control stays.

### R3-7. Shared Variant Constants Are Untyped

**Severity: medium.** Integer-coded enums (HTTP statuses, protobuf enums,
database codes) are common, and decision 4 made constants the idiom.

**Effect.** Decision 4 moves shared data into `v.info` without a type.
Encoding `HttpStatus.NotFound` as `404` needs a positional downcast, and a
misspelled `@json.repr("cod")` fails at run time (cases 2, 11).

**Candidates.**

- **A. Untyped list, documented.** `VariantInfo.shared: List[Any]` plus
  names, boxed once at compile time. *Q:* is a downcast acceptable?
- **B. Typed constant handles.** `describe` and `walk` pass one
  `Shared[S, F]` handle per constant through a third callback, typed like
  members. *Q:* is a third callback worth it?
- **C. Facts instead.** Libraries ask for `@pb.number(1)` on each variant
  and ignore constants. *Q:* is the duplication acceptable?

**Recommendation:** A. It is the smallest, it is checkable at the opt-in by
the fact check hook once R8 allows type-level checks, and B can come later.

### R3-8. Flattening Needs The Part Type's Cooperation

**Severity: medium.**

**Effect.** M16 makes flattening library policy. To flatten, json's walker
needs `value.encode_members(out)`, a second trait method that every `Encode`
impl must have, with a run-time error for scalars. Decoding buffers every
unclaimed key, and `deny_unknown_fields` becomes impossible for flattened
types (case 1), exactly as in serde. The schema uses `allOf` (case 3).
Separately, M16 says `get` returns "the part, a copy"; 08 says a read
yields the part itself (`data.part.alias`).

**Candidates.**

- **A. Document the pattern** (a members-only method plus a buffered
  `missing`) as the library idiom, and correct M16's "copy" to 08's alias.
  *Q:* is serde's cost acceptable?
- **B. Language flattening for parts.** The walk visits a part's members
  with handles that project through the part. M16 rejected this because
  `build` cannot fill a shadowed member. *Q:* none; listed only to record
  that the evidence does not change M16's reason.

**Recommendation:** A.

### R3-9. Payload Members: No Facts Yet, No Positional Flag

**Severity: medium.**

**Effect.** P11d says declaration facts on payload parameters apply, but
`Logout(user: i64, @json.rename("why") reason: string)` is a
`syntax-error` today; Error Conversion decision 12 decided decorators on
payload parameters for `@from` and `@source` but the grammar has not been
updated. Unnamed payloads (`Num(f64)`) need a member name and a flag so
json can write serde's `{"Num": 1.5}` (case 11). clap's per-variant
`--force` needs the same facts (case 5).

**Candidates.**

- **A. Ride on decision 12.** Payload-parameter decorators produce member
  facts; an unnamed payload's `Member.name` is `_0`, `_1`, ... (Error
  Conversion gap 3), and `Member` gains `positional: bool`. *Q:* agreed?
- **B. Payload data types.** Facts go on a data type the variant carries.
  *Q:* is `Logout(LogoutInfo)` everywhere acceptable?

**Recommendation:** A.

### R3-10. No In-Place Traversal

**Severity: low.**

**Effect.** `zeroize`, in-place normalization, and the `merge` crate's
`fn merge(&mut self, other)` write through places. hd's walk hands out
values and `build` makes new ones; `mut self` cannot be replaced (cases 7,
8).

**Candidates.**

- **A. Accept it.** Hand-write in-place traits; merge returns a new value.
  *Q:* agreed?
- **B. A `set` on handles.** `h.set(s: mut S, value: F)` for ordinary
  members, with parts stored by copy. *Q:* is a write capability on escaping
  handles (R5) acceptable?

**Recommendation:** A. Zeroizing is also weak under a garbage collector
that may copy objects, so its main use case is doubtful in hd.

### R3-11. Two-Value Code Can Still Panic On Another Variant

**Severity: low.**

**Effect.** M19 removed the panics from one-value walks. Diff, merge, and
patch still call `h.get(other)`, which panics when `other` holds another
variant; with `type Error = never` a walker cannot stop at the mismatch
(cases 7, 8).

**Candidates.**

- **A. Document the `holds` check** in the walker guide, with diff as the
  example. *Q:* enough?
- **B. `try_get(s) -> F?`.** *Q:* is an allocation per read acceptable?

**Recommendation:** A.

### R3-12. The Record Is Out Of Date In Five Places

**Severity: low.** Readers and future agents take the record literally.

**Effect.**

1. The status line says the design is deferred and that "M1-M16 below are
   kept"; M17-M19 were decided after it.
2. The Current Design example and Current Rules still show M14.
3. M18's P11a text says shared fields are "visited first with handles
   marked `shared`"; Enum Semantics decision 4 replaced that.
4. M16's "`get` returns the part, a copy" contradicts `data.part.alias`.
5. `Structure`'s signatures under M19 (does `walk` take `self`? is
   `describe` receiverless?) are not written. Also, 08's Shared Fields
   still describes per-value, assignable shared data.

**Candidate.** *Q:* should the record be updated in one pass, with
Structure's M19 signatures as this report's surface assumes (A1)?

**Recommendation:** yes.

## Questions For The Owner

Smallest first. Each has the labeled recommendation from its problem.

1. **R3-12.** Update the record's status line, example, P11a wording, M16
   wording, and M19 `Structure` signatures in one pass? Recommendation: yes.
2. **R3-11.** Keep `get` panicking and document the `holds` check for
   two-value code? Recommendation: yes (A).
3. **R3-9.** Payload-parameter facts ride on Error Conversion decision 12's
   grammar; unnamed payloads are named `_0` and flagged `positional`?
   Recommendation: yes (A).
4. **R3-2.** Add `of_data` and `doc` to `VariantInfo`, with a data type's
   variant carrying the type's name and doc? Recommendation: yes (A).
5. **R3-7.** Expose shared constants as an untyped `List[Any]` with names?
   Recommendation: yes (A).
6. **R3-6.** Lint differing member lines across same-package blocks?
   Recommendation: yes (A).
7. **R3-10.** Leave in-place traversal out? Recommendation: yes (A).
8. **R3-8.** Document flattening as library code and fix M16's "copy"?
   Recommendation: yes (A).
9. **R3-1.** Forbid derived `build` for types with a `mut` member, making
   every handle's `F` the read type? Recommendation: yes (B).
10. **R3-5.** Give newtypes `Structure` as one-member data types instead of
    deriving through the base? Recommendation: C, fallback A.
11. **R3-3.** Adopt the four-method `Source` (`variant`, `next`,
    `member(h, previous)`, `missing`)? Recommendation: yes (A).
12. **R3-4.** Allow compile-time per-opt-in plans built with pure
    describers? Recommendation: A, fallback B.

## Parse Log

Every `text` block above was checked with the reference parser
(`parseSource` in `spec/reference-parser/parser.ts`) on 2026-09-27. Parsing
checks syntax only: names such as `Field`, `Members`, `json.Writer`, and
the library helpers are not resolved, and nothing was type-checked.

| Blocks | Where | Result |
| --- | --- | --- |
| 1-3 | Surface: `std.structure`, the proposed `Source`, generated `build` for `Post` | Parse. |
| 4-6 | Case 1: `Post`, `Encode` and `Decode` with templates, `Encoder`, `FieldSource` | Parse. |
| 7 | Case 1: `Session`'s two tier-2 blocks | `syntax-error` at line 2, the first M3 member line (`cache = pass`), as expected. Marked `# hypothetical syntax`. |
| 8-10 | Case 2: `SearchRequest`, `WireSource`, `EnumNumber` | Parse. |
| 11 | Case 3: `Schema` template and `SchemaDescriber` | Parse. |
| 12 | Case 3: `Invoice` drift example | `syntax-error` at line 8, the member line `total_cents = [...]`, as expected. Marked `# hypothetical syntax`. |
| 13-15 | Case 4: `UserRow`, `RowSource`, the rejected `Bad` enum | Parse. |
| 16-17 | Case 5: `Cli`, `Command`, `ArgSource` | Parse. |
| 18 | Case 6: `Session` and `DebugWalker` | Parse. |
| 19-22 | Case 7: `Clone`, the `ShareSource` counterexample, `DiffWalker`, `Zeroize` | Parse. |
| 23-24 | Case 8: `LayerSource`, `MergeSource` | Parse. |
| 25 | Case 9: `Arbitrary` and `VariantSizes` | Parse. |
| 26 | Case 10: `SearchArgs` and `Order` | Parse. |
| 27-28 | Case 11: `HttpStatus`, `Token` | Parse. |
| 29 | Case 11: a decorator on a payload parameter | `syntax-error` at line 2, the decorated parameter. Marked `# hypothetical syntax` (R3-9). |
| 30 | Case 11: `Point` beside `Shape.Point` | Parse. |
| 31-32 | Case 12: `Page[T]`, `Json`, `UserId` and its forwarded impls | Parse. |
| 33-34 | Case 13: `Place` with `geo.Point`, the `JsonPoint` workaround | Parse. |
| 35 | Case 14: generated `walk` for `Post` and `Event` | Parse. |

Reference-parser findings from this round:

- Decorators on payload parameters (`Logout(user: i64, @json.rename("why")
  reason: string)`, and Error Conversion decision 12's `Io(@from FsError)`)
  are a `syntax-error`. Decision 12 needs a grammar change, and P11d relies
  on it (R3-9).
- A doc comment directly before a `pub` field is still reported as
  `doc-comment-without-target` (round 2's finding). Cases 5 and 10 write
  documented fields without `pub`.
- Doc comments before variants, and decorators before variants
  (`@pb.tag(5)`), parse.
