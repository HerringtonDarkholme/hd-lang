# Type-Checking Rules: Questions For The Owner

Applied decisions have been removed from this file; the commit messages are
their record (TQ-1 to TQ-6, TQ-4 and its follow-ups,
TQ-9, TQ-11, TQ-12, TQ-15 to TQ-17, TQ-19, TQ-20, TQ-23, TQ-27 to TQ-31,
TQ-36, TY-13, EQ-1, E1 to E5, M2, O1 to O3, P2, P6, VE and VE-S, Cut 2,
trait delegation, the embedding limits, the single view, A2, A3, C1 to C3,
TUP-1 except its `@message` part, and TQ-10 as revised on 2026-09-27:
suspending methods and row parameters are allowed on dynamically safe
traits, replacing the earlier answer that excluded them, and only `reified`
parameters and packs stay excluded). Superseded questions (TQ-7, TQ-8, TQ-32 to TQ-35) are gone too,
and so is TQ-22: its impl-target half is applied, and the Inspectable
decisions, also applied, replaced its `downcast` half.
TQ-21 is answered by K2: a bound may bind associated types, as in
`I < Supplier[Item = T]`. Findings with the evidence are in
[FINDINGS.md](FINDINGS.md), and the proposed rule text in
[PROPOSED_RULES.md](PROPOSED_RULES.md).

## Decided, Not Yet Applied

None. TQ-18 was superseded on 2026-09-27: typed derivation removed
`Annotate` and the root-application orphan exception, so there is no marker
to spell. The `@message` part of TUP-1 (`_0`, `_1`, ... in a variant's
message) was applied on 2026-09-29 with the `@error` intrinsic, in
[`annot.error.message.scope`](../../spec/lang/14-annotations.md#r-annot.error.message.scope).

## Decided, No Specification Change

TQ-13 is gone: typed derivation replaced its closed `@derive` list.

- **TQ-14** (TY-20): assignability stays single-step. `let wide: i64? =
  small_i8` and passing a `User` to a `Display?` parameter need explicit
  conversions.

## Parked

TQ-24 to TQ-26: the owner does not want to discuss NonEscapable now. The
initial answers, not to be applied until the owner reopens the topic, are
declared-and-checked propagation (TQ-24), opt-in generic parameters (TQ-25),
and no NonEscapable returns (TQ-26).

### TQ-24: How NonEscapable propagates
    data HiddenFile:
        file: File        # File is NonEscapable
Options: (A) structural and automatic, like Rust auto traits; (B) declared: a
data or enum type holding a NonEscapable field must itself be declared
NonEscapable, and the compiler checks it; generic types are NonEscapable
exactly when an argument is.
**Recommend B.** It is visible in the declaration, and a public type's
property cannot change when a private field changes (Swift's resilience
argument). In both options, erasure to `Any` or to a trait value that does not
extend NonEscapable is rejected. Provider values may be NonEscapable only
through such traits.

### TQ-25: Do generic parameters accept NonEscapable arguments by default?
Options: (A) no, a parameter accepts them only when it opts in (Swift's
`~Escapable` model); (B) yes, and every generic body is checked
conservatively.
**Recommend A.** Existing generic code stays valid, and opt-in is explicit.

### TQ-26: Dependent-return provenance
    fn first_line(file: File) -> Line    # Line keeps file alive
Options: (A) a NonEscapable result depends conservatively on all NonEscapable
parameters, with no syntax; (B) the signature names the parameters it depends
on; (C) no NonEscapable returns.
**Recommend A now and B later**, when a real API needs the precision.
