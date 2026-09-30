# Typed Derivation: Open Points

Status: open. Nothing here is accepted behavior. Owner decisions M1-M30
(2026-09-26 to 2026-09-29) decide typed derivation, and all of them
are applied in [Typed Derivation](../spec/14-annotations.md#typed-derivation),
with grammar in [02](../spec/02-grammar.md#traits-and-implementations) and
rules in [08](../spec/08-data-and-enums.md#typed-derivation-of-data-and-enums)
and [09](../spec/09-traits.md#derived-implementations). The specification is
authoritative. The survey, the design options, the worked examples, and
the three stress tests behind the decisions are in git history.

Error derivation is the separate `@error` intrinsic, applied in
[Error Derivation](../spec/14-annotations.md#error-derivation).

The prototype implements M1-M29 by lowering ([src/README.md](../src/README.md)).
Its gaps are rows of [KNOWN_FAILURES.tsv](../test/portable/KNOWN_FAILURES.tsv),
mostly `K1` (the shape intrinsics) and `F-250`, plus one `M29` fixture that
needs package roles.

## Owner Decisions M30 (2026-09-29)

The owner answered the leftover points; all are applied (2026-09-29) in
[Typed Derivation](../spec/14-annotations.md#typed-derivation). There is
no fact check hook (readers validate); `T -> U` mapping is out of scope;
generated `walk`, `describe`, and `build` keep their names; the derived
bound needs no rule change; plan constants, typed shared constants, and
composing templates are deferred until a real template needs them; and
`default()` allocation is accepted and not specified. M30 also confirms
every M26 apply-pass reading.

Still waiting on other areas: non-escaping handles (NonEscapable,
parked), function targets (FN_TYPE Q9/Q10, parked), and `Clone`'s module
and the derived-function cache (std).

## Owner Decision SR1 (2026-09-30)

Batch 17, recorded and not yet applied. The owner chose compiler-supplied
information over an intrinsic `Arbitrary` (testing point PT-d), then
generalized it for every template.

| Part | Decision |
| --- | --- |
| Declaration | `std.structure` gains a compiler-computed `pub enum SelfRef: Absent / Optional / Required`, and a `self_ref: SelfRef` field on `VariantInfo` and on `Member`. |
| `Absent` | The member or variant never refers to the enclosing type. |
| `Optional` | It refers to the enclosing type only where the simplest value is empty or `.None` (`List`, `Map`, `T?`), so recursion can end. |
| `Required` | Its simplest value still needs the enclosing type: testing Q7's "recursive". |
| One enum | Not two bools, because needing the enclosing type implies mentioning it. |
| Derived `Arbitrary` | An ordinary `std.testing` template that reads `SelfRef`. The prototype's checker-generated `Arbitrary` is a stopgap. |

Uses the owner listed:

| Use | How it reads `self_ref` |
| --- | --- |
| Derived `Arbitrary` and `Default` | The simplest variant is the first with `self_ref != .Required`. |
| No finite value | Every variant, or one data field, is `.Required`: the no-finite panic. |
| Codecs | A nesting-depth limit when any `self_ref != .Absent`. |
| Schema generators | A named definition and a `$ref` when any `self_ref != .Absent`. |
| `Debug` | May truncate deep output. |

## Remaining Open

Nothing below is decided. Each item waits for the owner. The specification
lists them as [Undecided Parts](../spec/14-annotations.md#undecided-parts).

- **Non-escaping handles** (M18 R5). Whether the NonEscapable design (TQ-24
  to TQ-26) makes handles non-escaping once it is ready.
- **`Clone`'s module** (M24). Chosen with the standard library
  ([STDLIB](STDLIB.md#clone)).
- **Derived-function cache** (M24). Its API and module are chosen with the
  standard library ([STDLIB](STDLIB.md#derived-function-cache)).
- **Function targets.** Deriving for functions waits for
  [FN_TYPE](FN_TYPE.md) questions 9 and 10. An ordinary decorator before a
  function already attaches a value
  ([Prefix Decorators](../spec/14-annotations.md#prefix-decorators)).

## Readings Awaiting Confirmation

None. M30 confirmed every reading of the M26 apply pass, and the
specification states each under
[Trait-Less Derivation Blocks](../spec/14-annotations.md#trait-less-derivation-blocks)
and in [`trait.by.trait-less.error`](../spec/09-traits.md#r-trait.by.trait-less.error).
