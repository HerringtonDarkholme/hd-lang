# MVP Audit Report

**Subject:** the Wasm GC MVP compiler in `src/`, commit `bd985d7`.
**Date:** 2026-09-25.
**Plan:** [`PLAN.md`](PLAN.md). Every number below comes from a command that
was run. The evidence folder for each section is linked in place.

## 1. Verdict

The MVP largely delivers what it claims, and its authors did not grade their
own homework unfairly:

- Its own suites pass: 173 TypeScript tests and 551 portable cases.
- Slice gates S0 to S4 are met, and S5 is partly met.
- 141 of 150 claims in its status documents were verified by execution.
- No fixture was weakened, and no spec rule was bent to fit the code.

Independent tests written from the spec alone pass at **72%** (43 of 60).
They pass 100% on the hardest semantic areas: evaluation order,
cancellation, requirement rows, and iterator invalidation.

The failures cluster where the suite's authors chose not to look:

- three soundness holes:
  - plain closures can mutate captured `mut` values;
  - the module-initialization check misses trait dispatch, so a program reads
    a zero global;
  - `Ok`/`Err` payload types are not checked, so a type-checked program
    fails Wasm validation;
- common operations that do not work: every `f64` `<`, `<=`, `>`, `>=`
  crashes code generation, and primitives do not satisfy `PartialEq`,
  `PartialOrd`, or `Display` bounds;
- a test harness that is not yet portable: the fixture format lives in
  `test/` and `src/`, half the fixture codes are not in the spec, and 327 of
  342 `test/fixtures` files are language tests outside `spec/conformance/`.

**Architecture:** sound for a single-file semantic prototype. The HIR is a
real typed and resolved boundary, and concrete requirement rows cost nothing
per call. It is not a base for a production compiler:

- types are strings;
- there is no lowered IR and no liveness;
- boxing reaches even concrete lists and optionals;
- maps are linear, and dictionaries are rebuilt on every call;
- suspension costs about 13 times a plain call and grows code
  super-linearly.

This matches the plan's framing of the MVP as a semantic probe that will be
replaced.

## 2. Scorecard

| Measure                                   | Result                                                            | Evidence |
| ----------------------------------------- | ----------------------------------------------------------------- | -------- |
| portable suite                            | 551/551 (209 conformance + 342 fixtures); 173/173 TypeScript tests | [`00-baseline`](evidence/00-baseline/) |
| full conformance                          | 210/286 (73.4%): 209 selected + 1 unselected passing              | [`02-coverage`](evidence/02-coverage/SUMMARY.md) |
| unselected cases                          | 77: 51 deferred, 25 in scope and failing, 1 passing               | same |
| structured rejection of deferred features (MVP goal 5) | 7 of 51                                              | same |
| blind fixtures                            | 43/60 (71.7%); 45/60 if two ambiguities go the implementation's way | [`03-blind-run`](evidence/03-blind-run/SUMMARY.md) |
| gap probes (high-risk mechanisms)         | 16/19; all 3 failures are `f64` ordering                          | [`01-harness`](evidence/01-harness/SUMMARY.md) |
| harness sensitivity                       | 10 of 35 planted failures missed                                  | same |
| claim ledger                              | 150 claims: 141 verified, 9 contradicted, 0 without evidence      | [`02-claims`](evidence/02-claims/) |
| spec examples                             | 85 of 141 accept or mixed examples compile; 82 run cleanly        | [`02-coverage`](evidence/02-coverage/SUMMARY.md) |
| test-case quality                         | 327/342 fixtures misplaced; 69/140 fixture markers use non-spec codes; 121 reject cases lack a nearby accept twin; stub runner: 239/551 fail with relative paths | [`01-test-quality`](evidence/01-test-quality/SUMMARY.md) |
| spec edits in the MVP window              | 7 hunks: 0 relaxed; 179 fixtures touched, 0 weakened              | [`01-spec-edits`](evidence/01-spec-edits/SUMMARY.md) |
| fuzzing                                   | 30,000 generated cases plus 1,000 cross-implementation cases: 0 phase-consistency violations, 0 cross-implementation disagreements, 17 findings (4 of them in the reference parser) | [`03-fuzz`](evidence/03-fuzz/SUMMARY.md) |
| findings                                  | 112 filed (1 blocker, 31 major, 60 minor, 20 note); about 96 after merging cross-worker duplicates | [`findings-table.md`](evidence/findings-table.md) |

