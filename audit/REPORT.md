# Prototype Audit: Open Items

**Subject:** the Wasm GC prototype compiler in `src/`, audited at commit
`bd985d7` on 2026-09-25. Measurements below come from that commit unless a
later date is given; the evidence folder for each section is linked in place.

Everything the audit found that has since been fixed or decided is gone from
this report: the verdict, the scorecard, the claim ledger, the coverage and
blind-fixture runs, and the fuzzing rounds. The repository history keeps
them. What remains is the architecture review, which still describes the
prototype, and the findings that are still open. On 2026-09-27 the prototype
passes 983 of the 1,090 conformance cases; [`README.md`](README.md) says
where the other 107 are listed.

The architecture is sound for a single-file semantic prototype. The HIR is a
real typed and resolved boundary, and concrete requirement rows cost nothing
per call. It is not a base for a production compiler: types are strings,
there is no lowered IR and no liveness, boxing reaches even concrete lists
and optionals, maps are linear, dictionaries are rebuilt on every call, and
suspension costs about 13 times a plain call and grows code super-linearly.

## 1. Architecture

### 1.1 Runtime Representation

Evidence: [`05-object-model`](evidence/05-object-model/SUMMARY.md),
[`05-requirements`](evidence/05-requirements/SUMMARY.md).

| Construct            | Representation                                               | Cost                                              |
| -------------------- | ------------------------------------------------------------ | ------------------------------------------------- |
| data                 | typed GC struct; all fields `mut`                            | 1 allocation; string literals re-allocated per evaluation |
| generic data         | one erased struct with `anyref` fields                       | a box per primitive; a cast and unbox per read     |
| enum                 | one flat struct per enum: tag plus the union of all variant fields | `match` is a linear if-chain on the tag      |
| fieldless variant    | global singleton                                             | 0 allocations                                     |
| `T?`, `Result`       | shared `{tag, anyref}` variant                               | `.None` allocates; a present `i32` costs 2 allocations |
| tuple                | `anyref` array                                               | a box per scalar; a cast per read                  |
| `List[T]`            | vector plus `anyref` array                                   | a box per element, even for concrete `List[i32]`  |
| `Map[K, V]`          | parallel arrays, no hashing                                  | O(n) `get` and insert; O(n²) build                |
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
  - a generic list sum against the same sum over a concrete `List[i32]` is
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
  exceeds 1 s. Command-line wall time had a 687 ms median, and 136 of 551
  invocations exceeded 1 s, dominated by loading Binaryen (F-560). On an
  idle machine on 2026-09-26, `hd parse` takes 0.33 s, 0.2 s of it Binaryen.

**Erasure verdict:** uniform erasure was the right MVP choice. As
implemented, it is not a good long-term default. It needs:

- static dictionaries;
- unboxed optional, tuple, and scalar-list storage where no generic sharing
  occurs;
- selective specialization, with erasure kept as the fallback.

The HIR already carries most of the fields specialization needs. Two gaps
remain: calls have no explicit type-argument list, and types are strings.

### 1.2 Compiler Architecture

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
  - 13 functions sit at 258 to 300 lines against the 300 cap (re-measured
    2026-09-26);
  - two files are within 12 lines of the 1,500-line cap (1,500 and 1,488 on
    2026-09-26);
  - duplication is 31.7% within `emitter.ts`;
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

**Scaling hazard:** nested unannotated closures double check time per level
(F-604).

**HIR verdict:** the design makes sense for a single-file MVP. Before
multi-module or incremental work, it needs:

- interned structured types;
- stable declaration identities;
- one shared lowered IR.

## 2. Open Findings

The most important open findings, ranked by impact. All 44 open findings,
with the conformance cases each one keeps failing, are in
[`evidence/findings-table.md`](evidence/findings-table.md). Duplicates found
by several workers are merged under one canonical ID.

### Correctness

| Rank | ID                    | Severity | Finding                                                                                     |
| ---- | --------------------- | -------- | ------------------------------------------------------------------------------------------- |
| 1    | F-403                 | major    | `hd test` shares one instance across `main` and all test blocks, against chapter 02        |
| 2    | F-163                 | major    | `xs == [1, 2]` is rejected with `type-mismatch`                                            |
| 3    | F-201                 | minor    | the compiler accepts undeclared requirement keys on non-entry functions                     |
| 4    | F-401 (F-611)         | minor    | replay identity hashes each function's source: a changed helper replays, a reformatted one fails |

### Unimplemented features and codes

| Rank | ID    | Severity | Finding                                                                                      |
| ---- | ----- | -------- | -------------------------------------------------------------------------------------------- |
| 5    | F-250 | major    | deferred features get generic or wrong diagnostics (MVP goal 5 not met)                      |
| 6    | F-155 | minor    | runtime panics carry no source location                                                      |

### Performance and architecture

| Rank | ID    | Severity | Finding                                                                        |
| ---- | ----- | -------- | ------------------------------------------------------------------------------ |
| 7    | F-552 | major    | suspension code size grows super-linearly with bang-call sites                 |
| 8    | F-501 | major    | maps have no hashing                                                           |
| 9    | F-502 | major    | dictionaries are rebuilt per call, plus a trait value per method call          |
| 10   | F-550 | major    | the row-generic callback adapter copies the provider pack once per lookup      |

Merged duplicates:

- F-401 = F-611 = F-264;
- F-265 = F-162 = F-306;
- F-250 = F-312.

## 3. Fuzzing

The fuzzer is now [`spec/tools/fuzz/`](../spec/tools/fuzz/README.md). Its
audit rounds are finished; the one implementation bug they found that is
still open on its own is F-310. The minimized fixtures for open findings
(F-306 in F-265, F-310, and F-312 in F-250) are in
[`evidence/03-fuzz/findings/`](evidence/03-fuzz/findings/).
