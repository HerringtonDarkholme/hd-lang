# Spec Follow-Ups

Logged from the 2026-09-27 chapter restyles. The owner decided these, and
they were applied in one `spec-update` pass on 2026-09-29. Each item below
names where it landed, or why it no longer applies. The three points left
for the owner were decided and applied as batch 10, under
[Open, for the owner](#open-for-the-owner).

## Decided, to apply

Applied on 2026-09-29, except where marked obsolete.

| Item | Result |
| --- | --- |
| Trait values expose their trait's and every supertrait's methods; "only" excludes the concrete type's inherent methods | Applied: [`trait.dyn.methods-supertraits`](../spec/09-traits.md#r-trait.dyn.methods-supertraits) replaces `trait.dyn.methods` |
| Glossary: every bold defined term of the restyled chapters | Applied: 55 entries added to the [glossary](../spec/README.md#glossary), each linked to its rule or section. The name-category labels of 03 and the words "fields" and "methods" are left out. **shape** has two senses, 04's machine representation and 14's runtime value; the entry lists both |
| 09 delegation ambiguity counts only `pub` (promoted) inherent methods | Applied: [`trait.by.dot-call`](../spec/09-traits.md#r-trait.by.dot-call) |
| 09 "may be ambiguous" becomes `ambiguous-method` when both traits are available | Applied: [`trait.conflict.ambiguous-available`](../spec/09-traits.md#r-trait.conflict.ambiguous-available), with an error example |
| 05 exponentiation's checked multiplication applies to an integer base | Applied: [`expr.power.checked`](../spec/05-expressions.md#r-expr.power.checked) |
| 10 "that have identity" qualifies only runtime handles | Applied: [`module.prelude.anyref`](../spec/10-modules.md#r-module.prelude.anyref) |
| "may not" becomes "must not" | Applied in 09 and 11; no other numbered chapter used it |
| STYLE.md adopts "Panic: `code`." | Applied: [Section Template](../spec/STYLE.md#section-template) and [Writing Rules](../spec/STYLE.md#writing-rules) |
| The agents' literal readings of unclear referents stand | Nothing to apply |
| 04: `types.string.host-utf8` stays normative | Obsolete: Strings STR6 retired it for `types.string.host-bytes` |
| 04: package interfaces carry generic and pack bodies; fix the Implementation Model note | Applied: [Shapes and Generic Code](../spec/04-type-system.md#shapes-and-generic-code) |
| 04: for a named function, inference covers only the result type and the requirement row | Applied: [`types.infer.named-fn`](../spec/04-type-system.md#r-types.infer.named-fn) |
| 06/04: `break` and `continue` are `never` expressions; the list of `never` expressions is five forms | Applied: [`types.never.expressions`](../spec/04-type-system.md#r-types.never.expressions), [`types.never.fits`](../spec/04-type-system.md#r-types.never.fits) |
| Fixture fix F-150: add `pub` in the entry-point fixtures | Applied to `nondisplay-entry-error.hd`, `nonhost-entry-requirement.hd`, `resource-disposed-result.hd`, and the 10 example; F-150 is closed |
| TQ-2: an expected type does not weaken freshly built elements; fix `trait-argument-owner-impl.hd` | Applied: [`types.fresh.element-no-weaken`](../spec/04-type-system.md#r-types.fresh.element-no-weaken); the fixture declares `Iterable[mut Word]` |
| 07: one declared result type in a recursive cycle is enough | Applied: [`fn.recursion.named-cycle`](../spec/07-functions.md#r-fn.recursion.named-cycle), [`fn.decl.omitted-recursion`](../spec/07-functions.md#r-fn.decl.omitted-recursion), [`types.infer.explicit.results-declared`](../spec/04-type-system.md#r-types.infer.explicit.results-declared), [`types.infer.body-result-private`](../spec/04-type-system.md#r-types.infer.body-result-private) |
| 02/11: chapter 02 is the only grammar authority | Applied: 11's four EBNF blocks became links to 02 |
| 01: confusable and mixed-script identifiers are warnings | Applied: [`lex.ident.confusable.warning`](../spec/01-lexical-structure.md#r-lex.ident.confusable.warning), [`lex.ident.mixed-script.warning`](../spec/01-lexical-structure.md#r-lex.ident.mixed-script.warning) |
| 06: `break value` in a loop without `else` is `break-value-context` | Applied: [`flow.loop.void.break`](../spec/06-control-flow.md#r-flow.loop.void.break), [`flow.loop.else.plain-break`](../spec/06-control-flow.md#r-flow.loop.else.plain-break) |
| 07: a `pub` inherent method declares its result type; inference only for non-public functions, methods, and local `fn`s | Obsolete: already applied as [`fn.decl.result-required-pub`](../spec/07-functions.md#r-fn.decl.result-required-pub) and [`req.row.omitted.inferred-private`](../spec/11-requirements-and-suspension.md#r-req.row.omitted.inferred-private) |
| A raw tab inside a string is an error | Already stated by [`lex.tab.content`](../spec/01-lexical-structure.md#r-lex.tab.content); its code, `tab-whitespace`, came with batch 10 (SF2) |
| A lone `"` inside `"""..."""` is allowed | Applied: the `multiline_string_character` class in [String And Character Literals](../spec/01-lexical-structure.md#string-and-character-literals) |
| 06's list of discarded suites is complete | Nothing to apply |
| State the vararg `List[T]` rule in words | Applied: [`fn.vararg.list`](../spec/07-functions.md#r-fn.vararg.list) |
| Name `syntax-error` consistently in 02 | Applied: every 02 rule that states a syntax error names the code, and no 02 error example is left without one |
| 11: `all!` never polls a completed child again | Applied: [`req.schedule.all-unfinished`](../spec/11-requirements-and-suspension.md#r-req.schedule.all-unfinished), [`req.schedule.all-completed`](../spec/11-requirements-and-suspension.md#r-req.schedule.all-completed) |
| 11: `req.model.no-reinterpretation` wording | Applied: [`req.model.never-reinterpreted`](../spec/11-requirements-and-suspension.md#r-req.model.never-reinterpreted) |

## Logged, not scheduled (owner: do not spend on it now)

The owner scheduled this list on 2026-09-29, and it is applied: each rule
now ends with "Error: `code`." (or "Panic: `code`."), and every bare
`# error` line of a numbered chapter names its code.

| Chapter | Codes | Result |
| --- | --- | --- |
| 03 | `duplicate-module-name`, `non-reassignable-binding`, `binding-not-yet-visible`, `possibly-uninitialized-binding`, `duplicate-binding`, `unknown-name` | Applied |
| 04 | `integer-literal-range`, `float-literal-range`, `invalid-map-key`, `mixed-signedness`, `implicit-narrowing`, `invalid-variance`, `variance-representation-change`, `identity-needs-reference-bound`, `trait-not-dynamically-safe` | Applied |
| 05 | `missing-contextual-enum-type`, `identity-requires-references`, `unsigned-negation`, `type-mismatch`, `suspension-forbidden-context` | Applied; `type-mismatch` is named where a 05 rule owns the check |
| 09 | `trait-not-dynamically-safe`, `local-impl-nonlocal-pair`, inherent impl outside the owning package | Applied; the inherent implementation's code, `orphan-impl`, came with batch 10 (SF1) |
| 10 | `direct-variant-use`, `top-level-read-before-initialization`, `private-type-leak`, `missing-requirement`, `missing-partial-eq`, `syntax-error`, `duplicate-module-name` | Applied; `duplicate-module-name` is obsolete for 10, because 03's [`names.module.unique`](../spec/03-names-and-scopes.md#r-names.module.unique) names it for every module name, uses included |
| 12 | `multiple-positional-value-packs`, `pack-length-mismatch`, `pack-map-mapper-mismatch` | Applied |
| 13 | `impossible-gadt-pattern` | Applied |
| 02 | `generic-kind-mismatch`, `comparison-chaining`, `multi-binding-needs-parentheses`, `argument-order`, `trailing-block-position`, `pattern-order`, `trait-method-visibility` | Applied |
| 11 | `missing-requirement`, `bang-call-outside-suspension`, `nonhost-entry-requirement` | Applied |
| 01 | `unexpected-bom`, `tab-whitespace`, `invalid-dedent`, `invalid-escape`, `reserved-semicolon`, `unknown-name` | Applied |
| 06 | `break-value-context`, `break-outside-loop`, `nonexhaustive-match`, `duplicate-data-pattern-field`, `return-outside-function`, `readonly-root`, `unknown-data-field`, `iterator-invalidated` | Applied |
| 07 | `missing-return-value`, `non-reassignable-parameter-binding`, `nonfinal-positional-spread`, `trailing-block-position`, `recursive-closure-needs-result-type`, `readonly-root`, `argument-count`, `type-mismatch`, `no-common-type` | Applied; the 07 `type-mismatch` fixtures test general assignability, which the general code covers without a chapter rule |

## Open, for the owner

Batch 10. The owner decided these on 2026-09-29: "accept all
recommendations". They are applied; see the
[Revision Notes](../spec/README.md#revision-notes) entry "Spec follow-ups
batch 10". The prototype does not implement them yet, so their fixtures are
known failures tagged `SF`.

| # | Point | Decision | Applied |
| --- | --- | --- | --- |
| SF1 | Which code rejects an inherent implementation outside the package that owns its target? | `orphan-impl`, the code for the same mistake with a trait implementation | [`trait.own.inherent`](../spec/09-traits.md#r-trait.own.inherent) names it, with an error example; fixture `foreign-inherent-impl.hd` |
| SF2 | Which code rejects a raw tab inside a string or character literal? | `tab-whitespace`, the one tab code | [`lex.tab.content`](../spec/01-lexical-structure.md#r-lex.tab.content) names it; the reference parser now rejects the tab; fixture `tab-in-string-literal.hd` |
| SF3 | Should `missing-partial-eq` be renamed, since EQ-1 removed `PartialEq`? | Rename it `missing-eq`, and retire the old name | [`module.testing.no-implicit-eq`](../spec/10-modules.md#r-module.testing.no-implicit-eq) and four fixtures use `missing-eq`; the Diagnostics table drops `missing-partial-eq` |
