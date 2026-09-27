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

- **04, decided 2026-09-27:** `types.string.host-utf8` stays normative;
  package interfaces carry the generic and pack function bodies needed
  downstream (fix the Implementation Model note to say per-shape bodies are
  compiled in the defining package but carried bodies may be used for
  specialization, and packages ship sources); for a named function, inference
  applies only to its result type and its requirement row; its parameter
  types, generic parameters, and bounds are always written in its
  declaration (clarifies Type Inference Boundaries).

- **06/04, decided 2026-09-27:** `break` and `continue` are expressions of
  type `never`, like `return`, `panic`, and calls returning `never`, so they
  fit any expected type (`.None => continue` in a match arm,
  `if empty: break else: pop()`); this explains the existing "every
  reachable branch" rule and changes no program's validity. The complete
  list of `never` expressions is those five forms.
- **Fixture fix:** F-150's fixtures need `pub` added in `spec/conformance`
  (from the prototype catch-up).

- **TQ-2 fresh elements, decided 2026-09-27:** an expected type does not
  weaken the `mut` of freshly built elements. A comprehension of `Word`
  literals returned as `Iterator[Word]` is an error; the author declares
  `Iterator[mut Word]` or builds readonly elements explicitly. Fix the
  fixture `trait-argument-owner-impl.hd` accordingly.

- **Enum shared constructor data is read-only** (ENUM_SEMANTICS question 2,
  decided 2026-09-27): a `mut` enum view can no longer reassign a named
  shared constructor field, so an enum's own slots (tag, payload, shared
  fields) never change once built. This is shallow: a payload may be a
  mutable reference (`Loaded(user: mut User)`), and that object can still
  change, as with a tuple holding references. The
  value-category question (question 1) is still open.

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
| 01 | `unexpected-bom`, `tab-whitespace`, `invalid-dedent`, `invalid-escape`, `reserved-semicolon`, `unknown-name` |
| 06 | `break-value-context`, `break-outside-loop`, `nonexhaustive-match`, `duplicate-data-pattern-field`, `return-outside-function`, `readonly-root`, `unknown-data-field`, `iterator-invalidated` |
| 07 | `missing-return-value`, `non-reassignable-parameter-binding`, `nonfinal-positional-spread`, `trailing-block-position`, `partial-generic-arguments`, `unresolved-generic-placeholder`, `recursive-closure-needs-result-type`, `readonly-root`, `argument-count`, `type-mismatch`, `no-common-type`, `mutable-capture-requires-mut-fn` |

## Open, for the owner

From the 01, 06, 07 restyles (7380905, 8fb85c1, 7c73971):
- 01: raw tab inside a string: `string_character` admits it, the tab rule
  says tabs occur only as `\t`.
- 01: a lone `"` inside `"""..."""` looks invalid by
  `multiline_string_character`, though the literal ends only at `"""`.
- 01: confusable and mixed-script identifiers "must be diagnosed" (error?)
  but README lists them as warnings.
- 06: is the list of suites whose value is discarded
  (`flow.must-use.suite-final`) complete?
- 06: `break value` in a loop without `else` is never called an error.
- 07: recursion rules conflict (`fn.decl.omitted-cycle.resolve`: one member's
  result type resolves a cycle; `fn.recursion.named`: every member must
  declare one).
- 07: may a `pub` inherent method omit its result type?
- 07: `fn.vararg.list` states its rule only through the example.

