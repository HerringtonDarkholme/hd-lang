# Migrated TypeScript Tests

These TypeScript tests now have implementation-neutral conformance fixtures
under `spec/conformance/`. Each line maps one test to its fixtures, or to the
existing fixture that already covered it. A test marked "not migrated" stays
in TypeScript, for the reason given. Delete a test only when its line names
fixtures or duplicates and no "not migrated" part. Remove a line once its
test is deleted, and delete this file when it is empty.

Paths are relative to `spec/conformance/`.

## test/requirement-key-validation.test.ts

- test/requirement-key-validation.test.ts :: aliases are expanded before requirement keys are validated -> typing/invalid/alias-unknown-target.hd, typing/invalid/row-alias-unknown-key.hd (Q5, ALIAS-MISSING); the TS test asserts a result the decision makes wrong: it expects `unknown-trait` at the use `$ MissingAlias`, and the spec gives `unknown-type` at the alias declaration

## test/generic-callable-requirements.test.ts

- test/generic-callable-requirements.test.ts :: unordered requirement keys do not guess an ambiguous binder mapping -> typing/invalid/ambiguous-requirement-key-solution.hd, typing/valid/ambiguous-requirement-key-annotated.hd (Q6, AMBIGUOUS-TYPE); the TS test asserts a result the decision makes wrong: it expects `cannot-infer-type`, and the spec gives `ambiguous-type`

## test/typed-derivation.test.ts

- test/typed-derivation.test.ts :: typed derivation reports its diagnostics at the opt-in -> duplicates of typing/invalid/derive-member-not-derivable.hd, typing/invalid/derive-error-trait.hd, typing/invalid/unknown-annotation-member.hd, typing/invalid/omitted-member-without-default.hd, and typing/invalid/derive-before-function.hd; `@derive(Missing)` is typing/invalid/derive-unknown-trait.hd (Q8, DERIVE-MISSING), and the TS test asserts a result the decision makes wrong, since it expects `underivable-trait` and the spec gives `unknown-trait`

## test/compiler-suspension.test.ts and test/suspension.test.ts (pending-first-poll)

Each fixture runs under `# fixture-runtime-scenario: pending-first-poll`. Its suspension points are `Console.write_line!` host calls, because the scenario makes only host calls pending. The poll-order and WAT assertions of the TypeScript tests are implementation details and stay there.

- test/compiler-suspension.test.ts :: CFG suspension lowering preserves nested argument order -> runtime/valid/pending-first-poll-argument-order.hd; the per-function poll trace is not migrated
- test/compiler-suspension.test.ts :: CFG suspension lowering branches and short-circuits around child frames -> runtime/valid/pending-first-poll-branches.hd; the poll trace is not migrated

## test/cli.test.ts and test/cli-commands.test.ts (CLI cases)

Paths `cli/NAME` are CLI cases under `spec/conformance/cli/`, indexed by `cli-cases.tsv`.

