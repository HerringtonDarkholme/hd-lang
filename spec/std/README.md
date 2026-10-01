# Standard Library Specification

This directory holds the stdlib tier of the specification. The numbered
chapters in the [parent directory](../README.md) are the language tier.
Both tiers are one specification, written to one
[Specification Style](../STYLE.md), with one conformance suite.

## Tiers

| Tier | Holds | Where |
| --- | --- | --- |
| language | syntax, static and dynamic semantics, intrinsics, and every std item the compiler must know by name | the [numbered chapters](../README.md#contents) |
| stdlib | std APIs that `lib/std` writes in ordinary hd over the language tier | this directory |

The language tier names a std item only when the compiler must know it:

- a lang item, such as `Iterable` for `for`, `std.process.Termination`, or
  a `std.ops` operator trait;
- an intrinsic, such as `List.append`, `string.len`, or `shape::[T]()`;
- a prelude name and its signature;
- the test-position rules that the compiler checks;
- the conformance harness: `it`, `assert`, `assert_equal`, and `println`.

The owner placed the test registration functions `it_each`, `it_prop`, and
`it_prop_with` in this tier, though the compiler checks their calls
([Registration Functions](testing.md#registration-functions)).

Every other decided std API belongs in this directory.

### The Tier Test

> Could `lib/std` implement the item in ordinary hd, over language-tier
> items only, with no compiler knowledge of its name, and keep the same
> observable behavior?

Yes puts the item in the stdlib tier. A diagnostic code about the item, a
language rule that names it, or a position rule that lists it each mean
no.

Only `std` may add inherent methods to built-in types
([`trait.own.inherent.std`](../09-traits.md#r-trait.own.inherent.std)).
That alone does not make a method language-tier.

## Chapters

Each chapter is one unnumbered file named after its std module. The
Spec Tiers migration plan
added the files below, and it is complete. A new decided std module gets a
file of its own.

| File | Module | Rule ID prefix | Scope |
| --- | --- | --- | --- |
| [`testing.md`](testing.md) | `std.testing` | `std-testing` | the registration functions `it_each`, `it_prop`, and `it_prop_with`, property tests, the draw budget, derived `Arbitrary`, table-test rows, snapshot files, the `timeout` option |
| [`iter.md`](iter.md) | `std.iter` | `std-iter` | iterator adapters, collect targets, `FromIterator` and its impls, `map` on a list or an optional |
| [`text.md`](text.md) | `std.text` | `std-text` | string methods above the intrinsics, the `r` prefix |
| [`format.md`](format.md) | `std.format` | `std-format` | the text `debug` returns, `Debug` builders and layout, derived builder calls |
| [`time.md`](time.md) | `std.time` | `std-time` | `Duration` and its suffixes |
| [`task.md`](task.md) | `std.task` | `std-task` | the `retry!` combinator |
| [`ops.md`](ops.md) | `std.ops` | `std-ops` | the `Default` trait and its standard implementations |
| [`cmp.md`](cmp.md) | `std.cmp` | `std-cmp` | what derived `Eq`, `PartialOrd`, and `Ord` compare; tuple comparison |
| [`hash.md`](hash.md) | `std.hash` | `std-hash` | what derived `Hash` hashes; tuple hashing |

## Glossary

This glossary lists the terms of the stdlib chapters. Each entry links to
the rule, or the section, that defines the term. The language terms are in
the [language glossary](../README.md#glossary).

| Term | Definition |
| --- | --- |
| **collect target** | The collection that `collect` builds, named by the expected type. See [Collect Targets](iter.md#collect-targets). |
| **Debug builders** | The `DebugWriter` methods that describe a value as a struct, tuple, list, or map. See [Debug Builders](format.md#debug-builders). |
| **draw budget** | The per-case limit on draws from `Choices`; once it is spent, every draw returns its simplest value. See [`std-testing.budget`](testing.md#r-std-testing.budget). |
| **iterator adapters** | Methods of the prelude `Iterator[T]` that wrap an iterator in a new one, or drain it. See [Iterator Adapters](iter.md#iterator-adapters). |
| **property test** | A test case whose body runs on inputs drawn from a `Choices` source. See [Property Tests](testing.md#property-tests). |

## Rule IDs

1. A rule ID here starts with its file's prefix: `std-` and the module
   name.
2. A rule that moves here from a numbered chapter is retired there and
   keeps its tail under the new prefix. `module.testing.prop.debug`
   becomes `std-testing.prop.debug`.
3. The retired ID is deleted, as [Stability](../STYLE.md#stability)
   requires.

## Links Between Tiers

- A stdlib chapter may cite any language rule.
- A language chapter links here only from a Note or a See also, never from
  a numbered rule. No language rule depends on stdlib behavior.
- A moved heading is deleted, and every link to it is fixed when it moves.

## Conformance And Diagnostics

- A case's tier is the tier of its `specification` column in
  [`cases.tsv`](../conformance/cases.tsv). A path under `std/` is stdlib.
- Every diagnostic code stays in the [Diagnostics](../README.md#diagnostics)
  table, and every panic category stays in
  [Control Flow](../06-control-flow.md). Each is a compiler check.
- One [Revision Notes](../README.md#revision-notes) log covers both tiers.
