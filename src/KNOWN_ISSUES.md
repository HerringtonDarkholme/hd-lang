# Known Issues In The Prototype

The prototype compiler in `src/` lags the specification. This file lists
what it gets wrong today. Each finding has one line: its F-id, its effect,
and the conformance fixture that shows it, if one does. Design questions
are in [future-work/OPEN_ISSUES.md](../future-work/OPEN_ISSUES.md), and
git history keeps the audit evidence behind each finding.

## Known Failures

[`test/portable/KNOWN_FAILURES.tsv`](../test/portable/KNOWN_FAILURES.tsv)
lists the conformance cases the prototype fails. Each row is tagged with a
finding below or with an applied decision. On 2026-10-05 the suite has
2,581 cases: 2,545 selected in `test/portable/cases.tsv` and 36 known
failures. The selected cases are 2,194 language tier, 300 stdlib tier, and 51
CLI tier; the known failures are 24 language tier, 2 stdlib tier, and 10
CLI tier.

| Tag | Cases | Why they fail |
| --- | ---: | --- |
| CLI-DOC | 8 | no `hd doc` command |
| F-250 | 5 | GADT variant results give generic diagnostics |
| F-259 | 1 | the `disposed-file` runtime profile does not exist |
| FACT-PATTERN | 11 | a typed fact's `@annotate` argument must be one of its type parameters |
| TYPE-GAPS | 5 | remaining batch 51 inference codes and batch 51b type rules are not checked |
| VOID-UNIT | 2 | `void` is kept apart from the empty tuple `()`, so a void success has no `Eq` |
| RETRY-WITH | 1 | `retry_with!` is held because its current std dependency would load `std.time` eagerly |
| MVP-PATTERN | 1 | a `let` data pattern on a generic data value is `unsupported-match-subject` |

## Findings

Correctness and diagnostics:

- **F-155**: a runtime panic prints only `CODE: runtime panic`, with no
  source location, so nothing checks a panic marker's line.
- **F-161**: unbounded recursion ends in a Node `RangeError` stack trace,
  not a `stack-exhausted` panic.
- **F-250**: a GADT variant result gets `syntax-error`,
  `expected-expression`, or `unsupported-gadt-result`, and a pack function
  gets `unsupported-generic-parameter`, not one stable code per deferred
  feature. Fixtures: the five rows tagged F-250.
- **F-259**: the adapter rejects the `disposed-file` runtime profile, so
  `runtime/valid/resource-disposed-result.hd` cannot run.
- **F-265**: code-generation failures and an adapter `entry` option with no runnable
  export exit through a JavaScript stack trace, not a stable code.
- **F-401**: replay code identity hashes each function's source text, not
  the module's semantic content, so a changed callee replays and a
  formatting edit does not.
- **F-403**: a panic outside `expect_panic` stops `hd test`, so the later
  test cases never run.
- **F-605**: parsing stops at the first error, and checking reports one
  error per function; a signature error hides every body error.
- **F-611**: some diagnostics print `u32` where `types.alias.usize.display`
  asks for `usize`: an expected type written `usize`, a field read, and the
  types that `no-common-type` and a generic-inference conflict list. The
  found type of a mismatch, operator operands, and the REPL print `usize`.

Runtime cost:

- **F-501**: `Map` is an unhashed association list, so `get` and `insert`
  are O(n) and building a map is quadratic.
- **F-502**: a bounded generic call rebuilds its dictionaries on every call,
  and each bound method call allocates a trait value.
- **F-503**: interpolation concatenates pairwise and copies string literals
  on every evaluation.
- **F-504**: every `.None`, `for` step, and `map.get` allocates an optional
  carrier.
- **F-505**: `List[i32]` stores boxes even in concrete code, so every store
  allocates.
- **F-506**: suspension frames keep dead locals, so a stored suspension can
  hold garbage alive.
- **F-550**: a row-generic callback adapter copies the provider pack once per
  lookup, O(K²) for a K-entry row.
- **F-551**: `$hd.provider_concat(left, null)` copies `left` instead of
  sharing it.
- **F-552**: suspension code size grows about N³ with N bang-call sites in
  one function.
- **F-553**: resume and CFG dispatch walk linear `if` chains, not
  `br_table`.
- **F-554**: a `fn!` call that completes at once costs about 13 times a
  plain call.
- **F-555**: `hd run` busy-polls at 100% CPU when a host provider stays
  pending.
- **F-557**: every module embeds the whole runtime library, used or not.
- **F-558**: strings cross the host boundary one byte per import call.
- **F-604**: checking nested unannotated closures doubles in time per
  nesting level.

Compiler structure:

- **F-607**: HIR types are strings that the emitter re-parses, and nominal
  types are keyed by bare name.
- **F-608**: program-wide numbering makes one inserted declaration rewrite
  half the WAT.
