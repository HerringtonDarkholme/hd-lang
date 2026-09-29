# Typed Derivation: Open Points

Status: open. Nothing here is accepted behavior. Owner decisions M1-M29
(2026-09-26 to 2026-09-28) fully decide typed derivation, and all of them
are applied in [Typed Derivation](../spec/14-annotations.md#typed-derivation),
with grammar in [02](../spec/02-grammar.md#traits-and-implementations) and
rules in [08](../spec/08-data-and-enums.md#typed-derivation-of-data-and-enums)
and [09](../spec/09-traits.md#derived-implementations). The specification is
authoritative. The survey, the design options, the worked examples, and
the three stress tests behind the decisions are in git history.

Error derivation is the separate `@error` intrinsic, which is decided but
not yet applied ([Error Conversion](ERROR_CONVERSION.md#owner-decisions)).

The prototype implements M1-M29 by lowering ([src/README.md](../src/README.md)).
Its gaps are rows of [KNOWN_FAILURES.tsv](../test/portable/KNOWN_FAILURES.tsv),
mostly `K1` (the shape intrinsics) and `F-250`, plus one `M29` fixture that
needs package roles.

## Owner Decisions M30 (2026-09-29)

The owner answered the leftover points. They are not yet applied to
spec/14's Undecided Parts.
- **Fact check hook:** none. Readers validate (Decorators D2 and D3), so
  remove it from Undecided Parts.
- **`T -> U` mapping:** out of scope; remove it.
- **Name clashes:** generated `walk`, `describe` and `build` keep their
  names. On a clash, the qualified call `Structure::walk(self, w)` (method
  reference form) picks the generated one. State this as a Note.
- **Derived bound:** no rule change. The diagnostic names the walker's
  strengthened bound, the real obligation.
- **Plan constants and typed shared constants:** deferred until a real
  template (for example std.json) needs them.
- **`default()` allocation:** accepted and not specified. Allocation is an
  implementation detail.
- **Composing templates:** deferred until needed; M9's restriction stays.
- **The M26 readings are all confirmed** as applied (the two tables
  below).
- Still waiting on other areas: non-escaping handles (NonEscapable,
  parked), function targets (FN_TYPE Q9/Q10, parked), and `Clone`'s module
  and the derived-function cache (std).

## Remaining Open

Nothing below is decided. Each item waits for the owner. The specification
lists them as [Undecided Parts](../spec/14-annotations.md#undecided-parts).

- **Fact check hook** (M15). The form of a fact type's compile-time `check`,
  and whether it covers cross-member and type-level checks (round 2 R8).
- **Non-escaping handles** (M18 R5). Whether the NonEscapable design (TQ-24
  to TQ-26) makes handles non-escaping once it is ready.
- **Plan constants** (M21 R3-4). The declaration and reference syntax of a
  template's constant, and the compile-time evaluator's exact limits.
- **Typed shared constants** (M21 R3-7). Typed constant handles may come
  later.
- **`T -> U` mapping** (M14). Whether mapping between two types is in
  scope.
- **Name clashes** (round 2 R13). `walk`, `describe`, and `build` collide
  with trait methods of the same name; `Structure::walk(self, w)` avoids it.
- **Derived bound** (round 2 R14). The derived bound names the trait, while
  the checked obligation is the walker's or source's strengthened bound;
  they differ when a walker asks for more than the trait.
- **`default()` allocation** (round 2 R15). 04 and 05 give every `.Some`
  its own identity, so `default()` may allocate for every member type, not
  only reference-shaped ones.
- **Composing templates** (round 1 P16). A wrapper walker cannot forward to
  an inner walker's `member`, because M9 lets only generated code call it
  through a generic parameter.
- **`Clone`'s module** (M24). Chosen with the standard library
  ([STDLIB](STDLIB.md#clone)).
- **Derived-function cache** (M24). Its API and module are chosen with the
  standard library ([STDLIB](STDLIB.md#derived-function-cache)).
- **Function targets.** Deriving for functions waits for
  [FN_TYPE](FN_TYPE.md) questions 9 and 10. An ordinary decorator before a
  function already attaches a value
  ([Prefix Decorators](../spec/14-annotations.md#prefix-decorators)).

## Readings Awaiting Confirmation

The M26 apply pass (2026-09-28) applied these readings of the trait-less
derivation block. The specification states each, so each can change
without breaking a decision. Each waits for the owner to confirm.

| Point | Applied reading | Rules |
| --- | --- | --- |
| Scope | The block must be in the module that declares its target, as an inherent implementation and a derivation block must. Elsewhere it is `misplaced-derivation`. | [`annot.traitless.module`](../spec/14-annotations.md#r-annot.traitless.module) |
| Parameter metadata | `@` on the parameter only. A block for a function is not possible, since its target must be a type. | [`annot.metadata.params-at-only`](../spec/14-annotations.md#r-annot.metadata.params-at-only) |
| Combining | Decorator values come first. A `+=` line appends after them, a `=` line replaces them, and `Self` lines do the same for type-level facts. A per-trait block's lines then edit the result. | [`annot.traitless.after-decorators`](../spec/14-annotations.md#r-annot.traitless.after-decorators), [`annot.traitless.self`](../spec/14-annotations.md#r-annot.traitless.self), [`annot.traitless.then-blocks`](../spec/14-annotations.md#r-annot.traitless.then-blocks) |

M26 says the block is only for writing shared metadata. The apply pass
read that as the rules below, also for the owner to confirm:

| Point | Applied reading | Rules |
| --- | --- | --- |
| An omit line, `f = pass` | `invalid-member-line`: omitting a member changes generated code, so it is not metadata | [`annot.traitless.no-omit`](../spec/14-annotations.md#r-annot.traitless.no-omit) |
| A method or associated type in the block | `misplaced-derivation` on that member | [`annot.traitless.lines-only`](../spec/14-annotations.md#r-annot.traitless.lines-only) |
| A block in a local scope | `misplaced-derivation`, keeping the old rule that local declarations carry no metadata | [`annot.traitless.local`](../spec/14-annotations.md#r-annot.traitless.local), [`names.local.no-metadata`](../spec/03-names-and-scopes.md#r-names.local.no-metadata) |
| Target kinds | A data type or enum only; a newtype, like any other target, is `misplaced-derivation`. A GADT enum is allowed, since the block derives nothing | [`annot.traitless.target`](../spec/14-annotations.md#r-annot.traitless.target) |
| `impl C by E` without a trait, where `E` is not `Structure` | `invalid-delegation`, as for a bad delegation | [`trait.by.trait-less.error`](../spec/09-traits.md#r-trait.by.trait-less.error) |
