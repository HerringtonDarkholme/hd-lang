# MVP Audit Plan

**Subject:** the Wasm GC MVP compiler in `src/`, as of commit `bd985d7`
(2026-09-25).
**Goal:** decide how much of the MVP's claimed behavior is real, whether its
architecture can carry the language forward, and what the implementation
teaches us about the specification.

## Ground Rules

1. **Execution is the evidence.** Every verdict cites something that was run:
   a command, its output, a fixture, a benchmark, or a WAT excerpt. Reading
   source can locate a mechanism; it cannot confirm behavior. A claim that
   was only read is labeled `UNVERIFIED`.
2. **All audit work lives in `audit/`.** Probe programs, blind fixtures,
   benchmarks, scripts, logs, and reports go here. The audit does not edit
   `src/`, `spec/`, `test/`, or `guide/`. No mutation testing: the audit never
   modifies compiler source, in this tree or a worktree.
3. **Record the run.** Each evidence file notes the commit, the command, and
   the date. Rerunning any step must reproduce its result.
4. **Findings, not fixes.** The audit reports defects and design questions.
   Design choices become candidate `OPEN_ISSUES.md` entries phrased as open
   questions, not defaults to implement.
5. **Independence.** Blind fixtures (phase 3) are written from the spec text
   alone, by an author who has not read `src/` or `test/`.

## Folder Layout

```text
audit/
  PLAN.md                  this file
  REPORT.md                final report (written last)
  evidence/NN-name/        raw outputs per step: logs, tables, WAT excerpts
  probes/                  small .hd programs written to test one claim
  blind/                   phase-3 fixtures written from the spec only
  bench/                   phase-5.7 direction benchmarks and runner
  scripts/                 audit harness scripts (Node + strip-types)
  fuzz/                    implementation-neutral fuzzer (see 3.2)
  proposals/               drafted spec text for the owner to apply
  findings/                one file per confirmed defect or design issue
```

Scripts under `audit/scripts/` are TypeScript run with
`node --experimental-strip-types`, matching the repository. They must pass
`oxlint`, because `npm run lint` lints the whole tree.

Each finding file uses this shape:

```text
# F-NNN: short title
Severity: blocker | major | minor | note
Area: correctness | coverage | test-integrity | runtime | architecture | spec
Evidence: audit/evidence/...  (command + output)
Effect: what a user or implementer sees
Recommendation: implementation change | spec change | OPEN_ISSUES question
```

## Baseline (already recorded)

Measured on `bd985d7`, stored in `audit/evidence/00-baseline/`:

- `npm run check` passes in about 62 s: types, lint, format, 173 TypeScript
  tests, 209 selected portable cases, and `spec/check.sh`.
- `spec/conformance/cases.tsv` has 286 cases; `test/portable/cases.tsv`
  selects 209.
- Of the 77 unselected cases, 76 fail under `hd check`. Most failures are
  generic parse errors (`expected-expression`, `expected-token`,
  `unknown-type`), not structured "unsupported feature" diagnostics.
- `typing/valid/traits.hd` passes but is not selected.
- The last two commits changed 194 files, including about 1,200 lines of
  specification text (mostly chapter 14) and many fixture markers.
- `.oxlintrc.json` caps files at 1,500 lines; `emitter/function-body.ts`
  (1,487) and `emitter/emitter.ts` (1,460) sit just under the cap.

Step 0 reruns this baseline and saves the full logs before anything else.

---

## Phase 1: Test Integrity

The same agent wrote the compiler, its tests, and recent specification edits.
Test results mean little until the tests are shown to be honest.

### 1.1 Specification edit review

- Diff every numbered chapter (`spec/[0-9][0-9]-*.md`) from the commit before
  MVP work began to `bd985d7`. Identify that base commit from `git log` on
  `src/` and record it.
- Classify each hunk: `clarification`, `new rule`, `rule relaxed`,
  `rule tightened`, or `editorial`.
- For each `new rule` or `rule relaxed` hunk, check whether the implementation
  depends on it. If a rule was written to match the code, file a finding.
