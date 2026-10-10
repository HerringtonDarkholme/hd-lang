# New Compiler Conformance

Status: measured 2026-10-07 against `103dd1e3`. This is implementation
coverage, not accepted language behavior. The Cargo test runs every indexed
fixture; unsupported surface records progress without failing.

## Summary

| Pass | Fail | Unsupported | Total |
| ---: | ---: | ---: | ---: |
| 2459 | 298 | 88 | 2845 |

## By Chapter

| Group | Pass | Fail | Unsupported | Total |
| --- | ---: | ---: | ---: | ---: |
| `lang/01-lexical-structure.md` | 137 | 8 | 0 | 145 |
| `lang/02-grammar.md` | 198 | 10 | 7 | 215 |
| `lang/03-names-and-scopes.md` | 97 | 3 | 1 | 101 |
| `lang/04-type-system.md` | 311 | 39 | 2 | 352 |
| `lang/05-expressions.md` | 244 | 32 | 1 | 277 |
| `lang/06-control-flow.md` | 133 | 20 | 4 | 157 |
| `lang/07-functions.md` | 113 | 14 | 3 | 130 |
| `lang/08-data-and-enums.md` | 87 | 28 | 8 | 123 |
| `lang/09-traits.md` | 315 | 39 | 5 | 359 |
| `lang/10-modules.md` | 216 | 31 | 11 | 258 |
| `lang/11-requirements-and-suspension.md` | 199 | 29 | 31 | 259 |
| `lang/14-annotations.md` | 116 | 30 | 7 | 153 |
| `std/cli.md` | 5 | 0 | 0 | 5 |
| `std/cmp.md` | 14 | 0 | 0 | 14 |
| `std/collections.md` | 24 | 5 | 0 | 29 |
| `std/console.md` | 7 | 0 | 0 | 7 |
| `std/digest.md` | 2 | 0 | 0 | 2 |
| `std/encoding.md` | 3 | 0 | 0 | 3 |
| `std/error.md` | 14 | 0 | 0 | 14 |
| `std/format.md` | 7 | 1 | 0 | 8 |
| `std/fs.md` | 3 | 1 | 0 | 4 |
| `std/hash.md` | 8 | 1 | 0 | 9 |
| `std/host.md` | 2 | 0 | 0 | 2 |
| `std/http.md` | 2 | 0 | 0 | 2 |
| `std/iter.md` | 23 | 1 | 1 | 25 |
| `std/json.md` | 18 | 0 | 1 | 19 |
| `std/net.md` | 1 | 0 | 0 | 1 |
| `std/num.md` | 16 | 2 | 0 | 18 |
| `std/ops.md` | 10 | 1 | 2 | 13 |
| `std/option.md` | 3 | 0 | 0 | 3 |
| `std/path.md` | 2 | 0 | 0 | 2 |
| `std/process.md` | 3 | 0 | 0 | 3 |
| `std/random.md` | 8 | 0 | 0 | 8 |
| `std/regex.md` | 13 | 0 | 0 | 13 |
| `std/result.md` | 5 | 0 | 0 | 5 |
| `std/serde.md` | 5 | 0 | 3 | 8 |
| `std/sys.md` | 1 | 0 | 0 | 1 |
| `std/task.md` | 7 | 0 | 0 | 7 |
| `std/testing.md` | 35 | 3 | 1 | 39 |
| `std/text.md` | 35 | 0 | 0 | 35 |
| `std/time.md` | 17 | 0 | 0 | 17 |

## By Directory

| Group | Pass | Fail | Unsupported | Total |
| --- | ---: | ---: | ---: | ---: |
| `parse/invalid` | 181 | 14 | 0 | 195 |
| `parse/valid` | 106 | 0 | 0 | 106 |
| `runtime/panic` | 86 | 15 | 4 | 105 |
| `runtime/valid` | 845 | 17 | 67 | 929 |
| `typing/invalid` | 827 | 234 | 14 | 1075 |
| `typing/valid` | 403 | 9 | 3 | 415 |
| `typing/warnings` | 11 | 9 | 0 | 20 |

## Failure Buckets

A failure bucket is the first diagnostic code. An unsupported bucket is the
compiler stage that first declined the case.

| Bucket | Cases |
| --- | ---: |
| `fail:argument-count` | 5 |
| `fail:bang-call-outside-suspension` | 2 |
| `fail:bare-variant-pattern` | 3 |
| `fail:boundary-private-field` | 1 |
| `fail:cannot-infer-type` | 5 |
| `fail:duplicate-argument` | 1 |
| `fail:duplicate-data-pattern-field` | 1 |
| `fail:identity-requires-references` | 9 |
| `fail:implicit-narrowing` | 1 |
| `fail:invalid-test-statement` | 1 |
| `fail:invalid-token` | 2 |
| `fail:missing-entry-point` | 1 |
| `fail:missing-required-field` | 2 |
| `fail:missing-requirement` | 4 |
| `fail:missing-supertrait-implementation` | 3 |
| `fail:mutable-impl-target` | 1 |
| `fail:no-diagnostic` | 131 |
| `fail:nonlocal-impl` | 1 |
| `fail:orphan-impl` | 1 |
| `fail:overlapping-impl` | 1 |
| `fail:pattern-arity` | 1 |
| `fail:placeholder-outside-pipe` | 1 |
| `fail:private-import` | 1 |
| `fail:private-main` | 4 |
| `fail:re-export-loop` | 1 |
| `fail:readonly-root` | 1 |
| `fail:runtime-exit` | 11 |
| `fail:stdout` | 1 |
| `fail:suspension-forbidden-context` | 1 |
| `fail:syntax-error` | 10 |
| `fail:tab-whitespace` | 2 |
| `fail:trait-used-as-type` | 1 |
| `fail:type-mismatch` | 37 |
| `fail:unknown-data-field` | 6 |
| `fail:unknown-import` | 1 |
| `fail:unknown-method` | 14 |
| `fail:unknown-module` | 1 |
| `fail:unknown-name` | 11 |
| `fail:unknown-trait` | 7 |
| `fail:unknown-type` | 3 |
| `fail:unknown-variant` | 1 |
| `fail:unused-local-binding` | 6 |
| `unsupported:Body` | 35 |
| `unsupported:Collect` | 20 |
| `unsupported:Emit` | 2 |
| `unsupported:Link` | 3 |
| `unsupported:RunCase` | 28 |

<details><summary><code>fail:argument-count</code> (5)</summary>

- `runtime/valid/map-grow-and-remove.hd`
- `typing/invalid/variant-duplicate-argument.hd`
- `typing/invalid/duplicate-argument.hd`
- `typing/invalid/unknown-named-argument.hd`
- `typing/invalid/variant-unknown-payload-field.hd`

</details>

<details><summary><code>fail:bang-call-outside-suspension</code> (2)</summary>

- `typing/invalid/suspending-parameter-default.hd`
- `typing/invalid/suspending-data-field-default.hd`

</details>

<details><summary><code>fail:bare-variant-pattern</code> (3)</summary>

- `typing/invalid/bare-variant-pattern-in-match.hd`
- `typing/invalid/bare-some-call-pattern.hd`
- `typing/invalid/bare-ok-call-pattern.hd`

</details>

<details><summary><code>fail:boundary-private-field</code> (1)</summary>

- `runtime/valid/resource-disposed-result.hd`

</details>

<details><summary><code>fail:cannot-infer-type</code> (5)</summary>

- `typing/invalid/none-to-any.hd`
- `typing/invalid/none-without-expected-type.hd`
- `typing/invalid/handle-fact-pattern-mismatch.hd`
- `typing/invalid/eq-contextual-both-operands.hd`
- `typing/invalid/any-bare-none.hd`

</details>

<details><summary><code>fail:duplicate-argument</code> (1)</summary>

- `typing/invalid/nonfinal-spread.hd`

</details>

<details><summary><code>fail:duplicate-data-pattern-field</code> (1)</summary>

- `typing/invalid/duplicate-data-pattern-field.hd`

</details>

<details><summary><code>fail:identity-requires-references</code> (9)</summary>

- `typing/invalid/primitive-identity.hd`
- `typing/invalid/unbounded-generic-identity.hd`
- `typing/invalid/tuple-identity.hd`
- `typing/invalid/string-identity.hd`
- `typing/invalid/closure-identity.hd`
- `typing/invalid/named-function-identity.hd`
- `typing/invalid/function-field-identity.hd`
- `typing/invalid/enum-identity.hd`
- `typing/invalid/optional-identity.hd`

</details>

<details><summary><code>fail:implicit-narrowing</code> (1)</summary>

- `typing/invalid/no-widening-list-literal.hd`

</details>

<details><summary><code>fail:invalid-test-statement</code> (1)</summary>

- `runtime/valid/test-registration-renamed-import.hd`

</details>

<details><summary><code>fail:invalid-token</code> (2)</summary>

- `parse/invalid/leading-tuple-index-does-not-continue.hd`
- `parse/invalid/backslash-line-continuation.hd`

</details>

<details><summary><code>fail:missing-entry-point</code> (1)</summary>

- `runtime/valid/script-empty-run.hd`

</details>

<details><summary><code>fail:missing-required-field</code> (2)</summary>

- `typing/invalid/data-literal-unknown-field.hd`
- `typing/invalid/data-field-shorthand-unknown-name.hd`

</details>

<details><summary><code>fail:missing-requirement</code> (4)</summary>

- `typing/invalid/parameter-default-requires-provider.hd`
- `typing/invalid/data-default-requires-provider.hd`
- `typing/invalid/missing-requirement-cold-construction.hd`
- `typing/invalid/hd-run-outside-integration.hd`

</details>

<details><summary><code>fail:missing-supertrait-implementation</code> (3)</summary>

- `typing/invalid/derive-error-trait.hd`
- `typing/invalid/num-sealed-impl.hd`
- `typing/invalid/derived-ord-without-partial-ord.hd`

</details>

<details><summary><code>fail:mutable-impl-target</code> (1)</summary>

- `typing/invalid/mutable-inherent-impl-target.hd`

</details>

<details><summary><code>fail:no-diagnostic</code> (131)</summary>

- `typing/invalid/duplicate-generic-embedded-name.hd`
- `typing/invalid/float-literal-range.hd`
- `typing/invalid/tuple-map-key.hd`
- `typing/invalid/overload.hd`
- `typing/invalid/private-type-leak.hd`
- `typing/invalid/private-type-embedded-in-public-data.hd`
- `typing/warnings/unreachable-code.hd`
- `typing/invalid/duplicate-field.hd`
- `typing/invalid/unknown-annotation-member.hd`
- `typing/invalid/bare-parameter-impl-target.hd`
- `typing/invalid/child-trait-redeclares-supertrait-member.hd`
- `typing/invalid/child-trait-redeclares-transitive-supertrait-member.hd`
- `typing/invalid/nonhost-entry-requirement.hd`
- `typing/warnings/variant-binding-name-mismatch.hd`
- `typing/invalid/private-requirement-row.hd`
- `typing/invalid/duplicate-inherent-member.hd`
- `typing/invalid/suspension-forbidden-context.hd`
- `typing/invalid/nondisplay-entry-error.hd`
- `typing/warnings/unreachable-after-infinite-loop.hd`
- `typing/warnings/variant-binding-names-swapped.hd`
- `typing/invalid/generic-supertrait-upcast-mismatch.hd`
- `typing/invalid/enum-default-requires-provider.hd`
- `typing/invalid/duplicate-literal-match-arm.hd`
- `typing/invalid/incompatible-identity-operands.hd`
- `typing/invalid/function-typed-field-method-call.hd`
- `typing/invalid/embedded-copy-required.hd`
- `typing/invalid/embedded-copy-required-fresh-literal.hd`
- `typing/invalid/embedded-assignment-copy-required.hd`
- `typing/invalid/copy-into-ordinary-field.hd`
- `typing/invalid/embedding-depth-four.hd`
- `typing/invalid/embedding-depth-four-generic.hd`
- `typing/invalid/four-embedded-fields.hd`
- `typing/invalid/embedding-cycle-direct.hd`
- `typing/invalid/embedding-cycle-indirect.hd`
- `typing/invalid/embedding-cycle-generic.hd`
- `typing/invalid/inspectable-requirement-key.hd`
- `typing/invalid/inspectable-user-implementation.hd`
- `typing/invalid/inspectable-member-redeclared.hd`
- `typing/invalid/embedded-non-data.hd`
- `typing/invalid/embedded-collection-type.hd`
- `typing/invalid/embedded-type-parameter.hd`
- `typing/invalid/inherent-member-unifying-targets.hd`
- `typing/invalid/trait-resolution-depth.hd`
- `typing/invalid/entry-result-not-termination.hd`
- `typing/invalid/structure-outside-template.hd`
- `typing/invalid/marker-template.hd`
- `typing/invalid/duplicate-fact.hd`
- `typing/invalid/member-line-payload-member.hd`
- `typing/invalid/invalid-member-line.hd`
- `typing/invalid/generic-member-call.hd`
- `typing/invalid/newtype-derivation-self.hd`
- `typing/warnings/derivation-line-drift.hd`
- `typing/invalid/structure-without-use.hd`
- `typing/invalid/duplicate-tests-block.hd`
- `typing/invalid/duplicate-declaration-fact.hd`
- `typing/invalid/trait-less-block-omit.hd`
- `typing/invalid/trait-less-block-duplicate-fact.hd`
- `typing/invalid/duplicate-type-level-fact.hd`
- `typing/invalid/trait-less-block-second.hd`
- `typing/invalid/member-line-not-list.hd`
- `typing/invalid/test-module-tests-block.hd`
- `typing/invalid/integration-test-tests-block.hd`
- `typing/invalid/duplicate-function-fact.hd`
- `typing/invalid/row-union-list-no-convert.hd`
- `typing/invalid/intrinsic-method-user.hd`
- `typing/invalid/init-group-cycle.hd`
- `typing/invalid/operator-function-left-exact.hd`
- `typing/invalid/pipe-placeholder-in-closure.hd`
- `typing/invalid/pipe-multi-line-step.hd`
- `typing/invalid/type-default-bound.hd`
- `typing/invalid/error-marker-outside.hd`
- `typing/invalid/foreign-inherent-impl.hd`
- `typing/invalid/function-type-rest-not-list.hd`
- `typing/invalid/vararg-type-not-collection.hd`
- `typing/invalid/display-tuple-element-without-display.hd`
- `typing/invalid/default-tuple-element-without-default.hd`
- `typing/invalid/typed-fact-mismatch.hd`
- `typing/invalid/typed-fact-expected-mismatch.hd`
- `typing/invalid/tuple-template-overlap.hd`
- `typing/invalid/arbitrary-with-wrong-generator.hd`
- `typing/invalid/typed-fact-function-suspending.hd`
- `typing/invalid/typed-fact-bound.hd`
- `typing/invalid/typed-fact-value-mismatch.hd`
- `typing/invalid/typed-fact-fn-pattern-arity.hd`
- `typing/invalid/typed-fact-fn-pattern-field.hd`
- `typing/invalid/typed-fact-fn-pattern-bound.hd`
- `typing/invalid/typed-fact-fn-pattern-suspending.hd`
- `typing/invalid/typed-fact-concrete-pattern-mismatch.hd`
- `typing/invalid/generic-inference-variance-conflict.hd`
- `typing/invalid/generic-inference-optional-conflict.hd`
- `typing/invalid/list-float-map-key.hd`
- `typing/invalid/row-slot-bare-explicit-argument.hd`
- `typing/warnings/unsigned-comparison-countdown.hd`
- `typing/warnings/unsigned-comparison-explicit.hd`
- `typing/invalid/impl-target-key-bound-not-implied.hd`
- `typing/invalid/signature-key-bound-not-implied.hd`
- `typing/invalid/field-key-bound-not-implied.hd`
- `typing/invalid/inspectable-requirement-function-type.hd`
- `typing/invalid/inspectable-requirement-provider-scope.hd`
- `typing/invalid/nested-local-annotation-bare-trait.hd`
- `typing/invalid/contravariant-inferred-private-result.hd`
- `typing/invalid/contravariant-inferred-invariant-result.hd`
- `typing/invalid/ambiguous-requirement-key-solution.hd`
- `typing/invalid/method-type-parameter-reuses-impl-parameter.hd`
- `typing/invalid/module-named-pkg.hd`
- `typing/invalid/duplicate-variant.hd`
- `typing/warnings/mixed-script-identifier.hd`
- `typing/invalid/script-test-init-requirement.hd`
- `typing/warnings/unused-debug-text.hd`
- `typing/invalid/duplicate-field-declaration.hd`
- `typing/invalid/data-field-shorthand-duplicate.hd`
- `typing/invalid/assoc-call-parameter-two-bounds.hd`
- `typing/invalid/dyn-self-parameter-unavailable.hd`
- `typing/invalid/sealed-member-written-in-impl.hd`
- `typing/invalid/typeid-of-never.hd`
- `typing/invalid/child-trait-redeclares-associated-type.hd`
- `typing/invalid/child-trait-redeclares-associated-function.hd`
- `typing/invalid/private-type-leak-data-field.hd`
- `typing/invalid/private-type-leak-enum-payload.hd`
- `typing/invalid/private-type-leak-trait-bound.hd`
- `typing/invalid/uninhabited-binding.hd`
- `typing/invalid/extra-trait-member.hd`
- `typing/invalid/duplicate-supertrait.hd`
- `typing/invalid/trait-assoc-call-self-undetermined.hd`
- `typing/invalid/shared-enum-payload-name-duplicate.hd`
- `typing/invalid/entry-point-parameters.hd`
- `typing/invalid/generic-entry-point.hd`
- `typing/invalid/module-path-not-identifier.hd`
- `typing/invalid/template-helper-missing-result.hd`
- `typing/invalid/dyn-inherent-nonlocal.hd`
- `typing/invalid/template-names-binding.hd`

</details>

<details><summary><code>fail:nonlocal-impl</code> (1)</summary>

- `typing/invalid/nonlocal-impl.hd`

</details>

<details><summary><code>fail:orphan-impl</code> (1)</summary>

- `typing/invalid/from-reflexive-impl.hd`

</details>

<details><summary><code>fail:overlapping-impl</code> (1)</summary>

- `typing/invalid/num-sealed-user-number.hd`

</details>

<details><summary><code>fail:pattern-arity</code> (1)</summary>

- `typing/invalid/tuple-binding-arity.hd`

</details>

<details><summary><code>fail:placeholder-outside-pipe</code> (1)</summary>

- `typing/invalid/pipe-duplicate-placeholder.hd`

</details>

<details><summary><code>fail:private-import</code> (1)</summary>

- `typing/invalid/private-std-name-in-group.hd`

</details>

<details><summary><code>fail:private-main</code> (4)</summary>

- `typing/invalid/unresolved-generic-return-placeholder.hd`
- `typing/invalid/prelude-shadow-hash-local.hd`
- `typing/invalid/unused-cold-suspension.hd`
- `typing/invalid/trailing-block-ineligible.hd`

</details>

<details><summary><code>fail:re-export-loop</code> (1)</summary>

- `typing/invalid/pub-use-loop.hd`

</details>

<details><summary><code>fail:readonly-root</code> (1)</summary>

- `typing/invalid/callable-value-short-binding-store.hd`

</details>

<details><summary><code>fail:runtime-exit</code> (11)</summary>

- `runtime/panic/for-loop-iterator-invalidated.hd`
- `runtime/panic/invalidated-iterator.hd`
- `runtime/panic/defer-block-on-indirect.hd`
- `runtime/panic/for-iterator-invalidated-via-helper.hd`
- `runtime/panic/exhausted-iterator-invalidated-by-growth.hd`
- `runtime/panic/alias-growth-invalidates-readonly-iterator.hd`
- `runtime/panic/list-view-reversed.hd`
- `runtime/panic/list-view-index-out-of-bounds.hd`
- `runtime/panic/list-view-invalidated.hd`
- `runtime/panic/deque-invalidated.hd`
- `runtime/panic/unbounded-recursion.hd`

