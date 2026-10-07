# New Compiler Design: Response To The Codex Review

Part of the [compiler design](README.md).

Status: Response to the Codex review of 8bb6860d, 2026-10-07. Backend lane rows; frontend rows are added by the frontend lane.

The review is not in the repository. It reviewed commit 8bb6860d. Commit
613c7b7a, made after it, rewrote the TIR encoding and added the wire
rule, so each claim below was checked against the current files and the
spec, not against the reviewed commit.

**Verdicts.** `accepted-fixed`: the claim holds and the design is fixed
where the row says. `accepted-pending-owner`: the claim holds, and the
fix needs an owner decision. `partly`: part of the claim holds, and that
part is fixed. `rejected`: the claim does not hold. `already-fixed`: the
claim held at 8bb6860d and 613c7b7a fixed it. `frontend lane`: verified
and fixed by the frontend agent.

The review has five areas. It has no sections on the back end,
contradictions, simplifications or build order, so there are no rows for
those.

## Backend Lane

### Blockers

| Id | Verdict | Fixed in | Reason |
| --- | --- | --- | --- |
| 1 | accepted-fixed (owner decision B; spec change queued for the spec pass) | [wasm-layout.md §15.2](wasm-layout.md#152-values) | True: nullable-reference `Option` and `i31ref` erasure broke `expr.is.some` and `expr.is.box.distinct`. The owner chose B: `.Some` and boxes lose identity, so the layouts stand. `Result` still had a value layout with identity, so it is now one struct per construction until open question 23.1-7 is answered. `is` on trait values now also compares type ids. |
| 2 | accepted-fixed | [codegen.md §13.3](codegen.md#133-instance-keys), [§13.8](codegen.md#138-code-entries), [testing-the-compiler.md §21.4](testing-the-compiler.md#214-codegen-cache-soundness) | True: the instance key held the callee's `tir_hash`, and relocations named it. The instance key is now a symbol without the body hash; the code key carries it, and link resolves symbols to this build's entries. |
| 3 | frontend lane | | Solver: owner-module lookup with unknown trait arguments. |
| 4 | frontend lane | | Defaults memoized and facts evaluated at run time. |
| 5 | accepted-fixed | [codegen.md §13.5](codegen.md#135-dictionaries-trait-values-and-gadt-evidence), §13.2 step 5; [checking-and-tir.md](checking-and-tir.md#instruction-catalog) `CallDyn` | True: no strategy existed for a method type parameter behind a vtable. Erased instances take bound evidence as parameters, as the spec's shapes section describes; `CallDyn` now carries the method's type arguments. |
| 6 | frontend lane | | Hidden template helpers and the syntax-only interface. |
| 7 | frontend lane | | The M3 row solve. |
| 8 | frontend lane | | Solver memo keys. |
| 9 | accepted-fixed | [cache.md §5.5](cache.md#55-the-stat-manifest-and-change-detection), [data-structures.md §3.20.3](data-structures.md#3203-the-stat-manifest), [testing-the-compiler.md §8.2](testing-the-compiler.md#82-incremental-soundness) | True: "A restored file with an old mtime has a different inode or size" fails for a same-length in-place rewrite. The record now holds `ctime`, which no user API can set, as git's index does. `written_at` is a file-system timestamp, a read is bracketed by two stats, and the exact edit is in the corpus. |
| 10 | already-fixed | [data-structures.md §3.20.2](data-structures.md#3202-the-wire-rule-entry-local-tables-mine), [checking-and-tir.md](checking-and-tir.md#lifetime-and-the-tir-entry) wire form | 613c7b7a added entry-local string, path and type tables, generated remapping of every ID word, and span columns instead of `NodeIdx`. |

### Area 1: Incremental Soundness

| Id | Verdict | Fixed in | Reason |
| --- | --- | --- | --- |
| A1 | accepted-fixed | [cache.md §5.3](cache.md#53-key-composition), [commands.md §7.1](commands.md#71-hd-check-package-mode) | True: the root package key was the constant `root`. It is now `H("root", manifest_key)`, over the package name, dependency bindings, executables, toolchain and manifest diagnostics. Manifest errors stop the run before any cache lookup. |
| A2 | partly | [cache.md §5.3](cache.md#53-key-composition) | No limit is configurable today, so no wrong hit exists yet. The contract is fixed anyway: effective semantic limits go into `toolchain_key`, and interrupted work is never cached. |
| A3 | accepted-fixed | [cache.md §5.3](cache.md#53-key-composition), [data-structures.md §3.7](data-structures.md#37-spans-and-files), [testing-the-compiler.md §8.2](testing-the-compiler.md#82-incremental-soundness) | True: `iface` and `coh` entries stored absolute spans under position-free keys. Spans there are now relative to a declaration, resolved through a per-file `locs` entry, as rustc does. The producers in resolution-and-interfaces.md follow this rule. |
| A4 | accepted-fixed | [codegen.md §13.8](codegen.md#138-code-entries), [testing-the-compiler.md §21.4](testing-the-compiler.md#214-codegen-cache-soundness) | True: "every item its TIR names" misses concrete type arguments and impls selected under substitution. The code key now covers what collection reads: layout hashes, selected impls, callee interfaces and inline summaries. |
| A5 | frontend lane | | Synthetic impls and tuple templates. |
| A6 | accepted-fixed | [commands.md §7.4](commands.md#74-hd-test---affected) | True: one package-wide source record missed dependency changes and filtered runs. The record is now a per-program fingerprint over `prog_key` and test options, written only when the whole program passed. |
| A7 | accepted-fixed | [cache.md §5.3](cache.md#53-key-composition) | True: suggestions searched traits outside the key. Suggestions now search only keyed inputs; [type-checking.md §10.5](type-checking.md#105-fix-its-for-common-mistakes) (frontend file) follows the rule. |

### Area 2: Determinism

| Id | Verdict | Fixed in | Reason |
| --- | --- | --- | --- |
| D1 | accepted-fixed | [codegen.md §12.4](codegen.md#124-rows-and-providers), [checking-and-tir.md](checking-and-tir.md#instruction-catalog) callee records, [testing-the-compiler.md §21.4](testing-the-compiler.md#214-codegen-cache-soundness) | True, and worse after 613c7b7a: rows are re-sorted on disk while TIR provider lists kept in-run `Ty` order. Providers are now `(key, provider)` pairs, and one key order, `canon(K)` bytes, serves signatures, calls, contexts and entries. |
| D2 | accepted-fixed | [scheduler.md §6.5](scheduler.md#65-deterministic-output-assembly), [checking-and-tir.md §4.14](checking-and-tir.md#414-diagnostics) | True: coherence and init diagnostics could arrive after a module's release. Diagnostics now print after all tasks finish; test results keep their cursor. Fix-its are collected, sorted, then the first 20 verified. |
| D3 | partly | [engines-and-test-runner.md §19.4](engines-and-test-runner.md#194-property-tests-panics-and-timeouts) | True for splitting one property's cases across workers: removed. Shrink-attempt reuse is a deterministic function of seed and outcomes, so it stays. |
| D4 | accepted-fixed | [testing-the-compiler.md §8.1](testing-the-compiler.md#81-the-determinism-matrix), [checking-and-tir.md §4.14](checking-and-tir.md#414-diagnostics) | True: `modules_checked` differs cold and warm by design. Semantic output and keys are compared exactly, entry bytes only under equal keys, operational fields per cache state. |

### Area 3: Performance

The figures were estimates; no target was measured. Each row now says
so and names the slice that measures it.

| Id | Verdict | Fixed in | Reason |
| --- | --- | --- | --- |
| P1 | accepted-fixed | [build-order.md §9.1](build-order.md#91-status-of-the-pillar-1-and-2-targets), [data-structures.md §3.24](data-structures.md#324-memory-budget) | The 13 MB figure is an estimate, not RSS. Status: plausible, not established; slice 4 measures empty, 100-file and 10,000-file packages. |
| P2 | accepted-fixed | [build-order.md §9.1](build-order.md#91-status-of-the-pillar-1-and-2-targets), [cache.md §5.4](cache.md#54-entry-format-and-atomic-publish), [§5.7](cache.md#57-eviction-and-the-size-cap) | Latency depends on module size. Status: plausible for small modules. "Flush" now means close without `fsync`, and eviction on interactive commands is bounded to about 20 ms. |
| P3 | accepted-fixed | [build-order.md §9.1](build-order.md#91-status-of-the-pillar-1-and-2-targets) | Line count does not bound header or coherence work. Status: plausible for ordinary code; an ordinary benchmark and a pathological set are reported apart. |
| P4 | accepted-fixed | [commands.md §7.1](commands.md#71-hd-check-package-mode), [build-order.md §9.1](build-order.md#91-status-of-the-pillar-1-and-2-targets) | `--version` returns early, `hd check` creates no engine, and the pool starts only on a fast-key miss. Status: feasible only with this lazy path. |
| P5 | accepted-fixed | [build-order.md §9.1](build-order.md#91-status-of-the-pillar-1-and-2-targets) | The backend contracts are fixed by rows 10, D1, D2 and D4. Status: not established; the solver memo (finding 8) is frontend. |
| P6 | accepted-fixed | [design-overview.md §1.4](design-overview.md#14-what-a-run-touches) | The count is `modules_checked`; init order, template helpers and facts are listed as exceptions, and no necessary work is skipped to meet it. |
| P7 | accepted-fixed | [build-order.md §22.1](build-order.md#221-how-the-pillar-3-targets-are-met) | Status changed from "at risk" to "not established, at risk". Decision B keeps `Option` free; `Result` allocates until question 23.1-7. Slice 7 benchmarks the real layouts on wasmtime; the browser is reported apart. |
| P8 | accepted-fixed | [engines-and-test-runner.md §19.3](engines-and-test-runner.md#193-running), [build-order.md §22.1](build-order.md#221-how-the-pillar-3-targets-are-met) | 0.1 ms is an estimate; init per case is the program's cost. Status changed from "met by estimate" to "plausible, not established". |
| P9 | accepted-fixed | [wasm-layout.md §15.6](wasm-layout.md#156-the-2-kb-tiny-program), [build-order.md §22.1](build-order.md#221-how-the-pillar-3-targets-are-met) | 800 B is a guess per part. Status changed from "met by estimate" to "plausible, not established"; slice 6 starts with a real hello-world spike. |

### Area 4: Type Checker And Trait Solver

| Id | Verdict | Fixed in | Reason |
| --- | --- | --- | --- |
| T1 | frontend lane | | Associated projections. |
| T2 | frontend lane | | Rollback of mutable checker state. |
| T3 | frontend lane | | GADT scoped pop. |
| T4 | frontend lane | | Expected-result filtering of candidates. |
| T5 | frontend lane | | Cross-statement literal joins. |
| T6 | frontend lane | | Fuel and scaling claims. |
| T7 | frontend lane | | Header validation stage. |

### Area 5: IR And Data Structures

| Id | Verdict | Fixed in | Reason |
| --- | --- | --- | --- |
| I1 | accepted-fixed | [checking-and-tir.md](checking-and-tir.md#encoding) encoding, catalog, builder, invariants 1, 5, 14; [data-structures.md §3.9.5](data-structures.md#395-building-scratch-buffer-checkpoints-truncation) | True: `Break` named a `Loop` emitted after it. Operands are typed as value, child block, label or local; def-before-use applies to values only; labels are allocated at open. |
| I2 | accepted-fixed | [data-structures.md §3.9.5](data-structures.md#395-building-scratch-buffer-checkpoints-truncation), [checking-and-tir.md](checking-and-tir.md#the-builder-api) builder | True for the TIR side: reserved slots let the checker fill a block position, an arm-tail coercion or a callee record later, once, before `finish`. When the checker reserves is in type-checking.md (frontend). |
| I3 | partly | [checking-and-tir.md](checking-and-tir.md#instruction-catalog), [build-order.md §9](build-order.md#9-build-order) slice 3 | `Builtin` choices were added by 613c7b7a. Added `Is`, `CallDyn` type arguments and a bare `Return`. A source-to-TIR coverage table is now slice 3's exit. `Default` is frontend (finding 4). |
| I4 | accepted-fixed | [codegen.md §12.1](codegen.md#121-analysis-then-one-emission-walk), §12.6; [suspension.md §14.2](suspension.md#142-the-state-machine) | True: escape and loop liveness need later uses. Bounded analysis passes with side tables now run before the one emission walk; liveness is one fixed-point dataflow, not a scan per point. |
| I5 | partly | [data-structures.md §3.9.4](data-structures.md#394-tables-as-struct-of-arrays) | Nanosecond figures were estimates. Mixed-access tables may be small structs, chosen by measurement in slice 3. The inference-variable byte claim is in type-checking.md §13 (frontend). |

### Rejected

None. Two rows are `partly`, because the claim's failure cannot happen
today (A2), or because one half is deterministic already (D3).

### Spec Changes For The Spec Pass

Owner decision B, 2026-10-07: `.Some(...)` and primitive, string or tuple
boxes have no observable identity, and `is` on optionals compares
payloads. `.None is .None` stays true. The spec pass applies these.
Items 7 and 8 are this lane's reading of B and need the owner's check.

| # | Rule | File | New meaning |
| --- | --- | --- | --- |
| 1 | `expr.is.some` | [05-expressions.md](../../spec/lang/05-expressions.md#allocation-identity) | `.Some(value)`, including an implicit wrap, has no identity of its own. `is` on two optionals is true when both are `.None`, or both are `.Some` and their payloads satisfy `is`. |
| 2 | `types.option.identity.some` | [04-type-system.md](../../spec/lang/04-type-system.md#optional-identity-and-variance) | The same as item 1. |
| 3 | `types.option.identity` | 04-type-system.md | `is` compares optionals by their payloads, not as other enum values. |
| 4 | `expr.is.box` | 05-expressions.md | Converting a primitive, string or tuple value to a trait value or `Any` creates no identity. The result has none of its own. |
| 5 | `expr.is.box.identity` | 05-expressions.md | Replaced: `is` between two trait or `Any` values whose payloads are `AnyVal` values has an unspecified result, as for function values (`expr.is.function-unspecified`). |
| 6 | `expr.is.box.distinct` | 05-expressions.md | Removed. |
| 7 | `types.sealed.anyref`, `types.sealed.anyval-types` | 04-type-system.md | An optional takes its payload's category: `T?` implements `AnyRef` when `T` does and `AnyVal` when `T` does. So `is` on `i32?` is `identity-requires-references`. The other reading, comparing scalar payloads by value, makes `is` a value equality. |
| 8 | `module.prelude.anyval-types`, `trait.sealed.anyval-types` | [10-modules.md](../../spec/lang/10-modules.md), [09-traits.md](../../spec/lang/09-traits.md) | Follow item 7: `AnyVal` also holds optionals of `AnyVal` types. |

Unchanged: `expr.is.none` and `types.option.identity.none` (`.None` is
canonical), `expr.is.conversion` and `expr.is.no-wrapper` (a heap
composite keeps its identity when erased), and
`trait.downcast.same-reference`.

Text without rule IDs that the pass rewrites, all in
04-type-system.md: the Value Categories table and its sentence
"Converting a value without identity ... allocates a box with its own
identity"; the Shapes and Generic Code row "the result has its own
identity"; and the Composite Representation bullet "Because each `.Some`
construction has its own identity, a present value cannot be represented
by the payload itself", which becomes the opposite.

Fixtures that change:
`spec/conformance/runtime/valid/optional-identity.hd`, where
`first is second` becomes true and `count is other_count` moves to an
invalid typing case under item 7; and
`spec/conformance/runtime/valid/boxed-primitive-identity.hd`, which keeps
its alias assertions and drops those about distinct boxes.

### Owner Questions Raised

- **Open question 23.1-7: extend decision B to `Result`.** Recommendation:
  yes, with the same `is` rule as optionals; then `Result` allocates
  nothing ([open-questions.md](open-questions.md#231-open-questions-for-the-owner)).
- **Items 5 and 7 above:** "unspecified" for boxes, and "an optional takes
  its payload's category". Both follow from B; the owner may prefer value
  comparison instead.

## Frontend Lane

The frontend rows (findings 3, 4, 6, 7 and 8, A5, T1 to T7, P2 and P6)
are in [codex-review-response-frontend.md](codex-review-response-frontend.md).