- test/cli.test.ts :: the hd executable reports an unknown command with its usage text; an internal error that escapes main ends the process with status 1 -> cli/exit-program-status (the ExitCode(3) status), cli/exit-usage-error (unknown commands), cli/json-run (the FILE --format json passthrough); the usage text and the adapter entry error are not migrated
- test/cli.test.ts :: removed CLI commands name themselves as unknown commands -> cli/exit-usage-error (the 101 status of each removed command); the usage text is not migrated
- test/cli.test.ts :: hd test fails a test whose result is .Err -> cli/exit-test-failure; the message text is not migrated
- test/cli.test.ts :: hd run and hd test judge suspending results by Termination -> cli/exit-program-status (statuses 0, 3, and 1), cli/exit-test-failure; the message text is not migrated
- test/cli.test.ts :: hd test compares snapshot_file text with its snapshot file -> cli/test-snapshot-file (the missing-file 1, the --update 0 and snapshot file); the recorded content, the pass count, and the tamper verdict stay
- test/cli.test.ts :: hd test fails a test case that runs longer than its timeout -> cli/test-timeout (the pass 0 and the overrun 1); the pass count and the timeout text stay
- test/cli.test.ts :: documented CLI commands work end to end -> not migrated: hd parse and hd debug hir have no cli.* rules, and the check/test/run/build wordings are message text
- test/cli.test.ts :: hd run on a module without main exits 0 and prints nothing -> not migrated: no cli.* rule states the no-entry behavior (owner decision, batch 42; F-265)
- test/cli.test.ts :: hd run on a script that needs Console prints and exits 0 -> not migrated: the run-path provider binding is language-tier (entry requirement rows)
- test/cli.test.ts :: hd run runs Console.write_line! on host and program providers -> not migrated: Console routing is language-tier (10-modules console, MHP-1), and the assertions are message output
- test/cli.test.ts :: a failed assert_equal shows the reason and both values -> not migrated: message wording under r-module.testing.assert-equal-debug (language tier)
- test/cli.test.ts :: hd test runs a _test.hd test module -> not migrated: test-module placement and the misplaced-tests-block wording are language-tier
- test/cli.test.ts :: hd test runs each it_each row in a fresh instance -> not migrated: row isolation is std/lang-tier (table-test-rows, r-module.testing.instance); the assertions are verdict text
- test/cli.test.ts :: hd test shrinks a failing property case -> not migrated: shrink accounting, seeds, and regression files are runner internals under std-testing.prop rules
- test/cli.test.ts :: hd test runs property examples first -> not migrated: example ordering is std-tier (r-std-testing.prop.examples); the assertion is verdict text
- test/cli.test.ts :: hd test fails a property that discards too many cases -> not migrated: discard accounting is std-tier (r-std-testing.prop.discard-limit); the assertion is verdict text
- test/cli-commands.test.ts :: hd test on a package tests each module, and with no path the current package -> cli/exit-test-failure, cli/exit-package-file; the printed counts and the loose-directory case are not migrated
- test/cli-commands.test.ts :: hd run, check, and build on a package file link the package -> cli/exit-package-file for the check; the run and build forms are not migrated (cli.run.file makes `hd run FILE` an error)
- test/cli-commands.test.ts :: hd run resolves super uses, and reports a package error in its own file -> cli/json-file-location for the file of the error; the run part is not migrated

## test/std/*.hd (stdlib-tier tests, run by test/std.test.ts)

The hd files stay: `test/std.test.ts` still runs each through `hd test`. A
later task deletes them.