- Output: `evidence/01-spec-edits/hunks.tsv` with chapter, hunk, class,
  dependent source file, verdict.

### 1.2 Fixture marker review

- For every fixture added or changed in the MVP window, confirm that the cited
  section in `cases.tsv` requires that diagnostic or panic code on that line.
- Flag fixtures whose expectation is weaker than the spec. Examples: an
  `accept` where the spec says `reject`, or a generic code where the spec
  names a specific one.
- Output: `evidence/01-fixtures/markers.tsv`.

### 1.3 Harness sensitivity

Plant known failures in copies of fixtures under `audit/probes/harness/` and
run them through `test/run-portable.ts` logic (a copied manifest). The harness
must fail on each of these:

| Plant                                             | Expected harness verdict |
| ------------------------------------------------- | ------------------------ |
| right diagnostic code, wrong line                 | fail                     |
| wrong diagnostic code, right line                 | fail                     |
| extra unexpected error in a reject case           | fail                     |
| accept case that emits a warning                  | decide and record        |
| panic case that ends in a generic `runtime-error` | fail                     |
| `expect-result` with the wrong value              | fail                     |
| runtime case that never runs its test blocks      | fail                     |
| crash (stack trace) instead of a diagnostic       | fail                     |

Also confirm by running each phase that parse fixtures are not only parsed
when the harness claims to type-check or execute them.

### 1.4 Coverage gaps without mutation

Mutation testing is out of scope. Coverage gaps are found from the outside
instead:

- the claim ledger (2.3) marks claims with no exercising fixture;
- blind fixtures (3.1) test behavior the authors of the suite did not choose;
- for each high-risk mechanism below, write one probe that would fail if the
  mechanism were missing, and confirm it passes:
  - permission and readonly checks;
  - argument evaluation order;
  - `defer` execution on cancellation;
  - the one-shot suspension state check;
  - row subtraction;
  - the `f64` NaN branch in ordering;
  - the iterator version check.
- Output: `evidence/01-gap-probes/results.tsv`.

### 1.5 Special-casing scan

- Search `src/` for fixture names, fixture-specific identifiers, and file-path
  checks that could make a fixture pass without general behavior.
- Search for `TODO`, `unsupported`, `not implemented`, and `throw new Error`
  sites. List what each one covers.

### 1.6 Test case quality and portability

The test cases should outlive this compiler, like the fuzzer (3.2). This step
checks whether they can, and whether each one tests what it claims.

**Inventory.** There are three test bodies today:

| Body                    | Count         | Intended role                                    |
| ----------------------- | ------------- | ------------------------------------------------ |
| `spec/conformance/`     | 286 fixtures  | language contract for every implementation       |
| `test/fixtures/`        | 342 fixtures  | implementation fixtures run by the portable runner |
| TypeScript tests        | 173 tests     | implementation details only (AST, HIR, WAT, host) |

Classify every `test/fixtures/` file and every TypeScript test as `language
behavior` or `implementation detail`. A language-behavior case outside
`spec/conformance/` is misplaced. `test/README.md` already states this rule;
the audit measures how well it is followed. Output: a list of cases to
promote.

**Portability of the fixture format:**

- List every directive in use and where it is defined. Today:
  - `# expect-result:` (169 uses) and `# expect:` (62 uses) are documented
    only in `test/README.md`;
  - `fixture-runtime-profile`, `fixture-runtime-scenario`,
    `fixture-runtime-pending-function`, and `fixture-package-role` are
    documented in `spec/conformance/README.md`.

  A directive another implementation cannot learn from `spec/` is a
  portability gap.
- For each harness hook (profiles, scenarios, pending functions, package
  roles), check whether its description is specific enough for a new
  implementation to build without reading `test/run-portable.ts` or `src/`.
- Check self-containment. Each fixture must use only primitives, prelude
  names, and its own declarations, as `spec/conformance/README.md` requires.
  Flag reliance on implementation-only names, file layout, or working
  directory.
