# Spec Tiers: Language Spec And Stdlib Spec

Status: decided plan. The owner accepted every recommendation on
2026-09-30 ([Owner Decisions](#owner-decisions)), and migration steps 1
to 4 and 6 are done. Nothing here is accepted behavior until a move task
puts it in the spec. Step 4 moved Property Tests and Draw Budget to
[Testing](../spec/std/testing.md), and step 6 moved the `timeout` option's
effect, table-test rows, and snapshot files there; nothing else has moved
yet. It answers
the owner's request of 2026-09-30:

> is Arbitrary/stdlib stuff should be inside language spec? if they can be
> implemented outside compiler. i would suggest have tiers of spec, language
> spec + stdlib spec, and move conformance accordingly.

Under review: the AGENTS.md rule
[Spec Scope For The Standard Library](../AGENTS.md#spec-scope-for-the-standard-library),
[Standard Testing](../spec/10-modules.md#standard-testing),
[Built-In Methods](../spec/10-modules.md#built-in-methods),
[Iterator Adapters](../spec/06-control-flow.md#iterator-adapters),
[Debug Trait](../spec/09-traits.md#debug-trait),
[Rule IDs](../spec/STYLE.md#rule-ids), and the
[conformance format](../spec/conformance/README.md).

## Contents

1. [Problem](#problem)
2. [Tier Criteria](#tier-criteria)
3. [Inventory](#inventory)
4. [Layout](#layout)
5. [Conformance Split](#conformance-split)
6. [Tooling And Process](#tooling-and-process)
7. [Migration Plan](#migration-plan)
8. [Owner Decisions](#owner-decisions)

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
| [Property Tests](../spec/std/testing.md#property-tests), [Draw Budget](../spec/std/testing.md#draw-budget) | `module.testing.choices.*`, `.prop.*`, `.budget.*` | `Choices`, `Arbitrary`, discard limits, and regression files are runner and library behavior, not prelude |
| [Derived Arbitrary](../spec/10-modules.md#derived-arbitrary) | `module.testing.arbitrary.*` | [`module.testing.arbitrary.derive.template`](../spec/10-modules.md#r-module.testing.arbitrary.derive.template) says the compiler supplies nothing for it |
| [String Methods](../spec/10-modules.md#string-methods) | `module.string.*` | `trim`, `lower`, `split`, `replace`, `starts_with` are std code over bytes |
| [Iterator Adapters](../spec/06-control-flow.md#iterator-adapters) | `flow.adapter.*`, `flow.collect.*` | [`flow.adapter.methods`](../spec/06-control-flow.md#r-flow.adapter.methods) calls them ordinary methods |
| [Debug Builders](../spec/09-traits.md#debug-builders) | `trait.debug.builder.*`, `.derive-builders.*` | `DebugStruct` and its layout are not prelude names |
| [Snapshot Files](../spec/std/testing.md#snapshot-files), [Table-Test Rows](../spec/std/testing.md#table-test-rows) | `module.testing.snapshot*`, `.it-each.*` | file paths, update runs, and row names are runner behavior |
| [Literal Suffixes](../spec/05-expressions.md#literal-suffixes), [Prefixed Strings](../spec/05-expressions.md#prefixed-strings) | `expr.suffix.std.*`, `expr.prefix.std.*` | `ms`, `s`, and `r` are ordinary functions |

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
([`trait.own.inherent.std`](../spec/09-traits.md#r-trait.own.inherent.std)),
so that test would pull every string method into the language tier.

### Borderline Items

| Item | Compiler knows it? | Tier |
| --- | --- | --- |
| `assert_equal` | yes: `missing-eq` ([`module.testing.no-implicit-eq`](../spec/10-modules.md#r-module.testing.no-implicit-eq)) | language; also harness |
| `it` | yes: prelude name, literal name check | language |
| `it_each`, `it_prop`, `it_prop_with` registration | yes: [`module.testing.position-statements`](../spec/10-modules.md#r-module.testing.position-statements) lists them | language for registration; stdlib for rows, generation, shrinking |
| `snapshot`'s literal `expect` | yes: `non-literal-test-argument` | language for the check; stdlib for files and update runs |
| `it`'s `timeout` option | no: an ordinary `Duration` argument | stdlib; the runner enforces it |
| `Iterator[T]`, `from_fn`, `next` | yes: `Iterable.iter` returns it | language |
| `filter`, `map`, `fold`, `collect` | no | stdlib |
| `Debug`, `debug` | yes: prelude; `assert_equal` needs it | language |
| `DebugWriter` builders, layout, derived mapping | no | stdlib |
| `println`, `Console` | prelude, host capability, harness | language |
| `Duration`, `ms`, `s`, `r` | no: ordinary suffix and prefix functions | stdlib |
| retry combinator in `std.task` | no, if a `fn!` loop of bang calls expresses it | stdlib; check in task 10 |
| derived `Arbitrary` | no, by its rules; the prototype still needs the checker | stdlib, with two gaps below |

### Gaps Found By The Prototype

The SR1 pass (commit `06ad949b`) moved derived `Arbitrary` into a
`std.testing` template, as `module.testing.arbitrary.derive.template`
requires. Two rules still cannot be written that way:

| Gap | Cause | Kind |
| --- | --- | --- |
| `arbitrary.with` | [`annot.walker.obligation`](../spec/14-annotations.md#r-annot.walker.obligation) makes every member meet the source's `member[F < Arbitrary]` bound. A tuned member need not implement `Arbitrary`. | language-tier gap: templates have no per-member bound that a fact discharges |
| enum name in the panic | `VariantInfo` and `Members` carry no type name, and the template's `T` is not `reified`, so `shape[T]()` is out | language-tier gap: `Structure` exposes no type name; ST8 adds `name()` |

The prototype keeps a checker path for a target with an `arbitrary.with`
fact. Neither gap keeps derived `Arbitrary` in the language tier: its rules
are std behavior, and the first gap belongs to the template machinery. ST7
and ST8 in [Owner Decisions](#owner-decisions) settle both.

## Inventory

A chapter not listed stays whole in the language tier: 01 to 04, 07, 08,
12, 13, and 14. Chapters 01, 03, and 04 have examples that import
`std.time.s` or `std.text.r`. Each such example declares its own suffix or
prefix instead, so the rule it shows stays language-tier.

| Chapter | Section or rule group | Tier | Target |
| --- | --- | --- | --- |
| [05](../spec/05-expressions.md) | everything else, incl. suffix and prefix mechanisms | language | stays |
| 05 | `expr.suffix.std.*`, `expr.prefix.std.*` | stdlib | `std/time.md`, `std/text.md` |
| [06](../spec/06-control-flow.md) | For Loops, Iteration Protocols, Built-In Collection Iteration, Iterator Invalidation, the rest | language | stays |
| 06 | Iterator Adapters, Collect Targets (`flow.adapter.*`, `flow.collect.*`) | stdlib | `std/iter.md` |
| [09](../spec/09-traits.md) | all but the next row, incl. every `trait.debug.*` rule except `.render` | language | stays |
| 09 | Debug Builders, derived builder mapping, `trait.debug.render` format | stdlib | `std/format.md` |
| [10](../spec/10-modules.md) | Package Manifest to Dependency Cycles, Module Initialization, Public Uses, Name Resolution, Executable Entry Point, Wasm Boundary | language | stays |
| 10 | Prelude, Collection Type Names, Prelude Functions, Console, Value-Category Traits | language | stays, less the three import rules below |
| 10 | `module.prelude.time-suffixes`, `.text-r`, `.from-iterator` | stdlib | with their modules |
| 10 | Built-In Methods: `len`, `iter`, `append`, `get`, `remove`, `to_string`; Map Complexity | language | stays |
| 10 | Built-In Methods: `List.map`, optional `map` | stdlib | `std/iter.md` |
| 10 | String Methods: `module.string.utf8`, `.byte-offsets`, `.bytes`, `.slice.*` | language | stays |
| 10 | String Methods: `chars`, `char_indices`, `lower`, `trim`, `split`, `replace`, `starts_with` | stdlib | `std/text.md` |
| 10 | Standard Testing exports, Test Cases (less `timeout`), Test Outcomes | language | stays |
| 10 | `timeout` option rules | stdlib | `std/testing.md` |
| 10 | Table Tests: `module.testing.it-each.import`, `.it-each.body-closure`, `.it-each.name-clash`, `.it-prop`, `.it-prop.import`, `.variants.*` | language | stays |
| 10 | Table Tests: `it-each` rows, names, run timing | stdlib | `std/testing.md` |
| 10 | Property Tests, Draw Budget, Derived Arbitrary | stdlib | `std/testing.md` |
| 10 | Snapshots: the `snapshot` signature and `module.testing.snapshot.literal` | language | stays |
| 10 | Snapshots: the rest | stdlib | `std/testing.md` |
| [11](../spec/11-requirements-and-suspension.md) | everything but the next row | language | stays |
| 11 | `req.combinator.retry` | stdlib if a `fn!` loop expresses it | `std/task.md` |
| [README](../spec/README.md) | diagnostics, glossary, Revision Notes | language | stays; moved terms' glossary rows move |

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
- **Rule IDs.** Follow [Stability](../spec/STYLE.md#stability): a rule that
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
- **Audit counts.** [audit/README.md](../audit/README.md#conformance)
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
`npm run website:build`, and a push. Spec comes first; `src/` and
`lib/std` comments change only to fix links.

| # | Task | Moves |
| --- | --- | --- |
| 1 | Done. Scaffold: `spec/std/README.md`, prefix table, `spec-prose.ts`, style lint, `check.sh` loops, rule-inventory glob, `hd explain` index, website nav | nothing |
| 2 | Done. Process: AGENTS.md rule, spec-update skill, STDLIB.md tier column | nothing |
| 3 | Done. Conformance plumbing: README Tiers section, split list, `check.sh` name check (`spec/check-spec-tiers.ts`, `stdlib-items.tsv`), `--tier` in both runners; `tier-crossings.tsv` records 37 crossing fixtures | nothing |
| 4 | Done. `std/testing.md`, part 1: Property Tests and Draw Budget, 37 IDs; `std.testing.Arbitrary` and `.Choices` in `stdlib-items.tsv`; the 7 Derived Arbitrary fixtures recorded in `tier-crossings.tsv` until task 5 | 6 cases |
| 5 | `std/testing.md`, part 2: Derived Arbitrary; `Structure.name()` and the no-finite message (ST8); the `arbitrary.with` gap into [Open Issues](OPEN_ISSUES.md) (ST7) | 7 cases |
| 6 | Done. `std/testing.md`, part 3: Test Timeout, Table-Test Rows, Snapshot Files, 18 IDs; `std.testing.snapshot_file` in `stdlib-items.tsv`; the three `timeout` and `snapshot_file` crossing rows removed | 6 cases, and 1 new case split from two language cases |
| 7 | `std/iter.md`: adapters, collect targets, `FromIterator`, `List.map`, optional `map` | 13 cases |
| 8 | `std/text.md`: string methods above the intrinsics, and `r` | 4 cases |
| 9 | `std/format.md`: Debug builders and layout; `std/time.md`: `Duration` and suffixes; local suffixes and prefixes in the ch01, 03, 04, 05 examples | 4 cases, and most of the 17 fixtures that import `std.time` or `std.text` |
| 10 | Retry combinator, if a `fn!` loop expresses it; else record why it stays | 0 or 1 |
| 11 | Links: `future-work/TESTING.md` (23), `SPECIAL_CASES.md`, `RUNTIME_AND_LIBRARY.md`, `lib/std` and `src/` comments; audit per-tier counts | nothing |

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
| ST6 | `it_each`, `it_prop`, and `it_prop_with` registration stays in the language tier. Rows, generation, shrinking, and the draw budget move to the stdlib tier. | Q7 |
| ST7 | Derived `Arbitrary` moves to the stdlib tier now. The `arbitrary.with` template gap is recorded in [Open Issues](OPEN_ISSUES.md). | Q8 |
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
compiler-supplied. It is recorded here only. The spec text changes when
Derived Arbitrary moves, in task 5.