- test/std/annotation.hd :: finds each attached value by its type -> runtime/valid/facts-of-literal-generic-none.hd
- test/std/annotation.hd :: reads a generic function's values -> runtime/valid/facts-of-literal-generic-none.hd
- test/std/annotation.hd :: a function without decorators holds none -> runtime/valid/facts-of-literal-generic-none.hd
- test/std/collections.hd :: map and filter -> runtime/valid/list-and-optional-map.hd for `map`; `filter` is a duplicate of runtime/valid/list-access-building.hd
- test/std/collections.hd :: first, last, and reversed -> duplicate of runtime/valid/list-access-building.hd
- test/std/collections.hd :: sorted_by is stable -> duplicate of runtime/valid/list-access-building.hd
- test/std/collections.hd :: chunks and zip -> duplicate of runtime/valid/list-chunks.hd and runtime/valid/list-access-building.hd
- test/std/collections.hd :: chunks rejects a zero size -> duplicate of runtime/panic/list-chunks-zero.hd
- test/std/collections.hd :: Set writes one debug_list entry per element, in insertion order -> not migrated: the test asserts the compact text `[2, 1]` of `debug(set)`, and `std-format.debug.render` says `debug` text is multi-line (Q10)
- test/std/collections.hd :: windows rejects a zero size -> duplicate of runtime/panic/list-windows-size.hd
- test/std/cmp-iter.hd :: min, max, and clamp -> duplicate of runtime/valid/cmp-min-max.hd and runtime/valid/cmp-clamp.hd
- test/std/cmp-iter.hd :: min and max give the first of equal values -> runtime/valid/cmp-min-max-distinguishable-tie.hd
- test/std/cmp-iter.hd :: clamp rejects an empty range -> duplicate of runtime/panic/cmp-clamp-reversed.hd
- test/std/cmp-iter.hd :: Reverse inverts the order -> duplicate of runtime/valid/cmp-reverse.hd
- test/std/cmp-iter.hd :: adapters -> duplicate of runtime/valid/iterator-adapters-run.hd
- test/std/cmp-iter.hd :: take reads only what it yields -> duplicate of runtime/valid/iterator-adapters-run.hd (`std-iter.adapter.take.limit`)
- test/std/cmp-iter.hd :: composites compare through std.cmp -> duplicate of runtime/valid/structural-ordering.hd, runtime/valid/structural-equality.hd, and runtime/valid/nan-ordering-composites.hd; `min` of two lists is in runtime/valid/cmp-min-max-distinguishable-tie.hd
- test/std/cmp-iter.hd :: repeated comparisons start each time from the first element -> not migrated: a regression guard of the prototype, with no rule beyond ordinary comparison
- test/std/handle-fact.hd :: reads a typed fact through a handle whose F has no Inspectable bound -> runtime/valid/handle-fact-exact-type.hd
- test/std/console.hd :: BufferConsole records each line -> duplicate of runtime/valid/buffer-console.hd
- test/std/console.hd :: println records inside a test body -> duplicate of runtime/valid/println-in-test-body.hd
- test/std/console.hd :: a new BufferConsole is empty -> duplicate of runtime/valid/buffer-console.hd
- test/std/derive.hd :: derived ordering is lexicographic in field order -> runtime/valid/derived-ordering-run.hd
- test/std/derive.hd :: derived enum ordering follows variant order, then payloads -> runtime/valid/derived-ordering-run.hd
- test/std/derive.hd :: derived partial ordering of floats -> runtime/valid/derived-ordering-run.hd
- test/std/derive.hd :: derived Hash hashes the fields in order -> duplicate of runtime/valid/hash-bytes-derived.hd
- test/std/derive.hd :: == uses a generic derived Eq -> runtime/valid/derived-equality-generic.hd
- test/std/derive.hd :: a newtype derives from its base type -> duplicate of runtime/valid/derived-newtype.hd
- test/std/derive.hd :: a type with Eq and Hash keys a map -> runtime/valid/map-key-types.hd
- test/std/error.hd :: an absent optional source ends the chain -> runtime/valid/error-chain-derived-causes.hd
- test/std/error.hd :: a transparent variant skips its member -> runtime/valid/error-chain-derived-causes.hd
- test/std/error.hd :: an erased error reports its causes -> runtime/valid/error-chain-derived-causes.hd
- test/std/error.hd :: an empty report displays its message alone -> duplicate of runtime/valid/error-report.hd
- test/std/hash.hd :: a string hashes its UTF-8 length as a u64, then its UTF-8 bytes -> duplicate of runtime/valid/hash-bytes-sequences.hd
- test/std/hash.hd :: integers hash their little-endian bytes at their own width -> duplicate of runtime/valid/hash-bytes-scalars.hd
- test/std/hash.hd :: a type parameter bounded by Eq and Hash keys a map -> runtime/valid/map-key-types.hd
- test/std/map.hd :: a char keys a map -> runtime/valid/map-key-types.hd
- test/std/map.hd :: an i64 keys a map -> runtime/valid/map-key-types.hd
- test/std/map.hd :: a derived Eq and Hash type keys a map -> runtime/valid/map-key-types.hd
- test/std/map.hd :: a type parameter keys a map -> runtime/valid/map-key-types.hd
- test/std/map.hd :: collect builds a map, the later value wins in the first position -> duplicate of runtime/valid/collect-targets-run.hd
- test/std/map.hd :: collect builds a map over a type-parameter key -> not migrated: no rule beyond `std-iter.collect.map`, which runtime/valid/collect-targets-run.hd covers for concrete keys
- test/std/map.hd :: a map entry meets a Display bound -> duplicate of runtime/valid/display-tuples.hd (an entry is a tuple)
- test/std/map.hd :: debug writes a map's entries in order -> not migrated: the test asserts the compact text `{"a": 1}`, and `std-format.debug.render` says `debug` text is multi-line (Q10)
- test/std/num.hd :: checked arithmetic reports overflow as None -> duplicate of runtime/valid/num-checked-wrapping.hd
- test/std/num.hd :: wrapping and saturating arithmetic -> duplicate of runtime/valid/num-checked-wrapping.hd, runtime/valid/num-saturating.hd, and runtime/valid/num-abs-diff.hd
- test/std/num.hd :: bit counts -> duplicate of runtime/valid/num-bit-counts.hd
- test/std/num.hd :: i64 methods -> duplicate of runtime/valid/num-every-width.hd
- test/std/num.hd :: f64 classification -> duplicate of runtime/valid/num-is-nan.hd and runtime/valid/num-is-finite.hd
- test/std/num.hd :: parse_i32 and parse_i64 -> duplicate of runtime/valid/num-parse-integers.hd
- test/std/num.hd :: every integer width -> duplicate of runtime/valid/num-every-width.hd
- test/std/providers.hd :: ManualClock starts where it is told and sleep! advances it -> duplicate of runtime/valid/manual-clock.hd
- test/std/providers.hd :: ManualClock covers a Clock row -> duplicate of runtime/valid/manual-clock.hd and runtime/valid/clock-helpers.hd
- test/std/providers.hd :: SeededRandom repeats its draws for a seed -> duplicate of runtime/valid/seeded-random.hd
- test/std/providers.hd :: SeededRandom covers a Random row -> duplicate of runtime/valid/seeded-random.hd and runtime/valid/rng-from-random.hd
- test/std/property.hd :: addition commutes -> runtime/valid/property-generators-scalars.hd
- test/std/property.hd :: draws stay in range -> runtime/valid/property-generators-scalars.hd
- test/std/property.hd :: lists stay short -> runtime/valid/property-generators-collections.hd
- test/std/property.hd :: assume discards odd values -> runtime/valid/property-generators-scalars.hd; also a duplicate of runtime/valid/property-assume-discards.hd
- test/std/property.hd :: a string is at most 16 chars -> not migrated: the spec gives `Arbitrary` for `string` no length limit, and `Choices` has no size (`std-testing.choices.no-size`) (Q11)
- test/std/property.hd :: a map holds at most max entries -> runtime/valid/property-generators-collections.hd
- test/std/property.hd :: int takes its type from the context -> runtime/valid/property-generators-scalars.hd
- test/std/property.hd :: the draw budget ends recursion -> runtime/valid/property-generators-collections.hd; also a duplicate of runtime/valid/property-draw-budget.hd
- test/std/property.hd :: an f64 may be any value -> runtime/valid/property-generators-scalars.hd
- test/std/option-result.hd :: option map and unwrap_or -> runtime/valid/list-and-optional-map.hd for `map`; `unwrap_or` is a duplicate of runtime/valid/option-and-then.hd
- test/std/option-result.hd :: option ok_or, is_some, is_none, expect -> duplicate of runtime/valid/option-tests-conversions.hd
- test/std/option-result.hd :: option expect panics on None -> duplicate of runtime/panic/option-expect-none.hd
- test/std/option-result.hd :: result map and map_err -> duplicate of runtime/valid/result-map.hd and runtime/valid/result-and-then.hd
- test/std/option-result.hd :: result ok, err, is_ok, is_err, unwrap_or, expect -> duplicate of runtime/valid/result-tests-conversions.hd and runtime/valid/result-and-then.hd
- test/std/sized-numeric.hd :: narrow signed arithmetic stays in range -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: i8 overflow panics -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: i8 MIN / -1 panics -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: unsigned 32-bit values compare and display as unsigned -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: u32 subtraction below zero panics -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: u64 keeps its full range -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: u64 overflow panics -> runtime/valid/sized-integer-arithmetic.hd
- test/std/sized-numeric.hd :: numeric casts convert in range -> runtime/valid/numeric-casts-in-range.hd
- test/std/sized-numeric.hd :: an out-of-range integer cast wraps -> duplicate of runtime/valid/narrowing-cast-wraps.hd
- test/std/sized-numeric.hd :: an out-of-range float cast saturates -> duplicate of runtime/valid/float-cast-saturates.hd
- test/std/sized-numeric.hd :: f32 arithmetic keeps f32 width -> runtime/valid/numeric-casts-in-range.hd
- test/std/sized-numeric.hd :: narrow shifts keep the width -> runtime/valid/sized-integer-arithmetic.hd
- test/std/time-process.hd :: Duration counts whole milliseconds -> duplicate of runtime/valid/duration-api.hd
- test/std/time-process.hd :: Duration arithmetic and Display -> duplicate of runtime/valid/duration-arithmetic.hd and runtime/valid/duration-display.hd
- test/std/time-process.hd :: Termination reports exit codes -> duplicate of runtime/valid/termination-report.hd
- test/std/time-process.hd :: ScriptedProcess answers by program -> duplicate of runtime/valid/scripted-process.hd
- test/std/text.hd :: is_empty, ends_with, and contains -> duplicate of runtime/valid/string-more-methods.hd
- test/std/text.hd :: find returns a byte offset -> duplicate of runtime/valid/string-more-methods.hd
- test/std/text.hd :: trim_start, trim_end, and upper -> duplicate of runtime/valid/string-more-methods.hd
- test/std/text.hd :: strip_prefix and strip_suffix -> duplicate of runtime/valid/string-more-methods.hd
- test/std/text.hd :: lines and repeat -> duplicate of runtime/valid/string-lines.hd and runtime/valid/string-repeat.hd
- test/std/text.hd :: join and StringBuilder -> duplicate of runtime/valid/text-join-builder.hd
- test/std/text.hd :: to_utf8 and string::from_utf8 -> duplicate of runtime/valid/utf8-valid-text.hd, runtime/valid/utf8-invalid-bytes.hd, runtime/valid/utf8-truncated.hd, and runtime/valid/utf8-overlong.hd
- test/std/text-prefix.hd :: interpolate joins pieces and values in order -> duplicate of runtime/valid/text-prefix-helpers.hd
- test/std/text-prefix.hd :: process_escapes replaces each escape -> duplicate of runtime/valid/text-prefix-helpers.hd
- test/std/text-prefix.hd :: a prefix can process escapes in its pieces -> duplicate of runtime/valid/text-prefix-helpers.hd
- test/std/tuple-text.hd :: writes a one-element tuple with a comma -> the Display half is a duplicate of runtime/valid/display-tuples.hd; the Debug half is not migrated: it asserts the compact text `(1,)` of `debug((1,))` (Q10)
- test/std/tuple-text.hd :: writes the empty tuple and wider tuples without one -> not migrated: it asserts the compact `debug` text of `()`, `(1, 2)`, and `Some(1)` (Q10)