- Check determinism. Flag expectations that depend on unspecified behavior:
  float formatting beyond the spec, map order where the spec is silent, exact
  message text, or timing.

**Harness portability, by running it.** Point the portable runner at an
implementation other than this compiler. Use `HD_TEST_COMMAND`:

1. Write `audit/scripts/stub-hd.ts`. It implements `parse` with the
   reference parser and prints `unsupported-feature` for `check`, `run`, and
   `test`.
2. Run `test/run-portable.ts` with the stub.
3. Every parse case should pass. Every other case should fail cleanly with
   an unsupported result.
4. Anything else shows hidden coupling: path assumptions, `bin/hd.js`
   defaults, output parsing that fits only this compiler, or runtime cases
   that inspect WAT or HIR.

**Oracle strength:**

- **Reject cases:** delete or neutralize the marked line in a copy. The
  fixture should then be accepted, or at least lose that diagnostic. If the
  code still appears, the marker is incidental, and the fixture does not
  isolate the rule it names.
- **Reject cases:** check that the marked diagnostic is the only error. Extra
  errors make the case pass for the wrong reason on another implementation.
- **Runtime accept cases:** count those with no observation (no `assert`,
  `assert_equal`, `expect-result`, or checked cleanup result). A case that
  only proves "does not crash" is weak; list them.
- **Pairs:** for each reject rule, check that an accept fixture sits near the
  boundary (the closest valid program). Without it, an implementation that
  rejects too much still passes.
- **Scope:** one behavior per fixture, and a `# test:` name that states it.
  Flag fixtures that test several rules at once.

**Redundancy and gaps.** Group fixtures by spec section, from `cases.tsv` and
by reading `test/fixtures/`. Report sections with no fixture, and clusters of
near-duplicates that add count but not coverage.

**Output:** `evidence/01-test-quality/`, containing:

- `inventory.tsv`: every case, with its body, class, and promotion target;
- `directives.tsv`: each directive, its definition site, and portability;
- `oracle.tsv`: per-fixture results of the oracle-strength checks;
- the stub-runner log.

Findings go in `findings/` as usual.

## Phase 2: Coverage Against The Specification

### 2.1 Classify the unselected cases

Rerun all 77 unselected cases through the matching command (`hd parse`,
`hd check`, or `hd test`). Put each in one class:

- **deferred:** in the plan's deferred list (GADTs, packs, variance,
  multi-package, annotations). It should get a stable unsupported diagnostic.
- **in scope, failing:** a defect. File a finding.
- **passing, unselected:** should join the portable selection.

For deferred cases, record whether the diagnostic is structured or a generic
parse or type error. MVP goal 5 requires structured rejection.

### 2.2 Slice gates

Each slice in `src/MVP_IMPLEMENTATION_PLAN.md` has a "Done when" sentence.
Name the fixtures that demonstrate each gate and run them:

| Slice | Gate to demonstrate                                                                                  |
| ----- | ---------------------------------------------------------------------------------------------------- |
| S0    | selected parse fixtures agree with `cases.tsv`; AST snapshot is stable across two runs               |
| S1    | curated core runtime cases compile to Wasm GC and execute in Node                                    |
| S2    | chapter-11 provider examples run; missing-row, subtraction, and key-collision diagnostics appear     |
| S3    | provider capture at construction; cancellation runs `defer` from innermost frame outward             |
| S4    | selected trait fixtures and erased collection programs run                                           |
| S5    | record/replay round trip; replay rejects a changed executed function; survives unrelated insertions  |

A gate with no demonstrating fixture gets a probe written in
`audit/probes/gates/`.

### 2.3 Claim ledger

- Extract every specific behavioral claim from the status section of
  `src/MVP_IMPLEMENTATION_PLAN.md` and the "Implemented Surface" list in
  `src/README.md`. Expect about 100 claims.
- For each claim, link a fixture or probe that exercises it, run it, and
  record `verified`, `contradicted`, or `no evidence`.
