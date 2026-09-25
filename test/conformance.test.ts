import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

import { analyze, instantiate } from "../src/compiler.ts";
import { parse } from "../src/parser.ts";

const CASES = [
  ["parse/valid/functions-and-closures.hd", "accept"],
  ["parse/valid/layout.hd", "accept"],
  ["parse/valid/unicode-identifiers.hd", "accept"],
  ["parse/valid/doc-comments.hd", "accept"],
  ["parse/invalid/doc-comment-without-target.hd", "reject:doc-comment-without-target"],
  ["parse/invalid/mutable-field-modifier.hd", "reject:mutable-field-modifier"],
  ["parse/invalid/mutable-embedded-field.hd", "reject:mutable-embedded-field"],
  ["parse/invalid/old-struct-declaration.hd", "reject:old-struct-declaration"],
  ["parse/invalid/declaration-without-let.hd", "reject:missing-let"],
  ["parse/invalid/reserved-name-collision.hd", "reject:reserved-name"],
  ["parse/invalid/old-import-declaration.hd", "reject:old-import-declaration"],
  ["parse/invalid/old-export-declaration.hd", "reject:old-export-declaration"],
  ["parse/invalid/pub-trait-method.hd", "reject:trait-method-visibility"],
  ["parse/valid/utf8-bom.hd", "accept"],
  ["parse/invalid/tab-indentation.hd", "reject:tab-whitespace"],
  ["parse/invalid/invalid-escape.hd", "reject:invalid-escape"],
  ["parse/invalid/mid-file-bom.hd", "reject:unexpected-bom"],
  ["parse/invalid/semicolon.hd", "reject:reserved-semicolon"],
  ["parse/invalid/chained-comparison.hd", "reject:comparison-chaining"],
  ["parse/invalid/positional-after-named.hd", "reject:argument-order"],
  ["parse/invalid/named-pattern-before-positional.hd", "reject:pattern-order"],
  ["parse/invalid/trailing-block-in-brackets.hd", "reject:trailing-block-position"],
  ["parse/invalid/multi-binding-needs-parentheses.hd", "reject:multi-binding-needs-parentheses"],
] as const;

