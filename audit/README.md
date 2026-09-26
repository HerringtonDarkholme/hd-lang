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
| L6 | Inferring `mut T` from a readonly argument is `mutable-upgrade`. |
| L7 | `is` operands are compatible when, with `mut` removed at every level, their types are equal, or one is a trait value or `Any` type the other converts to. |
| L8 | Fieldless data values are canonical: one identity per fieldless data type. |
| L9 | An indirect supertrait cycle is reported once, on the cycle member first in source order. |
