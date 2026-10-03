# Compiler Repair Queue

Status: Owner-requested implementation queue, recorded 2026-10-03. Unchecked items are pending, not deferred or disproven.

Deliver one root-cause repair at a time, in priority order. Do not alter the spec, trim std to bypass failures, raise timeouts, or weaken tests.
After evaluating each item, record **fixed**, **deferred** with a reason, or **not reproducible** with the reproduction and result.
Tagged items are fixed only when their rows pass and move from `test/portable/KNOWN_FAILURES.tsv` to `cases.tsv`.
Before each push, fetch/rebase, run the full `pnpm run check`, and watch main's Test workflow until green.
Completed checkpoints remain checked until the next audit cleanup; detailed validation belongs in Git history.

## P1: Standard Library Loading

- [x] **1. Fixed: emit reachable std items only.** HIR declaration-index reachability selects std functions, closures, defaults, iterators, and dictionaries; structural backend linking removes unused runtime declarations, types, globals, and imports. Original IDs and host-callable exports are preserved. Full check: 1,927 conformance cases and 377 unit tests passed; 162 colocated source tests, website build, and fuzz smoke passed. Main CI passed.
- [ ] **2. Registration implemented; acceptance partially complete.** Added `std.host`, `std.fs`, `std.path`, `std.json`, `std.encoding`, and `std.digest` to `STANDARD_MODULES`. Fifteen cases pass and move to `cases.tsv`: ten registration cases plus five JSON cases enabled by item 3. `json-suite` is now correctly classified under the missing `parse_f64` hook (item 19). The remaining `HOST-CATALOG` rows need APIs or provider behavior absent from `lib/std` or the runtime host.
- [ ] **3. Structural binding implemented; tagged acceptance remains.** Parsed std declarations and their references are renamed through lexical AST scopes; member names, variants, fields, strings, comments, parameters, locals, and generic binders are untouched. The `Duration` string corruption and JSON `Number` collision are fixed, and parsed `use` declarations are dropped structurally. Five JSON rows pass and move; synthetic coverage proves a free helper may share a trait method's spelling. One loader edge remains: if a future retained std API (not a stripped derivation template) names a compiler-loaded `std.structure` declaration, the loader must preserve that dependency by declaration identity rather than the discarded `use` text. `HOST-CATALOG` and `DERIVE-DEFAULT` rows still need their missing std declarations before their compiler path can be exercised.
- [x] **4. Fixed: derive joined std declarations.** The checker now joins std and compiler-loaded inspect declarations before the typed-derivation phase, preserves exact std identities for templates, typed facts, newtypes, and generated support, and marks generated std code as std-owned. Reachability retains a standard derived handle's one module constant only when it is used, preserving constant identity without making it an unconditional root. Empty programs no longer load structural support merely because a stripped std template body contains a tuple. Coverage derives prelude and non-prelude traits for joined std declarations, checks std facts, exercises inspect-phase ordering and newtype lowering, and proves unused helpers disappear while called helpers remain. The `STD-DEBUG` fixture remains in `KNOWN_FAILURES.tsv`: current `lib/std` still has no `@derive(Debug)` on `TypeId` or `SelfRef`, and this repair is restricted to `src/`.
- [x] **5. Fixed: preserve std diagnostic locations.** Joined AST positions now carry non-serialized physical-source metadata while retaining their logical user anchor for source-order checking. Text, JSON, package, and REPL diagnostics select the physical std document and coordinates; related spans and edits retain their own source. The mapping covers ordinary modules, derivation templates, compiler-injected `std.inspect` and `std.structure`, generated typed-fact helpers, and parser failures including the two preludes. Source-aware transforms and diagnostic deduplication no longer collapse or strip origins. Runtime host-call record/replay now identifies the emitting HIR function explicitly and hashes its physical body, instead of interpreting a shared logical anchor as a user-source offset. Focused coverage passes for every loader path and the complete 181-test colocated `src` suite.

  Two consumer/schema follow-ups remain outside this `src`-only repair. The playground's no-`main` entry-input adapter ignores the `file` carried by `ReplMessage` (its linked-prelude path is fixed); it must use that file without entry-line relocation. Package JSON relocation also predates this repair and relocates line/column but not UTF-16 offsets or cross-file fix ownership. Neither changes compiler diagnostic provenance, but both should become adapter regressions when edits outside `src/` are allowed.

### Item 1 Size Checkpoint

Exact WAT UTF-8 bytes, using `compileToWat` with default options. Baseline: `5ba7393e` (spec pass 76), identical library files and inputs.

| Input | Before | After |
| --- | ---: | ---: |
| `pub fn main() -> void: pass` followed by a newline | 741,925 | 49 |
| `tests:` block containing one `it("one")` with body `pass` | 1,157,161 | 2,459 |

The exact sources are in [reachability regressions](../../src/emitter/reachability.test.ts).
The earlier pre-pass-76 measurements were 670,018 and 1,085,281 bytes; these are not the same-library comparison.
For the empty `main`, the historical 741,925-byte WAT assembled to a 75,477-byte
Wasm binary before backend linking. In the repaired pipeline, HIR reachability
first produces 66,814 bytes of intermediate WAT (5,780 bytes when assembled),
then backend linking produces the final 49-byte WAT and 35-byte Wasm module.
None of the 66–75 KB measurements is a shipped empty executable. A program with
no entry point at all produces 10 bytes of WAT and an 8-byte Wasm header.

## P2: Checker And Diagnostics

Dogfood references: [F1–F13](../dogfood-199.md).