Blind pass rate by area:

| Area                         | Pass |
| ---------------------------- | ---- |
| 1 evaluation order           | 7/7  |
| 2 cancellation and `defer`   | 8/8  |
| 3 requirement rows           | 7/7  |
| 4 erasure boundaries         | 3/8  |
| 5 identity                   | 3/6  |
| 6 permission weakening       | 2/6  |
| 7 iterator invalidation      | 6/6  |
| 8 defaults and copy-update   | 3/5  |
| 9 strings                    | 4/7  |

None of the 17 blind failures was a fixture error:

- 9 are implementation bugs;
- 9 are unsupported features (6 with a structured diagnostic, 3 with only a
  parse error);
- 3 are spec ambiguities.

Some fixtures have more than one cause.

## 3. Coverage by Chapter

| Chapter   | Fixtures | Selected | Selected passing | Unselected passing | Deferred | In-scope failing | Spec examples compiling |
| --------- | -------: | -------: | ---------------: | -----------------: | -------: | ---------------: | ----------------------: |
| 01        | 11       | 10       | 10               | 0                  | 0        | 1                | 4/5                     |
| 02        | 17       | 12       | 12               | 0                  | 1        | 4                |                         |
| 03        | 8        | 7        | 7                | 0                  | 0        | 1                | 10/12                   |
| 04        | 46       | 30       | 30               | 0                  | 6        | 10               | 6/14                    |
| 05        | 30       | 28       | 28               | 0                  | 0        | 2                | 12/13                   |
| 06        | 32       | 32       | 32               | 0                  | 0        | 0                | 7/7                     |
| 07        | 18       | 16       | 16               | 0                  | 0        | 2                | 15/20                   |
| 08        | 18       | 18       | 18               | 0                  | 0        | 0                | 10/14                   |
| 09        | 25       | 16       | 16               | 1                  | 7        | 1                | 10/13                   |
| 10        | 17       | 14       | 14               | 0                  | 0        | 3                | 1/8                     |
| 11        | 26       | 25       | 25               | 0                  | 1        | 0                | 10/10                   |
| 12        | 6        | 0        | 0                | 0                  | 6        | 0                | 0/3                     |
| 13        | 3        | 0        | 0                | 0                  | 2        | 1                | 0/5                     |
| 14        | 29       | 1        | 1                | 0                  | 28       | 0                | 0/17                    |
| **Total** | 286      | 209      | 209              | 1                  | 51       | 25               | 85/141                  |

The 25 in-scope failures:

| Cause                                           | Cases | Finding |
| ----------------------------------------------- | ----- | ------- |
| sized numeric types                             | 8     | F-253   |
| parser gaps                                     | 4     | F-252   |
| `type` and local declarations                   | 4     | F-254   |
| prelude names                                   | 3     | F-255   |
| row-kinded arguments                            | 2     | F-256   |
| unsound accept                                  | 1     | F-251   |
| `mut fn`                                        | 1     | F-257   |
| wrong code                                      | 1     | F-258   |
| missing profile                                 | 1     | F-259   |

60 substantive spec sections have no fixture at all, and 17 are covered only
by `test/fixtures` ([`01-test-quality`](evidence/01-test-quality/SUMMARY.md)).

## 4. Architecture

### 4.1 Runtime representation

Evidence: [`05-object-model`](evidence/05-object-model/SUMMARY.md),
[`05-requirements`](evidence/05-requirements/SUMMARY.md).