- Output: `evidence/02-claims/ledger.tsv`. A `no evidence` claim gets a probe.

### 2.4 Specification examples

`spec/conformance/examples.tsv` maps each `text` fence to fixtures. Run every
`accept` or `mixed` example through the full pipeline, not only the parser
oracle. Report a coverage table per chapter: examples, runnable, passing.

## Phase 3: Independent Tests

### 3.1 Blind fixtures

An author who has not read `src/` or `test/` writes 40 to 60 fixtures from the
spec text, using the portable fixture format. Target areas where bugs are
likely:

- evaluation order across suspension, short-circuiting, and named arguments;
- cancellation while a `defer` suite runs, and nested frame unwinding;
- row union, subtraction, and lexical restoration of removed providers;
- erasure boundaries: `f64` NaN, -0, infinities, and `i32` extremes passing
  through generic functions, optionals, lists, and maps;
- reference identity on boxed primitives and canonical fieldless variants;
- permission weakening through generics, fields, and closures;
- iterator invalidation after map removal and list growth;
- defaults that reference earlier parameters, and copy-update evaluation order;
- string edge cases: empty strings, multi-byte scalars, `split` with an empty
  separator, interpolation of `Display` values.

Run them. The pass rate, broken down by area, is the headline quality number.
Each failure is triaged as an implementation bug, a fixture error, or a spec
ambiguity.

### 3.2 Fuzzing

The current compiler is a prototype that will be replaced. The fuzzer is
built as implementation-neutral language tooling that outlives it. The
current compiler is only its first target.

#### Portability rules

1. **The implementation under test is a command.** It is configured the same
   way as `test/run-portable.ts`: `HD_FUZZ_COMMAND` (default
   `node --experimental-strip-types bin/hd.js`) or `--compiler "<cmd>"`. The
   fuzzer never imports `src/`, and never reads the AST, HIR, WAT, or
   TypeScript stack traces.
2. **Oracles come from the language, not the implementation.** Allowed
   sources:
   - the specification's reference parser, imported as
     `parseSource(source)` from `spec/reference-parser/parser.ts`;
   - the diagnostic-code inventory in `spec/README.md`;
   - the panic categories in `spec/06-control-flow.md`;
   - the command contract below.
3. **Inputs are written in the language.** Seeds come from
   `spec/conformance/**/*.hd`. The generator reads the chapter-02 EBNF from
   `spec/02-grammar.md`, not from any parser's internal tables.
4. **Output is written in the language, too.** Each minimized finding is
   saved as a `.hd` file in the portable fixture format (`# test:`,
   `# diagnostic:`, `# panic:` markers). It can then be promoted into
   `spec/conformance/` for every implementation, whoever wrote it.
5. **Backend checks are plugins.** Wasm validation applies only to a Wasm
   backend. It lives behind an optional adapter, so an interpreter or a
   native backend runs the same core fuzzer without it.
6. **N-way comparison.** `--compiler` may be given more than once. With two
   or more implementations, the fuzzer also compares them with each other.
   This is how a future replacement is checked against the current one.

#### Command contract

The fuzzer depends only on this behavior. The contract exists today only in
`test/run-portable.ts` and `test/README.md`. Writing it down in
`audit/fuzz/CONTRACT.md` is part of this step, and it is a phase 7 candidate
for the specification:

| Command         | Success                  | Rejection                                                                  |
| --------------- | ------------------------ | -------------------------------------------------------------------------- |
| `parse FILE`    | exit 0                   | exit 1, at least one `PATH:LINE:COL: CODE: message` line                   |
| `check FILE`    | exit 0, warnings allowed | exit 1, at least one located diagnostic with a code from `spec/README.md`  |
| `run FILE`      | exit 0                   | exit 1, located diagnostic or a panic code from chapter 06                 |

Any other outcome is a **contract violation**:

- a nonzero exit with no located code;
- a code that is not in the spec inventory;
- a signal, or a timeout (10 s);
- exit 0 on `check` while `parse` rejects the same file.