## test/package-relative.test.ts

- test/package-relative.test.ts :: ordinary source files resolve self children and super siblings from their own module -> duplicate of runtime/valid/relative-self-top-level.hd
- test/package-relative.test.ts :: nested files resolve child, sibling and repeated-parent uses -> duplicate of runtime/valid/relative-self-current.hd for `self` and `super`; typing/valid/relative-repeated-super.hd for `super.super`
- test/package-relative.test.ts :: ordinary source files cannot move above the package root -> not migrated: `module.relative.above-root` names no error code, and a fixture needs one (Q12)

## test/package.test.ts

- test/package.test.ts :: a use of another module's public declarations links and runs -> duplicate of runtime/valid/init-group-order.hd
- test/package.test.ts :: relative uses, re-exports, and initialization order follow the use graph -> typing/valid/relative-repeated-super.hd for `super.super`; duplicate of typing/valid/pub-use-chain.hd and runtime/valid/init-group-order.hd
- test/package.test.ts :: package use errors point at the use declaration of their file -> typing/invalid/unknown-package-name.hd for `unknown-import`; duplicate of typing/invalid/unknown-pkg-module.hd, typing/invalid/private-package-name.hd, typing/invalid/unknown-dep-module.hd, and typing/invalid/root-file-super.hd; the namespace and `as` uses that now link duplicate runtime/valid/relative-self-current.hd and typing/invalid/module-path-missing-member.hd
- test/package.test.ts :: folders that depend on each other in a loop are rejected -> duplicate of typing/invalid/folder-cycle-facade.hd, typing/valid/folder-cycle-leaf-folder.hd, and typing/invalid/folder-cycle-nested.hd; the message text is not migrated
- test/package.test.ts :: shared names and bad paths are rejected -> not migrated: two paths that fold to one module are `duplicate-module-name` (`module.path.case-collision`), but no fixture can hold both on a case-insensitive file system; a used name that collides with a declaration has no code (`names.use.no-collision`), and `unclosed-delimiter` appears only in the README table (Q13)
- test/package.test.ts :: single-declaration uses and the package root module resolve -> typing/valid/use-path-only-declaration.hd for the path-only use of one declaration; the `src/mod.hd` root is not migrated, since `module.path.no-root-mod` makes it an error, and typing/valid/lib-root-pkg.hd covers `pkg.{X}` from `src/lib.hd`