- **F-609**: desugaring is split between checker and emitter, and control
  flow is lowered on three paths.
- **F-610**: expression dispatch is split into `??` chains, so `tsc` cannot
  flag a forgotten kind.

## Applied Decisions The Prototype Does Not Follow Yet

| Tag | Decision and gap |
| --- | --- |
| EMB-S | A trait method is a candidate only where its trait is available. The prototype tracks trait imports only for a trait of another package (`checker/package-ownership.ts`); every trait of the calling module's own package, and every std trait, stays available. No fixture shows the gap. |
| P2 | Member lookup skips members not visible from the calling module. The prototype hides a member without `pub` from another package and a std type's from code outside std; another module of the same package still sees it. No fixture shows the gap. |
| TYPE-GAPS | Batches 51 and 51b: a generic call's other argument conflicts are `type-mismatch`; an impl parameter outside the head is `unconstrained-impl-parameter`; an impl's own bounds must prove its supertraits; `void` satisfies `Any`; derived `Arbitrary` needs no inspectable member. The requirement-key dynamic-safety rule is implemented; the prototype does not check these remaining gaps. |
| VOID-UNIT | Batch 52: `void` is an alias for `()`. The prototype keeps a separate `void` type, so `let u: void = ()`, a `()` result for `-> void`, and `(void, i32)` are rejected. A void success takes only the literal `()`, as in `.Ok(())`, not another `void` expression such as `.Ok(log())`. |
| FACT-PATTERN | A typed fact's `@annotate` type argument is a pattern, such as `fn(T) -> R` or `i32`, whose parameters are inferred from the target as a call's are, and `h.fact::[D]()` infers `D`'s arguments from the handle's `F` the same way (batch 59). The prototype accepts only one of the fact type's own type parameters and reports `type-mismatch` at `@annotate`. |
| HOST-CATALOG | Batch 64: the default profile binds `Args`, `Env`, `ConsoleInput`, `Clock`, `Random`, `FsRead`, and `FsWrite`, with free helpers over them; `Console` gains `write_error_line!` and `eprintln`; `std.task` gains `Backoff` and `retry_with!`. The prototype binds the whole profile (src/commands/default-profile.ts). `lib/std` declares the other items but `Backoff` and `retry_with!`. |

## Gaps No Fixture Reaches

- **Host boundary shapes**: the checker rejects boundary types the
  specification allows (`module.boundary.allowed`,
  `module.profile.host-result.shape`): a `Map`, a generic enum, or an
  enum with shared fields reports `unsupported-host-provider-signature`,
  as an argument or a result. The prototype has no registered boundary
  functions, so it checks the consent of
  [`module.boundary.consent.out`](../spec/lang/10-modules.md#r-module.boundary.consent.out)
  only for host capability methods, the default profile's included.
- **Toolchain keys**: `[package] hd` and `[toolchain] pin`
  (`cli.manifest.toolchain-keys`) are checked for shape only. The prototype
  has no toolchain version, so it rejects no graph for a too-new minimum
  (`module.toolchain.graph-minimum`), does not fetch a pinned toolchain, and
  accepts `[toolchain]` in a dependency's manifest (`module.toolchain.pin`).
- **Script entry and other modules**: when a package's entry module is a
  script (top-level statements, no `main`; c7d9c4e3), the requirement row
  is inferred from the whole joined top level, so a top-level `println` in
  a non-entry module is accepted instead of rejected
  (`module.init.requirement-free`). Fixing it needs each top-level
  statement's module of origin.
- **Shapes** (batch 42): the spec removed `shape`, `shape_of`, and the
  shape types, but `src/checker/shapes.ts` and `lib/std/annotation.hd` still
  implement them.
- **Testing**: only functions of a `tests:` block are hidden from code
  outside it, and dev dependencies are not implemented.
- **Testing T8**: a test body's `Result` reports only its outer tag, not its
  `.Ok` value's `ExitCode`.
- **Testing T40**: a trailing block binds the final parameter only for calls
  the checker plans, not for built-in functions it special-cases.
- **Map implementations** (Q-map): `lib/std/iter.hd` writes
  `impl[K, V] Iterable[(K, V)] for Map[K, V]` without `K < Eq & Hash`,
  through a std-only checker exception the spec does not have.
- **Closures**: a suspending closure in a generic function cannot call a
  method through the enclosing function's bounds.
- **Ranges**: a range expression's hidden std type names, such as
  `__std_ops_RangeTo`, appear in diagnostics when the program does not
  import them. `for` over a range steps an `Iterator` closure, not a
  counted loop.
- **Derived `Arbitrary`** is generated by the checker
  (`checker/derive-arbitrary.ts`), not a `std.testing` template.
  `Choices.int` never draws a `u64` above the largest `i64`, and the runner
  prints no panic message.
