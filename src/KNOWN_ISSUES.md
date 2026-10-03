# Known Issues In The Prototype

The prototype compiler in `src/` lags the specification. This file lists
what it gets wrong today. Each finding has one line: its F-id, its effect,
and the conformance fixture that shows it, if one does. Design questions
are in [future-work/OPEN_ISSUES.md](../future-work/OPEN_ISSUES.md), and
git history keeps the audit evidence behind each finding.

## Known Failures

[`test/portable/KNOWN_FAILURES.tsv`](../test/portable/KNOWN_FAILURES.tsv)
lists the conformance cases the prototype fails. Each row is tagged with a
finding below or with an applied decision. On 2026-10-03 the suite has
2,173 cases: 2,057 selected in `test/portable/cases.tsv` and 116 known
failures. The selected cases are 1,813 language tier and 244 stdlib tier;
the known failures are 83 language tier and 33 stdlib tier.

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
| VOID-UNIT | 1 | `void` is kept apart from the empty tuple `()` |
| STD-HELPERS | 8 | std helper declarations remain blocked by eager host glue and unresolved helper/provider support |
| ERR-HELPERS | 1 | trait-value `find` enters a forwarding adapter with static TypeId evidence and traps on an illegal cast |
| HOST-CONTRACT | 2 | host results are not checked against their declared types and the panic category is absent |
| HOST-NAN | 1 | the `special-float-host` runtime profile does not exist |
| PENDING-WRITE | 2 | the pending-write runtime profile does not exist |
| FLOAT-PARSE | 4 | the correctly rounded `parse_f64` host hook is absent |
| SNAPSHOT-ROW | 3 | snapshot operations do not use the current TestRunner row |
| RUNNER-SURFACE | 1 | PropertyRunner lacks the current PropertyCase protocol |
| LIST-POP | 4 | `List.pop`, `insert`, `remove_at`, `clear`, and their list-truncate hook are absent |
| STD-DEBUG | 1 | std derivation ordering leaves TypeId and SelfRef without Debug |
| ALL-LIST | 1 | erasing concrete closures in generic lists can produce an illegal cast |
| METHOD-DEFAULT | 1 | the parser rejects a default value on a method parameter |
| FIXED-HOOK | 1 | the `format_f64_fixed` host hook is absent |
| STD-1 | 3 | the checker accepts bounded inherent Map impls, but the specified Map methods are absent from `lib/std` |
| RETRY-WITH | 1 | `retry_with!` is held because its current std dependency would load `std.time` eagerly |
| SPEC-U32-DRIFT | 6 | fixtures still encode the retired signed-size behavior; `audit/compiler/u32-spec-drift.md` records the conflicts |
| TEST-REG-ID | 4 | test registration recognizes a bare spelling instead of the imported declaration identity |
| EQ-CONTEXTUAL | 2 | equality does not contextually type a variant from the opposite operand |
| FRESH-MUT | 1 | generic inference does not weaken `mut T` and `T` to their readonly join |
| FORWARD-BOUNDS | 1 | inference does not solve a bound that names a later type parameter |
| QUALIFIED-PREFIX | 3 | a string prefix after `.` does not report `qualified-string-prefix` |

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
| HOST-CATALOG | Batch 64: the default profile binds `Args`, `Env`, `ConsoleInput`, `Clock`, `Random`, `FsRead`, and `FsWrite`, with free helpers over them; `Console` gains `write_error_line!` and `eprintln`; `std.task` gains `Backoff` and `retry_with!`. The prototype binds only `Console`, and of the new items it declares only `Random`. |
| STD-1 | Batch 64 specifies Map helpers. Bounded inherent Map implementations are now legal, but `lib/std/collections.hd` does not yet define `get_or`, `keys`, or `values`. |

## Gaps No Fixture Reaches

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
