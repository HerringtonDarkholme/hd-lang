# All Findings

Generated from `audit/findings/` on 2026-09-27. "Merged duplicates" lists the IDs folded into each finding (its `Duplicates:` line). "Known failures" counts the rows tagged with the ID in `test/portable/KNOWN_FAILURES.tsv`.

| ID | Severity | Area | Title | Merged duplicates | Known failures |
| -- | -------- | ---- | ----- | ----------------- | -------------- |
| [F-150](../findings/F-150-entry-cases-private-type-leak.md) | minor | test-integrity | Entry-point cases use a private declaration in `pub fn main`, so they also report `private-type-leak` |  | 2 |
| [F-155](../findings/F-155-panic-line-unchecked-and-unreported.md) | minor | runtime | Runtime panics carry no source location, so panic marker lines are never checked |  |  |
| [F-161](../findings/F-161-stack-exhaustion-is-a-host-crash.md) | minor | runtime | Unbounded recursion crashes the host instead of panicking with `stack-exhausted` |  |  |
| [F-163](../findings/F-163-list-literal-operand-type-mismatch.md) | major | correctness | Comparing a readonly list binding with a list literal is rejected |  |  |
| [F-201](../findings/F-201-undeclared-requirement-key-accepted.md) | minor | correctness | The compiler accepts an undeclared requirement key on a non-entry function |  |  |
| [F-250](../findings/F-250-deferred-features-lack-structured-diagnostics.md) | major | coverage | Deferred features are rejected with generic or wrong diagnostics, not structured unsupported diagnostics | F-312 | 44 |
| [F-253](../findings/F-253-sized-numeric-types-unsupported.md) | minor | coverage | Sized numeric types (i8-i64, u8-u64, f32) are unimplemented and not listed as deferred |  | 15 |
| [F-255](../findings/F-255-prelude-surface-gaps.md) | minor | coverage | Prelude names `Hash` and `Hasher` are unknown |  | 2 |
| [F-259](../findings/F-259-disposed-file-profile-missing.md) | minor | test-integrity | The `disposed-file` runtime profile named in spec/conformance/README.md does not exist |  | 1 |
| [F-265](../findings/F-265-cli-uncaught-exceptions.md) | minor | architecture | Replay rejection and several CLI errors exit through uncaught JavaScript exceptions | F-162, F-306 |  |
| [F-310](../findings/F-310-colon-line-attaches-trailing-block.md) | minor | correctness | A line starting with `:` is parsed as a trailing block on the previous statement |  |  |
| [F-354](../findings/F-354-map-value-covariance-missing.md) | minor | correctness | Readonly `Map[K, V]` is not covariant in `V` |  |  |
| [F-401](../findings/F-401-replay-accepts-changed-callee.md) | minor | runtime | Replay code identity is a per-function source hash, not the decided whole-module semantic hash | F-611, F-264, F-611 |  |
| [F-402](../findings/F-402-cli-provider-configuration-constant.md) | minor | runtime | `hd replay` accepts a history recorded under a different runtime profile |  |  |
| [F-403](../findings/F-403-test-blocks-share-one-instance.md) | major | runtime | `hd test` runs `main` and every test block in one shared instance |  |  |
| [F-404](../findings/F-404-failing-runs-leave-no-history.md) | minor | runtime | `hd record` writes no history when the run panics or never finishes |  |  |
| [F-501](../findings/F-501-map-is-linear-association-list.md) | major | runtime | `Map[K, V]` is an unhashed association list with O(n) get and insert |  |  |
| [F-502](../findings/F-502-bounded-calls-allocate-dictionaries.md) | major | runtime | Bounded generic calls rebuild dictionaries and allocate a trait value per method call |  |  |
| [F-503](../findings/F-503-interpolation-pairwise-concat.md) | minor | runtime | String interpolation lowers to pairwise concatenation and re-allocates literals |  |  |
| [F-504](../findings/F-504-optional-carrier-allocations.md) | minor | runtime | The optional carrier allocates for every `.None`, every loop step, and every map get |  |  |
| [F-505](../findings/F-505-concrete-lists-rebox-elements.md) | note | architecture | `List[i32]` stores boxes even in concrete code, so every store allocates |  |  |
| [F-506](../findings/F-506-frames-retain-dead-locals.md) | note | runtime | Suspension frames keep every local, including dead temporaries and heap references |  |  |
| [F-550](../findings/F-550-row-adapter-copies-pack-per-lookup.md) | major | runtime | row-generic callback adapter copies the whole provider pack once per lookup |  |  |
| [F-551](../findings/F-551-provider-concat-copies-left-pack.md) | minor | runtime | `$hd.provider_concat(left, null)` copies the left pack instead of sharing it |  |  |
| [F-552](../findings/F-552-suspension-frame-code-size-superlinear.md) | major | architecture | suspension lowering code size grows super-linearly with bang-call sites |  |  |
| [F-553](../findings/F-553-linear-state-dispatch.md) | minor | runtime | resume and CFG dispatch use linear `if` chains instead of `br_table` |  |  |
| [F-554](../findings/F-554-immediate-fn-bang-costs-13x.md) | minor | runtime | a `fn!` call that completes immediately costs about 13x a plain call, and -O2 does not help |  |  |
| [F-555](../findings/F-555-entry-drive-spins-on-pending.md) | minor | runtime | the `main` entry export busy-polls forever when a host provider stays pending |  |  |
| [F-557](../findings/F-557-runtime-library-always-inlined.md) | minor | architecture | every module embeds the full runtime library, used or not |  |  |
| [F-558](../findings/F-558-string-boundary-byte-per-call.md) | minor | runtime | strings cross the host boundary one byte per import call |  |  |
| [F-559](../findings/F-559-checked-programs-crash-emission.md) | minor | correctness | A type-checked multi-provider use crashes Wasm emission with an internal error |  |  |
| [F-560](../findings/F-560-cli-always-loads-binaryen.md) | note | architecture | every CLI command loads binaryen.js, even commands that emit no Wasm |  |  |
| [F-600](../findings/F-600-explain-requirements-skips-hir-kinds.md) | minor | correctness | explain-requirements skips 23 HIR kinds and reports real uses as "declared" |  |  |
| [F-601](../findings/F-601-explain-requirements-closure-paths.md) | minor | correctness | explain-requirements shows closure-routed requirements as a direct `$.use` |  |  |
| [F-604](../findings/F-604-nested-closure-inference-exponential.md) | minor | architecture | Checking nested unannotated closures doubles in time per nesting level |  |  |
| [F-605](../findings/F-605-error-recovery-limits.md) | minor | architecture | Error recovery stops at the first parse error and the first error per function |  |  |
| [F-606](../findings/F-606-interpolation-diagnostic-location.md) | minor | correctness | Diagnostics inside `${...}` interpolation are reported at 1:1 |  |  |
| [F-607](../findings/F-607-hir-types-are-strings.md) | note | architecture | HIR types are strings that the emitter re-parses, with nominal types keyed by bare name |  |  |
| [F-608](../findings/F-608-order-dependent-numbering.md) | note | architecture | Program-wide numbering makes one inserted declaration rewrite half the WAT |  |  |
| [F-609](../findings/F-609-lowering-split-and-triplicated.md) | note | architecture | Desugaring is split between checker and emitter, and control flow is lowered three times |  |  |
| [F-610](../findings/F-610-dispatch-chains-defeat-exhaustiveness.md) | note | architecture | Expression dispatch is split into `??` chains, so missing kinds fail only at runtime |  |  |
| [F-612](../findings/F-612-explain-requirements-path-explosion.md) | note | architecture | explain-requirements prints every call path, so output grows exponentially |  |  |