| Construct            | Representation                                               | Cost                                              |
| -------------------- | ------------------------------------------------------------ | ------------------------------------------------- |
| data                 | typed GC struct; all fields `mut`                            | 1 allocation; string literals re-allocated per evaluation |
| generic data         | one erased struct with `anyref` fields                       | a box per primitive; a cast and unbox per read     |
| enum                 | one flat struct per enum: tag plus the union of all variant fields | `match` is a linear if-chain on the tag      |
| fieldless variant    | global singleton                                             | 0 allocations                                     |
| `T?`, `Result`       | shared `{tag, anyref}` variant                               | `nil` allocates; a present `i32` costs 2 allocations |
| tuple                | `anyref` array                                               | a box per scalar; a cast per read                  |
| `list[T]`            | vector plus `anyref` array                                   | a box per element, even for concrete `list[i32]`  |
| `map[K, V]`          | parallel arrays, no hashing                                  | O(n) `get` and insert; O(n²) build                |
| string               | UTF-8 byte array                                             | `len` is O(n); interpolation concatenates pairwise |
| closure              | `{funcref, env}`                                             | 2 allocations, even with no captures              |
| dynamic trait value  | `{value, bounds, methods...}`                                | method table copied into each value               |
| stored `Suspend[T]`  | frame plus a 4-field wrapper                                 | a poll goes through `call_ref` and a cast adapter; the result is boxed |

Measured costs:

- **Requirements:** concrete rows add 0 ns per call (5.6 ns for 0, 1, 5, or
  10 requirements). Row-generic callbacks are quadratic: 13, 101, and 417 ns
  for 1, 5, and 10 requirements (F-550).
- **Erasure:**
  - generic sum is 3 to 7 times slower than concrete sum;
  - a generic list sum against the same sum over a concrete `list[i32]` is
    only 1.07 times, because concrete lists are boxed too;
  - dictionaries are rebuilt on every call: 2 allocations for a bounded call,
    3 through a supertrait, 8 through a blanket implementation (F-502);
  - NaN, -0, and extreme values survive boxing bit for bit (33 of 33 checks).
- **Suspension:**
  - an immediately ready `fn!` call costs about 13 times a plain call, even
    at `-O2` (F-554);
  - a poll is O(depth), about 120 ns per level;
  - 48 bang calls in one function produce 549 KB of Wasm (F-552);
  - frames keep every local, and liveness would remove 75 to 100% of slots;
  - cancellation order matches the spec.
- **Runtime library:** always inlined; an empty `main` carries 27 unused
  functions (F-557).
- **Host boundary:** narrow but unversioned; strings cross one byte per call
  (1 MiB takes 374 ms, F-558).
- **Optimizer headroom:** the dev build runs 1.49 times slower than `-O2` on
  scalar code, and pair ratios are unchanged at `-O2`. V8 already removes
  most emission waste; the remaining costs come from the representation.
- **Edit loop:** in-process compilation has an 11 ms median and never
  exceeds 1 s. Command-line wall time has a 687 ms median, and 136 of 551
  invocations exceed 1 s, dominated by loading Binaryen (F-560).

**Erasure verdict:** uniform erasure was the right MVP choice. As
implemented, it is not a good long-term default. It needs:

- static dictionaries;
- unboxed optional, tuple, and scalar-list storage where no generic sharing
  occurs;
- selective specialization, with erasure kept as the fallback.

The HIR already carries most of the fields specialization needs. Two gaps
remain: calls have no explicit type-argument list, and types are strings.

### 4.2 Compiler architecture

Evidence: [`06-compiler`](evidence/06-compiler/SUMMARY.md).

- **HIR:** fully typed and resolved, and the emitter never touches the AST.
  But `ValueType` is a string, re-parsed at about 60 emitter sites and
  resolved by bare name (F-607).
- **Desugaring** is split across parser, checker, and emitter. Control flow
  is lowered three times: plain, linear-suspension, and control-flow
  suspension (F-609).
- **Middle IR:** missing. `SuspensionPlan` is a partial control-flow IR for
  suspending functions only, without liveness. Boxing and adapters are
  decided while WAT text is written.
- **Structure:**
  - the checker is a 12-class inheritance chain with about 30 mutable
    fields;
  - 13 functions sit at 250 to 299 lines against the 300 cap;
  - two files are at 97 to 99% of the 1,500-line cap;
  - duplication is 31.5% within `emitter.ts`;
  - folder boundaries are clean (0 violations).
- **Error recovery:** one error per function body; a signature error hides
  every body error (F-605).

| Stage                 | Parallel readiness                                   |
| --------------------- | ---------------------------------------------------- |
| lex and parse         | ready                                                |
| signature collection  | ready (cheap serial prepass)                         |
| body checking         | needs refactor (global closure numbering, `globals`) |
| module-init check     | needs refactor; its whole-module graph is a language rule |
| emission              | needs refactor (global registries)                   |
| assembly              | blocked by design: one WAT string, 85 to 90% of time |

