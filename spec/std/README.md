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
- an intrinsic, such as `List.append`, `string.len`, or `shape[T]()`;
- a prelude name and its signature;
- test registration that the compiler checks, such as `it_each` and
  `it_prop`;
- the conformance harness: `it`, `assert`, `assert_equal`, and `println`.

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
[Spec Tiers migration plan](../../future-work/SPEC_TIERS.md#migration-plan)
adds the files below one at a time. Until a file exists, its rules stay in
the numbered chapters.

| File | Module | Rule ID prefix | Scope |
| --- | --- | --- | --- |
| `testing.md` | `std.testing` | `std-testing` | property tests, the draw budget, derived `Arbitrary`, table-test rows, snapshot files, the `timeout` option |
| `iter.md` | `std.iter` | `std-iter` | iterator adapters, collect targets, `FromIterator` and its impls |
| `text.md` | `std.text` | `std-text` | string methods above the intrinsics, the `r` prefix |
| `format.md` | `std.format` | `std-format` | `Debug` builders and layout |
| `time.md` | `std.time` | `std-time` | `Duration` and its suffixes |

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