</details>

<details><summary><code>fail:stdout</code> (1)</summary>

- `runtime/valid/crlf-line-endings.hd`

</details>

<details><summary><code>fail:suspension-forbidden-context</code> (1)</summary>

- `typing/invalid/defer-bang-call-cold-twin.hd`

</details>

<details><summary><code>fail:syntax-error</code> (10)</summary>

- `parse/invalid/old-and-operator.hd`
- `parse/invalid/unparenthesized-result-row.hd`
- `parse/invalid/old-where-clause.hd`
- `parse/invalid/mut-function-type.hd`
- `parse/invalid/mut-closure-literal.hd`
- `parse/invalid/reserved-word-suffix.hd`
- `parse/invalid/pipe-step-trailing-block.hd`
- `parse/invalid/trailing-block-in-same-line-suite.hd`
- `parse/invalid/for-bare-name-list.hd`
- `parse/invalid/range-inclusive-no-end.hd`

</details>

<details><summary><code>fail:tab-whitespace</code> (2)</summary>

- `parse/invalid/tab-indentation.hd`
- `parse/invalid/tab-in-function-body.hd`

</details>

<details><summary><code>fail:trait-used-as-type</code> (1)</summary>

- `typing/invalid/callback-parameter-bare-trait.hd`

</details>

<details><summary><code>fail:type-mismatch</code> (37)</summary>

- `typing/invalid/heterogeneous-list.hd`
- `typing/invalid/mixed-signedness.hd`
- `typing/invalid/variance-representation-change.hd`
- `typing/invalid/derived-generic-bound.hd`
- `typing/invalid/contravariant-representation-change.hd`
- `typing/invalid/heterogeneous-list-bool-int.hd`
- `typing/invalid/heterogeneous-map-values.hd`
- `runtime/valid/multi-provider-use-order.hd`
- `runtime/valid/suspending-function-values.hd`
- `runtime/valid/requirement-row-union-and-order.hd`
- `typing/invalid/closure-returns-without-common-type.hd`
- `runtime/valid/nested-optional-layers.hd`
- `typing/invalid/none-beside-values-needs-expected-type.hd`
- `typing/invalid/none-branch-needs-expected-type.hd`
- `typing/invalid/least-common-type-supertrait-widening.hd`
- `typing/invalid/function-result-representation-change.hd`
- `typing/invalid/row-union-other-parts.hd`
- `typing/invalid/row-union-nested-lists.hd`
- `typing/invalid/row-union-if-nested-lists.hd`
- `typing/invalid/operator-left-literal.hd`
- `runtime/valid/intrinsic-method-calls.hd`
- `typing/invalid/placeholder-outside-pipe.hd`
- `typing/invalid/generic-inference-trait-value.hd`
- `typing/invalid/range-mixed-signedness.hd`
- `typing/invalid/generic-inference-supertrait-widening.hd`
- `typing/invalid/no-widening-operator.hd`
- `typing/invalid/no-widening-float.hd`
- `runtime/valid/lct-optional-injection.hd`
- `typing/invalid/lct-optional-two-layers.hd`
- `typing/invalid/num-abs-diff-unsigned.hd`
- `typing/invalid/num-bit-count-u32.hd`
- `typing/valid/literal-one-fit-left-operand.hd`
- `typing/valid/test-runner-every-registration-form.hd`
- `typing/invalid/literal-operand-range-wide.hd`
- `typing/invalid/mixed-width-operands-no-widening.hd`
- `typing/invalid/usize-u32-operand.hd`
- `typing/invalid/derived-eq-bound-unmet-data-argument.hd`

</details>

<details><summary><code>fail:unknown-data-field</code> (6)</summary>

- `typing/invalid/tuple-index-out-of-range.hd`
- `typing/valid/type-default-declarations.hd`
- `typing/valid/let-pattern-mut.hd`
- `typing/invalid/let-pattern-mut-readonly-field.hd`
- `runtime/valid/impl-owned-target-and-trait-argument.hd`
- `typing/invalid/default-body-self-field.hd`

</details>

<details><summary><code>fail:unknown-import</code> (1)</summary>

- `typing/invalid/use-through-pub-use-loop.hd`

</details>

<details><summary><code>fail:unknown-method</code> (14)</summary>

- `typing/valid/mutable-suspension.hd`
- `typing/invalid/readonly-suspension-cancel.hd`
- `runtime/panic/reentrant-cancel.hd`
- `runtime/valid/cold-suspension-cancel-idempotent.hd`
- `runtime/panic/drive-cancelled-suspension.hd`
- `typing/invalid/cancel-readonly-suspension.hd`
- `runtime/panic/block-on-cancelled-suspension.hd`
- `runtime/valid/cancel-cold-and-completed-suspension.hd`
- `runtime/panic/defer-cancels-active-ancestor.hd`
- `runtime/valid/memory-fs-directories.hd`
- `typing/valid/std-types-debug.hd`
- `typing/invalid/literal-var-method-missing.hd`
- `typing/invalid/iterator-sum-non-numeric.hd`
- `typing/invalid/list-sum-non-numeric.hd`

</details>

<details><summary><code>fail:unknown-module</code> (1)</summary>

- `typing/invalid/non-test-code-uses-test-module.hd`

</details>

<details><summary><code>fail:unknown-name</code> (11)</summary>

- `typing/valid/recursive-local-closure.hd`
- `typing/invalid/recursive-closure-inferred-result.hd`
- `typing/valid/numeric-types.hd`
- `typing/invalid/parameter-default-later-parameter.hd`
- `runtime/valid/recursive-local-closure.hd`
- `typing/invalid/positional-after-spread.hd`
- `typing/invalid/default-later-parameter-earlier-twin.hd`
- `typing/invalid/variant-pattern-unknown-field.hd`
- `typing/invalid/misplaced-test-case.hd`
- `typing/invalid/test-case-as-value.hd`
- `runtime/valid/raw-identifiers-as-names.hd`

</details>

<details><summary><code>fail:unknown-trait</code> (7)</summary>

- `typing/invalid/user-suspend-implementation.hd`
- `typing/invalid/reference-trait-name-unknown.hd`
- `typing/invalid/row-parameter-on-data.hd`
- `typing/invalid/row-parameter-on-trait.hd`
- `typing/valid/folder-cycle-leaf-folder.hd`
- `typing/invalid/row-parameter-on-newtype.hd`
- `typing/invalid/row-parameter-unmarked.hd`

</details>

<details><summary><code>fail:unknown-type</code> (3)</summary>

- `typing/invalid/row-parameter-as-type.hd`
- `typing/invalid/sibling-module-alias-not-imported.hd`
- `typing/invalid/fn-constructor-needs-import.hd`

</details>

<details><summary><code>fail:unknown-variant</code> (1)</summary>

- `typing/invalid/some-pattern-nonoptional.hd`

</details>

<details><summary><code>fail:unused-local-binding</code> (6)</summary>

- `typing/invalid/least-type-weakening-variance.hd`
- `typing/invalid/closure-parameter-without-type.hd`
- `typing/invalid/mut-map-key.hd`
- `typing/invalid/all-function-value.hd`
- `typing/invalid/range-float-bound.hd`
- `typing/invalid/local-annotation-bare-trait.hd`

</details>

<details><summary><code>unsupported:Body</code> (35)</summary>

- `typing/valid/enums.hd`
- `typing/invalid/literal-payload-pattern-nonexhaustive.hd`
- `runtime/valid/literal-payload-patterns.hd`
- `runtime/valid/binding-expressions.hd`
- `runtime/valid/suspending-generic-method.hd`
- `runtime/valid/generic-suspending-associated-function-qualified-call.hd`
- `typing/invalid/for-over-non-iterable.hd`
- `runtime/valid/tuple-impl-target.hd`
- `runtime/valid/bang-call-explicit-type-arguments.hd`
- `runtime/valid/embedded-store-copies.hd`
- `typing/invalid/copy-assignment-ordinary-field.hd`
- `runtime/valid/part-copy-is-copy-update.hd`
- `typing/invalid/lowercase-list-type.hd`
- `typing/valid/type-default-trait-method.hd`
- `typing/valid/newtype-unwrap-permission.hd`
- `typing/invalid/newtype-unwrap-readonly.hd`
- `typing/invalid/race-no-tasks.hd`
- `typing/invalid/all-explicit-type-arguments.hd`
- `typing/invalid/race-empty-list-literal.hd`
- `runtime/valid/module-qualified-variant.hd`
- `runtime/valid/module-qualified-associated-call.hd`
- `typing/invalid/local-annotation-row-unknown-trait.hd`
- `typing/invalid/closure-row-unknown-trait.hd`
- `typing/invalid/provider-scope-unknown-trait.hd`
- `runtime/valid/shared-enum-data-computed-once.hd`
- `runtime/valid/trait-default-method-instantiates-params.hd`
- `runtime/valid/iterator-list-sum.hd`
- `typing/invalid/sibling-module-requirement-key-not-imported.hd`
- `typing/invalid/set-type-not-prelude.hd`
- `runtime/valid/let-patterns-and-let-else.hd`
- `runtime/valid/binding-chain-with-suite.hd`
- `runtime/valid/pattern-forms.hd`
- `runtime/valid/assignment-and-break-forms.hd`
- `typing/invalid/grammar-assignment-nonplace.hd`
- `runtime/valid/bang-call-and-prefix-not.hd`

</details>

<details><summary><code>unsupported:Collect</code> (20)</summary>

- `runtime/valid/derived-newtype.hd`
- `runtime/valid/typed-derivation-walk.hd`
- `runtime/valid/data-variant-facts-empty.hd`
- `runtime/valid/trait-less-block-facts.hd`
- `runtime/valid/member-line-list-expression.hd`
- `runtime/valid/init-group-order.hd`
- `runtime/valid/derived-arbitrary-with.hd`
- `runtime/valid/structure-qualified-self.hd`
- `runtime/valid/derived-default-enum.hd`
- `runtime/valid/derived-default-declared-no-bound.hd`
- `runtime/valid/derived-debug-newtype.hd`
- `runtime/valid/handle-fact-exact-type.hd`
- `runtime/valid/serde-std-writes.hd`
- `runtime/valid/serde-derive-read-order.hd`
- `runtime/valid/serde-variant-member-facts.hd`
- `runtime/valid/json-typed-members.hd`
- `runtime/valid/init-group-statements-by-identity.hd`
- `runtime/panic/fact-evaluation-panics-on-read.hd`
- `runtime/valid/derive-members-of-data-and-enums.hd`
- `runtime/valid/type-declaration-forms.hd`

</details>

<details><summary><code>unsupported:Emit</code> (2)</summary>

- `runtime/valid/suspending-calls-in-branches.hd`
- `runtime/valid/suspending-match-guards.hd`

</details>

<details><summary><code>unsupported:Link</code> (3)</summary>

- `runtime/valid/boundary-derived-round-trip.hd`
- `runtime/panic/boundary-deserialize-error.hd`
- `runtime/valid/boundary-redacted-round-trip.hd`

</details>

<details><summary><code>unsupported:RunCase</code> (28)</summary>

- `runtime/valid/cancellation-unwinds-nested-frames.hd`
- `runtime/valid/cancellation-unwinds-suspending-closure.hd`
- `runtime/panic/competing-suspension-drivers.hd`
- `runtime/panic/reentrant-suspension-poll.hd`
- `runtime/valid/cancellation-runs-defer.hd`
- `runtime/valid/cancellation-runs-dynamic-method-defer.hd`
- `runtime/valid/cancellation-cleanup-order.hd`
- `runtime/valid/cancellation-skips-unreached-defer.hd`
- `runtime/valid/cancellation-lifo-across-three-frames.hd`
- `runtime/valid/cancellation-loop-and-branch-scopes.hd`
- `runtime/valid/cancellation-provider-scope-cleanup.hd`
- `runtime/valid/folder-graph-test-edges.hd`
- `runtime/valid/integration-shared-use.hd`
- `runtime/valid/pending-first-poll-argument-order.hd`
- `runtime/valid/pending-first-poll-branches.hd`
- `runtime/valid/pending-first-poll-comprehension-propagation.hd`
- `runtime/valid/pending-first-poll-dynamic-method.hd`
- `runtime/valid/pending-first-poll-generic-callable-list.hd`
- `runtime/valid/pending-first-poll-loops.hd`
- `runtime/valid/pending-first-poll-match-guards.hd`
- `runtime/valid/pending-first-poll-nested-return-defer.hd`
- `runtime/valid/pending-first-poll-result-defer.hd`
- `runtime/valid/pending-first-poll-result-propagation.hd`
- `runtime/valid/pending-first-poll-return-wide-result.hd`
- `runtime/valid/pending-first-poll-void-return-operand.hd`
- `runtime/valid/pending-first-poll-void-return.hd`
- `runtime/valid/integration-test-public-view.hd`
- `runtime/valid/test-module-uses-test-module.hd`

</details>

## Checked-In Pass List

`HD_UPDATE_CONFORMANCE=1` replaces this list with every case that passes.