**Incremental readiness:** there is no cache, so a one-line edit costs the
same as a cold build. Inserting one function changes 308 of 628 WAT
functions (F-608). Row inference is local to closures, not a whole-program
fixpoint, because named functions declare rows. That language rule helps
incremental compilation.

**Scaling hazards:** the module-initialization check is exponential in call
depth (8.6 s at depth 22, F-602). Nested unannotated closures double check
time per level (F-604).

**HIR verdict:** the design makes sense for a single-file MVP. Before
multi-module or incremental work, it needs:

- interned structured types;
- stable declaration identities;
- one shared lowered IR.

## 5. Key Findings

Ranked by impact. Duplicates found by several workers are merged under one
canonical ID, and the other IDs are listed. The complete list of 95 is in
[`evidence/findings-table.md`](evidence/findings-table.md).

### Correctness

| Rank | ID                    | Severity | Finding                                                                                     |
| ---- | --------------------- | -------- | ------------------------------------------------------------------------------------------- |
| 1    | F-351 (F-251)         | major    | a plain closure mutates a captured `mut` value; accepted and executed. `mutable-capture-requires-mut-fn` is never produced. Reproduced. |
| 2    | F-603                 | major    | module-initialization check misses bounded and dynamic trait dispatch; the program reads a zero global. Reproduced (prints `5`). |
| 2a   | F-304                 | major    | `Ok`/`Err` payload types are unchecked: `Ok("text")` for `Result[i32, string]` passes `check`, then fails Wasm validation. Reproduced. |
| 2b   | F-305, F-307 (F-559)  | major    | type-checked programs that cannot run: `_ := f()?` on `Result[void, E]` emits unparseable WAT; a `main` returning `Result[void, E]` has no runnable export |
| 3    | F-160 (F-262)         | major    | any `f64` `<`, `<=`, `>`, `>=` crashes code generation (missing `)` in a WAT template). Reproduced. One worker rated it blocker; it is a one-line fix, but no fixture covered it. |
| 4    | F-500 (F-350, F-263)  | major    | primitives do not satisfy `PartialEq`, `PartialOrd`, or `Display` bounds. Reproduced.      |
| 5    | F-201                 | major    | the compiler accepts undeclared requirement keys, in 29 fixtures                            |
| 6    | F-403                 | major    | `hd test` shares one instance across `main` and all test blocks, against chapter 02        |
| 7    | F-252                 | major    | the parser rejects core forms the reference parser accepts: single-line `if`/`else`, `use self.`/`super.`, unnamed payloads |
| 8    | F-163                 | major    | `xs == [1, 2]` is rejected with `type-mismatch`                                            |
| 9    | F-261 (F-353)         | major    | `is` rejects operands with `mut` access                                                    |
| 10   | F-400                 | major    | a leading U+FEFF is lost at the host boundary and in replay                                 |
| 11   | F-401                 | minor    | replay accepts a changed executed non-suspending function and prints a different result     |

### Test integrity and portability

| Rank | ID    | Severity | Finding                                                                                      |
| ---- | ----- | -------- | -------------------------------------------------------------------------------------------- |
| 12   | F-250 | major    | deferred features get generic or wrong diagnostics (MVP goal 5 not met)                      |
| 13   | F-153 | major    | `# panic: runtime-error` accepts any crash, and hid a raw trap for `2 ** -1`                 |
| 14   | F-200 | major    | the fixture format and command contract are defined outside `spec/`                          |
| 15   | F-205, F-313 | major | half the `test/fixtures` reject markers, and 58.5% of fuzzed `check` rejections, use codes the spec does not define (48 codes) |
| 16   | F-202 | major    | `expect-result` compares JavaScript formatting of Wasm values                                |
| 17   | F-150, F-151, F-152, F-154, F-155 | minor | missed harness plants: extra errors, a same-code warning, panic codes anywhere in output, scenario test blocks, unchecked panic lines |
| 18   | F-206, F-300, F-358, F-359, F-360 | minor to major | reference-parser bugs: `\u{...}`, nested function types, literals in `${...}`, nested named arguments, `$.with` spread order |

### Performance and architecture

