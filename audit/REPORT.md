# MVP Audit Report

**Subject:** the Wasm GC MVP compiler in `src/`, commit `bd985d7`.
**Date:** 2026-09-25.
Every number below comes from a command that was run at that commit. The
evidence folder for each section is linked in place.

**Since the audit:** the conformance work fixed every test-integrity,
fixture-format, and reference-parser finding, and the owner's decisions are
in the specification. This report now keeps only what is still open. The
current compiler status is 620 of 767 conformance cases passing; each of the
172 failures is listed in `test/portable/KNOWN_FAILURES.tsv` with a finding or
decision ID, and [`evidence/w9/failures-by-id.tsv`](evidence/w9/failures-by-id.tsv)
groups them. Decision IDs are in [`README.md`](README.md).

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
  `PartialOrd`, or `Display` bounds.

All of these have since been fixed in the prototype (F-351, F-603, F-304,
F-160, F-500).

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
| claim ledger                              | 150 claims: 141 verified, 9 contradicted, 0 without evidence      | [`02-claims`](evidence/02-claims/) |
| spec examples                             | 85 of 141 accept or mixed examples compile; 82 run cleanly        | [`02-coverage`](evidence/02-coverage/SUMMARY.md) |
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

## 3. Architecture

### 3.1 Runtime representation

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

### 3.2 Compiler architecture

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
depth (8.6 s at depth 22, F-602, since fixed: 1 ms). Nested unannotated closures double check
time per level (F-604).

**HIR verdict:** the design makes sense for a single-file MVP. Before
multi-module or incremental work, it needs:

- interned structured types;
- stable declaration identities;
- one shared lowered IR.

## 4. Key Findings

Ranked by impact. Duplicates found by several workers are merged under one
canonical ID, and the other IDs are listed. The 89 findings still open are in
[`evidence/findings-table.md`](evidence/findings-table.md).

### Correctness

| Rank | ID                    | Severity | Finding                                                                                     |
| ---- | --------------------- | -------- | ------------------------------------------------------------------------------------------- |
| 1    | F-201                 | major    | the compiler accepts undeclared requirement keys, in 29 fixtures                            |
| 2    | F-403                 | major    | `hd test` shares one instance across `main` and all test blocks, against chapter 02        |
| 3    | F-252                 | major    | the parser rejects core forms the reference parser accepts: single-line `if`/`else`, `use self.`/`super.`, unnamed payloads |
| 4    | F-163                 | major    | `xs == [1, 2]` is rejected with `type-mismatch`                                            |
| 5    | F-261 (F-353)         | major    | `is` rejects operands with `mut` access                                                    |
| 6    | F-400                 | major    | a leading U+FEFF is lost at the host boundary and in replay                                 |
| 7    | F-401                 | minor    | replay accepts a changed executed non-suspending function and prints a different result     |

### Unimplemented features and codes

| Rank | ID    | Severity | Finding                                                                                      |
| ---- | ----- | -------- | -------------------------------------------------------------------------------------------- |
| 8    | F-250 | major    | deferred features get generic or wrong diagnostics (MVP goal 5 not met)                      |
| 9    | F-205 | major    | the compiler emits 48 codes the spec does not define                                         |
| 10   | F-155 | minor    | runtime panics carry no source location                                                      |

### Performance and architecture

| Rank | ID    | Severity | Finding                                                                        |
| ---- | ----- | -------- | ------------------------------------------------------------------------------ |
| 11   | F-552 | major    | suspension code size grows super-linearly with bang-call sites                 |
| 12   | F-501 | major    | maps have no hashing                                                           |
| 13   | F-502 | major    | dictionaries are rebuilt per call, plus a trait value per method call          |
| 14   | F-550 | major    | the row-generic callback adapter copies the provider pack once per lookup      |

Cross-worker duplicates:

- F-353 = F-261;
- F-611 = F-264;
- F-162 = F-265;
- F-356 = F-267;
- F-352 and F-357 = F-257;
- F-252 = F-309;
- F-162 = F-306.

## 5. Fuzzing

Evidence: [`03-fuzz`](evidence/03-fuzz/SUMMARY.md); tool, now in the spec:
[`spec/tools/fuzz/`](../spec/tools/fuzz/README.md).

The fuzzer is implementation-neutral: it drives any compiler through the
command contract in [`spec/conformance/README.md`](../spec/conformance/README.md) and imports
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
  inventory (F-205).
- Implementation bugs still open: F-308, F-310, F-311, F-316 (front end).
- The grammar itself derives same-line-suite forms that layout cannot
  produce (F-314).
- Minimized findings are portable `.hd` fixtures in
  [`evidence/03-fuzz/findings/`](evidence/03-fuzz/findings/); the ones the
  spec settles are now conformance cases.
