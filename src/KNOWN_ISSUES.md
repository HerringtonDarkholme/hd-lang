# Known Issues In The Prototype

The prototype compiler in `src/` lags the specification. This file lists
what it gets wrong today. Each finding has one line: its F-id, its effect,
and the conformance fixture that shows it, if one does. Design questions
are in [future-work/OPEN_ISSUES.md](../future-work/OPEN_ISSUES.md), and
git history keeps the audit evidence behind each finding.

## Known Failures

[`test/portable/KNOWN_FAILURES.tsv`](../test/portable/KNOWN_FAILURES.tsv)
lists the conformance cases the prototype fails. Each row is tagged with a
finding below or with an applied decision. On 2026-10-04 the suite has
2,377 cases: 2,235 selected in `test/portable/cases.tsv` and 142 known
failures. The selected cases are 1,966 language tier and 269 stdlib tier;
the known failures are 132 language tier and 10 stdlib tier.

| Tag | Cases | Why they fail |
| --- | ---: | --- |
| F-250 | 6 | GADT variant results give generic diagnostics |
| F-259 | 1 | the `disposed-file` runtime profile does not exist |
| F-310 | 1 | a line that starts with `:` attaches a trailing block to the statement before it |
| TQ-2 | 1 | a package-role fixture cannot express ownership of a trait argument |
| EMB-S | 4 | package trait visibility is not modeled by the linked checker namespace |
| P2 | 5 | package member visibility is not modeled by the linked checker namespace |
| M29 | 1 | the fixture needs a second package to distinguish derivation ownership |
| DC7 | 1 | group statements are not interleaved across modules |
| MHP-1 | 1 | no inferred script entry requirement row |
| CLI-ENTRY | 11 | no `hd FILE` command for the runner's last step |
| NONPKG | 1 | a `use self` in a single-file program is not reported |
| SELF-CURRENT | 2 | relative lookup is fixed; these fixtures still need package import aliases |
| ROOTS | 2 | `src/lib.hd` is not the root module, and `src/main.hd` can be used |
| TASK-PROGRAMS | 1 | integration test modules are linked as one program |
| FOLDER-SELF | 1 | `x.hd` is not in folder `x` with its child modules |
| FACT-PATTERN | 11 | a typed fact's `@annotate` argument must be one of its type parameters |
| TYPE-GAPS | 6 | remaining batch 51 inference codes and batch 51b type rules are not checked |
| DERIVE-DEFAULT | 3 | no `@default` marker or count check for derived `Default` |
| DEFAULT-FIELD | 1 | derived `Default` requires `Default` of a member that declares a default |
| RACE-EMPTY | 2 | an empty `race!` task list is neither rejected nor a panic |
| CLI-57 | 2 | the test runner binds no `Process`, and `hd_run!` has no integration-only check |
| VOID-UNIT | 2 | `void` is kept apart from the empty tuple `()`, so a void success has no `Eq` |
| LIST-POP | 4 | `List.pop`, `insert`, `remove_at`, `clear`, and their list-truncate hook are absent |
| STD-DEBUG | 1 | std derivation ordering leaves TypeId and SelfRef without Debug |
| METHOD-DEFAULT | 1 | the parser rejects a default value on a method parameter |
| RETRY-WITH | 1 | `retry_with!` is held because its current std dependency would load `std.time` eagerly |
| TEST-REG-ID | 4 | test registration recognizes a bare spelling instead of the imported declaration identity |
| EQ-CONTEXTUAL | 2 | equality does not contextually type a variant from the opposite operand |
| FRESH-MUT | 1 | generic inference does not weaken `mut T` and `T` to their readonly join |
| QUALIFIED-PREFIX | 3 | a string prefix after `.` does not report `qualified-string-prefix` |
| LITERAL-FIRST-USE | 23 | an unannotated literal binding falls back before its first deciding use |
| SIGN-FALLBACK | 10 | an unsigned literal group falls back to `i32`, not `usize`, and `unsigned-comparison-always` is not checked |
| ONE-FIT | 8 | a literal is `i32` on the spot, so one fitting candidate never decides it, several fitting candidates never wait for the `usize` fallback, a literal receiver's arguments are checked at `i32`, and the fallback hint is absent |
| QUALIFIED-PATH | 6 | a used module name works before a function, but not before a type, a variant, a variant pattern, or an associated call; a whole-module use of a package module is `unsupported-package-use` |
| NO-IMPLIED-BOUND | 2 | a missing key bound at an impl target is reported again at each method's `self`, and at a field it adds `void-data-field` |
| DOC-TESTS | 1 | `hd` blocks in `##` comments are not extracted or run as doc tests |
| AMBIGUOUS-TYPE | 1 | an ambiguous requirement-key solution reports `cannot-infer-type`, not `ambiguous-type` |
| SHADOW-TPARAM | 3 | a method type parameter, local declaration, or local value may reuse an enclosing type parameter's name |
| VARIANCE-MUT-SELF | 1 | `mut self` inherent methods are skipped by the variance check |
| ALIAS-MISSING | 2 | an unused alias's right side is never resolved |
| PRIVATE-STD | 1 | a module path to a private std function reports `unknown-name` |
| DERIVE-MISSING | 1 | `@derive` of a name that resolves to nothing reports `underivable-trait` |

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
- **F-259**: `--profile disposed-file` is a usage error, so
  `runtime/valid/resource-disposed-result.hd` cannot run.
