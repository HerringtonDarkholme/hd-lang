# Compiler Repair Queue

Status: Owner-requested implementation queue, recorded 2026-10-03. Unchecked items are pending, not deferred or disproven.

Deliver one root-cause repair at a time, in priority order. Do not alter the spec, trim std to bypass failures, raise timeouts, or weaken tests.
After evaluating each item, record **fixed**, **deferred** with a reason, or **not reproducible** with the reproduction and result.
Tagged items are fixed only when their rows pass and move from `test/portable/KNOWN_FAILURES.tsv` to `cases.tsv`.
Before each push, fetch/rebase, run the full `pnpm run check`, and watch main's Test workflow until green.
Remove completed items after recording their validation in Git history.

## P1: Standard Library Loading

- [ ] **1. Emit reachable std items only.** Follow semantic references from program roots, including methods, generic specializations, callbacks, dictionaries, and initialization dependencies. Do not emit every function of joined modules. Record exact inputs and before/after WAT bytes for a tiny program and a one-test program. Reported baselines: approximately 671 KB and 1,086 KB. Next investigation.
- [ ] **2. Register std modules.** Add `std.host`, `std.fs`, `std.path`, `std.json`, `std.encoding`, and `std.digest` to `STANDARD_MODULES`. Tags: `STD-LOADER`, `HOST-CATALOG`.
- [ ] **3. Bind std names by declaration identity.** Replace textual renaming so free helpers can share trait-method names: `now`, `sleep!`, `read_text!`, `write_text!`, `read_line!`, and the `default` derive marker. Coordinate with audit A01 rather than introducing spelling exceptions. Tags: `HOST-CATALOG` (clock helpers, console input helper), `DERIVE-DEFAULT`.
- [ ] **4. Derive on std declarations.** Run `@derive` on `lib/std` declarations, not only before std is joined. Verify generated Debug implementations without changing std to conceal compiler gaps.
- [ ] **5. Preserve std diagnostic locations.** Errors inside `lib/std` must identify their std file and line, not the user's first position or `use` line.

## P2: Checker And Diagnostics

Dogfood references: [F1–F13](../dogfood-199.md).

- [ ] **6. Contextual empty field literals (F3).** Infer `Walk { state: {} }` from field type `mut Map[string, i32]` instead of reporting `cannot-infer-type`.
- [ ] **7. Print source-form permission types (F2).** Print `mut List[char]` in diagnostics, not internal `mut:List[char]` encodings.
- [ ] **8. Explain readonly receiver failures (F1).** If a named `mut self` method exists but the receiver binding is readonly, explain that restriction in `unknown-method` diagnostics.
- [ ] **9. Suggest postfix spread (F13).** For `[...xs]`, suggest `[xs...]` instead of a bare `expected-expression`.
- [ ] **10. Compare compatible outer permissions.** Accept spec-permitted equality such as `f() == P { x: 1 }`, rather than rejecting `P` versus `mut:P`.
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
- [ ] For item 1, record reproducible before/after WAT sizes using identical inputs and options.
- [ ] Move only passing tagged conformance rows to `cases.tsv`, preserving fixture intent.
- [ ] Fetch/rebase; full check; push one repair; watch main Test until green.