This check replaces "look for a JavaScript stack trace". It works for any
implementation language.

#### Fuzzers

| Fuzzer            | Input                                                                    | Oracle                                                                              |
| ----------------- | ------------------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| parse agreement   | mutated seeds (token delete, duplicate, swap, reindent) and EBNF-generated programs | reference parser and each implementation agree on accept or reject for `parse` |
| contract          | mutated seeds that the reference parser accepts                           | `check` and `run` satisfy the command contract                                      |
| phase consistency | the same inputs                                                          | `parse` rejects, so `check` rejects; `check` rejects, so `run` rejects               |
| cross-impl        | all inputs, when two or more `--compiler` values are given                | implementations agree on outcome class (accept, reject by code, panic by code)      |
| Wasm validity     | inputs accepted by `check`, Wasm adapter only                            | emitted module validates in Binaryen and, if installed, `wasm-tools validate`       |

#### Budget and stopping rules

- Seeded and count-based: 5,000 cases per fuzzer, so a rerun covers the same
  inputs on any machine and with any implementation. At about 0.3 s per CLI
  call and 8 jobs, the whole run takes about 10 minutes against the current
  compiler.
- Group findings by signature: fuzzer, outcome class, and first diagnostic or
  panic code.
- Stop a fuzzer early after 20 distinct signatures.
- After triage, run one more round with a new seed. It shows whether the
  findings were exhausted or only sampled.

#### Triage

- Minimize one example per signature. Delta minimization must keep the
  signature, using the same command contract, so it is portable too.
- Save it under `audit/fuzz/findings/` as a portable fixture.
- Classify it:
  - implementation bug;
  - reference-parser bug;
  - spec ambiguity;
  - input handled correctly by all tools (discarded).

#### Layout

```text
audit/fuzz/
  README.md       how to run it against any implementation
  CONTRACT.md     the command contract above
  fuzz.ts         entry point: --compiler (repeatable), --seed, --cases, --jobs
  mutate.ts       seed mutation operators
  generate.ts     EBNF-driven program generator
  oracles/        parse agreement, contract, phase consistency, cross-impl
  adapters/wasm.ts optional backend adapter
  minimize.ts     contract-preserving delta minimizer
  findings/       minimized portable fixtures
```

It may import only Node built-ins and `spec/`. A check in `audit/fuzz/README.md`
documents this, and a grep for `src/` imports enforces it. When the audit
ends, the folder can move to `spec/tools/` or `test/` unchanged.

#### Limits

The fuzzer tests robustness, parser agreement, phase consistency, and
agreement between implementations. With only one type checker, it cannot
tell whether an accepted program is correctly typed. Cross-implementation
mode closes that gap once a second implementation exists.

## Phase 4: Runtime Behavior

### 4.1 Record and replay

- Round-trip every suspension fixture with `hd record` and `hd replay`.
- Insert an unrelated declaration, then replay. It must succeed.
- Change an executed function body, then replay. It must be rejected.
- Make a formatting-only edit (whitespace, comments), then replay. Record
  whether the function code identity is stable.
- Replay with a changed provider configuration. It must be rejected.
- Round-trip `f64` special values and multi-byte strings through host
  providers.

### 4.2 Panics and host boundary

- Confirm each panic category in chapter 06 has a fixture that produces it
  exactly.
- Confirm a poisoned instance is discarded and a second run is clean.
- Check where runtime panics point. Do they carry a source location?

### 4.3 Diagnostics quality

- Read about 30 diagnostics as a user would. For each, record: does the span
  point at the right token, does the message say what to change, and are
  multiple independent errors reported in one run.

---

## Phase 5: Runtime Architecture

Each item answers a question with WAT excerpts or measured numbers. Probe
programs go in `audit/probes/arch/`; excerpts go in `evidence/05-*/`.

### 5.1 Object model

Build one probe per construct. Record its Wasm representation from
`hd build --wat`:

