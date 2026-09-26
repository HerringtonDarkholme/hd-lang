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

## Specification Follow-Ups

- F-205: every held-back fixture with a decided code is now in
  `spec/conformance`. The prototype still emits its own code for 22
  conformance cases (tagged F-205 in `test/portable/KNOWN_FAILURES.tsv`),
  mostly parser codes and `generic-type-mismatch`.

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
| I5 | A non-public function, inherent method, or local `fn` may omit its result type. Public functions, trait methods, and trait-impl methods must declare it (`missing-result-type`). A cycle among omitted results is `recursive-function-needs-result-type`. |
| I6 | Such non-public callables may also omit their requirement clause; the row is inferred. Public ones keep "no clause means the empty row". |

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
