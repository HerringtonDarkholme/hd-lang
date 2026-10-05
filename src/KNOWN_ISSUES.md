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
2,478 cases: 2,416 selected in `test/portable/cases.tsv` and 62 known
failures. The selected cases are 2,094 language tier, 284 stdlib tier, and 38
CLI tier; the known failures are 52 language tier, 2 stdlib tier, and 8
CLI tier.

| Tag | Cases | Why they fail |
| --- | ---: | --- |
| CLI-DOC | 8 | no `hd doc` command |
| MODULE-DOC | 1 | the lexer reports `doc-comment-without-target` for the first `##` block of a file, which documents the module |
| F-250 | 6 | GADT variant results give generic diagnostics |
| F-259 | 1 | the `disposed-file` runtime profile does not exist |
| TQ-2 | 1 | the linked checker does not know which package declares a type, so the orphan rule sees a dependency type as local |
| EMB-S | 2 | trait imports are not tracked, so a dependency trait is available without a use |
| P2 | 2 | member visibility is checked only for std types, so a private member of a dependency type is visible |
| M29 | 1 | the linked checker does not know which package declares a fact type |
| FACT-PATTERN | 11 | a typed fact's `@annotate` argument must be one of its type parameters |
| TYPE-GAPS | 6 | remaining batch 51 inference codes and batch 51b type rules are not checked |
| VOID-UNIT | 2 | `void` is kept apart from the empty tuple `()`, so a void success has no `Eq` |
| RETRY-WITH | 1 | `retry_with!` is held because its current std dependency would load `std.time` eagerly |
| TEST-REG-ID | 4 | test registration recognizes a bare spelling instead of the imported declaration identity |
| MVP-PATTERN | 1 | a `let` data pattern on a generic data value is `unsupported-match-subject` |
| PRIMITIVE-LEFT-TRAIT | 1 | a primitive left operand never searches operator traits, so `3 * price` with only `impl Mul[Money] for i64` is rejected |
| DOC-TESTS | 4 | `hd` blocks in `##` comments are not extracted or run as doc tests, so a doc-test file registers no test case and `hd test FILE` exits 101 |
| AMBIGUOUS-TYPE | 1 | an ambiguous requirement-key solution reports `cannot-infer-type`, not `ambiguous-type` |
| SHADOW-TPARAM | 3 | a method type parameter, local declaration, or local value may reuse an enclosing type parameter's name |
| VARIANCE-MUT-SELF | 1 | `mut self` inherent methods are skipped by the variance check |
| ALIAS-MISSING | 2 | an unused alias's right side is never resolved |
| DERIVE-MISSING | 1 | `@derive` of a name that resolves to nothing reports `underivable-trait` |
| BOUND-AMBIGUOUS | 2 | a bound-only parameter that several instantiations fit reports `cannot-infer-type`, not `ambiguous-type` |

## Findings

Correctness and diagnostics:

- **F-155**: a runtime panic prints only `CODE: runtime panic`, with no
  source location, so nothing checks a panic marker's line.
- **F-161**: unbounded recursion ends in a Node `RangeError` stack trace,
  not a `stack-exhausted` panic.