| Construct                         | Questions                                                              |
| --------------------------------- | ---------------------------------------------------------------------- |
| data                              | struct layout; field mutability; typed or erased fields                |
| generic data                      | one erased layout; box and unbox sites                                 |
| enum                              | tag field versus subtypes; match lowering (`br_table`, casts)          |
| fieldless variant / data          | shared singleton or allocated each time                                |
| `T?` and `Result`                 | is `nil` a null ref; does `i32?` box                                   |
| tuple                             | erased `anyref` array: casts per read, boxing per element              |
| `list[T]`                         | growth policy; element storage; bounds check cost                      |
| `map[K, V]`                       | lookup complexity (linear scan?); hashing; insertion-order cost        |
| string                            | byte array; cost of `len`, index, concat, interpolation (O(n) or n²)   |
| closure                           | environment layout; one allocation per closure creation                |
| dynamic trait value               | receiver plus method refs: size; built once or per call                |
| `Suspend[T]` wrapper              | uniform wrapper plus frame: indirection per poll                       |

Deliverable: an object-model table in the report, and a verdict on whether
the representation matches the plan's "Representation Shortcuts" section.

### 5.2 Allocation

- Write a script that counts allocation instructions (`struct.new*`,
  `array.new*`, and boxing helpers) in the emitted WAT. At runtime, count
  them with an instrumented build, or by wrapping allocation helpers if the
  emitter centralizes them.
- Measure allocations per iteration for: a generic loop over `list[i32]`, a
  closure called in a loop, string interpolation in a loop, a
  requirement-bearing call in a loop, and `fn!` calls that complete
  immediately.
- Measure suspension frame size against the live locals at each suspension
  point. The plan says frames store every local. Estimate what liveness
  analysis would save.

### 5.3 Generic erasure

- **Cost:** benchmark a generic `sum[T]` or sort over `list[i32]` and
  `list[f64]` against hand-written monomorphic versions. Report the ratio.
- **Correctness:** NaN, -0, and extremes survive boxing; `ref.eq` on boxed
  primitives does not expose box identity where the spec forbids it; generic
  equality agrees with concrete equality.
- **Dictionaries:** are dictionaries built per call, per call site, or once?
  Measure allocation for a bounded generic call inside a loop, including
  blanket and supertrait dictionaries.
- **Evolution:** can the typed HIR support selective monomorphization later
  without redesign? Answer with the specific HIR fields that would carry it.
- Verdict: is uniform erasure the right MVP choice, and is it the right
  long-term default?

### 5.4 Requirement passing

- Record the ABI: how many hidden parameters a call carries per concrete row
  entry and per generic row, and how keyed provider packs are laid out.
- Measure pack lookup: linear search by key or indexed? Cost versus row size.
- Measure allocation for `$.with`, context spreads, row subtraction, and
  closure or frame capture of providers.
- Scaling probe: a 10-deep call chain with 1, 5, and 10 requirements. Does
  per-call cost stay constant?
- Host providers: is the `externref` dynamic wrapper built once per entry or
  once per call?

### 5.5 Suspension lowering

- Record the state-machine shape: dispatch instruction, spill strategy, and
  child-frame chain.
- Measure resume cost against suspension depth: is a poll O(depth)?
- Check the driver guard: where it lives, whether it is per instance, and how
  it behaves with reentrant host callbacks.
- Check that cancellation cleanup order matches the spec for three or more
  nested frames with multiple `defer` suites each.

### 5.6 Runtime library and host imports

- Determine how runtime helpers enter a module: inlined WAT per module,
  emitted on demand, or a separate module. Compile a hello-world program and
  list every helper and import it contains. Unused helpers in a minimal
  module are a finding.
- List the full host import surface. Judge whether it is narrow, versionable,
  and how far it is from a WASI or Component Model boundary.
- Measure string crossing cost for 1 KB, 100 KB, and 1 MB strings.

### 5.7 Generated code quality and direction benchmarks

A full benchmark suite is out of scope. A small direction set is in scope. It
shows which representation costs dominate, so future work can target them. It
is not a performance claim.

