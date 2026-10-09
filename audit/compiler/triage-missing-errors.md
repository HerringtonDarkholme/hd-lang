# Triage: Missing Errors On Invalid Programs (R5)

Method: same per-case log as R1/R2, rerun at `42dbf1ac` (250 lines
with expectation `reject:<code>` and verdict `fail:no-diagnostic`;
the brief said "about 260"). Load stayed above 30 for the whole
session, so no fresh full conformance run was started per the queue's
load rule; two O-lane commits landed after the log's base (both
`compiler/crates` product work, no fixture changes), so counts may
have shifted by a case or two — the log is the source of truth and
every row below was mechanically assigned (250/250, verified by
script). Top shrinks were verified with `target/release/hd check`
(scratch file for decorators) and `hd build` (scratch package for
entry rules). Read only: no compiler or fixture edits.

| # | Missing check (count) | Cases | Minimal program | Spec rule / expected code | Stage that should raise it |
| --- | --- | --- | --- | --- | --- |
| 1 | Decorators, annotations and facts unchecked (21). Matches #20, #24, #25, #27. | typed-fact-mismatch, typed-fact-expected-mismatch, typed-fact-value-mismatch, typed-fact-fn-pattern-arity, typed-fact-fn-pattern-field, typed-fact-fn-pattern-suspending, typed-fact-concrete-pattern-mismatch, typed-fact-function-suspending, typed-fact-bound, typed-fact-fn-pattern-bound, duplicate-fact, duplicate-declaration-fact, trait-less-block-duplicate-fact, duplicate-type-level-fact, duplicate-function-fact, unknown-annotation-member, member-line-payload-member, invalid-facts-of-target ×2, marker-template, arbitrary-with-wrong-generator, facts-of-closure, facts-of-local-binding | `@totally_bogus_decorator` on a function → clean (verified); `@fact("x")` on an `i32` member → clean (verified, `typed-fact-mismatch.hd`) | annot.typed-fact.check, annot.metadata.duplicate, annot.decorator.duplicate, annot.fact.duplicate-decorator; `type-mismatch` / `duplicate-fact` / `unknown-annotation-member` / `invalid-facts-of-target` | Check (decorator resolution and checking) |
| 2 | Test-harness validation missing (14) | duplicate-test-name, it-each-name-clash, test-registration-qualified-duplicate, non-literal-test-name, test-option-not-literal, unknown-test-option, misplaced-test-case, test-case-as-value, misplaced-tests-block ×2, test-timeout-string, duplicate-tests-block, public-test-item, invalid-test-statement ×2 | `it("counts"):` twice in one `tests:` block → clean (from `duplicate-test-name.hd`); `it("waits", timeout="5s"):` → clean (verified pattern, `test-timeout-string.hd`) | Test Cases / Test Blocks rules; `duplicate-test-name`, `type-mismatch`, `misplaced-test-case`, … | Check (test overlay) |
| 3 | Entry checks only fire in package builds (7) | entry-result-not-termination, nonhost-entry-requirement, entry-point-parameters ×2 (incl. generic-entry-point), nondisplay-entry-error, test-body-error-not-display, test-body-optional-result | `pub fn main() -> i32: 0` in a bare file → clean (verified); the same in a package `hd build` → errors, but as `unsupported` (Collect: no `Termination` impl), not the expected `unsatisfied-trait-bound` (verified) | Entry Point / entry behavior; `unsatisfied-trait-bound`, `nonhost-entry-requirement`, `entry-point-parameters` | Check under an entry goal (bare-file `Analyze` skips them); plus a wrong-code follow-up at build |
| 4 | Function-type formation unchecked (6) | function-type-rest-not-list, vararg-type-not-collection, tuple-rest-not-list, vararg-function-value-list, function-type-non-tuple-inputs, function-type-unbounded-inputs | `fn apply(callback: fn(i32...) -> i32) -> i32: 0` → clean (from `function-type-rest-not-list.hd`) | Rest/vararg well-formedness; `type-mismatch` / `generic-kind-mismatch` | Check (type formation) |
| 5 | Visibility leaks unchecked (12). Matches #38, #40. | private-type-leak ×6 (signature, embedded, row, field, payload, bound), private-member ×3 (iterator-step-private, sibling-module-private-field, data-literal-private-field-other-module), boundary-private-field ×3 | `pub fn reveal() -> Secret:` with private `data Secret` → clean (from `private-type-leak.hd`) | module.vis.signature*; `private-type-leak`, `private-member`, `boundary-private-field` | Check (Resolve/visibility) |
| 6 | Forbidden suspension contexts unchecked (6) | suspension-forbidden-context, block-on-in-defer/fact/metadata, println-in-defer, println-direct-forbidden-context-rule | `block_on(pending)` in a plain function → clean (from `suspension-forbidden-context.hd`) | Driving/suspension contexts; `suspension-forbidden-context` | Check |
| 7 | Associated-type and requirement-key binding checks missing (16). Matches #57. | generic-requirement-key-collision ×5 (incl. nested, lexical, bindings, closure), duplicate-associated-binding ×4, ambiguous-associated-type ×3, unknown-associated-type ×4 | `impl[K, V] Catalog for Map[K, V]`-style collisions → clean (headers sampled) | trait.binding.once*, req.with.collision*; `duplicate-associated-binding`, `ambiguous-associated-type`, `unknown-associated-type`, `generic-requirement-key-collision` | Check (Resolve) |
| 8 | Supertrait satisfaction unchecked (5) | missing-supertrait-implementation, supertrait-binding-mismatch, supertrait-impl-bounds, anyref-subtrait ×2 | Headers sampled | trait.super.*; `missing-supertrait-implementation` | Check (solver) |
| 9 | Test-runner requirement rows unchecked (5) | unit-test-missing-requirement, row-union-missing-requirement, test-body-uses-property-runner, script-test-init-requirement, unit-test-real-clock | Headers sampled | module.testing.unit-row.*; `missing-requirement` | Check (test overlay) |
| 10 | Overlapping impls unchecked (5) | trait-less-block-second, error-hand-written-display, error-hand-written-from, error-from-same-type, tuple-template-overlap | Headers sampled | annot.no-trait.unique, annot.error.hand-written, annot.template.tuple.overlap; `overlapping-impl` | Check (Collect) |
| 11 | Error-attribution bounds unchecked (5) | error-message-not-display, error-cause-not-error, error-transparent-not-error, display-tuple-element-without-display, default-tuple-element-without-default | `@error enum SaveError: Path(@source reason: string)` → clean (from `error-cause-not-error.hd`) | annot.error.*; `unsatisfied-trait-bound` | Check (derive) |
| 12 | Duplicate declarations unchecked (16) | duplicate-field ×4 (incl. declaration, shorthand, payload), duplicate-trait-member ×4 (child-trait-redeclares ×4), duplicate-module-name ×2 (incl. overload, tests-block-name-collision), duplicate-inherent-member ×2, duplicate-binding/variant/supertrait/embedded-field/generic-embedded-name ×1 | `data` with one field twice → clean (from `duplicate-field.hd` pattern) | data.literal.duplicate, trait.binding.*, module uniqueness; `duplicate-field`, `duplicate-trait-member`, … | Check |
| 13 | Embedding shape checks unchecked (16). Matches #41. | embedded-copy-required ×3, embedding-cycle ×3, embedded-non-data ×3, invalid-member-line ×3, embedding-too-deep ×2, copy-into-ordinary-field, too-many-embedded-fields | Headers sampled | Data embedding rules; `embedded-copy-required`, `embedding-cycle`, `embedded-non-data`, `invalid-member-line`, … | Check |
| 14 | Delegation checks unchecked (5, incl. a missing-method case) | delegation-to-ordinary-field, delegation-part-lacks-trait, delegation-binds-associated-type, trait-less-delegation, delegation-associated-function-missing (expects `missing-trait-method`) | Headers sampled | Delegation rules | Check |
| 15 | std.testing Eq/Debug bounds unenforced (3) | assert-equal-non-eq, assert-equal-without-debug, assert-equal-numeric-widening | `assert_equal(actual, .Ok(1))` with non-`Eq` `Error` → clean (verified, `assert-equal-non-eq.hd`) | module.testing.no-implicit-eq; `unsatisfied-trait-bound` | Check (call checking of `assert_equal`) |
| 16 | Map key bounds unenforced (2) | tuple-map-key, list-float-map-key | `let points: Map[(i32, f64), string] = {}` → clean (verified, `tuple-map-key.hd`) | Map key `Hash`/`Eq`; `unsatisfied-trait-bound` | Check |
| 17 | Join ignores variance/invariance (3) | generic-inference-variance-conflict, generic-inference-optional-conflict, row-union-list-no-convert | `pick(owned, shared)` for `List[mut User]`/`List[User]` → clean (from `generic-inference-variance-conflict.hd`); `serve_all(handlers)` row-union list → clean (verified, `row-union-list-no-convert.hd`) | types.generic.infer.join.*; `type-mismatch` | Check (inference join) |
| 18 | Ordering/default/visibility odds and ends (24) | defaults-order ×10 (default-order ×3, partial-generic-arguments ×3, omitted-member-without-default ×2, invalid-default-variant ×2), init-order ×7 (binding-not-yet-visible ×3, top-level-read-before-initialization ×3, possibly-uninitialized-binding), kind-checks ×5 (row-kind-mismatch, type-default-kind, row-parameter-marked-on-data, row-slot-bare ×2), invalid-impl-target ×3, unconstrained-impl ×3, inspectable-keys ×3, prelude-shadow ×4, pipe-rules ×2, contravariance ×2, defer-cf ×2, missing-result-type ×2, nonfinal-vararg ×2, unknown-import ×2, unknown-method ×2, argument-count ×2, extra-member ×2, supertrait-cycle ×2, dyn-unsafe ×2, trait-value-target ×2, other-bounds ×2 | Headers sampled; codes as listed | Per-fixture rules; codes as listed | Check (mostly formation/Resolve) |
| 19 | Type-mismatch singletons (5) | generic-supertrait-upcast-mismatch (`dyn` widen), requirement-key-binding-provider, operator-function-left-exact, concrete-value-binding-mismatch, literal-integer-into-float | `let ratio: f64 = 1` → clean (from `literal-integer-into-float.hd`) | Per-fixture rules; `type-mismatch` | Check |
| 20 | Unresolved-inference singletons (2) | trait-assoc-call-self-undetermined, unresolved-generic-return-placeholder | Headers sampled | `cannot-infer-type` | Check |
| 21 | Misc singletons (24, one row each in the log) | float-literal-range, bare-parameter-impl-target, requirement-in-default, duplicate-literal-match-arm, incompatible-identity-operands, inspectable-member-redeclared, structure-outside-template, generic-member-call, newtype-derivation-self, structure-without-use, test-body-string-error, unknown-panic-category, nested-local-annotation-bare-trait, module-named-pkg, module-path-not-identifier, assoc-call-parameter-two-bounds, ambiguous-requirement-key-solution, dyn-self-parameter-unavailable, dyn-inherent-nonlocal, foreign-inherent-impl, uninhabited-binding, template-names-binding, hd-run-outside-integration, trait-resolution-depth | Not individually examined — codes and fixtures as listed | Per-fixture codes | Check |

## Questions

- Row 3 (entry checks): is bare-file silence acceptable (entry rules
  only apply under a package/entry goal), with the remaining work being
  the wrong-code follow-up (`unsupported` instead of
  `unsatisfied-trait-bound` at build)? Or should `hd check FILE` also
  enforce entry rules? Recommendation: keep bare-file silence as
  designed (a bare file is not a program), fix the code at build.
  Needs an owner ruling before the orchestrator queues it.
- Row 1 (decorators): unresolved decorators are silently ignored even
  today (`@totally_bogus_decorator` passes). If that is intended
  (decorators resolve later, at a different stage), several rows here
  collapse into "one decorator stage"; if not, it is its own check.
  Same owner-level call.

## Loose ends

- Log base is `42dbf1ac`; rerun the per-case listing on current main
  before queueing tasks (load rule blocked it this session).
- Rows 8–21 beyond the named cases were grouped by code and header,
  not shrunk; shrinking is the first step of each compiler task.
- No spec/fixture issues found that belong in
  `audit/compiler/diagnostic-notes.md` (every sampled fixture reads
  as a genuinely missing check, not a wrong fixture).