const TYPE_CASES = [
  ["typing/valid/data-patterns.hd", "accept"],
  ["typing/valid/data-types.hd", "accept"],
  ["typing/valid/bindings.hd", "accept"],
  ["typing/valid/fresh-mutable-values.hd", "accept"],
  ["typing/valid/copy-update-permissions.hd", "accept"],
  ["typing/valid/generic-field-permission.hd", "accept"],
  ["typing/valid/readonly-outer-mutable-field-init.hd", "accept"],
  ["typing/valid/map-indexing.hd", "accept"],
  ["typing/valid/readonly-list-mutable-elements.hd", "accept"],
  ["typing/valid/generic-result-permission.hd", "accept"],
  ["typing/valid/generic-argument-placeholder.hd", "accept"],
  ["typing/valid/readonly-callable-mutable-result.hd", "accept"],
  ["typing/valid/functions.hd", "accept"],
  ["typing/valid/readonly-map-mutable-values.hd", "accept"],
  ["typing/valid/row-polymorphic-callback.hd", "accept"],
  ["typing/valid/host-entry-requirement.hd", "accept"],
  ["typing/valid/console-error-entry.hd", "accept"],
  ["typing/valid/public-requirement-row.hd", "accept"],
  ["typing/valid/explicit-discard-and-void-result.hd", "accept"],
  ["typing/valid/optional-present-pattern.hd", "accept"],
  ["typing/valid/stored-suspension-driving.hd", "accept"],
  ["typing/valid/control-flow.hd", "accept"],
  ["typing/valid/local-functions.hd", "accept"],
  ["typing/valid/match-guards.hd", "accept"],
  ["typing/valid/contextual-variants.hd", "accept"],
  ["typing/valid/option-result.hd", "accept"],
  ["typing/valid/recursive-local-closure.hd", "accept"],
  ["typing/valid/defer-cleanup.hd", "accept"],
  ["typing/valid/resource-disposed-result.hd", "accept"],
  ["typing/valid/mutable-paths.hd", "accept"],
  ["typing/valid/embedded-trait-opt-in.hd", "accept"],
  ["typing/invalid/implicit-data-equality.hd", "reject:missing-partial-eq"],
  ["typing/invalid/implicit-data-ordering.hd", "reject:missing-partial-ord"],
  ["typing/invalid/bare-variant-pattern.hd", "reject:bare-variant-pattern"],
  ["typing/invalid/nonexhaustive-match.hd", "reject:nonexhaustive-match"],
  ["typing/invalid/top-level-return.hd", "reject:return-outside-function"],
  ["typing/invalid/recursive-closure-inferred-result.hd", "reject:recursive-closure-needs-result-type"],
  ["typing/invalid/missing-return-value.hd", "reject:missing-return-value"],
  ["typing/invalid/nonfinal-spread.hd", "reject:nonfinal-positional-spread"],
  ["typing/invalid/reassign-short-binding.hd", "reject:non-reassignable-binding"],
  ["typing/invalid/reassign-parameter.hd", "reject:non-reassignable-parameter-binding"],
  ["typing/invalid/discarded-suspension.hd", "reject:discarded-must-use-value"],
  ["typing/invalid/float-literal-range.hd", "reject:float-literal-range"],
  ["typing/invalid/heterogeneous-list.hd", "reject:no-common-type"],
  ["typing/invalid/invalid-map-key.hd", "reject:invalid-map-key"],
  ["typing/invalid/float-map-key.hd", "reject:invalid-map-key"],
  ["typing/invalid/nonnumeric-unary-plus.hd", "reject:nonnumeric-unary-plus"],
  ["typing/invalid/readonly-suspension-cancel.hd", "reject:mutable-receiver-required"],
  ["typing/invalid/guarded-match-not-exhaustive.hd", "reject:nonexhaustive-match"],
  ["typing/invalid/duplicate-data-pattern-field.hd", "reject:duplicate-data-pattern-field"],
  ["typing/invalid/nonexhaustive-bool-match.hd", "reject:nonexhaustive-match"],
  ["typing/invalid/overload.hd", "reject:duplicate-module-name"],
  ["typing/invalid/redeclare-core-type.hd", "reject:prelude-name-shadow"],
  ["typing/invalid/prelude-shadow.hd", "reject:prelude-name-shadow"],
  ["typing/invalid/function-equality.hd", "reject:unsupported-equality"],
  ["typing/invalid/unsaturated-enum-constructor.hd", "reject:unsaturated-enum-constructor"],
  ["typing/invalid/contextual-variant-without-type.hd", "reject:missing-contextual-enum-type"],
  ["typing/invalid/ambiguous-trait-method.hd", "reject:ambiguous-method"],
  ["typing/invalid/missing-trait-method.hd", "reject:missing-trait-method"],
  ["typing/invalid/trait-method-signature.hd", "reject:trait-method-signature"],
  ["typing/invalid/readonly-mut-field-method.hd", "reject:mutable-receiver-required"],
  ["typing/invalid/duplicate-inherent-member.hd", "reject:duplicate-inherent-member"],
  ["typing/invalid/embedded-mut-trait-bodyless.hd", "reject:promoted-mutable-requirement"],
  ["typing/invalid/duplicate-generic-embedded-name.hd", "reject:duplicate-embedded-field"],
  ["typing/invalid/duplicate-data-field.hd", "reject:duplicate-field"],
  ["typing/invalid/present-pattern-nonoptional.hd", "reject:optional-pattern-requires-optional"],
  ["typing/invalid/missing-requirement.hd", "reject:missing-requirement"],
  ["typing/invalid/bang-call-outside-suspension.hd", "reject:bang-call-outside-suspension"],
  ["typing/invalid/row-subtraction-unsound.hd", "reject:missing-requirement"],
  ["typing/valid/row-subtraction-entailment.hd", "accept"],
  ["typing/valid/provider-capturing-closure.hd", "accept"],
  ["typing/valid/distinct-requirement-keys.hd", "accept"],
  ["typing/valid/data-field-defaults.hd", "accept"],
  ["typing/invalid/missing-required-data-field.hd", "reject:missing-required-field"],
  ["typing/invalid/generic-requirement-key-collision.hd", "reject:generic-requirement-key-collision"],
  ["typing/warnings/requirement-subtract-absent.hd", "warn:requirement-subtract-absent"],
  ["typing/valid/string-semantics.hd", "accept"],
  ["typing/invalid/string-indexing.hd", "reject:unsupported-string-indexing"],
  ["typing/invalid/duplicate-trait-member.hd", "reject:duplicate-trait-member"],
  ["typing/invalid/mixed-numeric-power.hd", "reject:mixed-numeric-types"],
  ["typing/invalid/defer-suspends.hd", "reject:suspending-defer"],
  ["typing/invalid/top-level-defer.hd", "reject:defer-outside-cleanup-scope"],
  ["typing/invalid/impure-data-default.hd", "reject:impure-data-default"],
  ["typing/warnings/unreachable-code.hd", "warn:unreachable-code"],
  ["typing/warnings/unused-local-binding.hd", "warn:unused-local-binding"],
  ["typing/warnings/variant-binding-name-mismatch.hd", "warn:variant-binding-name-mismatch"],
  ["typing/valid/named-variant-bindings.hd", "accept"],
  ["typing/valid/shared-enum-defaults.hd", "accept"],
  ["typing/valid/enums.hd", "accept"],
  ["typing/valid/closure-identity.hd", "accept"],
  ["typing/valid/reference-generic-identity.hd", "accept"],
  ["typing/invalid/primitive-identity.hd", "reject:identity-requires-references"],
  ["typing/invalid/tuple-identity.hd", "reject:identity-requires-references"],
  ["typing/invalid/tuple-map-key.hd", "reject:invalid-map-key"],
  ["typing/invalid/unbounded-generic-identity.hd", "reject:identity-needs-reference-bound"],
  ["typing/invalid/readonly-generic-field-init.hd", "reject:mutable-upgrade"],
  ["typing/invalid/readonly-spread-mutable-field.hd", "reject:mutable-upgrade"],
  ["typing/invalid/copy-update-readonly-child.hd", "reject:readonly-edge"],
  ["typing/invalid/readonly-mutation.hd", "reject:readonly-root"],
  ["typing/invalid/readonly-function-result.hd", "reject:readonly-argument-to-mutable-parameter"],
  ["typing/invalid/mut-upgrade.hd", "reject:mutable-upgrade"],
  ["typing/invalid/missing-mutable-edge.hd", "reject:readonly-edge"],
  ["typing/invalid/readonly-mut-field-reassignment.hd", "reject:readonly-root"],
  ["typing/invalid/readonly-mut-field-mutation.hd", "reject:readonly-root"],
  ["typing/invalid/readonly-list-element-replacement.hd", "reject:readonly-root"],
  ["typing/invalid/readonly-map-entry-replacement.hd", "reject:readonly-root"],
  ["typing/invalid/partial-generic-arguments.hd", "reject:partial-generic-arguments"],
  ["typing/invalid/unresolved-generic-placeholder.hd", "reject:unresolved-generic-placeholder"],
  ["typing/invalid/nonhost-entry-requirement.hd", "reject:nonhost-entry-requirement"],
  ["typing/invalid/private-requirement-row.hd", "reject:private-type-leak"],
  ["typing/invalid/private-type-leak.hd", "reject:private-type-leak"],
  ["typing/invalid/nondisplay-entry-error.hd", "reject:entry-error-not-display"],
  ["typing/invalid/discarded-result.hd", "reject:discarded-must-use-value"],
  ["typing/invalid/impure-shared-enum-default.hd", "reject:impure-enum-default"],
  ["typing/invalid/shared-enum-default-order.hd", "reject:enum-default-order"],
] as const;

