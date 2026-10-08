# New Compiler Design: Open Questions

Part of the [compiler design](README.md).

**Answered (owner and orchestrator, 2026-10-07).**

| Question | Answer |
|---|---|
| 10: where compiled entries live | `$HD_CACHE/obj/`; the `hd clean --cache` layout rule is amended (orchestrator's call) |
| 10: default thread count | `min(cores, 8)`, with `--jobs` / `HD_JOBS` (orchestrator's call) |
| 10: `cache-contention` target | no corruption, and N concurrent warm checks cost at most 1.5x the CPU of one; concurrent cold misses of one key may each compute it, since misses are not coordinated (orchestrator's call, amended for Codex re-review N-C3) |
| 10: templates calling private helpers | allowed, as hidden interface items (owner) |
| 10: one code per limit | separate codes (orchestrator's call) |
| 10: `hd fmt` on a file with syntax errors | the file is left untouched (orchestrator's call) |
| 23.1-1: bounded inlining and scalar replacement | in the first release (owner) |
| 23.1-2: polymorphic recursion | build error `instantiation-too-deep` (owner) |
| 23.1-3: suspension panic categories | add `suspension-deadlock` and `suspension-forbidden-context` (orchestrator's call) |
| 23.1-4: limit flags | `--max-heap SIZE` and `--time-limit DURATION`, no default (orchestrator's call) |
| 23.1-5: property-test base seed | from the test's stable name, plus `--seed N` (orchestrator's call) |
| 23.1-6: a panic in the REPL | the session continues; the input adds no binding (orchestrator's call) |
| 23.2-7: the 5 ms start target | it counts instantiation to first output, not process start (orchestrator's call) |
| 23.1-7: extend decision B to `Result` | yes: `.Ok` and `.Err` have no identity; `Result` uses the `multi` layout (owner) |
| `is` on value types | a compile error on an operand whose static type is a value type and on function values; unspecified on an `Any` that holds a value (owner) |
| literal width | decided per connected literal class (owner) |

Each finding's verdict on the Codex review of 8bb6860d is recorded where the finding was fixed. The re-review verdicts are in [codex-rereview-response.md](codex-rereview-response.md).

## 10. Open Questions For The Owner

Settled by the owner on 2026-10-07 and used above, not asked again: no
default memory cap, with an opt-in cap and a hard-limit diagnostic naming
the stage (§4.15); a 10 GB LRU shared cache with automatic eviction and
`hd cache gc` (§5.7); `hd build` on a library-only package writes only its
interface and cache entries (§7.5).

1. **Where compiled cache entries live.** `hd clean --cache` refuses a
   cache directory holding anything but `pkg`, `hash` and `tmp`
   ([`cli.clean.cache.layout`](../../spec/cli/command-line.md#r-cli.clean.cache.foreign)).
   **Recommendation:** put entries in `$HD_CACHE/obj/`, add `obj` to the
   rule's list, and let `hd clean --cache` remove it too, since it holds
   only derived data. The alternative, a per-package `build/cache`,
   loses sharing across worktrees, which pillar 2 depends on.
2. **Default thread count.** Many agents run `hd` at once on one machine,
   and too many threads slowed the prototype's suite about 9x.
   **Recommendation:** `min(cores, 8)` by default, `--jobs N` and
   `HD_JOBS` to change it, and `--jobs 1` as the serial mode. The
   `parallel-speedup` metric already stops at 8 cores.
3. **The `cache-contention` target.** The proposed metric says "each entry
   computed once" across N processes. The no-daemon cache gives "no
   corruption" for free, but "computed once" needs lock files.
   **Recommendation:** change the target to "no corruption, and N
   concurrent identical checks cost at most 1.5x the CPU of one". Add lock
   files later only for D2's expensive entries if a measurement asks.
4. **Templates that call private helpers (mine).** The spec does not say
   whether a template body may name private items of its trait's module.
   If it may, a dependent's check of the instantiated template needs their
   signatures. **Recommendation:** allow it. The interface carries those
   items as hidden items, which user code cannot name and which the
   interface validator accepts only when a template names them.
5. **Diagnostic codes for limits.** Answer 5 left the names to the
   orchestrator. This design proposes `file-too-large`, `nesting-too-deep`,
   `item-too-complex`, `type-too-large`, `match-too-complex` and
   `memory-limit` (§4.15). No owner question unless the owner wants fewer
   codes, for example one `limit-exceeded` with the limit named in the
   message. **Recommendation:** separate codes, as the Day 1 list asks.
6. **`hd fmt` on a file with a syntax error.** **Recommendation:** leave
   the file untouched and report its first syntax error, as gofmt does.
   Formatting around error nodes, as Biome does, risks rewriting code the
   parser misunderstood.

### 10.1 Inconsistencies Found In The Inputs

1. **Layout.** The research says layout needs no parser feedback; the spec
   says layout and parsing cooperate at suite colons and for `SUITE_END`.
   Resolved by the parser-driven layout cursor (§4.2).
2. **Pack bodies.** [`module.interface.contents`](../../spec/lang/10-modules.md#r-module.interface.contents)
   lists "the bodies of pack code", but packs were removed
   ([chapter 12](../../spec/lang/12-variadic-generics.md)).
3. **Dictionaries.** [`module.interface.dictionaries`](../../spec/lang/10-modules.md#r-module.interface.generic-compilation)
   still says generic functions compile once with dictionaries, against
   answer 8 (code per concrete type).
4. **Local impl heads.** The same table lists local impl heads "needed for
   coherence". A local impl must involve a local type or trait, so no
   other module can overlap it. The research's Q4b noted this too.
5. **Fact values.** [`module.interface.fact-values`](../../spec/lang/10-modules.md#r-module.interface.fact-expressions)
   puts fact values in the package interface; the check interface here
   keeps expressions and their types, and D2 computes values (the
   research's Q4b contradiction 2).
6. **Cache directory.** The CLI spec's clean rule allows only `pkg`,
   `hash` and `tmp` under `HD_CACHE` (question 1).
7. **Drive summaries.** The research's Q3 step 5 and Q7 slices 3 and 4
   still mention drive summaries and their hash, which answer 13 removed.
8. **Initialization order** needs bodies across the modules of an
   module cycle, a cross-module body fact not listed in the
   research. Handled inside the folder by init summaries (§4.13.10).
9. **"Query engine."** The Day 1 list says "incremental checking on a
   query engine", while the decision is a per-module cache with no salsa.
   The wording should follow the decision.
10. **CLI surface.** `hd fmt`, `hd fix`, `hd test --affected`,
    `--max-errors`, `--jobs`, `modules_checked` and the JSON `fixes` field
    are triaged or answered but not yet in the CLI spec. The spec pass
    adds them.
11. **The `block_on` ban is still transitive in the spec**
    (type-checking.md §16.2 item 1).
    [`req.drive.block-on.transitive`](../../spec/lang/11-requirements-and-suspension.md#r-req.drive.block-on.indirect),
    `req.drive.block-on.unprovable`,
    [`flow.defer.block-on`](../../spec/lang/06-control-flow.md#r-flow.defer.block-on.direct)
    and [`annot.fact.no-block-on`](../../spec/lang/14-annotations.md#r-annot.fact.block-on.direct)
    with its `unprovable` rule predate answer 13: direct-only, with a
    run-time panic for an indirect call. For facts, the indirect case is
    the runtime panic `fact-evaluation-failed`, since facts are run-time
    values (owner, 2026-10-07; codegen.md §12.3).
12. **`println` in the ban.** Answer 13 bans a direct `println` as well
    as `block_on`, but no spec rule names `println` (type-checking.md
    §16.2 item 2).
13. **Redundancy warnings.** The design brief asks for redundancy
    warnings, but the spec makes an unreachable arm an error
    (`unreachable-match-arm`). The design follows the spec
    (type-checking.md §16.2 item 3).

### Changes To D1

The owner reviewed D1 while D2 was written and asked three questions:
how abstract the checked IR is, whether the design is truly
data-oriented, and whether all these IRs are needed. D2 answers them by
editing D1 in place:

1. **§3.9 Data-Oriented Encoding (new).** The InternPool for types and
   constants; one dense tag + data + `extra` encoding; struct-of-arrays
   tables with hash maps only as indexes; the scratch buffer and rollback
   by truncation; one schema per IR generating typed views, builders,
   verifiers and printers; byte budgets; what it buys. Credits Zig,
   Carbon, Yuku and Vx.
2. **§3.10 IR Abstraction Contracts (new).** For each IR: what is
   resolved in it and what is deliberately not, its lifetime and bytes per
   source line, peak memory for a check and a build, its verifier, and a
   table mapping each prototype failure in the audit to the rule that
   prevents it.
3. **One typed IR.** THIR and the planned MIR are merged into **TIR**,
   one typed IR per body that the checker emits directly (§4.13.11, which
   now holds its full instruction catalog, desugaring table, builder API
   and invariants). No instance is ever materialized as IR: emission walks
   the generic TIR under a substitution (§13.8). The chain is tokens,
   syntax tree, interface, TIR, Wasm.
4. **§3.3, §3.4, §3.5.** Types, rows and constants live in the InternPool;
   `TyKind` is a decoded view; body arenas are reused column sets.
5. **§4.1, §4.4.** Token classes and keyword hashing after Yuku; the
   syntax tree's named-field views at its boundary, its wire format, and
   recovery by truncation.
6. **§4.6** is now the item index: an index over the syntax tree and the
   interface, not a copy.
7. **§4.10, §4.12.1.** The interface is its blob read in place; impl
   tables are sorted columns with a hash index.
8. **§1.1, §1.2, §2.1, §3.1, §4.13, §5.1, §5.2, §9.** Renamed to TIR; the
   crate `hd_tir` added; the `tir` cache entry added; D2's rows filled in.
9. **§4.15.** The instantiation depth row is filled in (§13.4).
10. **§7.3.** The test plan lists each program's statically registered
    test cases, since registration names are string literals
    ([`module.testing.reg.name`](../../spec/lang/10-modules.md#r-module.testing.reg.name)).
    Only `it_each` row counts are learned at run time (§19.2).
11. **§7.3 and §7.5** end with a pointer to §20, which finishes them.

## 23. Open Questions And Inconsistencies

### 23.1 Open Questions For The Owner

1. **Bounded inlining and scalar replacement in the first release.** The
   triage put inlining and escape analysis in Later, but `allocations`
   (at most 1 per iterator chain) and `runtime` (1.5x Node) need them for
   iterator chains and closures. **Recommendation:** pull a bounded
   version into slice 10: inline callees under a size budget and closures
   passed to known callees, and replace non-escaping closures and cells by
   locals. Both run in the one shared emission (§12.6).
2. **Polymorphic recursion: error or boxed fallback.** Answer 8's
   research proposed an error at an instantiation depth limit; the spec's
   [Shapes and Generic Code](../../spec/lang/04-type-system.md#shapes-and-generic-code)
   says such code falls back to shared boxed bodies. A fallback needs a
   second, erased code path. **Recommendation:** the error,
   `instantiation-too-deep`, reported by `hd build`, `hd run` and
   `hd test` (§13.4); it is contrived code with a simple fix. The spec
   pass rewrites that section to answer 8.
3. **Two panic categories.** A deadlocked entry (§14.8) and an indirect
   `block_on` or `println` in a forbidden context (§14.9) have no stable
   category. **Recommendation:** add `suspension-deadlock` and
   `suspension-forbidden-context`, the latter matching the check-time
   code, as answer 11 added two.
4. **Resource limit flags.** `--max-heap` is named in the triage; the
   time limit has no flag. **Recommendation:** `--max-heap SIZE` and
   `--time-limit DURATION` on `hd run`, `hd FILE`, `hd FILE.wasm` and
   `hd test` (where it caps each case), with no default limit.
5. **The property-test base seed.** The `determinism` metric wants equal
   output on every run, but a fixed seed explores the same cases each
   time. **Recommendation:** derive each property's first seed from its
   test's stable name, so runs are reproducible, and add `--seed N` to
   vary it.
6. **A panic in the REPL.** The REPL chapter does not say what a panic
   does to the session. **Recommendation:** report it and keep the
   session; the panicking input adds no binding, and mutations it made
   before the panic remain. The spec pass adds a rule.
7. **Extend decision B to `Result`?** Answered yes (owner,
   2026-10-07); wasm-layout.md §15.2 applies it. (From Codex
   finding 1.) Decision B removed the identity of `.Some` and of boxes.
   `Result` is still an enum whose every `.Ok` and `.Err` has its own
   identity, so it is laid out as one struct per construction
   (wasm-layout.md §15.2). That costs one allocation of about 16 to 24
   bytes per fallible call whose result is not scalar-replaced, which
   `?`-heavy code pays on every call. **Recommendation:** extend B:
   `.Ok` and `.Err` have no identity, and `is` on two `Result`s
   compares tags and then payloads, as for optionals. Then `Result`
   uses the `multi` layout `(i32 tag, T', E')` and allocates nothing.
   No known program compares `Result`s by identity.

8. **Does `hd check` evaluate facts?** **Answered (owner,
   2026-10-07): no command evaluates facts at compile time; facts are
   run-time values**
   ([`annot.fact.eval.lazy`](../../spec/lang/14-annotations.md#r-annot.fact.eval.lazy)).
   Shared enum data instead run during their declaring module's initialization
   ([`data.shared.module-init`](../../spec/lang/08-data-and-enums.md#r-data.shared.module-init)).
   The rest of this item is the superseded analysis. (From Codex
   finding 4.) Facts and metadata are evaluated once
   at compile time by an interpreter over TIR (codegen.md §12.3). A
   failing fact is `fact-evaluation-failed`. Evaluating needs the TIR of
   every function the fact calls, across modules and packages.
   **Recommendation:** evaluate only in `hd build`, `hd run` and
   `hd test`, the same split the owner accepted for
   `instantiation-too-deep`. `hd check` stays a per-module check, and a
   fact that panics is rare and contrived. The alternative, evaluating
   in `hd check` too, makes a check's result depend on other modules'
   bodies, which its cache key does not cover.

9. **Must an unread fact that panics fail the build?** **Answered
   (owner, 2026-10-07): an unread fact is never evaluated**
   ([`annot.fact.eval.unread`](../../spec/lang/14-annotations.md#r-annot.fact.eval.unread)).
   The rest is superseded. (From Codex re-review N-C2 and question 3.) The spec says
   a fact "is evaluated once, at compile time", and a panic during
   evaluation is a build error. It does not say whether a fact that no
   program reads is evaluated. **Recommendation:** demand-driven. Only
   the facts a built program reads are evaluated (codegen.md §12.3), and
   a spec note says so: an unread fact's panic is not reported. The
   cost is that adding a read can surface an old panic; a panicking fact
   is rare and contrived. Eager evaluation of every attached fact in the
   program's modules is the alternative; it costs interpreter time for
   facts nobody reads.
10. **Which values can a compile-time value hold?** **Answered (owner,
   2026-10-07): a fact may hold anything a global can**
   ([`annot.fact.value`](../../spec/lang/14-annotations.md#r-annot.fact.value)).
   The rest is superseded. (From Codex re-review N6, N-C2 and question
   4.) The old rule said "any compile-time value" without defining the
   term.
   **Recommendation:** define it as identity-free values (primitives,
   strings, enums, tuples, capture-free function values) plus an
   acyclic graph of data, list, map and array objects allocated during
   the evaluation, whose sharing and distinctness are kept. A closure
   with captures, a suspension, a handle or a cycle is
   `fact-evaluation-failed`. Each is rejected because a constant
   expression cannot rebuild it, and no known template needs one.

### 23.2 Inconsistencies Found In The Inputs

1. **Folding reference instances.** The research says `List[Point].push`
   and `List[User].push` fold. With exact Wasm types they fold only when
   `Point` and `User` have the same layout (§13.7); folding them always
   needs erased element storage and a cast per read.
2. **Shapes in the spec.** The type-system chapter's
   [Shapes and Generic Code](../../spec/lang/04-type-system.md#shapes-and-generic-code)
   describes one shared body for reference types, dictionaries for trait
   bounds, a boxed fallback for polymorphic recursion, and one reference
   shape for every enum. Answer 8 replaced that strategy. D1 listed only
   `module.interface.dictionaries` (§10.1, item 3).
3. **The transitive `block_on` ban** remains in
   [`req.drive.block-on.transitive`](../../spec/lang/11-requirements-and-suspension.md#r-req.drive.block-on.indirect)
   and [`flow.defer.block-on`](../../spec/lang/06-control-flow.md#r-flow.defer.block-on.direct),
   though answer 13 made it direct-only.
4. **The Component Model** is still named by
   [`req.host-wait.leaf`](../../spec/lang/11-requirements-and-suspension.md#r-req.host-wait.abi)
   and by HOST_CAPABILITIES' boundary table, against answer 7.
5. **Resource limits.** The stable panic categories lack
   `heap-exhausted` and `time-limit`
   ([`flow.panic.stable-categories`](../../spec/lang/06-control-flow.md#r-flow.panic.stable-categories)),
   though answer 11 added them, and the CLI chapter has no limit flags.
6. **The `allocations` metric** reads V8's heap statistics, while the
   first release runs programs on wasmtime. It can run the release
   `.wasm` under Node through the generated glue (§17.10), or count
   through a stats build's allocation hooks (§16.3).
7. **Start to first output.** `size-startup-heap` asks for at most 5 ms,
   measured through a process, while `startup` allows 20 ms for
   `hd --version`. The 5 ms can only hold for instantiation to output,
   not for a cold process.
8. **One instance per property test** (a first-release feature) meets
   [`flow.panic.poison`](../../spec/lang/06-control-flow.md#r-flow.panic.poison):
   a discard is a panic, so each discard costs a fresh instance (§19.4).
   Not a contradiction, but a cost the feature text does not mention.
9. **D1's test plan** said cases are registered only at run time; the
   spec lists them statically. Fixed in §7.3.
10. **The research's crates** `hd_mir` and `hd_opt` and its MIR stage are
    superseded by the one-IR decision (§3.9.1, §11.4).
11. **The triage** keeps inlining and escape analysis in Later while two
    goal metrics depend on them (question 1).

### 23.3 What Was Kept Brief

Sections 21 to 23 are brief by plan. Two designs need a measurement
before more text: the dev pipeline's Cranelift setting (spike T1 of
tiering.md) and the null collector
for tests (§18.6).