| Rank | ID    | Severity | Finding                                                                        |
| ---- | ----- | -------- | ------------------------------------------------------------------------------ |
| 19   | F-602 | major    | the module-initialization check is exponential in call depth                   |
| 20   | F-552 | major    | suspension code size grows super-linearly with bang-call sites                 |
| 21   | F-501 | major    | maps have no hashing                                                           |
| 22   | F-502 | major    | dictionaries are rebuilt per call, plus a trait value per method call          |
| 23   | F-550 | major    | the row-generic callback adapter copies the provider pack once per lookup      |

Severity totals as filed: 1 blocker, 31 major, 60 minor, 20 note (112
files).
Cross-worker duplicates:

- F-160 = F-262;
- F-351 = F-251;
- F-500 = F-350 = F-263;
- F-353 = F-261;
- F-611 = F-264;
- F-162 = F-265;
- F-356 = F-267;
- F-352 and F-357 = F-257;
- F-210 overlaps F-266;
- F-206 = F-301 (and part of F-303);
- F-360 = F-302;
- F-252 = F-309;
- F-405 = F-312;
- F-162 = F-306.

## 6. Fuzzing

Evidence: [`03-fuzz`](evidence/03-fuzz/SUMMARY.md); tool:
[`fuzz/`](fuzz/README.md).

The fuzzer is implementation-neutral: it drives any compiler through the
command contract in [`fuzz/CONTRACT.md`](fuzz/CONTRACT.md) and imports
nothing from `src/`. It ran two seeded rounds of 5,000 cases for each of
four fuzzers, plus 1,000 cross-implementation cases:

| Fuzzer                 | Round 1 signatures | Round 2 signatures | Result                                                        |
| ---------------------- | ------------------ | ------------------ | ------------------------------------------------------------- |
| parse agreement        | 19                 | 18                 | disagreements with the reference parser in both directions    |
| contract               | 13                 | 11                 | crashes and codes outside the spec inventory                  |
| phase consistency      | 0                  | 0                  | `parse`, `check`, and `run` never contradict each other       |
| Wasm validity          | 3                  | 3                  | type-checked programs that fail Binaryen validation           |
| cross-implementation   | 0 (1,000 cases)    |                    | a negative control produced 20 of 20 disagreements, as intended |

- Round 2 repeated 16 of 19 parse classes and every contract and Wasm
  class, so the common failure classes are exhausted.
- 58.5% of `check` rejections carry one of 48 codes missing from the spec
  inventory (F-313).
- Implementation bugs:
  - F-304: `Ok`/`Err` payload types are unchecked;
  - F-305: `_ := f()?` on `Result[void, E]` emits unparseable WAT;
  - F-307: a `main` returning `Result[void, E]` passes `check` but cannot
    run;
  - F-308, F-310, F-311, F-316: smaller front-end bugs.
- Reference-parser bugs: F-300 (nested function types, an Earley
  completion bug), plus duplicates of F-206 and F-360.
- The grammar itself derives same-line-suite forms that layout cannot
  produce (F-314).
- Minimized findings are portable `.hd` fixtures in
  [`fuzz/findings/`](fuzz/findings/), ready to promote into
  `spec/conformance/`.

## 7. Specification Feedback

The full decision log, with evidence and a recommendation for each entry,
is [`evidence/07-spec-feedback/DECISION_LOG.md`](evidence/07-spec-feedback/DECISION_LOG.md).
The drafted fixture-format text is
[`proposals/fixture-format.md`](proposals/fixture-format.md).

Recommended spec changes (clarifications, no new features):

1. Apply the fixture-format proposal, so the conformance suite is fully
   defined by `spec/`, including the command contract (A1, A2).
2. Remove `runtime-error` as a conformance panic expectation (A5).
3. State that a missing requirement clause on a named function means the
   empty row (B1).
4. State that suspension frame contents are not observable, so liveness
   optimization is permitted (C4).
5. Fix the reference-parser bugs (four found by the blind author, plus
   F-300 from fuzzing), and add fixtures for them (A11).
6. Reconcile the chapter-02 EBNF with the layout rules (A14).
7. Promote the 107 ready language fixtures from `test/fixtures` into
   `spec/conformance/`, and add fixtures for the 60 uncovered sections (A12).

