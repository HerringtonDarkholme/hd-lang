# All Findings

Generated from `audit/findings/` on 2026-09-25. "Duplicate of" comes from each file's `Duplicates:` line.

| ID | Severity | Area | Title | Duplicate of |
| -- | -------- | ---- | ----- | ------------ |
| [F-101](../findings/F-101-defaults-reject-top-level-reads.md) | minor | correctness | Parameter and data-field defaults reject any read of a top-level binding |  |
| [F-150](../findings/F-150-reject-cases-ignore-extra-errors.md) | minor | test-integrity | Reject cases pass even when unrelated errors are also reported |  |
| [F-155](../findings/F-155-panic-line-unchecked-and-unreported.md) | minor | runtime | Runtime panics carry no source location, so panic marker lines are never checked |  |
| [F-160](../findings/F-160-f64-relational-operators-crash-compiler.md) | major | correctness | Any f64 `<`, `<=`, `>`, or `>=` crashes code generation |  |
| [F-161](../findings/F-161-stack-exhaustion-is-a-host-crash.md) | minor | runtime | Unbounded recursion crashes the host instead of panicking with `stack-exhausted` |  |
| [F-162](../findings/F-162-cli-entry-misuse-crashes.md) | minor | runtime | `hd run` and `hd test` crash with stack traces on entry-shape errors |  |
| [F-163](../findings/F-163-list-literal-operand-type-mismatch.md) | major | correctness | Comparing a readonly list binding with a list literal is rejected |  |
| [F-201](../findings/F-201-fixtures-depend-on-undeclared-requirement-keys.md) | major | correctness | 29 fixtures use undeclared requirement keys that the compiler wrongly accepts |  |
| [F-205](../findings/F-205-fixture-codes-missing-from-spec.md) | major | spec | Half of test/fixtures reject markers use diagnostic codes that spec/README.md does not define |  |
| [F-250](../findings/F-250-deferred-features-lack-structured-diagnostics.md) | major | coverage | Deferred features are rejected with generic or wrong diagnostics, not structured unsupported diagnostics | F-405 |
| [F-251](../findings/F-251-plain-closure-mutates-captures.md) | major | correctness | A plain `fn` closure may mutate a captured mutable list; the program is accepted and runs | F-351 |
| [F-252](../findings/F-252-parser-rejects-core-grammar.md) | major | correctness | The parser rejects core chapter-02 forms that the reference parser accepts |  |
| [F-253](../findings/F-253-sized-numeric-types-unsupported.md) | minor | coverage | Sized numeric types (i8-i64, u8-u64, f32) are unimplemented and not listed as deferred |  |
| [F-254](../findings/F-254-type-and-local-declarations-unsupported.md) | minor | coverage | `type` declarations and local type/impl declarations are rejected as syntax errors |  |
| [F-255](../findings/F-255-prelude-surface-gaps.md) | minor | coverage | Prelude names `Any`, `Eq`, `Hash`, and `Hasher` are unknown |  |
| [F-256](../findings/F-256-row-kinded-arguments-unparsed.md) | minor | coverage | Row-kinded generic data arguments such as `Job[$()]` fail to parse |  |
| [F-257](../findings/F-257-mut-fn-closures-unsupported.md) | minor | coverage | `mut fn` closures are unsupported, and assigning a captured `let` reports `unknown-name` | F-352, F-357 |
| [F-258](../findings/F-258-least-common-type-code.md) | minor | correctness | Mixed-permission list literal reports `no-common-type` instead of `no-least-common-type` |  |
| [F-259](../findings/F-259-disposed-file-profile-missing.md) | minor | test-integrity | The `disposed-file` runtime profile named in spec/conformance/README.md does not exist |  |
| [F-261](../findings/F-261-identity-rejects-mut-operands.md) | major | correctness | `is` rejects operands with `mut` access | F-353 |
| [F-264](../findings/F-264-replay-code-identity-includes-comments.md) | minor | runtime | A comment inside an executed function invalidates a recorded replay | F-611 |
| [F-265](../findings/F-265-cli-uncaught-exceptions.md) | minor | architecture | Replay rejection and several CLI errors exit through uncaught JavaScript exceptions | F-162 |
| [F-266](../findings/F-266-order-fixtures-cannot-observe-order.md) | minor | test-integrity | Evaluation-order fixtures return the same value under any evaluation order | F-210 |
| [F-267](../findings/F-267-later-parameter-default-code.md) | minor | spec | A default that reads a later parameter is reported as impure | F-356 |
| [F-268](../findings/F-268-typing-fixture-panics-when-run.md) | note | test-integrity | `typing/valid/explicit-generic-method.hd` panics when executed |  |
| [F-304](../findings/F-304-result-constructor-payload-unchecked.md) | major | correctness | `hd check` does not check `Ok`/`Err` payloads against the expected Result type |  |
| [F-305](../findings/F-305-discard-void-propagation-invalid-wat.md) | major | correctness | `_ := f()?` on `Result[void, E]` emits WAT that Binaryen cannot parse | F-559 |
| [F-306](../findings/F-306-run-without-main-crashes.md) | minor | correctness | `hd run` and `hd test` crash with a stack trace when there is no entry point |  |
| [F-307](../findings/F-307-result-main-not-runnable.md) | major | correctness | A `main` returning `Result[void, E]` passes check but cannot run |  |
| [F-308](../findings/F-308-private-main-requirement-crash.md) | minor | correctness | A non-`pub` `main` with a non-host requirement passes check and crashes `run` |  |
| [F-309](../findings/F-309-same-line-if-else-rejected.md) | major | correctness | hd rejects same-line `if c: a else: b`, a form chapter 01 uses as an example | F-252 |
| [F-310](../findings/F-310-colon-line-attaches-trailing-block.md) | minor | correctness | A line starting with `:` is parsed as a trailing block on the previous statement |  |
| [F-311](../findings/F-311-continuation-suite-indentation.md) | minor | correctness | hd accepts a bracketed nested suite whose body is not indented past its header |  |
| [F-312](../findings/F-312-decorators-misreported.md) | minor | correctness | Every decorator is rejected as `decorator-not-top-level`, even at top level | F-405 and F-250 |
| [F-314](../findings/F-314-grammar-layout-mismatch.md) | note | spec | The EBNF derives same-line suite forms that layout processing cannot produce |  |
| [F-315](../findings/F-315-grammar-valid-forms-generic-codes.md) | note | correctness | Grammar-valid forms are rejected at parse with generic codes, not unsupported diagnostics |  |
| [F-316](../findings/F-316-pub-inherent-method-false-doc-comment.md) | minor | correctness | A `pub fn` inside an inherent `impl` is rejected with `doc-comment-without-target` |  |
| [F-350](../findings/F-350-primitives-miss-display-bound-and-trait-values.md) | major | correctness | Primitives do not satisfy `T: Display` and cannot become `Display` trait values |  |
| [F-351](../findings/F-351-plain-closure-mutates-captures.md) | major | correctness | A plain `fn` closure may mutate through a captured `mut` value |  |
| [F-352](../findings/F-352-mut-fn-closure-literal-unparsed.md) | minor | coverage | `mut fn` closure literals do not parse |  |
| [F-353](../findings/F-353-identity-rejects-mut-operands.md) | minor | correctness | `is` rejects operands whose static type is `mut T` |  |
| [F-354](../findings/F-354-map-value-covariance-missing.md) | minor | correctness | Readonly `map[K, V]` is not covariant in `V` |  |
| [F-355](../findings/F-355-trim-uses-host-whitespace-set.md) | minor | correctness | `string.trim()` removes U+FEFF and keeps U+0085 |  |
| [F-356](../findings/F-356-later-parameter-default-code.md) | note | spec | A default that names a later parameter reports a code outside the spec inventory |  |
| [F-357](../findings/F-357-closure-let-assignment-unknown-name.md) | note | correctness | Assigning captured `let` storage in a plain closure reports `unknown-name` |  |
| [F-400](../findings/F-400-host-strings-drop-leading-bom.md) | major | correctness | Strings that start with U+FEFF lose it at the host boundary |  |
| [F-401](../findings/F-401-replay-accepts-changed-callee.md) | minor | runtime | Replay accepts a changed body in an executed non-suspending function |  |
| [F-402](../findings/F-402-cli-provider-configuration-constant.md) | minor | runtime | `hd replay` accepts a history recorded under a different runtime profile |  |
| [F-403](../findings/F-403-test-blocks-share-one-instance.md) | major | runtime | `hd test` runs `main` and every test block in one shared instance |  |
| [F-404](../findings/F-404-failing-runs-leave-no-history.md) | minor | runtime | `hd record` writes no history when the run panics or never finishes |  |
| [F-500](../findings/F-500-primitives-miss-comparison-bounds.md) | major | correctness | Primitives do not satisfy `T: PartialEq` or `T: PartialOrd` |  |
| [F-501](../findings/F-501-map-is-linear-association-list.md) | major | runtime | `map[K, V]` is an unhashed association list with O(n) get and insert |  |
| [F-502](../findings/F-502-bounded-calls-allocate-dictionaries.md) | major | runtime | Bounded generic calls rebuild dictionaries and allocate a trait value per method call |  |
| [F-503](../findings/F-503-interpolation-pairwise-concat.md) | minor | runtime | String interpolation lowers to pairwise concatenation and re-allocates literals |  |
| [F-504](../findings/F-504-optional-carrier-allocations.md) | minor | runtime | The optional carrier allocates for every `nil`, every loop step, and every map get |  |
| [F-505](../findings/F-505-concrete-lists-rebox-elements.md) | note | architecture | `list[i32]` stores boxes even in concrete code, so every store allocates |  |
| [F-506](../findings/F-506-frames-retain-dead-locals.md) | note | runtime | Suspension frames keep every local, including dead temporaries and heap references |  |
| [F-550](../findings/F-550-row-adapter-copies-pack-per-lookup.md) | major | runtime | row-generic callback adapter copies the whole provider pack once per lookup |  |
| [F-551](../findings/F-551-provider-concat-copies-left-pack.md) | minor | runtime | `$hd.provider_concat(left, null)` copies the left pack instead of sharing it |  |
| [F-552](../findings/F-552-suspension-frame-code-size-superlinear.md) | major | architecture | suspension lowering code size grows super-linearly with bang-call sites |  |
| [F-553](../findings/F-553-linear-state-dispatch.md) | minor | runtime | resume and CFG dispatch use linear `if` chains instead of `br_table` |  |
| [F-554](../findings/F-554-immediate-fn-bang-costs-13x.md) | minor | runtime | a `fn!` call that completes immediately costs about 13x a plain call, and -O2 does not help |  |
| [F-555](../findings/F-555-entry-drive-spins-on-pending.md) | minor | runtime | the `main` entry export busy-polls forever when a host provider stays pending |  |
| [F-556](../findings/F-556-driver-guard-stuck-after-panic.md) | note | runtime | after a panic inside a drive, later drives report `suspension-competing-driver` |  |
| [F-557](../findings/F-557-runtime-library-always-inlined.md) | minor | architecture | every module embeds the full runtime library, used or not |  |
| [F-558](../findings/F-558-string-boundary-byte-per-call.md) | minor | runtime | strings cross the host boundary one byte per import call |  |
| [F-559](../findings/F-559-checked-programs-crash-emission.md) | minor | correctness | two type-checked fixtures crash Wasm emission with an internal error |  |
| [F-560](../findings/F-560-cli-always-loads-binaryen.md) | minor | architecture | every CLI command loads binaryen.js, which dominates the edit loop |  |
| [F-600](../findings/F-600-explain-requirements-skips-hir-kinds.md) | minor | correctness | explain-requirements skips 23 HIR kinds and reports real uses as "declared" |  |
| [F-601](../findings/F-601-explain-requirements-closure-paths.md) | minor | correctness | explain-requirements shows closure-routed requirements as a direct `$.use` |  |
| [F-602](../findings/F-602-module-init-check-exponential.md) | major | architecture | Module-initialization check takes exponential time on call DAGs |  |
| [F-603](../findings/F-603-module-init-misses-trait-dispatch.md) | major | correctness | Initialization check misses generic-bound and dynamic trait dispatch |  |
| [F-604](../findings/F-604-nested-closure-inference-exponential.md) | minor | architecture | Checking nested unannotated closures doubles in time per nesting level |  |
| [F-605](../findings/F-605-error-recovery-limits.md) | minor | architecture | Error recovery stops at the first parse error and the first error per function |  |
| [F-606](../findings/F-606-interpolation-diagnostic-location.md) | minor | correctness | Diagnostics inside `${...}` interpolation are reported at 1:1 |  |
| [F-607](../findings/F-607-hir-types-are-strings.md) | note | architecture | HIR types are strings that the emitter re-parses, with nominal types keyed by bare name |  |
| [F-608](../findings/F-608-order-dependent-numbering.md) | note | architecture | Program-wide numbering makes one inserted declaration rewrite half the WAT |  |
| [F-609](../findings/F-609-lowering-split-and-triplicated.md) | note | architecture | Desugaring is split between checker and emitter, and control flow is lowered three times |  |
| [F-610](../findings/F-610-dispatch-chains-defeat-exhaustiveness.md) | note | architecture | Expression dispatch is split into `??` chains, so missing kinds fail only at runtime |  |
| [F-611](../findings/F-611-replay-identity-formatting.md) | note | runtime | Replay code identity changes on whitespace or comment edits inside a function |  |
| [F-612](../findings/F-612-explain-requirements-path-explosion.md) | note | architecture | explain-requirements prints every call path, so output grows exponentially |  |
| [F-700](../findings/F-700-closure-propagation-invalid-wat.md) | major | correctness | Optional `?` inside a closure emits WAT that Binaryen cannot parse |  |
| [F-701](../findings/F-701-generic-function-value-rejected.md) | minor | correctness | A generic function cannot be used as a value, even when its type arguments are known |  |
| [F-702](../findings/F-702-pub-path-only-use-accepted.md) | minor | correctness | `pub` on a path-only `use` is accepted |  |
| [F-703](../findings/F-703-pub-top-level-binding-accepted.md) | minor | correctness | `pub` on a top-level binding is accepted |  |
| [F-704](../findings/F-704-string-replace-unimplemented.md) | minor | coverage | `string.replace` is not implemented |  |
| [F-705](../findings/F-705-declared-variance-deferred.md) | note | coverage | Declared variance is deferred, so covariant data cannot be written |  |
| [F-706](../findings/F-706-raw-string-escaped-quote.md) | minor | correctness | A backslash before a quote in a raw string is a lexer error |  |
| [F-707](../findings/F-707-same-line-suite-comma-accepted.md) | minor | correctness | A comma at depth zero does not end a same-line suite |  |