- Four programs in `audit/bench/`, each isolating one cost:
  1. **scalar baseline:** recursive `fib` or a checked integer loop, with no
     allocation;
  2. **generic erasure:** a generic sum or sort over `list[i32]`, next to a
     monomorphic version of the same code;
  3. **requirement passing:** a 10-deep call chain carrying 1 and then 5
     requirements;
  4. **suspension:** a loop of `fn!` calls that complete immediately, next to
     the same loop without `!`.
- Record for each: `.wasm` size, compile time, and run time. Collect both the
  development build and a `binaryen -O2` build.
- Report ratios within a pair, not absolute speed: erased/monomorphic,
  5-row/1-row, suspending/plain, dev/`-O2`. A large dev/`-O2` gap means naive
  emission. A small gap with a large ratio means the representation itself
  is costly.
- Read WAT on the hottest paths for: repeated `ref.cast` of one value, box
  followed by unbox, dead locals, redundant null checks, and helper calls
  that could be inlined.
- Check MVP goal 2: the edit, compile, validate, run loop stays under one
  second for every existing fixture. Report the distribution.

## Phase 6: Compiler Architecture

### 6.1 Pipeline and HIR

- Map the stages and the data each hands to the next. Use
  `ast-grep outline` on `src/hir.ts`, `src/checker/`, and `src/emitter/`.
- **Typed HIR:** is it fully resolved? The emitter should never consult the
  AST, rerun inference, or look up names.
- **Desugaring location:** for defaults, varargs, copy-update,
  comprehensions, `for`, trailing blocks, test blocks, and interpolation,
  record where each is lowered: parser, checker, HIR, or emitter. Scattered
  desugaring is a finding.
- **Missing middle IR:** suspension state machines, boxing, and closure
  conversion appear to be done during WAT emission (`emitter/suspension.ts`,
  1,191 lines). Assess whether a lower IR with explicit control flow,
  boxing, and allocation is needed for liveness analysis, specialization,
  and optimization.
- **Requirements in HIR:** are rows carried on HIR types, and does
  `explain-requirements` read them or recompute them?
- **Module structure:** measure the size and fan-in of `checker/shared.ts`,
  `checker/context.ts`, and `checker/expression-calls.ts`. Is there one large
  mutable context threaded everywhere? Note which files are held just under
  the 1,500-line lint cap and whether the splits are cohesive or arbitrary.
- **Error recovery:** does the checker continue after the first error and
  report several independent ones?

### 6.2 Parallelism readiness

- Once signatures are collected, is checking each function body independent?
  List every shared mutable table the body checker writes to.
- Can the emitter produce functions independently, or do global tables (type
  section, dictionary types, closure types, string constants) force serial
  emission?
- Score readiness per stage: `ready`, `needs refactor`, `blocked by design`.
  The MVP compiles one file, so this measures future cost, not current benefit.

### 6.3 Incremental compilation readiness

- Can each declaration be compiled as a pure function of its own source and
  the signatures it depends on? Or do module-order effects (binding
  visibility, top-level initialization checks) require a whole-module pass?
- **Row inference:** is least-row inference a whole-program fixpoint? Measure
  the effect of one edit: change a leaf function's requirements and count how
  many other functions' inferred rows change.
- Is there any cache or query layer? Time a one-line edit on the largest
  program against a cold compile.
- Replay identity is derived from source. Check whether it is stable for
  formatting-only edits (shared with 4.1).
- Deliverable: a list of the specific design points that block a query-based
  or incremental compiler, each marked as an implementation choice or a
  language rule.

### 6.4 Code health

- Look for duplicated lowering paths. The plan lists many parallel variants:
  static, dynamic, bounded, stored, default-method, and suspending. Measure
  how much code each variant duplicates.
- List functions near the 300-line lint cap.
- Check that folder `index.ts` boundaries are respected, as `src/README.md`
  claims. No consumer should import a folder's internal files.

## Phase 7: Specification Feedback