Spec rules to keep, where the implementation must change: B2 (the
initialization rule itself), B3, B4, B5.

Found not to apply: no rule was invented by the implementation. Float
display, map order, `nil` ordering, and the panic list all predate `src/`.

## 8. Open Questions for `OPEN_ISSUES.md`

Phrased as questions; none is a default to implement. Each has evidence in
the decision log.

1. Should the spec define each diagnostic's anchor location, or should
   runners accept any line in the construct's span? (A3)
2. Must a reject case produce only its marked diagnostic? (A4)
3. Should panic reports carry a checkable source location? (A6)
4. Should the spec define a test-driver interface in source terms instead
   of named harness scenarios? (A7)
5. Which implementation codes join the inventory (`type-mismatch`,
   `unknown-name`, `unknown-type`, `argument-count`, `missing-display`,
   `impure-parameter-default`, `generic-type-mismatch`)? (A8)
6. Should there be a portable "unsupported by this implementation"
   category that runners treat as skipped? (A9)
7. Should expected values use `Display` rendering, or only
   `assert_equal`? (A10)
8. Module initialization: does trait dispatch include bounded and dynamic
   dispatch, and does "later function bodies" mean source or execution
   order? (B2)
9. May defaults read reassignable top-level bindings? Should the `pure`
   qualifier return? (B6)
10. Does the owner confirm the three issues closed in `1e55fc9` (provider
    escape, `pure`, `all!`/`race!` intrinsics)? (B7)
11. Which construct raises `failed-checked-cast`, and which category covers a
    negative exponent? (B8)
12. Are shared-data variants canonical? Is `let x: mut T = keep(readonly)` a
    `mutable-upgrade`? What are compatible composite reference types for
    `is`? (B9)
13. Is distinct identity for fieldless data intended? (B10)
14. Does chapter 04 (erasure is normative) or chapter 09 ("may
    monomorphize") govern, and may identity-free values use specialized
    layouts? (C1)
15. May a statically known `Suspend[T]` frame skip the uniform wrapper? (C2)
16. May row packs be positional under a canonical key order? (C3)
17. Should map operation and `string.len()` complexity be specified? (C5)
18. Should replay code identity be transitive and normalized? (D1)
19. Should panicking runs produce histories, and what happens past a
    history's end? (D2)
20. Is the runtime profile part of the provider-configuration identity? (D3)
21. Is a busy-polling entry driver conforming? (D4)
22. Should the runtime poison an instance after a panic? (D5)
23. Should the spec define a development host boundary, or only the
    Component Model boundary? (D6)
24. Which diagnostics may `parse` report, as opposed to `check`? (A13)
25. Is a non-`pub` `main` an entry point, which `main` result types are
    runnable, and what do `run` and `test` do on a library file? (B11)
26. Should the fuzzer move to `spec/tools/`, with a seeded smoke run in
    `spec/check.sh`? (A15)

## 9. Process Notes

- **Workers.** Eleven audit workers ran: ten in parallel, one per plan
  section (the blind author among them), then a separate blind runner. Workers could not write
  report files, so the coordinator wrote each `SUMMARY.md` from the worker's
  final report. The coordinator independently reproduced F-160, F-351,
  F-500, and F-603.
- **Baseline correction.** The plan's baseline said about 1,200 lines of
  spec text changed. The real figure is +25/-10 in the numbered chapters
  ([`00-baseline/CORRECTIONS.md`](evidence/00-baseline/CORRECTIONS.md)).
- **Timings** ran on a contended CPU (load 57 to 76 on 14 cores). Ratios
  within a run are reliable; absolute times should be re-measured on an idle
  machine.
- **Changes outside `audit/`:**
  - `examples/core.hd` was modified at 16:59 so that `main` tests
    `1.0 < 2.0`. No audit worker made the edit, and the coordinator left it
    untouched.
  - Someone restored it before the audit ended; the working tree is clean
    apart from `:w`.
  - A stray `case.wasm`, and an `x.wasm`, came from the fuzzer's early
    smoke runs. The fuzz worker deleted both and fixed its Wasm adapter to
    build in a fresh directory.
  - A root file named `:w` predates the audit.
- **Not done:** mutation testing, which was out of scope. The other gaps are
  listed under "Not Done" in each evidence summary.