## Mixed-file rows (batch 3)

- test/inherent-implementation-bounds.test.ts :: a bounded standard inherent Map implementation keeps its key bounds -> not migrated: it marks a user implementation as `standard`, which user code cannot do (a user `impl Map[K, V]` is `orphan-impl`)
- test/inherent-implementation-bounds.test.ts :: an unbounded standard inherent Map implementation still fails its key bound -> not migrated: same reason

## Batch 4 rows (test/suspension.test.ts, test/captured-cells.test.ts)

- test/captured-cells.test.ts :: shared capture lookup uses explicit closure index rather than array position -> not migrated: asserts unit-only HIR cell rewriting
- test/captured-cells.test.ts :: cell conversion is idempotent and keeps distinct activations' local identities separate -> not migrated: asserts unit-only HIR cell rewriting
- test/captured-cells.test.ts :: typed rewriting reaches dictionary bounds and match tests while leaving metadata intact -> not migrated: asserts unit-only HIR cell rewriting

## Batch 4 rows (test/call-speculation.test.ts)

- test/call-speculation.test.ts :: the remaining eight tests (registry traversal, journal unwind, aliases, savepoints, Map order, graph reachability, nested trials, cyclic graphs) -> not migrated: assert checker-internal speculation journals and snapshots

## Batch 4 rows (test/compiler-suspension.test.ts)

