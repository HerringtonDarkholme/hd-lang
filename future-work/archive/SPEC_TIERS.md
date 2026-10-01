# Spec Tiers: Language Spec And Stdlib Spec

> **Archived 2026-10-01.** Every decision in this record is applied or
> superseded, and the [specification](../../spec/README.md) is
> authoritative. The record is kept as history, so its examples and rule
> IDs may describe retired rules.

Status: migration complete. The owner accepted every recommendation on
2026-09-30 ([Owner Decisions](#owner-decisions)), and migration steps 1
to 11 are done. Nothing here is accepted behavior until a move task
puts it in the spec. Step 4 moved Property Tests and Draw Budget to
[Testing](../../spec/std/testing.md), step 5 moved Derived Arbitrary, and
step 6 moved the `timeout` option's effect, table-test rows, and snapshot
files there. Step 7 moved the iterator adapters and collect targets to
[Iterators](../../spec/std/iter.md), and step 8 moved the string methods
above the intrinsics and `r` to [Text](../../spec/std/text.md). Step 9
moved `debug`'s text and the Debug builders to [Format](../../spec/std/format.md),
and `Duration` and its suffixes to [Time](../../spec/std/time.md). Step 10
left the retry combinator in the language tier, and step 11 cleaned the
language chapters' examples and links. Batch 29 then moved `retry!` to
[Task](../../spec/std/task.md) and the registration of `it_each`, `it_prop`,
and `it_prop_with` to Testing ([Still Open](#still-open)). It answers
the owner's request of 2026-09-30:

> is Arbitrary/stdlib stuff should be inside language spec? if they can be
> implemented outside compiler. i would suggest have tiers of spec, language
> spec + stdlib spec, and move conformance accordingly.

Under review: the AGENTS.md rule
[Spec Scope For The Standard Library](../../AGENTS.md#spec-scope-for-the-standard-library),
[Standard Testing](../../spec/10-modules.md#standard-testing),
[Built-In Methods](../../spec/10-modules.md#built-in-methods),
[Iterator Adapters](../../spec/std/iter.md#iterator-adapters),
[Debug Trait](../../spec/09-traits.md#debug-trait),
[Rule IDs](../../spec/STYLE.md#rule-ids), and the
[conformance format](../../spec/conformance/README.md).

## Contents

1. [Problem](#problem)
2. [Tier Criteria](#tier-criteria)
3. [Inventory](#inventory)
4. [Layout](#layout)
5. [Conformance Split](#conformance-split)
6. [Tooling And Process](#tooling-and-process)
7. [Migration Plan](#migration-plan)
8. [Owner Decisions](#owner-decisions)
9. [Still Open](#still-open)

## Problem

The one spec mixes three kinds of text:

| Kind | Example | Needs the compiler? |
| --- | --- | --- |
| Language rules | layout, typing, `mut`, matching | yes |
| Intrinsics and lang items | `Iterable` driving `for`, `Termination`, `all!` | yes, by name |
| Std APIs written in hd | `Choices.list`, `split`, `filter` | no |

AGENTS.md already limits the third kind (owner direction, 2026-09-28):

> The spec names a std item only when the language needs it:
> - the compiler or runtime gives it support;
> - it is in the prelude;
> - or syntax refers to it, such as a literal form.

The spec exceeds that rule in several places. About 130 of its 3,996 rule
IDs cover std behavior that ordinary hd could implement. Of its 1,619
fixtures, 87 use such an item, and about 40 cite its section.

| Section | Rule IDs | Why it exceeds the rule |
| --- | --- | --- |
| [Property Tests](../../spec/std/testing.md#property-tests), [Draw Budget](../../spec/std/testing.md#draw-budget) | `module.testing.choices.*`, `.prop.*`, `.budget.*` | `Choices`, `Arbitrary`, discard limits, and regression files are runner and library behavior, not prelude |
| [Derived Arbitrary](../../spec/std/testing.md#derived-arbitrary) | `module.testing.arbitrary.*` | [`std-testing.arbitrary.derive.template`](../../spec/std/testing.md#r-std-testing.arbitrary.derive.template) says the compiler supplies nothing for it |
| [String Methods](../../spec/std/text.md#string-methods) | `module.string.*` | `trim`, `lower`, `split`, `replace`, `starts_with` are std code over bytes |
| [Iterator Adapters](../../spec/std/iter.md#iterator-adapters) | `flow.adapter.*`, `flow.collect.*` | [`std-iter.adapter.methods`](../../spec/std/iter.md#r-std-iter.adapter.methods) calls them ordinary methods |
| [Debug Builders](../../spec/std/format.md#debug-builders) | `trait.debug.builder.*`, `.derive-builders.*` | `DebugStruct` and its layout are not prelude names |
| [Snapshot Files](../../spec/std/testing.md#snapshot-files), [Table-Test Rows](../../spec/std/testing.md#table-test-rows) | `module.testing.snapshot*`, `.it-each.*` | file paths, update runs, and row names are runner behavior |
| [Duration Suffixes](../../spec/std/time.md#duration-suffixes), [Raw Text Prefix](../../spec/std/text.md#raw-text-prefix) | `expr.suffix.std.*`, `expr.prefix.std.*` | `ms`, `s`, and `r` are ordinary functions |

Go and Rust keep these apart. The
[Go spec](https://go.dev/ref/spec) defines built-ins such as `len` and
`append` but no std package; the [`testing` package](https://pkg.go.dev/testing)
and [modules](https://go.dev/ref/mod) have their own documents. The
[Rust Reference](https://doc.rust-lang.org/reference/special-types-and-traits.html)
lists only the lang items the compiler knows, and
[`std`](https://doc.rust-lang.org/std/) is documented separately. Rust's
Reference also specifies the [`#[test]` attribute](https://doc.rust-lang.org/reference/attributes/testing.html),
while the test harness stays outside it.

## Tier Criteria

**Language tier.** Syntax, static and dynamic semantics, intrinsics, and
anything the compiler must know by name:

- lang items: `Iterable` for `for`, `std.process.Termination`, the `std.ops`
  operator traits, `Index` and `IndexSet`, `Num`, `Integer`, `Float`;
- `Eq`, `Hash`, `PartialOrd`, `Ord`, `Display`, `Debug`, `From`, `Error` with
  `@error`, `Inspectable`, `TypeId`;
- `std.structure` bodies and `SelfRef` computation, `shape`, `shape_of`;
- `$.use`, `$.with`, `Suspend`, `all!`, `race!`, `block_on`;
- the representation intrinsics of built-in types, such as `List.append`,
  `Map.get`, `string.len`, `string.bytes`, and `string.slice`;
- test registration that the compiler checks, and the prelude's names and
  signatures;
- the **conformance harness**: `it`, `assert`, `assert_equal`, and `println`,
  through which the language suite observes results.

**Stdlib tier.** APIs that `lib/std` can write in ordinary hd over the
language tier: `Arbitrary`, `Choices`, the runner's property-test behavior,
string and collection methods above the intrinsics, `std.text`, iterator
adapters, `FromIterator` and its impls, `std.time`, and `Debug`'s builders.

### The Test

> Could `lib/std` implement the item in ordinary hd, over language-tier
> items only, with no compiler knowledge of its name, and keep the same
> observable behavior?

Yes puts it in the stdlib tier. A diagnostic code about it, a language rule
that names it, or a position rule that lists it each mean no. This is the
owner's own wording, "implemented outside compiler".

The stricter test, "could a third-party package provide it?", is not
proposed. Only `std` may add inherent methods to built-in types
([`trait.own.inherent.std`](../../spec/09-traits.md#r-trait.own.inherent.std)),
so that test would pull every string method into the language tier.

### Borderline Items

| Item | Compiler knows it? | Tier |
| --- | --- | --- |
| `assert_equal` | yes: `missing-eq` ([`module.testing.no-implicit-eq`](../../spec/10-modules.md#r-module.testing.no-implicit-eq)) | language; also harness |
| `it` | yes: prelude name, literal name check | language |
| `it_each`, `it_prop`, `it_prop_with` registration | yes: test registration functions under [`module.testing.position-statements`](../../spec/10-modules.md#r-module.testing.position-statements) | stdlib by the owner's decision ST6, revised (batch 29); the test-position rules stay language |
| `snapshot`'s literal `expect` | yes: `non-literal-test-argument` | language for the check; stdlib for files and update runs |
| `it`'s `timeout` option | no: an ordinary `Duration` argument | stdlib; the runner enforces it |
| `Iterator[T]`, `from_fn`, `next` | yes: `Iterable.iter` returns it | language |
| `filter`, `map`, `fold`, `collect` | no | stdlib |
| `Debug`, `debug` | yes: prelude; `assert_equal` needs it | language |
| `DebugWriter` builders, layout, derived mapping | no | stdlib |
| `println`, `Console` | prelude, host capability, harness | language |
| `Duration`, `ms`, `s`, `r` | no: ordinary suffix and prefix functions | stdlib |
| retry combinator in `std.task` | no: `retry!` is a library loop over a `fn!` attempt (RETRY, batch 29) | stdlib: [Task](../../spec/std/task.md#retry) |
| derived `Arbitrary` | no, by its rules; the prototype still needs the checker | stdlib; the two gaps below are settled |

### Gaps Found By The Prototype

The SR1 pass (commit `06ad949b`) moved derived `Arbitrary` into a
`std.testing` template, as `std-testing.arbitrary.derive.template`
requires. Two rules could not be written that way:

| Gap | Cause | Kind |
| --- | --- | --- |
| `arbitrary.with` | [`annot.walker.obligation`](../../spec/14-annotations.md#r-annot.walker.obligation) makes every member meet the source's `member[F < Arbitrary]` bound. A tuned member need not implement `Arbitrary`. | language-tier gap: templates have no per-member bound that a fact discharges |
| enum name in the panic | `VariantInfo` and `Members` carry no type name, and the template's `T` is not `reified`, so `shape[T]()` is out | language-tier gap: `Structure` exposes no type name; ST8 adds `name()` |

The prototype keeps a checker path for a target with an `arbitrary.with`
fact. Neither gap keeps derived `Arbitrary` in the language tier: its rules
are std behavior, and the first gap belongs to the template machinery. ST7
and ST8 in [Owner Decisions](#owner-decisions) settle both, and step 5
applied them:

| Gap | Settled by |
| --- | --- |
| `arbitrary.with` | Testing AT-with (option B): every member must implement `Arbitrary` and be inspectable, tuned or not ([`std-testing.arbitrary.derive.member-bounds`](../../spec/std/testing.md#r-std-testing.arbitrary.derive.member-bounds)). Member-typed facts, which would lift the bound from a tuned member, are a future option in [Open Issues](../OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets). |
| enum name in the panic | [`annot.structure.name`](../../spec/14-annotations.md#r-annot.structure.name) and [`std-testing.arbitrary.derive.no-finite.message`](../../spec/std/testing.md#r-std-testing.arbitrary.derive.no-finite.message) |

## Inventory

A chapter not listed stays whole in the language tier: 01 to 04, 07, 08,
12, 13, and 14. Chapters 01, 03, and 04 have examples that import
`std.time.s` or `std.text.r`. Each such example declares its own suffix or
prefix instead, so the rule it shows stays language-tier.

| Chapter | Section or rule group | Tier | Target |
| --- | --- | --- | --- |
| [05](../../spec/05-expressions.md) | everything else, incl. suffix and prefix mechanisms | language | stays |
| 05 | `expr.suffix.std.*`, `expr.prefix.std.*` | stdlib | `std/time.md`, `std/text.md` |
| [06](../../spec/06-control-flow.md) | For Loops, Iteration Protocols, Built-In Collection Iteration, Iterator Invalidation, the rest | language | stays |
| 06 | Iterator Adapters, Collect Targets (`flow.adapter.*`, `flow.collect.*`) | stdlib | `std/iter.md` |
| [09](../../spec/09-traits.md) | all but the next row, incl. every `trait.debug.*` rule except `.render` | language | stays |
| 09 | Debug Builders, derived builder mapping, `trait.debug.render` format | stdlib | `std/format.md` |
| [10](../../spec/10-modules.md) | Package Manifest to Dependency Cycles, Module Initialization, Public Uses, Name Resolution, Executable Entry Point, Wasm Boundary | language | stays |
| 10 | Prelude, Collection Type Names, Prelude Functions, Console, Value-Category Traits | language | stays, less the three import rules below |
| 10 | `module.prelude.time-suffixes`, `.text-r`, `.from-iterator` | stdlib | with their modules |
| 10 | Built-In Methods: `len`, `iter`, `append`, `get`, `remove`, `to_string`; Map Complexity | language | stays |
| 10 | Built-In Methods: `List.map`, optional `map` | stdlib | `std/iter.md` |
| 10 | String Methods: `module.string.utf8`, `.byte-offsets`, `.chars`, `.char-indices`, `.bytes`, `.slice.*` | language | stays; step 8 kept `chars` and `char_indices`, because [`flow.for.string-explicit`](../../spec/06-control-flow.md#r-flow.for.string-explicit) names them |
| 10 | String Methods: `lower`, `trim`, `split`, `replace`, `starts_with` | stdlib | `std/text.md` |
| 10 | Standard Testing exports, Test Cases (less `timeout`), Test Outcomes | language | stays |
| 10 | `timeout` option rules | stdlib | `std/testing.md` |
| 10 | Table Tests: `module.testing.it-each.import`, `.it-each.body-closure`, `.it-each.name-clash`, `.it-prop`, `.it-prop.import`, `.variants.*` | stdlib (ST6, revised) | `std/testing.md`, batch 29 |
| 10 | Table Tests: `it-each` rows, names, run timing | stdlib | `std/testing.md` |
| 10 | Property Tests, Draw Budget, Derived Arbitrary | stdlib | `std/testing.md` |
| 10 | Snapshots: the `snapshot` signature and `module.testing.snapshot.literal` | language | stays |
| 10 | Snapshots: the rest | stdlib | `std/testing.md` |
| [11](../../spec/11-requirements-and-suspension.md) | everything but the next row | language | stays |
| 11 | `req.combinator.retry` | stdlib (RETRY) | `std/task.md` as `std-task.combinator.retry`, batch 29 |
| [README](../../spec/README.md) | diagnostics, glossary, Revision Notes | language | stays; moved terms' glossary rows move |

Every diagnostic code stays in the README table: each is a compiler check.
The panic categories stay in chapter 06.

## Layout

```
spec/
  README.md          language spec index; links to std/README.md
  01-...14-*.md      language chapters, unchanged names
  STYLE.md           one style guide for both tiers
  conformance/       one suite, both tiers
  std/
    README.md        stdlib scope, the tier test, chapter table
    format.md        prefix std-format
    iter.md          prefix std-iter
    text.md          prefix std-text
    time.md          prefix std-time
    testing.md       prefix std-testing
    task.md          prefix std-task, only if the retry combinator moves
```

- **Where.** `spec/std/`, not a top-level `stdlib-spec/`. The conformance
  README promises that an implementation needs nothing outside `spec/`, and
  `check-spec-anchors.ts` already walks subdirectories.
- **Names.** One file per std module, unnumbered. Module names are stable,
  and a stdlib reader looks things up by module, as in Go's package docs.
- **Rule IDs.** Follow [Stability](../../spec/STYLE.md#stability): a rule that
  moves chapters is retired and gets the new prefix. Keep the tail, so
  `module.testing.prop.debug` becomes `std-testing.prop.debug`. The
  Revision Notes entry states the mapping as one pattern per group. The
  retired IDs are deleted, not listed.
- **Cross-links.** A stdlib chapter may cite any language rule. A language
  chapter may link to `std/` only from a Note or See also, never from a
  numbered rule, so no language rule depends on stdlib behavior.
- **Website.** `website/src/pages.ts` gains a "Standard Library" nav section
  after "Reference", with pages at `spec/std/<module>.html`.
- **Revision Notes.** One log in `spec/README.md`; each entry names its tier.

## Conformance Split

- **Tier marker.** No new column, directive, or directory. A case's tier is
  the tier of its `specification` column: a path under `std/` is stdlib.
  Fixture directories stay informative, as today.
- **Self-containment.** The conformance README's list of usable std modules
  splits in two. A language-tier fixture may use only language-tier std
  items. A stdlib fixture may use both tiers.
- **Check.** `spec/check.sh` rejects a language-tier fixture that uses a
  stdlib item, by matching its `use std.` lines against the stdlib list.
  That list starts empty, and each move task adds its names.
- **Fixtures that cross tiers.** About 45 fixtures cite a language section
  but use a stdlib item, such as `use std.time.s`. Each drops the item, or
  cites a stdlib section when the item is its point.
- **`cases.tsv` and `examples.tsv`.** No format change. Moved cases get a
  new `specification` value, and std chapter examples get rows keyed
  `std/<module>.md`.
- **`test/portable/cases.tsv`, `KNOWN_FAILURES.tsv`.** No format change. The
  tier comes from joining on `path` with `spec/conformance/cases.tsv`. None
  of today's 29 known failures is stdlib-tier.
- **Audit counts.** [audit/README.md](../../audit/README.md#conformance)
  reports passes per tier, as "language: X of Y; stdlib: X of Y".
- **Runners.** `spec/tools/run-conformance.ts` and `test/run-portable.ts`
  take `--tier language|std`. Selection manifests still narrow further.

**An alternative std should run the stdlib suite alone:**
`--tier std`. A new compiler with only the harness in its std should run
`--tier language` and see no stdlib dependency.

## Tooling And Process

| File | Change |
| --- | --- |
| `spec/check.sh` | fence, example-inventory, and anchor loops cover `std/*.md`; the language-fixture name check |
| `spec/tools/spec-prose.ts` | `CHAPTER_PREFIXES` keys `std/<module>.md` to `std-<module>` |
| `spec/check-spec-style.ts` | lints `std/*.md` as chapters |
| `spec/tools/rule-inventory.ts` | its chapter glob adds `spec/std/*.md`, so a moved ID is not reported lost twice |
| `spec/tools/run-conformance.ts` | `--tier` |
| `spec/tools/fuzz/*` | none: the fuzzer's oracles are the parser and the grammar |
| reference parser | none: no tier adds syntax |
| `spec/STYLE.md` | the prefix table gains the stdlib rows |
| `spec/conformance/README.md` | a Tiers section; the split self-containment list |
| `.agents/skills/spec-update/SKILL.md` | step 3 picks the tier by the test, then the chapter prefix; step 5 checks the fixture's tier |
| AGENTS.md "Spec Scope" | rewritten as the two tiers and the test |
| future-work/STDLIB.md | "What The Specification Already Names" gains a tier column; STDLIB keeps only undecided std design |
| `website/src/pages.ts` | the stdlib nav section |
| `test/run-portable.ts`, `test/portable/README.md` | `--tier` |

The new AGENTS.md rule, in full:

> The language spec names a std item only when the compiler must know it: a
> lang item, an intrinsic, a prelude name, or the conformance harness. Any
> other std API that the owner decides lives in `spec/std/`. Undecided std
> design lives in future-work/STDLIB.md.

## Migration Plan

Each task is about an hour, and each ends with `bash spec/check.sh`,
`pnpm run website:build`, and a push. Spec comes first; `src/` and
`lib/std` comments change only to fix links.

| # | Task | Moves |
| --- | --- | --- |
| 1 | Done. Scaffold: `spec/std/README.md`, prefix table, `spec-prose.ts`, style lint, `check.sh` loops, rule-inventory glob, `hd explain` index, website nav | nothing |
| 2 | Done. Process: AGENTS.md rule, spec-update skill, STDLIB.md tier column | nothing |
| 3 | Done. Conformance plumbing: README Tiers section, split list, `check.sh` name check (`spec/check-spec-tiers.ts`, `stdlib-items.tsv`), `--tier` in both runners; `tier-crossings.tsv` records 37 crossing fixtures | nothing |
| 4 | Done. `std/testing.md`, part 1: Property Tests and Draw Budget, 37 IDs; `std.testing.Arbitrary` and `.Choices` in `stdlib-items.tsv`; the 7 Derived Arbitrary fixtures recorded in `tier-crossings.tsv` until task 5 | 6 cases |
| 5 | Done. `std/testing.md`, part 2: Derived Arbitrary, 16 IDs moved and one retired, with Testing AT-with; `Structure.name()` and the no-finite message (ST8); Self References restated (SIMPLE); the `arbitrary.with` gap and option D in [Open Issues](../OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets) (ST7); `std.testing.arbitrary` in `stdlib-items.tsv`; the 7 crossing rows removed; four questions in [Open Issues](../OPEN_ISSUES.md#applied-decisions) | 7 cases, and 5 new cases |
| 6 | Done. `std/testing.md`, part 3: Test Timeout, Table-Test Rows, Snapshot Files, 18 IDs; `std.testing.snapshot_file` in `stdlib-items.tsv`; the three `timeout` and `snapshot_file` crossing rows removed | 6 cases, and 1 new case split from two language cases |
| 7 | Done. `std/iter.md`: adapters, collect targets, `FromIterator`, `List.map`, optional `map`, 32 IDs; `std.iter.FromIterator` in `stdlib-items.tsv`; five `List.map`, `map`, and `collect` fixtures rewritten over local helpers or a `for` loop, and their crossing rows removed | 13 cases, and 1 new case split from `try-operand-expected-type.hd` |
| 8 | Done. `std/text.md`: `trim`, `lower`, `split`, `replace`, `starts_with`, and `r`, 12 IDs; `chars` and `char_indices` stay language tier; `std.text.r` in `stdlib-items.tsv`; the 23 text crossing rows removed: 8 cases now cite `std/text.md`, 13 fixtures use language-tier operations or a local prefix instead, and 2 need nothing | 11 cases |
| 9 | Done. `std/format.md`: `debug`'s text, the Debug builders and layout, and the derived builder mapping, 17 IDs; `std/time.md`: `Duration`, its API, and the `ms`, `s`, `min`, and `h` suffixes, 11 IDs; the ch01, 03, 04, and 05 examples declare local suffixes and prefixes; `std.time` and the four `std.format` builder types in `stdlib-items.tsv`; the 6 `std.time` crossing rows removed, which empties `tier-crossings.tsv`; the `Duration` reading in [Still Open](#still-open) | 5 cases, and 1 new case split from `literal-suffix-duration.hd` |
| 10 | Done. The retry combinator stays in the language tier: the tier test turns on its undecided signature ([Still Open](#still-open)) | 0 |
| 11 | Done. Cleanup: the language chapters' examples in 01, 05, 07, and 09 use language-tier operations or a local helper, with the fixtures they index; `lex.raw-string.none-text` retired for `lex.raw-string.prefix`; the parse-phase reading in the conformance README Tiers section; a stdlib [Glossary](../../spec/std/README.md#glossary); links in `lib/std` and `src/` comments; per-tier counts in [audit/README.md](../../audit/README.md#conformance) | nothing |

"Cases" counts rows whose primary section moves. Each move task edits the
spec text, the IDs, `cases.tsv`, `examples.tsv`, and the links in `spec/`
and `guide/`, and adds one Revision Notes entry. It also adds its names to
the stdlib list and fixes the cross-tier fixtures that the check flags.
It deletes each fixed fixture's row from `spec/conformance/tier-crossings.tsv`.
About 140 links point at moved anchors today, 35 of them in `cases.tsv`.

## Owner Decisions

The owner accepted all eight recommendations on 2026-09-30. The decisions
are final.

| # | Decision | Was |
| --- | --- | --- |
| ST1 | The stdlib spec lives in `spec/std/`, one unnumbered file per module. The website gets a "Standard Library" nav section. | Q4 |
| ST2 | A moved rule is re-IDed with a `std-<module>` prefix that keeps the tail: `module.testing.prop.debug` becomes `std-testing.prop.debug`. The old IDs are deleted. | Q2 |
| ST3 | A moved heading is deleted, and every link to it is fixed in the same task. No stub headings. | Q3 |
| ST4 | A fixture's tier comes from its `specification` column. The runners get `--tier language\|std`. `spec/check.sh` stops a language-tier fixture from using a stdlib item. | Q5 |
| ST5 | `it`, `assert`, `assert_equal`, and `println` stay in the language tier. | Q6 |
| ST6 | `it_each`, `it_prop`, and `it_prop_with` registration stays in the language tier. Rows, generation, shrinking, and the draw budget move to the stdlib tier. Revised by ST6, revised, in [Still Open](#still-open): registration moves too. | Q7 |
| ST7 | Derived `Arbitrary` moves to the stdlib tier now. The `arbitrary.with` template gap is recorded in [Open Issues](../OPEN_ISSUES.md). | Q8 |
| ST8 | Revised by the owner, below: the no-finite panic message keeps naming the type, through a new `Structure.name()`. | Q1 |

**ST8, revised.** The owner revised ST8 on 2026-09-30. The rule that the
no-finite panic message names the type stays. `std.structure`'s
`Structure` gains a receiverless, compiler-supplied `fn name() -> string`
next to `facts()`.

| Point | Decision |
| --- | --- |
| Value | The declared name only: no module path and no type arguments. |
| Use | A compile-time constant, usable only inside templates. It is not a runtime reflection hook; runtime type information stays opt-in through `Inspectable`. |
| Newtype | A newtype has its own name. |
| Alias | A transparent alias has no structure of its own; it uses its base's. |
| Message | The derived `Arbitrary` template panics with `"${T::name()} has no finite value"`. |
| Known cost | Once codecs emit the name, renaming a type changes their output, as with serde or Go. Because it is the declared name only, moving a file does not change it. |

`Structure.name()` is a language-tier addition: `std.structure` bodies are
compiler-supplied. Step 5 applied it in
[The Structure Trait](../../spec/14-annotations.md#the-structure-trait).
Batch 21 (2026-09-30) accepted that a newtype, which gets no `Structure`
today, panics with its base's name, and resolved a clash with a trait's
own `facts` or `name` by qualifying the call
([Open Issues](../OPEN_ISSUES.md#applied-decisions)).

## Still Open

The owner answered both items below in batch 29 (2026-09-30), and asked
for a third move, ST6, revised. All three are applied; the
[Revision Notes](../../spec/README.md#revision-notes) list them.

| # | Decision | Where |
| --- | --- | --- |
| DUR | Option A: the language-tier harness signature names the stdlib type `std.time.Duration`, in a Note. No language rule depends on its values. | the Note in [Test Cases](../../spec/10-modules.md#test-cases), now for `it` alone |
| ST6, revised | The owner: "also move it_prop to stdlib, it_each as well, unless they are used in spec conformance test". `it_each`, `it_prop`, and `it_prop_with`, their signatures and registration rules, move to the stdlib tier. `it`, `assert`, `assert_equal`, and `println` stay (ST5). No language-tier fixture needs them: the five language-tier fixtures that used `it_each` tested the moved rules and now cite `std/testing.md`, so the move is complete. | [Registration Functions](../../spec/std/testing.md#registration-functions) |
| RETRY | `retry!` is a plain library loop: `pub fn retry![T, E](times: i32, attempt: fn!() -> Result[T, E]) -> Result[T, E]`, with a row parameter since batch 33. Cancellation follows the ordinary rules. `req.combinator.retry` moves to the new `spec/std/task.md` as `std-task.combinator.retry`, with the new prefix `std-task`. | [Task](../../spec/std/task.md#retry) |

**Still open from applying batch 29.** The specification applies the
reading in the middle column; each point asks the owner to confirm it.

| # | Question | Applied reading and **Recommendation** |
| --- | --- | --- |
| ST6-hook | The language tier must still let the three functions stand in test position, without naming a stdlib item in a numbered rule. | `module.testing.position-statements` names `it` and "the registration functions that `std.testing` declares in the stdlib tier"; [`std-testing.registration`](../../spec/std/testing.md#r-std-testing.registration) lists them. **Recommendation:** keep it. The compiler still knows the three names, which the tier test would call language tier; the owner's decision outranks it. **Confirmed (owner, batch 33, 2026-10-01).** |
| RETRY-zero | The decision does not say what `retry!(0, f)` or a negative `times` returns. With no attempt there is no `Result` to return. | Unstated. **Recommendation:** a `times` below 1 makes one attempt, as a loop `for i in 0..max(times, 1)` would; a panic is the alternative. **Applied (owner, batch 33, 2026-10-01)** in 33b, as recommended: [`std-task.combinator.retry.at-least-once`](../../spec/std/task.md#r-std-task.combinator.retry.at-least-once). |
| RETRY-row | The decided `attempt: fn!() -> Result[T, E]` has an empty requirement row, so an attempt cannot use a provider such as `$ Http`. | Signature as decided. **Recommendation:** add a row parameter, `attempt: fn!() -> Result[T, E] $ R` and `-> Result[T, E] $ R`, as `it` has. **Applied (owner, batch 33, 2026-10-01)** in 33b, as recommended: [Retry](../../spec/std/task.md#retry). |

The reasons each item was open follow, for the record.


### Duration In The Test Signatures

The language tier's signatures of `it`, `it_each`, `it_prop`, and
`it_prop_with` name `timeout: Duration?`. Step 9 moved `Duration` to
[Time](../../spec/std/time.md#duration).

| Option | Language-tier text | Kinds of change |
| --- | --- | --- |
| A. The harness signatures name a std type | a Note in [Test Cases](../../spec/10-modules.md#test-cases) says `Duration` is the stdlib-tier `std.time.Duration`, named in these signatures only | none |
| B. `Duration` becomes a lang item | a language rule declares `std.time.Duration` and its representation | compiler intrinsic |
| C. `timeout` takes a language-tier type | the signatures change, for example to milliseconds in an `i64` | a design change, with every `timeout` fixture |

**Reading applied (option A).** The language tier names `Duration` as the
parameter type and specifies nothing else about it. No language rule
depends on its values: `module.testing.it.options-strings` checks only
`ignore` and `expect_panic`, and `timeout="5s"` fails as any string
argument to a non-string parameter does. What the option does is already
stdlib tier ([Test Timeout](../../spec/std/testing.md#test-timeout)). The
same reading already covers `Arbitrary` and `Choices` in the `it_prop` and
`it_prop_with` signatures, which step 4 moved. It is consistent with ST5,
since `it` stays language tier, and with ST6, since registration stays
while generation moves.

**Recommendation.** Keep option A. It needs no new rule and no compiler
knowledge of `Duration`. A compiler with only the language-tier std must
still declare some `std.time.Duration` for these signatures to resolve.
That is one data type, and the language suite never builds a value of it.

### The Retry Combinator

`req.combinator.retry`, now [`std-task.combinator.retry`](../../spec/std/task.md#r-std-task.combinator.retry),
stayed in the language tier at step 10, since the tier test did not clearly move it:

| Point | Effect |
| --- | --- |
| Signature | None is decided. [`req.combinator.library-rest`](../../spec/11-requirements-and-suspension.md#r-req.combinator.library-rest) leaves it, and the complete intrinsic set, to std design. |
| Candidate | [STDLIB's draft](../STDLIB.md) `retry!` takes `attempt: fn() -> mut Suspend[Result[T, E]]`, so it drives `Suspend` values, as `all!` and `race!` do. |
| Intrinsic? | [`req.combinator.intrinsic`](../../spec/11-requirements-and-suspension.md#r-req.combinator.intrinsic) makes the polling combinators intrinsics, and [Open Issues](../OPEN_ISSUES.md) lists retry among the compiler-intrinsic `std.task` combinators. |
| A `fn!` loop | A retry whose attempt is a `fn!` body is a loop of bang calls. Its cancellation rule then follows from [Cancellation](../../spec/11-requirements-and-suspension.md#cancellation) alone. |

So the answer turns on a signature the owner has not chosen. Moving the
rule now would also make `std/task.md` a chapter for one rule about an
API with no specified signature.

**Recommendation.** Leave the rule where it is. When the owner decides
`retry!`'s signature, apply the tier test to it: a `fn!` body moves the
rule to `std/task.md` as `std-task.combinator.retry`, and a `Suspend`
constructor keeps it beside `all!` and `race!`.
