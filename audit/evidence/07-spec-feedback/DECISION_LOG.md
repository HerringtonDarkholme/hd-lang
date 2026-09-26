# Phase 7: Specification Feedback Decision Log

Commit `bd985d7`, 2026-09-25. Written by the coordinator from the evidence
of phases 1 to 6. Each entry states what the implementation revealed, the
evidence, and one recommendation:

- **keep spec:** the spec is right; the implementation must change;
- **spec change:** a wording or rule change, usually a clarification;
- **spec tooling:** fix the reference parser or check scripts;
- **OPEN_ISSUES question:** a design decision for the owner, phrased as a
  question.

No entry proposes a new language feature.

## A. Conformance Infrastructure

| #  | What the implementation revealed | Evidence | Recommendation |
| -- | -------------------------------- | -------- | -------------- |
| A1 | The fixture format (`expect-result`, `expect:`, `test:`, phase-to-command mapping, command options, exit statuses, located-line format) is defined only in `test/` and `src/`. | F-200; `01-test-quality/directives.tsv` | **spec change** (decided 2026-09-25): apply `audit/proposals/fixture-format.md`, resolving its `[OPEN-n]` markers first. |
| A2 | Implementations need a written command contract. The runner only works if the compiler echoes the argv path verbatim. | F-207; `audit/fuzz/CONTRACT.md` | **spec change:** make the contract in `CONTRACT.md` normative in `spec/conformance/README.md` (covered by A1). **OPEN_ISSUES question:** match paths by exact argv, or by a normalized relative path? |
| A3 | Marker lines are normative, but the spec has no rule for where a diagnostic is located. Every tested line equals this compiler's span. | F-102 | **OPEN_ISSUES question:** should the spec define the anchor for each diagnostic, or should runners accept any line within the construct's span? |
| A4 | Reject cases can pass while reporting unrelated extra errors. | F-150, F-208 | **OPEN_ISSUES question:** must a reject case produce only its marked diagnostic? |
| A5 | `# panic: runtime-error` accepts any crash, which hid a raw trap for `2 ** -1`. | F-153 | **spec change:** remove `runtime-error` as a conformance expectation; every panic case names a chapter-06 category. |
| A6 | Panic reports carry no source location, so panic marker lines are unchecked. | F-155, F-204 | **OPEN_ISSUES question:** should a panic report carry a location that fixtures check? |
| A7 | Harness hooks `competing-drivers` and `reentrant-poll` cannot be built from their description; `fixture-package-role` packages exist nowhere; `ready-*` profiles live only in `src/cli.ts`. | F-212, F-259 | **OPEN_ISSUES question:** should the spec define a test-driver interface in source terms (drive, poll, cancel a named suspension) instead of named scenarios? |
| A8 | Half the `test/fixtures` reject markers use codes missing from `spec/README.md`: `type-mismatch`, `unknown-name`, `unknown-type`, `argument-count`, `missing-display`, `impure-parameter-default`, `generic-type-mismatch`, `unsupported-*`, and syntax codes. | F-205, F-356, F-258 | **OPEN_ISSUES question:** which of these join the inventory? Is `no-least-common-type` or `no-common-type` the right code for the F-258 case? |
| A9 | Deferred features fall into generic parse errors, so a deferral looks like a rejection. | F-250 | **OPEN_ISSUES question:** should the spec define a portable "unsupported by this implementation" category, which conformance runners treat as skipped rather than failed? |
| A10 | `expect-result` compares JavaScript formatting of Wasm results. | F-202 | **OPEN_ISSUES question:** should expected values use `Display` rendering, or should fixtures use `assert_equal` only? |
| A11 | The reference parser rejects valid source in four places: `\u{...}` escapes, string literals in `${...}`, nested named arguments, and a spread after an explicit `$.with` binding. It also disagrees with chapter 01 on other lexical rules. | F-206, F-358, F-359, F-360 | **spec tooling:** fix `spec/reference-parser/`; add a fixture for each case. |
| A12 | 17 spec sections are covered only by `test/fixtures`, and 60 substantive sections have no fixture at all (for example chapter 05 evaluation order, chapter 07 receivers, chapter 10 packages). | `01-test-quality/coverage.tsv` | **spec change:** promote the 107 ready language fixtures into `spec/conformance/`; add fixtures for the uncovered sections. |
| A13 | Several diagnostics could reasonably be reported either by `parse` or by `check`: `reserved-name`, `nonfinal-positional-spread`, `duplicate-mutable-permission`, `invalid-assignment-target`, `nonfinal-vararg`. | `03-fuzz/SUMMARY.md` | **OPEN_ISSUES question:** which diagnostics may `parse` report, and which belong to `check`? |
| A14 | The chapter-02 grammar derives same-line-suite forms that the layout rules cannot produce, such as a comma inside a same-line suite. | F-314 | **spec change:** reconcile the EBNF with the layout rules, then say which F-314 forms are valid. |
| A15 | The implementation-neutral fuzzer exists and works against any command. | `audit/fuzz/README.md` | **OPEN_ISSUES question:** should it move to `spec/tools/`, with a seeded smoke run in `spec/check.sh`? |