<!-- pass-list-start -->
```text
parse/invalid/associated-binding-outside-bound.hd
parse/invalid/bare-binding-list.hd
parse/invalid/bare-carriage-return.hd
parse/invalid/bare-hex-prefix.hd
parse/invalid/bare-octal-prefix.hd
parse/invalid/bare-row-list-in-type.hd
parse/invalid/binary-literal-bad-digit.hd
parse/invalid/binary-literal-letter-suffix.hd
parse/invalid/binding-before-positional-argument.hd
parse/invalid/binding-list-one-name.hd
parse/invalid/brace-escape.hd
parse/invalid/call-suffix-on-next-line.hd
parse/invalid/chained-comparison.hd
parse/invalid/character-multiple-scalars.hd
parse/invalid/closure-body-absorbs-next-argument.hd
parse/invalid/closure-body-followed-by-argument-line.hd
parse/invalid/closure-body-line-ends-in-closer.hd
parse/invalid/closure-line-between-header-and-body.hd
parse/invalid/closure-type-parameters.hd
parse/invalid/colon-generic-bound.hd
parse/invalid/colon-line-after-if-suite.hd
parse/invalid/colon-supertrait.hd
parse/invalid/comparison-chaining-with-names.hd
parse/invalid/comprehension-bare-name-list.hd
parse/invalid/copy-update-spread-not-first.hd
parse/invalid/data-field-without-type.hd
parse/invalid/data-pattern-equals-label.hd
parse/invalid/declaration-without-let.hd
parse/invalid/dedent-to-unused-column.hd
parse/invalid/derive-before-alias.hd
parse/invalid/direct-variant-use.hd
parse/invalid/discard-binding-ends-in-suite.hd
parse/invalid/doc-comment-without-target.hd
parse/invalid/double-numeric-separator.hd
parse/invalid/dyn-mut-order.hd
parse/invalid/empty-raw-identifier.hd
parse/invalid/float-bare-leading-point.hd
parse/invalid/float-separator-after-exponent-marker.hd
parse/invalid/float-separator-before-point.hd
parse/invalid/float-trailing-separator.hd
parse/invalid/for-mut-name.hd
parse/invalid/for-pattern-mut.hd
parse/invalid/function-type-mutable-permission.hd
parse/invalid/generic-parameter-ellipsis.hd
parse/invalid/header-after-indented-suite.hd
parse/invalid/hex-literal-letter-suffix.hd
parse/invalid/hexadecimal-float.hd
parse/invalid/impl-generic-default.hd
parse/invalid/impl-header-associated-binding.hd
parse/invalid/index-suffix-on-next-line.hd
parse/invalid/invalid-escape.hd
parse/invalid/keyword-type-as-parameter-type.hd
parse/invalid/leading-dot-after-open-same-line-suite.hd
parse/invalid/leading-operator-does-not-continue.hd
parse/invalid/let-bare-list.hd
parse/invalid/let-list-one-name.hd
parse/invalid/let-mut-bare-list.hd
parse/invalid/let-mut-short-binding.hd
parse/invalid/local-decorator.hd
parse/invalid/map-key-same-line-conditional.hd
parse/invalid/match-arm-mut-pattern.hd
parse/invalid/mid-file-bom.hd
parse/invalid/module-documentation-second-block.hd
parse/invalid/multi-binding-in-list.hd
parse/invalid/multi-binding-nested.hd
parse/invalid/mut-non-self-parameter.hd
parse/invalid/mutable-embedded-field.hd
parse/invalid/mutable-field-modifier.hd
parse/invalid/mutable-provider-binding.hd
parse/invalid/mutable-provider-use.hd
parse/invalid/mutable-requirement-key.hd
parse/invalid/named-pattern-before-positional.hd
parse/invalid/nested-same-line-if.hd
parse/invalid/nested-suite-body-left-of-header-line.hd
parse/invalid/nested-suite-body-level-with-header.hd
parse/invalid/nested-suite-body-level-with-statement.hd
parse/invalid/nested-tests-block.hd
parse/invalid/non-nfc-identifier.hd
parse/invalid/numeric-member-access-float.hd
parse/invalid/numeric-member-access.hd
parse/invalid/old-bound-operator-generic.hd
parse/invalid/old-bound-operator-impl.hd
parse/invalid/old-bound-operator-supertrait.hd
parse/invalid/old-export-declaration.hd
parse/invalid/old-import-declaration.hd
parse/invalid/old-not-operator.hd
parse/invalid/old-or-operator.hd
parse/invalid/old-row-separator-callback.hd
parse/invalid/old-row-separator-header.hd
parse/invalid/old-row-separator-in-context.hd
parse/invalid/old-row-separator-in-type.hd
parse/invalid/old-row-separator-type-argument.hd
parse/invalid/old-struct-declaration.hd
parse/invalid/optional-question-pattern.hd
parse/invalid/out-of-range-unicode-escape.hd
parse/invalid/parenthesized-row.hd
parse/invalid/pipe-leading-dot-after-bare-step.hd
parse/invalid/pipe-leading-dot-expression-rule.hd
parse/invalid/pipe-leading-dot-line.hd
parse/invalid/pipe-leading-line-open-suite.hd
parse/invalid/placeholder-in-ordinary-type.hd
parse/invalid/positional-after-named-argument.hd
parse/invalid/positional-after-named.hd
parse/invalid/power-compound-assignment.hd
parse/invalid/prefix-context-spread-in-context.hd
parse/invalid/prefix-context-spread-in-scope.hd
parse/invalid/prefix-separator-without-digits.hd
parse/invalid/prefixed-string-pattern.hd
parse/invalid/prefixed-string-reserved-dollar.hd
parse/invalid/pub-before-impl.hd
parse/invalid/pub-embedded-field.hd
parse/invalid/pub-path-only-use.hd
parse/invalid/pub-top-level-binding.hd
parse/invalid/pub-trait-method.hd
parse/invalid/qualified-bang-call-type-arguments-before-bang.hd
parse/invalid/qualified-call-binding.hd
parse/invalid/qualified-string-prefix-value.hd
parse/invalid/radix-literal-quote-suffix.hd
parse/invalid/radix-literal-suffix-without-separator.hd
parse/invalid/range-chained.hd
parse/invalid/range-glued-to-spread.hd
parse/invalid/range-pattern-char-bound.hd
parse/invalid/raw-character-literal.hd
parse/invalid/raw-identifier-around-other-text.hd
parse/invalid/raw-identifier-lone-underscore.hd
parse/invalid/raw-identifier-unclosed.hd
parse/invalid/recovery-first-error-in-statement.hd
parse/invalid/reified-generic-parameter.hd
parse/invalid/repeated-numeric-separator.hd
parse/invalid/reserved-word-interpolation.hd
parse/invalid/reserved-word-member-name.hd
parse/invalid/reserved-word-prefix.hd
parse/invalid/return-binding-ends-in-suite.hd
parse/invalid/row-alias-and-operator.hd
parse/invalid/row-parameter-bound.hd
parse/invalid/row-subtraction.hd
parse/invalid/same-line-if-in-defer-suite.hd
parse/invalid/same-line-if-in-function-body.hd
parse/invalid/same-line-let-comma.hd
parse/invalid/same-line-suite-bare-let-list.hd
parse/invalid/same-line-suite-comma.hd
parse/invalid/semicolon-after-binding.hd
parse/invalid/semicolon.hd
parse/invalid/short-binding-data-pattern.hd
parse/invalid/short-binding-tuple-pattern.hd
parse/invalid/short-binding-tuple-same-line.hd
parse/invalid/short-binding-variant-pattern.hd
parse/invalid/spread-pattern-not-last.hd
parse/invalid/string-literal-suffix.hd
parse/invalid/suffix-after-separator.hd
parse/invalid/suffixed-literal-pattern.hd
parse/invalid/surrogate-unicode-escape.hd
parse/invalid/tab-between-tokens.hd
parse/invalid/tab-in-string-literal.hd
parse/invalid/tests-as-identifier.hd
parse/invalid/tests-block-in-function.hd
parse/invalid/trailing-block-in-brackets.hd
parse/invalid/trailing-block-same-line-body.hd
parse/invalid/trailing-integer-separator.hd
parse/invalid/trait-parameter-without-type.hd
parse/invalid/trait-qualified-brackets-without-marker.hd
parse/invalid/tuple-element-spread.hd
parse/invalid/tuple-rest-not-last.hd
parse/invalid/tuple-spread-not-last.hd
parse/invalid/tuple-type-ellipsis.hd
parse/invalid/two-trailing-blocks.hd
parse/invalid/type-in-place-of-type-keyword.hd
parse/invalid/type-name-brackets-without-marker.hd
parse/invalid/unclosed-delimiter-suppresses-fallout.hd
parse/invalid/unclosed-delimiter.hd
parse/invalid/unmatched-delimiter.hd
parse/invalid/use-declaration-needs-root.hd
parse/invalid/vararg-default-before-ellipsis.hd
parse/invalid/vararg-ellipsis-after-type.hd
parse/invalid/vararg-with-default.hd
parse/invalid/variant-field-block.hd
parse/invalid/variant-named-argument-before-positional.hd
parse/invalid/variant-payload-default.hd
parse/invalid/variant-result-type-removed.hd
parse/invalid/variant-type-parameters-removed.hd
parse/invalid/wildcard-use.hd
parse/valid/bang-and-type-argument-forms.hd
parse/valid/binding-list.hd
parse/valid/binding-same-line-if-else.hd
parse/valid/bound-and-lists.hd
parse/valid/bracketed-for-multi-name-binding.hd
parse/valid/bracketed-suite-header-line.hd
parse/valid/closing-delimiter-ends-nested-suite.hd
parse/valid/closure-body-ends-at-comma-line.hd
parse/valid/compound-assignment-tokens.hd
parse/valid/consecutive-trailing-block-calls.hd
parse/valid/context-spread-after-binding.hd
parse/valid/copy-update-spread.hd
parse/valid/curried-function-type.hd
parse/valid/data-embedding.hd
parse/valid/data-field-shorthand.hd
parse/valid/declarations.hd
parse/valid/decorator-lines-on-every-target.hd
parse/valid/decorators.hd
parse/valid/defer-same-line-nested-suite.hd
parse/valid/derivation-member-lines.hd
parse/valid/doc-comments.hd
parse/valid/documentation-comments.hd
parse/valid/embedded-copy-syntax.hd
parse/valid/embedded-data-member.hd
parse/valid/every-operator-and-delimiter-token.hd
parse/valid/explicit-generic-arguments-with-placeholder.hd
parse/valid/explicit-generic-data-arguments.hd
parse/valid/expression-interpolation.hd
parse/valid/expressions.hd
parse/valid/for-data-pattern.hd
parse/valid/for-tuple-binding-else.hd
parse/valid/functions-and-closures.hd
parse/valid/grammar-disambiguation.hd
parse/valid/header-resumes-after-nested-suite.hd
parse/valid/identifier-interpolation.hd
parse/valid/inherent-and-trait-impls.hd
parse/valid/layout.hd
parse/valid/leading-dot-continuation.hd
parse/valid/let-else-same-line.hd
parse/valid/let-list-without-mut.hd
parse/valid/let-mut-names.hd
parse/valid/list-item-binding-expression.hd
parse/valid/literal-field-comparison-value.hd
parse/valid/literal-payload-pattern.hd
parse/valid/logical-operator-words-are-identifiers.hd
parse/valid/method-references.hd
parse/valid/module-documentation.hd
parse/valid/multi-provider-use.hd
parse/valid/mut-self-receiver.hd
parse/valid/mutable-list-parameter-index-assignment.hd
parse/valid/mutable-parameter-field-assignment.hd
parse/valid/named-call-arguments.hd
parse/valid/named-local-function.hd
parse/valid/named-payload-pattern-labels.hd
parse/valid/nested-layout.hd
parse/valid/nested-named-arguments.hd
parse/valid/nested-result-payload-pattern.hd
parse/valid/nested-suite-body-deeper-than-statement.hd
parse/valid/next-line-else-after-same-line-if.hd
parse/valid/numeric-member-selectors.hd
parse/valid/operator-and-member-lines-continue-in-brackets.hd
parse/valid/parameter-defaults.hd
parse/valid/paren-line-new-statement.hd
parse/valid/pipe-expressions.hd
parse/valid/prefixed-strings.hd
parse/valid/public-declarations.hd
parse/valid/range-expressions.hd
parse/valid/range-patterns.hd
parse/valid/raw-string-dollars.hd
parse/valid/raw-string-escaped-quote.hd
parse/valid/requirement-key-bindings.hd
parse/valid/requirement-row-lists.hd
parse/valid/reserved-receiver-names.hd
parse/valid/same-line-for-list.hd
parse/valid/same-line-loop-else.hd
parse/valid/same-line-suite-let-list.hd
parse/valid/shared-enum-fields.hd
parse/valid/single-and-grouped-use.hd
parse/valid/spread-before-named-argument.hd
parse/valid/spread-pattern-syntax.hd
parse/valid/stray-dollar-in-string.hd
parse/valid/stray-dollar.hd
parse/valid/string-in-interpolation.hd
parse/valid/suffixed-literals.hd
parse/valid/supertrait-binding.hd
parse/valid/test-block-discard.hd
parse/valid/test-block.hd
parse/valid/trailing-block-under-if.hd
parse/valid/trailing-callback-statement.hd
parse/valid/trailing-doc-comments-are-commentary.hd
parse/valid/trait-less-derivation-block.hd
parse/valid/trait-value-bindings.hd
parse/valid/tuple-element-binding-expression.hd
parse/valid/tuple-rest-syntax.hd
parse/valid/tuples-and-groups.hd
parse/valid/type-argument-defaults.hd
parse/valid/type-forms-and-row-owners.hd
parse/valid/unicode-escape.hd
parse/valid/unicode-identifiers.hd
parse/valid/unicode-scalar-escape.hd
parse/valid/use-declaration-forms.hd
parse/valid/use-group-trailing-comma.hd
parse/valid/uses.hd
parse/valid/utf8-bom.hd
parse/valid/vararg-and-spread.hd
parse/valid/while-else.hd
runtime/panic/assert-equal-bool-unequal.hd
runtime/panic/assert-equal-char-unequal.hd
runtime/panic/assert-equal-f64-unequal.hd
runtime/panic/assert-equal-generic-nominal-unequal.hd
runtime/panic/assert-equal-generic-primitive-unequal.hd
runtime/panic/assert-equal-i32-unequal.hd
runtime/panic/assert-equal-list-unequal.hd
runtime/panic/assert-equal-lists-in-main.hd
runtime/panic/assert-equal-map-unequal.hd
runtime/panic/assert-equal-nominal-unequal.hd
runtime/panic/assert-equal-optional-unequal.hd
runtime/panic/assert-equal-result-unequal.hd
runtime/panic/assert-equal-string-unequal.hd
runtime/panic/assert-equal-tuple-unequal.hd
runtime/panic/assert-failure-in-main.hd
runtime/panic/block-on-completed-suspension.hd
runtime/panic/char-to-digit-radix-high.hd
runtime/panic/char-to-digit-radix-low.hd
runtime/panic/cli-duplicate-option.hd
runtime/panic/cmp-clamp-reversed.hd
runtime/panic/compound-assign-map-missing-key.hd
runtime/panic/doc-test-failing-assert.hd
runtime/panic/duration-add-overflow.hd
runtime/panic/duration-suffix-overflow.hd
runtime/panic/explicit-panic-skips-defer.hd
runtime/panic/explicit-panic.hd
runtime/panic/for-map-insert-invalidates.hd
runtime/panic/for-map-remove-invalidates.hd
runtime/panic/generic-i32-overflow.hd
runtime/panic/host-result-out-of-range.hd
runtime/panic/i32-add-overflow-in-function.hd
runtime/panic/i32-min-divided-by-minus-one.hd
runtime/panic/index-trait-map-missing.hd
runtime/panic/integer-add-overflow.hd
runtime/panic/integer-divide-by-zero.hd
runtime/panic/integer-negation-overflow.hd
runtime/panic/integer-power-overflow.hd
runtime/panic/intrinsic-method-overflow.hd
runtime/panic/invalid-shift.hd
runtime/panic/iterator-sum-overflow.hd
runtime/panic/list-chunks-zero.hd
runtime/panic/list-index-out-of-bounds.hd
runtime/panic/list-index-u64-beyond-u32.hd
runtime/panic/list-index-underflow.hd
runtime/panic/list-insert-out-of-range.hd
runtime/panic/list-remove-at-out-of-range.hd
runtime/panic/list-set-out-of-bounds.hd
runtime/panic/list-set-u64-beyond-u32.hd
runtime/panic/list-slice-out-of-range.hd
runtime/panic/list-slice-reversed.hd
runtime/panic/list-sum-overflow.hd
runtime/panic/list-windows-size.hd
runtime/panic/literal-var-fallback-overflow.hd
runtime/panic/manual-clock-negative-sleep.hd
runtime/panic/map-index-missing-key.hd
runtime/panic/map-iterator-invalidated.hd
runtime/panic/num-from-i64-overflow.hd
runtime/panic/num-to-fixed-digits-range.hd
runtime/panic/operator-generic-overflow.hd
runtime/panic/option-expect-none.hd
runtime/panic/println-console-closed.hd
runtime/panic/println-in-default-indirect.hd
runtime/panic/race-empty-at-run-time.hd
runtime/panic/range-from-overflow.hd
runtime/panic/removed-map-iterator-invalidated.hd
runtime/panic/result-expect-err.hd
runtime/panic/rng-int-empty-range.hd
runtime/panic/rng-sample-too-many.hd
runtime/panic/second-drive-of-completed-suspension.hd
runtime/panic/second-suspension-drive.hd
runtime/panic/set-invalidated.hd
runtime/panic/sign-fallback-balance-underflow.hd
runtime/panic/signed-min-division-overflow.hd
runtime/panic/snapshot-file-missing.hd
runtime/panic/snapshot-mismatch.hd
runtime/panic/string-index-out-of-bounds.hd
runtime/panic/string-slice-inside-scalar.hd
runtime/panic/string-slice-past-end.hd
runtime/panic/string-slice-range-boundary.hd
runtime/panic/string-slice-range-out-of-range.hd
runtime/panic/string-slice-range-reversed.hd
runtime/panic/string-slice-reversed.hd
runtime/panic/structure-variant-mismatch.hd
runtime/panic/u8-add-overflow.hd
runtime/panic/usize-len-underflow.hd
runtime/panic/usize-max-overflow-wasm32.hd
runtime/valid/abrupt-assignment-stores-nothing.hd
runtime/valid/any-accepts-non-optional-values.hd
runtime/valid/assert-equal-bool.hd
runtime/valid/assert-equal-char.hd
runtime/valid/assert-equal-cross-check.hd
runtime/valid/assert-equal-f64.hd
runtime/valid/assert-equal-float-edge-values.hd
runtime/valid/assert-equal-generic-nominal.hd
runtime/valid/assert-equal-generic-primitive.hd
runtime/valid/assert-equal-i32.hd
runtime/valid/assert-equal-list.hd
runtime/valid/assert-equal-map.hd
runtime/valid/assert-equal-nested-tuple.hd
runtime/valid/assert-equal-nominal.hd
runtime/valid/assert-equal-optional.hd
runtime/valid/assert-equal-result.hd
runtime/valid/assert-equal-string.hd
runtime/valid/assert-equal-tuple.hd
runtime/valid/assert.hd
runtime/valid/assignment-place-before-value.hd
runtime/valid/associated-binding-positions.hd
runtime/valid/associated-closure-named-args.hd
runtime/valid/associated-function-calls.hd
runtime/valid/associated-function-qualified-call.hd
runtime/valid/associated-type-bindings.hd
runtime/valid/associated-type-projections.hd
runtime/valid/available-trait-method-across-packages.hd
runtime/valid/bang-call-arguments-before-body.hd
runtime/valid/bare-marker-decorator.hd
runtime/valid/binding-expression-tuple-value.hd
runtime/valid/blanket-impl-dynamic-and-bound.hd
runtime/valid/blanket-impl-for-list.hd
runtime/valid/block-on-inside-driver.hd
runtime/valid/block-on-pending-write.hd
runtime/valid/block-on-stored-suspension.hd
runtime/valid/bom-inside-comment.hd
runtime/valid/bool-match.hd
runtime/valid/boolean-literals-and-absence.hd
runtime/valid/bound-inference-chain.hd
runtime/valid/bound-inference-default.hd
runtime/valid/bound-inference-explicit.hd
runtime/valid/bound-inference-two-bounds.hd
runtime/valid/bound-inference-user-trait.hd
runtime/valid/boundary-public-fields-cross.hd
runtime/valid/bounded-blanket-impl.hd
runtime/valid/bounded-blanket-supertraits.hd
runtime/valid/branch-scopes-shadow.hd
runtime/valid/buffer-console-error-lines.hd
runtime/valid/buffer-console.hd
runtime/valid/buffered-println-program-console.hd
runtime/valid/call-and-closure-forms.hd
runtime/valid/call-rest-inference.hd
runtime/valid/callable-values-run.hd
runtime/valid/callee-and-operand-evaluation-order.hd
runtime/valid/candidate-closure-selection-reversed.hd
runtime/valid/candidate-closure-selection.hd
runtime/valid/candidate-scope-isolation-reversed.hd
runtime/valid/candidate-scope-isolation.hd
runtime/valid/char-ascii-classes.hd
runtime/valid/char-to-digit.hd
runtime/valid/char-unicode-digits.hd
runtime/valid/char-unicode-letters.hd
runtime/valid/char-unicode-whitespace.hd
runtime/valid/character-literals.hd
runtime/valid/choices-choose.hd
runtime/valid/cli-errors.hd
runtime/valid/cli-flag-forms.hd
runtime/valid/cli-parse-args.hd
runtime/valid/cli-usage.hd
runtime/valid/clock-helpers.hd
runtime/valid/closure-as-function-argument.hd
runtime/valid/closure-body-name-before-comma.hd
runtime/valid/closure-captured-let-shared.hd
runtime/valid/closure-captures-local.hd
runtime/valid/closure-captures-per-iteration.hd
runtime/valid/closure-captures-provider-scope.hd
runtime/valid/closure-explicit-requirement-row.hd
runtime/valid/closure-inferred-requirement-row.hd
runtime/valid/closure-mutable-capture-runtime.hd
runtime/valid/closure-parameter-inference.hd
runtime/valid/closure-result-inference.hd
runtime/valid/closure-result-keeps-requirement-row.hd
runtime/valid/cmp-clamp.hd
runtime/valid/cmp-min-max-distinguishable-tie.hd
runtime/valid/cmp-min-max.hd
runtime/valid/cmp-ordering-eq.hd
runtime/valid/cmp-reverse.hd
runtime/valid/collect-target-from-try-hint.hd
runtime/valid/collect-targets-run.hd
runtime/valid/colons-in-brackets-and-trailing-blocks.hd
runtime/valid/comments-hide-code-from-the-parser.hd
runtime/valid/comparisons-beside-brackets.hd
runtime/valid/composite-identity-and-views.hd
runtime/valid/compound-assign-index-once.hd
runtime/valid/compound-assign-index.hd
runtime/valid/compound-assign-map-run.hd
runtime/valid/compound-assignment-alias.hd
runtime/valid/compound-assignment-run.hd
runtime/valid/compound-assignment-value-kind.hd
runtime/valid/comprehension-bang-calls.hd
runtime/valid/comprehension-forms.hd
runtime/valid/comprehension-propagation.hd
runtime/valid/comprehensions.hd
runtime/valid/conditional-call-program.hd
runtime/valid/console-error-line-default.hd
runtime/valid/console-error-line-override.hd
runtime/valid/console-error-traits.hd
runtime/valid/console-input-helper.hd
runtime/valid/construction-provider-capture.hd
runtime/valid/context-error-debug.hd
runtime/valid/context-values-install-providers.hd
runtime/valid/contextual-variant-after-same-line-if.hd
runtime/valid/contextual-variant-expressions.hd
runtime/valid/contextual-variant-patterns.hd
runtime/valid/contextual-words-as-names.hd
runtime/valid/continuation-line-opens-with-a-bracket.hd
runtime/valid/copy-update-copies-embedded-part.hd
runtime/valid/copy-update-evaluation-order.hd
runtime/valid/copy-update-shallow.hd
runtime/valid/copy-update-skips-defaults.hd
runtime/valid/copy-update-source-order-observed.hd
runtime/valid/covariant-readonly-weakening.hd
runtime/valid/data-and-enum-declaration-forms.hd
runtime/valid/data-expression-forms.hd
runtime/valid/data-field-defaults.hd
runtime/valid/data-field-evaluation-order.hd
runtime/valid/data-field-shorthand.hd
runtime/valid/data-fields-named-in-any-order.hd
runtime/valid/data-literal-evaluation-order.hd
runtime/valid/data-patterns.hd
runtime/valid/data-visibility-across-packages.hd
runtime/valid/dbg-prints-void.hd
runtime/valid/dbg-without-requirement.hd
runtime/valid/debug-derive-data.hd
runtime/valid/debug-derive-variants.hd
runtime/valid/debug-source-text.hd
runtime/valid/debug-tuple-rest.hd
runtime/valid/default-body-supertrait-member.hd
runtime/valid/default-hasher.hd
runtime/valid/default-method-conflict-inherent-resolves.hd
runtime/valid/default-standard-types.hd
runtime/valid/default-tuple-rest.hd
runtime/valid/default-tuple-thirteen-elements.hd
runtime/valid/defaults-reference-earlier-parameters.hd
runtime/valid/defer-after-return-value.hd
runtime/valid/defer-and-discard-statements.hd
runtime/valid/defer-closure-propagation.hd
runtime/valid/defer-lifo-and-loop-exits.hd
runtime/valid/defer-order.hd
runtime/valid/definite-init-diverging-branch.hd
runtime/valid/delegation-associated-function-written.hd
runtime/valid/depth-two-promotion.hd
runtime/valid/deque-ends.hd
runtime/valid/derived-arbitrary-no-finite-data.hd
runtime/valid/derived-arbitrary-no-finite-value.hd
runtime/valid/derived-arbitrary-recursive-members.hd
runtime/valid/derived-default-data.hd
runtime/valid/derived-default-declared.hd
runtime/valid/derived-eq-every-member.hd
runtime/valid/derived-equality-generic.hd
runtime/valid/derived-equality-members.hd
runtime/valid/derived-ordering-run.hd
runtime/valid/diamond-shallower-copy-wins.hd
runtime/valid/digest-sha256-long.hd
runtime/valid/digest-sha256-vectors.hd
runtime/valid/direct-member-hides-promoted.hd
runtime/valid/directory-module-and-child.hd
runtime/valid/discard-propagated-void-result.hd
runtime/valid/display-dispatch.hd
runtime/valid/display-tuple-rest.hd
runtime/valid/display-tuple-thirteen-elements.hd
runtime/valid/display-tuples.hd
runtime/valid/doc-test-compile-fail.hd
runtime/valid/doc-test-passes.hd
runtime/valid/doc-test-private-item.hd
runtime/valid/doc-test-text-fence.hd
runtime/valid/duration-api.hd
runtime/valid/duration-arithmetic.hd
runtime/valid/duration-display.hd
runtime/valid/duration-negation.hd
runtime/valid/duration-order.hd
runtime/valid/dyn-inherent-methods.hd
runtime/valid/dynamic-suspending-method.hd
runtime/valid/dynamic-trait-value-satisfies-own-bound.hd
runtime/valid/else-if-chain.hd
runtime/valid/embedded-construction-copies.hd
runtime/valid/embedded-copy-at-field-position.hd
runtime/valid/embedded-field-satisfies-trait.hd
runtime/valid/embedded-part-follows-container.hd
runtime/valid/embedded-part-mut-alias.hd
runtime/valid/embedded-trait-method-via-part.hd
runtime/valid/empty-string-operations.hd
runtime/valid/encoding-base64-vectors.hd
runtime/valid/encoding-decode-errors.hd
runtime/valid/encoding-hex-vectors.hd
runtime/valid/entry-suspending-main.hd
runtime/valid/enum-payload-match.hd
runtime/valid/enum-payload-mut-shallow.hd
runtime/valid/enum-shared-data-per-variant.hd
runtime/valid/enum-value-fixed-at-construction.hd
runtime/valid/eprintln-error-line.hd
runtime/valid/erased-error-result.hd
runtime/valid/error-bound-downcast-evidence.hd
runtime/valid/error-chain-derived-causes.hd
runtime/valid/error-chain-method.hd
runtime/valid/error-chain.hd
runtime/valid/error-context.hd
runtime/valid/error-derivation-run.hd
runtime/valid/error-downcast-through-inspectable.hd
runtime/valid/error-find-erased.hd
runtime/valid/error-find-trait-value.hd
runtime/valid/error-find.hd
runtime/valid/error-report.hd
runtime/valid/error-root-cause.hd
runtime/valid/escape-sequences.hd
runtime/valid/evaluation-order-elements-and-indexing.hd
runtime/valid/explicit-type-args-associated.hd
runtime/valid/explicit-type-args-method.hd
runtime/valid/f32-display-width-through-generics.hd
runtime/valid/f64-nan-ordering.hd
runtime/valid/f64-ordering-operators.hd
runtime/valid/fact-unread-never-evaluated.hd
runtime/valid/facts-of-literal-generic-none.hd
runtime/valid/facts-of-read.hd
runtime/valid/failed-trial-no-leak.hd
runtime/valid/field-and-inherent-method-share-name.hd
runtime/valid/field-and-trait-method-share-name.hd
runtime/valid/field-read-beside-trait-method.hd
runtime/valid/fieldless-data-argument.hd
runtime/valid/fieldless-data-canonical.hd
runtime/valid/final-vararg.hd
runtime/valid/float-cast-saturates.hd
runtime/valid/float-display.hd
runtime/valid/float-eq-bound.hd
runtime/valid/float-literal-forms.hd
runtime/valid/float-remainder-run.hd
runtime/valid/floating-power.hd
runtime/valid/for-loops-lists-and-maps.hd
runtime/valid/for-patterns.hd
runtime/valid/from-direct-call.hd
runtime/valid/from-propagation-and-panic.hd
runtime/valid/fs-helpers.hd
runtime/valid/function-field-returns-mutable-data.hd
runtime/valid/function-type-impl-method.hd
runtime/valid/function-type-sugar-without-import.hd
runtime/valid/function-typed-field-call.hd
runtime/valid/function-value-argument.hd
runtime/valid/function-value-vararg-call.hd
runtime/valid/generic-associated-function-qualified-call.hd
runtime/valid/generic-bound-dispatch.hd
runtime/valid/generic-call-nested-fresh-literal.hd
runtime/valid/generic-call-nested-fresh-pair.hd
runtime/valid/generic-callable-adapter.hd
runtime/valid/generic-data-embedding.hd
runtime/valid/generic-data-fields.hd
runtime/valid/generic-data-let-pattern.hd
runtime/valid/generic-data-match-pattern.hd
runtime/valid/generic-data-pattern-in-generic-function.hd
runtime/valid/generic-data-pattern-nested-generic.hd
runtime/valid/generic-enum-payloads.hd
runtime/valid/generic-forward-bound-explicit-first-slot.hd
runtime/valid/generic-function-value-argument-inference.hd
runtime/valid/generic-function-value-instantiation.hd
runtime/valid/generic-inference-explicit-conversions.hd
runtime/valid/generic-inference-from-requirement-key.hd
runtime/valid/generic-inference-literal-any-position.hd
runtime/valid/generic-inference-mut-weakening.hd
runtime/valid/generic-inference-scalars-and-data.hd
runtime/valid/generic-inherent-box.hd
runtime/valid/generic-list-element.hd
runtime/valid/generic-method-candidate-local.hd
runtime/valid/generic-methods.hd
runtime/valid/generic-mut-argument-preserved.hd
runtime/valid/generic-optional-and-result.hd
runtime/valid/generic-parameter-forms.hd
runtime/valid/generic-partial-eq-bound-primitives.hd
runtime/valid/generic-requirement-key-substitution.hd
runtime/valid/generic-storage-callable-list-identity.hd
runtime/valid/generic-storage-invokes-callables.hd
runtime/valid/generic-supertraits.hd
runtime/valid/generic-suspending-function-bound.hd
runtime/valid/generic-suspending-function.hd
runtime/valid/generic-trait-instantiation-by-argument.hd
runtime/valid/generic-trait-literal-default-instantiation.hd
runtime/valid/generic-trait-method-qualified-call.hd
runtime/valid/generic-trait-qualified-calls.hd
runtime/valid/generic-variant-constructor-argument.hd
runtime/valid/hash-bytes-derived.hd
runtime/valid/hash-bytes-result.hd
runtime/valid/hash-bytes-scalars.hd
runtime/valid/hash-bytes-sequences.hd
runtime/valid/hash-of-reference-values.hd
runtime/valid/header-and-bracket-expression-positions.hd
runtime/valid/heap-order.hd
runtime/valid/heap-reverse.hd
runtime/valid/heterogeneous-tuples.hd
runtime/valid/homogeneous-varargs.hd
runtime/valid/host-args-env.hd
runtime/valid/host-result-special-floats.hd
runtime/valid/http-scripted-provider.hd
runtime/valid/http-scripted-unknown-url.hd
runtime/valid/i32-extremes-through-generics.hd
runtime/valid/i32-minimum-literal.hd
runtime/valid/i32-minimum-through-generic-optional.hd
runtime/valid/i64-u64-precision-through-generics.hd
runtime/valid/i64-widening-checked.hd
runtime/valid/identifier-spellings.hd
runtime/valid/identity-ignores-permissions.hd
runtime/valid/impl-distinct-target-arguments.hd
runtime/valid/impl-method-generics-renamed.hd
runtime/valid/implicit-continuation-in-delimiters.hd
runtime/valid/indentation-levels.hd
runtime/valid/index-then-call-element.hd
runtime/valid/index-traits-builtin-run.hd
runtime/valid/index-traits-run.hd
runtime/valid/indexed-replacement-list-and-map.hd
runtime/valid/infinite-loop-else.hd
runtime/valid/infinite-loop-nested-break.hd
runtime/valid/infinite-loop-return.hd
runtime/valid/inherent-members-disjoint-targets.hd
runtime/valid/inherent-method-beats-trait-method.hd
runtime/valid/inherent-methods.hd
runtime/valid/init-read-through-trait-dispatch.hd
runtime/valid/init-ready-groups-by-identity.hd
runtime/valid/inspectable-alias-and-newtype-identity.hd
runtime/valid/inspectable-downcast-mut.hd
runtime/valid/inspectable-downcast-success-and-failure.hd
runtime/valid/inspectable-dynamic-vs-static-identity.hd
runtime/valid/inspectable-erasure-example.hd
runtime/valid/inspectable-function-fields-and-trait-arguments.hd
runtime/valid/inspectable-generic-arguments-exact.hd
runtime/valid/inspectable-generic-downcast-targets.hd
runtime/valid/inspectable-generic-inner-mut-argument.hd
runtime/valid/inspectable-generic-target-helper.hd
runtime/valid/inspectable-inner-mut-identity.hd
runtime/valid/inspectable-primitives-collections-options.hd
runtime/valid/installer-function-runs.hd
runtime/valid/integer-literal-forms.hd
runtime/valid/integer-power-associativity.hd
runtime/valid/interpolation-display-order.hd
runtime/valid/interpolation-expression-spacing.hd
runtime/valid/interpolation-forms.hd
runtime/valid/it-body-by-name.hd
runtime/valid/it-each-options.hd
runtime/valid/it-each-propagation.hd
runtime/valid/it-each-rows.hd
runtime/valid/iterator-adapters-run.hd
runtime/valid/iterator-chain-iterable.hd
runtime/valid/iterator-drives-loops.hd
runtime/valid/iterator-flat-map.hd
runtime/valid/iterator-from-fn.hd
runtime/valid/iterator-search-count.hd
runtime/valid/iterator-shape-versus-value-changes.hd
runtime/valid/iterator-single-pass.hd
runtime/valid/iterator-skip-take-while.hd
runtime/valid/iterator-zip-iterable.hd
runtime/valid/json-errors.hd
runtime/valid/json-escapes.hd
runtime/valid/json-float-text.hd
runtime/valid/json-number-grammar.hd
runtime/valid/json-numbers.hd
runtime/valid/json-object-order.hd
runtime/valid/json-pretty.hd
runtime/valid/json-round-trip.hd
runtime/valid/json-serde-private-round-trip.hd
runtime/valid/json-suite.hd
runtime/valid/json-typed-enum.hd
runtime/valid/json-typed-errors.hd
runtime/valid/json-typed-missing-key-ignores-default.hd
runtime/valid/json-typed-optional.hd
runtime/valid/json-typed-primitives.hd
runtime/valid/json-typed-round-trip.hd
runtime/valid/lazy-result-candidate.hd
runtime/valid/leading-dot-chain.hd
runtime/valid/leading-dot-deeper-continues.hd
runtime/valid/leading-dot-lines-join-the-chain.hd
runtime/valid/leading-dot-statement-indent-tail.hd
runtime/valid/leading-pipe-lines-join-the-chain.hd
runtime/valid/let-after-same-line-if-keeps-else.hd
runtime/valid/list-access-building.hd
runtime/valid/list-and-optional-map.hd
runtime/valid/list-append-grows.hd
runtime/valid/list-chunks.hd
runtime/valid/list-counts.hd
runtime/valid/list-flat-map-windows.hd
runtime/valid/list-group-by.hd
runtime/valid/list-insert-remove-clear.hd
runtime/valid/list-iterator.hd
runtime/valid/list-map-key.hd
runtime/valid/list-min-max.hd
runtime/valid/list-of-trait-values.hd
runtime/valid/list-partition-search.hd
runtime/valid/list-pop.hd
runtime/valid/list-slice-mutable.hd
runtime/valid/list-sorted-by-key.hd
runtime/valid/list-suffix-spread.hd
runtime/valid/list-view-run.hd
runtime/valid/list-view-to-list-mut.hd
runtime/valid/literal-erased-fallback.hd
runtime/valid/literal-fallback-hint-fix.hd
runtime/valid/literal-patterns.hd
runtime/valid/literal-receiver-params-differ-annotated.hd
runtime/valid/literal-suffix-call.hd
runtime/valid/literal-suffix-calls.hd
runtime/valid/literal-suffix-default-parameter.hd
runtime/valid/literal-suffix-generic-num-run.hd
runtime/valid/literal-trait-value-float-fallback.hd
runtime/valid/literal-var-flow.hd
runtime/valid/literal-var-instantiation-wait.hd
runtime/valid/local-declarations-in-block-suites.hd
runtime/valid/local-impl-known-after-declaration.hd
runtime/valid/local-impl-visible-after-declaration.hd
runtime/valid/local-inherent-and-trait-impls.hd
runtime/valid/local-shadows-module-namespace.hd
runtime/valid/logical-operators.hd
runtime/valid/loop-iteration-binding-capture.hd
runtime/valid/loop-iteration-cells.hd
runtime/valid/manual-clock.hd
runtime/valid/map-args-env.hd
runtime/valid/map-get-or.hd
runtime/valid/map-index-reads-value.hd
runtime/valid/map-is-empty.hd
runtime/valid/map-iteration-order.hd
runtime/valid/map-iterator.hd
runtime/valid/map-key-types.hd
runtime/valid/map-keys-values.hd
runtime/valid/map-literal-and-comprehension-order.hd
runtime/valid/map-lookup-and-duplicate-keys.hd
runtime/valid/map-remove-absent-key.hd
runtime/valid/map-sys.hd
runtime/valid/match-arm-binding-reuse.hd
runtime/valid/match-guards.hd
runtime/valid/memory-fs.hd
runtime/valid/method-call-never-selects-field.hd
runtime/valid/method-receiver-before-arguments.hd
runtime/valid/method-references-run.hd
runtime/valid/module-alias-function-value.hd
runtime/valid/module-binding-shared-with-functions.hd
runtime/valid/module-initialization-before-main.hd
runtime/valid/module-qualified-beside-direct-use.hd
runtime/valid/module-qualified-function-call.hd
runtime/valid/module-qualified-function-value-parenthesized.hd
runtime/valid/module-qualified-function-value.hd
runtime/valid/module-qualified-generic-function-value.hd
runtime/valid/module-qualified-prelude-function.hd
runtime/valid/module-qualified-type.hd
runtime/valid/module-qualified-variant-pattern.hd
runtime/valid/module-scope-before-initialization.hd
runtime/valid/multibyte-scalar-strings.hd
runtime/valid/multiline-string-literals.hd
runtime/valid/multiple-bounds-dispatch.hd
runtime/valid/multiple-dedents-at-once.hd
runtime/valid/multiple-inline-closures.hd
runtime/valid/multiply-by-zero-64.hd
runtime/valid/mut-bound-value-passed-on.hd
runtime/valid/mut-self-primitive-arithmetic.hd
runtime/valid/mut-trait-value-from-mut.hd
runtime/valid/mut-trait-value-satisfies-mut-bound.hd
runtime/valid/mutable-data-paths-share-identity.hd
runtime/valid/mutable-provider-state.hd
runtime/valid/mutable-receivers.hd
runtime/valid/mutable-trait-bound.hd
runtime/valid/named-arguments-evaluate-in-source-order.hd
runtime/valid/named-arguments-reordered.hd
runtime/valid/named-arguments-source-evaluation-order.hd
runtime/valid/named-arguments-trait-dispatch.hd
runtime/valid/named-enum-payload-evaluation-order.hd
runtime/valid/named-enum-payload-patterns.hd
runtime/valid/named-local-functions.hd
runtime/valid/named-local-suspending-function.hd
runtime/valid/nan-equality-through-generics.hd
runtime/valid/nan-ordering-composites.hd
runtime/valid/narrowing-cast-wraps.hd
runtime/valid/nested-block-on.hd
runtime/valid/nested-closure-captures.hd
runtime/valid/nested-control-flow-as-expressions.hd
runtime/valid/nested-mutable-captures.hd
runtime/valid/nested-provider-restoration.hd
runtime/valid/nested-provider-scope.hd
runtime/valid/nested-suspending-call.hd
runtime/valid/nested-variant-positional-bindings.hd
runtime/valid/net-own-provider.hd
runtime/valid/never-break-continue.hd
runtime/valid/no-entry-point-no-tests.hd
runtime/valid/no-final-line-ending.hd
runtime/valid/none-with-expected-list-type.hd
runtime/valid/not-granted-display.hd
runtime/valid/num-abs-diff.hd
runtime/valid/num-bit-counts.hd
runtime/valid/num-checked-wrapping.hd
runtime/valid/num-every-width.hd
runtime/valid/num-is-finite.hd
runtime/valid/num-is-nan.hd
runtime/valid/num-ordered-display.hd
runtime/valid/num-parse-f64-errors.hd
runtime/valid/num-parse-f64-round-trip.hd
runtime/valid/num-parse-f64-specials.hd
runtime/valid/num-parse-f64-values.hd
runtime/valid/num-parse-integers.hd
runtime/valid/num-parse-unsigned.hd
runtime/valid/num-rotate.hd
runtime/valid/num-saturating.hd
runtime/valid/num-to-fixed.hd
runtime/valid/num-traits-run.hd
runtime/valid/numeric-candidate-tie-break.hd
runtime/valid/numeric-casts-in-range.hd
runtime/valid/numeric-explicit-widening.hd
runtime/valid/operands-across-suspension-order.hd
runtime/valid/operator-generic-primitive-run.hd
runtime/valid/operator-string-add.hd
runtime/valid/operator-syntax-without-import.hd
runtime/valid/operator-traits-run.hd
runtime/valid/operators-longest-match.hd
runtime/valid/operators-without-spaces.hd
runtime/valid/option-and-then.hd
runtime/valid/option-enum-spellings.hd
runtime/valid/option-impl-target.hd
runtime/valid/option-match-patterns.hd
runtime/valid/option-result-prelude-spellings.hd
runtime/valid/option-tests-conversions.hd
runtime/valid/optional-alias-mutation.hd
runtime/valid/optional-closure-arguments.hd
runtime/valid/optional-erased-to-any.hd
runtime/valid/optional-mutable-match.hd
runtime/valid/optional-propagation.hd
runtime/valid/ord-supertrait-dispatch.hd
runtime/valid/outer-field-beside-embedded-method.hd
runtime/valid/own-module-private-members.hd
runtime/valid/parameter-default-earlier-parameter.hd
runtime/valid/parameter-defaults-after-explicit-arguments.hd
runtime/valid/parenthesized-nested-same-line-if.hd
runtime/valid/partial-equality-dispatch.hd
runtime/valid/partial-ordering-dispatch.hd
runtime/valid/path-operations.hd
runtime/valid/pipe-bare-method-step.hd
runtime/valid/pipe-evaluation-order.hd
runtime/valid/pipe-method-reference-step.hd
runtime/valid/pipe-nested-placeholder.hd
runtime/valid/pipe-placeholder-steps.hd
runtime/valid/pipe-suspending-substitution-step.hd
runtime/valid/plain-dollar-text.hd
runtime/valid/precedence-and-associativity.hd
runtime/valid/prefixed-string-template.hd
runtime/valid/prelude-cmp-method-direct.hd
runtime/valid/prelude-eq-method-direct.hd
runtime/valid/prelude-hash-method-direct.hd
runtime/valid/prelude-partial-cmp-method-direct.hd
runtime/valid/primitive-bool-eq-method-direct.hd
runtime/valid/primitive-char-cmp-method-direct.hd
runtime/valid/primitive-display-bound-and-trait-values.hd
runtime/valid/primitive-float-cmp-method-direct.hd
runtime/valid/primitive-integer-cmp-method-direct.hd
runtime/valid/primitive-operator-calls-method.hd
runtime/valid/primitive-string-cmp-method-direct.hd
runtime/valid/println-console-stdout.hd
runtime/valid/println-in-test-body.hd
runtime/valid/println-pending-write.hd
runtime/valid/println-provider-suspending-body.hd
runtime/valid/println-recording-provider.hd
runtime/valid/println-under-main-driver.hd
runtime/valid/private-function-inferred-result.hd
runtime/valid/private-function-inferred-row.hd
runtime/valid/private-main-is-ordinary-function.hd
runtime/valid/process-error-display.hd
runtime/valid/process-error-results.hd
runtime/valid/process-exit-data-eq.hd
runtime/valid/process-not-granted.hd
runtime/valid/process-scripted-provider.hd
runtime/valid/promoted-method-no-override.hd
runtime/valid/propagation-before-cleanup.hd
runtime/valid/propagation-from-two-domains.hd
runtime/valid/propagation-in-closure-targets-closure.hd
runtime/valid/propagation-into-erased-error.hd
runtime/valid/propagation-prefers-assignability.hd
runtime/valid/property-assume-discards.hd
runtime/valid/property-body-discard-message-fails.hd
runtime/valid/property-draw-budget.hd
runtime/valid/property-generators-collections.hd
runtime/valid/property-generators-scalars.hd
runtime/valid/provider-capture-timing.hd
runtime/valid/provider-from-suspending-call.hd
runtime/valid/provider-scope-dynamic-callback.hd
runtime/valid/provider-scope-lexical-capture.hd
runtime/valid/pub-own-member-hides-promoted-other-module.hd
runtime/valid/pub-own-member-hides-promoted.hd
runtime/valid/pub-use-same-declaration.hd
runtime/valid/qualified-calls-beside-promoted-method.hd
runtime/valid/question-mark-finds-std-from.hd
runtime/valid/range-eq.hd
runtime/valid/range-inclusive-field.hd
runtime/valid/range-iteration.hd
runtime/valid/range-pattern-exclusive-to.hd
runtime/valid/range-pattern-run.hd
runtime/valid/raw-identifiers.hd
runtime/valid/readonly-root-generic-mutable-path.hd
runtime/valid/recursive-data-types.hd
runtime/valid/recursive-private-functions-least-row.hd
runtime/valid/reference-bounded-dynamic-method.hd
runtime/valid/reference-cycles-are-ordinary-data.hd
runtime/valid/reference-identity.hd
runtime/valid/regex-anchors-groups.hd
runtime/valid/regex-captures-linear-time.hd
runtime/valid/regex-captures.hd
runtime/valid/regex-classes.hd
runtime/valid/regex-errors.hd
runtime/valid/regex-find-all.hd
runtime/valid/regex-leftmost-first.hd
runtime/valid/regex-linear-time.hd
runtime/valid/regex-literals-escapes.hd
runtime/valid/regex-named-groups.hd
runtime/valid/regex-repetition.hd
runtime/valid/regex-replace.hd
runtime/valid/regex-split.hd
runtime/valid/relative-self-current.hd
runtime/valid/relative-self-top-level.hd
runtime/valid/replace-empty-old.hd
runtime/valid/replace-non-overlapping.hd
runtime/valid/requirement-function-value.hd
runtime/valid/requirement-key-binding-run.hd
runtime/valid/requirement-row-duplicate-after-substitution.hd
runtime/valid/requirement-row-forwarded-through-calls.hd
runtime/valid/requirement-row-order-data-field.hd
runtime/valid/requirement-row-order-stored-suspension.hd
runtime/valid/requirement-row-order-trait-value.hd
runtime/valid/resource-error-operation-payload.hd
runtime/valid/result-and-then.hd
runtime/valid/result-entry-point-ok.hd
runtime/valid/result-enum-spellings.hd
runtime/valid/result-map.hd
runtime/valid/result-ok-unit.hd
runtime/valid/result-pattern-nested-enum.hd
runtime/valid/result-propagation-evaluates-once.hd
runtime/valid/result-propagation.hd
runtime/valid/result-tests-conversions.hd
runtime/valid/result-type-candidate.hd
runtime/valid/retry-with-backoff.hd
runtime/valid/rng-from-random.hd
runtime/valid/rng-int-range.hd
runtime/valid/rng-seeded-sequence.hd
runtime/valid/rng-shuffle-choose-sample.hd
runtime/valid/row-alias-bare-runs.hd
runtime/valid/row-alias-mutable-key.hd
runtime/valid/row-alias-runs.hd
runtime/valid/row-extension-absent-key-runs.hd
runtime/valid/row-extension-provider-restoration.hd
runtime/valid/row-extension-restores-provider.hd
runtime/valid/row-forced-overlap-nearest.hd
runtime/valid/row-inference-empty-row.hd
runtime/valid/row-parameter-callable-in-list.hd
runtime/valid/row-polymorphic-forwarding.hd
runtime/valid/row-polymorphic-union-forwarding.hd
runtime/valid/row-subsumption-runs.hd
runtime/valid/row-union-branches-run.hd
runtime/valid/row-union-closure-result-runs.hd
runtime/valid/row-union-list-runs.hd
runtime/valid/row-variable-binds-union.hd
runtime/valid/row-variable-plus-key.hd
runtime/valid/same-line-suite-body-forms.hd
runtime/valid/same-line-suite-boundaries.hd
runtime/valid/script-top-level-runs.hd
runtime/valid/scripted-input.hd
runtime/valid/scripted-process.hd
runtime/valid/sealed-member-name-inherent-method.hd
runtime/valid/sealed-supertrait-extension.hd
runtime/valid/seeded-random.hd
runtime/valid/self-interpolation.hd
runtime/valid/sequential-suspending-calls.hd
runtime/valid/serde-derive-call-order.hd
runtime/valid/serde-std-reads.hd
runtime/valid/serde-writer-first-error.hd
runtime/valid/set-basics.hd
runtime/valid/shared-enum-data-defaults.hd
runtime/valid/shared-enum-fact-evaluation.hd
runtime/valid/shared-mutable-child-no-invariants.hd
runtime/valid/shift-count-unsigned.hd
runtime/valid/short-circuit-and-conditional-evaluation.hd
runtime/valid/shorter-promotion-path-wins.hd
runtime/valid/sibling-module-enum-and-trait.hd
runtime/valid/sibling-module-names-imported.hd
runtime/valid/sibling-module-pub-members.hd
runtime/valid/sibling-module-std-name-imported.hd
runtime/valid/sibling-module-trait-imported.hd
runtime/valid/signed-zero-and-infinity-through-generics.hd
runtime/valid/single-payload-variant-function-value.hd
runtime/valid/sized-integer-arithmetic.hd
runtime/valid/slicing-full-run.hd
runtime/valid/slicing-run.hd
runtime/valid/spelled-function-type-values.hd
runtime/valid/split-empty-input-nonempty-separator.hd
runtime/valid/spread-forms-and-positions.hd
runtime/valid/spread-pattern.hd
runtime/valid/static-and-dynamic-trait-dispatch.hd
runtime/valid/std-errors-erased-codecs.hd
runtime/valid/std-errors-erased.hd
runtime/valid/stored-suspension-parameter.hd
runtime/valid/stored-suspension-single-drive.hd
runtime/valid/string-and-char-literal-contents.hd
runtime/valid/string-byte-methods.hd
runtime/valid/string-concatenation-and-numeric-selectors.hd
runtime/valid/string-count.hd
runtime/valid/string-interpolation-built-ins.hd
runtime/valid/string-length-counts-bytes.hd
runtime/valid/string-lines.hd
runtime/valid/string-more-methods.hd
runtime/valid/string-ordering.hd
runtime/valid/string-pad-default-fill.hd
runtime/valid/string-pad.hd
runtime/valid/string-prefix-imported-by-name.hd
runtime/valid/string-prefix-plain-dollar-digit.hd
runtime/valid/string-prefix-std.hd
runtime/valid/string-prefix-template.hd
runtime/valid/string-repeat.hd
runtime/valid/string-split-once.hd
runtime/valid/string-split-whitespace.hd
runtime/valid/string-split.hd
runtime/valid/string-trim-and-lower.hd
runtime/valid/strings-and-comments-hide-keywords-and-operators.hd
runtime/valid/structural-equality.hd
runtime/valid/structural-ordering.hd
runtime/valid/structure-name.hd
runtime/valid/structure-self-ref-enum.hd
runtime/valid/structure-self-ref-omitted.hd
runtime/valid/structure-self-ref-type-arguments.hd
runtime/valid/structure-self-ref.hd
runtime/valid/suite-statement-right-sides.hd
runtime/valid/supertrait-methods.hd
runtime/valid/suspending-argument-candidate.hd
runtime/valid/suspending-blanket-impls.hd
runtime/valid/suspending-call-in-scoped-defer.hd
runtime/valid/suspending-call-preserves-locals.hd
runtime/valid/suspending-call-with-defer.hd
runtime/valid/suspending-call-with-requirement.hd
runtime/valid/suspending-calls-as-arguments.hd
runtime/valid/suspending-calls-in-binary-expression.hd
runtime/valid/suspending-calls-in-loops.hd
runtime/valid/suspending-closure-captures.hd
runtime/valid/suspending-closure-requirement-row.hd
runtime/valid/suspending-result-propagation.hd
runtime/valid/suspending-test-body.hd
runtime/valid/suspending-trailing-block.hd
runtime/valid/suspending-trait-default-method.hd
runtime/valid/suspending-trait-dispatch.hd
runtime/valid/tab-only-as-content.hd
runtime/valid/task-all-list-empty.hd
runtime/valid/task-all-list-order.hd
runtime/valid/task-retry-at-least-once.hd
runtime/valid/task-retry.hd
runtime/valid/template-derived-trait-self.hd
runtime/valid/termination-report.hd
runtime/valid/termination-void-reports-zero.hd
runtime/valid/test-block-on.hd
runtime/valid/test-block-propagation.hd
runtime/valid/test-body-explicit-closure.hd
runtime/valid/test-body-return.hd
runtime/valid/test-case-fresh-instance.hd
runtime/valid/test-case-options.hd
runtime/valid/test-expect-panic.hd
runtime/valid/test-module-top-level-cases.hd
runtime/valid/test-registration-qualified-call.hd
runtime/valid/test-registration-qualified-prop.hd
runtime/valid/test-timeout-options.hd
runtime/valid/tests-block-items.hd
runtime/valid/tests-block-use-shadow.hd
runtime/valid/text-join-builder.hd
runtime/valid/text-prefix-helpers.hd
runtime/valid/time-date-utc.hd
runtime/valid/time-parse-errors.hd
runtime/valid/time-rfc3339-parse.hd
runtime/valid/time-rfc3339-text.hd
runtime/valid/time-serde-forms.hd
runtime/valid/time-unix-milliseconds.hd
runtime/valid/trailing-block-return-targets-callback.hd
runtime/valid/trailing-block-right-hand-sides.hd
runtime/valid/trailing-block-right-sides.hd
runtime/valid/trailing-callback-blocks.hd
runtime/valid/trailing-commas-everywhere.hd
runtime/valid/trait-and-impl-declaration-forms.hd
runtime/valid/trait-associated-call-infers-self.hd
runtime/valid/trait-associated-function-reference.hd
runtime/valid/trait-associated-functions.hd
runtime/valid/trait-availability-prelude-and-scope.hd
runtime/valid/trait-default-method-inherited.hd
runtime/valid/trait-default-method-overridden.hd
runtime/valid/trait-delegation-as-written.hd
runtime/valid/trait-delegation-forwards.hd
runtime/valid/trait-delegation-vararg.hd
runtime/valid/trait-qualified-associated-and-named-calls.hd
runtime/valid/trait-qualified-calls.hd
runtime/valid/trait-value-as-provider.hd
runtime/valid/trait-value-binding-identity.hd
runtime/valid/trait-value-generic-method-value-args.hd
runtime/valid/trait-value-satisfies-instantiated-bound.hd
runtime/valid/trim-unicode-white-space.hd
runtime/valid/try-operand-expected-type.hd
runtime/valid/tuple-bound-vararg-call.hd
runtime/valid/tuple-derived-order.hd
runtime/valid/tuple-element-permission.hd
runtime/valid/tuple-ordering-nan-unordered.hd
runtime/valid/tuple-rebuild.hd
runtime/valid/tuple-rest-derived.hd
runtime/valid/tuple-rest-literal.hd
runtime/valid/tuple-rest-map-key.hd
runtime/valid/tuple-rest-spread-list.hd
runtime/valid/tuple-spread-candidate.hd
runtime/valid/tuple-spread-fixed.hd
runtime/valid/tuple-spread-rest.hd
runtime/valid/tuple-thirteen-elements.hd
runtime/valid/tuple-vararg-function-value.hd
runtime/valid/tuple-vararg-infer.hd
runtime/valid/tuple-vararg-rest.hd
runtime/valid/tuple-vararg-spread-tail.hd
runtime/valid/tuple-vararg.hd
runtime/valid/type-arguments-in-expressions.hd
runtime/valid/type-expression-forms.hd
runtime/valid/typed-derivation-build-defaults.hd
runtime/valid/typed-derivation-embedded-generic-walk.hd
runtime/valid/typeid-mut-names.hd
runtime/valid/typeid-nested-mut-trait-argument.hd
runtime/valid/typeid-of-equality.hd
runtime/valid/typeid-qualified-trait-name.hd
runtime/valid/typeid-same-name-modules.hd
runtime/valid/u8-checked-add.hd
runtime/valid/unavailable-trait-method-invisible.hd
runtime/valid/underscore-tuple-members.hd
runtime/valid/unicode-function-names.hd
runtime/valid/unit-pattern-void-success.hd
runtime/valid/unit-test-manual-clock.hd
runtime/valid/unsafe-trait-static-bound.hd
runtime/valid/unsigned-exponent.hd
runtime/valid/use-declaration-position-independent.hd
runtime/valid/use-module-member-beside-root-declaration.hd
runtime/valid/user-iterable-for-loop.hd
runtime/valid/usize-width-wasm32.hd
runtime/valid/utf8-error-traits.hd
runtime/valid/utf8-invalid-bytes.hd
runtime/valid/utf8-overlong.hd
runtime/valid/utf8-truncated.hd
runtime/valid/utf8-valid-text.hd
runtime/valid/vararg-function-values.hd
runtime/valid/varargs-in-trait-and-suspending-methods.hd
runtime/valid/while-break-and-continue.hd
runtime/valid/while-else-break-value.hd
runtime/valid/while-else-exhaustion-value.hd
runtime/valid/write-line-around-suspending-provider-scope.hd
runtime/valid/write-line-suspending-call-argument.hd
typing/invalid/absolute-path-in-expression.hd
typing/invalid/adapter-callback-scope-key.hd
typing/invalid/alias-bound-bare-trait.hd
typing/invalid/alias-cycle.hd
typing/invalid/alias-inherent-impl.hd
typing/invalid/alias-unknown-target.hd
typing/invalid/all-bang-child.hd
typing/invalid/all-non-suspend-argument.hd
typing/invalid/all-spread-argument.hd
typing/invalid/ambiguous-associated-binding.hd
typing/invalid/ambiguous-default-and-written-trait-method.hd
typing/invalid/ambiguous-default-methods.hd
typing/invalid/ambiguous-method-two-traits.hd
typing/invalid/ambiguous-projection-binding.hd
typing/invalid/ambiguous-projection.hd
typing/invalid/ambiguous-row-pattern.hd
typing/invalid/ambiguous-trait-method.hd
typing/invalid/annotate-before-function.hd
typing/invalid/any-exposes-no-methods.hd
typing/invalid/anyref-rejects-enum.hd
typing/invalid/anyref-rejects-primitive.hd
typing/invalid/anyref-rejects-tuple.hd
typing/invalid/anyref-subtrait-unbounded-generic-newtype.hd
typing/invalid/anyref-subtrait-value-newtype.hd
typing/invalid/anyval-bound-rejects-data.hd
typing/invalid/anyval-user-impl.hd
typing/invalid/arbitrary-with-non-inspectable-member.hd
typing/invalid/arithmetic-on-bool.hd
typing/invalid/arithmetic-on-list.hd
typing/invalid/assert-equal-fieldless-data-without-partial-eq.hd
typing/invalid/assert-equal-non-eq.hd
typing/invalid/assert-equal-numeric-widening.hd
typing/invalid/assert-equal-without-debug.hd
typing/invalid/assoc-call-type-no-candidate.hd
typing/invalid/associated-binding-mismatch.hd
typing/invalid/associated-binding-value-bare-trait.hd
typing/invalid/associated-function-ambiguous-traits.hd
typing/invalid/available-trait-method-beside-promoted.hd
typing/invalid/bang-call-in-comprehension.hd
typing/invalid/bang-call-in-defer.hd
typing/invalid/bang-call-in-plain-function.hd
typing/invalid/bang-call-non-suspending.hd
typing/invalid/bang-call-outside-suspension.hd
typing/invalid/bare-err-is-unknown-name.hd
typing/invalid/bare-none-is-unknown-name.hd
typing/invalid/bare-none-pattern.hd
typing/invalid/bare-ok-is-unknown-name.hd
typing/invalid/bare-some-is-unknown-name.hd
typing/invalid/bare-variant-pattern.hd
typing/invalid/binding-expression-redeclaration.hd
typing/invalid/binding-on-non-trait.hd
typing/invalid/bitwise-and-bool.hd
typing/invalid/bitwise-or-float.hd
typing/invalid/bitwise-xor-string.hd
typing/invalid/block-on-in-defer.hd
typing/invalid/block-on-in-fact.hd
typing/invalid/block-on-in-metadata.hd
typing/invalid/bodyless-impl-via-promotion.hd
typing/invalid/bool-match-missing-false.hd
typing/invalid/bool-ordering.hd
typing/invalid/bound-argument-bare-trait.hd
typing/invalid/bound-binding-bare-trait.hd
typing/invalid/bound-inference-blanket-impl-unmet.hd
typing/invalid/bound-inference-bounds-disagree.hd
typing/invalid/bound-inference-incompatible-argument.hd
typing/invalid/bound-inference-mixed-several-impls.hd
typing/invalid/bound-inference-no-impl.hd
typing/invalid/bound-inference-several-impls.hd
typing/invalid/bound-reference-associated-function.hd
typing/invalid/bound-reference-readonly-receiver.hd
typing/invalid/boundary-nested-private-field.hd
typing/invalid/boundary-private-field-no-traits.hd
typing/invalid/boundary-result-needs-deserialize.hd
typing/invalid/branch-binding-does-not-leak.hd
typing/invalid/break-outside-loop.hd
typing/invalid/break-value-in-void-loop.hd
typing/invalid/break-value-without-else.hd
typing/invalid/bytes-prefix-unknown.hd
typing/invalid/call-missing-requirement.hd
typing/invalid/callable-value-arguments.hd
typing/invalid/callable-value-no-update.hd
typing/invalid/callable-value-readonly-parameter.hd
typing/invalid/cannot-infer-type.hd
typing/invalid/cast-literal-out-of-range.hd
typing/invalid/child-module-not-in-parent.hd
typing/invalid/closure-argument-type-mismatch.hd
typing/invalid/closure-escapes-provider-scope.hd
typing/invalid/closure-inferred-row-missing-requirement.hd
typing/invalid/closure-not-inspectable.hd
typing/invalid/closure-row-key-collision.hd
typing/invalid/collect-target-not-fromiterator.hd
typing/invalid/compound-assign-data-no-operator.hd
typing/invalid/compound-assign-index-no-read.hd
typing/invalid/compound-assign-not-place.hd
typing/invalid/compound-assign-readonly.hd
typing/invalid/compound-assign-value-no-operator.hd
typing/invalid/compound-assign-value-parameter.hd
typing/invalid/compound-index-negative-literal.hd
typing/invalid/comprehension-binding-does-not-leak.hd
typing/invalid/comprehension-refutable-pattern.hd
typing/invalid/concrete-value-binding-mismatch.hd
typing/invalid/console-readonly-binding-write-line.hd
typing/invalid/context-result-type-mismatch.hd
typing/invalid/context-row-unknown-trait.hd
typing/invalid/contextual-variant-as-function-value.hd
typing/invalid/contextual-variant-binding-without-type.hd
typing/invalid/contextual-variant-without-type.hd
typing/invalid/continue-outside-loop.hd
typing/invalid/contravariant-enum-requirement-row.hd
typing/invalid/contravariant-method-requirement-row.hd
typing/invalid/contravariant-method-result.hd
typing/invalid/copy-update-private-field-other-module.hd
typing/invalid/copy-update-readonly-child.hd
typing/invalid/copy-update-readonly-embedded-mutable-edge.hd
typing/invalid/copy-update-readonly-source-mut-field.hd
typing/invalid/copy-update-source-type-mismatch.hd
typing/invalid/covariant-data-requirement-row.hd
typing/invalid/covariant-embedded-field.hd
typing/invalid/covariant-enum-method-parameter.hd
typing/invalid/covariant-method-requirement-row.hd
typing/invalid/covariant-mut-method-parameter.hd
typing/invalid/covariant-mut-self-method-parameter.hd
typing/invalid/covariant-optional-field.hd
typing/invalid/covariant-optional-method-parameter.hd
typing/invalid/covariant-private-method-parameter.hd
typing/invalid/covariant-producer-callback-parameter.hd
typing/invalid/covariant-result-field.hd
typing/invalid/covariant-suspending-method-parameter.hd
typing/invalid/covariant-trait-argument-parameter.hd
typing/invalid/data-bound-bare-trait.hd
typing/invalid/data-default-reads-other-field.hd
typing/invalid/data-field-bare-trait.hd
typing/invalid/data-field-default-type-mismatch.hd
typing/invalid/data-literal-field-type-mismatch.hd
typing/invalid/data-literal-missing-field.hd
typing/invalid/data-literal-private-field-other-module.hd
typing/invalid/data-nominal-same-fields.hd
typing/invalid/data-pattern-nonexhaustive.hd
typing/invalid/data-pattern-repeated-field.hd
typing/invalid/data-pattern-unknown-field.hd
typing/invalid/dbg-void-binding.hd
typing/invalid/debug-missing-derive.hd
typing/invalid/declaration-requirement-not-on-result.hd
typing/invalid/declared-generic-key-lexical-collision.hd
typing/invalid/decorator-target-kind.hd
typing/invalid/decorator-target-member-line.hd
typing/invalid/decorator-target-newtype.hd
typing/invalid/default-body-inherent-method.hd
typing/invalid/default-names-later-beside-forward-bound.hd
typing/invalid/defer-propagate-query-suspending.hd
typing/invalid/defer-propagate-query.hd
typing/invalid/defer-return.hd
typing/invalid/defer-suspends.hd
typing/invalid/delegation-associated-function-missing.hd
typing/invalid/delegation-binds-associated-type.hd
typing/invalid/delegation-part-lacks-trait.hd
typing/invalid/delegation-to-ordinary-field.hd
typing/invalid/derive-and-block-overlap.hd
typing/invalid/derive-before-function.hd
typing/invalid/derive-before-trait.hd
typing/invalid/derive-beside-written-impl.hd
typing/invalid/derive-member-not-derivable.hd
typing/invalid/derive-newtype-base-missing-trait.hd
typing/invalid/derive-unknown-trait.hd
typing/invalid/derived-arbitrary-function-member.hd
typing/invalid/derived-arbitrary-generic-bound.hd
typing/invalid/derived-arbitrary-tuned-member-not-arbitrary.hd
typing/invalid/derived-default-member-not-default.hd
typing/invalid/derived-default-no-variant.hd
typing/invalid/derived-default-several-variants.hd
typing/invalid/derived-eq-enum-payload-missing-trait.hd
typing/invalid/derived-hash-field-not-hash.hd
typing/invalid/derived-hash-without-eq.hd
typing/invalid/derived-total-order-float.hd
typing/invalid/diamond-same-depth-conflict.hd
typing/invalid/discarded-optional-result.hd
typing/invalid/discarded-result.hd
typing/invalid/discarded-suspension.hd
typing/invalid/distinct-traits-not-overloads.hd
typing/invalid/downcast-mut-readonly-value.hd
typing/invalid/downcast-result-readonly.hd
typing/invalid/downcast-target-not-inspectable.hd
typing/invalid/downcast-value-type-target.hd
typing/invalid/drive-readonly-suspension.hd
typing/invalid/duplicate-associated-binding-two-bounds.hd
typing/invalid/duplicate-associated-binding.hd
typing/invalid/duplicate-binding-in-one-pattern.hd
typing/invalid/duplicate-bool-match-arm.hd
typing/invalid/duplicate-impl-member.hd
typing/invalid/duplicate-test-name.hd
typing/invalid/duplicate-trait-member.hd
typing/invalid/dyn-associated-function-bound.hd
typing/invalid/dyn-inherent-method-on-concrete.hd
typing/invalid/dyn-unknown-associated-binding.hd
typing/invalid/embedded-field-not-called.hd
typing/invalid/embedded-part-readonly-alias.hd
typing/invalid/empty-list-without-context.hd
typing/invalid/empty-literal-generic-field-unsolved.hd
typing/invalid/empty-map-without-context.hd
typing/invalid/enum-bound-bare-trait.hd
typing/invalid/enum-match-missing-variant.hd
typing/invalid/enum-payload-bare-trait.hd
typing/invalid/enum-shared-constructor-payload.hd
typing/invalid/enum-shared-field-assignment.hd
typing/invalid/eprintln-without-console.hd
typing/invalid/erase-readonly-to-mut-inspectable.hd
typing/invalid/error-argument-identifier.hd
typing/invalid/error-argument-number.hd
typing/invalid/error-bare-before-data.hd
typing/invalid/error-before-function.hd
typing/invalid/error-cause-not-error.hd
typing/invalid/error-extra-argument.hd
typing/invalid/error-find-concrete-receiver.hd
typing/invalid/error-find-non-error.hd
typing/invalid/error-form-other-rule.hd
typing/invalid/error-from-beside-other-member.hd
typing/invalid/error-from-marker-argument.hd
typing/invalid/error-from-same-type.hd
typing/invalid/error-from-type-parameter.hd
typing/invalid/error-hand-written-display.hd
typing/invalid/error-hand-written-from.hd
typing/invalid/error-implementation-local-type.hd
typing/invalid/error-message-before-enum.hd
typing/invalid/error-message-not-display.hd
typing/invalid/error-message-self.hd
typing/invalid/error-message-shared-unnamed.hd
typing/invalid/error-message-unknown-name.hd
typing/invalid/error-second-cause.hd
typing/invalid/error-transparent-not-error.hd
typing/invalid/expected-i32-found-usize.hd
typing/invalid/extra-associated-type.hd
typing/invalid/facts-find-unbounded-key.hd
typing/invalid/facts-of-as-value.hd
typing/invalid/facts-of-closure.hd
typing/invalid/facts-of-local-binding.hd
typing/invalid/facts-of-method.hd
typing/invalid/facts-of-type-name.hd
typing/invalid/facts-of-without-import.hd
typing/invalid/field-reference.hd
typing/invalid/float-literal-map-key.hd
typing/invalid/float-map-key.hd
typing/invalid/float-misses-ord-bound.hd
typing/invalid/float-ord-bound.hd
typing/invalid/fn-row-argument-unknown-trait.hd
typing/invalid/folder-cycle-facade.hd
typing/invalid/folder-cycle-nested.hd
typing/invalid/for-binding-arity.hd
typing/invalid/for-binding-not-visible-in-else.hd
typing/invalid/for-over-string.hd
typing/invalid/for-refutable-pattern.hd
typing/invalid/forward-module-binding.hd
typing/invalid/fresh-element-not-weakened.hd
typing/invalid/from-implementation-suspending.hd
typing/invalid/from-implementation-with-requirement.hd
typing/invalid/from-iterator-not-prelude.hd
typing/invalid/from-no-fitting-instantiation.hd
typing/invalid/from-trait-value-target.hd
typing/invalid/function-anyref-bound.hd
typing/invalid/function-equality.hd
typing/invalid/function-identity-against-any.hd
typing/invalid/function-result-type-mismatch.hd
typing/invalid/function-type-non-tuple-inputs.hd
typing/invalid/function-type-orphan-impl.hd
typing/invalid/function-type-overlapping-impl.hd
typing/invalid/function-type-row-unknown-trait-nested.hd
typing/invalid/function-type-unbounded-inputs.hd
typing/invalid/function-value-no-default-arguments.hd
typing/invalid/generic-bound-unsatisfied.hd
typing/invalid/generic-call-built-nested-mut-not-weakened.hd
typing/invalid/generic-call-readonly-into-mut-part.hd
typing/invalid/generic-data-field-type-mismatch.hd
typing/invalid/generic-enum-variant-without-context.hd
typing/invalid/generic-erasure-without-bound.hd
typing/invalid/generic-function-value-argument-unsolved.hd
typing/invalid/generic-function-value-without-arguments.hd
typing/invalid/generic-inference-conflict.hd
typing/invalid/generic-inference-numeric-widening.hd
typing/invalid/generic-provider-key-collision-nested.hd
typing/invalid/generic-provider-key-collision-spread.hd
typing/invalid/generic-readonly-argument-to-mut-parameter.hd
typing/invalid/generic-requirement-key-collision.hd
typing/invalid/generic-trait-instantiation-no-fit.hd
typing/invalid/generic-trait-instantiations-ambiguous.hd
typing/invalid/generic-trait-literal-without-default.hd
typing/invalid/generic-variant-constructor-argument-unsolved.hd
typing/invalid/grammar-mutable-field-modifier.hd
typing/invalid/guarded-catch-all-not-exhaustive.hd
typing/invalid/guarded-match-not-exhaustive.hd
typing/invalid/hand-written-hash-beside-derived-eq.hd
typing/invalid/hd-run-requires-process.hd
typing/invalid/if-branch-misses-expected-type.hd
typing/invalid/impl-bound-bare-trait.hd
typing/invalid/impl-duplicate-exact-pair.hd
typing/invalid/impl-head-projection.hd
typing/invalid/impl-iterator-trait.hd
typing/invalid/impl-method-generic-bound-added.hd
typing/invalid/impl-method-generic-bound-changed.hd
typing/invalid/impl-method-generic-bound-dropped.hd
typing/invalid/impl-method-generic-bounds-reordered.hd
typing/invalid/impl-method-generic-count.hd
typing/invalid/impl-target-row-extension.hd
typing/invalid/implicit-data-equality.hd
typing/invalid/implicit-data-ordering.hd
typing/invalid/implicit-narrowing-i64.hd
typing/invalid/index-missing.hd
typing/invalid/index-set-missing.hd
typing/invalid/index-set-string.hd
typing/invalid/indirect-supertrait-cycle.hd
typing/invalid/indirect-top-level-forward-read.hd
typing/invalid/inferred-mutable-upgrade.hd
typing/invalid/inferred-row-reaches-public-caller.hd
typing/invalid/infinite-loop-break-missing-return.hd
typing/invalid/infinite-loop-variable-condition.hd
typing/invalid/init-read-through-generic-bound.hd
typing/invalid/init-read-through-trait-value.hd
typing/invalid/inspectable-child-trait-without-impl.hd
typing/invalid/inspectable-needs-import.hd
typing/invalid/integer-literal-match-without-catch-all.hd
typing/invalid/integer-literal-range.hd
typing/invalid/integer-narrowing.hd
typing/invalid/integration-file-beside-directory.hd
typing/invalid/integration-program-use.hd
typing/invalid/integration-super-above-test-root.hd
typing/invalid/integration-test-missing-requirement.hd
typing/invalid/integration-test-private-name.hd
typing/invalid/integration-tests-root-use.hd
typing/invalid/interpolation-without-display.hd
typing/invalid/invalid-map-key.hd
typing/invalid/it-each-name-clash.hd
typing/invalid/it-each-non-literal-name.hd
typing/invalid/it-each-outside-test-position.hd
typing/invalid/it-outside-test-code.hd
typing/invalid/it-shadowed.hd
typing/invalid/iterator-adapter-readonly.hd
typing/invalid/iterator-any-callback-row.hd
typing/invalid/iterator-chain-iterator-arg.hd
typing/invalid/iterator-filter-callback-row.hd
typing/invalid/iterator-map-callback-row.hd
typing/invalid/iterator-step-private.hd
typing/invalid/iterator-take-negative-literal.hd
typing/invalid/json-decode-not-derived.hd
typing/invalid/json-encode-not-derived.hd
typing/invalid/json-map-key-not-string.hd
typing/invalid/let-else-falls-through.hd
typing/invalid/let-else-irrefutable.hd
typing/invalid/let-else-name-in-else.hd
typing/invalid/let-mut-pattern-readonly-annotation.hd
typing/invalid/let-mut-pattern-readonly-element.hd
typing/invalid/let-mut-pattern-readonly-name.hd
typing/invalid/let-mut-primitive-annotation.hd
typing/invalid/let-mut-primitive.hd
typing/invalid/let-mut-readonly-annotation.hd
typing/invalid/let-mut-readonly-call-result.hd
typing/invalid/let-mut-readonly-mut-field.hd
typing/invalid/let-mut-readonly-value.hd
typing/invalid/let-mut-spread-mut-field.hd
typing/invalid/let-mut-tuple.hd
typing/invalid/let-readonly-fresh-literal.hd
typing/invalid/let-refutable-literal-field.hd
typing/invalid/let-refutable-without-else.hd
typing/invalid/list-call-not-callable.hd
typing/invalid/list-helper-callback-row.hd
typing/invalid/list-index-negative-literal.hd
typing/invalid/list-index-signed.hd
typing/invalid/list-parameter-bare-trait.hd
typing/invalid/list-slice-negative-literal.hd
typing/invalid/list-slice-signed-bound.hd
typing/invalid/list-spread-non-list.hd
typing/invalid/list-view-assignment.hd
typing/invalid/list-view-negative-literal.hd
typing/invalid/literal-dependent-no-backward-annotation.hd
typing/invalid/literal-dependent-no-backward.hd
typing/invalid/literal-erased-any-range.hd
typing/invalid/literal-fallback-hint.hd
typing/invalid/literal-first-use-bound.hd
typing/invalid/literal-first-use-closure-conflict.hd
typing/invalid/literal-first-use-conflict.hd
typing/invalid/literal-first-use-other-body.hd
typing/invalid/literal-first-use-range.hd
typing/invalid/literal-integer-into-float.hd
typing/invalid/literal-operand-negation-unsigned.hd
typing/invalid/literal-receiver-params-differ.hd
typing/invalid/literal-several-fit-bound.hd
typing/invalid/literal-suffix-extra-parameter.hd
typing/invalid/literal-suffix-float-into-integer.hd
typing/invalid/literal-suffix-generic-unbounded.hd
typing/invalid/literal-suffix-negation-no-neg.hd
typing/invalid/literal-suffix-negative-range.hd
typing/invalid/literal-suffix-no-parameter.hd
typing/invalid/literal-suffix-parameter-type.hd
typing/invalid/literal-suffix-requirement.hd
typing/invalid/literal-suffix-suspending.hd
typing/invalid/literal-suffix-unmarked.hd
typing/invalid/literal-trait-value-no-one-fit.hd
typing/invalid/literal-var-int-to-float.hd
typing/invalid/literal-var-range-bound-conflict.hd
typing/invalid/literal-var-structure-conflict.hd
typing/invalid/literal-var-top-level-undecided.hd
typing/invalid/literal-var-tuple-fallback.hd
typing/invalid/local-data-later-declaration.hd
typing/invalid/local-data-reuses-type-parameter.hd
typing/invalid/local-impl-after-field-default.hd
typing/invalid/local-impl-after-trait-default.hd
typing/invalid/local-impl-before-declaration.hd
typing/invalid/local-impl-child-suite-not-parent.hd
typing/invalid/local-impl-closure-before-declaration.hd
typing/invalid/local-impl-nonlocal-pair.hd
typing/invalid/local-impl-second-pair.hd
typing/invalid/local-impl-sibling-overlap.hd
typing/invalid/local-impl-trait-value-before-declaration.hd
typing/invalid/local-inherent-impl-before-declaration.hd
typing/invalid/local-inherent-impl-nonlocal-target.hd
typing/invalid/local-method-captures-local.hd
typing/invalid/local-supertrait-impl-after-child.hd
typing/invalid/local-value-reuses-type-parameter.hd
typing/invalid/main-not-importable.hd
typing/invalid/marker-bound-unproven.hd
typing/invalid/match-arm-after-catch-all.hd
typing/invalid/match-arm-misses-expected-type.hd
typing/invalid/match-guard-type-mismatch.hd
typing/invalid/member-line-outside-block.hd
typing/invalid/method-bound-bare-trait.hd
typing/invalid/method-without-requirement-clause.hd
typing/invalid/missing-associated-type-binding.hd
typing/invalid/missing-mutable-edge.hd
typing/invalid/missing-required-data-field.hd
typing/invalid/missing-requirement.hd
typing/invalid/missing-return-value.hd
typing/invalid/missing-supertrait-implementation.hd
typing/invalid/missing-trait-method.hd
typing/invalid/mixed-derived-law.hd
typing/invalid/mixed-numeric-power.hd
typing/invalid/module-file-and-directory-module.hd
typing/invalid/module-path-missing-member.hd
typing/invalid/module-path-private-member.hd
typing/invalid/module-path-private-std-function.hd
typing/invalid/module-qualified-without-use.hd
typing/invalid/mut-any-bound-readonly-argument.hd
typing/invalid/mut-iterator-iterable-bound.hd
typing/invalid/mut-on-primitive-annotation.hd
typing/invalid/mut-on-primitive-generic-optional.hd
typing/invalid/mut-on-primitive-optional.hd
typing/invalid/mut-on-primitive-parameter.hd
typing/invalid/mut-on-tuple-generic-optional.hd
typing/invalid/mut-on-tuple-optional.hd
typing/invalid/mut-on-type-parameter.hd
typing/invalid/mut-self-reference-readonly-callback.hd
typing/invalid/mut-tuple-annotation.hd
typing/invalid/mut-tuple-parameter.hd
typing/invalid/mut-upgrade.hd
typing/invalid/mutable-impl-target.hd
typing/invalid/mutable-provider-install-readonly-field.hd
typing/invalid/mutable-provider-install-readonly-value.hd
typing/invalid/mutual-recursion-omitted-results.hd
typing/invalid/negative-literal-exponent.hd
typing/invalid/nested-optional-needs-some.hd
typing/invalid/newtype-bound-bare-trait.hd
typing/invalid/newtype-derivation-block.hd
typing/invalid/newtype-over-data-permission.hd
typing/invalid/newtype-value-category.hd
typing/invalid/nil-is-unknown-name.hd
typing/invalid/no-fit-candidate.hd
typing/invalid/no-widening-argument.hd
typing/invalid/no-widening-assignment.hd
typing/invalid/no-widening-range-bounds.hd
typing/invalid/no-widening-return.hd
typing/invalid/nominal-map-key.hd
typing/invalid/non-entry-top-level-println.hd
typing/invalid/non-literal-test-name.hd
typing/invalid/nonexhaustive-bool-match.hd
typing/invalid/nonexhaustive-match.hd
typing/invalid/nonfinal-vararg-declaration.hd
typing/invalid/nonfinal-vararg-then-parameter.hd
typing/invalid/nonnumeric-unary-plus.hd
typing/invalid/num-bound-newtype.hd
typing/invalid/num-suffix-before-data.hd
typing/invalid/num-trait-needs-import.hd
typing/invalid/numeric-candidate-no-fit.hd
typing/invalid/omitted-embedded-part.hd
typing/invalid/omitted-member-without-default.hd
typing/invalid/operator-newtype-no-inherit.hd
typing/invalid/operator-power-user.hd
typing/invalid/operator-rhs-default-mismatch.hd
typing/invalid/operator-trait-needs-import.hd
typing/invalid/option-invariant.hd
typing/invalid/optional-generic-payload-weakening.hd
typing/invalid/optional-match-missing-none.hd
typing/invalid/optional-payload-mutable-outer.hd
typing/invalid/optional-payload-weakening.hd
typing/invalid/optional-readonly-payload-store.hd
typing/invalid/orphan-impl-alias-target.hd
typing/invalid/orphan-impl-foreign-trait-argument.hd
typing/invalid/orphan-impl-nested-trait-argument.hd
typing/invalid/orphan-impl-standard-data.hd
typing/invalid/orphan-impl-standard-enum.hd
typing/invalid/orphan-impl.hd
typing/invalid/overlapping-generic-impl-heads.hd
typing/invalid/overlapping-impl-despite-bounds.hd
typing/invalid/overlapping-impl-unifying-targets.hd
typing/invalid/overlapping-option-impl.hd
typing/invalid/overlapping-tuple-impl.hd
typing/invalid/parameter-bare-trait.hd
typing/invalid/parameter-default-order.hd
typing/invalid/parameter-default-type-mismatch.hd
typing/invalid/parent-declaration-not-in-child.hd
typing/invalid/part-trait-method-beside-method-promoted-into-part.hd
typing/invalid/part-trait-method-not-promoted.hd
typing/invalid/partial-eq-removed.hd
typing/invalid/partial-generic-arguments.hd
typing/invalid/partial-ord-requires-eq.hd
typing/invalid/payload-free-enum-equality.hd
typing/invalid/pipe-brackets-after-bare-step.hd
typing/invalid/pipe-call-step-without-placeholder.hd
typing/invalid/pipe-suspending-bare-step.hd
typing/invalid/pipe-suspending-method-reference.hd
typing/invalid/plain-break-in-value-loop.hd
typing/invalid/positional-spread-duplicates-vararg.hd
typing/invalid/positional-spread-without-vararg.hd
typing/invalid/possibly-uninitialized-binding.hd
typing/invalid/power-float-widths.hd
typing/invalid/power-mixed-numeric-types.hd
typing/invalid/prelude-shadow-console-parameter.hd
typing/invalid/prelude-shadow-println-function.hd
typing/invalid/prelude-shadow-renamed-use.hd
typing/invalid/prelude-shadow-result-generic.hd
typing/invalid/prelude-shadow-usize-alias.hd
typing/invalid/prelude-shadow.hd
typing/invalid/println-direct-forbidden-context-rule.hd
typing/invalid/println-in-defer.hd
typing/invalid/println-without-console.hd
typing/invalid/private-embedded-field-nothing-visible.hd
typing/invalid/private-field-nothing-visible.hd
typing/invalid/private-own-field-beside-conflicting-promoted.hd
typing/invalid/private-own-field-beside-deep-promoted.hd
typing/invalid/private-own-field-beside-promoted.hd
typing/invalid/private-own-field-needs-another-name.hd
typing/invalid/private-own-method-beside-promoted.hd
typing/invalid/private-own-method-nothing-visible.hd
typing/invalid/private-package-name.hd
typing/invalid/private-promoted-method-nothing-visible.hd
typing/invalid/private-std-function.hd
typing/invalid/private-std-type.hd
typing/invalid/promoted-field-assignment-readonly.hd
typing/invalid/promoted-field-conflict-at-declaration.hd
typing/invalid/promoted-method-conflict-at-declaration.hd
typing/invalid/promoted-mut-method-on-readonly-receiver.hd
typing/invalid/promoted-mut-self-method.hd
typing/invalid/propagation-conversion-then-injection.hd
typing/invalid/propagation-error-without-conversion.hd
typing/invalid/propagation-nearest-function-target.hd
typing/invalid/propagation-no-chained-conversion.hd
typing/invalid/propagation-operand-not-optional.hd
typing/invalid/propagation-without-target.hd
typing/invalid/property-examples-wrong-type.hd
typing/invalid/property-input-not-debug.hd
typing/invalid/provider-spread-of-non-context.hd
typing/invalid/provider-value-type-mismatch.hd
typing/invalid/pub-use-private-declaration.hd
typing/invalid/public-function-missing-result-type.hd
typing/invalid/public-method-missing-result-type.hd
typing/invalid/public-method-without-requirement-clause.hd
typing/invalid/public-test-item.hd
typing/invalid/qualified-string-prefix-call.hd
typing/invalid/qualified-string-prefix.hd
typing/invalid/range-full-not-iterable.hd
typing/invalid/range-pattern-bound-range.hd
typing/invalid/range-pattern-covered.hd
typing/invalid/range-pattern-empty.hd
typing/invalid/range-pattern-non-integer.hd
typing/invalid/range-pattern-nonexhaustive.hd
typing/invalid/range-to-inclusive-not-iterable.hd
typing/invalid/range-to-not-iterable.hd
typing/invalid/readonly-argument-for-mut-bound.hd
typing/invalid/readonly-bound-value-for-mut-bound.hd
typing/invalid/readonly-closure-capture-mutation.hd
typing/invalid/readonly-data-mutating-trait-method.hd
typing/invalid/readonly-edge-mut-self-call.hd
typing/invalid/readonly-embedded-source-with-mutable-edge.hd
typing/invalid/readonly-function-result.hd
typing/invalid/readonly-generic-field-init.hd
typing/invalid/readonly-iterator-in-comprehension.hd
typing/invalid/readonly-iterator-iterable-bound.hd
typing/invalid/readonly-list-append.hd
typing/invalid/readonly-list-element-replacement.hd
typing/invalid/readonly-loop-element-mutation.hd
typing/invalid/readonly-map-entry-replacement.hd
typing/invalid/readonly-map-remove.hd
typing/invalid/readonly-mut-field-method.hd
typing/invalid/readonly-mut-field-mutation.hd
typing/invalid/readonly-mut-field-reassignment.hd
typing/invalid/readonly-mutation.hd
typing/invalid/readonly-parent-mut-field-argument.hd
typing/invalid/readonly-provider-mut-method.hd
typing/invalid/readonly-requirement-trait-mut-use.hd
typing/invalid/readonly-spread-mut-field-twins.hd
typing/invalid/readonly-spread-mutable-field.hd
typing/invalid/readonly-trait-value-mut-bound.hd
typing/invalid/readonly-trait-value-mutating-method.hd
typing/invalid/reassign-parameter.hd
typing/invalid/reassign-short-binding-in-function.hd
typing/invalid/reassign-short-binding.hd
typing/invalid/reassign-short-module-binding.hd
typing/invalid/recursive-data-optional-field-required.hd
typing/invalid/redeclare-core-type.hd
typing/invalid/reference-bound-unsatisfied.hd
typing/invalid/relative-above-package-root.hd
typing/invalid/relative-path-into-std.hd
typing/invalid/relative-shared-test-above-test-root.hd
typing/invalid/requirement-key-argument-bare-trait.hd
typing/invalid/requirement-key-binding-collision.hd
typing/invalid/requirement-key-binding-missing.hd
typing/invalid/requirement-key-binding-provider.hd
typing/invalid/requirement-key-binding-row-unknown-trait.hd
typing/invalid/requirement-key-binding-subsumption.hd
typing/invalid/requirement-key-binding-unknown-type.hd
typing/invalid/requirement-key-duplicate-binding.hd
typing/invalid/requirement-key-missing-generic-argument.hd
typing/invalid/requirement-key-unbound.hd
typing/invalid/requirement-key-unknown-argument-type.hd
typing/invalid/requirement-key-unknown-binding-rule.hd
typing/invalid/requirement-key-unknown-binding.hd
typing/invalid/requirement-key-unknown-generic-trait.hd
typing/invalid/requirement-key-unknown-trait.hd
typing/invalid/result-and-then-error-type.hd
typing/invalid/result-constructor-without-context.hd
typing/invalid/result-invariant.hd
typing/invalid/result-ok-payload-type-mismatch.hd
typing/invalid/result-ok-without-unit.hd
typing/invalid/root-file-lib-super.hd
typing/invalid/root-file-super.hd
typing/invalid/root-mod-file.hd
typing/invalid/root-orphan-impl.hd
typing/invalid/row-alias-as-type.hd
typing/invalid/row-alias-bare-alias.hd
typing/invalid/row-alias-bare-target.hd
typing/invalid/row-alias-binding.hd
typing/invalid/row-alias-cycle.hd
typing/invalid/row-alias-explicit-type-argument.hd
typing/invalid/row-alias-generic-kind.hd
typing/invalid/row-alias-in-use.hd
typing/invalid/row-alias-mut-key.hd
typing/invalid/row-alias-type-argument.hd
typing/invalid/row-alias-unknown-key.hd
typing/invalid/row-extension-keeps-other-keys.hd
typing/invalid/row-extension-unsound.hd
typing/invalid/row-extension-without-provider.hd
typing/invalid/row-inference-conflict.hd
typing/invalid/row-inference-unavailable-provider.hd
typing/invalid/row-kind-mismatch.hd
typing/invalid/row-parameter-in-context.hd
typing/invalid/row-parameter-marked-on-data.hd
typing/invalid/row-parameter-propagates-callback-requirement.hd
typing/invalid/row-slot-bare-alias-argument.hd
typing/invalid/row-slot-bare-context.hd
typing/invalid/row-slot-bare-function-type.hd
typing/invalid/row-subsumption-missing-key.hd
typing/invalid/row-union-missing-requirement.hd
typing/invalid/same-depth-promotion-conflict.hd
typing/invalid/same-module-private-field-not-promoted.hd
typing/invalid/same-module-private-method-not-promoted.hd
typing/invalid/script-top-level-bang-call.hd
typing/invalid/self-receiver-is-readonly.hd
typing/invalid/serde-derive-member-not-deserialize.hd
typing/invalid/serde-derive-member-not-serialize.hd
typing/invalid/serde-map-key-not-string.hd
typing/invalid/shared-enum-constructor-missing-argument.hd
typing/invalid/shared-enum-default-order.hd
typing/invalid/shared-enum-payload-field-access.hd
typing/invalid/shared-enum-readonly-collection.hd
typing/invalid/shared-enum-variant-without-constructor.hd
typing/invalid/shift-count-signed.hd
typing/invalid/short-binding-readonly-root.hd
typing/invalid/sibling-module-binding-not-visible.hd
typing/invalid/sibling-module-function-not-imported.hd
typing/invalid/sibling-module-private-field.hd
typing/invalid/sibling-module-private-function-bare.hd
typing/invalid/sibling-module-private-function-import.hd
typing/invalid/sibling-module-private-method.hd
typing/invalid/sibling-module-std-name-not-imported.hd
typing/invalid/sibling-module-trait-name-not-imported.hd
typing/invalid/sibling-module-trait-not-imported.hd
typing/invalid/sibling-module-type-not-imported.hd
typing/invalid/sign-fallback-no-instantiation.hd
typing/invalid/signed-exponent.hd
typing/invalid/signed-integer-exponent.hd
typing/invalid/single-file-self-use.hd
typing/invalid/slice-assignment.hd
typing/invalid/snapshot-file-needs-test-runner.hd
typing/invalid/snapshot-non-literal-expect.hd
typing/invalid/spelled-function-type-not-inspectable.hd
typing/invalid/spread-pattern-arity.hd
typing/invalid/spread-pattern-fixed-tuple.hd
typing/invalid/spread-pattern-required.hd
typing/invalid/std-child-path-through-parent.hd
typing/invalid/str-prefix-before-data.hd
typing/invalid/strengthened-missing-bound.hd
typing/invalid/string-index-assignment.hd
typing/invalid/string-index-signed.hd
typing/invalid/string-pad-signed-width.hd
typing/invalid/string-prefix-extra-parameter.hd
typing/invalid/string-prefix-parameter-type.hd
typing/invalid/string-prefix-requirement.hd
typing/invalid/string-prefix-suspending.hd
typing/invalid/string-prefix-unmarked.hd
typing/invalid/string-prefix-value-mismatch.hd
typing/invalid/string-prefix-value-narrowing.hd
typing/invalid/string-repeat-negative-literal.hd
typing/invalid/string-slice-negative-offset.hd
typing/invalid/structure-implementation.hd
typing/invalid/supertrait-binding-conflict.hd
typing/invalid/supertrait-binding-mismatch.hd
typing/invalid/supertrait-binding-unknown.hd
typing/invalid/supertrait-cycle.hd
typing/invalid/supertrait-impl-bounds.hd
typing/invalid/supertrait-widening-not-reversed.hd
typing/invalid/suspension-constructor-to-bang-function.hd
typing/invalid/task-retry-row-missing.hd
typing/invalid/test-body-error-not-display.hd
typing/invalid/test-body-optional-result.hd
typing/invalid/test-body-string-error.hd
typing/invalid/test-body-uses-property-runner.hd
typing/invalid/test-module-name-not-imported.hd
typing/invalid/test-option-not-literal.hd
typing/invalid/test-registration-qualified-duplicate.hd
typing/invalid/test-registration-renamed-misplaced.hd
typing/invalid/test-timeout-string.hd
typing/invalid/tests-block-binding.hd
typing/invalid/tests-block-item-outside.hd
typing/invalid/tests-block-name-collision.hd
typing/invalid/tests-block-use-leak.hd
typing/invalid/top-level-defer.hd
typing/invalid/top-level-let-annotation-type-mismatch.hd
typing/invalid/top-level-return.hd
typing/invalid/trait-associated-reference-unsolved-self.hd
typing/invalid/trait-generic-instantiations-unrelated.hd
typing/invalid/trait-impl-method-missing-result-type.hd
typing/invalid/trait-impl-missing-method.hd
typing/invalid/trait-less-block-generic-argument.hd
typing/invalid/trait-less-block-generic-bound.hd
typing/invalid/trait-less-block-local.hd
typing/invalid/trait-less-block-method.hd
typing/invalid/trait-less-block-newtype.hd
typing/invalid/trait-less-delegation.hd
typing/invalid/trait-method-bare-trait.hd
typing/invalid/trait-method-beside-promoted-method.hd
typing/invalid/trait-method-receiver-mismatch.hd
typing/invalid/trait-method-result-type-mismatch.hd
typing/invalid/trait-method-row-unknown-trait.hd
typing/invalid/trait-method-signature.hd
typing/invalid/trait-parameter-bound-bare-trait.hd
typing/invalid/trait-qualified-supertrait-call.hd
typing/invalid/trait-qualified-supertrait-reference.hd
typing/invalid/trait-reference-unsolved-self.hd
typing/invalid/trait-value-binding-mismatch.hd
typing/invalid/trait-value-impl-target.hd
typing/invalid/trait-value-self-default.hd
typing/invalid/trait-value-unbound-associated-type.hd
typing/invalid/trait-value-unrelated-bound.hd
typing/invalid/trait-variance-marker.hd
typing/invalid/transitive-uninitialized-binding.hd
typing/invalid/tuple-binding-non-tuple.hd
typing/invalid/tuple-bound-non-tuple.hd
typing/invalid/tuple-element-assignment.hd
typing/invalid/tuple-inherent-impl.hd
typing/invalid/tuple-no-inherent-member.hd
typing/invalid/tuple-option-parameter-bare-trait.hd
typing/invalid/tuple-rest-assign.hd
typing/invalid/tuple-rest-literal-short.hd
typing/invalid/tuple-rest-not-list.hd
typing/invalid/tuple-spread-arity.hd
typing/invalid/tuple-spread-plain-into-vararg.hd
typing/invalid/tuple-spread-rest-into-plain.hd
typing/invalid/tuple-spread-tuple-operand.hd
typing/invalid/tuple-trait-user-impl.hd
typing/invalid/tuple-vararg-arity.hd
typing/invalid/tuple-vararg-no-auto-spread.hd
typing/invalid/two-payload-variant-as-function-value.hd
typing/invalid/type-argument-list-too-long.hd
typing/invalid/type-default-impl-mismatch.hd
typing/invalid/type-default-kind.hd
typing/invalid/type-default-later-parameter.hd
typing/invalid/type-default-order.hd
typing/invalid/type-name-as-value.hd
typing/invalid/typed-fact-not-field.hd
typing/invalid/typeid-of-any.hd
typing/invalid/typeid-of-function-argument.hd
typing/invalid/typeid-of-unbounded-parameter.hd
typing/invalid/u32-to-usize-binding.hd
typing/invalid/unary-minus-string.hd
typing/invalid/unary-plus-i32-literal-range.hd
typing/invalid/unavailable-trait-method-not-found.hd
typing/invalid/unconstrained-impl-parameter-unused.hd
typing/invalid/unconstrained-impl-parameter.hd
typing/invalid/unit-pattern-non-void.hd
typing/invalid/unit-test-missing-requirement.hd
typing/invalid/unit-test-real-clock.hd
typing/invalid/unknown-associated-type.hd
typing/invalid/unknown-dep-module.hd
typing/invalid/unknown-literal-suffix.hd
typing/invalid/unknown-package-name.hd
typing/invalid/unknown-panic-category.hd
typing/invalid/unknown-pkg-module.hd
typing/invalid/unknown-std-module-grouped.hd
typing/invalid/unknown-std-module.hd
typing/invalid/unknown-std-name.hd
typing/invalid/unknown-string-prefix.hd
typing/invalid/unknown-test-option.hd
typing/invalid/unknown-value-name.hd
typing/invalid/unknown-variant.hd
typing/invalid/unsaturated-enum-constructor.hd
typing/invalid/unsigned-negation.hd
typing/invalid/use-alias-original-name-unbound.hd
typing/invalid/use-module-and-root-declaration.hd
typing/invalid/use-without-provider.hd
typing/invalid/user-anyref-implementation.hd
typing/invalid/user-anyval-implementation.hd
typing/invalid/user-map-key-bound.hd
typing/invalid/usize-to-u32-argument.hd
typing/invalid/vararg-function-value-arity.hd
typing/invalid/vararg-function-value-list.hd
typing/invalid/vararg-rest-distinct.hd
typing/invalid/variance-contravariant-target-result.hd
typing/invalid/variance-covariant-target-parameter.hd
typing/invalid/variance-position.hd
typing/invalid/variant-pattern-missing-payload.hd
typing/invalid/variant-result-owner.hd
typing/invalid/written-type-too-many-arguments.hd
typing/valid/adapter-callback-captured-provider.hd
typing/valid/all-tuple-result.hd
typing/valid/ambiguous-requirement-key-annotated.hd
typing/valid/annotate-identifier.hd
typing/valid/annotations.hd
typing/valid/any-optional-none-and-mut-any.hd
typing/valid/any-void-never.hd
typing/valid/anyref-bound-accepts-references.hd
typing/valid/anyref-subtrait-reference-newtypes.hd
typing/valid/anyval-bound-accepts-values.hd
typing/valid/arbitrary-with-generic-generator.hd
typing/valid/associated-type-bindings.hd
typing/valid/bindings.hd
typing/valid/blanket-impl-and-associated-function.hd
typing/valid/bound-implies-supertrait.hd
typing/valid/callable-values.hd
typing/valid/child-trait-distinct-member-names.hd
typing/valid/choices-arbitrary.hd
typing/valid/choices-generic-draws.hd
typing/valid/closure-assigns-captured-let.hd
typing/valid/closure-mutable-capture-argument.hd
typing/valid/closure-mutable-capture.hd
typing/valid/closure-with-ignores-outer-key.hd
typing/valid/collect-targets.hd
typing/valid/collections.hd
typing/valid/comparison-traits.hd
typing/valid/composite-ordering.hd
typing/valid/compound-assignment.hd
typing/valid/console-error-entry.hd
typing/valid/console-host-provider-mutable.hd
typing/valid/console-recording-provider.hd
typing/valid/context-spread-normalization.hd
typing/valid/contextual-some-constructor.hd
typing/valid/contextual-variants.hd
typing/valid/control-flow.hd
typing/valid/copy-update-permissions.hd
typing/valid/covariant-callback-parameter.hd
typing/valid/covariant-inferred-private-result.hd
typing/valid/covariant-unrelated-requirement-row.hd
typing/valid/data-decorator.hd
typing/valid/data-default-calls-function-value.hd
typing/valid/data-field-defaults.hd
typing/valid/data-field-row-any-trait.hd
typing/valid/data-pattern-colon-labels.hd
typing/valid/data-patterns.hd
typing/valid/data-types.hd
typing/valid/debug-standard-types.hd
typing/valid/debug-writer-builders.hd
typing/valid/declaration-owns-requirement-clause.hd
typing/valid/declared-key-bounds.hd
typing/valid/decorator-target-kinds.hd
typing/valid/decorator-target-newtype-kind.hd
typing/valid/decorator-targets.hd
typing/valid/default-before-final-function.hd
typing/valid/default-calls-requirement-free-trait-method.hd
typing/valid/default-reads-top-level-binding.hd
typing/valid/default-trait.hd
typing/valid/defer-cleanup.hd
typing/valid/defer-indirect-block-on-checks.hd
typing/valid/derive-debug-shapes.hd
typing/valid/derive-debug.hd
typing/valid/derive-without-structure-use.hd
typing/valid/derived-arbitrary-generic.hd
typing/valid/derived-arbitrary-non-inspectable-member.hd
typing/valid/derived-equality.hd
typing/valid/derived-generic-bound.hd
typing/valid/derived-hash.hd
typing/valid/derived-ordering.hd
typing/valid/diamond-different-depths.hd
typing/valid/distinct-requirement-keys.hd
typing/valid/duplicate-trait-bound.hd
typing/valid/dyn-any-trait.hd
typing/valid/dynamic-safety-implied-anyref.hd
typing/valid/dynamic-safety-row-parameter.hd
typing/valid/dynamic-safety-transitive-anyref.hd
typing/valid/dynamic-trait-suspending-method.hd
typing/valid/embedded-field-decorator.hd
typing/valid/embedding-depth-three.hd
typing/valid/empty-literal-field-context.hd
typing/valid/entry-exit-code.hd
typing/valid/enum-closed-exhaustive-match.hd
typing/valid/eq-contextual-left-operand.hd
typing/valid/eq-contextual-right-operand.hd
typing/valid/eq-fresh-literal-readonly-operand.hd
typing/valid/equality.hd
typing/valid/erased-error-entry-point.hd
typing/valid/error-derivation.hd
typing/valid/error-generated-bounds.hd
typing/valid/error-interpolated-bound.hd
typing/valid/error-value-newtype.hd
typing/valid/error-without-import.hd
typing/valid/expect-panic-host-contract.hd
typing/valid/explicit-discard-and-void-result.hd
typing/valid/explicit-empty-row.hd
typing/valid/explicit-generic-method.hd
typing/valid/expression-type-arguments.hd
typing/valid/facts-find-generic-key.hd
typing/valid/float-remainder.hd
typing/valid/folder-loop-within-folder.hd
typing/valid/folder-parent-file.hd
typing/valid/for-over-mut-iterator.hd
typing/valid/fresh-mutable-values.hd
typing/valid/from-iterator-imported.hd
typing/valid/function-type-constructors.hd
typing/valid/function-type-impl-target.hd
typing/valid/function-type-tuple-impl.hd
typing/valid/function-type-variance.hd
typing/valid/functions.hd
typing/valid/generic-argument-placeholder.hd
typing/valid/generic-bound-names-later-parameter.hd
typing/valid/generic-call-fresh-list-of-boxes.hd
typing/valid/generic-data-embedding.hd
typing/valid/generic-field-permission.hd
typing/valid/generic-join-fresh-list-elements.hd
typing/valid/generic-join-mut-and-readonly.hd
typing/valid/generic-map-key.hd
typing/valid/generic-method-dynamically-safe.hd
typing/valid/generic-missing-call.hd
typing/valid/generic-mutual-bounds.hd
typing/valid/generic-optional-field-default.hd
typing/valid/generic-provider-keys-consistent-substitution.hd
typing/valid/generic-provider-keys-distinct.hd
typing/valid/generic-provider-keys-occurs-check.hd
typing/valid/generic-result-permission.hd
typing/valid/handle-fact-pattern.hd
typing/valid/hash-unverified-against-eq.hd
typing/valid/hd-run-integration.hd
typing/valid/host-entry-requirement.hd
typing/valid/identity-implied-anyref-bound.hd
typing/valid/identity-trait-value-operand.hd
typing/valid/impl-distinct-trait-arguments.hd
typing/valid/impl-parameter-fixed-by-binding.hd
typing/valid/impl-serves-mutable-view.hd
typing/valid/index-traits-builtin.hd
typing/valid/index-traits.hd
typing/valid/index-unsigned-widths.hd
typing/valid/inferred-mutable-from-mutable.hd
typing/valid/inferred-row-through-private-callers.hd
typing/valid/inherited-mut-method-requirement.hd
typing/valid/inner-mut-trait-instantiations.hd
typing/valid/iteration-protocol.hd
typing/valid/iterator-adapters.hd
typing/valid/lct-drops-never.hd
typing/valid/lct-structural-join-order.hd
typing/valid/lct-variance-every-site.hd
typing/valid/lct-wide-variance-join.hd
typing/valid/let-data-pattern.hd
typing/valid/let-else-diverging-forms.hd
typing/valid/let-else-scope.hd
typing/valid/let-else.hd
typing/valid/let-mut-infer.hd
typing/valid/let-mut-optional.hd
typing/valid/let-mut-pattern.hd
typing/valid/let-mut-spread-fresh-field.hd
typing/valid/let-nested-pattern.hd
typing/valid/let-pattern-read.hd
typing/valid/let-readonly-rebind.hd
typing/valid/let-underscore-discard.hd
typing/valid/lib-root-pkg.hd
typing/valid/list-literal-compares-with-readonly-binding.hd
typing/valid/literal-classes-separate-widths.hd
typing/valid/literal-dependent-join.hd
typing/valid/literal-first-use-closure.hd
typing/valid/literal-first-use-fallback.hd
typing/valid/literal-first-use-float-argument.hd
typing/valid/literal-first-use-generic-join.hd
typing/valid/literal-first-use-generic-wait.hd
typing/valid/literal-first-use-method.hd
typing/valid/literal-first-use-top-level.hd
typing/valid/literal-first-use-while-len.hd
typing/valid/literal-one-fit-bound.hd
typing/valid/literal-one-fit-receiver.hd
typing/valid/literal-suffix-compile-time.hd
typing/valid/literal-suffix-duration.hd
typing/valid/literal-suffix-generic-num.hd
typing/valid/literal-suffix-generic-target.hd
typing/valid/literal-suffix-generic.hd
typing/valid/literal-suffix-local-shadow.hd
typing/valid/literal-suffix-negation.hd
typing/valid/literal-suffix-requirement.hd
typing/valid/literal-suffix-result-type.hd
typing/valid/literal-suffix-user.hd
typing/valid/literal-var-data-pattern.hd
typing/valid/literal-var-fallback.hd
typing/valid/literal-var-flow-types.hd
typing/valid/literal-var-for-range-index.hd
typing/valid/literal-var-list-adopts.hd
typing/valid/literal-var-list-push.hd
typing/valid/literal-var-match-payload.hd
typing/valid/literal-var-method-fallback.hd
typing/valid/literal-var-range-bound.hd
typing/valid/literal-var-tuple.hd
typing/valid/local-fn-in-provider-scope.hd
typing/valid/local-functions.hd
typing/valid/local-recursive-data.hd
typing/valid/local-types.hd
typing/valid/map-indexing.hd
typing/valid/match-guard-reads-binding.hd
typing/valid/match-guards.hd
typing/valid/method-references.hd
typing/valid/module-bindings.hd
typing/valid/mut-outer-views-valid.hd
typing/valid/mut-self-primitive-impl.hd
typing/valid/mut-trait-value.hd
typing/valid/mutable-access-bound.hd
typing/valid/mutable-closure-capture.hd
typing/valid/mutable-loop-element-mutation.hd
typing/valid/mutable-paths.hd
typing/valid/mutable-provider-rows.hd
typing/valid/mutual-recursion-one-annotated.hd
typing/valid/named-variant-bindings.hd
typing/valid/names-and-scopes.hd
typing/valid/nested-data-argument-from-field-type.hd
typing/valid/nested-dynamically-safe-trait-values.hd
typing/valid/newtype-value-category.hd
typing/valid/newtypes-and-widening.hd
typing/valid/nonpublic-main-ordinary.hd
typing/valid/num-families.hd
typing/valid/num-trait-imported.hd
typing/valid/num-traits.hd
typing/valid/numeric-corners.hd
typing/valid/numeric-literal-adopts-width.hd
typing/valid/numeric-literal-left-operand.hd
typing/valid/numeric-widening-and-display-list.hd
typing/valid/ok-err-are-not-prelude-names.hd
typing/valid/operator-bitwise-user.hd
typing/valid/operator-generic-primitive.hd
typing/valid/operator-newtype-hand-written.hd
typing/valid/operator-rhs-default.hd
typing/valid/operator-traits.hd
typing/valid/option-result.hd
typing/valid/optional-outer-weakening.hd
typing/valid/optional-result-explicitly-discarded.hd
typing/valid/optional-some-pattern.hd
typing/valid/outer-constructor-permits-impl.hd
typing/valid/own-from-and-error-names.hd
typing/valid/own-member-hides-promoted-conflict.hd
typing/valid/own-trait-method-beside-part-trait.hd
typing/valid/pack-ordinary-name.hd
typing/valid/parameter-decorator.hd
typing/valid/parameter-default-writes-state.hd
typing/valid/part-trait-method-beside-promoted-inherent.hd
typing/valid/part-trait-method-ignored-by-lookup.hd
typing/valid/partly-consumed-iterator.hd
typing/valid/path-newtype.hd
typing/valid/per-trait-self-line-foreign-fact.hd
typing/valid/pipe-steps.hd
typing/valid/prelude-surface.hd
typing/valid/println-top-level-script.hd
typing/valid/private-data-embeds-private-type.hd
typing/valid/private-function-inferred-void.hd
typing/valid/private-method-infers-result-and-row.hd
typing/valid/private-own-member-beside-private-part-member.hd
typing/valid/private-type-in-public-body.hd
typing/valid/property-runner-capabilities.hd
typing/valid/provider-capturing-closure.hd
typing/valid/pub-own-member-shadows-promoted.hd
typing/valid/pub-use-chain.hd
typing/valid/public-requirement-row.hd
typing/valid/race-plain-signature.hd
typing/valid/random-trait.hd
typing/valid/range-pattern-coverage.hd
typing/valid/range-types.hd
typing/valid/raw-identifiers.hd
typing/valid/readonly-callable-mutable-result.hd
typing/valid/readonly-embedded-source-mutable-result.hd
typing/valid/readonly-embedded-source-with-mutable-edge-readonly-result.hd
typing/valid/readonly-list-mutable-elements.hd
typing/valid/readonly-map-mutable-values.hd
typing/valid/readonly-outer-mutable-field-init.hd
typing/valid/readonly-part-nested-data-literal.hd
typing/valid/readonly-value-direct-mut-field.hd
typing/valid/reference-generic-identity.hd
typing/valid/reimport-prelude-name.hd
typing/valid/relative-repeated-super.hd
typing/valid/relative-self-lib.hd
typing/valid/relative-self-main.hd
typing/valid/relative-self-mod-file.hd
typing/valid/relative-shared-test-module.hd
typing/valid/requirement-key-any-trait.hd
typing/valid/requirement-key-binding.hd
typing/valid/requirement-key-dynamically-safe-generics.hd
typing/valid/requirement-key-supertrait-any-trait.hd
typing/valid/requirement-key-supertrait-binding.hd
typing/valid/requirement-key-trait-value-argument.hd
typing/valid/requirement-key-user-trait-named-inspectable.hd
typing/valid/requirement-row-duplicate-keys.hd
typing/valid/requirement-row-plus-list.hd
typing/valid/requirements-and-suspension.hd
typing/valid/resource-disposed-result.hd
typing/valid/retention-metadata.hd
typing/valid/row-alias-bare-context.hd
typing/valid/row-alias-bare-impl-head.hd
typing/valid/row-alias-bare-slots.hd
typing/valid/row-alias-empty.hd
typing/valid/row-alias-generic.hd
typing/valid/row-alias-nested.hd
typing/valid/row-alias-one-key.hd
typing/valid/row-alias.hd
typing/valid/row-extension-absent-key.hd
typing/valid/row-extension-entailment.hd
typing/valid/row-kinded-arguments.hd
typing/valid/row-list-copy-wider.hd
typing/valid/row-parameter-marked.hd
typing/valid/row-pattern-fixed-by-parameter.hd
typing/valid/row-polymorphic-callback.hd
typing/valid/row-slots.hd
typing/valid/row-subsumption-list.hd
typing/valid/row-subsumption-sites.hd
typing/valid/row-union-closure-result.hd
typing/valid/row-union-if.hd
typing/valid/row-union-list.hd
typing/valid/row-union-map.hd
typing/valid/row-union-match.hd
typing/valid/row-union-omitted-result.hd
typing/valid/row-union-private-result.hd
typing/valid/row-union-spread.hd
typing/valid/same-module-private-member-explicit-path.hd
typing/valid/shared-enum-default-writes-state.hd
typing/valid/shared-enum-defaults.hd
typing/valid/shift-count-unsigned.hd
typing/valid/sign-fallback-separate-groups.hd
typing/valid/sign-fallback-signed.hd
typing/valid/sign-fallback-unsigned.hd
typing/valid/slice-full-types.hd
typing/valid/slice-types.hd
typing/valid/snapshot-inline.hd
typing/valid/std-derivation-types-debug.hd
typing/valid/std-host-types-debug.hd
typing/valid/std-joined-types-debug.hd
typing/valid/std-reexported-child-item.hd
typing/valid/stored-suspension-driving.hd
typing/valid/string-bytes.hd
typing/valid/string-prefix-generic.hd
typing/valid/string-prefix-local-shadow.hd
typing/valid/string-prefix-requirement.hd
typing/valid/string-prefix-std-r.hd
typing/valid/string-prefix-user.hd
typing/valid/string-semantics.hd
typing/valid/supertrait-associated-binding.hd
typing/valid/supertrait-binding-diamond-merge.hd
typing/valid/supertrait-binding.hd
typing/valid/supertrait-chain.hd
typing/valid/task-retry-row.hd
typing/valid/temp-dir-integration.hd
typing/valid/test-body-uses-test-runner.hd
typing/valid/test-runner-capabilities.hd
typing/valid/test-timeout-call.hd
typing/valid/test-timeout-duration.hd
typing/valid/three-embedded-fields.hd
typing/valid/trait-argument-owner-impl.hd
typing/valid/trait-delegation-associated-type.hd
typing/valid/trait-delegation-mut-self.hd
typing/valid/trait-delegation.hd
typing/valid/trait-features.hd
typing/valid/trait-less-block-generic-rename.hd
typing/valid/trait-less-block-generic.hd
typing/valid/trait-less-derivation-block.hd
typing/valid/trait-less-self-line-fact.hd
typing/valid/trait-qualified-call-generic-receiver.hd
typing/valid/trait-qualified-declaring-trait.hd
typing/valid/trait-resolution-depth-limit.hd
typing/valid/trait-value-as-target-argument.hd
typing/valid/trait-value-bound-associated-type.hd
typing/valid/traits.hd
typing/valid/transitive-initialized-binding.hd
typing/valid/tuple-derived-traits.hd
typing/valid/tuple-template.hd
typing/valid/tuple-trait-bound.hd
typing/valid/tuple-trait-rest.hd
typing/valid/type-default-calls.hd
typing/valid/type-default-function-value.hd
typing/valid/type-level-fact-without-template.hd
typing/valid/type-parameter-shadows-module-name.hd
typing/valid/typed-derivation-build.hd
typing/valid/typed-derivation.hd
typing/valid/typed-fact-concrete-pattern.hd
typing/valid/typed-fact-expected-line.hd
typing/valid/typed-fact-expected.hd
typing/valid/typed-fact-fn-pattern.hd
typing/valid/typed-fact-function.hd
typing/valid/typed-fact-generic-owner.hd
typing/valid/typed-fact-other-params.hd
typing/valid/typed-fact-pattern-mut-field.hd
typing/valid/typed-fact-pattern-other-params.hd
typing/valid/typed-fact.hd
typing/valid/unsigned-comparison-signed-countdown.hd
typing/valid/untyped-fact-default.hd
typing/valid/unused-underscore-bindings.hd
typing/valid/use-path-only-declaration.hd
typing/valid/user-defined-all.hd
typing/valid/user-trait-for-standard-type.hd
typing/valid/usize-positions.hd
typing/valid/usize-type.hd
typing/valid/value-category-traits.hd
typing/valid/variance-contravariant-target-parameter.hd
typing/valid/variance-invariant-target.hd
typing/valid/variance-opposing-target-signs.hd
typing/valid/variance-permission-weakening.hd
typing/valid/variance-receiverless-function.hd
typing/valid/variance-separate-trait-impl.hd
typing/valid/variant-pattern-lists-payload.hd
typing/valid/void-unit-alias.hd
typing/warnings/derivation-block-decorator-std.hd
typing/warnings/derivation-block-decorator.hd
typing/warnings/let-list-redundant-mut.hd
typing/warnings/let-mut-optional-payload.hd
typing/warnings/private-main-suspending.hd
typing/warnings/private-main.hd
typing/warnings/redundant-let-mut.hd
typing/warnings/same-line-let-list-unused.hd
typing/warnings/unused-after-rejected-trials.hd
typing/warnings/unused-local-binding.hd
typing/warnings/unused-nested-optional-binding.hd
```
<!-- pass-list-end -->

