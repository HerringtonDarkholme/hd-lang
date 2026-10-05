# Standard Library Plan

Status: research, 2026-10-02 (task #172). Nothing here is accepted
behavior except where a note links the specification. Spec pass 64
(2026-10-02) applied the owner's answers to this plan's six questions and
part of the [Earlier Owner Decisions](#earlier-owner-decisions), spec
pass 70 applied decisions 4 and 10, and spec pass 73 applied
[`std.encoding`](../spec/std/encoding.md) and
[`std.digest`](../spec/std/digest.md), and spec pass 74 applied the UTC
[`Date` and RFC 3339 text](../spec/std/time.md#dates),
[`Deque`](../spec/std/collections.md#deque), and
[`Heap`](../spec/std/collections.md#heap), and spec pass 76 applied
[`all_list!`](../spec/std/task.md#all-list) and
[`parse_f64`](../spec/std/num.md#float-parsing), and spec pass 78 applied
the [text helpers](../spec/std/text.md#splitting-and-padding),
[`to_fixed`](../spec/std/num.md#fixed-point-text), the
[`List` helpers](../spec/std/collections.md#list-helpers),
[`counts`](../spec/std/collections.md#counts),
[`Set`](../spec/std/collections.md#set), `Map.get_or`, and the
[`Iterator` helpers](../spec/std/iter.md#more-adapters), which the owner
approved, and spec pass 85 applied the [`Rng` helpers](../spec/std/random.md#rng)
and [`std.cli`](../spec/std/cli.md), and spec passes 86 and 87 applied
[`std.regex`](../spec/std/regex.md), as stdlib calls; every other module
sketch below is still a proposal for the owner. The spec reconcile pass
(task #257) keeps decisions that wait for a design in
[Decided, Not Yet Applied](#decided-not-yet-applied), and those that wait
only for the prototype in [Compiler Handoff](#compiler-handoff).

Under review: the stdlib tier ([spec/std/](../spec/std/README.md)), the
library itself ([lib/std/](../lib/std/)), the host rules of
[Modules](../spec/lang/10-modules.md) ([Console](../spec/lang/10-modules.md#console),
[Processes](../spec/lang/10-modules.md#processes),
[Runtime Profiles](../spec/lang/10-modules.md#runtime-profiles)), the
[Command Line](../spec/cli/command-line.md) tier
([Single Files](../spec/cli/command-line.md#single-files),
[Host Capabilities](../spec/cli/command-line.md#host-capabilities)), and the
[standard combinators](../spec/lang/11-requirements-and-suspension.md#standard-combinators).

Owner direction: "If you need multiple files, deps, create a package...
let's provide rich, useful, ergonomic stdlib." A single-file program
(`hd FILE`) and a task (`tasks/NAME.hd`) may use only `std`, so `std` must
cover what scripts do. The owner also asked for a review of the
[Effect v4 API](https://effect.website/docs/v4/api/effect), noting that not
every feature belongs in `std`.

## Contents

1. [Findings In Brief](#findings-in-brief)
2. [What Exists Today](#what-exists-today)
3. [Earlier Owner Decisions](#earlier-owner-decisions)
4. [Decided, Not Yet Applied](#decided-not-yet-applied)
5. [Audit Gaps Left](#audit-gaps-left)
6. [Compiler Handoff](#compiler-handoff)
7. [Survey Matrices](#survey-matrices)
8. [Gap Survey By Area](#gap-survey-by-area)
9. [A Script With The Proposed Surface](#a-script-with-the-proposed-surface)
10. [The Effect Review](#the-effect-review)
11. [Ranked Rollout](#ranked-rollout)
12. [Sources](#sources)
13. [Parse Log](#parse-log)

## Findings In Brief

- **A script cannot do real work today.** `std` has no way to read
  program arguments, the environment, files, the clock, or randomness,
  and no way to write to standard error. `Process` exists but no runtime
  profile binds it. The trait that reads arguments waits on the host
  catalog ([`cli.args.pass` Note](../spec/cli/command-line.md#program-arguments)).
- **Decided std design was lost.** The STDLIB design record held owner
  decisions of 2026-09-26 and 2026-09-29 that were "applied in this record
  only". The record was archived and then deleted, so those decisions now
  live only in git history. See [Earlier Owner Decisions](#earlier-owner-decisions).
- **Typed JSON is specified.** `ToJson` and `FromJson` derive through
  `Walker` and `Source`, as `Hash` and `Arbitrary` do:
  [Typed JSON](../spec/std/json.md#typed-json).
- **Most gaps are pure library work.** Collections, text, JSON, and
  argument parsing need no host. Host areas need one decision, the first host catalog, plus small
  prototype hooks.
- **Effect maps well onto hd.** Requirement rows already are Effect's
  `Context` and `Layer`, `fn!` is the suspended effect, and `all!`,
  `race!`, and `retry!` exist. Worth adding: `Clock` with `sleep!`,
  `timeout!`, a small backoff policy, and `Duration` helpers. Exclude
  fibers, STM, streams, and `Scope`.

## What Exists Today

| Module | In `lib/std` | Stdlib-tier spec | Notes |
| --- | --- | --- | --- |
| `std.text` | `string` methods (`split`, `trim`, `replace`, `find`, `lines`, `repeat`, `split_once`, `split_whitespace`, `pad_start`, `pad_end`, `count`, ...), `join`, `StringBuilder`, `r` prefix, UTF-8 conversion with `Utf8Error`; `char` classification and `to_digit` | [text.md](../spec/std/text.md) | `pad_start` and `pad_end` lack the `fill` default: the prototype parser rejects a method parameter default (`METHOD-DEFAULT`) |
| `std.collections` | `List`: `map`, `filter`, `first`, `last`, `reversed`, `sorted_by`, `chunks`, `zip`, `view`, `sorted_by_key`, `group_by`, `partition`, `any`, `all`, `find`, `flat_map`, `windows`, `contains`, `index_of`, `sorted`, `min`, `max`; `counts`; `Map`: `contains_key`, `keys`, `values`, `get_or`, `is_empty`; `Set`, `Deque`, `Heap` | [collections.md](../spec/std/collections.md) | none |
| `std.iter` | `Iterator` with `filter`, `take`, `enumerate`, `map`, `fold`, `collect`, `skip`, `take_while`, `zip`, `chain`, `flat_map`, `any`, `all`, `find`, `count`; `FromIterator` | [iter.md](../spec/std/iter.md) | none |
| `std.option`, `std.result` | `map`, `and_then`, `unwrap_or`, `ok_or`, `expect`, `map_err`, `ok`, `err`, `is_*` | [option.md](../spec/std/option.md), [result.md](../spec/std/result.md) | `map` on `T?` is in [iter.md](../spec/std/iter.md#list-and-optional-map) |
| `std.num` | numeric traits; checked, wrapping, and saturating ops, `abs_diff`, `count_ones`, and `leading_zeros` on every integer type; `is_nan`, `is_finite`; `parse_i32`, `parse_i64`, `parse_f64`; `to_fixed` | [num.md](../spec/std/num.md); the traits are language tier | none |
| `std.cmp`, `std.hash`, `std.format`, `std.ops` | comparison, hashing, `Display`, `Debug`, operators, `Default` | [cmp.md](../spec/std/cmp.md), [hash.md](../spec/std/hash.md), [format.md](../spec/std/format.md), [ops.md](../spec/std/ops.md) | none |
| `std.time` | `Duration` (milliseconds) with `Add`, `Sub`, and `Display`, suffixes `ms`, `s`, `min`, `h`; `Clock`, `now`, `sleep!`, `Timestamp` with `+ Duration` and `unix_milliseconds`, `Instant`, `ManualClock`; the UTC `Date`, `to_rfc3339`, `parse_rfc3339`, `TimeParseError` | [time.md](../spec/std/time.md) | no time zones or local time |
| `std.task` | `race!`, `retry!`, `all_list!` in hd; `all!`, `block_on` intrinsic | [task.md](../spec/std/task.md) | no `timeout!`; no `Backoff` or `retry_with!`, held out (`RETRY-WITH`) |
| `std.console` | `Console` with `write_error_line!`, `println`, `eprintln`, `ConsoleInput`, `read_line!`, `BufferConsole`, `ScriptedInput` | [console.md](../spec/std/console.md); `Console` and `println` are language tier | the prototype's host `Console` answers only `write_line!`, so `eprintln` under the default profile fails (`STD-HELPERS`); no profile binds `ConsoleInput` |
| `std.process` | `ExitCode`, `Termination`, `Process.run!` with `ProcessError`, `ScriptedProcess::new`, and `Eq` and `Debug` for the exit data | [process.md](../spec/std/process.md); the trait is language tier | no profile binds `Process`; no working directory or environment |
| `std.resource` | `ResourceError[E]` | none | no handle type uses it yet |
| `std.error` | `Error` with `root_cause` and `find`, `chain`, `ErrorReport`, `report_of`, `Result.context`, `ContextError` | [error.md](../spec/std/error.md); `Error` is language tier | none |
| `std.testing`, `std.structure`, `std.inspect`, `std.annotation`, `std.function` | test, derivation, and type-identity support | [testing.md](../spec/std/testing.md) | complete for their purpose |
| `std.random` | `Random`, `SeededRandom`; `Rng` with `int`, `float`, `bool`, `choose`, `shuffle`, and `sample`; `rng` | [random.md](../spec/std/random.md) | none |
| `std.cli` | `Cli` with `flag`, `option`, `positional`, `parse`, `parse_args`, and `usage`; `Parsed`, `CliError` | [cli.md](../spec/std/cli.md) | no typed `FromArgs` derivation |
| `std.host`, `std.fs`, `std.path` | `Args`, `Env`, `MapArgs`, `MapEnv`; `FsRead`, `FsWrite`, `FsError`, `MemoryFs`, `read_text!`, `write_text!`; `Path` | [host.md](../spec/std/host.md), [fs.md](../spec/std/fs.md), [path.md](../spec/std/path.md) | no profile binds `FsRead` or `FsWrite` |
| `std.encoding`, `std.digest` | `hex_encode`, `hex_decode`, `base64_encode`, `base64_decode`, `DecodeError`; `sha256`, `sha256_hex` | [encoding.md](../spec/std/encoding.md), [digest.md](../spec/std/digest.md) | no URL-safe base64, no streaming hasher |
| `std.json` | `Json`, `Number`, `parse`, `JsonError`, `Display`, the `pretty` method, and the `Json` accessors; `ToJson` and `FromJson` with their templates and standard implementations, `encode`, and `decode` | [json.md](../spec/std/json.md) | `parse` reads a number with a fraction or an exponent through `parse_f64`; no field renames, conditional skips, or defaults per field |
| `std.regex` | `Regex` with `new`, `as_str`, `is_match`, `find`, `find_all`, `captures`, `captures_all`, `replace`, `replace_all`, and `split`; `Match`; `Captures`; `RegexError`, `RegexErrorKind` | [regex.md](../spec/std/regex.md) | no flags |
| absent | log, http | none | the gap this plan covers |

The prototype also lacks the inferred row of a script's top level
(`MHP-1` in [src/KNOWN_ISSUES.md](../src/KNOWN_ISSUES.md)).

## Earlier Owner Decisions

The deleted record `future-work/STDLIB.md` (last version at commit
`ed7fbfc5^`) lists these owner decisions. They are not in the
specification or in [Open Issues](OPEN_ISSUES.md). This plan builds on
them and reopens none.

| # | Decided | What it fixes for this plan |
| --- | --- | --- |
| 2 | 2026-09-26 | I/O suspends (`fs`, network are `!` calls); clock, random, and environment reads are plain calls |
| 3 | 2026-09-26 | the file system is two traits, `FsRead` and `FsWrite`; console input is its own `ConsoleInput` |
| 4 | 2026-09-26 | each host trait's deterministic provider lives beside it, as `std.time.ManualClock` |
| 5 | 2026-09-26 | a library `std.bytes.Bytes`, convertible to and from `List[u8]` |
| 6 | 2026-09-26 | one error enum per domain, as `FsError`, `HttpError` |
| 7 | 2026-09-26 | the prelude does not grow; new names are imported |
| 9 | 2026-09-26 | `decimal` is the only number type past the primitives; `BigInt` is a package |
| 10 | 2026-09-26 | virtual time auto-advances: `sleep!` on a manual clock returns at once |
| 11 | 2026-09-26 | tasks are structured scopes only: `scope!`, `start`, `join!`; no detached spawn |
| 12 | 2026-09-26 | `Secret[T]` is removed for now |
| 13 | 2026-09-26 | ship an untyped `std.json.Json` with one `Number` type modeled on `serde_json::Number` |
| Q15 | 2026-09-29 | `and_then` on `T?` and `Result` |
| Q16 | 2026-09-29 | `Map` gets `contains_key`, `keys`, and `values`, in insertion order |
| Q18 | 2026-09-29 | out-of-range counts panic, as `repeat(-1)` and `chunks(0)` |
| Q21 | 2026-09-29 | std value types implement `Eq`; `Duration`, `Timestamp`, and `Instant` implement `Ord`; error enums implement `Display` |

The same record drafted module names (`std.host`, `std.fs`, `std.path`,
`std.random`, `std.json`, `std.http`) and a `RetryPolicy`. Those drafts were
not decided; this plan reuses the names to avoid churn.

Spec pass 64 applied the ones that are pure spec text and fit today's
specification:

| # | Applied in |
| --- | --- |
| 2 | I/O methods are bang methods, reads are plain calls: [Clock](../spec/std/time.md#clock), [Host](../spec/std/host.md#plain-reads), [Random](../spec/std/random.md#random-source), [Fs](../spec/std/fs.md#suspension), [Console Input](../spec/std/console.md#console-input) |
| 3 | `FsRead`, `FsWrite`, and `ConsoleInput`: [Fs](../spec/std/fs.md), [Console Input](../spec/std/console.md#console-input) |
| 6 | one `FsError` for every file system method: [File System Errors](../spec/std/fs.md#file-system-errors) |
| 7 | every new name is imported, not a prelude name |
| Q16 | `contains_key`, `keys`, and `values`, as insertion-order snapshots: [Map Methods](../spec/std/collections.md#map-methods) |
| Q18 | `chunks(0)` and `clamp` with `low > high` panic, and `repeat(-1)` did until counts became `usize` (SIZES-UNSIGNED) and made it a compile error: [Text](../spec/std/text.md#string-methods), [Collections](../spec/std/collections.md#list-methods), [Clamp](../spec/std/cmp.md#clamp) |
| Q21 | `Eq` on the new value types; `Ord` on `Duration`, `Timestamp`, and `Instant`; `Display` on `FsError`: [Time](../spec/std/time.md), [Fs](../spec/std/fs.md#file-system-errors) |
| 2026-09-29 | `lines()` follows Rust: [Text](../spec/std/text.md#string-methods) |

Spec pass 70 applied two more:

| # | Applied in |
| --- | --- |
| 4 | the deterministic providers, each beside its trait: [Map Providers](../spec/std/host.md#map-providers), [Manual Clock](../spec/std/time.md#manual-clock), [Memory File System](../spec/std/fs.md#memory-file-system), [Seeded Random](../spec/std/random.md#seeded-random); a test names the providers it needs in `$.with(...)` |
| 10 | `sleep!` on a `ManualClock` returns at once and advances its time: [Manual Clock](../spec/std/time.md#manual-clock) |

Spec pass 75 applied the untyped part of one more:

| # | Applied in |
| --- | --- |
| 13 | an untyped `Json` with one `Number` modeled on `serde_json::Number`: [Json](../spec/std/json.md) |

The spec reconcile pass (task #257) found one more complete:

| # | Applied in |
| --- | --- |
| 12 | nothing to write: no chapter names `Secret[T]`, and [Open Issues](OPEN_ISSUES.md) keeps the removed draft |

The rest are listed in [Decided, Not Yet Applied](#decided-not-yet-applied),
or wait for the prototype in [Compiler Handoff](#compiler-handoff).

## Decided, Not Yet Applied

These owner decisions still stand, and each waits for a design that is
not decided yet. A decision whose spec side is complete, and that waits
only for `src/` or `lib/std`, is in [Compiler Handoff](#compiler-handoff).

| Decision | Decided | What it says | Waits for |
| --- | --- | --- | --- |
| 5 | 2026-09-26 | a library `std.bytes.Bytes`, readonly and compact, convertible to and from `List[u8]` | waits for design: a use that `List[u8]` serves badly |
| 9 | 2026-09-26 | `decimal` is the only number type past the primitives; `BigInt` is a package | waits for design: a `decimal` design |
| 11 | 2026-09-26 | tasks are structured scopes only: `scope!`, `start`, `join!`; no detached spawn | waits for design: a new polling intrinsic, a language-tier item |
| JSON-FIELD-FACTS | 2026-10-03 | `ToJson` and `FromJson` get renames, conditional skips, and defaults per field through typed member facts, not in the first version ([Typed JSON](../spec/std/json.md#typed-json)) | waits for design: a `std.json` fact design: fact types, such as a rename, that the two templates read through `h.fact::[D]()`; an omit line `f = pass` already leaves a member out of one derivation |

## Audit Gaps Left

The stdlib audit (task #239) was applied in passes 89 and 90. These of its
recommendations are not applied yet; each is a stdlib call to make in a
later pass.

| Gap | Recommendation |
| --- | --- |
| `Console.write_error_line!` | add it to `lib/std/console.hd` with its spec default; `BufferConsole` records both streams (audit inconsistency 23) |
| sum of a list | `Iterator.sum()` for `T < Num`, as Rust |
| float math | `sqrt`, `floor`, `round`, `abs`, `powi` on `f64` in `std.num`; most need a host hook, so the primitives need owner approval |
| `Path` operations | `join`, `parent`, `file_name`, `extension`, as Go's `path/filepath` |
| unsigned parsing | `parse_u32`, `parse_u64`, `parse_usize` beside `parse_i32` |
| `print` without a newline | add when a prompt needs it; it needs a `Console` method (language tier) |
| integer rotation | `rotate_left`, `rotate_right` in `std.num` |
| hint diagnostics | `unsatisfied-trait-bound` on `Display` for an optional suggests `debug(...)`; `unknown-method` `unwrap` suggests `expect`; `Map::new()` suggests `{}` |

## Compiler Handoff

The spec, the fixtures, and the indexes have every decision below; the
prototype does not follow it yet. Each row keeps the work left in `src/`,
and in `lib/std` where the row says so, with the tag of its rows in
[`test/portable/KNOWN_FAILURES.tsv`](../test/portable/KNOWN_FAILURES.tsv).
When a row's cases pass, move them back to `test/portable/cases.tsv` and
delete the row.

| Decision | Decided | What the spec says | Work left | Known failures |
| --- | --- | --- | --- | --- |
| SNAPSHOT-ROW, RUNNER-SURFACE | 2026-10-02 | [Runner Capabilities](../spec/std/testing.md#runner-capabilities) | `lib/std/testing.hd` uses the new surface, so the compiler chat may now delete the old shims: the `resultType === "i32"` answer and the `seed`, `size`, `draw`, and `discard` methods in `src/property-tests.ts`, and the `snapshot_file_check` host function in `src/snapshots.ts` (with its uses in `src/compiler.ts` and `src/README.md`) | none |
| STD-DEBUG | 2026-10-03 | `std.inspect`'s `TypeId` and `std.structure`'s `SelfRef` implement `Debug` ([`std-format.debug.std-types`](../spec/std/format.md#r-std-format.debug.std-types)) | The typed-derivation pass declares both modules before `std.format`, so `inspect.hd` and `structure.hd` cannot name `DebugWriter`, and an impl in `format.hd` names `SelfRef` in programs that join no `std.structure` (`unknown-type`), even with reachable-only emission | `STD-DEBUG` 1 |
| RETRY-WITH | 2026-10-03 | the spec has `Backoff` and `retry_with!` ([Retry With Backoff](../spec/std/task.md#retry-with-backoff)), but `lib/std/task.hd` leaves them out | Held by the owner (batch 73): implementing them makes `std.task` import `std.time`, which costs every program a `std.time` check. No work until the owner lifts the hold | `RETRY-WITH` 1 |
| TEST-REG-ID | 2026-10-03 | a test registration call is known by declaration identity: renamed imports and `testing.it_each(...)` count ([`module.testing.reg.identity`](../spec/lang/10-modules.md#r-module.testing.reg.identity)) | The tests-block checker matches the bare spelling only. Recognize a registration call by the declaration it resolves to, under a renamed import and as `testing.it_each(...)` | `TEST-REG-ID` 4 |
| LITERAL-FIRST-USE | 2026-10-03, type variables in pass 92 | an unsuffixed literal with no expected type has an [open variable](../spec/lang/04-type-system.md#open-literal-width), `{integer}` or `{float}`, that flows unchanged through ranges, lists, maps, generic enums and data, tuples, generic calls, and closures; patterns (`let`, tuples, `for`, payloads, data) bind names to it. Uses in one body unify it, independent of order; a conflict is blamed at the later use of the first conflicting pair in source order, naming the earlier one. An integer variable never unifies with a float type. Width-sensitive checks are obligations discharged once at the end of the body, after the fallback (`usize` or `i32` by SIGN-FALLBACK, `f64` for floats): a choice among several instantiations (`price.add(n)`, `k * price`), bounds, methods of some widths (a dependent result variable), and literal ranges; only one fitting candidate fixes a variable mid-body (ONE-FIT). No cross-body flow: a function that reads an undecided top-level binding is `cannot-infer-type`; each REPL input is its own body | In `src/checker`, follow the guidance below. Then move the `LITERAL-FIRST-USE` rows back to `test/portable/cases.tsv`. Update the comment at `src/checker/expression-operators.ts:291`, which cites the retired `expr.op.left-literal`: it is now `expr.op.left-literal.join`. After that, the `let i: usize = 0` annotations in `lib/std`, the guide, and `examples/` may be dropped | `LITERAL-FIRST-USE` 23 |
| SIGN-FALLBACK | 2026-10-03, batch 83, pass 93 | an integer variable that no use fixes falls back to `usize`, or to `i32` when its [literal group](../spec/lang/04-type-system.md#r-types.literal.local.group) holds a [signed literal](../spec/lang/04-type-system.md#r-types.literal.local.signed) such as `-1` or `+5` ([`types.literal.local.default`](../spec/lang/04-type-system.md#r-types.literal.local.default)); floats stay `f64`. `t >= 0`, `0 <= t`, `t < 0`, and `0 > t` on an unsigned `t` are the warning `unsigned-comparison-always` ([Unsigned Comparisons With Zero](../spec/lang/05-expressions.md#unsigned-comparisons-with-zero)). An `integer-overflow` report on a fallback `usize` says so and suggests a sign or an annotation ([`flow.panic.report.fallback`](../spec/lang/06-control-flow.md#r-flow.panic.report.fallback)) | In `src/checker`, on top of LITERAL-FIRST-USE: keep one signed-seen bit per union-find root, set by a signed literal and ORed on union; the end-of-body fallback picks `usize` or `i32` from that bit; record which variables took the `usize` fallback, with a binding name, and carry that provenance to the overflow trap of each operation of that type, so the panic message can name it. Then move the `SIGN-FALLBACK` rows back to `test/portable/cases.tsv`. Update the REPL expectations in `website/e2e.ts` and `website/playground/e2e.ts` (`x := 21` then `x * 2` shows `42 : usize`). Check `lib/std` and `examples/` for unannotated literals that go negative or meet `< 0` | `SIGN-FALLBACK` 10 |
| ONE-FIT | 2026-10-03, task #254 | exactly one fitting candidate decides an open variable at once ([`types.literal.local.join`](../spec/lang/04-type-system.md#r-types.literal.local.join)): one type of the kind that satisfies a generic call's bounds, one that provides a method or a left operand's operator, or one fitting instantiation of a generic trait. Two or more decide nothing, and a check that fails after the fallback suggests `+5` or an annotation ([`types.literal.local.hint`](../spec/lang/04-type-system.md#r-types.literal.local.hint)). A literal joined with a dependent variable takes the resolved method's result type ([`types.literal.local.statement`](../spec/lang/04-type-system.md#r-types.literal.local.statement)); no use of a method result decides the receiver ([`types.literal.local.statement`](../spec/lang/04-type-system.md#r-types.literal.local.statement)); a literal erased to `Any` or `Inspectable` takes the fallback ([`types.literal.local.erased`](../spec/lang/04-type-system.md#r-types.literal.local.erased)). Task #262: a conversion to a trait value type is never a one-fit site ([`types.literal.local.erased`](../spec/lang/04-type-system.md#r-types.literal.local.erased)), and a receiver's width fits by the receiver alone; widths whose methods declare different parameter lists are `ambiguous-method` unless a use fixes the receiver ([`types.literal.local.form.receiver`](../spec/lang/04-type-system.md#r-types.literal.local.form.receiver)) | On top of LITERAL-FIRST-USE and SIGN-FALLBACK: when an obligation's candidates leave exactly one fit, unify the variable with it at once and wake the obligations that wait on it; count receiver candidates from the receiver alone, never from its arguments, the expected type, or later uses of the result; report `ambiguous-method` when the fitting widths' methods declare different parameter lists and no use fixes the receiver; check a conversion to a trait value type at the fixed width; keep a variable joined with a dependent result out of the fallback until the method resolves; record which arguments took the fallback and add the `+5` or annotation hint to the error | `ONE-FIT` 8 |
| LITERAL-SIDE | 2026-10-03 | an unsuffixed integer literal takes the other operand's type on either side ([`types.num.binary.literal-left`](../spec/lang/04-type-system.md#r-types.num.binary.literal-left)) | The fixtures pass. Update the comments at `src/checker/expression-operators.ts:714` and `src/checker/shared.ts:1428`, which cite the retired `types.num.binary.literal` (now `types.num.binary.literal-join`) | none |
| STD-HELPERS | 2026-10-03 | the spec has the helpers `eprintln`, `read_line!` ([Console](../spec/std/console.md)), `now`, `sleep!` ([Clock Helpers](../spec/std/time.md#clock-helpers)), `read_text!`, and `write_text!` ([File Helpers](../spec/std/fs.md#file-helpers)), and `Console.write_error_line!` ([Console](../spec/lang/10-modules.md#console)) | `lib/std` has every helper. In `src/host-functions.ts`: a host `Console.write_error_line` entry that writes to standard error, as the [default profile](../spec/cli/command-line.md#host-capabilities) says. Today `eprintln` under the default profile fails with `host-contract: host provider Console.write_error_line broke its contract: returned no boundary result` | none: no fixture runs `eprintln` under the default profile; `fs-helpers.hd` now fails only on `VOID-UNIT` |
| DOC-TESTS | 2026-10-04 | each fenced `hd` block in a `##` comment of a module under `src/` is a doc test: its own program with the public view and the integration row, holding one test case named `doc <module>.<item>[i]` ([Doc Tests](../spec/lang/10-modules.md#doc-tests), [Test Runs](../spec/cli/command-line.md#test-runs)) | (1) Extract the `hd` fences from the `##` text the lexer joins, per documented item, private items included; skip other fences. (2) Build one program per block with the integration view: its leading `use` lines, then the rest as the body of one test case; `self` and `super` are `unknown-module`, dev dependencies are allowed, and the row comes from the profile. (3) Map each position in that program back to its `##` line in the `.hd` file, for diagnostics and failures. (4) Compile-fail checking: a block with `# error: CODE` never runs and passes only when its compile reports CODE; `hd check --tests` skips it. (5) The name `doc <module>.<item>[i]`, with `pkg` as the `<module>` of `src/lib.hd` and `Type.member` as a member's `<item>`, `--filter`, `hd test FILE`, and the JSON `name`, `file`, and `line` ([`cli.test.doc.name.root`](../spec/cli/command-line.md#r-cli.test.doc.name.root)). (6) `hd test --update` rewrites a failing doc test `snapshot`'s expected text in place inside the block's `##` lines ([`cli.test.doc.update`](../spec/cli/command-line.md#r-cli.test.doc.update), owner, 2026-10-04) | `DOC-TESTS` 1 |
| AMBIGUOUS-TYPE | 2026-10-04 | inference with several valid solutions is the new code `ambiguous-type`; `cannot-infer-type` stays for no solution ([`types.infer.ambiguous.code`](../spec/lang/04-type-system.md#r-types.infer.ambiguous.code)) | Report `ambiguous-type` where requirement-key matching finds two binder mappings, as `Job { callback: read_both }` for `$ Repo[User] + Repo[Post]` against `Repo[A] + Repo[B]`; it reports `cannot-infer-type` today. The TS test `unordered requirement keys do not guess an ambiguous binder mapping` asserts the old code | `AMBIGUOUS-TYPE` 1 |
| SHADOW-TPARAM | 2026-10-04 | no declaration within a type parameter's scope may reuse its name: a method's own type parameter, a local declaration, a local value, or a parameter ([`names.type-param.no-redeclare`](../spec/lang/03-names-and-scopes.md#r-names.type-param.no-redeclare)) | Report `duplicate-binding` for `fn echo[T]` inside `impl[T]`, and for a local `data T` or value `T` inside `fn work[T]`, instead of renaming the method binder (`src/checker/generic-method-scope.ts`). The variance TS tests that use a shadowing binder assert acceptance | `SHADOW-TPARAM` 3 |
| VARIANCE-MUT-SELF | 2026-10-04 | a `mut self` inherent method counts toward declared variance ([`types.variance.surface.mut-self`](../spec/lang/04-type-system.md#r-types.variance.surface.mut-self)) | Check `mut self` methods in the variance pass; `pub fn set(mut self, value: U)` on `Box[+T]` is accepted today. The TS test `associated construction and mutable receiver signatures are not readonly instance views` asserts acceptance | `VARIANCE-MUT-SELF` 1 |
| ALIAS-MISSING | 2026-10-04 | an alias whose right side names nothing is an error at the alias, used or not: `unknown-type`, or `unknown-trait` for a key after `$` ([`types.alias.target-unknown`](../spec/lang/04-type-system.md#r-types.alias.target-unknown)) | Resolve every alias's right side at its declaration; today an unused one is never checked, and a used one reports `unknown-trait` at the use. The TS test `aliases are expanded before requirement keys are validated` asserts the error at the use | `ALIAS-MISSING` 2 |
| DERIVE-MISSING | 2026-10-04 | a `@derive` entry that names nothing is `unknown-trait`; `underivable-trait` stays for a real trait with no template ([`annot.derive.unknown`](../spec/lang/14-annotations.md#r-annot.derive.unknown)) | Resolve each `@derive` name before the template lookup; `@derive(Sortable)` reports `underivable-trait` today. The TS test `typed derivation reports its diagnostics at the opt-in` asserts `underivable-trait` for `@derive(Missing)` | `DERIVE-MISSING` 1 |
| HD-DOC | 2026-10-04, task #263 | `hd doc` writes HTML and Markdown pages side by side, with `llms.txt` and `llms-full.txt`, and `hd doc NAME` prints one item ([Documentation](../spec/cli/command-line.md#documentation)); `hd new --pages` writes a GitHub Pages workflow ([`cli.new.pages`](../spec/cli/command-line.md#r-cli.new.pages)); `broken-doc-link` is a warning | Add the `hd doc` command with `--private`, `--out`, `--open`, and `NAME` lookup; build a doc model from the checked package (signatures as written, the `pub` filter, impls per type with derived ones marked, `pub use` links); render Markdown, and HTML from it (`markdown-it` is only a dev dependency today); resolve `` [`Name`] `` links and report `broken-doc-link`; write `llms.txt` and `llms-full.txt`. `hd new --pages` and its prompt are done. The `setup-hd` action and release tarballs the workflow installs are infrastructure, not compiler work. | `CLI-DOC` |
| MODULE-DOC | 2026-10-04, task #263 | the first `##` block of a file, with a blank line after it, documents the module ([`lex.doc.module`](../spec/lang/01-lexical-structure.md#r-lex.doc.module)); its doc tests are named `doc <module>[i]` ([`cli.test.doc.name.module`](../spec/cli/command-line.md#r-cli.test.doc.name.module)) | Attach that block to the module in the lexer instead of reporting `doc-comment-without-target`, expose its text to `hd doc`, and give its doc tests the module-level name. | `MODULE-DOC` |
| BOUND-AMBIGUOUS | 2026-10-04, task #288 | a bound-only parameter that several instantiations fit, and that has no default, reports `ambiguous-type`; bounds that allow different single instantiations stay `cannot-infer-type` ([`types.generic.infer.bound.no-default.ambiguous`](../spec/lang/04-type-system.md#r-types.generic.infer.bound.no-default.ambiguous)) | Where call inference leaves a bound-only parameter unsolved, report `ambiguous-type` when any bound allows several instantiations, else `cannot-infer-type`; both fixtures report `cannot-infer-type` today | `BOUND-AMBIGUOUS` 2 |
| VARIANCE-ASSOC-FN | 2026-10-04, task #288 | an inherent associated function with no `self` receiver does not count toward declared variance, so `pub fn new(value: U) -> Box[U]` on `Box[+T]` is accepted ([`types.variance.surface.no-receiver`](../spec/lang/04-type-system.md#r-types.variance.surface.no-receiver)) | none: the prototype's variance pass already skips receiverless functions; keep that when VARIANCE-MUT-SELF adds the `mut self` check | none |
| HD-DOC-2 | 2026-10-04, task #288 | the root module's pages are `pkg.md` and `pkg.html`, and `index.md` and `index.html` are an entry page ([`cli.doc.files`](../spec/cli/command-line.md#r-cli.doc.files)); `hd doc std.MODULE.ITEM` prints a std item with a link to its reference ([`cli.doc.name.std`](../spec/cli/command-line.md#r-cli.doc.name.std)); `src/main.hd` has a page only with `--private` ([`cli.doc.main`](../spec/cli/command-line.md#r-cli.doc.main)); a top-level module `index` has its pages under `index/` ([`cli.doc.index-module`](../spec/cli/command-line.md#r-cli.doc.index-module)) | Fold into the HD-DOC command: name the root pages `pkg`, write the entry page, resolve `std.` names against `spec/std`, document `src/main.hd` as module `main` under `--private`, and put a module `index` under `index/` | `CLI-DOC` 8 |

LITERAL-FIRST-USE implementation guidance for the compiler session (not
spec text):

1. Check bidirectionally first. A literal with an expected type gets its concrete type on the spot, and a binary operator checks its non-literal operand first, on either side. Only a literal with no expected type gets a variable.
2. Keep variables as integer IDs in flat per-body arrays: a union-find parent with path halving and rank, a binding (a width or none), and the first deciding span for blame. Free the arena after the body.
3. Unify in O(α). Detect a conflict at union time, with the stored blame span.
4. Sweep only what is open: a has-vars bit on interned types, and a per-body list of nodes whose types hold variables. The end-of-body sweep walks only that list.
5. Keep obligations in an append-only list of (node, kind). After the fallback, process each once and patch the result into a side table, with no argument re-check.
6. For speculation, push union-find bindings on a trail (an undo log), and roll back by popping it. Never copy checker state (audit O-06).
7. Bodies are independent: check the top-level body first, then function bodies in any order, in parallel or lazily. An edit re-checks only its body.
8. Queue a generic instantiation that meets an open variable until after the sweep, then deduplicate it through the instantiation cache by concrete types. Codegen sees only concrete types.
9. The cost is O(n·α) per body, and nothing extra for a literal with an expected type.

## Survey Matrices

Each cell says where the area lives: **S** in the standard library, **P**
partly in it, or **X** in a common external package, with the name. Twenty
libraries are grouped into three tables so that each stays readable. Each
column's sources are in [Sources](#sources).

Legend: S = std; P = std, partly; X = external package; "boot" = a
package that ships with the compiler but is not the base library.

### Systems And Enterprise Languages

| Area | Go | Rust | Zig | Nim | Java | C#/.NET |
| --- | --- | --- | --- | --- | --- | --- |
| files | S `os.ReadFile` | S `std::fs` | S `std.fs` | S `readFile` | S `Files` | S `File` |
| path | S `path/filepath` | S `std::path` | S `std.fs.path` | S `std/paths` | S `Path` | S `Path` |
| env, args | S `os.Getenv`, `os.Args` | S `std::env` | S `std.process` | S `getEnv`, `commandLineParams` | S `System.getenv` | S `Environment` |
| process | S `os/exec` | S `process::Command` | S `process.Child` | S `osproc` | S `ProcessBuilder` | S `Process` |
| JSON | S `encoding/json` | X `serde_json` | S `std.json` | S `std/json` | X Jackson, Gson | S `System.Text.Json` |
| time | S `time`, zones | P `Instant`, `SystemTime`; X `chrono`, `jiff` | P `std.time`, no calendar | S `std/times` | S `java.time` | S `DateTime`, `TimeProvider` |
| regex | S `regexp` (RE2) | X `regex` | X none common | S `std/re` | S `java.util.regex` | S `Regex` |
| collections | S `slices`, `maps`, `container/heap`; no set | S `VecDeque`, `BTreeMap`, `HashSet`, `BinaryHeap` | S `ArrayList`, `HashMap`, `PriorityQueue` | S `tables`, `sets`, `deques` | S rich | S rich, `PriorityQueue` |
| random | S `math/rand/v2`, `crypto/rand` | X `rand` | S `std.Random` | S `std/random` | S `Random`, `SecureRandom` | S `Random.Shared` |
| CLI args | S `flag` | X `clap` | X (std only iterates args) | S `parseopt` | X picocli | X `System.CommandLine` |
| logging | S `log/slog` | X `log`, `tracing` | S `std.log` | S `std/logging` | S `System.Logger` | X `Microsoft.Extensions.Logging` |
| HTTP client | S `net/http` | X `reqwest` | S `std.http.Client` | S `httpclient` | S `java.net.http` | S `HttpClient` |
| hash, encoding | S `crypto/sha256`, `encoding/base64`, `encoding/hex` | X `sha2`, `base64`, `hex` | S `std.crypto`, `std.base64` | P `base64`; X `checksums` | S `MessageDigest`, `Base64`, `HexFormat` | S `SHA256`, `Convert` |
| concurrency | S goroutines, `context`; X `errgroup` | P threads; X `tokio` | S `std.Thread` | P `asyncdispatch` | S `java.util.concurrent` | S `Task`, `Channel` |

### Scripting Runtimes

| Area | Python | Ruby | Julia | Node | Deno | Bun |
| --- | --- | --- | --- | --- | --- | --- |
| files | S `pathlib`, `open` | S `File` | S `read`, `write` | S `fs` | S `Deno.readTextFile` | S `Bun.file`, `Bun.write` |
| path | S `pathlib` | S `Pathname` | S `joinpath` | S `path` | S `@std/path` | S `node:path` |
| env, args | S `os.environ`, `sys.argv` | S `ENV`, `ARGV` | S `ENV`, `ARGS` | S `process.env`, `process.argv` | S `Deno.env`, `Deno.args` | S `Bun.env`, `Bun.argv` |
| process | S `subprocess.run` | S `Open3` | S `run` | S `child_process` | S `Deno.Command` | S `Bun.spawn`, `Bun.$` |
| JSON | S `json` | S `json` | X `JSON3.jl` | S `JSON` | S `JSON` | S `JSON` |
| time | S `datetime`, `zoneinfo` | S `Time`, `Date` | S `Dates` | P `Date` | P `Date`, `@std/datetime` | P `Date` |
| regex | S `re` | S `Regexp` | S `Regex` | S `RegExp` | S `RegExp` | S `RegExp` |
| collections | S `collections`, `heapq`, `itertools` | S `Array`, `Set` | P `Dict`, `Set`; X `DataStructures.jl` | P `Map`, `Set` | P `Map`, `Set`, `@std/collections` | P `Map`, `Set` |
| random | S `random`, `secrets` | S `Random`, `SecureRandom` | S `Random` | S `crypto.randomInt` | S `@std/random` | S `crypto` |
| CLI args | S `argparse` | S `OptionParser` | X `ArgParse.jl` | S `util.parseArgs` | S `@std/cli` | S `util.parseArgs` |
| logging | S `logging` | S `Logger` | S `Logging` | P `console` | P `@std/log` | P `console` |
| HTTP client | S `urllib.request`; X `requests` | S `Net::HTTP` | S `Downloads`; X `HTTP.jl` | S `fetch` | S `fetch` | S `fetch` |
| hash, encoding | S `hashlib`, `base64` | S `Digest`; `base64` a bundled gem | S `SHA`, `Base64` | S `crypto`, `Buffer` | S Web Crypto, `@std/encoding` | S `Bun.CryptoHasher` |
| concurrency | S `asyncio` | S `Thread`, `Queue` | S `Threads`, `Channel` | S `Promise`, `worker_threads` | S `Promise`, `@std/async` | S `Promise`, workers |

### Functional And Application Languages

| Area | Kotlin | Swift | Scala | Dart | Haskell | OCaml | Elixir | Effect |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| files | P `kotlin.io` (JVM) | P Foundation | P `scala.io.Source`; X `os-lib` | S `dart:io` | S `readFile`; boot `directory` | S `In_channel` | S `File` | S `FileSystem` |
| path | P `kotlin.io.path` (JVM) | P `URL`; X `swift-system` | P `java.nio.file` | X `path` | boot `filepath` | S `Filename` | S `Path` | S `Path` |
| env, args | P `System.getenv` (JVM) | S `ProcessInfo`, `CommandLine` | S `sys.env` | S `Platform` | S `System.Environment` | S `Sys` | S `System` | S `Config` |
| process | P `ProcessBuilder` (JVM) | P Foundation `Process` | S `scala.sys.process` | S `Process.run` | boot `process` | P `Unix`, `Sys.command` | S `System.cmd` | S `ChildProcess` |
| JSON | X `kotlinx.serialization` | S `Codable` + Foundation | X circe, upickle | S `dart:convert` | X `aeson` | X `yojson` | S `JSON` (1.18) | S `Schema` |
| time | S `kotlin.time`; X `kotlinx-datetime` | S `Duration`, `Clock` | P `java.time` | S `DateTime`, `Duration` | boot `time` | P `Unix.gettimeofday` | S `DateTime`, `Calendar` | S `DateTime`, `Clock`, `Cron` |
| regex | S `Regex` | S `Regex` | S `Regex` | S `RegExp` | X `regex-tdfa` | P `Str`; X `re` | S `Regex` | P `RegExp` helpers |
| collections | S rich | S `Array`, `Set`; X `swift-collections` | S rich | S `dart:collection` | boot `containers` | S `Map`, `Set`, `Queue` | S `Map`, `MapSet` | S `HashMap`, `HashSet`, `Chunk` |
| random | S `kotlin.random` | S `SystemRandomNumberGenerator` | S `Random` | S `dart:math` | X `random` | S `Random` | P Erlang `:rand` | S `Random` |
| CLI args | X `clikt` | X `swift-argument-parser` | X `scopt` | X `args` | S `GetOpt`; X `optparse-applicative` | S `Arg`; X `cmdliner` | S `OptionParser` | S `Cli` modules |
| logging | X `kotlin-logging` | X `swift-log` | X `scala-logging` | X `logging` | X `co-log` | X `logs` | S `Logger` | S `Logger` |
| HTTP client | X `ktor` | P `URLSession` | X `sttp` | S `HttpClient`; X `http` | X `http-client` | X `cohttp` | P `:httpc`; X `Req` | S `HttpClient` |
| hash, encoding | P `Base64`, `HexFormat` | P `Data` base64; X `swift-crypto` | P JVM | P `base64`; X `crypto` | X `crypton` | P `Digest` (MD5, BLAKE2) | S `:crypto`, `Base` | S `Encoding`, `Crypto` |
| concurrency | X `kotlinx.coroutines` | S `async`, `TaskGroup` | S `Future` | S `Future`, `Isolate` | S `forkIO`; X `async` | S `Domain`; X `eio` | S `Task`, OTP | S `Fiber`, `Queue` |

### Takeaways

1. **Files, paths, env, args, and processes ship with every toolchain.**
   Most put them in std; Kotlin and Scala reuse the JVM's, and Haskell
   ships them as boot packages. Even Rust and Zig, the smallest libraries
   here, have them.
2. **JSON, CLI parsing, and HTTP split about half and half.** The
   scripting runtimes (Python, Ruby, Node, Deno, Bun) ship all three;
   compiled languages with strong package managers (Rust, Kotlin, Scala,
   Haskell, OCaml) leave them to packages. hd's single-file rule puts it
   with the scripting runtimes.
3. **Regex is std in 15 of the 20.** The holdouts are Rust, Zig, Haskell,
   OCaml, and Effect (which reuses JavaScript's).
4. **Calendar and time-zone logic is the most often external area.** Rust,
   Kotlin, Zig, Haskell, and OCaml keep it out; Go, Java, .NET, and Python
   pay for a time-zone database.
5. **Testable time is a recent addition.** .NET added `TimeProvider` in
   .NET 8, Go added `testing/synctest` in Go 1.25, and Effect ships
   `TestClock`. hd already has the seam: a `Clock` requirement trait.

## Gap Survey By Area

Each area gives the standout designs worth copying, the minimal hd module,
and its dependencies. Sketches are signatures with `pass` bodies; they
parse but are not type-checked. Tier names refer to the
[Ranked Rollout](#ranked-rollout).

### Program Arguments And Environment

Standouts:

- **Deno** reads `Deno.args` and `Deno.env.get`, and each needs a
  permission flag such as `--allow-env`: authority is declared, as hd's
  row declares it.
- **Effect `Config`** reads typed settings through a swappable
  `ConfigProvider`, so tests supply a map. hd's `MapEnv` is the same seam.

Minimal `std.host` (reads are plain calls by decision 2):

```text
pub trait Args:
    fn program(self) -> string
    fn list(self) -> List[string]

pub trait Env:
    fn get(self, name: string) -> string?
    fn names(self) -> List[string]

pub data MapArgs:
    program: string
    values: List[string]

pub data MapEnv:
    values: Map[string, string]

pub fn args() -> List[string] $ Args:
    $.use(Args).list()

pub fn env(name: string) -> string? $ Env:
    $.use(Env).get(name)
```

Depends on: the first host catalog, now [Host Capabilities](../spec/cli/command-line.md#host-capabilities).
The prototype's host bridge carries scalars and strings only, so `list`
needs a small hook that returns `List[string]`.

### Console Output And Input

Standouts:

- **Rust `eprintln!`** and **Go `fmt.Fprintln(os.Stderr, ...)`** keep
  diagnostics off standard output, so `script | jq` still works.
- **Python `sys.stdin.read()`** reads piped input whole.

Minimal additions to `std.console`:

```text
pub trait ErrorConsole:
    fn write_line!(mut self, text: string) -> Result[void, ConsoleError]

pub fn eprintln[T < Display](value: T) -> void $ ErrorConsole:
    pass

pub fn read_line!() -> string? $ ConsoleInput:
    pass

pub fn read_all!() -> string $ ConsoleInput:
    pass
```

`eprintln` drives its write with `block_on`, exactly as `println` does.
Depends on: the standard error decision, now [Standard Error](../spec/std/console.md#standard-error).

### Files And Paths

Standouts:

- **Go `os.ReadFile` and `os.WriteFile`** cover most script needs in one
  call each, and `fs.FS` with `testing/fstest.MapFS` gives an in-memory
  file system for tests.
- **Go `path/filepath`** works on plain strings: `Join`, `Dir`, `Base`,
  `Ext`, `Match`, `WalkDir`.
- **Python `pathlib`** and **Bun `Glob`** make globbing one call.

Minimal `std.path`, a newtype over the text, as Go's functions over
strings:

```text
pub type Path(string)

impl Path:
    pub fn join(self, child: string) -> Path: pass
    pub fn parent(self) -> Path?: pass
    pub fn file_name(self) -> string?: pass
    pub fn extension(self) -> string?: pass
    pub fn with_extension(self, extension: string) -> Path: pass
    pub fn components(self) -> List[string]: pass
    pub fn normalize(self) -> Path: pass
    pub fn matches(self, glob: string) -> bool: pass
```

Minimal `std.fs`, whole-file operations only (decisions 2, 3, 6):

```text
use std.path.Path

pub enum FsError:
    NotFound(path: Path)
    PermissionDenied(path: Path)
    AlreadyExists(path: Path)
    NotADirectory(path: Path)
    IsADirectory(path: Path)
    InvalidUtf8(path: Path)
    Other(message: string)

pub enum EntryKind:
    File
    Directory
    Symlink

pub data Entry:
    pub path: Path
    pub kind: EntryKind
    pub size: u64

pub trait FsRead:
    fn read_bytes!(self, path: Path) -> Result[List[u8], FsError]
    fn read_text!(self, path: Path) -> Result[string, FsError]
    fn list_dir!(self, path: Path) -> Result[List[Entry], FsError]
    fn stat!(self, path: Path) -> Result[Entry?, FsError]

pub trait FsWrite:
    fn write_bytes!(mut self, path: Path, bytes: List[u8]) -> Result[void, FsError]
    fn write_text!(mut self, path: Path, text: string) -> Result[void, FsError]
    fn append_text!(mut self, path: Path, text: string) -> Result[void, FsError]
    fn create_dir_all!(mut self, path: Path) -> Result[void, FsError]
    fn remove!(mut self, path: Path) -> Result[void, FsError]
    fn rename!(mut self, from: Path, to: Path) -> Result[void, FsError]

pub data MemoryFs:
    files: mut Map[string, List[u8]]

pub fn read_text!(path: Path) -> Result[string, FsError] $ FsRead:
    $.use(FsRead).read_text!(path)

pub fn write_text!(path: Path, text: string) -> Result[void, FsError] $ FsWrite:
    $.use(FsWrite).write_text!(path, text)

pub fn walk!(root: Path) -> Result[List[Entry], FsError] $ FsRead:
    pass

pub fn glob!(root: Path, pattern: string) -> Result[List[Path], FsError] $ FsRead:
    pass
```

`walk!` and `glob!` are hd loops over `list_dir!`. Open file handles and
streaming wait for the parked NonEscapable design
([Resource Non-Escape](OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy)).

Depends on: the host catalog; a host binding (WASI preopened directories
in the official runtime, Node's `fs` in the prototype).

### Processes

Standouts:

- **Python `subprocess.run(args, capture_output=True, check=True)`**
  turns a nonzero status into an error on request.
- **Bun `$` and zx** build a command from a template literal and quote
  each interpolated value as one argument, so no shell injection.

The spec's `Process.run!(program, args, stdin)` now returns
`Result[ProcessOutput, ProcessError]` (PROCESS-RESULT, batch 80). The
sketch below predates that decision: its two variants would become
helper errors beside the spec's enum. Minimal additions:

```text
pub enum ProcessError:
    NotFound(program: string)
    Failed(program: string, output: ProcessOutput)

impl ProcessOutput:
    pub fn success(self) -> bool: pass

pub fn run!(program: string, args: List[string] = [], stdin: string = "") -> Result[ProcessOutput, ProcessError] $ Process:
    pass

pub fn run_ok!(program: string, args: List[string] = []) -> Result[string, ProcessError] $ Process:
    pass
```

`run_ok!` returns the standard output, or `Failed` on a nonzero status,
as `check=True` does. A working directory and environment need a change
to the language-tier `Process` trait, so they wait. A `cmd"git log -n ${n}"`
prefix in Bun's style is possible with
[`@str_prefix`](../spec/lang/05-expressions.md#prefixed-strings), but it
is an agent-originated adaptation and is left out of the plan.

Depends on: a profile that binds `Process` for `hd FILE` and tasks. WASI
has no subprocess interface, so this is a toolchain host extension, as
`hd test` already binds one for integration tests.

### JSON

Standouts:

- **Rust `serde_json`**: an untyped `Value` plus derived typed codecs, and
  a `Number` that keeps 64-bit integers exact (decision 13 copies it).
- **Go `encoding/json`**: one call each way, `Marshal` and `Unmarshal`.
- **Elixir 1.18** moved JSON into std after a decade of `Jason`, a sign
  that scripts need it built in.

Both parts of `std.json` are specified: [Json](../spec/std/json.md).
The typed part, `ToJson`, `FromJson`, `encode`, and `decode`, uses the
derivation protocol that `Hash` and `Arbitrary` use. Renames, conditional
skips, and defaults per field come later through typed member facts; see
[Decided, Not Yet Applied](#decided-not-yet-applied).

### Time And Dates

Standouts:

- **Rust** splits `Instant` (monotonic) from `SystemTime` (wall), so
  elapsed-time code cannot go backwards.
- **Java `java.time`** splits `Instant` from calendar types, and keeps time
  zones in their own types.
- **.NET `TimeProvider`**, **Go `synctest`**, and **Effect `TestClock`**
  make time injectable; hd's `ManualClock` does that (decision 10).

Minimal additions to `std.time`, UTC only:

```text
pub data Timestamp:
    unix_millis: i64

pub data Instant:
    ticks: i64

pub trait Clock:
    fn now(self) -> Timestamp
    fn monotonic(self) -> Instant
    fn sleep!(mut self, duration: Duration) -> void

pub data ManualClock:
    current: Timestamp

impl Duration:
    pub fn minutes(count: i64) -> Duration: pass
    pub fn as_seconds(self) -> i64: pass

impl Timestamp:
    pub fn from_unix_milliseconds(millis: i64) -> Timestamp: pass
    pub fn since(self, earlier: Timestamp) -> Duration: pass

pub fn now() -> Timestamp $ Clock:
    $.use(Clock).now()

pub fn sleep!(duration: Duration) -> void $ Clock:
    $.use(Clock).sleep!(duration)
```

Time zones and locale formatting are excluded: they need a time-zone
database, and Rust, Kotlin, and Zig keep them out too. Spec pass 74
applied the UTC `Date`, `to_rfc3339`, `parse_rfc3339`, and
`TimeParseError` ([Dates](../spec/std/time.md#dates)).

### Text And Formatting

Standouts:

- **Rust `split_once`** and **Python `str.partition`** parse `key=value`
  in one call.
- **Python format specs** and **JavaScript `toFixed`** print `3.14` from
  `3.14159`; scripts that print money or timings need it.

Spec pass 78 applied the minimal additions:
[Splitting And Padding](../spec/std/text.md#splitting-and-padding) and
[Fixed-Point Text](../spec/std/num.md#fixed-point-text), with the
`format_f64_fixed` host hook. Regex is tier 11 of the
[Ranked Rollout](#ranked-rollout).

### Collections And Iterators

Standouts:

- **Python `collections`**: `Counter`, `defaultdict`, `deque`, and
  `itertools.groupby` cover most data shaping in scripts.
- **Kotlin collections**: `groupBy`, `associateBy`, `partition`,
  `windowed`, and `sortedBy` as methods.
- **Rust** `VecDeque` and `BinaryHeap`, and `sort_by_key`.

Spec pass 78 applied the minimal additions:
[List Helpers](../spec/std/collections.md#list-helpers),
[Counts](../spec/std/collections.md#counts), `get_or` in
[Map Methods](../spec/std/collections.md#map-methods),
[Set](../spec/std/collections.md#set), and
[More Adapters](../spec/std/iter.md#more-adapters). Decisions Q15, Q16,
and Q18 were applied earlier. `List.pop`, `insert`, `remove_at`, and
`clear` are [specified](../spec/lang/10-modules.md#built-in-methods) as
built-in methods over one list-truncate hook. Spec pass 74 applied
`Deque` and `Heap` ([Deque](../spec/std/collections.md#deque),
[Heap](../spec/std/collections.md#heap)); a min-heap is a heap of
`std.cmp.Reverse` values.

### Random Numbers

Standouts:

- **Go `math/rand/v2`**: a seeded generator value (`rand.New(rand.NewPCG(...))`)
  beside a host-seeded default.
- **Python `secrets`**: secure randomness is a separate, named API.

Spec pass 85 applied a minimal surface: [Rng](../spec/std/random.md#rng),
a seeded xoshiro128** generator with unbiased integer ranges, and `rng`,
which seeds one from `$ Random`. Its 32-bit steps need no wrapping `u64`
multiplication.

### Command-Line Parsing

Standouts:

- **Node `util.parseArgs`** and **Deno `@std/cli` `parseArgs`**: one
  function, an options table in, values and positionals out.
- **Go `flag`** prints usage text from the same table.
- **Rust `clap` derive** builds a typed struct; in hd that is a later
  `Source` template, like `FromJson`.

Spec pass 85 applied a builder, [Cli](../spec/std/cli.md): flags,
options, positionals, `--`, `CliError`, and a generated `usage` text.
`parse_args` reads `$ Args`. A typed `FromArgs` template over the same
parser is still a proposal.

### Hashing And Encoding

Standouts:

- **Go** keeps one package per format: `encoding/hex`,
  `encoding/base64` (`StdEncoding`, `URLEncoding`), `crypto/sha256.Sum256`.
- **Deno `@std/encoding`** gives the same as plain functions.

Spec pass 73 applied a minimal surface:
[Encoding](../spec/std/encoding.md) and [Digest](../spec/std/digest.md).
Still proposals: the URL-safe base64 alphabet, and a streaming `Sha256`
hasher with `update` and `finish`. MD5, SHA-1, and HMAC are excluded;
`std.fingerprint`'s algorithm is still open
([Open Issues](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work)).

### Concurrency Helpers

Standouts:

- **Deno `@std/async`**: `delay`, `deadline`, `retry` with
  `{maxAttempts, minTimeout, multiplier, maxTimeout}`, and `pooledMap`
  with a concurrency limit.
- **Go `context.WithTimeout`** and **Effect `Effect.timeoutOption`** turn a
  deadline into an ordinary outcome.

Minimal additions to `std.task`, all hd over existing intrinsics:

```text
use std.time.{Clock, Duration}

pub fn timeout![T](limit: Duration, task: mut Suspend[T]) -> T? $ Clock:
    race!(finished(task), expired(limit))

fn finished![T](task: mut Suspend[T]) -> T?:
    .Some(task!())

fn expired![T](limit: Duration) -> T? $ Clock:
    $.use(Clock).sleep!(limit)
    .None

pub data Backoff:
    pub attempts: usize
    pub initial: Duration
    pub factor: i32
    pub max: Duration

pub fn retry_with![T, E, $R](backoff: Backoff, attempt: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Clock:
    pass

pub fn map_limited![T, U, $R](items: List[T], limit: i32, work: fn!(T) -> U $ R) -> List[U] $ R:
    pass
```

`timeout!` races the task against a sleep; the loser is cancelled by
[`req.combinator.race-losers`](../spec/lang/11-requirements-and-suspension.md#r-req.combinator.race-losers).
[`all_list!`](../spec/std/task.md#all-list) is specified, as nested `all!`
calls. `map_limited!` runs batches of `limit` through `all_list!`:
a sliding window would need a new polling intrinsic, since user code
cannot write one
([`req.combinator.user`](../spec/lang/11-requirements-and-suspension.md#r-req.combinator.user)).
The structured `scope!` of decision 11 needs a new intrinsic too, so it
is a language-tier item and waits.

### Logging

Standouts:

- **Go `log/slog`**: levels, key-value fields, and a swappable handler.
- **Effect `Logger`**: the logger is a provided service, as an hd
  requirement would be.

A `std.log` with `info`, `warn`, and `error` over a `Log` requirement
trait waits on the decided but unapplied
[Observability Hooks](OPEN_ISSUES.md#observability-hooks). Until then,
`eprintln` covers script diagnostics.

### HTTP Client

Standouts:

- **`fetch`** (Node, Deno, Bun): one function from a request to a response.
- **Go `net/http/httptest`**: a scripted server for tests, as hd's
  `ScriptedHttp` would be.

The archived `Http.send!` sketch stands. Its host binding needs
`wasi:http` in the official runtime, and structured values across the
prototype's host bridge, which carries only scalars and strings. So HTTP
comes after the other host areas.

## A Script With The Proposed Surface

A task that counts words in the files a pattern names, and writes a JSON
summary. Every name past the prelude is a proposal from this plan:

```text
use std.console.{ErrorConsole, eprintln}
use std.fs.{FsError, FsRead, FsWrite, glob, read_text, write_text}
use std.host.{Args, args}
use std.json.{Json, Number, pretty}
use std.path.Path
use std.process.ExitCode

pub fn main!() -> Result[ExitCode, FsError] $ Args + FsRead + FsWrite + ErrorConsole:
    let argv = args()
    if argv.len() != 1:
        eprintln("usage: hd run count -- PATTERN")
        return .Ok(ExitCode(2))
    let totals: mut Map[string, Json] = {}
    for path in glob!(Path("."), argv[0])?:
        text := read_text!(path)?
        totals["$path"] = Json.Number(Number::from_i64(i64(text.split_whitespace().len())))
    write_text!(Path("counts.json"), pretty(Json.Object(totals)))?
    .Ok(ExitCode(0))
```

The row lists exactly what the script touches, and `hd` binds each key
([`cli.host.entry-row`](../spec/cli/command-line.md#r-cli.host.entry-row)).
The file reads are bang calls, so the script needs `main!`: a script's top
level is not a driver
([`module.init.script-not-driver`](../spec/lang/10-modules.md#r-module.init.script-not-driver)).

## The Effect Review

Effect is a TypeScript library whose `Effect<A, E, R>` is a suspended
computation with a value, a typed error, and required services. hd has
each part in the language: `fn!() -> Result[A, E] $ R`.

### Covered By hd's Own Means

| Effect | hd today |
| --- | --- |
| `Effect<A, E, R>` | a `fn!` with a `Result` and a requirement row |
| `Context`, `Tag`, `Layer`, `Effect.provide` | requirement rows, `$.use`, `$.with`, row aliases, reusable contexts |
| `Effect.all`, `Effect.race` | `all!`, `race!` |
| `Effect.retry` with a count | `retry!` |
| interruption | `cancel`, which runs `defer` suites ([Cancellation](../spec/lang/11-requirements-and-suspension.md#cancellation)) |
| `Exit`, `Option`, `Result` | `Result`, `T?` |
| `Cause.Fail` versus `Cause.Die` | `.Err` versus a panic |
| `Data`, `Equal`, `Hash`, `Order` | derived `Eq`, `Hash`, `Ord` |
| `Match`, `Pipeable` | `match`, pipe expressions |
| `Arbitrary` | `std.testing` `Arbitrary` |
| `Brand`, `Newtype` | newtypes |

### Worth Adding To std

| Effect | hd addition | Tier |
| --- | --- | --- |
| `Clock`, `TestClock` | `Clock`, `ManualClock` (decision 10) | 4 |
| `Effect.sleep`, `Effect.delay` | `sleep!` | 4 |
| `Effect.timeoutOption` | `timeout!` returning `T?`; a typed error is `.ok_or(...)` | 8 |
| `Schedule.exponential` with `Schedule.recurs` | `Backoff` data and `retry_with!`, Deno's option set | 8 |
| `Duration` helpers | `minutes`, `as_seconds` | 4 |
| `Effect.forEach` with `concurrency` | `map_limited!`, batched | 8 |
| `Random` | `std.random` | 9 |
| `Config` with `ConfigProvider` | `Env` with `MapEnv`; a typed config template later | 1 |
| `FileSystem`, `Path`, `ChildProcess` | `std.fs`, `std.path`, `std.process` helpers | 1, 2 |
| `Cli` | `std.cli` `Cli` and `parse_args` | 9 |
| `HashSet`, `Chunk` | `Set` | 3 |

### Excluded

| Effect | Why not in std |
| --- | --- |
| `Fiber`, `fork`, `FiberSet`, `FiberMap` | decision 11: structured scopes only, no detached tasks |
| `Scope`, `acquireRelease`, `addFinalizer` | `defer` is hd's cleanup, and async or fallible cleanup is the parked [Resource Non-Escape](OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy) question |
| `Schedule` combinators (`union`, `intersect`, `andThen`, `jittered`) | an algebra no other surveyed std ships; Deno's four retry options cover scripts |
| `Effect.repeat`, `repeatWhile` | a `while` loop with `sleep!` |
| `Cause.Parallel`, `Cause.Sequential` | `all!` children carry no error channel, and a failing `defer` is out of scope |
| `Stream`, `Sink`, `Channel`, `Pull` | `Iterator` covers pure pulls; a suspending stream needs a suspending `next` and handle lifetimes |
| `Queue`, `PubSub`, `Deferred`, `Latch`, `Semaphore` | useful only beside `scope!` tasks, which wait on an intrinsic |
| `Ref`, `SynchronizedRef`, STM (`TxRef`, `TxQueue`, ...) | one instance runs on one thread ([`module.instance.thread`](../spec/lang/10-modules.md#r-module.instance.thread)); `mut` is `Ref` |
| `Metric`, `Tracer`, `Logger` layers | wait on [Observability Hooks](OPEN_ISSUES.md#observability-hooks); exporters are providers |
| `Cache`, `ScopedCache`, `Pool`, `RcMap`, `RequestResolver` | application-level, and they need scopes |
| `Redacted` | decision 12 parks `Secret[T]` |
| `Cron`, `Trie`, `Graph`, `HashRing` | domain libraries; packages |
| `ManagedRuntime`, `Runtime` | hosts and entry drivers do this |
| `Ai`, `Cluster`, `Rpc`, `Sql`, `Workflow` | application domains; durable workflows are hd's replay runtime work |

## Ranked Rollout

Each tier is about one hour of `lib/std` work by one agent. Before it
starts, the owner approves its surface, and a `spec-update` pass writes
its stdlib-tier chapter (spec before implementation). Tiers that touch a
host also add a prototype host binding, a minimal TypeScript hook.

| Tier | Contents | Depends on | Value for scripts |
| --- | --- | --- | --- |
| 1 | `std.host` `Args`, `Env`, `MapArgs`, `MapEnv`, `args()`, `env()`; `ErrorConsole`, `eprintln`, `read_line!`, `read_all!` | [Host Capabilities](../spec/cli/command-line.md#host-capabilities), [Standard Error](../spec/std/console.md#standard-error); a `List[string]` result on the host bridge | a script can take input and report errors |
| 2 | `std.path` `Path`; `std.fs` `FsRead`, `FsWrite`, `FsError`, `MemoryFs`, `read_text!`, `write_text!`, `walk!`, `glob!` | tier 1's catalog; a Node `fs` binding in the prototype | a script can read and write files |
| 3 | collections and iterators: the `List`, `Iterator`, `Set`, and `counts` helpers and `Map.get_or`: [specified](../spec/std/collections.md#list-helpers); `List.pop`, `insert`, `remove_at`, and `clear` are [specified](../spec/lang/10-modules.md#built-in-methods) | the list-truncate hook in the compiler | data shaping without hand loops |
| 4 | `Clock`, `Timestamp`, `Instant`, `ManualClock`, `now()`, `sleep!` | the catalog for `Clock` | timing |
| 5 | text helpers: [specified](../spec/std/text.md#splitting-and-padding); `to_fixed`: [specified](../spec/std/num.md#fixed-point-text); `parse_f64`: [specified](../spec/std/num.md#float-parsing) | the prototype's `format_f64_fixed` and `parse_f64` hooks | formatting |
| 6 | `std.json` `Json`, `Number`, `parse`, `Display`, `pretty`, accessors: [specified](../spec/std/json.md), with float text through `parse_f64` | the prototype's `parse_f64` hook | reading and writing JSON |
| 7 | `ToJson` and `FromJson` templates; `encode`, `decode`: [specified](../spec/std/json.md#typed-json) | tier 6 | typed JSON |
| 8 | `timeout!`, `map_limited!`; `Backoff`, `retry_with!`, and `all_list!`: [specified](../spec/std/task.md) | tier 4's `Clock`; [Retry With Backoff](../spec/std/task.md#retry-with-backoff) | robust automation |
| 9 | `std.random` `Rng` and `rng`: [specified](../spec/std/random.md#rng); `std.cli` `Cli`, `parse_args`, and `usage`: [specified](../spec/std/cli.md) | tier 1's `Args`; the prototype's std loader listing `cli` | real command-line tools |
| 11 | `std.regex`: the RE2 subset, linear time, no backreferences, written in hd: done. The syntax, `is_match`, `find`, `find_all`, `captures`, `replace`, and `split`: [specified](../spec/std/regex.md); no flags | the prototype's std loader listing `regex` | filtering lines by pattern |

Later, blocked:

| Item | Blocked on | Kind |
| --- | --- | --- |
| `std.process` helpers bound for `hd FILE` and tasks | a toolchain profile that binds `Process` | host profile |
| `std.http` | `wasi:http` binding; structured values over the host bridge | runtime |
| `std.log` | [Observability Hooks](OPEN_ISSUES.md#observability-hooks), decided, not applied | language and runtime |
| `scope!`, `start`, `join!` | a new polling intrinsic | language tier |
| file handles, streaming, sockets | the parked NonEscapable design | language feature |
| `Process` with working directory and environment | a change to the language-tier `Process` trait | language tier |
| `std.bytes.Bytes` (decision 5) | none; it waits for a use that `List[u8]` serves badly | library |
| typed config from `Env`, typed CLI structs | tiers 7 and 9, as more `Source` templates | library |

Tier 3 needs no host decision, so it can run first if the catalog
question waits.

## Sources

Effect:

- [Effect v4 API reference](https://effect.website/docs/v4/api/effect) (module list).
- [Built-in schedules](https://effect.website/docs/scheduling/built-in-schedules/).
- [Timing out](https://effect.website/docs/error-management/timing-out/).
- [Scope](https://effect.website/docs/resource-management/scope/).
- [Cause](https://effect.website/docs/data-types/cause/).

Standard library references, one per column:

- Go: [pkg.go.dev/std](https://pkg.go.dev/std); [testing/synctest](https://pkg.go.dev/testing/synctest).
- Rust: [doc.rust-lang.org/std](https://doc.rust-lang.org/std/).
- Zig: [ziglang.org/documentation/master/std](https://ziglang.org/documentation/master/std/).
- Nim: [nim-lang.org/docs/lib.html](https://nim-lang.org/docs/lib.html); [Nim 2.0 changelog](https://nim-lang.org/blog/2023/08/01/nim-v20-released.html) (checksums moved out).
- Java: [docs.oracle.com/en/java/javase/21/docs/api](https://docs.oracle.com/en/java/javase/21/docs/api/index.html).
- C#/.NET: [learn.microsoft.com/dotnet/api](https://learn.microsoft.com/en-us/dotnet/api/); [TimeProvider](https://learn.microsoft.com/en-us/dotnet/api/system.timeprovider).
- Python: [docs.python.org/3/library](https://docs.python.org/3/library/index.html).
- Ruby: [docs.ruby-lang.org](https://docs.ruby-lang.org/en/master/); [Ruby 3.4 bundled gems](https://www.ruby-lang.org/en/news/2024/12/25/ruby-3-4-0-released/).
- Julia: [docs.julialang.org standard library](https://docs.julialang.org/en/v1/).
- Node: [nodejs.org/api](https://nodejs.org/api/); [util.parseArgs](https://nodejs.org/api/util.html#utilparseargsconfig).
- Deno: [jsr.io/@std](https://jsr.io/@std); [@std/async](https://jsr.io/@std/async).
- Bun: [bun.sh/docs](https://bun.sh/docs); [Bun shell](https://bun.sh/docs/runtime/shell).
- Kotlin: [kotlinlang.org/api/core/kotlin-stdlib](https://kotlinlang.org/api/core/kotlin-stdlib/).
- Swift: [Swift standard library](https://developer.apple.com/documentation/swift/swift-standard-library); [SE-0329 Clock](https://github.com/apple/swift-evolution/blob/main/proposals/0329-clock-instant-duration.md).
- Scala: [scala-lang.org/api](https://www.scala-lang.org/api/current/).
- Dart: [api.dart.dev](https://api.dart.dev/).
- Haskell: [base on Hackage](https://hackage.haskell.org/package/base).
- OCaml: [OCaml standard library](https://ocaml.org/manual/latest/api/index.html).
- Elixir: [hexdocs.pm/elixir](https://hexdocs.pm/elixir/); [Elixir 1.18 JSON](https://hexdocs.pm/elixir/JSON.html).

hd sources: the deleted design record, `git show ed7fbfc5^:future-work/STDLIB.md`;
[lib/std/structure.hd](../lib/std/structure.hd);
[lib/std/task.hd](../lib/std/task.hd);
[src/host-functions.ts](../src/host-functions.ts).

## Parse Log

Each `text` block was parsed with `hd debug parse`. Parsing checks syntax
only; no block is type-checked.

| Block | Section | Result |
| --- | --- | --- |
| 1 | Program Arguments And Environment | parses |
| 2 | Console Output And Input | parses |
| 3, 4 | Files And Paths | parse |
| 5 | Processes | parses |
| 6 | Time And Dates | parses |
| 7 | Concurrency Helpers | parses |
| 8 | A Script With The Proposed Surface | parses |

A first draft of block 8 wrote module-qualified types, such as
`Map[string, json.Json]`, and the parser rejected them; the block now
imports the names.
