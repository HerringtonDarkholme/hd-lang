# Lowering, Suspension, And Host Boundary: First Pass

> Historical baseline review. Fixed findings remain here only as original evidence; use [REPORT.md](REPORT.md) and [findings.tsv](findings.tsv) for remaining work.

Status: preliminary audit evidence; nothing here establishes accepted language behavior or an owner decision. This is a source inspection, not a completed conformance audit.

Baseline: `823f346878028aad4a4c9351593217f04445bd4c`. Inspection and this report belong to the isolated audit worktree. No compiler, specification, fixture, or implementation test was changed. No checks, compilation probes, or benchmarks were executed.

The sections under review are [Return](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/06-control-flow.md#return), [Deferred Cleanup](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/06-control-flow.md#deferred-cleanup), [Captures](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/07-functions.md#captures), the call rules in [Expressions](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/05-expressions.md#L934), and [Requirements And Suspension](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/11-requirements-and-suspension.md). The remaining decision-record reconciliation includes the observability and runtime gaps in [OPEN_ISSUES](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/future-work/OPEN_ISSUES.md#L140). Existing implementation claims are cross-checked against [KNOWN_ISSUES](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/KNOWN_ISSUES.md#L68).

## Architectural Assessment

This area has a plausible general mechanism: typed expression nodes, shared heap cells for mutable captures, explicit suspension frames, and a control-flow graph for nested suspension. However, the backend preserves three implementations of language control flow. Which implementation runs depends on the syntactic placement of suspension drives.

The actual inspected path is:

1. [Program checking](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/program-lower.ts#L157) constructs checked HIR through `FunctionChecker`.
2. [Capture conversion](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/program-lower.ts#L214) rewrites captured mutable locals into heap-cell operations.
3. [CFG selection](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/emitter.ts#L1173) selects suspending functions using `needsSuspensionCfg`.
4. [Suspension emission](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/emitter.ts#L175) chooses CFG, linear continuation, or a whole-body wrapper.
5. [Ordinary expression emission](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/function-body.ts#L163) still handles high-level control expressions and much semantic desugaring.
6. [Host instantiation](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/compiler.ts#L370) connects emitted imports to callbacks, recording, and replay.

This is a correctness concern because the linear path delegates some expressions back to ordinary function emission. That emitter's `return` means a native Wasm return, whereas suspension completion means storing a language result and returning the poll protocol's readiness value.

## L1. Linear Suspension Emission Can Confuse Language Return With Poll Return

Priority: high. Classification: source-derived defect candidate and confirmed architectural risk. Confidence: high in the call-path analysis; runtime reproduction remains pending.

[The selector](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/suspension.ts#L138) looks for nested suspension drives or multiple drives within an expression. A branch containing a language `return`, but no drive, does not require CFG under this predicate. A different top-level statement containing a direct drive selects the linear suspension path.

[The linear continuation](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/emitter.ts#L1005) intercepts only top-level `return` statements. Other statements reach `emitStatement`, and nested `if` bodies use [ordinary block emission](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/function-body.ts#L904). Its [return case](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/function-body.ts#L128) emits a native Wasm return carrying the language value.

Those instructions are embedded in [the poll function](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/emitter.ts#L853), whose return type is `i32`. The proper [completion routine](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/emitter.ts#L1109) instead stores the language result, runs registered cleanup, marks the frame complete, and returns readiness `1`.

Consequences inferred from this path:

- An `i32` language return can escape as a readiness value without storing the result or completing the frame.
- A non-`i32` language return can make the generated Wasm invalid.
- Top-level defers tracked by the linear continuation are absent from the ordinary emitter's cleanup stack on this nested exit.
- Postfix propagation in a separate non-driving expression has the same risk: [ordinary propagation](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/function-body.ts#L302) also emits native return.

The applicable rules are `flow.return.value`, `flow.defer.run`, `flow.defer.operands`, and the suspension protocol's completion and one-shot checks. Their intent is coherent here: nested return completes the language function, regardless of drive placement. This inspection has not established a conflicting owner decision.

The existing [propagation fixture](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/runtime/valid/suspending-result-propagation.hd) applies propagation directly to a bang call. That expression chooses CFG, so it does not exercise the separate non-driving propagation case. The [sequential fixture](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/runtime/valid/sequential-suspending-calls.hd) has no nested early return.

The broader duplication is already recorded as F-609. Ordinary [cleanup emission](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/function-body.ts#L1453), CFG [cleanup lowering](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/suspension.ts#L243), and linear [cleanup accumulation](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/emitter.ts#L1012) independently implement scope exits. The candidate above demonstrates why this is more than a code-size concern.

There is no exhaustive dispatch guarantee across these paths. [Expression emission](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/function-body.ts#L163) chains four partial handlers with `??`, and [CFG lowering](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/suspension.ts#L385) has a partial aggregate handler followed by [a final exception](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/suspension.ts#L733). F-610 records this risk. Adding a HIR kind may leave one path uncovered without a TypeScript error.

[Compilation](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/compiler.ts#L309) directly calls `emitWat` after checking; those lowering exceptions can escape its diagnostic boundary. F-265 records related stack-trace behavior. Invariant exceptions are legitimate for compiler bugs, but exhaustive visitors and validated pass outputs should establish coverage before a checked program reaches emission.

## L2. Entry Execution Uses A Busy-Poll Protocol Instead Of The Specified Waker Protocol

Priority: high. Classification: explicit compiler/runtime conformance gap. Confidence: high from emitted instructions; actual CPU measurements were not repeated.

The whole-body, CFG, and linear drivers all emit an unconditional loop around polling: [whole-body](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/emitter.ts#L263), [CFG](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/emitter.ts#L489), and [linear](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/emitter.ts#L881). The [stored-suspension driver](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/stored-suspension.ts#L160) follows the same pattern. The [entry wrapper](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/emitter.ts#L919) invokes the generated driver.

The [entry-driver rules](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/11-requirements-and-suspension.md#L1067) require host control to resume after `Pending`, followed by polling only on wake. Rule `req.entry.busy-poll` explicitly rejects continued polling without returning to the host. [PollContext and Wakers](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/11-requirements-and-suspension.md#L1110) require retained, coalesced wakes.

[InstantiateOptions](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/compiler.ts#L147) supplies poll-decision callbacks, but no waker delivery API. Deterministic callbacks that return pending a fixed number of times model progress caused by polling itself. They cannot model a host callback that needs the JavaScript event loop before readiness can change.

This is documented as F-555. The development `__hd_start`/`__hd_poll` exports are a useful lower-level host boundary; they return after a poll. They do not establish that the public entry driver implements the specified scheduler.

The naive correct implementation needs a single poll protocol and a host-side executor that resumes on wake. An unconditional spin loop can remain appropriate only where independently specified, such as a synchronous driver with appropriate host support. The entry and `block_on` contracts require separate review.

## L3. Shared Capture Cells Are General In Concept, Fragile In Representation

Priority: medium. Classification: architectural correctness risk, not a reproduced failure. Confidence: high in the implementation description.

[Capture conversion](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/captured-cells.ts#L19) identifies storage by `HirLocal` object identity. It synthesizes a type string with the `cell:` prefix and rewrites bindings, assignments, reads, and closure capture lists. Heap cells directly implement `fn.capture.storage.shared` and `fn.capture.storage.lifetime`; using cells is a sound naive choice.

The fragility is [the recursive rewrite](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/checker/captured-cells.ts#L36): it traverses arbitrary `unknown` objects through `Object.entries`, selects nodes by string `kind`, and casts the result back to HIR. Neither exhaustiveness nor field preservation is checked by TypeScript. Shared capture lookup also assumes `closures[closureIndex]` retrieves the closure represented by that index.

Cloning a local before conversion can silently break sharing because identity, rather than a stable local ID, controls substitution. Adding a HIR field containing a `Map`, or another non-plain object, needs special traversal support; the current generic traversal would reconstruct it as a plain object. This is a future extension hazard, not evidence that current capture nodes contain such a field.

The [emitter](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/function-body.ts#L1) imports `cellInner` from the checker. The representation crosses phases without an explicit post-conversion HIR contract. A typed closure-conversion pass, stable storage IDs, and structural internal cell types would make the existing simple mechanism easier to verify.

Existing probes include [shared let capture](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/runtime/valid/closure-captured-let-shared.hd), [nested captures](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/runtime/valid/nested-closure-captures.hd), [per-iteration capture](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/runtime/valid/closure-captures-per-iteration.hd), and [suspending closure capture](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/runtime/valid/suspending-closure-captures.hd). Composition of all four remains unreviewed.

## L4. Host ABI Behavior Is Duplicated And Only Partially Validated

Priority: medium. Classification: host-contract risk and specification coverage gap. Confidence: high in coercion behavior; severity depends on the host input contract.

[The emitter](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/host-providers.ts#L22) and [host bridge](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/compiler.ts#L174) separately recognize `Result` by bare type name and maintain scalar boundary sets. The emitter determines representations, and JavaScript separately determines acceptable payloads. Their synchronization is an informal invariant.

[Scalar canonicalization](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/compiler.ts#L231) validates only broad JavaScript categories. Boolean values normalize every nonzero number to true; integer-like values use `value | 0`, accepting fractional, non-finite, and out-of-range numbers. `char` follows this path too. These are proven source behaviors, not executed tests.

That coercion may be an intended external ABI convention for `i32`. It must be stated as such. Otherwise a malformed callback can introduce values that contradict the typed language model. The reviewed language rules do not settle whether malformed host values are rejected, normalized, or outside the host contract.

[Result emission](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/emitter/host-providers.ts#L141) represents unsupported payload types as null; [the bridge](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/compiler.ts#L191) rejects host attempts to build them. Comments justify this for payload-free opaque errors such as `ConsoleError`, but this is not a general representation for arbitrary result error types. The admitted capability signatures and their rejection boundary still need inspection.

Owner question after that inspection: which malformed host scalar values are admissible ABI inputs, and what happens to inadmissible values? This is a contract question; it does not justify changing language rules to match current coercions.

Positive evidence: replay strings use [fatal UTF-8 decoding](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/compiler.ts#L265), and f64 replay stores bit patterns, preserving signed zero. Byte-by-byte copies are inefficient but can remain semantically correct.

## L5. Replay Identity Remains An Experiment, With A Spec Traceability Gap

Priority: medium. Classification: confirmed mismatch with recorded implementation intent, plus incomplete specification/decision traceability. Confidence: high in mechanism; durable replay semantics remain outside this pass's coverage.

[Instantiation](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/compiler.ts#L379) hashes each declaration's source substring. [Provider sites](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/compiler.ts#L453) embed function-relative source byte offsets. Replay checks those IDs and [throws at exhausted history](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/src/compiler.ts#L506).

A formatting change alters identity. Changing a callee without changing its caller can leave the caller's recorded identity unchanged. Whether the changed callee is independently checked depends on which recorded events it produces. F-401 records the underlying identity problem.

[OPEN_ISSUES](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/future-work/OPEN_ISSUES.md#L276) explicitly says these experiments predate decided Replay Rules, including program identity, byte-offset-free sites, and continued execution after existing history. [The observability entry](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/future-work/OPEN_ISSUES.md#L146) cites execution ID plus event index instead.

[The normative determinism section](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/11-requirements-and-suspension.md#L1390) uses code identity but does not define its construction or equivalence relation. [Closure capture rules](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/lang/07-functions.md#L767) defer serializable capture and identity to runtime design. Accordingly, compiler behavior alone cannot establish that formatting must be identity-preserving under the current language specification.

The decision records cited as Replay Rules must be located and reconciled before prescribing a replacement. The report should distinguish language determinism, durable runtime decisions, and prototype replay tests. Tests accepting unrelated declaration edits establish one particular experiment's behavior; they do not validate program identity.

## Review Coverage And Next Validation

Inspected: program-level checking/lowering, the entire capture-cell conversion, suspension-plan selection and major builder paths, ordinary return/propagation/cleanup emission, linear continuation completion, suspension driver construction, host-provider scalar/result encoding, replay identity, and representative runtime helpers.

Reviewed-file inventory: `src/checker/program-lower.ts` and `src/checker/captured-cells.ts` were read completely. Relevant sections were read in `src/checker/checker.ts`, `src/checker/statements.ts`, `src/hir.ts`, `src/emitter/context.ts`, `src/emitter/shared.ts`, `src/emitter/function-body.ts`, `src/emitter/suspension.ts`, `src/emitter/emitter.ts`, `src/emitter/host-providers.ts`, `src/emitter/stored-suspension.ts`, `src/compiler.ts`, and `src/emitter/runtime/runtime.wat`. `src/emitter/runtime/index.ts` was read completely. `src/emitter/runtime/boundary.wat` was searched and its boundary-copy structure inspected; `map.wat` remains unreviewed.

Not completely reviewed: every expression handler, complete CFG loops/comprehensions/match lowering, generic dictionary and provider lifetime behavior, stored-suspension combinator invariants, numeric/runtime WAT in full, CLI executor selection, runtime panic mapping, all capability signature restrictions, or all relevant specification and owner records. This pass makes no claim that these files or subsystems are fully audited.

Existing fixtures to execute in the validation pass:

| Area | Existing evidence | What it can establish |
| --- | --- | --- |
| Ordinary cleanup | `runtime/valid/defer-after-return-value.hd`, `defer-lifo-and-loop-exits.hd` | Result saving and scope exit ordering |
| Linear suspensions | `runtime/valid/sequential-suspending-calls.hd`, `suspending-call-with-defer.hd` | Basic continuation and retained cleanup |
| CFG suspensions | `runtime/valid/suspending-calls-in-branches.hd`, `suspending-calls-in-loops.hd`, `suspending-result-propagation.hd` | Nested drive lowering and propagation |
| Cancellation | `runtime/valid/cancellation-skips-unreached-defer.hd`, `cancellation-loop-and-branch-scopes.hd`, `cancellation-cleanup-order.hd` | Registered cleanup and child-first cancellation |
| Captures | `runtime/valid/closure-captured-let-shared.hd`, `nested-closure-captures.hd`, `closure-captures-per-iteration.hd` | Storage aliasing and activation lifetime |
| Calls | `runtime/valid/named-arguments-evaluate-in-source-order.hd` | Source argument order; does not alone settle default initialization semantics |
| Implementation replay | `test/compiler-suspension.test.ts:490` onward | Current record/replay mechanism, not full durable replay |

Highest-value new reproduction: retain one direct top-level bang call, place a separate nested early return before or after it, and compare `i32`, non-`i32`, and `void` results. Add a registered outer defer. Repeat with separate postfix propagation. This was described rather than authored as an hd program; no parse or execution result is claimed.

The smallest existing source pattern is [sequential-suspending-calls.hd](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/runtime/valid/sequential-suspending-calls.hd): its `main!` binds `left!(20)` directly in the top-level suite. Retain that binding and introduce a separate branch with early return. For propagation, [suspending-result-propagation.hd](https://github.com/HerringtonDarkholme/hd-lang/blob/823f346878028aad4a4c9351593217f04445bd4c/spec/conformance/runtime/valid/suspending-result-propagation.hd) supplies the result types and ordinary inspection helper; separating the drive from subsequent propagation is the critical variation.

Specification review should also reconcile `fn.param.init-order` with `expr.call.source-order` and `fn.default.eval`. They can be coherent if explicit argument evaluation precedes parameter initialization; the phase distinction should be tested explicitly. No contradiction is established by this pass.

Architectural direction for review: one control-flow lowering pass that owns return, propagation, result saving, cleanup edges, and suspension boundaries. The emitted poll ABI should be a representation of that lowered graph. Keep general naive choices such as cells, dictionary passing, boxing, and linear dispatch where their semantics are clear; prioritize removal of duplicated language semantics.