- test/compiler-suspension.test.ts :: all!/race! poll order, cancellation, and trace sequences -> not migrated: assert trace event orders and manual __hd_poll/__hd_cancel driving
- test/compiler-suspension.test.ts :: race! with an empty list literal / over a list empty at run time -> not migrated: the fixtures are KNOWN_FAILURES rows (tag RACE-EMPTY); the TS tests pin the current behavior
- test/compiler-suspension.test.ts :: host record/replay, encoding, validation, console routing -> not migrated: assert the host bridge, not language behavior
- test/compiler-suspension.test.ts :: the CONF-behavior tests (frames, dictionaries, child pending, cancellation unwinding, CFG lowering, block_on, module bindings) -> behavior already covered by their conformance fixtures; the frame/trace assertions stay [kept]

## Batch 4 rows (test/compiler.test.ts)

- test/compiler.test.ts :: cannot-infer-type message text -> not migrated: asserts exact diagnostic messages; the codes are already covered by typing/invalid/cannot-infer-type.hd and its siblings
- test/compiler.test.ts :: optional and Result context errors have stable diagnostics -> not migrated: asserts codes on existing fixtures; the TS test stays as the message/code pin
- test/compiler.test.ts :: the CONF-behavior tests (power, varargs, defaults, named args, enums, strings, defer, panic, patterns, closures, function types) -> behavior already covered by their conformance fixtures; the WAT/HIR/trace assertions stay [kept]
- test/compiler.test.ts :: checker HIR shape, Wasm GC smoke, panic-tag consistency, panic-detail import flag -> not migrated: assert compiler internals

