# Audit: What Is Still Open

The 2026-09-25 audit of the prototype compiler, and the conformance and
specification work that followed, are finished. Finished items have been
removed from this folder. What remains:

- [`REPORT.md`](REPORT.md): the audit's verdict, architecture review, and the
  findings still open.
- [`findings/`](findings/): one file per open finding. Most are prototype
  compiler bugs or performance notes; the index is
  [`evidence/findings-table.md`](evidence/findings-table.md).
- [`evidence/`](evidence/), [`probes/`](probes/), [`scripts/`](scripts/):
  the runs that back those findings. [`evidence/w9/failures-by-id.tsv`](evidence/w9/failures-by-id.tsv)
  is the prototype's fix list, grouped by ID.
- [`bench/`](bench/): the small benchmark set kept for future direction.
- [`blind/fixtures/`](blind/fixtures/): two blind fixtures held back until
  AMB-14 and AMB-16 are answered.

## Specification Follow-Ups

- No chapter defines explicit type arguments in a qualified call
  (`Type::name[T](...)`, `Trait::name[T](...)`), so the chapter 02 EBNF does
  not derive them. Three held-back fixtures in `test/fixtures` wait on this.
- 24 held-back fixtures in `test/fixtures` mark checker rejections that
  have no code in `spec/README.md` (F-205).
- No code covers `is` between incompatible composite types (L7).
- The EBNF derives a header that continues on the line after an indented
  suite at delimiter depth zero, such as `if fn():` + body + `: x else: y`.
  The reference lexer does not carry the header across that suite (F-314).

## Readings to Confirm

The conformance work read these blind-test ambiguities as settled by the
current spec text and added fixtures for them. The owner has not confirmed
the readings yet.

| Tag | Reading | Fixture |
| --- | ------- | ------- |
| AMB-09 | Chapter 04's "shortest round-trip decimal digits" round-trips to the value's own width, so an `f32` displays as an `f32` even through generic code. | `runtime/valid/f32-display-width-through-generics.hd` |
| AMB-10 | Chapter 10 gives the empty list only for an empty separator; splitting a string that lacks the separator yields one piece. | `runtime/valid/split-empty-input-nonempty-separator.hd` |
| AMB-13 | Chapter 06: growth invalidates existing iterators, and their next `next` call panics, even after the iterator was exhausted. | `runtime/panic/exhausted-iterator-invalidated-by-growth.hd` |
| AMB-18 | Chapter 05 place rules accept any mutable composite expression as a receiver, and a field assignment evaluates its receiver before the value. | `runtime/valid/assignment-place-before-value.hd` |
| AMB-23 | Nothing forbids an ordinary method from assigning a top-level `let`. | `runtime/valid/interpolation-display-order.hd` |

## Applied Decisions the Prototype Does Not Follow Yet

`test/portable/KNOWN_FAILURES.tsv` tags failing cases with these IDs. Each
decision is already in the spec; the prototype compiler has not caught up.

| #  | Decision |
| -- | -------- |
| G2 | A dynamically safe trait may declare method-level generic parameters when each is bounded by `Reference` (more bounds allowed, passed as dictionaries). |
| G3 | Tuples are immutable: a tuple element is not a place (`invalid-assignment-target`). |
| L2 | An integer exponent must have an unsigned integer type; an unsuffixed literal exponent is typed `u32`; a signed exponent is `type-mismatch`. |
| L4 | An entry point is `pub fn main()` or `pub fn main!()`, returning `void` or `Result[void, E]` with `E < Display`, with an optional row of host capabilities. A non-`pub` `main` is an ordinary function. |
| L6 | Inferring `mut T` from a readonly argument is `mutable-upgrade`. |
| L7 | `is` operands are compatible when, with `mut` removed at every level, their types are equal, or one is a trait value or `Any` type the other converts to. |
| L8 | Fieldless data values are canonical: one identity per fieldless data type. |
| L9 | An indirect supertrait cycle is reported once, on the cycle member first in source order. |

## Evidence for Open Questions

The questions themselves are in
[`future-work/OPEN_ISSUES.md`](../future-work/OPEN_ISSUES.md), "Questions From
The Compiler Audit", under the same tags.

| Tag | What the audit saw | Evidence |
| --- | ------------------ | -------- |
| N1 | The `disposed-file` profile does not pin the `Files` trait surface or close semantics; the prototype does not implement it. | F-259; `spec/conformance/runtime/valid/resource-disposed-result.hd` |
| N2 | `ready-*` profiles exist only in `src/cli.ts`. | held-back `suspension/32`, `33`, `35`, `36` in `test/fixtures` |
| N3 | `competing-drivers` and `reentrant-poll` cannot be built from their description. | `spec/conformance/runtime/panic/competing-suspension-drivers.hd`, `reentrant-suspension-poll.hd`, `reentrant-cancel.hd` |
| N4 | The synthetic packages `dep.validation` and `dep.models` exist nowhere. | `spec/conformance/typing/valid/root-orphan-annotation.hd`, `typing/invalid/library-orphan-annotation.hd` |
| N5 | Console output cannot be observed by a fixture. | held-back `suspension/27` |
| C2 | Every stored `Suspend[T]` uses a uniform wrapper; an immediately ready `fn!` call costs about 13 times a plain call. | F-554 |
| C3 | Concrete rows cost nothing per call; row-generic code pays a linear lookup. | F-550 |
| C5 | Maps are linear, and `string.len()` is O(n). | F-501 |
| D1 | Replay identity hashes raw declaration text of suspending functions only. | F-401, F-611, F-264 |
| D2 | A panicking or never-finishing run leaves no history. | F-404 |
| D3 | The provider-configuration identity ignores the runtime profile. | F-402 |
| D4 | The entry driver busy-polls a pending host provider. | F-555 |
| D6 | The host boundary uses `externref` handles and byte-at-a-time strings. | F-558 |
| AMB-14 | Can `users[0].name = x` mutate through a readonly root when the generic argument is `mut`? | `blind/fixtures/a6-readonly-root-generic-path.hd` |
| AMB-16 | Which code does a plain closure get for passing a captured `mut T` to a `mut T` parameter? | `blind/fixtures/a6-closure-capture-permission.hd` |