const RUNTIME_CASES = [
  ["runtime/panic/integer-divide-by-zero.hd", "panic:integer-division-by-zero"],
  ["runtime/panic/signed-min-division-overflow.hd", "panic:integer-overflow"],
  ["runtime/panic/nested-block-on.hd", "panic:suspension-nested-driver"],
  ["runtime/panic/second-suspension-drive.hd", "panic:suspension-invalid-state"],
  ["runtime/panic/test-block-on.hd", "panic:suspension-nested-driver"],
] as const;

const RUNTIME_VALID_CASES = [
  "runtime/valid/construction-provider-capture.hd",
  "runtime/valid/defer-order.hd",
  "runtime/valid/map-insertion-order.hd",
] as const;

test("curated chapter-01 and chapter-02 conformance cases match the manifest", async () => {
  const manifest = await readFile(resolve("spec/conformance/cases.tsv"), "utf8");
  for (const [path, expectation] of CASES) {
    assert.ok(manifest.includes(`${path}\tparse\t${expectation}\t`), `${path} must retain its manifest expectation`);
    const source = await readFile(resolve("spec/conformance", path), "utf8");
    const result = parse(source);
    if (expectation === "accept") {
      assert.deepEqual(result.diagnostics, [], path);
    } else {
      const code = expectation.slice("reject:".length);
      assert.ok(result.diagnostics.some((diagnostic) => diagnostic.code === code), `${path} should report ${code}`);
    }
  }
});

