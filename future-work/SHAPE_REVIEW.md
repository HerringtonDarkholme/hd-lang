# Shape Review: Are `shape` And `shape_of` Still Needed?

Status: research record, 2026-10-01. Nothing here is accepted behavior,
and it changes no decision, spec text, fixture, or prototype code. It
answers the owner's request: "i don't think we should include
shape/shape_of. I didn't approve/vet that design ... Please add a research
to see if shape/shape_of should still be needed, are they covered by
Structure."

Under review:
- [Common Shape Representation](../spec/14-annotations.md#common-shape-representation)
  and [Shape Intrinsics](../spec/14-annotations.md#shape-intrinsics);
- [`annot.decorator.fn-read`](../spec/14-annotations.md#r-annot.decorator.fn-read)
  and the Note under [Target Kinds](../spec/14-annotations.md#target-kinds);
- [Shape Descriptors](../spec/04-type-system.md#shape-descriptors) and
  [`types.assign.shape`](../spec/04-type-system.md#r-types.assign.shape);
- [`grammar.primary.shape-intrinsics`](../spec/02-grammar.md#r-grammar.primary.shape-intrinsics);
- [`module.prelude.shape`](../spec/10-modules.md#r-module.prelude.shape)
  and the `std.annotation` row of the [Prelude](../spec/10-modules.md#prelude);
- the alternative it is measured against:
  [Typed Derivation](../spec/14-annotations.md#typed-derivation), its
  [Facts](../spec/14-annotations.md#facts) and
  [Handles](../spec/14-annotations.md#handles).

## Contents

- [Summary](#summary)
- [1. Provenance](#1-provenance)
- [2. Inventory](#2-inventory)
- [3. Uses](#3-uses)
- [4. Overlap With Structure](#4-overlap-with-structure)
- [5. Options](#5-options)
- [6. Recommendation](#6-recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Parse Log](#parse-log)

## Summary

| Question | Finding |
| --- | --- |
| Was the shape design approved? | No owner decision approved the shape types or `shape::[T]()`. Four later decisions assumed they existed; only one, Decorators D7, gave them a job. |
| What uses it? | Fixtures, one std test file, and guide examples. No `lib/std` code, no std design, no tool, and no compiler rule calls `shape` or `shape_of`. |
| What does Structure cover? | Every data and enum read: member names, docs, positions, facts, defaults, and typed values. It does not cover functions. |
| Is a function fact read needed? | The compiler reads its markers by qualified name, and tools read the package interface. Only user code that registers routes or tools by decorator needs it. Tool registration is by hand for now ([FN_TYPE decision 10](archive/FN_TYPE.md#owner-decisions)). |
| Recommendation | Option A, remove all of it: 10 numbered rules, about 215 lines of unnumbered chapter 14 text, 14 prelude names, one diagnostic code, and about 600 lines of prototype TypeScript. |

The shape surface is also the least reviewed text in chapter 14. Its 215
lines hold no rule ID. The prose promises a "declaration kind" on every
shape, but no shape type has a field of type `DeclarationKind`.

## 1. Provenance

### Timeline

| Date | Commit | What entered | Owner decision? |
| --- | --- | --- | --- |
| 2026-07-19 | `a59ee4e2` "update" | SYNTAX_NOTES "Provisional shape vocabulary": `shape(User)` is a `StructShape`, `shape(get_user)` a `FnShape`, fed to facet `map_field` and `finish` hooks. | No. Labelled provisional, and DESIGN_QUESTIONS listed the representation as an open question. |
| 2026-07 to 09-21 | "update" commits | DESIGN_QUESTIONS annotation decisions 12, 19, 27 and 34 name "declaration shapes" as the carrier of facet metadata. | The facet protocol was decided, with shapes as its input. The representation question stayed open. |
| 2026-09-22 | `89e8e616` "add spec", `b73509c9` "update spec" | The spec chapter states the full type set as "the normative shape surface". DESIGN_QUESTIONS drops open question 1 and lists "final shape APIs" as an open issue of the chapter. | No answer to question 1 is recorded. |
| 2026-09-24 | `4cc13122` "update" | `ShapeMetadata` and `unknown-shape-target`. | None found. |
| 2026-09-26 | `fb62b467` K1 | The `shape` keyword becomes `shape[T]()` and `shape_of(f)`. The same commit adds the specialized `fields` and `variants` records and the `shape_of` argument rules. | K1 decided the spelling. Its question text is not in the repo; audit/grammar/QUESTIONS.md says only "decided separately (K1 to K3)". |
| 2026-09-26 | `fae0c928` TQ-23 | `TypeShape` gains `Mut`, `Trait`, `Any`, `Suspend`, and `Newtype(decl, base)`. | Yes, to fill a coverage gap. It assumes shapes exist. |
| 2026-09-26 | Typed derivation decisions 1 and 7 | Design G (`Structure`) is chosen over Design C, "runtime shape-driven codecs". Decision 7 folds facets into derivation and lists "Kept: shapes, member metadata, ...". | Shapes kept by listing, while their original consumer moved to `Structure`. |
| 2026-09-27 | Typed derivation decision 10 | The facet protocol, the shapes' first consumer, is removed. | Yes; shapes stay with no consumer named. |
| 2026-09-28 | `fa52b0f0` Decorators D7 | Function decorator values become `FnShape` metadata, read through `shape_of(f).metadata[M]()`. | Yes, the one decision that gives shapes a job. |
| 2026-09-28 | `5964cad1` | Prototype implements `shape` and `shape_of`. | Not a decision. |

Commits before 2026-09-25 are owner-committed "update" commits with no
`Co-Authored-By` line. The repo cannot show who wrote their text.

### Owner Decisions That Mention Shapes

Decorators D7, recorded 2026-09-28 in `future-work/DECORATORS.md` (deleted
in `3a873f54`; see commit `fa52b0f0`):

> **D7: user code can read a function's decorators** through
> `shape_of(f).metadata[M]()`. It is one lookup by type on a known
> function. No listing of all decorated items exists, so there is no
> general reflection.

The record proposed it as "one small extension" of a `shape_of` that
already existed. The same record's owner direction D5 said "No full
compile-time reflection, and few compiler special cases".

Typed derivation decision 7 (2026-09-26, `future-work/archive/TYPED_DERIVATION.md`
at `e6549dcc`) ends: "Kept: shapes, member metadata,
`Annotation`/`Annotate`/`Info`, ...". Decision 10 then removed `Annotation`
and `Annotate` from that list.

TQ-23 (`audit/types/QUESTIONS.md` at `fae0c928`): "`TypeShape` gains
`Mut(inner)`, `Trait(decl, args)`, `Any`, `Suspend(result)`, and
`Newtype(decl, base)`."

### Verdict

No owner decision approved the shape types, `shape::[T]()`, the
specialized shape types, or `ShapeMetadata`. They came from a provisional
July sketch whose open question was never answered. K1 and TQ-23 refined
them; D7 and decision 7 kept them. Each of those decisions assumed shapes
already existed, as the owner suspects.

## 2. Inventory

### Numbered Rules

Citation counts are from `pnpm run spec refs <id>`.

| Rule | Chapter | Citations | Role |
| --- | --- | --- | --- |
| [`types.shape.consumes`](../spec/04-type-system.md#r-types.shape.consumes) | 04 | 1 (spec) | `shape::[T]()` consumes the reification descriptor |
| [`types.shape.result`](../spec/04-type-system.md#r-types.shape.result) | 04 | 1 (spec) | specialized shape type for data and enums |
| [`types.shape.other`](../spec/04-type-system.md#r-types.shape.other) | 04 | 1 (spec) | `TypeShape` for any other type |
| [`types.shape.erased`](../spec/04-type-system.md#r-types.shape.erased) | 04 | 1 (spec) | erased parameter rejected |
| [`types.shape.members`](../spec/04-type-system.md#r-types.shape.members) | 04 | 1 (spec) | member selection, `shape_of` |
| [`types.assign.shape`](../spec/04-type-system.md#r-types.assign.shape) | 04 | 1 (spec) | specialized shape assignable to its generic type |
| [`grammar.primary.shape-intrinsics`](../spec/02-grammar.md#r-grammar.primary.shape-intrinsics) | 02 | 1 (spec) | ordinary calls |
| [`module.prelude.shape`](../spec/10-modules.md#r-module.prelude.shape) | 10 | 2 (spec, records) | prelude intrinsics |
| [`trait.sealed.shape-metadata`](../spec/09-traits.md#r-trait.sealed.shape-metadata) | 09 | 1 (spec) | `ShapeMetadata` is sealed |
| [`annot.decorator.fn-read`](../spec/14-annotations.md#r-annot.decorator.fn-read) | 14 | 4 (spec, 2 fixtures, src) | function facts read by `shape_of` |

Rules that mention shapes but stay meaningful without them:

| Rule | Citations | Mention |
| --- | --- | --- |
| [`types.reified.metadata`](../spec/04-type-system.md#r-types.reified.metadata) | 2 (spec, records) | names `shape::[T]()` as the use of `reified` |
| [`lex.doc.field`](../spec/01-lexical-structure.md#r-lex.doc.field) | not counted | a doc comment is "the target shape's `doc` field" |
| [`annot.structure.find-lookup`](../spec/14-annotations.md#r-annot.structure.find-lookup) | 1 (spec) | `find` is "the same lookup as `metadata[M]`" |
| [`grammar.primary.no-reflection-syntax`](../spec/02-grammar.md#r-grammar.primary.no-reflection-syntax) | 1 (spec) | reflection has no syntax |
| [`annot.decorator.no-listing`](../spec/14-annotations.md#r-annot.decorator.no-listing) | 1 (spec) | no enumeration of decorated items |

### Unnumbered Text

| Item | Where | Size |
| --- | --- | --- |
| Common Shape Representation and Shape Intrinsics | chapter 14, lines 50 to 264 | about 215 lines, 0 rule IDs |
| Shape types | the same section | `SourcePosition`, `DeclarationKind`, `PrimitiveKind`, `TypeShape`, `FieldShape`, `DataShape`, `VariantShape`, `EnumShape`, `ParamShape`, `FnShape`, `ShapeMetadata`, plus the opaque `DeclarationId` |
| Terminology bullet "shape", and intro sentences | chapter 14, lines 9 and 14 to 37 | about 6 sentences |
| Target Kinds Note: "User code reads a function's values through `shape_of`" | chapter 14 | 1 sentence |
| Glossary: specialized data and enum shape types | spec/README.md | 2 entries |
| Revision Notes: K1, TQ-23, Decorators D1-D9 | spec/README.md | history, unchanged |
| Prelude row `std.annotation` | spec/10-modules.md | 14 names |

### Diagnostics

| Code | Named by | Fixtures |
| --- | --- | --- |
| `unknown-shape-target` | chapter 14 prose only; no numbered rule says `Error: unknown-shape-target` | 4 |
| `unsupported-reified-shape` | prototype only, not in the spec | 0 |

### Fixtures

| Fixture | Shape use |
| --- | --- |
| `parse/valid/shape-intrinsic-calls.hd` | whole file |
| `typing/valid/shape-api.hd` | whole file |
| `typing/valid/type-shape-mutable-field.hd` | whole file |
| `typing/invalid/retention-missing-field.hd` | `unknown-shape-target` |
| `typing/invalid/unknown-shape-variant.hd` | `unknown-shape-target` |
| `typing/invalid/shape-of-closure.hd` | `unknown-shape-target` |
| `typing/invalid/shape-of-local-binding.hd` | `unknown-shape-target` |
| `typing/invalid/metadata-type-as-value.hd` | `FieldShape.metadata(MaxLen)`, `type-used-as-value` |
| `typing/invalid/prelude-shadow-shape-parameter.hd` | `shape` as a prelude name |
| `runtime/valid/function-fact-read.hd` | `shape_of(f).metadata::[Route]()` |
| `runtime/valid/bare-marker-decorator.hd` | `shape_of(f).metadata::[Hidden]()` |
| `typing/valid/annotations.hd` | last 2 lines read metadata through a shape |
| `typing/valid/trait-less-derivation-block.hd` | last 2 lines read metadata through a shape |
| `typing/valid/data-decorator.hd` | last line, `shape::[User]()` |
| `typing/valid/embedded-field-decorator.hd` | last line, `shape::[Post]()` |
| `typing/valid/parameter-decorator.hd` | last line, `shape_of(get_user).params` |
| `typing/valid/retention-metadata.hd` | shapes passed as fact arguments |

### `lib/std`, Tests, And The Prototype

| Place | What | Size |
| --- | --- | --- |
| `lib/std/annotation.hd` | declarations of the shape types, `ShapeMetadata`, `TypeShape.is_optional` | lines 1 to 119 of 148 |
| `test/std/annotation.hd` | tests of the shape values | 58 lines |
| `src/checker/shapes.ts` | builders for `shape`, `shape_of`, `metadata` | 461 lines |
| `src/checker/expression-inspect.ts` | `shapeIntrinsicCall`, `shapeMetadataMethod`, `failReifiedShape` | about 100 lines |
| `src/checker/expression-calls.ts`, `expression-data.ts`, `expression-operators.ts`, `context.ts`, `program.ts` | hooks | about 20 lines |
| `src/checker/prelude-names.ts`, `standard-library.ts` | 14 names, 12 std declarations | about 30 lines |

No other file in `lib/std` calls `shape` or `shape_of`. `Facts.find` and
the typed-derivation code do not share code with `shapes.ts`.

### Guide And Records

| Place | Use |
| --- | --- |
| guide/LANGUAGE_TOUR.md | the `reified` section (`shape::[T]()` is the only observable use), the decorator section (route read), the shape listing |
| guide/USE_SCENARIOS.md | the tool scenario sentence, and the retention example |
| future-work/archive/FN_TYPE.md | option A1, `mcp.tool(get_user, shape_of(get_user))`, and decision 10's note |
| future-work/archive/SPECIAL_CASES.md | rows I13, I14, I30, S20, N2, and the `unknown-shape-target` count |
| future-work/archive/STDLIB.md | `std.annotation` row; the `std.json` needs list (written before typed derivation) |
| future-work/COMPILER_LIBRARY_AUDIT.md, ROADMAP.md, OPEN_ISSUES.md | one row or line each |

## 3. Uses

| # | Use | Where today | Needs shapes today? |
| --- | --- | --- | --- |
| U1 | Read a module-level function's fact, `shape_of(f).metadata::[M]()` | 2 fixtures, the guide's route example | Yes: the only reader |
| U2 | The compiler reads `@num_suffix`, `@str_prefix`, `Template` | [`expr.literal-fn.marker`](../spec/05-expressions.md#r-expr.literal-fn.marker): recognized by qualified name | No |
| U3 | Tools and the test runner read decorators | the [package interface](../spec/10-modules.md#r-module.interface.fact-values) records each fact's value | No |
| U4 | Parked tool adapters | [FN_TYPE](archive/FN_TYPE.md) option A1 passes `shape_of(f)`. Its recommended option B adds `FnStructure`, a `Structure` for functions. Decision 10 parks both. | No: A1 was not chosen |
| U5 | Derived-function cache | [STDLIB](archive/STDLIB.md#derived-function-cache): one value per (trait, type) | No |
| U6 | Read a field's or variant's metadata | fixtures, `test/std/annotation.hd` | No: templates read `h.info.facts` |
| U7 | Pass a field's shape as a fact argument | `retention-metadata.hd`, USE_SCENARIOS retention | Replaceable |
| U8 | Runtime descriptor of any type, `shape::[List[i32]]()` or `shape::[T]()` | the guide's `reified` section | No user beyond the guide |
| U9 | Doc comments at run time | [`lex.doc.field`](../spec/01-lexical-structure.md#r-lex.doc.field) | No: `Member.doc`, `VariantInfo.doc` |
| U10 | Declaration identity, qualified name, source position | the shape types | No reader anywhere |

U4 in more detail: FN_TYPE rejects A1 in its own text, because "nothing
ties the two arguments together". It notes that "Defaults are unusable,
because `FnShape` records only `has_default`." The design it recommends
for functions is a `Structure` analog, not shapes.

## 4. Overlap With Structure

### Data Types And Enums

| Shape surface | Structure surface | Covered? |
| --- | --- | --- |
| `DataShape.name`, `EnumShape.name` | `T::name()` | Yes |
| `field_list`, `variant_list` | `walk`, `describe`, `build` traversals | Yes, typed |
| `fields.email` (checked name) | handles passed to the walker; `members.find` by name | Yes, inside a template |
| `FieldShape.name`, `.position`, `.doc` | `Member.name`, `.position`, `.doc` | Yes |
| `VariantShape.payload` | the variant's members | Yes |
| `FieldShape.field_type: TypeShape` | the handle's static type `F` | Yes, statically |
| `ParamShape.has_default` (payload) | `h.has_default()`, `h.default()` | Yes, with the value |
| `metadata::[M]()` | `h.info.facts.find::[M]()`, typed `h.fact::[M]()` | Yes |
| embedded field metadata | `Member.embedded`, `Member.facts` | Yes |
| type-level facts | `T::facts()` | Yes |
| `id`, `qualified_name`, `source` | none | No reader exists |

The gap is reach, not content: `Structure` may be named only inside a
template ([`annot.structure.named-positions`](../spec/14-annotations.md#r-annot.structure.named-positions)).
A one-off read outside a template needs a small trait and template.
That is the owner's recorded model: "Information derived from a type is an
ordinary trait with an associated function, derived through a template"
(chapter 14 Terminology Note).

### Functions

`Structure` covers no function. A decorator before a function attaches a
value, and today three readers exist:

| Reader | How | Needs `shape_of`? |
| --- | --- | --- |
| The compiler | marker types by qualified name (U2) | No |
| Tools | the package interface (U3) | No |
| User code at run time | `shape_of(f).metadata::[M]()` (U1) | Yes |

Only the third needs a language read. No `lib/std` module, std design, or
tool design reads function facts at run time. Its users are route and
tool registries that read a decorator instead of a registration argument.
FN_TYPE decision 10 already has tools register by hand.

Traits, implementations, methods, newtypes, and parameters already take
decorators with no in-language reader. Their values are "for tools", by the
[Target Kinds](../spec/14-annotations.md#target-kinds) Note. Removing
`shape_of` puts functions in that same group.

### Other Languages

| Language | Runtime read of a function's annotations | Runtime read of a field's annotations |
| --- | --- | --- |
| Rust | No: attributes are compile-time only ([Reference, Attributes](https://doc.rust-lang.org/reference/attributes.html)) | No: derive macros generate code, as serde does |
| Go | No function annotations exist | Yes: struct tags through [`reflect.StructTag`](https://pkg.go.dev/reflect#StructTag) |
| Swift | No | No: [`Mirror`](https://developer.apple.com/documentation/swift/mirror) shows labels and values only |
| Kotlin, Java | Yes, with [`RetentionPolicy.RUNTIME`](https://docs.oracle.com/javase/8/docs/api/java/lang/annotation/RetentionPolicy.html) | Yes |

Rust and Go ship no runtime read of function annotations. hd's
templates give the field case the Rust model: generated typed code.

## 5. Options

### Option A: Remove `shape`, `shape_of`, And The Shape Types

**Before** (function fact):

```text
data Route:
    path: string

fn route(path: string) -> Route:
    Route { path: path }

@route("/users")
fn list_users() -> string:
    "[]"

fn users_path() -> string:
    match shape_of(list_users).metadata::[Route]():
        .Some(found) => found.path
        .None => ""
```

**After**: the registration states the path.

```text
fn list_users() -> string:
    "[]"

fn routes() -> List[(string, fn() -> string)]:
    [("/users", list_users)]
```

**Before** (field metadata), from `typing/valid/annotations.hd`:
`limit := shape::[User]().fields.name.metadata::[MaxLen]()`.

**After**: a trait with a template, the typed-derivation form.

```text
use std.structure.Structure

trait Limits:
    fn limits() -> List[(string, i32)]

data LimitList:
    out: List[(string, i32)]

impl[S] Describer[S] for LimitList:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        .Ok()

    fn member[F](mut self, h: Field[S, F]) -> Result[void, never]:
        match h.info.facts.find::[MaxLen]():
            .Some(m) => self.out.append((h.info.name, m.value))
            .None => pass
        .Ok()

impl[T] Limits for T by Structure:
    fn limits() -> List[(string, i32)]:
        let mut d = LimitList { out: [] }
        _ := T::describe(d)
        d.out

@derive(Limits)
data User:
    @max_len(80)
    name: string
```

**Before** (shape as a fact argument), from `retention-metadata.hd`:
`userId = [retention_owner(shape::[User]()), delete_when(shape::[User]().fields.deleted)]`.

**After**: a function-valued fact, as
[`std-testing.arbitrary.with`](../spec/std/testing.md#r-std-testing.arbitrary.with)
already uses. The accessor is type-checked, as `fields.deleted` was.

```text
use std.structure.Structure

fn user_deleted(user: User) -> bool:
    user.deleted

impl Post by Structure:
    userId = [delete_when(user_deleted)]
```

**Rule accounting**

| Item | Change |
| --- | --- |
| `types.shape.consumes`, `.result`, `.other`, `.erased`, `.members` | deleted |
| `types.assign.shape` | deleted |
| `grammar.primary.shape-intrinsics` | deleted |
| `module.prelude.shape` | deleted |
| `trait.sealed.shape-metadata` | deleted |
| `annot.decorator.fn-read` | deleted; a function's values join the "for tools" group |
| `types.reified.metadata` | reworded: drop the `shape::[T]()` example |
| `lex.doc.field` | reworded: the doc is `Member.doc` or `VariantInfo.doc`; other docs are for tools |
| `annot.structure.find-lookup` | reworded: state the lookup itself, not by reference to `metadata[M]` |
| `module.prelude.names` | unchanged ID; the `std.annotation` row and its 14 names are deleted |
| `grammar.primary.no-reflection-syntax`, `annot.decorator.no-listing` | unchanged |
| Common Shape Representation, Shape Intrinsics | deleted, about 215 lines; links in 5 other spec files and 4 records repointed |
| Terminology bullet, intro sentences, Target Kinds Note | reworded |
| Glossary: 2 shape-type entries | deleted |
| `unknown-shape-target` | deleted; `shape::[User]()` and `shape_of(f)` report `unknown-name` |
| 10 fixtures: the first 9 of the fixture table, and `function-fact-read.hd` | deleted; `type-used-as-value` keeps `type-name-as-value.hd` |
| `bare-marker-decorator.hd` | rewritten: the marker sits on a data type and a template reads it |
| `annotations.hd`, `trait-less-derivation-block.hd`, `data-decorator.hd`, `embedded-field-decorator.hd`, `parameter-decorator.hd` | rewritten: drop the shape lines |
| `retention-metadata.hd` | rewritten: a function-valued fact |

Net: 10 numbered rules deleted, 3 reworded, no rule added. The language
tier goes from 3,634 to 3,624 rules (`pnpm run spec counts`).

**What users lose**

| Loss | Replacement |
| --- | --- |
| Reading a function's decorator at run time | an explicit registration argument; tools read the interface |
| A function's parameter names and docs at run time | written by hand at registration, until FN_TYPE questions 9 and 10 |
| One-line metadata reads outside a template | a trait and a template, about 20 lines |
| `TypeShape`, a runtime type descriptor | static types inside templates; identity through `Inspectable` and `TypeId` |
| `reified` loses its only observable operation | a separate question, below |

**Prototype and `lib/std` code deleted:** `src/checker/shapes.ts` (461
lines), about 100 lines of `expression-inspect.ts`, about 50 lines of
hooks and name lists, lines 1 to 119 of `lib/std/annotation.hd`, and all
of `test/std/annotation.hd` (58 lines). `Target`, `Annotate`, and
`annotate` stay in `std.annotation`.

**Soundness.** Nothing that a shape rule rejected becomes valid, because
the names stop existing. A misspelled field, `shape::[User]().fields.deleted`,
was `unknown-shape-target`. Its replacement, `user.deleted` in an accessor,
is `unknown-data-field`.

### Option B: Remove `shape::[T]()`, Keep A Minimal Function-Fact Read

Keep D7's one read, but on the `Facts` type that typed derivation already
has, with no shape types. The name `facts_of` is a placeholder; it would
be imported, as `annotate` is, not a prelude name.

**After**:

```text
use std.annotation.facts_of

@route("/users")
fn list_users() -> string:
    "[]"

fn users_path() -> string:
    match facts_of(list_users).find::[Route]():
        .Some(found) => found.path
        .None => ""
```

The data and enum examples are as in option A.

**Rule accounting**

| Item | Change |
| --- | --- |
| `types.shape.*` (5), `types.assign.shape`, `grammar.primary.shape-intrinsics`, `module.prelude.shape`, `trait.sealed.shape-metadata` | deleted (9) |
| `annot.decorator.fn-read` | reworded: `facts_of(f).find::[M]()` |
| new: `facts_of` form, argument must name a module-level function, other arguments are errors | about 3 numbered rules, from today's unnumbered `shape_of` prose |
| `types.reified.metadata`, `lex.doc.field`, `annot.structure.find-lookup` | reworded, as in A |
| Shape types, specialized shape types, glossary entries, 14 prelude names | deleted; `std.annotation` exports `facts_of` |
| `unknown-shape-target` | kept, for `facts_of(closure)` |
| 7 fixtures | deleted: the first 9 of the fixture table except the 2 `shape_of` error cases |
| 10 fixtures | rewritten: the 2 `shape_of` error cases and the 2 runtime fixtures move to `facts_of`; 6 drop or replace shape lines |

Net: 9 numbered rules deleted, about 3 added, 4 reworded.

**What users lose:** as in A, except that a function's decorator stays
readable. Parameter names, docs, and parameter metadata are still lost.

**Prototype and `lib/std`:** as in A, except about 60 lines of `shapes.ts`
stay for the function lookup.

### Option C: Keep As Is

Nothing is deleted. The surface still needs work before it is reviewed
text:

| Debt | Detail |
| --- | --- |
| No rule IDs | about 215 lines of normative prose, never restyled |
| An unmet promise | "every shape provides ... declaration kind", but no shape type has that field, and `DeclarationKind` lacks traits, implementations, methods, and newtypes |
| Two lookups | `metadata::[M]()` beside `Facts.find::[M]()` and `h.fact::[M]()` |
| Two member descriptions | `FieldShape` beside `Member`, `VariantShape` beside `VariantInfo` |
| Two specialized types | the compiler-generated, unnameable shape types (SPECIAL_CASES I13) |
| Open gaps | `shape_of` on generic functions; parameter and payload access by position only |
| Prototype gap | `shape::[T]()` for a type parameter reports `unsupported-reified-shape` |

**What users keep:** a one-line runtime read of any declaration, function
parameter names at run time, and `TypeShape`.

## 6. Recommendation

**Recommendation: option A.** It is a pure deletion of compiler
intrinsics, the costliest kind of change in AGENTS.md "Design Cost Order"
that this area holds. Every data and enum use has a typed-derivation form.
The one function use has an explicit form that hd's registration model
already prefers. Function per-declaration data then waits for FN_TYPE
questions 9 and 10, whose recommended design is a `Structure` for
functions, not shapes.

Option B is the fallback if the owner wants D7's run-time read now. It
keeps one intrinsic and three rules, and no shape types.

A follow-up for either A or B: `reified` would have no observable
operation left. The guide says its descriptor is "observable only as
`shape::[T]()`", and SPECIAL_CASES S20 cites the same reason. Reviewing
`reified` is a separate cut, after this one is decided.

## Questions For The Owner

### Q1. May user code read a function's decorator values at run time?

Effect: decides whether D7 stays. If not, a function's values serve only
the compiler and tools, as a trait's or a method's do.

- **A.** No. Registration passes its data explicitly. **Recommended.**
- **B.** Yes, through one small read on the existing `Facts` type,
  `facts_of(f).find::[M]()`.
- **C.** Yes, through `shape_of(f)` and `FnShape`, as today.

```text
fn list_users() -> string:
    "[]"

fn routes() -> List[(string, fn() -> string)]:
    [("/users", list_users)]
```

### Q2. Remove `shape::[T]()` and every shape type?

Effect: data and enum structure is read only by templates over
`Structure`. Nine numbered rules go, ten if Q1 takes A. About 215 lines
of unnumbered text, 14 prelude names, and about 600 lines of prototype
code go with them.

- **A.** Remove them all. **Recommended.**
- **B.** Keep them, and restyle the section with rule IDs.

```text
use std.structure.Structure

fn user_deleted(user: User) -> bool:
    user.deleted

impl Post by Structure:
    userId = [delete_when(user_deleted)]
```

### Q3. If shapes go, should `reified` be reviewed next?

Effect: `reified` keeps its grammar, lexical, typing, and dynamic-safety
rules with no operation that reads its descriptor. This waits for Q2.

- **A.** Yes, queue a complexity review of `reified`. **Recommended.**
- **B.** No, keep `reified` for future runtime type needs.

```text
fn runtime_type[T < Inspectable]() -> TypeId:
    TypeId::of::[T]()
```

## Parse Log

Every `text` block above was parsed with `parseSource` from
`spec/reference-parser/parser.ts`. Parsing checks syntax only; no block is
claimed to type-check.

| Block | Where | Result |
| --- | --- | --- |
| 1 | Option A, Before (function fact) | parses |
| 2 | Option A, After (function fact) | parses |
| 3 | Option A, After (field metadata) | parses |
| 4 | Option A, After (fact argument) | parses |
| 5 | Option B, After | parses |
| 6 | Q1 | parses |
| 7 | Q2 | parses |
| 8 | Q3 | parses |
