# Enum Tagging Facts

Status: implemented 2026-10-29 in the
[`std.serde` specification](../../spec/std/serde.md#enum-names-and-tagging)
and [JSON tagged forms](../../spec/std/json.md#tagged-enum-forms).

This note addresses [#954](https://github.com/HerringtonDarkholme/hd-lang/issues/954).
It reviews the current [serde facts](../../spec/std/serde.md#member-names),
[fact model](../../spec/lang/14-annotations.md#facts), and the variant forms in
[#953](https://github.com/HerringtonDarkholme/hd-lang/issues/953).
This record preserves the accepted design and its prior-art review; the
specification links above are normative.

## Proposal

Add one enum-level `EnumTagging` fact with `Internal`, `Adjacent`, and `Untagged` values.
The `tag` and `untagged` constructors follow Serde's public spelling while keeping one fact type underneath.
No fact keeps the external representation.
JSON follows Serde's shapes, limits, and declaration-order matching.

```text
use std.serde.{Deserialize, Serialize, rename_all, tag}

@derive(Serialize, Deserialize)
@rename_all(.SnakeCase)
@tag("type")
enum Shape:
    Rect(width: i32, height: i32)
    Empty

# Shape.Rect(width=2, height=3) is
# {"type":"rect","width":2,"height":3}
```

The specification and conformance fixtures carry the public signatures and
examples that the former checked design package demonstrated.

## Problems it solves

| Case | External form today | Proposed form |
| --- | --- | --- |
| Event stream | `{"Rect":{"width":2,"height":3}}` | internal: `{"type":"Rect","width":2,"height":3}` |
| Pandoc-style node | `{"Para":["one","two"]}` | adjacent: `{"t":"Para","c":["one","two"]}` |
| Number-or-name field | `{"Number":443}` | untagged: `443` |

External tagging remains the safest default.
It identifies the variant before reading its payload and works without retrying a decode.

## How it works

### Public API

```text
use std.annotation.annotate

@derive(Debug, Eq)
@annotate(.Enum)
pub enum EnumTagging:
    Internal(tag: string)
    Adjacent(tag: string, content: string)
    Untagged

pub fn tag(tag: string, content: string? = .None) -> EnumTagging: panic("todo")
pub fn untagged() -> EnumTagging: panic("todo")
```

`EnumTagging` is one concrete fact type.
All three spellings produce that fact, so a second representation decorator uses the existing
`duplicate-fact` error on the later decorator.
The fact belongs on the enum declaration, so `Serialize` and `Deserialize` see the same choice.

| Form | Serde | Proposed hd | Rejected separate name |
| --- | --- | --- | --- |
| Internal | `#[serde(tag = "type")]` | `@tag("type")` | `@internal(tag="type")` |
| Adjacent | `#[serde(tag = "t", content = "c")]` | `@tag("t", content="c")` | `@adjacent(tag="t", content="c")` |
| Untagged | `#[serde(untagged)]` | `@untagged` | none |

The proposed names retain Serde's `tag`, `content`, and `untagged` vocabulary.
Hd's decorator syntax and one typed fact account for the remaining spelling differences.

### Wire semantics

| Fact | Unit | Newtype | Tuple | Record |
| --- | --- | --- | --- | --- |
| none | `"Empty"` | `{"Value":7}` | `{"Pair":[1,2]}` | `{"Rect":{"w":2}}` |
| `tag("type")` | `{"type":"Empty"}` | merge a struct or map payload beside `type` | checker error | `{"type":"Rect","w":2}` |
| `tag("t", content="c")` | `{"t":"Empty"}` | `{"t":"Value","c":7}` | `{"t":"Pair","c":[1,2]}` | `{"t":"Rect","c":{"w":2}}` |
| `untagged` | `null` | `7` | `[1,2]` | `{"w":2}` |

Tag values use the variant's serde wire name: explicit variant `rename`, then enum `rename_all`, then `VariantInfo.name`.
Today `rename` cannot target variants and enum `rename_all` changes payload members, not variant names.
Variant renaming is therefore phase 0 and must land before any tagged representation.
That phase extends `Rename` to `.Variant` and adds a `variant_name(&VariantInfo, Facts)` helper with this precedence.
Payload members keep their existing `rename` and `rename_all` behavior.
An internal newtype merges only a struct or `Map[string, V]` payload into its object.
A mixed named-and-positional hd variant has record form under #953, so internal tagging accepts it.

Untagged serialization writes the selected variant's payload shape.
Untagged decoding retries complete variants in declaration order and returns the first success.
A failed attempt must restore the reader completely before the next attempt.

### Checker diagnostics

| Invalid declaration | Code and span | Message and repair |
| --- | --- | --- |
| Internal tuple variant | `invalid-internal-tagged-variant` on the variant | `` `Point` has multiple positional members; internal tagging cannot represent it ``; use `tag("t", content="c")` or external tagging |
| Internal newtype over a scalar, list, tuple, or unknown generic | `invalid-internal-tagged-variant` on the payload type | `` `Text` wraps `string`; an internally tagged newtype must wrap a struct or Map ``; change the payload or representation |
| Two representation facts | existing `duplicate-fact` on the later fact | remove one representation |
| Internal tag equals a known member wire name | `enum-tag-name-conflict` on the member | choose a distinct literal tag or member rename |
| Adjacent tag equals content | `enum-tag-name-conflict` on `content` | use two distinct literal names |
| Internal form has a computed member rename | `enum-tag-name-not-static` on that rename | write a literal rename or choose another representation |

The checker validates every variant when `tag(...)` selects internal form, not only a reached variant.
A generic payload is rejected unless its declared type proves that it is a struct or map.
That is stricter than Serde's runtime failure and gives hd's requested checker error.
The `tag` and `content` arguments are static string literals.
The checker rejects equal adjacent names and every known internal tag/member wire-name collision.

### Decode path

Internal and adjacent readers can choose a variant from their tag before `Structure::build` reads members.
Their writers use the existing variant and member events, with a different JSON frame.

Untagged reading exposes one implementation gap.
`Structure::build` currently chooses one variant once, while `Deserializer` cannot checkpoint and rewind.
The smallest extension is an optional attempt protocol on `Deserializer` used only by untagged derives.
JSON's tree reader checkpoints its current value; its text reader checkpoints validated-input offsets and frames.

The derive template asks `build` to use one candidate, rewinds on `.Err`, and retries the next candidate.
No attempt may leak member positions, path frames, or a partial value into the next attempt.
When every candidate fails, decoding returns one `WrongType` at the enum's original input path.
Candidate errors are not selected or aggregated.
Fact-less and externally tagged types keep their current path and must not link retry support.

## Cost

| Component | Expected cost and gate |
| --- | --- |
| Code removed | None |
| Code added | One optional-content `tag`, `untagged`, variant wire names, checker validation, JSON frame modes, and retry support |
| Compile time | One enum-shape validation; measure a 100-variant enum before implementation merges |
| Runtime | Internal and adjacent are one pass; untagged is worst-case variants times payload decoding |
| Wasm size | Measure external-only, each single form, and all forms; external-only must stay byte-identical |
| Web compiler gzip | Measure the same matrix against the 2 MB cap; no fact may pull in unused readers |

The design PR adds only documentation and a checked package, so it has no implementation measurements.

## Appendix A: Stress test

### Rust-analyzer progress events

Rust-analyzer documents an internally tagged stream for project discovery.
The hd form keeps its flat JSONL record and has only named variants.

```text
use std.serde.{Deserialize, Serialize, rename_all, tag}

@derive(Serialize, Deserialize)
@rename_all(.SnakeCase)
@tag("kind")
enum DiscoverProjectData:
    Finished(buildfile: string, project: string)
    Error(error: string, source: string?)
    Progress(message: string)

# Progress(message="generating") is
# {"kind":"progress","message":"generating"}
```

This passes the internal-form limits.
It follows rust-analyzer's documented
[`DiscoverProjectData`](https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/config.rs).
Rust-analyzer renames variants to snake case.
Current hd `rename_all` does not rename variants, so phase 0 supplies that prerequisite before this example can match its wire format.

### Pandoc blocks

Pandoc's JSON AST uses adjacent `t` and `c` members for nodes.
Unit nodes omit `c`, while payload nodes keep arrays or objects under it.

```text
use std.serde.{Deserialize, Serialize, tag}

@derive(Serialize, Deserialize)
@tag("t", content="c")
enum Inline:
    Str(string)
    Space
    Emph(List[Inline])

# Space is {"t":"Space"}; Str("hi") is {"t":"Str","c":"hi"}
```

The recursive list checks that adjacent content preserves a newtype payload.
Pandoc publishes this shape through its [JSON filter interface](https://pandoc.org/filters.html).

### OpenAI Codex image references

Codex accepts either an inline image URL or a file id as one untagged object.
The two distinct required keys make the ordinary inputs unambiguous.

```text
use std.serde.{Deserialize, Serialize, untagged}

@derive(Serialize, Deserialize)
@untagged
enum ImageReference:
    Inline(image_url: string)
    File(file_id: string)

# Inline(image_url="data:...") is {"image_url":"data:..."}
```

This mirrors Codex's
[`ImageReference`](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/models.rs).
If an object contains both keys and unknown keys are ignored, the first variant wins.

### Adversarial cases

| Case | Result |
| --- | --- |
| `@tag("kind")` on `Pair(i32, i32)` | Checker error on `Pair` before code generation |
| `@tag("kind")` on `Text(string)` | Checker error on `string`; a runtime-only failure is rejected |
| Two untagged unit variants reading `null` | The first declaration always wins |
| Broad untagged record before a narrow record | The broad record may accept and discard distinguishing unknown members |
| No untagged variant accepts input | One `WrongType` at the original enum path |
| Internal tag equals a renamed member key | Ambiguous object; the accepted static-name rule rejects it |
| Internal map has the tag key at run time | Write both entries, tag first; reading the repeated tag returns `DuplicateField` |
| Handwritten newtype serializer writes a non-object | `explicit-panic` names the variant and requires an object payload |

Variant ordering and the dynamic collision behavior are observable API.

## Appendix B: Prior art

### Rust Serde

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum Message {
    Request { id: String },
    Response { result: Value },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
enum Block { Para(Vec<Inline>), Str(String) }
```

Serde defines external, internal, adjacent, and untagged representations.
Its [enum guide](https://serde.rs/enum-representations.html) defines their JSON and variant limits.
Hd takes those shapes, declaration-order rule, and public spellings:
`@tag("type")`, `@tag("t", content="c")`, and `@untagged`.
One `EnumTagging` fact remains underneath, so spelling parity adds no conflict mechanism.

Serde rejects internal tuple variants while deriving.
Hd rejects a declared non-object newtype payload in the checker. A declared struct
with a handwritten serializer can still write a non-object, so that path retains
Serde's run-time failure as an `explicit-panic`.
That difference removes a reached-variant runtime failure.

### Serde conflict checks

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
enum Event {
    Changed { kind: String }, // tag collision
}
```

Serde rejects a known internal tag/member collision and equal adjacent tag/content names.
The checks are in
[`serde_derive::internals::check`](https://github.com/serde-rs/serde/blob/master/serde_derive/src/internals/check.rs).
Hd therefore makes these fact arguments the same kind of checked literal as a test snapshot name.
An internal enum also rejects a computed member rename whose collision cannot be decided.

### Untagged matching costs

```rust
#[derive(Deserialize)]
#[serde(untagged)]
enum Port {
    Number(u16),
    Service(String),
}
```

Serde buffers and retries, then reports a generic no-match error.
Its [container guide](https://serde.rs/container-attrs.html) warns about weak errors and performance.
Hd takes ordered retry but keeps retry code out of types without `untagged`.

## Appendix C: Alternatives considered

### Three fact types

`InternalTag`, `AdjacentTag`, and `Untagged` let the checker identify internal form by fact type.
They also permit contradictory facts unless the checker adds a second conflict system.
One enum fact reuses `duplicate-fact` and is the accepted design.

### Separate `internal` and `adjacent` constructors

`@internal(tag="type")` and `@adjacent(tag="t", content="c")` state the shape directly.
They diverge from the established Serde spelling without an anti case and export the common identifier `internal`.
The proposal instead uses one `tag` constructor with optional `content`, while diagnostics still name the selected form.

### Per-variant fallback facts

Serde supports advanced variant-level exceptions in some configurations.
They make order and collision rules harder to explain, so this proposal keeps one form per enum.

### Handwritten `Deserialize`

A handwritten implementation already can inspect JSON shapes.
It repeats member names, defaults, and nested paths, which defeats the derivation use case.

## Appendix D: Today in hd

`std.serde` has `Rename`, `RenameAll`, `Skip`, and `SkipNone` facts.
`Facts` already reaches each serializer and deserializer variant call.
The default enum form is external, with the unit, newtype, tuple, and record payload shapes that #953 shipped.

`Rename` targets fields only.
An enum's `RenameAll` currently changes its payload member names but leaves `VariantInfo.name` unchanged.
Tagged forms therefore wait for phase 0 variant wire names instead of emitting source variant names accidentally.

The current `Source` protocol selects one variant before reading members.
It has no replay operation, so untagged decoding is not implementable as ordinary std code today.
The design therefore names retry support as an implementation prerequisite, not accepted behavior.

## Appendix E: Details and phases

- **Grammar and formatting:** no syntax changes; these are ordinary fact decorators.
- **Typing:** the fact targets enums, and the checker validates internal variant shapes at the opt-in.
- **Patterns and inference:** unchanged; tagging affects only serde wire behavior.
- **Modules:** the public fact type and constructors live in `std.serde`.
- **Ownership:** readers retry with local `&mut` state; no reference escapes an attempt.
- **Soundness:** a rejected attempt constructs no value visible to the caller.
- **Diagnostics:** add `invalid-internal-tagged-variant`; reuse `duplicate-fact`.
- **Specification:** add stdlib serde rules, language diagnostic registration, and conformance fixtures together.
- **No-feature alternative:** keep external tagging or write both serde implementations by hand.

Implementation phases:

0. Add explicit and `rename_all` variant wire names; test precedence and snake-case tagged streams.
1. Add `tag`, `untagged`, checker diagnostics, and invalid/valid declaration fixtures.
2. Add internal and adjacent JSON frames with one round-trip fixture per variant form.
3. Run external-only and tagged-form size gates, adversarial paths, web compiler tests, and documentation updates.
4. Add retryable deserializer attempts and untagged order/no-match fixtures last; measure its isolated Wasm and web-compiler gzip delta before merge.

## Parse log

Every `text` block in this note parsed with `scripts/hd-conformance.sh parse` on 2026-10-28.
That design-time parse covered syntax only; the implemented API and behavior
are now checked by the specification's conformance fixtures.
