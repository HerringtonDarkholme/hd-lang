# Known Issues In The Prototype

The prototype compiler in `src/` lags the specification. This file lists
what it gets wrong today. Each finding has one line: its F-id, its effect,
and the conformance fixture that shows it, if one does. Design questions
are in [future-work/OPEN_ISSUES.md](../future-work/OPEN_ISSUES.md), and
git history keeps the audit evidence behind each finding.

## Known Failures

[`test/portable/KNOWN_FAILURES.tsv`](../test/portable/KNOWN_FAILURES.tsv)
lists the conformance cases the prototype fails. Each row is tagged with a
finding below or with an applied decision. On 2026-10-06 the suite has
2,825 cases: 2,803 selected in `test/portable/cases.tsv` and 22 known
failures. The selected cases are 2,428 language tier, 308 stdlib tier, and 67
CLI tier; the known failures are 6 language tier, 1 stdlib tier, and 15
CLI tier.

| Tag | Cases | Why they fail |
| --- | ---: | --- |
| CLI-DOC | 8 | `hd doc` prints one item; it writes no pages and takes no flags |
| CLI-PAGES-HIDDEN | 2 | `hd new --pages` stays hidden until `hd doc` can build the site |
| F-259 | 1 | the `disposed-file` runtime profile does not exist |
| F-614 | 1 | a `.Variant` line right after a same-line `if` suite is joined to it |
| F-615 | 2 | an `if` or `match` with an expected type joins its branches by least common type |
| F-621 | 1 | a library module under test gets an entry row for its top level |
| F-622 | 1 | `derivation-line-drift` warns on blocks whose member lines agree |
| F-623 | 3 | the package graph is not modeled, so no `package-cycle` and no dependency back on the root |
| F-624 | 1 | a trait-qualified call does not take a type-parameter receiver |
| F-625 | 1 | `hd test` reports a test case's `.Err` on standard error, without the error |
| RETRY-WITH | 1 | `retry_with!` is held because its current std dependency would load `std.time` eagerly |

## Findings

Correctness and diagnostics:

- **F-161**: a `stack-exhausted` panic names no source location, because no
  panic site marks a call; the report says only the category and a hint.
- **F-259**: the adapter rejects the `disposed-file` runtime profile, so
  `runtime/valid/resource-disposed-result.hd` cannot run.
- **F-265**: code-generation failures and an adapter `entry` option with no runnable
  export exit through a JavaScript stack trace, not a stable code.
- **F-401**: replay code identity hashes each function's source text, not
  the module's semantic content, so a changed callee replays and a
  formatting edit does not. The replay experiments also predate the
  decided Replay Rules: their site IDs hold byte offsets, and they stop
  at the end of a history instead of resuming.
- **F-605**: parsing stops at the first error, and checking reports one
  error per function; a signature error hides every body error.
- **F-613**: a GADT variant in a let-else or `for` pattern refines only the
  lowered match arm that copies its bindings out, so after the `let` an
  existential binding has lost its bounds and the arm's type equalities no
  longer hold. A `match` arm keeps both.
- **F-614**: a `.Variant` line at statement indentation directly after a
  same-line `if cond: return x` continues that line as a member call, so
  the `if` becomes a value without `else` (`type-mismatch`). By
  `lex.dot.statement-indent` it starts a new statement;
  `runtime/valid/contextual-variant-after-same-line-if.hd` shows it.
- **F-615**: a value-producing `if` or `match` joins its branch types by
  least common type even when an expected type exists, so a branch that
  misses the expected type is `no-common-type` on the `if` or `match` line,
  not `type-mismatch` on that branch. By `flow.if.value.least-common` and
  `flow.match.result.least-common`, the least common type applies only
  without an expected type. The cause is the join in `checkMatch` and in
  the `if` case of `checkControlExpression`, both in
  `src/checker/expression-control.ts`.
  `typing/invalid/match-arm-misses-expected-type.hd` and
  `typing/invalid/if-branch-misses-expected-type.hd` show it.
