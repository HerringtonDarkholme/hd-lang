# Error Conversion: The `@error` Intrinsic

Status: decisions 21-27 (batch 9, 2026-09-29) are decided and not yet
applied; every earlier decision is applied. Nothing here is accepted
behavior; the specification is authoritative. Decisions 1-9, 11,
and 13-20 (2026-09-26 and 2026-09-27) are applied, or superseded by the
testing redesign:
[Propagation](../spec/05-expressions.md#propagation),
[Conversion Trait](../spec/09-traits.md#conversion-trait),
[Error Trait](../spec/09-traits.md#error-trait),
[Dynamic Trait Values](../spec/09-traits.md#dynamic-trait-values),
[Variant Constructors As Function Values](../spec/08-data-and-enums.md#variant-constructors-as-function-values),
[Generic Function Values](../spec/07-functions.md#generic-function-values),
[Entry Results](../spec/10-modules.md#entry-results),
[Exit Status](../spec/10-modules.md#exit-status), and
[Propagation In Test Blocks](../spec/05-expressions.md#propagation-in-test-blocks).
Decisions 10 and 12, the `@error` intrinsic, were applied on 2026-09-29 in
[Error Derivation](../spec/14-annotations.md#error-derivation). The
owner answered its apply-pass points as decisions 21-27.

The error-chain helpers (`Context`, `.context`, `chain`, `find`,
`root_cause`, `ErrorReport`) are library API in
[Standard Library Design](STDLIB.md#stderror). The survey, the candidate
designs, the error stress test, and decision 10's full text with its
revisions are in git history.

## Owner Decisions

Applied 2026-09-29:

10. **Question 11: error derivation is one compiler intrinsic, `@error`**
    (2026-09-27, revised twice the same day). It is Rust's `thiserror`
    moved into hd: messages, `@from`, `@source`, and
    `@error(transparent)`, and no error codes. The spelling revision
    replaced `@derive(Error)`, `@message`, and `@transparent`. Parts the
    apply pass cites below: E1 puts the payload members and the enum's
    common fields in a message's scope; gap 1 infers generated bounds per
    use, giving a carried parameter none; gap 3 names unnamed payload
    members `_0`, `_1`. Reference case: ast-grep's `RuleCoreError`.
12. **Annotations on enum payload parameters** (2026-09-27, review F1). The
    grammar part was applied with typed derivation
    ([`grammar.enum.payload-decorator`](../spec/02-grammar.md#r-grammar.enum.payload-decorator),
    [`annot.fact.payload`](../spec/14-annotations.md#r-annot.fact.payload)).
    What `@from` and `@source` mean there is now in
    [Error Types](../spec/14-annotations.md#error-types).

| Decision part | Rules |
| --- | --- |
| The intrinsic and its forms | [`annot.error.intrinsic`](../spec/14-annotations.md#r-annot.error.intrinsic), [`.name`](../spec/14-annotations.md#r-annot.error.name), [`.type`](../spec/14-annotations.md#r-annot.error.type), [`.generates`](../spec/14-annotations.md#r-annot.error.generates), the `annot.error.form.*` table |
| Generated impls are ordinary (E2) | [`annot.error.ordinary`](../spec/14-annotations.md#r-annot.error.ordinary), [`.hand-written`](../spec/14-annotations.md#r-annot.error.hand-written) |
| Markers (gap 2, spelling) | [`annot.error.marker`](../spec/14-annotations.md#r-annot.error.marker), [`.marker.no-value`](../spec/14-annotations.md#r-annot.error.marker.no-value), [`.marker.outside`](../spec/14-annotations.md#r-annot.error.marker.outside) |
| Messages (E1, gap 3, R7) | [Error Messages](../spec/14-annotations.md#error-messages) |
| Causes (`@source`, E4, decision 12) | [Error Causes](../spec/14-annotations.md#error-causes), [`trait.error.cause`](../spec/09-traits.md#r-trait.error.cause) |
| `@from` (R6) | [Error Conversions](../spec/14-annotations.md#error-conversions) |
| Transparent errors (E3, gap 4) | [Transparent Errors](../spec/14-annotations.md#transparent-errors) |
| Bounds (gap 1) | [Generated Error Bounds](../spec/14-annotations.md#generated-error-bounds) |

The review R12 rule about common fields without a default is not
specified: Enum Semantics decision 4 made shared enum data per-variant
constants, which made it moot.

Decided 2026-09-29, not yet applied (batch 9, the apply-pass points; all
as recommended):

21. **ERR SO1: a misplaced marker.** A misplaced `@error`, `@from`, or
    `@source` is `decorator-target-kind`.
22. **ERR SO2: invalid markers.** A second cause member, and `@from` on a
    bare type parameter, are a new code, `invalid-error-marker`. A cause
    or transparent member that is not an `Error` is
    `unsatisfied-trait-bound`.
23. **ERR SO3: `$self`.** `$self` in a message is `unknown-name`.
24. **ERR SO4: `$_0` in a message.** `$_0`, `$_1` in a message name the
    variant's unnamed payload members. Unnamed shared enum data is not in
    scope.
25. **ERR SO5: generated bounds.** A carried-only type parameter gets
    `P < Inspectable` on the generated `impl Error`; `Display` keeps
    gap 1, so it gives a carried-only parameter no bound. A transparent
    member of type `P` gets `P < Error`, so `cause` forwards. An example
    uses `@error(transparent) Inner(e: P)` without `@from`, since
    decision 22 makes `@from` on a bare `P` invalid.
26. **ERR SO6: no import.** Writing `@error` needs no
    `use std.error.Error`, matching
    [`annot.derive.no-use`](../spec/14-annotations.md#r-annot.derive.no-use).
    Fixtures that import it only for this reason drop the import where
    that is natural.
27. **Apply-pass readings confirmed.** Two `@from` members of one type, or
    a hand-written `Display`, `Error`, or `From` for an error type, are
    `overlapping-impl`. Markers attach no value. A message is evaluated
    each time the value is displayed. A bare `@error` before a data type,
    and `@error("...")` before an enum, are invalid; the code comes from
    decisions 21-26 where one covers it, and the applied reading stays
    otherwise.

### Apply-Pass Readings

The owner confirmed the apply pass's readings as decision 27.

## Current Design

The specification holds the design:
[Error Derivation](../spec/14-annotations.md#error-derivation).

## Still To Do

Apply decisions 21-27 to
[Error Derivation](../spec/14-annotations.md#error-derivation).

## Still Open

Nothing is open.

## Parse Log

This record has no `text` blocks.
