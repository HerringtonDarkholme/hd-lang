# Audit: What Is Still Open

The 2026-09-25 audit of the prototype compiler, and the conformance and
specification work that followed, are finished. Finished items have been
removed from this folder. What remains:

- [`REPORT.md`](REPORT.md): the audit's verdict, architecture review, and the
  findings still open.
- [`findings/`](findings/): one file per open finding (65 as of 2026-09-26).
  Most are prototype compiler bugs or performance notes; the index is
  [`evidence/findings-table.md`](evidence/findings-table.md).
- [`evidence/`](evidence/), [`probes/`](probes/), [`scripts/`](scripts/):
  the runs that back those findings. [`evidence/w9/failures-by-id.tsv`](evidence/w9/failures-by-id.tsv)
  is the prototype's fix list, grouped by ID.
- [`bench/`](bench/): the small benchmark set kept for future direction.

## Specification Follow-Ups

- F-150: `typing/invalid/nondisplay-entry-error.hd` and
  `nonhost-entry-requirement.hd` use a private declaration in the signature of
  `pub fn main`, so every conforming implementation also reports
  `private-type-leak`. The fixtures need `pub` on those declarations.

## Applied Decisions the Prototype Does Not Follow Yet

G2, G3, L6, L7, L8, L9 are implemented; L2's remaining case
(`unsigned-exponent.hd`) waits on sized numeric types and is tagged F-253.
`test/portable/KNOWN_FAILURES.tsv` tags the rest:

| #  | Decision |
| -- | -------- |
| K1 | `shape` is no longer a keyword: `shape[T]()` and `shape_of(f)` are prelude intrinsics, with typed `fields`/`variants` members on specialized shapes. The prototype has no shape intrinsics. |