- [x] **6. Fixed: contextual empty field literals (F3).** Data construction now has an inference-only expected-type mode distinct from a permission/coercion requirement. A fully known direct mutable field therefore supplies the key, value, or element types for empty map/list literals—including through control flow and nested generic data—without demanding that a named or nested readonly value become mutable. Unresolved generic fields still report `cannot-infer-type`. Seven focused regressions cover fresh mutation, explicit and inferred generic arguments, control flow, unresolved types, and both readonly boundaries.

  Follow-up edge: contextual enum variants still inspect a `mut Enum[T]` expectation without first taking its readonly nominal form. For example, `.Some(1)` cannot initialize a field typed `mut Choice[i32]`; this predates item 6 and needs its own repair and regression.
- [x] **7. Fixed: print source-form permission types (F2).** Checker and parser diagnostics now render resolved `ValueType` values through the structural source formatter at their construction boundary. This covers shared mismatch and generic-inference messages plus direct call, receiver, field, pattern, operator, suspension, requirement-row, provider-key, implementation-target, bound, derivation, entry-result, and parser diagnostics. It preserves nested permission scope instead of applying a textual `mut:` replacement to completed messages. Seven focused regressions cover the reported `mut List[char]` parameter, a mutable receiver, a mutable-upgrade target, `List[mut Box]`, a generic-inference conflict, an implementation target, and a provider key. The complete colocated source suite and full repository check pass.
- [x] **8. Fixed: explain readonly receiver failures (F1).** A rejected `mut self` call now uses one diagnostic path for inherent, implemented-trait, and dynamically bounded methods. A direct readonly receiver names its binding and concrete type; a readonly field or other expression instead identifies the receiver, so a mutable root binding is not blamed for a readonly edge. Five focused regressions cover generic `List.push`, a user inherent method, an implemented trait method, a dynamically bounded method, and a readonly field below a mutable root.
- [x] **9. Fixed: suggest postfix spread (F13).** A prefix `...` in any list-element position now reports `syntax-error` with the valid `[xs...]` spelling, rather than falling through to the generic `expected-expression` diagnostic. Focused parser regressions cover first and later elements, valid postfix list spreads, and both data copy-update forms that intentionally retain prefix `...`.
- [x] **10. Fixed: compare compatible outer permissions.** Equality, inequality, and ordering now normalize an exact outer `mut` difference to the shared readonly type before their same-type check. Both operand orders work for fresh and named values, while nested permission differences remain distinct. Focused checker regressions verify the normalized HIR comparison type, named views, a mutable outer list with mutable elements, and rejection of `List[P]` versus `List[mut P]`. The existing F-163 conformance fixture now checks directly; its portable ledger row remains untouched under this src-only repair.
- [ ] **11. Contextual wide integer literals.** A u64-sized literal nested inside an expression receives the expected type like other literals.
- [ ] **12. Preserve inherent impl bounds (F5).** Retain `K < Eq & Hash` in `prepareInherentImplementation` for Map impls; verify specified Map keys/values behavior. Tag: `STD-1` (`map-keys-values`).
- [ ] **13. Enforce Error's AnyRef contract.** Reject Error impls on AnyVal newtypes; accept dynamic-safe method parameters whose bounds imply AnyRef through supertraits; fix the illegal cast for `e.find::[Error]()` in `src/emitter/value-comparison.ts`. Tag: `ERR-HELPERS`.
- [ ] **14. Finish Default derive checks.** Verify `derived-default-declared-no-bound` and count `@default` variants, including `derived-default-no-variant`. Tag: `DERIVE-DEFAULT`.

## P3: CLI And Runtime Hosts

- [ ] **15. Test-only CLI execution (F4).** `hd test FILE` runs only test cases, never `main`, and does not count `main` as passing.
- [ ] **16. Console error output.** Host Console answers `write_error_line!`; exercise `eprintln` through real `hd run`. Resolve current console error-line ledger tags before repair.
- [ ] **17. Current runner capabilities.** Bind TestRunner for every test body; add `TestRunner.snapshot_check`, route `snapshot_file` through it, and remove the retired `snapshot_file_check` primitive. Implement PropertyRunner start → PropertyCase, record/show; Choices replays and records draws in hd. A panic with message `std.testing: case discarded` before `show` is a discard. Tags: `SNAPSHOT-ROW`, `RUNNER-SURFACE`.
- [ ] **18. Runtime profiles.** Implement `misbehaving-host`: u8 result 300 panics with category `host-contract`, validating host results against declared types. Implement `pending-write`: first console-write poll is pending. Tags: `HOST-CONTRACT`, `PENDING-WRITE`.
- [ ] **19. Floating-point parse hook.** Add `parse_f64` beside `format_f64`, correctly rounding the RFC 8259 number grammar for `std.json` and `std.num`. Tag: `FLOAT-PARSE`.

## P4: Housekeeping

- [ ] **20. List API rename.** Update src code, TS tests, and diagnostics from `List.append` to `push`, and support specified `pop`. Do not rename unrelated APIs.
- [ ] **21. Known-issue inventory.** Add `STD-LOADER`, `FLOAT-PARSE`, `PENDING-WRITE`, `HOST-CONTRACT`, and `ERR-HELPERS` to `src/KNOWN_ISSUES.md`; recount `STD-1` and `DEFAULT-HASHER` from the actual ledger.
- [ ] **22. Retired citations.** Remove or correct retired rule IDs in src comments; check with `pnpm run spec refs` and the dead-citation checker.

## Delivery Checks

- [ ] Record each evaluated item's outcome and evidence; remove verified fixed issues from the remaining-work audit.
- [x] For item 1, record reproducible before/after WAT sizes using identical inputs and options.
- [ ] Move only passing tagged conformance rows to `cases.tsv`, preserving fixture intent.
- [ ] Fetch/rebase; full check; push one repair; watch main Test until green.