## Batch 4 rows (test/compiler-types.test.ts)

- test/compiler-types.test.ts :: built-in comparison dictionaries carry their supertrait dictionaries (EQ-1) -> runtime/valid/ord-supertrait-dispatch.hd and typing/invalid/float-misses-ord-bound.hd; the HIR supertrait assertions stay [kept]
- test/compiler-types.test.ts :: a type parameter calls an associated function through its bound (TQ-9) -> not migrated: asserts HIR trait-call/dictionary shape; behavior already covered by runtime/valid/associated-function-calls.hd
- test/compiler-types.test.ts :: a generic inherent implementation lowers to a generic function (TQ-19) -> runtime/valid/generic-inherent-box.hd; the HIR genericParameters assertion stays [kept]
- test/compiler-types.test.ts :: i64 literals, explicit widening, checked arithmetic, and narrowing (F-253) -> runtime/valid/i64-widening-checked.hd and typing/invalid/implicit-narrowing-i64.hd; the fix-it edit assertions stay [kept]
- test/compiler-types.test.ts :: println drives a write_line! pending on a host operation until it finishes (MHP-1) -> not migrated: needs a custom hostSuspensionPending schedule
- test/compiler-types.test.ts :: u8 checked arithmetic and ExitCode entry results (T8) -> runtime/valid/u8-checked-add.hd and runtime/panic/u8-add-overflow.hd; the ExitCode entry halves stay [kept]
- test/compiler-types.test.ts :: the other 37 tests (reify, erasure, provider packs, row forwarding, dictionaries, GC layouts, HIR permissions) -> not migrated: assert Wasm/HIR lowering shapes

## Batch 4 rows (test/types.test.ts)

- test/types.test.ts :: inference advice preserves optional permission boundaries in source annotations -> not migrated: asserts internal canonical type strings through unit-only helpers
- test/types.test.ts :: optional payload permission and optional outer permission have distinct canonical types -> not migrated: asserts internal canonical type strings
- test/types.test.ts :: nesting preserves the scope of each mut prefix and optional constructor -> not migrated: asserts internal canonical type strings
- test/types.test.ts :: function optionality remains distinct from result optionality -> not migrated: asserts internal canonical type strings
- test/types.test.ts :: type substitution and generic matching preserve permission constructor boundaries -> not migrated: asserts unit-only substitution helpers
- test/types.test.ts :: deep permission erasure rebuilds canonical option and function types -> not migrated: asserts a unit-only erasure helper
- test/types.test.ts :: display rendering hides compiler-internal names but keeps user spellings -> not migrated: asserts the unit-only display renderer
- test/types.test.ts :: source type rendering round-trips mutable constructors rather than changing prefix scope -> not migrated: asserts internal canonical type strings
- test/types.test.ts :: optional type identity includes payload mut and not outer mut -> not migrated: asserts the unit-only inspect key splitter
- test/types.test.ts :: the REPL renders invariant optional mutable payloads without erasing their generic permissions -> not migrated: REPL session behavior, no portable fixture form
- test/types.test.ts :: the REPL preserves mutable outer optionals nested inside another optional -> not migrated: REPL session behavior, no portable fixture form
- test/types.test.ts :: typed build uses the shared optional constructor for declared mutable members -> not migrated: asserts another fixture file passes, not language behavior
