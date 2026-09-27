# Spec Follow-Ups

Logged from the 2026-09-27 chapter restyles. The owner decided these; they
are applied later in one `spec-update` pass, not per chapter.

## Decided, to apply

- **Trait values expose their trait's and every supertrait's methods**
  (09 `trait.dyn.methods` vs `trait.dyn.supertrait-methods`); "only" means
  the concrete type's inherent methods are not reachable.
- **Glossary:** add every bold defined term of the restyled chapters to the
  README glossary, linked to its rule (for example sealed trait, runtime
  identity, inspectable types, inherent method, part, depth, promoted
  member, prelude, script, entry module, program instance, executable entry
  point, runtime profile, type pack, readonly view, mutable edge, readonly
  edge, shape).
- **Editorial:** 09 delegation ambiguity counts only `pub` (promoted)
  inherent methods; 09 "may be ambiguous" becomes "is `ambiguous-method`
  when both traits are available"; 05 exponentiation's checked
  multiplication applies to an integer base (floats use IEEE `pow`); 10
  "that have identity" qualifies only runtime handles; "may not" becomes
  "must not"; STYLE.md adopts "Panic: `code`." for panic codes at the end of
  a rule; the agents' literal readings of unclear referents stand.

## Logged, not scheduled (owner: do not spend on it now)

Rules whose conformance fixtures expect a diagnostic code the rule does not
name; the fix is to append "Error: `code`." and use the code in the error
example, as the 08 pilot did.

| Chapter | Codes |
| --- | --- |
| 03 | `duplicate-module-name`, `non-reassignable-binding`, `binding-not-yet-visible`, `possibly-uninitialized-binding`, `duplicate-binding`, `unknown-name` |
| 04 | `integer-literal-range`, `float-literal-range`, `invalid-map-key`, `mixed-signedness`, `implicit-narrowing`, `invalid-variance`, `variance-representation-change`, `identity-needs-reference-bound`, `unsupported-string-indexing`, `trait-not-dynamically-safe` |
| 05 | `missing-contextual-enum-type`, `identity-requires-references`, `unsigned-negation`, `type-mismatch`, `suspension-forbidden-context` |
| 09 | dynamic-safety break (`trait-not-dynamically-safe`), misplaced local impl (`local-impl-nonlocal-pair`), inherent impl outside the owning package |
| 10 | `direct-variant-use`, `top-level-read-before-initialization`, `private-type-leak`, `missing-requirement`, `missing-partial-eq`, `syntax-error`, `duplicate-module-name` |
| 12 | `multiple-positional-value-packs`, `pack-length-mismatch`, `pack-map-mapper-mismatch` |
| 13 | `impossible-gadt-pattern` |

## Open, for the owner

- 04: the UTF-8 host-boundary sentence was inside a `Note:` paragraph; the
  restyle made it a rule (`types.string.host-utf8`). Confirm it is
  normative.
- 04: rules say package interfaces carry generic and pack function bodies
  needed downstream, while the non-normative Implementation Model says a
  downstream package needs only a generic function's signature.
- 04 `never`: whether "unconditional" covers `break` and `continue`, and
  whether the list of abrupt expressions is complete.
- 04 Type Inference Boundaries: "named function type parameters and bounds
  where applicable" is unclear.
