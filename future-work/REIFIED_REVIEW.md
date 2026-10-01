# Reified Review: Does `reified` Still Earn Its Rules?

Status: simplification review, 2026-10-01. Nothing here is accepted
behavior, and it changes no decision, spec text, fixture, or prototype
code. It answers Q3 of the owner's batch 42 decision on the
[Shape Review](SHAPE_REVIEW.md#q3-if-shapes-go-should-reified-be-reviewed-next):
with `shape::[T]()`, `shape_of`, and every shape type removed, and
`facts_of(f).find::[M]()` added, does `reified` still earn its rules?
Spec pass 42 may still be landing; this record treats shapes as removed.

Under review:
- [Reified Parameters](../spec/04-type-system.md#reified-parameters) and
  [`types.generic.specialized`](../spec/04-type-system.md#r-types.generic.specialized);
- [`types.trait.safe.no-reified`](../spec/04-type-system.md#r-types.trait.safe.no-reified)
  and [`trait.dyn.safe.reified`](../spec/09-traits.md#r-trait.dyn.safe.reified);
- [`fn.type.generic.reified`](../spec/07-functions.md#r-fn.type.generic.reified)
  and [`fn.generic.reified`](../spec/07-functions.md#r-fn.generic.reified);
- the contextual word, [`lex.contextual.reified`](../spec/01-lexical-structure.md#r-lex.contextual.reified),
  and [Generic Parameter Modifiers](../spec/02-grammar.md#generic-parameter-modifiers);
- [Runtime Type Identity](../spec/09-traits.md#runtime-type-identity), which
  supplies type identity through a bound instead;
- the fact lookup, [`annot.structure.facts-type`](../spec/14-annotations.md#r-annot.structure.facts-type),
  [`annot.structure.find-lookup`](../spec/14-annotations.md#r-annot.structure.find-lookup)
  and [`annot.handle.fact`](../spec/14-annotations.md#r-annot.handle.fact);
- the guide's `reified` sections in
  [LANGUAGE_TOUR.md](../guide/LANGUAGE_TOUR.md).

## Contents

- [Summary](#summary)
- [1. Provenance](#1-provenance)
- [2. Inventory](#2-inventory)
- [3. Uses After Shapes Go](#3-uses-after-shapes-go)
- [4. Options](#4-options)
- [5. Recommendation](#5-recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Parse Log](#parse-log)

## Summary

| Question | Finding |
| --- | --- |
| Was `reified` approved? | Yes, once: item 87 of the owner's "Current Design Decisions" log, 2026-08-08. It named five uses. Four have since moved to other mechanisms, and batch 42 removes the fifth. |
| What still reads its descriptor? | Nothing in the spec, `lib/std`, or the prototype. Type identity comes from the `Inspectable` bound; `name()` and derivation come from `Structure`. |
| Is anything left that needs a runtime descriptor for a type parameter? | One operation: the fact lookup `find::[M]()`, and `h.fact::[M]()`, when `M` mentions a type parameter. The spec never says where that key comes from. The prototype bounds it by `Inspectable`, not `reified`. |
| What do its rules protect? | Only themselves: the dynamic-safety exclusion, the marker-matching rule, and the lone-`reified` syntax error exist because the modifier exists. |
| Recommendation | Option A, remove `reified`: 19 numbered rules deleted, 11 reworded, a contextual word and two grammar alternatives gone. State the fact lookup's key with the `Inspectable` bound, as the prototype already does. |

## 1. Provenance

### Timeline

| Date | Commit | What entered | Owner decision? |
| --- | --- | --- | --- |
| 2026-08-08 | `a3394e32` "update" | LANGUAGE_IDEA "Current Design Decisions" item 87; SYNTAX_NOTES lists five operations that need reification; the guide gains both `reified` sections. | Yes: item 87 sits in the owner's decision log, not among open questions. |
| 2026-09-22 | `89e8e616` "add spec" | Chapters 04 and 07 state the reified rules; the idea log shrinks to pointers. | Carries item 87 over. |
| 2026-09-25 | `1e55fc94` | Open Issues "Runtime Type Identity And `reified`", direction 1: "The target of a type test needs a runtime descriptor, and that descriptor comes from `reified`." | A direction, then reversed below. |
| 2026-09-26 | `9ea5e2b0` GQ17 | `reified` stops being a reserved word and becomes contextual. | Yes, on spelling only. |
| 2026-09-26 | `4738afb4` B8 | `[reified]` becomes a `syntax-error`; write `` [`reified`] ``. | Yes, on spelling only. |
| 2026-09-26 | `1d143bcd`, `d0bce33f` | Inspectable decision 6: erasure needs `T < Inspectable`, and `reified T` alone does not allow it. Decision 15: the bound's dictionary carries the `TypeId`, "so no `reified` is needed". | Yes. Takes type tests and casts away from `reified`. |
| 2026-09-27 | `5bd21380` TQ-10 | The one-copy rule; "Only `reified` parameters and packs remain excluded" from dynamic safety. | Yes. Assumes `reified` exists. |
| 2026-09-27 | `f3f25bd9` | Chapter 04 restyled; the `types.reified.*` IDs appear. | Not a decision. |
| 2026-10-01 | Shape Review Q3 | "`reified` would have no observable operation left." | The question this record answers. |

Commits before 2026-09-25 are owner-committed "update" commits with no
`Co-Authored-By` line. As with shapes, the repo cannot show who wrote their
text. Unlike shapes, item 87 was filed under decisions, not as provisional.

### The Decision

LANGUAGE_IDEA.md at `a3394e32`, "Current Design Decisions", item 87:

> 87. Function generic parameters are erased at runtime by default. A
> parameter marked `reified`, such as `fn resolve[reified T]() -> T`,
> receives hidden runtime type metadata and may use `shape(T)`, runtime
> annotation lookup, type-directed dependency injection, and similar
> operations. Passing an outer generic parameter to a reified parameter
> requires the outer parameter to be reified as well. Reification does not
> require `inline`; JavaScript and WebAssembly backends may specialize or
> remove descriptors only when observable behavior is preserved. In v1,
> `reified` applies only to function generic parameters.

SYNTAX_NOTES.md, same commit, still in the tree today:

> The initial runtime-type operations requiring reification include:
> 1. `shape(T)` when `T` is a generic parameter.
> 2. Runtime annotation lookup for `T`.
> 3. Type-directed dependency injection such as `resolve[T]()`.
> 4. Runtime serialization or deserialization selected from `T`.
> 5. Runtime type tests or casts involving `T`, if those operations are added.

### Where Each Use Went

| # | 2026-08-08 use | Today | Needs `reified`? |
| --- | --- | --- | --- |
| 1 | `shape(T)` | removed by batch 42 | No consumer left |
| 2 | Runtime annotation lookup | `Facts.find::[F]()`, `h.fact::[M]()`, and batch 42's `facts_of(f).find::[M]()`; `ShapeMetadata.metadata[reified M]` goes with shapes | Unstated; see [section 3](#3-uses-after-shapes-go) |
| 3 | Type-directed DI, `resolve[T]()` | requirement rows keyed by trait, `$.use(Database)`; no spec, std, or fixture has `resolve` | No |
| 4 | Serialization selected from `T` | templates over `Structure`, bounded by a derived trait (typed derivation decisions 1 and 7) | No: the bound's dictionary |
| 5 | Type tests and casts | `Inspectable`, `TypeId::of::[T]()`, `downcast` (Inspectable decision 15) | No: the bound's dictionary |

### Verdict

An owner decision exists: item 87 of 2026-08-08. Its stated reason was the
five operations above. Decisions 6 and 15 of the Inspectable design moved
type tests to a bound, typed derivation moved serialization to templates,
and batch 42 removes shapes. No later decision restated a need for
`reified`. GQ17, B8, and TQ-10 refined or restricted it, each assuming it
already existed.

## 2. Inventory

### Numbered Rules

Citation counts are from `pnpm run spec refs <id>`. Every rule below is
cited only by its own definition, unless the count says otherwise.

| Rule | Ch. | Citations | Role |
| --- | --- | --- | --- |
| [`lex.contextual.reified`](../spec/01-lexical-structure.md#r-lex.contextual.reified) | 01 | 1 | the contextual word's position |
| [`lex.contextual.reified.modifier`](../spec/01-lexical-structure.md#r-lex.contextual.reified.modifier) | 01 | 1 | always the modifier there |
| [`lex.contextual.reified.lone`](../spec/01-lexical-structure.md#r-lex.contextual.reified.lone) | 01 | 1 | `[reified]` is `syntax-error` |
| [`lex.contextual.reified.raw`](../spec/01-lexical-structure.md#r-lex.contextual.reified.raw) | 01 | 3 (spec) | `` [`reified`] `` names a parameter |
| [`grammar.generic.reified-modifier`](../spec/02-grammar.md#r-grammar.generic.reified-modifier) | 02 | 1 | restates `.modifier` and `.lone` |
| [`grammar.generic.reified-positions`](../spec/02-grammar.md#r-grammar.generic.reified-positions) | 02 | 2 (spec) | function, method, variant, implementation parameters |
| [`types.generic.specialized`](../spec/04-type-system.md#r-types.generic.specialized) | 04 | 1 | "Calls with `reified` parameters are specialized." |
| [`types.reified.metadata`](../spec/04-type-system.md#r-types.reified.metadata) | 04 | 4 (spec 1, records 3) | carries metadata, "used by operations such as `shape::[T]()`" |
| [`types.reified.erased`](../spec/04-type-system.md#r-types.reified.erased) | 04 | 1 | erased parameter cannot supply identity; no error code |
| [`types.reified.inspectable`](../spec/04-type-system.md#r-types.reified.inspectable) | 04 | 1 | identity for `Inspectable` comes from the bound |
| [`types.reified.inspectable.uses`](../spec/04-type-system.md#r-types.reified.inspectable.uses) | 04 | 1 | erasure, `TypeId::of`, `downcast` need the bound, not `reified` |
| [`types.reified.abi`](../spec/04-type-system.md#r-types.reified.abi) | 04 | 1 | part of the public type and ABI |
| [`types.reified.specialize`](../spec/04-type-system.md#r-types.reified.specialize) | 04 | 1 | a backend "may" specialize |
| [`types.trait.safe.no-reified`](../spec/04-type-system.md#r-types.trait.safe.no-reified) | 04 | 2 (spec) | a method must not declare one |
| [`fn.type.generic.reified`](../spec/07-functions.md#r-fn.type.generic.reified) | 07 | 1 | a function value captures descriptors |
| [`fn.generic.reified`](../spec/07-functions.md#r-fn.generic.reified) | 07 | 1 | `reified T` requests metadata |
| [`trait.dyn.safe.reified`](../spec/09-traits.md#r-trait.dyn.safe.reified) | 09 | 3 (spec 2, fixtures 1) | not dynamically safe |
| [`trait.erase.reified`](../spec/09-traits.md#r-trait.erase.reified) | 09 | 1 | `reified T` alone does not allow erasure; `type-mismatch` |
| [`gadt.erasure.reified`](../spec/13-gadts.md#r-gadt.erasure.reified) | 13 | 1 | preserves only explicit descriptors |

Rules that mention `reified` but keep a job without it:

| Rule | Citations | Mention |
| --- | --- | --- |
| [`lex.raw.not-reserved`](../spec/01-lexical-structure.md#r-lex.raw.not-reserved) | 1 | example: `` `reified` `` is a parameter name |
| [`types.generic.sharing`](../spec/04-type-system.md#r-types.generic.sharing) | 1 | "an erased generic parameter has no runtime type identity" |
| [`fn.generic.erased`](../spec/07-functions.md#r-fn.generic.erased) | 1 | erased "by default" |
| [`trait.impl.generics.markers`](../spec/09-traits.md#r-trait.impl.generics.markers) | 1 | keeps the `reified` marker and the bounds |
| [`trait.bound.representation.semantics`](../spec/09-traits.md#r-trait.bound.representation.semantics) | 1 | "including reflection behavior for reified parameters" |
| [`trait.inspectable.not.parameter`](../spec/09-traits.md#r-trait.inspectable.not.parameter) | 1 | "whether or not it is `reified`" |
| [`trait.downcast.evidence`](../spec/09-traits.md#r-trait.downcast.evidence) | 1 | "No `reified` marker is needed." |
| [`module.interface.contents`](../spec/10-modules.md#r-module.interface.contents) | 1 | table row: "bodies of pack and reified code" |
| [`gadt.runtime.erased`](../spec/13-gadts.md#r-gadt.runtime.erased) | 1 | "unless a reified operation requires them" |
| [`gadt.erasure.no-descriptor`](../spec/13-gadts.md#r-gadt.erasure.no-descriptor) | 1 | no descriptor from a GADT match |
| [`annot.structure.find-lookup`](../spec/14-annotations.md#r-annot.structure.find-lookup) | 4 (spec 1, records 3) | "the same narrow runtime type lookup as `metadata[M]`", which was `[reified M]` |

Unnumbered text: the two EBNF alternatives `[ "reified" ]` in
`generic_parameter` and `function_generic_parameter`; the chapter 01 error
example; the `Lookup` example and one Why sentence under Dynamic Safety;
one sentence in chapter 04's Implementation Model; the revision notes for
GQ17, B8, and TQ-10, which stay as history. The section
[Shapes and Generic Code](../spec/04-type-system.md#shapes-and-generic-code)
uses "shape" for value layout, an unrelated sense, and stays.

### Diagnostics

No diagnostic code belongs to `reified`. It appears in `syntax-error`
(lone modifier), `trait-not-dynamically-safe`, and `type-mismatch`
(erasure). [`types.reified.erased`](../spec/04-type-system.md#r-types.reified.erased)
and the guide's "compile error: T is erased" have no code at all.

### Fixtures

| Fixture | Use |
| --- | --- |
| `parse/valid/contextual-reified.hd` | whole file: modifier, `` `reified` ``, both |
| `parse/invalid/lone-reified-generic-parameter.hd` | `[reified]`, `syntax-error` |
| `parse/invalid/lone-reified-before-bound.hd` | `[T, reified < Show]`, `syntax-error` |
| `typing/invalid/dynamic-safety-reified-parameter.hd` | `trait-not-dynamically-safe` |
| `typing/invalid/generic-erasure-reified-without-bound.hd` | `type-mismatch` |
| `parse/valid/type-argument-defaults.hd` | one method, `widen[reified T = i64]` |
| `runtime/valid/contextual-words-as-names.hd` | `reified` as a local name |
| `runtime/valid/inspectable-generic-target-helper.hd` | comment: "without reified" |

`cases.tsv` and `test/portable/cases.tsv` have 5 rows each, and
`examples.tsv` has 3. No runtime fixture observes a descriptor.

### `lib/std`, Prototype, Tools, Guide

| Place | What | Size |
| --- | --- | --- |
| `lib/std/annotation.hd` | `ShapeMetadata.metadata[reified M]` | 1 line; goes with shapes |
| `src/parser/decorators.ts` | parse the modifier, reject a lone one | about 15 lines |
| `src/ast.ts`, `src/hir.ts`, `src/parser/parser.ts`, `src/checker/program-types.ts` | carry `reifiedParameters` | 6 lines |
| `src/checker/shared.ts` | dynamic-safety check | 1 line |
| `src/checker/expression-inspect.ts` | `failReifiedShape`, `unsupported-reified-shape` | goes with shapes |
| `src/highlight.ts`, `test/repl.test.ts`, `website/playground/` tests, `editors/hd_nvim` | highlight the contextual word | about 12 lines |
| `spec/reference-parser/contextual.ts` | `loneReifiedParameter` | about 25 lines |
| `spec/check-example-overlap.ts` | keyword list | 1 word |
| guide/LANGUAGE_TOUR.md | two sections: `runtime_shape`, `resolve`, "observable only as `shape::[T]()`" | about 45 lines |
| SYNTAX_NOTES.md | the 2026-08-08 section | about 45 lines |

The prototype comment says it plainly: "the prototype erases every generic
parameter". It never builds a descriptor.

## 3. Uses After Shapes Go

Every operation in the spec that needs a type's runtime identity, and where
it gets it:

| Operation | Needs identity of | Source of identity | `reified` involved? |
| --- | --- | --- | --- |
| `shape::[T]()`, `shape_of(f)` | `T` | the reified descriptor | Removed by batch 42 |
| `ShapeMetadata.metadata[reified M]()` | `M` | the reified descriptor | Removed by batch 42 |
| Erasure of `x: T` to `Inspectable` | `T` | `T < Inspectable` evidence ([`trait.erase.parameter`](../spec/09-traits.md#r-trait.erase.parameter)) | No: [`trait.erase.reified`](../spec/09-traits.md#r-trait.erase.reified) rejects it |
| `TypeId::of::[T]()` | `T` | `T < Inspectable` | No |
| `downcast`, `downcast_mut`, `downcast_val` | the target `T` | `T < Inspectable` ([`trait.downcast.evidence`](../spec/09-traits.md#r-trait.downcast.evidence)) | No |
| `value.runtime_type()` | the erased value | recorded at erasure | No |
| `T::name()`, `T::facts()`, `walk`, `describe`, `build` | `T` | the `Structure` dictionary; `name()` is a compile-time constant ([`annot.structure.name.constant`](../spec/14-annotations.md#r-annot.structure.name.constant)) | No |
| `T::f()` under any bound | `T` | the bound's dictionary (TQ-9) | No |
| `is` on a type parameter | the reference | `T < AnyRef` | No |
| `Any` | nothing: erasure to `Any` is one-way | none | No |
| `Facts.find::[F]()`, `facts_of(f).find::[M]()` | `F` or `M` | **unstated** | Only by reference to the deleted `metadata[reified M]` |
| `h.fact::[M]()` | `M`, whose target argument is the handle's `F` | **unstated** | No: `fact[M]` is declared without `reified` |

So the one remaining operation is the fact lookup, and only when its type
argument mentions a type parameter. With a concrete argument, as in
`facts_of(list_users).find::[Route]()`, the compiler knows the key.

### The Fact Lookup Has No Stated Key

The spec states the lookup's result, not where its key comes from:

| Place | What it says |
| --- | --- |
| [`annot.structure.facts-type`](../spec/14-annotations.md#r-annot.structure.facts-type) | `facts.find::[F]()` returns the fact whose concrete type is `F`. `Facts` has no declared signature. |
| [`annot.structure.find-lookup`](../spec/14-annotations.md#r-annot.structure.find-lookup) | "the same narrow runtime type lookup as `metadata[M]`", which batch 42 deletes |
| The `std.structure` declarations | `pub fn fact[M](self) -> M?`, with no bound and no `reified` |
| `typing/valid/typed-fact.hd` | `note[S, F < Display]` calls `h.fact::[Fallback[F]]()` and is accepted |
| The prototype, `src/checker/typed-derivation.ts` | `find[F < Inspectable]` and `fact[HdM < Inspectable]`, built on `downcast_val` |
| `test/portable/KNOWN_FAILURES.tsv` | `typed-fact.hd` fails: "`h.fact::[Fallback[F]]()` needs `F < Inspectable`" |

A shared generic body cannot find `Fallback[F]` without knowing `F`. Today
the spec accepts that call with neither a bound nor a marker, so the gap
exists before any cut. Batch 42 widens it: `facts_of(f).find::[M]()` lets
ordinary code write generic fact helpers.

For `h.fact::[M]()`, part of the key is free. The handle fixes `M`'s target
argument to `F` ([`annot.handle.fact`](../spec/14-annotations.md#r-annot.handle.fact)),
so only `M`'s constructor and its other arguments need runtime identity.

Three ways to state the key, none of which is decided:

| Key | Rule | Generic helper | Costs |
| --- | --- | --- | --- |
| K1, a bound | `find[F < Inspectable]`, `fact[M < Inspectable]` | `fn fact_or[M < Inspectable](facts: Facts, fallback: M) -> M` | A fact must be inspectable; a template reading a typed fact needs `F < Inspectable` unless the handle's target argument is exempt |
| K2, a marker | `find[reified F]`, `fact[reified M]` | `fn fact_or[reified M](...)` | Keeps all of `reified`; a walker's `member[F]` would need `reified F` to read `Fallback[F]` |
| K3, static | the type argument may not mention a type parameter, except the handle's target argument | none | No generic fact helpers |

K1 reuses the mechanism that `downcast_val` already uses, and the prototype
implements it. K2 is option B below.

## 4. Options

### Option A: Remove `reified`

Every generic parameter is erased. Runtime type identity for a type
parameter comes only from `T < Inspectable`. The fact lookup states its key
by K1 or K3.

**Before**, valid today:

```text
trait Lookup:
    fn find[reified M < AnyRef](self) -> M?

fn keep[reified T](value: T) -> T:
    value
```

**After**: the marker goes; `Lookup` becomes dynamically safe, since `M` is
bounded by `AnyRef`.

```text
trait Lookup:
    fn find[M < AnyRef](self) -> M?

fn keep[T](value: T) -> T:
    value
```

**Runtime identity**, unchanged in every option:

```text
use std.inspect.{Inspectable, TypeId}

fn is_a[T < Inspectable](value: Inspectable) -> bool:
    value.runtime_type() == TypeId::of::[T]()
```

**Fact lookup with K1**: a generic helper names the bound, as `downcast_val`
does.

```text
use std.inspect.Inspectable
use std.structure.Facts

fn fact_or[M < Inspectable](facts: Facts, fallback: M) -> M:
    match facts.find::[M]():
        .Some(found) => found
        .None => fallback
```

**Batch 42's read** needs neither, because `Route` is concrete:

```text
use std.annotation.facts_of

data Route:
    path: string

fn route(path: string) -> Route:
    Route { path: path }

@route("/users")
fn list_users() -> string:
    "[]"

fn users_path() -> string:
    match facts_of(list_users).find::[Route]():
        .Some(found) => found.path
        .None => ""
```

**Meaning change.** `reified` becomes an ordinary identifier. A program
with `fn keep[reified T]` stops parsing: `reified T` is two names, a
`syntax-error`. `[reified]` and `[T, reified < Show]`, errors under B8,
become valid parameters named `reified`:

```text
fn named[reified](value: reified) -> reified:
    value
```

**Rule accounting**

| Item | Change |
| --- | --- |
| `lex.contextual.reified`, `.modifier`, `.lone`, `.raw` | deleted (4); the table row and the error example go |
| `grammar.generic.reified-modifier`, `.reified-positions` | deleted (2) |
| `types.generic.specialized` | deleted |
| `types.reified.metadata`, `.erased`, `.abi`, `.specialize` | deleted (4) |
| `types.reified.inspectable`, `.inspectable.uses` | deleted (2), merged into `trait.erase.parameter`, `trait.erase.bound-required`, and `trait.downcast.evidence`, which state the same |
| `types.trait.safe.no-reified` | deleted; its second sentence is `trait.dyn.safe.row-parameter` and `.suspending` |
| `fn.type.generic.reified`, `fn.generic.reified` | deleted (2) |
| `trait.dyn.safe.reified` | deleted; the `Lookup` example and the Why sentence go |
| `trait.erase.reified` | deleted; `trait.erase.bound-required` gives the same `type-mismatch` |
| `gadt.erasure.reified` | deleted |
| `lex.raw.not-reserved` | reworded: drop the `reified` example |
| `types.generic.sharing` | reworded: a type parameter has runtime identity only through `T < Inspectable` |
| `fn.generic.erased` | reworded: "Generic parameters are erased", without "by default" |
| `trait.impl.generics.markers` | reworded: "keeps the trait method's bounds" |
| `trait.bound.representation.semantics` | reworded: drop "reflection behavior for reified parameters" |
| `trait.inspectable.not.parameter` | reworded: drop "whether or not it is `reified`" |
| `trait.downcast.evidence` | reworded: drop "No `reified` marker is needed." |
| `module.interface.contents` | reworded: the row becomes "the bodies of pack code" |
| `gadt.runtime.erased` | reworded: "need not preserve type arguments" |
| `gadt.erasure.no-descriptor` | reworded: a GADT match supplies no `Inspectable` evidence |
| `annot.structure.find-lookup` | reworded by pass 42 anyway; states K1 or K3 |
| `generic_parameter`, `function_generic_parameter` | reworded: drop `[ "reified" ]`; the second alternative of `function_generic_parameter` merges into the first |
| Diagnostics | none deleted; a lone `reified` no longer reports `syntax-error`, and `[reified T]` does |
| `contextual-reified.hd`, both `lone-reified-*.hd`, `dynamic-safety-reified-parameter.hd` | deleted (4) |
| `generic-erasure-reified-without-bound.hd` | rewritten: `fn erase[T](value: T)`, same `type-mismatch` |
| `type-argument-defaults.hd` | rewritten: `widen[T = i64]` |
| `contextual-words-as-names.hd`, `inspectable-generic-target-helper.hd` | rewritten: comments only |
| Revision Notes for GQ17, B8, TQ-10 | unchanged; one new entry |

Net: 19 numbered rules deleted, 11 reworded, none added. The language
tier loses 19 rules beyond whatever pass 42 removes (`pnpm run spec counts`
gives 3,634 before either).

**What users lose**

| Loss | Replacement |
| --- | --- |
| A runtime descriptor for an unbounded type parameter | `T < Inspectable`, which also proves `T` is not a closure |
| Type-directed DI, `resolve::[T]()` | none needed: requirement rows key providers by trait |
| A future reflection API on `T` | would come back as a bound or a template, by the Design Cost Order |
| Naming a type parameter `reified` without backticks | gained, not lost |

**Prototype and tools deleted:** about 15 lines of `decorators.ts`, 8 lines
of carrying fields, the dynamic-safety clause, about 25 lines of the
reference parser's `contextual.ts`, and about 12 lines of highlighting and
its tests. The guide loses about 45 lines, and SYNTAX_NOTES its section.

**Soundness.** The reified rules protected two things. One-copy dynamic
safety holds, because no method can now ask for per-call specialization.
Erasing an unbounded `T` is still `type-mismatch` by
[`trait.erase.bound-required`](../spec/09-traits.md#r-trait.erase.bound-required):

```text
use std.inspect.Inspectable

fn erase[T](value: T) -> Inspectable:
    value  # error: type-mismatch
```

*Holds with condition*: the fact lookup's key must be stated (K1 or K3).
That gap exists today, with or without `reified`.

### Option B: Keep `reified`, Narrowed To The Fact Lookup

`reified` stays the only way to look up a fact by a type parameter (K2).
The std declarations gain the marker: `Facts.find[reified F]` and
`Field.fact[reified M]`.

**After**:

```text
use std.structure.Facts

fn fact_or[reified M](facts: Facts, fallback: M) -> M:
    match facts.find::[M]():
        .Some(found) => found
        .None => fallback
```

**Rule accounting**

| Item | Change |
| --- | --- |
| `types.reified.metadata` | reworded: consumed by `find` and `fact` |
| `grammar.generic.reified-positions` | reworded: functions and methods only; variant and implementation parameters have no consumer |
| `types.generic.specialized` | reworded: it says calls "are specialized", while `.abi` and `.specialize` say descriptors pass and a backend "may" specialize |
| `types.reified.erased` | reworded: give it an error code |
| `types.reified.inspectable`, `.inspectable.uses` | deleted (2), merged into chapter 09 as in A |
| `annot.structure.find-lookup`, `annot.handle.fact` | reworded: state K2, and exempt the handle's target argument |
| Walker, Describer, Source | unchanged only with that exemption; otherwise `member[F]` needs `reified F` to read `Fallback[F]` |
| All other rules in section 2 | unchanged |
| `unsupported-reified-shape` (prototype) | replaced by a real descriptor in the prototype |

Net: 2 deleted, about 6 reworded, about 1 added (the error code or the
exemption).

**What users keep:** a generic fact helper with no `Inspectable` bound,
and the marker for a future reflection operation. **Cost:** the prototype
must build descriptors it has never built, for one std method.

### Option C: Keep As Is

Nothing is deleted. The rules then carry debt:

| Debt | Detail |
| --- | --- |
| A rule names a deleted operation | `types.reified.metadata`: "used by operations such as `shape::[T]()`" |
| No consumer | nothing reads the descriptor; the guide says it is "observable only as `shape::[T]()`" |
| Contradiction | `types.generic.specialized` says calls "are specialized"; `types.reified.specialize` says a backend "may" |
| No error code | `types.reified.erased`, and the guide's propagation error |
| Unused positions | variant and implementation parameters may be `reified` |
| The fact-lookup key | still unstated |

**What users keep:** the syntax, for a reflection API that does not exist.

### Other Languages

| Language | Runtime identity of a type parameter | Source |
| --- | --- | --- |
| Kotlin | `reified T`, only in `inline` functions | [Inline functions, reified type parameters](https://kotlinlang.org/docs/inline-functions.html#reified-type-parameters) |
| Haskell | a `Typeable a` constraint, a compiler-supplied dictionary | [Data.Typeable](https://hackage.haskell.org/package/base/docs/Data-Typeable.html) |
| Rust | `TypeId::of::<T>()` with a `T: 'static` bound; no marker | [std::any::TypeId](https://doc.rust-lang.org/std/any/struct.TypeId.html) |
| hd, Inspectable decision 15 | `T < Inspectable`, a compiler-supplied dictionary | [Runtime Type Identity](../spec/09-traits.md#runtime-type-identity) |

hd already took the Haskell and Rust shape: identity is a bound. Kotlin
needs `reified` because JVM generics share erased bodies and offer no
constraint dictionary. hd has the dictionary.

## 5. Recommendation

**Recommendation: option A, with K1 for the fact lookup.** Removing
`reified` deletes a syntax form, the costliest kind of change in
AGENTS.md "Design Cost Order", and 19 rules whose only remaining job is to
restrict the form itself. Every use item 87 named now has another home.
K1 states the one open key with the bound `downcast_val` already uses, and
matches the prototype.

Under K1, `typed-fact.hd` needs `F < Display & Inspectable`, or the
handle's target argument is exempt. That choice is Q1 below; it is the same
under every option.

## Questions For The Owner

### Q1. Does `h.fact::[M]()` need runtime identity for the handle's own `F`?

Effect: decides whether a walker that reads a typed fact must bound its
member type. The handle already fixes `M`'s target argument to `F`, so the
lookup can match without it.

- **A.** No: the target argument is exempt; only `M`'s other arguments need
  a key. `typed-fact.hd` stands. **Recommended.**
- **B.** Yes: `note` and the walker's `member` need `F < Inspectable`, and
  the fixture changes.

```text
fn note[S, F < Display](h: Field[S, F]) -> string:
    match h.fact::[Fallback[F]]():
        .Some(found) => "/${found.value}"
        .None => ""
```

### Q2. What lets `find::[M]()` run with a type parameter `M`?

Effect: decides whether generic fact helpers exist, now that
`facts_of(f).find::[M]()` reaches ordinary code. Concrete reads such as
`find::[Route]()` work under every answer.

- **A.** K1: `M < Inspectable`, as `downcast_val` and the prototype do.
  **Recommended.**
- **B.** K3: `M` may not mention a type parameter.
- **C.** K2: `reified M`. This keeps `reified`, option B.

```text
use std.inspect.Inspectable
use std.structure.Facts

fn fact_or[M < Inspectable](facts: Facts, fallback: M) -> M:
    match facts.find::[M]():
        .Some(found) => found
        .None => fallback
```

### Q3. Remove `reified`?

Effect: 19 numbered rules, a contextual word, and two grammar alternatives
go. Every generic parameter is erased, and `[reified T]` becomes a
`syntax-error`. Q2 answer C implies answer B here.

- **A.** Remove it entirely. **Recommended.**
- **B.** Keep it, narrowed to the fact lookup, on functions and methods.
- **C.** Keep it as it is.

```text
trait Lookup:
    fn find[M < AnyRef](self) -> M?

fn keep[T](value: T) -> T:
    value
```

## Parse Log

Every `text` block above was parsed with `parseSource` from
`spec/reference-parser/parser.ts`. Parsing checks syntax only; no block is
claimed to type-check. `facts_of` is batch 42's name, not yet in a chapter;
it is an ordinary call, so no line is marked hypothetical.

| Block | Where | Result |
| --- | --- | --- |
| 1 | Option A, Before | parses |
| 2 | Option A, After | parses |
| 3 | Option A, runtime identity | parses |
| 4 | Option A, fact lookup with K1 | parses |
| 5 | Option A, batch 42's read | parses |
| 6 | Option A, meaning change | `syntax-error@1` today, by `lex.contextual.reified.lone`; it parses once option A drops that check |
| 7 | Option A, soundness | parses; the `type-mismatch` is a type error |
| 8 | Option B, After | parses |
| 9 | Q1 | parses |
| 10 | Q2 | parses |
| 11 | Q3 | parses |
