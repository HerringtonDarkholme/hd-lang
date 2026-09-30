# Error Conversion: The `@error` Intrinsic

Status: every owner decision of this record is applied. Nothing here is
accepted behavior; the specification is authoritative. Decisions 1-9, 11,
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
[Error Derivation](../spec/14-annotations.md#error-derivation). Points
that need the owner are under [Still Open](#still-open).

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

### Apply-Pass Readings

The apply pass read these points from the decision. None adds behavior the
decision did not state; each is listed so the owner can confirm it.

| Reading | Rule |
| --- | --- |
| A marker is not a name, so it attaches no value: other derivations of the type do not see it. | [`annot.error.marker.no-value`](../spec/14-annotations.md#r-annot.error.marker.no-value) |
| Two `@from` members of one type are `overlapping-impl`, reported on the later member, since generated impls are ordinary. | [`annot.error.from.same-type`](../spec/14-annotations.md#r-annot.error.from.same-type) |
| A hand-written `Display`, `Error`, or generated `From` is `overlapping-impl`, reported on the hand-written impl. | [`annot.error.hand-written`](../spec/14-annotations.md#r-annot.error.hand-written) |
| A message is evaluated each time the value is displayed, not once at compile time as a fact is. | [`annot.error.message.eval`](../spec/14-annotations.md#r-annot.error.message.eval) |
| "Common fields" are now the enum's shared fields; named ones are in scope. | [`annot.error.message.shared`](../spec/14-annotations.md#r-annot.error.message.shared) |
| Only the listed forms are valid, so a bare `@error` before a data type, and `@error("...")` before an enum, are invalid. | [`annot.error.form.other`](../spec/14-annotations.md#r-annot.error.form.other) |
| A transparent member must implement `Error`, since its `cause` is forwarded. | [`annot.error.transparent.type`](../spec/14-annotations.md#r-annot.error.transparent.type) |
| The compiler now generates `cause`, so the specification states its signature; `trait.error.api` is retired. | [`trait.error.cause`](../spec/09-traits.md#r-trait.error.cause) |

## Current Design

The specification holds the design:
[Error Derivation](../spec/14-annotations.md#error-derivation).

## Still To Do

Nothing remains to apply. The points below wait for the owner.

## Still Open

The apply pass met these points, where the recorded decision meets later
rules or leaves a code unnamed. Each waits for the owner; the Applied column
says what the specification states now.

| # | Point | Applied | **Recommendation** |
| --- | --- | --- | --- |
| 1 | Which code rejects a misplaced `@error`, `@from`, or `@source`, such as `@error` before a function or newtype? A misplaced `@derive` is `decorator-not-annotator`, while a value outside its `@annotate` kinds is `decorator-target-kind`. | "Invalid", with no code ([`annot.error.form.other`](../spec/14-annotations.md#r-annot.error.form.other)) | `decorator-target-kind`: the problem is the target's kind, as for `@annotate`. |
| 2 | Which codes reject a second cause member, a `@from` member whose type is a bare type parameter, and a cause or transparent member that is not an `Error`? The decision calls each an error without a code. | "Invalid", with no code ([`annot.error.cause.one`](../spec/14-annotations.md#r-annot.error.cause.one), [`.from.type-parameter`](../spec/14-annotations.md#r-annot.error.from.type-parameter), [`.cause.type`](../spec/14-annotations.md#r-annot.error.cause.type), [`.transparent.type`](../spec/14-annotations.md#r-annot.error.transparent.type)) | `unsatisfied-trait-bound` for a member that is not an `Error`, as for any unmet bound; one new code, `invalid-error-marker`, for the other two. |
| 3 | Which code rejects `$self` in a message? The decision (review R7) calls it a compile error, apart from an unknown name. `self` outside a method has no code today. | "Invalid", with no code ([`annot.error.message.no-self`](../spec/14-annotations.md#r-annot.error.message.no-self)) | `unknown-name`: a message is not inside a method, so `self` names nothing. |
| 4 | What does `$_0` name in a message when the enum also has unnamed shared data? Gap 3 names unnamed payload members `_0`, `_1`, and [`data.shared.underscore-field`](../spec/08-data-and-enums.md#r-data.shared.underscore-field) names unnamed shared data the same way. E1 put common fields in scope before Enum Semantics decision 4. | Only named shared fields are in scope ([`annot.error.message.shared`](../spec/14-annotations.md#r-annot.error.message.shared)) | The payload member wins, and unnamed shared data is not in scope: the message describes the value. |
| 5 | Which bounds does the generated `Error` get for a type parameter that is only carried, or that a transparent member has? Gap 1 gives a carried parameter no bound, but `impl Error` needs an inspectable target ([`trait.error.not-inspectable`](../spec/09-traits.md#r-trait.error.not-inspectable)), and an unbounded parameter is not inspectable ([`trait.inspectable.not.parameter`](../spec/09-traits.md#r-trait.inspectable.not.parameter)). A transparent member forwards `cause`, which needs `P < Error`. | Only `P < Error` for a `@from` or `@source` member ([`annot.error.bound.error`](../spec/14-annotations.md#r-annot.error.bound.error)); `Display` follows gap 1 | The generated `Error` gets `P < Inspectable` for a carried parameter and `P < Error` for a transparent one; `Display` keeps gap 1. |
| 6 | Does a module that writes `@error` need `use std.error.Error`? The decision's example imports it, but for other uses too. | No rule; the fixtures import it | No import, as [`annot.derive.no-use`](../spec/14-annotations.md#r-annot.derive.no-use) needs none for `Structure`. |

Point 5 in hd:

```text
@error
enum TaskError[E, T]:
    @error("task failed")
    Failed(@source error: E, input: T)
```

Under gap 1 alone, `impl[E < Error, T] Error for TaskError[E, T]` has a
target that is not inspectable, because `T` has no bound.

## Parse Log

The one `text` block of this record parses with the reference parser.
Parsing checks syntax only.

| Block | Result |
| --- | --- |
| 1 (`TaskError[E, T]`) | parse |