## CLI Conformance

The CLI tier (`spec/conformance/cli-cases.tsv`) runs in
`hd_cli/tests/cli_conformance.rs` against the `hd` binary.

| Pass | Fail | Unsupported | Total |
| ---: | ---: | ---: | ---: |
| 85 | 17 | 0 | 102 |

`HD_UPDATE_CONFORMANCE=1` replaces this list with every CLI case that passes.

<!-- cli-pass-list-start -->
```text
cli/build-instantiation-too-deep
cli/build-library-only
cli/build-output
cli/cap-env-notice
cli/cap-flag-overrides-table
cli/cap-partial-deny
cli/cap-total-deny
cli/check-summary
cli/clean-build
cli/clean-outside-package
cli/clean-workspace
cli/dbg-uses
cli/dbg-value-forms
cli/dbg-values
cli/dep-dev-remove
cli/dep-invalid-manifest
cli/dep-key-collision
cli/dep-missing-sum
cli/dep-no-library
cli/dep-one-key-per-line
cli/dep-outside-package
cli/dep-package-cycle
cli/dep-path-local
cli/dep-path-no-package
cli/dep-workspace-fetch
cli/derivation-lines-agree
cli/dev-dependency-cyclic-integration
cli/dev-dependency-cyclic-unit
cli/dev-dependency-integration
cli/dev-dependency-non-test
cli/dev-dependency-tests-block
cli/entry-err-chain
cli/entry-err-display
cli/exe-main-unlisted
cli/exe-missing-module
cli/exe-unselected-main
cli/exe-unselected-main-used
cli/exit-hd-failure
cli/exit-outside-package
cli/exit-package-check
cli/exit-package-file
cli/exit-program-status
cli/exit-test-empty
cli/exit-test-failure
cli/exit-usage-error
cli/fmt-check
cli/fmt-syntax-error
cli/json-check-clean
cli/json-check-error
cli/json-check-modules-checked
cli/json-check-warning
cli/json-diagnostic-fixes
cli/json-file-location
cli/json-file-single
cli/json-run
cli/json-test-fail
cli/json-test-ignored
cli/json-test-order
cli/json-test-pass
cli/manifest-unknown-key
cli/member-unlisted
cli/new-app
cli/new-existing
cli/new-lib
cli/new-no-kind
cli/new-no-pages
cli/new-pages
cli/new-pages-existing
cli/new-path
cli/new-vcs
cli/task-beside-dir
cli/task-name-clash
cli/test-err-report
cli/test-every-case
cli/test-integration-env
cli/test-report
cli/test-snapshot-file
cli/test-tasks
cli/test-timeout
cli/test-unit-fakes
cli/toolchain-too-old
cli/typeid-package-name
cli/wasm-cap-flags-only
cli/wasm-invalid
cli/wasm-run-built
```
<!-- cli-pass-list-end -->