Collect evidence throughout phases 1 to 6. For each candidate, record what
the implementation revealed, the evidence, and a recommendation: keep the
spec, change the spec, change the implementation, or file an
`OPEN_ISSUES.md` question.

Candidates to investigate:

- **Tuple and optional representation:** does any spec rule force erased
  storage, or block a struct per tuple shape and unboxed `T?`?
- **Row inference and incrementality:** if inferred rows are global, should
  public functions declare rows explicitly?
- **`Suspend[T]` uniformity:** does the spec force a dynamic wrapper on every
  stored suspension?
- **Rules invented by the implementation:** structural equality details (map
  order-independence, float ordering), string formatting, iterator versioning,
  host profile admission. Each needs a spec home or rejection.
- **Guesses:** places where the code picked a behavior with no spec rule:
  `all!`/`race!` signatures, `runtime-error` as a fallback, test-block
  lowering, module initialization order.
- **Fixture format (decided 2026-09-25):** the conformance suite must be
  fully specified by `spec/` alone. `expect-result`, `expect:`, and every
  harness hook belong in `spec/conformance/README.md`. The audit drafts that
  text in `audit/proposals/fixture-format.md`, using the 1.6 directive
  inventory, and does not edit `spec/` itself.
- **Command contract:** should the `parse`/`check`/`run` exit codes and the
  located diagnostic format from `audit/fuzz/CONTRACT.md` become part of the
  conformance specification, so every implementation can be tested the same
  way?
- **Harness hooks as semantics:** the competing-driver, reentrant-poll, and
  pending-function directives. Should the spec define a test-driver
  interface instead?
- **Spec edits from phase 1.1:** for each relaxed or new rule, decide whether
  the implementation found a real problem or only a convenience.
- **Diagnostics:** codes the implementation needed that the spec lacks, and
  spec codes no implementation path can produce.

---

## Order And Dependencies

```text
Step 0  baseline rerun
Phase 1 test integrity          must finish before trusting phase 2 numbers
Phase 2 coverage          ┐
Phase 3 independent tests ┘     parallel
Phase 4 runtime behavior        after phase 2 (reuses the claim ledger)
Phase 5 runtime architecture ┐
Phase 6 compiler architecture┘  parallel; can start alongside phase 2
Phase 7 spec feedback           gathered throughout, written last
REPORT.md                       last
```

## Final Report

`audit/REPORT.md` contains:

1. **Verdict:** one paragraph on how much of the MVP is real and fit to build
   on.
2. **Scorecard:**
   - portable pass rate;
   - full-conformance pass rate;
   - blind-fixture pass rate by area;
   - high-risk gap probes passing;
   - test-case quality: misplaced language cases, weak oracles, non-portable
     directives, stub-runner coupling failures;
   - claim ledger counts (verified, contradicted, no evidence);
   - fuzz findings.
3. **Coverage table** per spec chapter: fixtures, selected, passing,
   deferred, failing.
4. **Architecture:** object-model table, allocation profile, erasure and
   requirement-passing costs, direction-benchmark ratios, WAT quality notes, and
   parallel and incremental readiness scores.
5. **Findings** ranked by severity, linking `findings/F-NNN.md`.
6. **Spec feedback:** the phase 7 decision log.
7. **Open questions:** candidate `OPEN_ISSUES.md` entries, phrased as
   questions.

## Scope Decisions

Settled on 2026-09-25:

- **Blind fixtures (3.1):** in scope. They are written by a fresh agent that
  receives only `spec/` and the fixture-format section of `test/README.md`.
- **Benchmarks (5.7):** only the four-program direction set.
- **Fuzzing (3.2):** implementation-neutral tooling driven through the
  command contract; seeded, 5,000 cases per fuzzer, with the stopping rules
  above.
- **Mutation testing:** out of scope. Gap probes (1.4) replace it.
- **Fixture format:** `spec/` must fully define the conformance fixture
  format. Step 1.6 lists every directive defined only outside `spec/`, and
  phase 7 delivers the draft text.
