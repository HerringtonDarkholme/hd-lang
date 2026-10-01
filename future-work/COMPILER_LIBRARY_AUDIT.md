# Compiler/Library Audit: What The Prototype Implements In TypeScript

Status: research and audit, 2026-10-01. Nothing here changes the spec,
`src/`, or `lib/std`. It answers the owner's complaint of 2026-10-01:

> your impl is so fucking bad, a lot of things are implemented directly in
> compiler

It lists the features the TypeScript prototype implements by hand, sorts
them by whether they belong there, and orders the work that moves them
into `lib/std`. It extends the
[Compiler/Library Boundary](../src/README.md#compilerlibrary-boundary)
audit of 2026-09-28, which moved the string methods, `println`, and
`debug` into hd.

Inputs: the tier table in [AGENTS.md](../AGENTS.md#spec-scope-for-the-standard-library),
[Spec Tiers](SPEC_TIERS.md), [spec/std/](../spec/std/README.md),
`npm run spec -- counts` (3,616 language rules, 180 stdlib rules, 111
rules the heuristic calls intrinsic), `src/README.md`, `ast-grep outline`
of `src/checker` and `src/emitter`, and the 246 rows of
`test/portable/KNOWN_FAILURES.tsv`.

## Contents

1. [Summary](#summary)
2. [Inventory](#inventory)
3. [Classification](#classification)
4. [Spec Mismatches](#spec-mismatches)
5. [Migration Plan](#migration-plan)
6. [Pending Prototype Tasks The hd Way](#pending-prototype-tasks-the-hd-way)
7. [Questions For The Owner](#questions-for-the-owner)

## Summary

`src/` holds 46,395 lines of TypeScript and WAT. `lib/std` holds 3,434
lines of hd. The classes:

| Class | Meaning | TS lines | Share |
| --- | --- | --- | --- |
| A | True intrinsic: a spec rule names it compiler-supplied, or hd cannot express it | about 6,370 | 14% |
| B | Movable now: hd in `lib/std` with today's prototype | about 400 | 1% |
| C | Movable after a small hook | about 1,500 | 3% |
| D | Language semantics, and tooling | about 38,120 | 82% |

The owner's complaint holds for about 1,900 lines (B and C). They are
library features that the prototype wrote as checker cases, HIR nodes,
emitter strategies, or WAT. Seven of them depart from a spec rule that already
says "standard library" or "template" ([Spec Mismatches](#spec-mismatches)).

A further 1,050 lines in class A are A only because a spec rule says
"intrinsic": the `@derive(Eq, PartialOrd, Ord, Hash)` generator (about
385) and `@error` (663). Q4 asks whether the first should become std
templates, as `@derive(Debug)` and derived `Arbitrary` already are.

Most of the 82% is honest compiler work: the parser, type checking,
suspension lowering, codegen, and the CLI. The tooling alone (CLI, REPL,
`hd explain`/`def`/`doc`, the spec index, the package linker) is about
3,700 lines.

The best chunks, by TS deleted per hour:

| Chunk | Work | TS deleted |
| --- | --- | --- |
| M1 | Generic impl lookup, then `Eq` and `PartialOrd` for `List`, `T?`, `Result`, `Map` in hd | about 400 |
| M2 | Prelude traits declared in `lib/std`, found by standard name | about 345 |
| M3 | `it_each`, `it_prop`, `it_prop_with`, and `timeout` as hd functions | about 285 |
| M4 | `assert`, `assert_equal`, and `snapshot` as hd functions | about 250 |
| M5 | `std.structure`, `std.inspect`, and `std.testing.arbitrary` as hd files | about 175 |

Line counts below are from `wc -l` and function boundaries. A count for
part of a file is a range estimate, good to about 20%.

## Inventory

One row per feature that the compiler knows by name or implements by
hand. "Spec" names the rule or stdlib chapter that covers it. Class is
defined in [Classification](#classification).

### Derivation And Traits

| Feature | TS files | Lines | Spec | Class |
| --- | --- | --- | --- | --- |
| `@derive(Debug)` generator | `checker/derive-intrinsics.ts` (`deriveDebug`, Debug in `deriveNewtypeIntrinsic`), `checker/typed-derivation.ts` (`INTRINSIC_DERIVES`) | 55 | [`trait.debug.derive`](../spec/09-traits.md#r-trait.debug.derive) says template; [Format](../spec/std/format.md) | B |
| `@derive(Eq, PartialOrd, Ord, Hash)` generator, law partners | `checker/derive-intrinsics.ts` less Debug | 385 | [`trait.derive.intrinsic-set`](../spec/09-traits.md#r-trait.derive.intrinsic-set), Law Partners | A by spec; C after Q4 |
| `@error` derivation | `checker/error-derivation.ts`, `checker/error-generation.ts` | 663 | [`annot.error.intrinsic`](../spec/14-annotations.md#r-annot.error.intrinsic) | A |
| Typed derivation: `Structure`, handles, `walk`/`describe`/`build` | `checker/typed-derivation.ts` less the source string, `self-ref.ts`, `derivation-models.ts`, `member-lines.ts`, `declaration-facts.ts` | 2,013 | [`annot.derive.supplied`](../spec/14-annotations.md#r-annot.derive.supplied) | A |
| `std.structure` declarations as a TS string | `checker/typed-derivation.ts` (`STRUCTURE_SOURCE`) | 100 | [`annot.structure.bodies`](../spec/14-annotations.md#r-annot.structure.bodies) | B |
| `Inspectable`, `TypeId` declarations as a TS string | `checker/standard-traits.ts` (`INSPECT_SOURCE`) | 15 | [Runtime Type Identity](../spec/09-traits.md#runtime-type-identity) | B |
| `std.testing.arbitrary` submodule shim | `checker/arbitrary-module.ts` | 59 | [Derived Arbitrary](../spec/std/testing.md#derived-arbitrary) | C |
| Builtin `Debug` dictionary for primitives and composites ("writes nothing") | `checker/debug.ts`, `context.ts` plan, emitter `debug` branch, HIR | 78 | [`trait.debug.std-types`](../spec/09-traits.md#r-trait.debug.std-types): `std` implements it | C |
| Prelude traits declared in TS: `Display`, `Eq`, `PartialOrd`, `Ord`, `Ordering`, `Debug`, `Iterable`, `Any`, `Waker`, `Console`, `ResourceError` | `checker/program-types.ts` (`declareProgramTypes` tail, `declareComparisonTraits`) | 230 | [Prelude](../spec/10-modules.md#prelude), [Comparison Traits](../spec/09-traits.md#comparison-traits) | C |
| Std-name shims: hidden `Duration`, `ExitCode`, `Termination`, `DebugWriter`, `STANDARD_TRAITS` | `checker/standard-traits.ts` less `INSPECT_SOURCE` | 115 | lang items; [Time](../spec/std/time.md) | C |
| Runtime type identity: `downcast`, `downcast_val`, `TypeId::of`, keys | `checker/expression-inspect.ts`, `checker/inspectable.ts` | 508 | [`trait.inspect.supplied`](../spec/09-traits.md#r-trait.inspect.supplied) | A |
| `shape`, `shape_of` builders | `checker/shapes.ts` | 461 | [`module.prelude.shape`](../spec/10-modules.md#r-module.prelude.shape) | A |
| Literal suffix and prefix markers | `checker/literal-suffixes.ts` | 148 | [Literal Suffixes](../spec/05-expressions.md#literal-suffixes) | A |
| `Termination`, entry results | `checker/termination.ts` | 161 | [Executable Entry Point](../spec/10-modules.md#executable-entry-point) | A |

### Comparison, Display, And Collections

| Feature | TS files | Lines | Spec | Class |
| --- | --- | --- | --- | --- |
| `==` and `<` on `List`, `T?`, `Result`, `Map` as emitter strategies | `emitter/value-comparison.ts` (option, list, map, variant emitters), `checker/context.ts` (`equalityStrategy`, `orderingStrategy`, builtin Eq/Ord plan, `renumberBoundDispatches`), HIR strategy types | 400 | [`expr.eq.std`](../spec/05-expressions.md#r-expr.eq.std), [`expr.ord.std`](../spec/05-expressions.md#r-expr.ord.std): "standard-library implementations" | C |
| Tuple `==` and `<` strategies | `emitter/value-comparison.ts` (`emitTupleEquality`, `emitTupleOrdering`), checker tuple branches | 82 | [`trait.target.tuple.derived`](../spec/09-traits.md#r-trait.target.tuple.derived): intrinsic, every arity | C (no hook if Q3 is B) |
| Primitive `Display`: integer, `char`, `bool` text | `display` HIR node, `emitPrimitiveDisplay`, `runtime.wat` `i32_to_string`, `i64_to_string`, `u64_to_string`, `char_to_string` | 160 | [`expr.interp.std`](../spec/05-expressions.md#r-expr.interp.std): "the standard library provides" | B |
| Float text, `**`, float `%` | `runtime/float.wat`, host `format_f64` | 50 | operators and interpolation | A |
| `FromIterator` for `Map` | `checker/assignability.ts` (`mapCollectionPlan`), `value-comparison.ts` (`emitMapCollection`), HIR `map-collection` | 69 | [Collect Targets](../spec/std/iter.md#collect-targets) | B |
| `Map` storage, key equality, key kinds | `runtime/map.wat`, `shared.ts` (`mapKeyKind`, `setHashableKeyTypes`) | 233 + 30 | representation intrinsic ([Tier Criteria](SPEC_TIERS.md#tier-criteria)); [`types.map-key.declared-bound`](../spec/04-type-system.md#r-types.map-key.declared-bound) | A; key kinds D |
| `List` storage, cursors, invalidation | `runtime.wat` vectors, `emitter/iterator.ts`, `checker/iteration.ts` | 439 + 152 | Built-In Collection Iteration (06) | A and D |
| Checked arithmetic, sized integers, string primitives | `runtime.wat` rest, `emitter/sized-numeric.ts`, `numeric.ts` | 414 + 305 | 04, 05 | D |

### Testing

| Feature | TS files | Lines | Spec | Class |
| --- | --- | --- | --- | --- |
| `assert`, `assert_equal`, `snapshot` as HIR nodes | `checker/expression-calls.ts` (190), `emitter/function-body.ts` (42), `suspension.ts`, HIR | 250 | [Standard Testing](../spec/10-modules.md#standard-testing): harness; `snapshot`'s literal stays language | C |
| `it_each` rows, `it_prop`, `it_prop_with`, `timeout` | `parser/test-cases.ts` (`tableTest`, `propertyTest`), `checker/program-declarations.ts` (timeout), `checker/calls.ts`, HIR `each-row-index`, `each-row-count`, `test-timeout`, runtime globals | 285 | [Testing](../spec/std/testing.md): stdlib tier | C |
| Test runner: cases, rows, timeouts | `test-runner.ts` | 170 | runner | A (host tool) |
| Property runner: PRNG, draws, shrinking, regressions | `property-tests.ts` | 303 | [Property Tests](../spec/std/testing.md#property-tests) | A (host tool) |
| Snapshot files | `snapshots.ts` | 90 | [Snapshot Files](../spec/std/testing.md#snapshot-files) | A (host tool) |

### Host Boundary And Tasks

| Feature | TS files | Lines | Spec | Class |
| --- | --- | --- | --- | --- |
| `@intrinsic` runtime primitives and host functions | `emitter/intrinsics.ts`, `host-functions.ts`, `runtime/boundary.wat` | 215 | [Boundary Mechanisms](../src/README.md#boundary-mechanisms) | A |
| Capability bridge | `emitter/host-providers.ts`, `checker/host-capabilities.ts` | 407 | 11, Console | A |
| `block_on`, `all!`, `race!` | `checker/expression-suspensions.ts`, `emitter/suspension.ts` (parts) | in D | [`req.combinator.intrinsic`](../spec/11-requirements-and-suspension.md#r-req.combinator.intrinsic) | A |
| `retry!` | none: missing | 0 | [Task](../spec/std/task.md) | C (hook: a `lib/std/task.hd`) |

### Not Audited Line By Line

These are class D and were only sized: the lexer and parser (4,600),
`checker/` core (type checking, calls, patterns, exhaustiveness, variance,
inference, embedding, defaults: about 19,000), the emitter core and
suspension lowering (about 5,600), `hir.ts`, `types.ts`, `wasm.ts`,
`compiler.ts`, and the tooling. Within the checker core, a few rules name
std items by spelling, such as `Some`/`None`/`Ok`/`Err` in patterns and
`?`. Those are prelude enums the language tier owns.

## Classification

### (A) True Intrinsics, About 6,370 Lines

- **Named by a spec rule:** typed derivation's generated bodies
  (`annot.derive.supplied`), `shape`/`shape_of`, `Inspectable` and
  downcasts, `@error`, the four intrinsic derivations, `all!`/`race!`,
  operator bodies, `Termination`, literal markers.
- **Not expressible in hd:** the `@intrinsic` runtime primitives (byte
  length, byte read, byte slice, `char` from a scalar), host functions,
  the capability bridge, float text, and the `List` and `Map`
  representations.
- **Host tools:** `test-runner.ts`, `property-tests.ts`, and
  `snapshots.ts`. They run cases in fresh instances, catch panics, and
  touch files. hd has no panic catching, so shrinking cannot be hd code.
  The PRNG alone could move, which saves about 60 lines; not worth a
  chunk.

### (B) Movable Now, About 400 Lines

| Feature | hd to write | Notes |
| --- | --- | --- |
| `@derive(Debug)` | `impl[T] Debug for T by Structure` in `lib/std/format.hd`, a `Walker` that picks `debug_struct`, `debug_tuple`, or `write` per variant | `T::name()`, `v.info.name`, `h.info.name`, and `h.info.positional` exist today (`lib/std/testing.hd` uses the first three) |
| Primitive `Display` | `impl Display for i32` and the other integers, `char`, `bool` in `lib/std/format.hd`, a digit loop over `StringBuilder` | floats keep the host `format_f64` as an `@intrinsic` |
| `FromIterator` for `Map` | `impl[K < Eq & Hash, V] FromIterator[(K, V)] for Map[K, V]` in `lib/std/iter.hd` | a map literal with a bounded key already uses key kind 3 |
| `std.structure`, `std.inspect` sources | `lib/std/structure.hd`, `lib/std/inspect.hd` | the loader already joins modules by use; the hidden fields stay |

### (C) Movable After A Small Hook, About 1,500 Lines

| Feature | Hook | Hook size | TS deleted |
| --- | --- | --- | --- |
| `Eq`/`PartialOrd` for `List`, `T?`, `Result`, `Map` | **generic impl lookup**: `traitMethodDispatch` and `displayValue` match `impl[T] Eq for List[T]` by pattern (as `implementsDebug` already does), not by exact target | about 30 | about 400 |
| Builtin `Debug` dictionary | the same lookup | 0 more | 78 |
| Prelude traits in TS | **lang items by standard name**: look up `Eq`, `Ordering`, `Iterable`, `Console` by `standardName`, not by fixed trait index (`lowerRunTimeGaps` relies on `Console` being last) | about 40 | about 345 |
| `assert`, `assert_equal`, `snapshot` | **`panic_category` intrinsic**: a std-only `@intrinsic("panic_category")` that panics with a named category such as `assertion-failed`; keep the `missing-eq` remap and the literal `expect` check | about 35 | about 250 |
| `it_each`, `it_prop`, `it_prop_with`, `timeout` | **runner hooks as `@intrinsic`**: `case_index()`, `report_case_count(n)`, `report_timeout(ms)` replace three HIR nodes; the parser keeps only the registration position check and calls the std function as the case body | about 40 | about 285 |
| `std.testing.arbitrary` shim | **std submodules**: `lib/std/testing/arbitrary.hd` joins as `std.testing.arbitrary` | about 20 | 59 |
| Tuple `==`, `<`, `Hash` | **none if Q3 is B**: hd impls up to 12 elements and a rest tuple, as `Debug` has. Otherwise **tuple `Structure`**: tuples get compiler `Structure` so std templates derive them | 0, or about 120 | 82 |
| `retry!` | **`lib/std/task.hd`**: a std file beside the compiler-provided `block_on`, `all`, `race`, declared there as `@intrinsic` signatures | about 20 | 0; closes 4 known failures |

### (D) Language Semantics, About 38,120 Lines

Parsing, layout, name resolution, type checking and inference, patterns
and exhaustiveness, variance, embedding and delegation, requirement rows
and suspension lowering, codegen, Wasm validation, the package linker,
and the agent tooling. Not audited.

## Spec Mismatches

### Stdlib-Tier Features That Exist Only As TypeScript

1. **`@derive(Debug)`.** [`trait.debug.derive`](../spec/09-traits.md#r-trait.debug.derive)
   says it derives through its template, and [Format](../spec/std/format.md)
   lists it as a template. `lib/std/format.hd` has no template;
   `derive-intrinsics.ts` generates the builder calls in TS.
2. **Table and property tests.** `it_each` rows, `it_prop`,
   `it_prop_with`, and the `timeout` option are stdlib tier
   ([Testing](../spec/std/testing.md)). The prototype lowers them in the
   parser (`tableTest`, `propertyTest`) and with three HIR nodes.
3. **`FromIterator` for `Map`.** [Collect Targets](../spec/std/iter.md#collect-targets)
   is stdlib tier. The checker supplies the `Map` impl (`map-collection`).
4. **`retry!`.** [Task](../spec/std/task.md) has no prototype at all,
   because `std.task` is a compiler-provided module, not a `lib/std` file.

### Language Rules That Say "Standard Library" But The TS Hard-Codes

5. **Composite equality and order.** [`expr.eq.std`](../spec/05-expressions.md#r-expr.eq.std)
   and [`expr.ord.std`](../spec/05-expressions.md#r-expr.ord.std) say
   standard-library implementations compare lists, optionals, results, and
   maps. The emitter inlines them as strategies, so no `impl Eq for
   List[T]` exists for a bound to find through `lib/std`.
6. **Primitive `Display`.** [`expr.interp.std`](../spec/05-expressions.md#r-expr.interp.std)
   says the standard library provides it. The checker emits a `display`
   node and WAT digit loops.
7. **`Debug` for built-in types.** [`trait.debug.std-types`](../spec/09-traits.md#r-trait.debug.std-types)
   says `std` implements it, and `lib/std/format.hd` does. The checker
   still plans a builtin `debug` dictionary for primitives and composites
   whose `debug` "writes nothing" (`hir.ts`). This is reachable through a
   `T < Debug` bound on a built-in type when the exact-target lookup
   misses; not verified by a run.

### Behavior Gaps Found On The Way

8. **`assert_equal` shows nothing.** [`module.testing.assert-equal-debug`](../spec/10-modules.md#r-module.testing.assert-equal-debug)
   says a failure shows both values as `debug` renders them. The emitter
   panics with `assertion-failed` and no text, not even `reason`. An hd
   `assert_equal` fixes this for free.
9. **Map keys by a private rule.** `mapKeyKind` accepts a key by its own
   "MVP map-key contract", not the declared bound `Map[K < Eq & Hash, V]`
   ([`types.map-key.declared-bound`](../spec/04-type-system.md#r-types.map-key.declared-bound)).
   This is the SSC-Q7 group of known failures (7 rows). The runtime never
   calls `Hash`; it scans keys linearly, which no fixture can observe.

### Intrinsic Rules The TS Implements More Broadly

10. **Tuple arity.** [`trait.target.tuple.derived`](../spec/09-traits.md#r-trait.target.tuple.derived)
    makes tuple `Eq`, `PartialOrd`, `Ord`, and `Hash` an intrinsic over
    every arity. Tuple `Debug`, `Display`, and `Default` are std code up
    to 12 elements. The same family has two mechanisms; see Q3.
11. **`Ord` for built-ins.** The checker plans `Ord` for any type that
    `builtinTotallyOrdered` accepts, with no impl anywhere. The spec gives
    `Ord` to no built-in composite by rule; it follows only through
    `expr.ord.std` and the tuple derivation.

No rule that names an intrinsic is implemented more narrowly, except
`all!` and `race!`, which report `unsupported-task-combinator`
(ALL-INTRINSIC, Q7 known failures).

## Migration Plan

Each chunk is about one hour of agent work, with its own fixtures run
through `npm run test:portable`. The order is TS deleted per hour, best
first, with dependencies first. Every chunk keeps `lib/std` code on
Sonnet and logs hd-writing mistakes in
[audit/hd-writing-log.md](../audit/hd-writing-log.md), as AGENTS.md
requires.

| # | hd to write in `lib/std` | TS to delete | Hook | Fixtures that prove it |
| --- | --- | --- | --- | --- |
| M1 | `std.cmp`: `impl[T < Eq] Eq for List[T]`, `T?`, `Result[T, E]`, `Map[K, V]`; `PartialOrd` for `List[T]` and `T?`, lexicographic, `.None` first | about 400: composite strategies in `value-comparison.ts`, checker strategy branches, HIR strategy kinds | generic impl lookup in `traitMethodDispatch` and `displayValue` (30) | `assert-equal-list`, `-map`, `-optional`, `-result`, `composite-ordering`, `structural-ordering`, `nan-ordering-composites`, `display-tuples` (closes Q6-others) |
| M2 | `std.cmp`: `Eq`, `PartialOrd`, `Ord`, `Ordering`; `std.format`: `Display`, `Debug`; `std.iter`: `Iterable`; `std.console`: `Console`; `Any`, `Waker`, `ResourceError` | about 345: `program-types.ts` declarations, `standard-traits.ts` shims | lang items by standard name (40) | the whole portable suite; `bool-ordering`, `display-dispatch`, `partial-ordering-dispatch` |
| M3 | `std.testing`: `it_each`, `it_prop`, `it_prop_with` as hd functions over `case_index`, `report_case_count`, `report_timeout`, and the existing `prop_*` intrinsics | about 285: `tableTest`, `propertyTest`, timeout lowering, three HIR nodes | runner hooks as `@intrinsic` (40) | `it-each-rows`, `it-each-options`, `it-each-propagation`, `test-timeout-options`, `property-assume-discards`, `property-draw-budget`, `test/std/property.hd` |
| M4 | `std.testing`: `assert`, `assert_equal[T < Eq & Debug]` that panics with both `debug` texts and `reason`, `snapshot` over `assert_equal` | about 250: `assert`/`assert-equal` checker and emitter cases, HIR | `panic_category` intrinsic (35) | `assert`, the 19 `assert-equal-*` fixtures, `snapshot-mismatch`, `snapshot-file-missing`; needs M1 |
| M5 | `lib/std/structure.hd`, `lib/std/inspect.hd`, `lib/std/testing/arbitrary.hd` | about 175: `STRUCTURE_SOURCE`, `INSPECT_SOURCE`, `arbitrary-module.ts` | std submodules (20) | `typed-derivation*`, `derive-without-structure-use`, `arbitrary-with-*`, `typeid-*` |
| M6 | `std.format`: `Display` for the integers, `char`, `bool` | about 160: `display` node, `emitPrimitiveDisplay`, four WAT `*_to_string` | none | `string-interpolation-built-ins`, `interpolation-display-order`, `expression-interpolation` |
| M7 | `std.format`: `impl[T] Debug for T by Structure` | about 133: `deriveDebug`, the builtin `debug` plan, `debug.ts` | none; needs M1 | `derive-debug`, `derive-debug-shapes`, `debug-standard-types`, `debug-writer-builders` |
| M8 | `std.cmp`, `std.hash`: tuple `Eq`, `PartialOrd`, `Ord`, `Hash` up to 12 elements, and the rest tuple | 82: tuple strategies | none if Q3 is B | `tuple-derived-traits`, `tuple-derived-order`, `tuple-rest-derived`, `tuple-ordering-nan-unordered`, `assert-equal-tuple` (closes Q6, TR-traits) |
| M9 | `std.iter`: `FromIterator` for `Map` | 69 | none | `collect-targets`, `collect-targets-run`, `collect-target-from-try-hint` |
| M10 | `lib/std/task.hd`: `retry!` in hd, with `block_on`, `all`, `race` declared `@intrinsic` | 0 | `lib/std/task.hd` beside the compiler module (20) | `task-retry`, `task-retry-at-least-once` (closes RETRY) |
| M11 | after Q4: `std.cmp`, `std.hash` templates for `Eq`, `PartialOrd`, `Ord`, `Hash` | about 325 of `derive-intrinsics.ts`; law partners stay as a check | map a template member-bound error to `derive-field-missing-trait` (20) | `derived-equality`, `derived-ordering`, `derived-total-order-float`, the `derive-field-missing-trait` and `mixed-derived-law` fixtures |

Totals: M1 to M10 delete about 1,900 lines of TS and add about 1,000
lines of hd and 200 lines of hooks. M11 adds about 325 more after Q4.

Rules for every chunk:

- Write the hd first, then delete the TS path it replaces. Never keep both.
- Run the tier test before adding any TS. A stdlib-tier feature gets no
  new HIR node, checker case, or WAT helper.
- Expect slower runs: a dictionary call replaces an inlined comparison.
  The prototype is a toy, so speed is not a goal.

## Pending Prototype Tasks The hd Way

### #90: 246 Known Failures

Most rows are class D: parser and type-system catch-up. The hd way matters
for these groups:

| Group | Rows | The hd way |
| --- | --- | --- |
| RETRY | 4 | M10, not a checker case |
| Q6, TR-traits, Q6-others | 7 | M8 and M1: std impls, not new strategies |
| SSC-Q7 | 7 | delete `mapKeyKind`'s private rule; check `Map[K < Eq & Hash, V]` as an ordinary bound. This deletes TS |
| SSC-Q8 | 2 | `m[k]` typed `V`; then fix `Map`'s `Index` in `lib/std/ops.hd` |
| IT | 2 | delete `impl[T] Iterable[T] for Iterator[T]` from `lib/std/iter.hd`; no TS |
| AT-gen | 2 | fix the template's member bounds in `typed-derivation.ts`, not a new `Arbitrary` path |
| ALL-INTRINSIC, Q7 | 7 | stay intrinsic; see #97 |

The rest (BF 83, TUPLE-REST 22, SSC-Q4 20, LP 17, and smaller groups) are
grammar and typing. They have no library angle.

### #97: Packs, Varargs, `Tuple`, Rest And Spread, `all!`, Tuple Traits

- **Pack removal, varargs, rest elements, spread:** class D. Delete pack
  code; add no std names to the checker.
- **`std.function.Tuple`:** declare it in `lib/std` as a sealed marker
  trait. The compiler supplies its impls, as it does `Inspectable`; that
  part is A and small.
- **`all!`:** A by [`req.combinator.intrinsic`](../spec/11-requirements-and-suspension.md#r-req.combinator.intrinsic).
  Type it by its one rule. Declare its signature in `lib/std/task.hd`
  (M10) so the checker does not spell the name.
- **Tuple `Eq`/`Ord`/`Hash`:** the spec says compiler-derived. Do not
  extend the TS tuple strategies to `Hash` and rest tuples. Two hd ways:
  1. **Std impls up to 12 elements** plus the rest tuple, as `Debug` and
     `Display` have (M8). This needs Q3, because the spec says every
     arity.
  2. **A tuple `Structure`:** the compiler gives each tuple type the
     `Structure` that a positional data type gets, so the M11 templates
     derive tuples too. It keeps "every arity", costs about 120 lines in
     `typed-derivation.ts`, and depends on Q4.

  Recommended: option 1 now; option 2 only if the owner keeps every arity.

### #119: `::[`

Parser only, class D. It closes the 83 BF rows. The std names it touches,
such as `shape::[T]()`, `TypeId::of::[T]()`, and `find::[F]()`, need no
library change.

## Questions For The Owner

Smallest first. Each has a recommendation.

**Q1. Keep `@error` intrinsic?** A template cannot read a message string
and turn the members it names into code without a new hook. Recommendation:
keep it intrinsic (663 lines stay). No spec change.

**Q2. Keep `Map` storage in the compiler?** `runtime/map.wat` (233
lines) could become hd over a growable list, calling `Eq` and `Hash`
through bounds. It needs an array primitive and costs speed. The tier
table puts `Map.get` and friends in the language tier. Recommendation:
keep it; revisit with a real backend.

**Q3. Tuple traits up to 12 elements?** Tuple `Debug`, `Display`, and
`Default` are std impls up to 12 elements. Tuple `Eq`, `PartialOrd`,
`Ord`, and `Hash` are an intrinsic over every arity
([`trait.target.tuple.derived`](../spec/09-traits.md#r-trait.target.tuple.derived)).

- A: keep the intrinsic over every arity.
- B: make all four std impls up to 12 elements plus the rest tuple, as
  the others are. A 13-element tuple is not a map key.

**Recommendation: B.** It moves an intrinsic into the core library, the
cheapest kind of change in the [Design Cost Order](../AGENTS.md#design-cost-order).
It changes three rules and deletes the tuple strategies.

**Q4. `@derive(Eq, PartialOrd, Ord, Hash)` as std templates?**
[`trait.derive.intrinsic-set`](../spec/09-traits.md#r-trait.derive.intrinsic-set)
makes these four intrinsic. `Debug` and `Arbitrary` already derive through
templates over `Structure`, and the Why note under [`annot.derive.supplied`](../spec/14-annotations.md#r-annot.derive.supplied)
says "every format, comparison, or schema is library code".

- A: keep them intrinsic.
- B: write them as templates in `std.cmp` and `std.hash`. `Eq` walks
  `self` and reads the same member of `other` through `Field.get`;
  ordering compares variant indexes first. `derive-field-missing-trait`,
  `missing-derived-bound`, and `mixed-derived-law` stay as checks.

**Recommendation: B, after M7 shows the `Debug` template works.** It
deletes about 325 lines and leaves one derivation mechanism. Cost: the
derive rules move from "intrinsic" to the template rules, about 10 rules.
