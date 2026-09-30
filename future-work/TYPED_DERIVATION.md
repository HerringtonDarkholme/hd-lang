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
[Error Derivation](../spec/14-annotations.md#error-derivation)
([Error Conversion](ERROR_CONVERSION.md#owner-decisions)).

The prototype implements M1-M29 by lowering ([src/README.md](../src/README.md)).
Its gaps are rows of [KNOWN_FAILURES.tsv](../test/portable/KNOWN_FAILURES.tsv),
mostly `K1` (the shape intrinsics) and `F-250`, plus one `M29` fixture that
needs package roles.

## Owner Decisions M30 (2026-09-29)

The owner answered the leftover points. **Applied 2026-09-29** to
[Typed Derivation](../spec/14-annotations.md#typed-derivation):

| Point | Decision | Where |
| --- | --- | --- |
| Fact check hook | None; readers validate (Decorators D2 and D3) | the prose under [Member Metadata](../spec/14-annotations.md#member-metadata) |
| `T -> U` mapping | Out of scope | removed from Undecided Parts |
| Name clashes | Generated `walk`, `describe`, and `build` keep their names; `Structure::walk(self, w)` picks the generated `walk` | a Note under [Templates](../spec/14-annotations.md#templates) |
| Derived bound | No rule change; the diagnostic names the walker's strengthened bound | a Note under [Derived Bounds](../spec/14-annotations.md#derived-bounds) |
| Plan constants, typed shared constants | Deferred until a real template, such as `std.json`, needs them | a Note under [Limits](../spec/14-annotations.md#limits) says there are none |
| `default()` allocation | Accepted and not specified | removed from Undecided Parts |
| Composing templates | Deferred until needed; M9's restriction stays | the same Note under Limits |
| M26 readings | All confirmed as applied | see below |

Still waiting on other areas: non-escaping handles (NonEscapable,
parked), function targets (FN_TYPE Q9/Q10, parked), and `Clone`'s module
and the derived-function cache (std).

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
