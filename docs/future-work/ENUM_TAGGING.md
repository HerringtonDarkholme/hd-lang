# Enum Tagging Facts

Status: proposal, 2026-10-28; domain: `std.serde` enum representation; not accepted.

This note addresses [#954](https://github.com/HerringtonDarkholme/hd-lang/issues/954).
It reviews the current [serde facts](../../spec/std/serde.md#member-names),
[fact model](../../spec/lang/14-annotations.md#facts), and the variant forms in
[#953](https://github.com/HerringtonDarkholme/hd-lang/issues/953).
Nothing here changes the specification or implementation.

## Proposal

Add one enum-level `EnumTagging` fact with `Internal`, `Adjacent`, and `Untagged` values.
The `internal`, `adjacent`, and `untagged` constructors make the common spelling short.
No fact keeps the external representation.
JSON follows Serde's shapes, limits, and declaration-order matching.

```text
use std.serde.{Deserialize, Serialize, internal}

@derive(Serialize, Deserialize)
@internal(tag="type")
enum Shape:
    Rect(width: i32, height: i32)
    Empty

# Shape.Rect(width=2, height=3) is
# {"type":"Rect","width":2,"height":3}
```

The checked [signatures and examples](enum-tagging/) show every constructor and JSON form.

## Open questions

1. **Conflict handling. Recommendation: one fact type.** Then `@internal(...) @untagged()` is
   `duplicate-fact`; three fact types would need a new cross-type conflict error.
2. **No-match diagnostics. Recommendation: one `WrongType`.** `decode::[Port]("{}")` should report
   the enum path, rather than one candidate's error or an unstable aggregate.
3. **Collision safety. Recommendation: static names.** Reject `@internal(tag=tag_name())` and a
   computed member rename, rather than permit a runtime duplicate key.

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

pub fn internal(tag: string) -> EnumTagging: panic("todo")
pub fn adjacent(tag: string, content: string) -> EnumTagging: panic("todo")
pub fn untagged() -> EnumTagging: panic("todo")
```

`EnumTagging` is one concrete fact type.
Two representation decorators on one enum therefore use the existing `duplicate-fact` error.
The fact belongs on the enum declaration, so `Serialize` and `Deserialize` see the same choice.

### Wire semantics

| Fact | Unit | Newtype | Tuple | Record |
| --- | --- | --- | --- | --- |
| none | `"Empty"` | `{"Value":7}` | `{"Pair":[1,2]}` | `{"Rect":{"w":2}}` |
| `internal(tag="type")` | `{"type":"Empty"}` | merge a struct or map payload beside `type` | checker error | `{"type":"Rect","w":2}` |
| `adjacent(tag="t", content="c")` | `{"t":"Empty"}` | `{"t":"Value","c":7}` | `{"t":"Pair","c":[1,2]}` | `{"t":"Rect","c":{"w":2}}` |
| `untagged()` | `null` | `7` | `[1,2]` | `{"w":2}` |

Tags keep `VariantInfo.name` in this scope; payload members still use `rename` and `rename_all`.
An internal newtype merges only a struct or `Map[string, V]` payload into its object.
A mixed named-and-positional hd variant has record form under #953, so internal tagging accepts it.

Untagged serialization writes the selected variant's payload shape.
Untagged decoding retries complete variants in declaration order and returns the first success.
A failed attempt must restore the reader completely before the next attempt.

### Checker diagnostics

| Invalid declaration | Code and span | Message and repair |
| --- | --- | --- |
| Internal tuple variant | `invalid-internal-tagged-variant` on the variant | `` `Point` has multiple positional members; internal tagging cannot represent it ``; use `adjacent` or external tagging |
| Internal newtype over a scalar, list, tuple, or unknown generic | `invalid-internal-tagged-variant` on the payload type | `` `Text` wraps `string`; an internally tagged newtype must wrap a struct or Map ``; change the payload or representation |
| Two representation facts | existing `duplicate-fact` on the later fact | remove one representation |
| Internal tag equals a known member wire name | `enum-tag-name-conflict` on the member | choose a distinct literal tag or member rename |
| Adjacent tag equals content | `enum-tag-name-conflict` on `content` | use two distinct literal names |
| Internal form has a computed member rename | `enum-tag-name-not-static` on that rename | write a literal rename or choose another representation |

The checker validates every variant when `internal(...)` is attached, not only a reached variant.
A generic payload is rejected unless its declared type proves that it is a struct or map.
That is stricter than Serde's runtime failure and gives hd's requested checker error.
Literal tag names also make every statically known member collision a checker error.

### Decode path

Internal and adjacent readers can choose a variant from their tag before `Structure::build` reads members.
Their writers use the existing variant and member events, with a different JSON frame.

Untagged reading exposes one implementation gap.
`Structure::build` currently chooses one variant once, while `Deserializer` cannot checkpoint and rewind.
The smallest extension is an optional attempt protocol on `Deserializer` used only by untagged derives.
JSON's tree reader checkpoints its current value; its text reader checkpoints validated-input offsets and frames.

The derive template asks `build` to use one candidate, rewinds on `.Err`, and retries the next candidate.
No attempt may leak member positions, path frames, or a partial value into the next attempt.
Fact-less and externally tagged types keep their current path and must not link retry support.

## Cost

| Component | Expected cost and gate |
| --- | --- |
| Code removed | None |
| Code added | Three fact constructors, checker validation, JSON frame modes, and untagged retry support |
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
use std.serde.{Deserialize, Serialize, internal}

@derive(Serialize, Deserialize)
@internal(tag="kind")
enum DiscoverProjectData:
    Finished(buildfile: string, project: string)
    Error(error: string, source: string?)
    Progress(message: string)

# Progress(message="generating") is
# {"kind":"Progress","message":"generating"}
```

This passes the internal-form limits.
It follows rust-analyzer's documented
[`DiscoverProjectData`](https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/config.rs).
Rust-analyzer separately renames variants to snake case; hd variant renaming is outside #954.

### Pandoc blocks

Pandoc's JSON AST uses adjacent `t` and `c` members for nodes.
Unit nodes omit `c`, while payload nodes keep arrays or objects under it.

```text
use std.serde.{Deserialize, Serialize, adjacent}

@derive(Serialize, Deserialize)
@adjacent(tag="t", content="c")
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
@untagged()
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
| `@internal` on `Pair(i32, i32)` | Checker error on `Pair` before code generation |
| `@internal` on `Text(string)` | Checker error on `string`; a runtime-only failure is rejected |
| Two untagged unit variants reading `null` | The first declaration always wins |
| Broad untagged record before a narrow record | The broad record may accept and discard distinguishing unknown members |
| No untagged variant accepts input | One no-match error at the original enum path |
| Internal tag equals a renamed member key | Ambiguous object; static naming in recommendation 3 rejects it |

The last two rows are the hard cases.
Variant ordering is observable API, and internal key collision needs a rule before implementation.

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
Hd takes those shapes and the declaration-order rule.

Serde rejects internal tuple variants while deriving.
Its newtype payload restriction may still fail during serialization; hd rejects it in the checker.
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
Hd takes ordered retry but keeps retry code out of types without `untagged()`.

## Appendix C: Alternatives considered

### Three fact types

`InternalTag`, `AdjacentTag`, and `Untagged` let the checker identify internal form by fact type.
They also permit contradictory facts unless the checker adds a second conflict system.
One enum fact reuses `duplicate-fact` and is the recommendation.

### One `tag` fact with optional `content`

This copies Serde's attribute spelling closely.
It makes an absent optional value select internal form, which is less explicit in generated docs and diagnostics.

### Per-variant fallback facts

Serde supports advanced variant-level exceptions in some configurations.
They make order and collision rules harder to explain, so this proposal keeps one form per enum.

### Handwritten `Deserialize`

A handwritten implementation already can inspect JSON shapes.
It repeats member names, defaults, and nested paths, which defeats the derivation use case.

## Appendix D: Today in hd

`std.serde` has `Rename`, `RenameAll`, `Skip`, and `SkipNone` facts.
`Facts` already reaches each serializer and deserializer variant call.
The default enum form is external, with #953 proposing unit, newtype, tuple, and record payload shapes.

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

Implementation phases, after approval:

1. Add the facts, checker diagnostics, and invalid/valid declaration fixtures.
2. Add internal and adjacent JSON frames with one round-trip fixture per variant form.
3. Add retryable deserializer attempts and untagged order/no-match fixtures.
4. Run external-only size gates, adversarial paths, web compiler tests, and documentation updates.

## Parse log

Every `text` block in this note parsed with `scripts/hd-conformance.sh parse` on 2026-10-28.
Parsing checks syntax only; the proposal is not implemented and the blocks do not type-check against current std.