## B. Language Rules the Implementation Exposed

| #  | What the implementation revealed | Evidence | Recommendation |
| -- | -------------------------------- | -------- | -------------- |
| B1 | Named functions must declare requirement rows; only closures infer them. This makes edits local, which helps incremental compilation. | `06-compiler/row-ripple.md` | **keep spec**, plus **spec change**: state explicitly that a missing requirement clause on a named function means the empty row. |
| B2 | The module-initialization check misses generic-bound and dynamic trait dispatch, so an accepted program reads a zero global. The spec is unclear on which dispatches count. | F-603 (reproduced: prints `5`) | **keep spec** on the rule itself. **OPEN_ISSUES question:** does "functions reached by trait dispatch" include bounded and dynamic dispatch? Every implementation in scope, or only reachable ones? Does "later function bodies" (`10-modules.md:225`) mean source order or execution order? |
| B3 | Plain closures mutating captured `mut` values are accepted; `mutable-capture-requires-mut-fn` is never produced. | F-351, F-251 (reproduced) | **keep spec**; the implementation must change. |
| B4 | Primitives do not satisfy `PartialEq`, `PartialOrd`, or `Display` bounds. | F-500, F-350, F-263 (reproduced) | **keep spec**; the implementation must change. |
| B5 | `hd test` shares one instance across `main` and all test blocks, against the one-instance-per-test rule in chapter 02. | F-403 | **keep spec**; also answer: do test blocks run after `main` in the same instance? |
| B6 | Defaults reject any read of a top-level binding, which the spec allows. The dropped `pure` qualifier left no marker for purity. | F-101, F-100 | **OPEN_ISSUES question:** may defaults read reassignable top-level bindings? Should the `pure` qualifier return to `OPEN_ISSUES.md`? |
| B7 | `1e55fc9` closed three open issues without fixtures: provider escape (against its own recommendation), the `pure` qualifier, and `all!`/`race!` as intrinsics. The compiler still rejects the intrinsics with a code outside the inventory. | F-100 | **OPEN_ISSUES question:** does the owner confirm each closure? Should provider escape be qualified while NonEscapable providers remain open? |
| B8 | Two panic categories have no construct: `failed-checked-cast`, and a negative exponent. | F-406 | **OPEN_ISSUES question:** which construct raises `failed-checked-cast`, or should it be removed? Which category covers a negative integer exponent? |
| B9 | Several ambiguities affected blind-fixture outcomes: whether shared-data variants are canonical (AMB-08); `mutable-upgrade` against an inference conflict (AMB-17); the code for a default that names a later parameter (AMB-15); compatible composite reference types for `is` (AMB-24). | `03-blind-run/SUMMARY.md`, F-361 | **OPEN_ISSUES question** for each. |
| B10 | Fieldless data allocates distinct identities, while fieldless variants are canonical. | `05-object-model/SUMMARY.md` | **OPEN_ISSUES question:** is distinct identity for fieldless data intended? |
| B11 | `run` and `test` crash on files without an entry. A non-`pub` `main` with a non-host requirement passes `check`, then fails at run time. A `main` returning `Result[void, E]` passes `check` but cannot run. | F-306, F-307, F-308 | **OPEN_ISSUES question:** is a non-`pub` `main` an entry point, and which `main` result types are runnable? What must `run` and `test` do on a library file? |