test("curated type conformance cases match the manifest", async () => {
  const manifest = await readFile(resolve("spec/conformance/cases.tsv"), "utf8");
  for (const [path, expectation] of TYPE_CASES) {
    assert.ok(manifest.includes(`${path}\ttype\t${expectation}\t`), `${path} must retain its manifest expectation`);
    const source = await readFile(resolve("spec/conformance", path), "utf8");
    const result = analyze(source);
    if (expectation === "accept") {
      assert.deepEqual(result.diagnostics.filter((diagnostic) => diagnostic.severity !== "warning"), [], path);
      assert.ok(result.hir, `${path} should remain analyzable`);
    } else if (expectation.startsWith("reject:")) {
      const code = expectation.slice("reject:".length);
      assert.ok(result.diagnostics.some((diagnostic) => diagnostic.code === code), `${path} should report ${code}`);
    } else {
      const code = expectation.slice("warn:".length);
      assert.ok(result.hir, `${path} should remain analyzable with warnings`);
      assert.ok(result.diagnostics.some((diagnostic) => diagnostic.code === code && diagnostic.severity === "warning"), `${path} should warn ${code}`);
    }
  }
});

test("curated runtime panic cases match the manifest and trap in Wasm", async () => {
  const manifest = await readFile(resolve("spec/conformance/cases.tsv"), "utf8");
  for (const [path, expectation] of RUNTIME_CASES) {
    assert.ok(manifest.includes(`${path}\truntime\t${expectation}\t`), `${path} must retain its manifest expectation`);
    const source = await readFile(resolve("spec/conformance", path), "utf8");
    const { instance } = await instantiate(source);
    const entry = instance.exports.main ?? instance.exports.__hd_test_0;
    assert.equal(typeof entry, "function", `${path} must expose a runnable main or test driver`);
    assert.throws(
      () => (entry as CallableFunction)(),
      WebAssembly.RuntimeError,
      `${path} should trap`,
    );
  }
});

test("curated runtime-valid cases compile and complete in Wasm", async () => {
  const manifest = await readFile(resolve("spec/conformance/cases.tsv"), "utf8");
  for (const path of RUNTIME_VALID_CASES) {
    assert.ok(manifest.includes(`${path}\truntime\taccept\t`), `${path} must retain its manifest expectation`);
    const source = await readFile(resolve("spec/conformance", path), "utf8");
    const { instance } = await instantiate(source);
    const entry = instance.exports.main ?? instance.exports.__hd_test_0;
    assert.equal(typeof entry, "function", `${path} must expose a runnable main or test driver`);
    assert.doesNotThrow(() => (entry as CallableFunction)(), path);
  }
});