- **F-621**: a package build whose module under test is a library module,
  not `src/main.hd` or another entry, treats that module as the entry. So
  its top level gets an inferred entry row (`scriptEntry` in
  `src/package.ts`), and `println` there is accepted. By
  `module.init.requirement-free`, a non-entry module initializes
  requirement-free. `typing/invalid/non-entry-top-level-println.hd` shows it.
- **F-622**: `derivation-line-drift` warns on the later of two derivation
  blocks of one package even when their member lines are identical, as two
  blocks over `Serialize` and `Deserialize` that both write `cache = pass`.
  By `annot.line.drift`, only lines that differ warn. The comparison is in
  `lintDerivations` in `src/checker/typed-derivation.ts`. Found by Kimi
  (K10); `cli/derivation-lines-agree` shows it.
- **F-623**: the package graph is not modeled. A path requirement that
  reaches the root package back is not linked, so a cycle of packages is no
  `package-cycle` error (`module.cycle.package`), and a dev dependency that
  depends back on the package fails with `unknown-module`, where a test
  module should get `cyclic-test-dependency` and an integration test should
  build (`module.test.cyclic-dev-unit`, `module.test.cyclic-dev-allowed`).
  `cli/dep-package-cycle`, `cli/dev-dependency-cyclic-unit`, and
  `cli/dev-dependency-cyclic-integration` show it.
- **F-624**: a trait-qualified call `Trait::method(item)` whose receiver's
  type is a type parameter reports `unknown-method`, as in
  `Parent::id(item)` with `T < Parent` or `T < Child`. By
  `trait.qualified.receiver` the receiver only has to implement the trait,
  which a bound or a child trait's bound proves. `checkImplementedMemberCall`
  in `src/checker/expression-calls.ts` searches implementations only.
  `typing/valid/trait-qualified-call-generic-receiver.hd` shows it.
- **F-625**: when a test case's body returns `.Err`, `hd test` writes
  `test "NAME" returned Err` to standard error in text mode, and the JSON
  `message` holds the same text. By `module.testing.err-print`, the
  runner's own report on standard output holds the error and its
  `caused by: ` lines. The entry renderer (`checker/entry-error.ts`) covers
  only `main`, and `entryError` in `src/diagnostic-report.ts` writes the
  line.
  `cli/test-err-report` shows it.

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
- **F-626** (was O-06 and speed finding F1): a call with several
  candidate instantiations, as `C::from(x)` with two `From` impls, copies
  the checker's state per candidate (`speculate`), so a `main` of 1,600
  such lines checks in 96 s, against 1.7 s with one impl.
- **F-627** (was speed finding F10): the emitter's `linkWat` re-parses the
  generated WAT, and Binaryen's `parseText` parses it again, about a third
  of a calc-sized compile.

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
- **Visibility left over** (task P1a): a std trait outside the prelude is
  available without a use (`trait.avail.module`). Outside std, a std
  type's private fields can still be named or filled in a data literal or
  pattern, as in `Iterator { step: next }`, because the checker's
  derivations build std structure values in the deriving module, against
  `data.vis.literal`, `data.vis.private-fields`, and `data.pattern.subset`.
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
- **`void` comparison** (`types.void`): the empty tuple's `Eq`, `Ord`,
  `Hash`, `Display`, and `Debug` come from the std tuple templates, which
  the prototype instantiates for `void` only when the program's own code
  writes `()` or `void` as a type argument or element, as in
  `Result[void, E]`. `log() == log()` alone reports `type-mismatch`.
  Instantiating it for every program would declare `std.inspect` there,
  which clashes with a program's own trait named `Inspectable`.
- **Derived `Arbitrary`** is generated by the checker
  (`checker/derive-arbitrary.ts`), not a `std.testing` template.
  `Choices.int` never draws a `u64` above the largest `i64`, and the runner
  prints no panic message.