- **F-265**: code-generation failures and an `--entry` with no runnable
  export exit through a JavaScript stack trace, not a stable code.
- **F-310**: after an `if` or `match` suite, a line that starts with `:`
  passes `parse`, then `check` reports `not-callable`. Fixture:
  `parse/invalid/colon-line-after-if-suite.hd`.
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
| TQ-2 | The owner of a trait argument's outer constructor may write the impl. Implemented, but its fixture needs package roles. |
| EMB-S | A trait method is a candidate only where its trait is available. The prototype checks one module without trait imports, so every trait is available. |
| P2 | Member lookup skips members not visible from the calling module. The prototype links a package into one namespace, so every member is visible. |
| M29 | A `Self` line warns `unused-derivation-fact` when the fact's package does not supply the block's trait. The fixture needs a second package. |
| MHP-1 | A top-level `println` in a script is valid. The prototype infers no script entry row (`module.init.script-row`), so it reports `missing-requirement`. |
| DC7 | An initialization group runs statements in dependency order across modules. The linker joins modules whole, so `init-group-order.hd` reports `top-level-read-before-initialization`. |
| TYPE-GAPS | Batches 51 and 51b: a generic call's other argument conflicts are `type-mismatch`; an impl parameter outside the head is `unconstrained-impl-parameter`; an impl's own bounds must prove its supertraits; a bound implies its supertrait bounds; `void` satisfies `Any`; derived `Arbitrary` needs no inspectable member. The requirement-key dynamic-safety rule is implemented; the prototype does not check these remaining gaps. |
| DERIVE-DEFAULT | Batch 51: `@derive(Default)` through the `std.ops` template, with `@default` on one enum variant. The template in `lib/std/ops.hd` works for data types, but the marker function `std.ops.default` is missing: declaring a module function named `default` beside the trait breaks every `T::default()` call in the prototype. No check counts the marked variants. |
| RACE-EMPTY | Batch 51: `race!(tasks=[])` is `argument-count`, and an empty task list at run time panics with `explicit-panic`. The prototype accepts the first and hangs on the second. |
| CLI-53 | Batch 53 CLI rules: the prototype's `hd` has none of `--` program arguments, exit status 101, JSON lines with a summary record, `-p`, `--filter`, `--deny-skipped`, stdin as a program, the workspace search from a member, the `hd new --app` and `--lib` templates, or the executable, task, and test-root layout errors. Most have no fixture format. |
| CLI-57 | Batch 57 CLI rules: the prototype has none of `hd check --all`, the unlisted-member error, `exclude`, `-p` inside a member, a failing `--filter` on a FILE, the package REPL without `src/lib.hd`, `--format json` with its named fields on any command, or the `.gitignore` of `hd new`. `hd_run!` is plain hd over `Process` in `lib/std/testing.hd`, but `hd test` binds no `Process` for an integration test, and no check rejects `hd_run!` outside one. |
| VOID-UNIT | Batch 52: `void` is an alias for `()`. The prototype keeps a separate `void` type, so `let u: void = ()`, a `()` result for `-> void`, and `(void, i32)` are rejected. A void success takes only the literal `()`, as in `.Ok(())`, not another `void` expression such as `.Ok(log())`. |
| FACT-PATTERN | A typed fact's `@annotate` type argument is a pattern, such as `fn(T) -> R` or `i32`, whose parameters are inferred from the target as a call's are, and `h.fact::[D]()` infers `D`'s arguments from the handle's `F` the same way (batch 59). The prototype accepts only one of the fact type's own type parameters and reports `type-mismatch` at `@annotate`. |
| DEFAULT-FIELD | Batch 59: a member that declares a default needs no `Default` on its type. The `std.ops` template's `member[F < Default]` bound still covers it, so the prototype reports `member-not-derivable`. |
| HOST-CATALOG | Batch 64: the default profile binds `Args`, `Env`, `ConsoleInput`, `Clock`, `Random`, `FsRead`, and `FsWrite`, with free helpers over them; `Console` gains `write_error_line!` and `eprintln`; `std.task` gains `Backoff` and `retry_with!`. The prototype binds only `Console`, with no host `Console.write_error_line` entry, so `eprintln` under the default profile fails with `host-contract`. `lib/std` declares the other items but `Backoff` and `retry_with!`. |
| ONE-FIT | Task #254: exactly one fitting candidate (a bound's one implementing type, one receiver or left-operand type, one instantiation) decides an open literal; two or more decide nothing, and a failure after the fallback suggests `+5` or an annotation. A literal joined with a dependent method result takes its resolved type, no use of a method result decides the receiver, and a literal erased to `Any` or `Inspectable` takes the fallback. The prototype types the literal as `i32` on the spot and emits no hint. Task #262: a conversion to a trait value is no one-fit site, and widths whose methods declare different parameter lists make a literal receiver `ambiguous-method`; the prototype checks `cents.scale(4)` at `i32` and reports `argument-count`. |
| NO-IMPLIED-BOUND | Task #261: a generic type's declared bounds are never implied where the type is written, so a declaration that writes `Map[K, V]` must bound its own `K`. The prototype reports the missing bound, but repeats it at each method's implicit `self` and turns the field's type into `void`. |
| QUALIFIED-PATH | Task #260: a used module name qualifies a function, a type, a variant, a variant pattern, and an associated call. Task #262: a module path to a private or missing declaration reports `private-import` or `unknown-import`, and an absolute path outside a use is `unknown-name`. The prototype rejects `use pkg.words` as `unsupported-package-use`. |
| DOC-TESTS | Owner design, 2026-10-04: each fenced `hd` block in a `##` comment under `src/` is a doc test, compiled as its own program with the public view and run by `hd test` (`module.test.doc.*`, `cli.test.doc.*`). The prototype ignores the blocks, so `hd test` runs none, and `hd check --tests` checks none. |
| AMBIGUOUS-TYPE | Owner, 2026-10-04: inference with several valid solutions is `ambiguous-type` (`types.infer.ambiguous.code`). The prototype reports `cannot-infer-type` for a callback row that fits two generic keys in either order. |
| SHADOW-TPARAM | Owner, 2026-10-04: no declaration within a type parameter's scope may reuse its name (`names.type-param.no-redeclare`). The prototype renames a method's shadowing binder, and accepts a local `data T` or value `T` inside `fn work[T]`. |
| VARIANCE-MUT-SELF | Owner, 2026-10-04: a `mut self` inherent method counts toward declared variance (`types.variance.surface.mut-self`). The prototype skips those methods. |
| ALIAS-MISSING | Owner, 2026-10-04: an alias whose right side names nothing is `unknown-type`, or `unknown-trait` for a row key, at the alias, used or not (`types.alias.target-unknown`). The prototype checks the right side only where the alias is used. |
| PRIVATE-STD | Owner, 2026-10-04: a module path to a `std` declaration without `pub` is `private-import` (`expr.name.qualified.private`). The prototype reports `unknown-name` for the module. |
| DERIVE-MISSING | Owner, 2026-10-04: `@derive` of a name that resolves to nothing is `unknown-trait` (`annot.derive.unknown`). The prototype reports `underivable-trait`. |
| DOC-ROOT-NAME | Owner, 2026-10-04: the `<module>` of `src/lib.hd` is `pkg` in a doc test's name and a snapshot file's path (`cli.test.doc.name.root`, `std-testing.snapshot-file.module.root`). The prototype's snapshot files for `src/lib.hd` go to `__snapshots__/lib/`. No fixture reaches it. |

## Gaps No Fixture Reaches

- **Host boundary shapes**: the checker rejects boundary types the
  specification allows (`module.boundary.allowed`,
  `module.profile.host-result.shape`): an enum, a `Map`, or a
  `Result[T, E]` with a structural `T` reports
  `unsupported-host-provider-signature`. Data with private fields is
  correctly rejected (`module.boundary.pub`), pinned by the
  `host structural results expose only public data fields` test.
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