- **F-250**: a GADT variant result gets `syntax-error`,
  `expected-expression`, or `unsupported-gadt-result`, and a pack function
  gets `unsupported-generic-parameter`, not one stable code per deferred
  feature. Fixtures: the six rows tagged F-250.
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
| TQ-2 | The owner of a trait argument's outer constructor may write the impl. Implemented within one package; the linked checker does not know which package declares a dependency type. |
| EMB-S | A trait method is a candidate only where its trait is available. The prototype checks one module without trait imports, so every trait is available. |
| P2 | Member lookup skips members not visible from the calling module. The prototype links packages into one namespace and checks member visibility only for std types. |
| M29 | A `Self` line warns `unused-derivation-fact` when the fact's package does not supply the block's trait. The linked checker does not know which package declares a fact type. |
| TYPE-GAPS | Batches 51 and 51b: a generic call's other argument conflicts are `type-mismatch`; an impl parameter outside the head is `unconstrained-impl-parameter`; an impl's own bounds must prove its supertraits; a bound implies its supertrait bounds; `void` satisfies `Any`; derived `Arbitrary` needs no inspectable member. The requirement-key dynamic-safety rule is implemented; the prototype does not check these remaining gaps. |
| CLI-53 | Batch 53 CLI rules: the prototype's `hd` has none of `-p` or workspaces (`--filter` and `--deny-skipped` exist; nothing is ever skipped) and the workspace search from a member. Most have no fixture format. |
| CLI-57 | Batch 57 CLI rules: the prototype has none of the unlisted-member error, `exclude`, `-p` inside a member, or `--format json` with its named fields on any command. |
| VOID-UNIT | Batch 52: `void` is an alias for `()`. The prototype keeps a separate `void` type, so `let u: void = ()`, a `()` result for `-> void`, and `(void, i32)` are rejected. A void success takes only the literal `()`, as in `.Ok(())`, not another `void` expression such as `.Ok(log())`. |
| FACT-PATTERN | A typed fact's `@annotate` type argument is a pattern, such as `fn(T) -> R` or `i32`, whose parameters are inferred from the target as a call's are, and `h.fact::[D]()` infers `D`'s arguments from the handle's `F` the same way (batch 59). The prototype accepts only one of the fact type's own type parameters and reports `type-mismatch` at `@annotate`. |
| HOST-CATALOG | Batch 64: the default profile binds `Args`, `Env`, `ConsoleInput`, `Clock`, `Random`, `FsRead`, and `FsWrite`, with free helpers over them; `Console` gains `write_error_line!` and `eprintln`; `std.task` gains `Backoff` and `retry_with!`. The prototype binds the whole profile (src/commands/default-profile.ts). `lib/std` declares the other items but `Backoff` and `retry_with!`. |
| DOC-TESTS | Owner design, 2026-10-04: each fenced `hd` block in a `##` comment under `src/` is a doc test, compiled as its own program with the public view and run by `hd test` (`module.test.doc.*`, `cli.test.doc.*`). The prototype ignores the blocks, so `hd test` runs none, and `hd check --tests` checks none. |
| AMBIGUOUS-TYPE | Owner, 2026-10-04: inference with several valid solutions is `ambiguous-type` (`types.infer.ambiguous.code`). The prototype reports `cannot-infer-type` for a callback row that fits two generic keys in either order. |
| SHADOW-TPARAM | Owner, 2026-10-04: no declaration within a type parameter's scope may reuse its name (`names.type-param.no-redeclare`). The prototype renames a method's shadowing binder, and accepts a local `data T` or value `T` inside `fn work[T]`. |
| VARIANCE-MUT-SELF | Owner, 2026-10-04: a `mut self` inherent method counts toward declared variance (`types.variance.surface.mut-self`). The prototype skips those methods. |
| ALIAS-MISSING | Owner, 2026-10-04: an alias whose right side names nothing is `unknown-type`, or `unknown-trait` for a row key, at the alias, used or not (`types.alias.target-unknown`). The prototype checks the right side only where the alias is used. |
| DERIVE-MISSING | Owner, 2026-10-04: `@derive` of a name that resolves to nothing is `unknown-trait` (`annot.derive.unknown`). The prototype reports `underivable-trait`. |
| BOUND-AMBIGUOUS | Owner, 2026-10-04: a bound-only parameter with no default that several instantiations fit is `ambiguous-type` (`types.generic.infer.bound.no-default.ambiguous`). The prototype reports `cannot-infer-type`. |

## Gaps No Fixture Reaches

- **Host boundary shapes**: the checker rejects boundary types the
  specification allows (`module.boundary.allowed`,
  `module.profile.host-result.shape`): a `Map`, a generic enum, or an
  enum with shared fields reports `unsupported-host-provider-signature`,
  as an argument or a result. A program's data with private fields is
  correctly rejected (`module.boundary.pub`), pinned by the
  `host structural results expose only public data fields` test. A std
  type crosses with its private fields, as `Timestamp` must for the
  default profile's `Clock`; no rule states that exception yet.
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