## C. Representation and Performance

| #  | What the implementation revealed | Evidence | Recommendation |
| -- | -------------------------------- | -------- | -------------- |
| C1 | Chapter 04 makes erasure and boxing normative, while chapter 09 says the compiler "may monomorphize". Uniform erasure plus list identity forces shared erased storage for concrete lists. | `05-object-model/SUMMARY.md`, `erasure-shared-storage.hd` | **OPEN_ISSUES question:** which chapter governs? May values without identity use specialized layouts, while observable identity keeps the erased form? |
| C2 | Every stored `Suspend[T]` uses a uniform dynamic wrapper, and an immediately ready `fn!` call costs about 13 times a plain call. | F-554, `05-requirements/SUMMARY.md` | **OPEN_ISSUES question:** may a statically known frame skip the wrapper and result boxing? |
| C3 | Concrete rows cost nothing per call; row-generic code pays linear lookup (quadratic today because of F-550). | `05-requirements/SUMMARY.md` | **keep spec**. **OPEN_ISSUES question:** may row packs be positional under a canonical key order? |
| C4 | Frames keep every local; liveness would remove 75 to 100% of slots. | F-506, F-552 | **spec change:** state that frame contents are not observable, so liveness optimization is permitted. |
| C5 | Maps are linear, and `string.len()` is O(n). | F-501, `05-object-model/SUMMARY.md` | **OPEN_ISSUES question:** should the spec state complexity for map operations and `string.len()`? |

## D. Runtime, Host, and Replay

| #  | What the implementation revealed | Evidence | Recommendation |
| -- | -------------------------------- | -------- | -------------- |
| D1 | Replay identity hashes raw declaration text and covers only suspending functions: changed helpers replay silently, and formatting edits break replay. | F-401, F-611, F-264 | **OPEN_ISSUES question:** should replay code identity be transitive over executed functions, and computed from a normalized form? |
| D2 | A panicking or never-finishing run leaves no history; running past the end of a history is undefined. | F-404 | **OPEN_ISSUES question:** should a panicking run produce a replayable history? When the history runs out, should replay stop, or continue live and append? |
| D3 | The command-line provider-configuration identity is constant. | F-402 | **OPEN_ISSUES question:** is the runtime profile part of the provider-configuration identity? |
| D4 | The entry driver busy-polls a pending host provider forever; no conformance case observes waker-driven progress. | F-555 | **OPEN_ISSUES question:** is a driver that busy-polls without returning to the host conforming? |
| D5 | After a panic, a Node API instance keeps running with partial state, and the driver guard stays set. | F-556, `04-runtime/SUMMARY.md` | **OPEN_ISSUES question:** should the runtime poison an instance after a panic, or leave that to the host? |
| D6 | The host boundary uses `externref` handles, byte-at-a-time strings, and development hooks, far from the Component Model. | F-558, `05-requirements/SUMMARY.md` | **OPEN_ISSUES question:** should the spec define a development host boundary, or only the Component Model boundary with canonical-ABI strings? |

## E. Items from the Plan Found Not To Apply

- **"Rules invented by the implementation"** (float display, map insertion
  order, `nil` ordering first, the closed panic list): all of these predate
  `src/` and appear in `4cc1312` (`01-spec-edits/SUMMARY.md`). No action.
- **Row inference as a global fixpoint:** not the case (B1). No action beyond
  the B1 clarification.
- **Spec rules bent to fit the code:** none found; 0 relaxed hunks, 0
  weakened fixtures (`01-spec-edits/SUMMARY.md`).
