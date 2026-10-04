# Stdlib Audit (task #239)

Status: audit report. Nothing in it is accepted behavior. Verdicts on pure
stdlib choices are recommendations under the owner's delegation of stdlib
API design (2026-10-03). Language-tier and cross-cutting points are
questions at the end.

Under review, at `origin/main` 0b18886c:

- every `own` row of [STDLIB_CALLS.md](../future-work/STDLIB_CALLS.md);
- [STDLIB_PLAN.md](../future-work/STDLIB_PLAN.md), "What Exists Today" and
  "Decided, Not Yet Applied";
- all of [spec/std/](../spec/std/README.md), and the std parts of
  [Modules](../spec/lang/10-modules.md#prelude) (prelude, Console,
  Processes, Built-In Methods) and the
  [Error Trait](../spec/lang/09-traits.md#error-trait);
- `lib/std/*.hd`, `test/std/*.hd`,
  [KNOWN_FAILURES.tsv](../test/portable/KNOWN_FAILURES.tsv) (stdlib-tier
  rows), [stdlib-items.tsv](../spec/conformance/stdlib-items.tsv);
- [examples/dogfood](../examples/dogfood), [dogfood-199.md](dogfood-199.md)
  and [hd-writing-log.md](hd-writing-log.md) for usage friction.

The report has no hd code blocks, so it has no Parse Log.

## Summary

| Count | What |
| --- | --- |
| 89 | `own` calls reviewed: 77 keep, 9 revisit, 3 change |
| 30 | cross-module inconsistencies |
| 16 | gaps a script needs; 11 seen in the logs, 5 not yet hit |
| 23 | spec/lib agreement rows (12 unimplemented, 5 unspecified, 6 disagreeing), plus fixture coverage |
| 4 | capability findings |
| 4 | owner questions |
| 588 of 1050 | stdlib rule IDs named by at least one fixture (56%) |

### Applied In Pass 89

Pass 89 (#246) applied these items; the
[Audit Pass 89](../future-work/STDLIB_CALLS.md#audit-pass-89) rows log
each call.

| Item | Status |
| --- | --- |
| Q2, milliseconds spelling | applied |
| Top 10: items 1, 2, 4, 5, 9, 10 | applied |
| Top 10: item 6 | applied: every item specified; none made private |
| Top 10: item 7 | applied: tag `STD-HELPERS`, STDLIB_PLAN row |
| Top 10: item 8 | `ScriptedInput` applied; `write_error_line!` waits for its `lib/std` declaration and a host entry (STD-HELPERS; the host-glue cause is fixed); `ScriptedProcess::new` applied in pass 90 with [process.md](../spec/std/process.md) |
| Inconsistencies 1, 2, 4, 5, 6, 8, 10 to 17, 22, 29 | applied; 9, 18, 19, 24, 25, 26, 28 kept as recommended; 3 kept; 27 waits for Q3 |
| Inconsistencies 7, 20, 21, 30 | applied in pass 90 (#247); 27 applied with Q3 |
| Inconsistency 23 (lib) | not yet applied |
| Own calls rated change | all three applied |
| Own calls rated revisit | sizes and scalar primitives applied; the rest kept and logged; zip/chain waits for the owner |
| Regex row's `JsonError` claim | corrected |
| Q1, POSITION-UNIT | applied: the owner chose byte offsets |
| Q4, CHAR-SCALAR-U32 | applied: the owner chose `u32`; the checked `char_from_scalar` waits for the compiler (CHAR-SCALAR) |
| Q3, PROCESS-RESULT | applied in pass 90 (#247): the owner chose `Result[ProcessOutput, ProcessError]` with `NotFound`, `PermissionDenied`, and `Other(message)` |
| Footprint `hd_run!` move | measured in pass 90 (#247) and not applied: it drops only `std.process` from test joins, about 8 ms of 240 |

### Top 10 Recommended Changes

| # | Change | Why first |
| --- | --- | --- |
| 1 | `impl Eq for Ordering` in `std.cmp` | 4 log rows; owner Q21 says std value types implement `Eq`; Rust's `Ordering` is `Eq` |
| 2 | Every std error enum implements `std.error.Error` | Today only `ContextError` does, so `?` into `Result[T, Error]` and `.context(...)` reject `FsError`, `JsonError`, `ParseNumberError`, ... |
| 3 | One `position` unit across std errors (owner question Q1) | JSON, hex, time and number errors count characters; UTF-8 and regex errors count bytes; `slice` takes bytes |
| 4 | Close the std-type trait gaps: `ConsoleError` (`Debug`, `Eq`), `Captures` (`Debug`), `ExitCode` and `ProcessOutput` (`Eq`), `ResourceError` (`Display`) | `std-format.debug.std-types` and owner Q21 already require them |
| 5 | `List` gets `get(index) -> T?`, `is_empty`, `take(count)`, and `extend(other)` | Every other sequence has `get` and `is_empty`; the dogfood wrote `chunks(n).first()` for `take` |
| 6 | Specify the 23 public `lib/std` items that have no spec, or make them private | `string.upper`, `trim_start`, `contains`, `find`, `List.filter`, `first`, `reversed`, `sorted_by`, `cmp.min`, `max`, `join`, `StringBuilder`, `BufferConsole`, ... |
| 7 | Give the missing script helpers one tracking tag | `eprintln`, `read_line!`, `now`, `sleep!`, `read_text!`, `write_text!` are specified, absent, and in no `KNOWN_FAILURES` tag |
| 8 | Add a `ConsoleInput` test provider, `ScriptedProcess::new`, and `Console.write_error_line!` in `lib/std` | Owner decision 4 (a provider beside each host trait); the language-tier `Console` declares `write_error_line!` |
| 9 | Align `Rng` with `Choices`: one name for "pick one item", one shape for `int`, one seeded-constructor name | The pass-85 call claims they match; they do not |
| 10 | `retry!`'s `times` and `Backoff.attempts` become `usize` | Owner rule "every size is `usize`"; `it_prop`'s `cases` already is |

## Inconsistencies

| # | Item | Place A | Place B | Recommendation |
| --- | --- | --- | --- | --- |
| 1 | Error `position` unit | characters: [`std-json.error.position`](../spec/std/json.md#r-std-json.error.position), [`std-encoding.error.position`](../spec/std/encoding.md#r-std-encoding.error.position), [`std-time.parse-error.position`](../spec/std/time.md#r-std-time.parse-error.position), [`std-num.parse.invalid-digit`](../spec/std/num.md#r-std-num.parse.invalid-digit) | bytes: [`std-text.utf8.error.invalid`](../spec/std/text.md#r-std-text.utf8.error.invalid), [`std-regex.error.position`](../spec/std/regex.md#r-std-regex.error.position), [`module.string.byte-offsets`](../spec/lang/10-modules.md#r-module.string.byte-offsets) | owner question Q1; recommend bytes |
| 2 | Name of the offset field | `EscapeError.offset` (`lib/std/text.hd:33`) | `position` in every other error | rename to `position`, or make `EscapeError` private (it has no spec) |
| 3 | Every error variant has a position | `Utf8Error.Truncated` has none ([`std-text.utf8.error.display.truncated`](../spec/std/text.md#r-std-text.utf8.error.display.truncated)) | `InvalidSequence(position)`; Rust's `Utf8Error.valid_up_to()` covers both | `Truncated(position)`, the start of the cut sequence |
| 4 | Errors implement `Error` | only `ContextError` (`lib/std/error.hd:81`) | the language spec's own example writes `impl Error for FsError` ([Erased Errors](../spec/lang/09-traits.md#erased-errors)) | add `impl Error for X` to `ParseNumberError`, `Utf8Error`, `DecodeError`, `FsError`, `JsonError`, `TimeParseError`, `CliError`, `RegexError`, `ConsoleError` |
| 5 | `Eq` on std value types | `Ordering` has none (`lib/std/cmp.hd:9`) | owner Q21; `Deque`, `Set`, `Match` have it | `impl Eq for Ordering` |
| 6 | Error enum traits | `ConsoleError`: `Display` only (`lib/std/console.hd:55`) | `FsError`, `JsonError`, `CliError`: `Eq`, `Debug`, `Display` | add `Debug` and `Eq` |
| 7 | `Debug` on std types | `Captures` has none (`lib/std/regex.hd:42`) | [`std-format.debug.std-types`](../spec/std/format.md#r-std-format.debug.std-types): every public type whose members are `Debug` | add it |
| 8 | Draw one item | `Rng.choose(items) -> T?` ([`std-random.rng.choose`](../spec/std/random.md#r-std-random.rng.choose)) | `Choices.pick(items) -> T` ([`std-testing.choices.pick`](../spec/std/testing.md#r-std-testing.choices.pick)), which indexes `items[0]` on an empty list | one name, `choose`; both return `T?`, or both panic on `[]` |
| 9 | Integer draw shape | `Rng.int(range: Range[i64])` | `Choices.int[N](lo, hi)` | keep `Rng.int(range)`; give `Choices` the same `Range` form when `Integer` can name it |
| 10 | Seeded constructor | `SeededRandom::new(seed)` | `Rng::from_seed(seed)` | `Rng::new(seed)`, as every other provider and value uses `new` |
| 11 | Millisecond spelling | `Duration::milliseconds`, `as_milliseconds` ([`std-time.suffix.std.duration-api`](../spec/std/time.md#r-std-time.suffix.std.duration-api)) | `Timestamp::from_unix_millis`, `unix_millis`, `Instant::from_millis` ([`std-time.timestamp.public-api`](../spec/std/time.md#r-std-time.timestamp.public-api)) | owner question Q2 |
| 12 | Accessor prefix | `as_milliseconds()` | `unix_millis()` | follows Q2 |
| 13 | `Instant` value | `Instant` has no accessor | `Duration` and `Timestamp` do | add one with the Q2 spelling |
| 14 | Text accessor name | `Regex.as_str()` (`lib/std/regex.hd:66`) | `Json.as_text()` (`lib/std/json.hd:96`) | `Regex.pattern()`, Python's name; hd has no `str` type |
| 15 | Count types | `retry!(times: i32)`, `Backoff.attempts: i32` ([Retry](../spec/std/task.md#retry)) | `it_prop(cases: usize)`, `take(count: usize)` | `usize` |
| 16 | `is_empty` | `List` and `Map` have none ([Built-In Methods](../spec/lang/10-modules.md#built-in-methods)) | `string`, `Set`, `Deque`, `Heap` have it | add to both, stdlib tier |
| 17 | Optional lookup by index | `List` has no `get` | `Deque.get`, `Map.get`, `Json.at`, `Captures.get` return `T?` | `List.get(index) -> T?` |
| 18 | Text join | `join(parts, separator)`, a free function (`lib/std/text.hd:459`) | list helpers are methods; Rust writes `parts.join(sep)` | keep the free function until an inherent impl on `List[string]` is allowed; then a method |
| 19 | Parse entry points | free `parse_i32(text)`, `json.parse(text)` | associated `Timestamp::parse_rfc3339(text)`, `string::from_utf8(bytes)`, `Regex::new(pattern)` | keep; write the rule in spec/std/README: a module format is a free function, a type's own text is associated |
| 20 | Text output | free `pretty(json)` | methods `to_rfc3339()`, `to_fixed(n)` | `Json.pretty()`, a method |
| 21 | Provider constructors | `ScriptedProcess` has no `new` and private fields (`lib/std/process.hd`) | every other provider has `X::new` (STDLIB_CALLS, Time, pass 70) | add `ScriptedProcess::new(outputs)` (decided Q14-22) |
| 22 | Provider per host trait | `ConsoleInput` has none | `Console` has `BufferConsole`, `Clock` has `ManualClock`, ... (owner decision 4) | a `ScriptedInput::new(lines)` |
| 23 | `Console` surface | `lib/std/console.hd:12` has `write_line!` only | [Console](../spec/lang/10-modules.md#console) also declares `write_error_line!` with a default | add it; `BufferConsole` then records both streams |
| 24 | Name `min` | `std.time.min`, the minutes suffix | `std.cmp.min` and `List.min()` | keep (owner-decided suffixes); note the rename import in [Duration Suffixes](../spec/std/time.md#duration-suffixes) |
| 25 | Three meanings of count | `string.count(needle)` | `Iterator.count()`, `counts(items)` | keep: Python's and Rust's names |
| 26 | File size type | `Entry.size: u64` ([File System Errors](../spec/std/fs.md#file-system-errors)) | owner rule: sizes are `usize` (`u32`) | keep `u64`: files pass 4 GiB; say so in a Why |
| 27 | Absence vs failure | `Process.run! -> ProcessOutput?` ([Processes](../spec/lang/10-modules.md#processes)) | `FsRead` methods return `Result[_, FsError]` | owner question Q3 |
| 28 | `expect` text | `Result.expect` panics with `message` only ([`std-result.expect.panic`](../spec/std/result.md#r-std-result.expect.panic)) | Rust appends the error's `Debug` text | revisit when a bounded overload exists; until then keep |
| 29 | `Display` on time values | `Duration` has it | `Timestamp`, `Instant` have none | `Timestamp` displays as `to_rfc3339()`, as Go's `Time.String`; `Date` stays without (confirmed) |
| 30 | `Eq` on ranges and exit data | `Range`, `RangeFrom`, `RangeTo`, `ExitCode`, `ProcessOutput` have none (`lib/std/ops.hd`, `process.hd`) | owner Q21 | add `Eq` |

## Own-Call Review

Rows are in [STDLIB_CALLS.md](../future-work/STDLIB_CALLS.md) order.

### Hashing And Encoding

| Call | Verdict | Reason |
| --- | --- | --- |
| `string` hashes length, then bytes | keep | length-first is prefix-free, and one rule covers strings and lists; Rust's `0xff` terminator is the other common choice |
| tuple hashes elements only | keep | Rust's tuple `Hash` does the same |
| `T?` hashes a tag byte, then the payload | keep | Rust hashes the discriminant first |

### Numbers

| Call | Verdict | Reason |
| --- | --- | --- |
| `unwrap_or`, `map_err`, `checked_*`, ... specified because cited | keep | rules need targets |
| first error wins, `"99999999999x"` is `OutOfRange` | keep | Rust's `from_str_radix` and Go's `ParseInt` stop at the overflow too |
| `abs_diff` one impl per width | keep | internal; Rust declares it per width |
| `to_digit` panics outside 2 to 36 | keep | Rust's `char::to_digit` |
| `to_fixed` rounds the exact value, ties to even | keep | Rust's `{:.2}` and Python's `format` round the exact binary value |
| hooks write `NaN`, `inf`, `-inf` | keep | Rust's `Display` text |

### Text

| Call | Verdict | Reason |
| --- | --- | --- |
| Unicode 17.0 range tables | keep | compact; a version had to be named |
| `Utf8Error` is `InvalidSequence(position)` or `Truncated` | revisit | `Truncated` should carry a position (inconsistency 3) |
| `count("")` is chars plus one | keep | Python's `str.count` |
| `split_once("")` matches at 0 | keep | Rust's `split_once` |

### Collections And Iterators

| Call | Verdict | Reason |
| --- | --- | --- |
| `std.cmp.Reverse` specified | keep | Rust's `Reverse` |
| `List.push` forwards to `append` | keep | temporary; LIST-PUSH-POP removes it |
| `all_list!` from nested `all!` | keep | no intrinsic; the ALL-LIST trap is fixed, and `task-all-list-order` passes |
| `partition` is `(kept, rest)`; `windows(0)` panics | keep | Rust's order and panic |
| `take_while` consumes the rejected item; `zip` pulls `self` first | keep | Rust |
| `zip`, `chain` call `other.iter()` once at the call | keep | Rust's `into_iter` at the call |
| an `Iterator` argument to `zip`/`chain` is an error | revisit | Rust and Python accept any iterator; the cause is the language rule `flow.for.iterator-not-iterable`, so it waits for the owner |
| `max(a, b)` returns `a` on a tie | keep | Python's rule, and one rule with `List.max` |
| unspecified `List.zip` keeps `List[U]` | change | specify it or drop it; `xs.iter().zip(ys)` covers it |

### Time

| Call | Verdict | Reason |
| --- | --- | --- |
| providers built with `X::new`, private fields | keep | but `ScriptedProcess` breaks it (inconsistency 21) |
| `ManualClock` has no `advance`; monotonic from the epoch | keep | test bodies suspend, so `sleep!` is at hand |
| `MemoryFs` keeps a `dirs` list | keep | Go's `fstest.MapFS` also lists directories |
| Go-style `Duration` text | keep | Go's `Duration.String` |

### Errors

| Call | Verdict | Reason |
| --- | --- | --- |
| `ErrorReport` displays `caused by:` lines | keep | anyhow's `Debug` report lists `Caused by:` lines the same way |
| `ContextError` `Debug` shows the cause's `Display` | keep | the cause is erased |
| `ConsoleError.Closed` displays `console closed` | keep | text is fine; its `Debug` and `Eq` are missing (inconsistency 6) |

### JSON

| Call | Verdict | Reason |
| --- | --- | --- |
| externally tagged enums, object payload, `_0` keys | keep | serde's default tag, one payload shape |
| unknown keys ignored | keep | serde and Go `encoding/json` |
| missing key reads as `null` | keep | serde's `Option` handling |
| only `Map[string, V]` encodes | revisit | serde_json and Go stringify integer keys; `Map[i32, V]` is common in scripts |
| `JsonError` decode variants with string paths | keep | `serde_path_to_error`'s form; the position unit is Q1 |
| exact integers, `1.0` not an integer, NaN as `null` | keep | serde_json |
| `char` as a one-char string; no flattening | keep | serde |
| no tuple template yet | keep | add when asked |

### Testing

| Call | Verdict | Reason |
| --- | --- | --- |
| discard is a panic with a fixed message | keep | Hypothesis's `assume` raises too |
| fresh draws after `replay` runs out | keep | a shrunk replay must still finish its case; `0` forever skews it |

### Sizes

| Call | Verdict | Reason |
| --- | --- | --- |
| every std length, count, index, position is `usize` | revisit | `retry!`'s `times` and `Backoff.attempts` were missed (inconsistency 15) |
| negative-count rules retired | keep | a `usize` cannot be negative |
| retired IDs renamed | keep | bookkeeping |
| `Choices` maxima `usize`; `PropertyCase.size` `i32` | keep | a reach exponent is not a size |
| `TestRunner.row`, `PropertyRunner.start`, `Random.fill` take `usize` | keep | they are list indices and counts |
| `char_scalar`, `char_from_scalar` stay `i32` | revisit | a scalar is never negative and `to_digit` returns `u32`; Rust uses `u32` (owner question Q4) |
| `lib/std` keeps `i32` until the compiler adds `usize` | keep | tracked as SIZES-UNSIGNED |

### Random Numbers

| Call | Verdict | Reason |
| --- | --- | --- |
| `Rng` is xoshiro128**, SplitMix32 seeding | keep | fixed output across implementations; rand's `SmallRng` is the same family |
| `rng()` seeds from one `next_u64` | keep | Go's `math/rand/v2` |
| surface `from_seed`, `int`, `float`, `bool`, `choose`, `shuffle`, `sample` | change | "named as `Choices` names its draws" is untrue: `choose` vs `pick`, `int(range)` vs `int(lo, hi)`, `from_seed` vs `SeededRandom::new` |
| `int` takes one `Range[i64]` | keep | Go's `Int64N`; generic later |
| rejection sampling | keep | `arc4random_uniform` |
| `float` from 53 bits; `bool` from the top bit | keep | rand's construction |
| `choose([])` is `.None` | keep | Rust's `choose` |
| `shuffle` back to front; `sample` from the front; count past length panics | keep | Rust and Python |

### Command-Line Parsing

| Call | Verdict | Reason |
| --- | --- | --- |
| a builder, not a `FromArgs` derivation | keep | Go's `flag`, Node's `parseArgs` |
| value builder; duplicate names panic | keep | Go's `flag` panics |
| no short clusters or attached values | keep | Go's `flag` |
| options interleave; `--` ends them; `-` is positional | keep | POSIX, clap |
| a repeated option keeps its last value | revisit | `-I a -I b` cannot be read; argparse has `append`, clap `ArgAction::Append` |
| positionals required; extras kept | keep | one error kind |
| four `CliError` variants with `Eq`, `Debug`, `Display` | keep | one variant per failure kind |
| `Display` texts | keep | short, like Go's |
| no automatic `--help` | keep | no hidden flags |
| `Parsed` public fields plus `flag`, `value` | keep | plain data |
| `usage` layout | keep | Go's `flag` layout |
| `parse_args` with `$ Args` beside `parse` | keep | testable |

### Regular Expressions

| Call | Verdict | Reason |
| --- | --- | --- |
| no flags in part 1 | revisit | case-insensitive matching is a common script need; ASCII-only `(?i)` first avoids the folding choice |
| `\d`, `\w` ASCII; `\s` Unicode; `.` excludes `\n` | keep | RE2 plus hd's `trim` |
| byte offsets | keep | right unit; but its reason wrongly says `JsonError` takes bytes (Q1) |
| `RegexError` is data with a `kind` | keep | one position field; differs from other errors (an enum per variant) but reads well |
| `a**` is `NothingToRepeat` | keep | Go |
| stray `{`, `}`, `]` literal | keep | RE2, Go |
| `\b` is `BadEscape` | revisit | Go and RE2 support `\b`; word matching is common in scripts |
| counts at most 1000; size at most 10,000 | keep | RE2 |
| `^`, `$` at text ends only | keep | RE2 without `(?m)` |
| Pike VM in hd | keep | linear time |
| `Regex.as_str`, `Debug`; traits on `Match` and errors | change | `as_str` clashes with `Json.as_text`; name it `pattern()`. `Captures` also lacks `Debug` |
| flags stay out (pass 87) | revisit | as above |
| RE2 capture semantics | keep | RE2, Go, Rust agree |
| empty optional iteration not taken | keep | stated for portability |
| `Captures` with `len`, `get`, `name` | keep | Rust's shape |
| both named-group spellings | keep | RE2, Go 1.22, Rust |
| `BadGroupName`, `DuplicateGroupName` | keep | distinct fixes |
| `find_all` empty-match rule | keep | Go and Rust |
| `captures_all` | keep | Rust's `captures_iter` |
| `$N`, `${N}`, `${name}`, `$$` | keep | Go and Rust minus bare names |
| string idiom in the spec | keep | needed by interpolation |
| `split` gives k + 1 pieces | keep | Rust |
| O(m × g × n) bound | keep | documented like Rust |

## Gaps

Ranked by how often real code hit them. "Log" rows are lines of
[hd-writing-log.md](hd-writing-log.md) by their Wrote cell; "F" rows are
[dogfood-199.md](dogfood-199.md) findings.

| Rank | Gap | Hits | Evidence | Status / recommendation |
| --- | --- | --- | --- | --- |
| 1 | `Ordering` `Eq` and `Display` | 5 | log: `"abc".cmp("abd") == Ordering.Less`, `assert_equal(later_first(2, 1), .Less, ...)`, `item.cmp(best) == .Greater`, `"${(1).cmp(2)}"` twice | add `Eq` (top 1); keep no `Display` (Rust has none), `debug(o)` works |
| 2 | `List` `push`/`pop` | 4 | log: `chars.push(c)` twice, `walk.path.filter(...)` to pop, `keys = keys + [key]`; F7 | specified; LIST-PUSH-POP pending |
| 3 | `Map.keys`, `get_or`, `contains_key` | 3 | log: `fields.keys()`; F5; textstats' `match counts.get(word)` counter | specified; STD-1 pending |
| 4 | `Display` for an optional | 2 | log: `"${a.checked_add(10)}"`, `"${'z'.to_digit(36)}"` | keep none (Rust has none); have `unsatisfied-trait-bound` suggest `debug(...)` |
| 5 | `min`/`max` need a `use` | 2 | log: `max(at, level[dep] + 1)`; F11 | keep: owner decision 7, the prelude does not grow |
| 6 | `all!` over a list | 2 | log: `all!(...items.map(...))`; F8 | `all_list!` specified; ALL-LIST fixed, its fixture passes |
| 7 | `List.take(n)` | 1 | textstats: `.chunks(limit).first().unwrap_or([])` | add `take(count)` (top 5) |
| 8 | sum of a list | 1 | textstats: `.iter().fold(0, ...)` | add `Iterator.sum()` for `T < Num`, as Rust |
| 9 | `Result.unwrap` | 1 | log: `Regex::new(p).unwrap()` | keep `expect` only: it names the assumption; have `unknown-method` suggest `expect` |
| 10 | list concatenation | 1 | log: `keys + [key]` | add `extend(mut self, other: List[T])`; Rust's `extend` |
| 11 | `Map::new()`, `Map.insert` | 1 | log: `Map.new()`, `m.insert(5, "five")` | keep `{}` and `m[k] = v`; a did-you-mean hint |
| 12 | float math: `sqrt`, `floor`, `round`, `abs`, `powi` | 0 | absent from spec and `lib/std` | add to `std.num` on `f64`; most need a host hook or exact hd code, so the owner approves primitives |
| 13 | `Path` operations: `join`, `parent`, `file_name`, `extension` | 0 | [`path.md`](../spec/std/path.md) has only the newtype | add, as Go's `path/filepath` |
| 14 | unsigned parsing: `parse_u32`, `parse_u64`, `parse_usize` | 0 | `usize` indices now come from text | add beside `parse_i32` |
| 15 | `print` without a newline | 0 | `Console` writes lines only | add when a prompt is needed; it needs a `Console` method (language tier) |
| 16 | integer `rotate_left`, `rotate_right` | 0 | `lib/std/digest.hd` writes a private `rotate_right` | add to `std.num` |

Resolved since the dogfood: `char.is_ascii_digit` (F9), `Timestamp.unix_millis` (F10), and `Eq`/`Debug` on `Utf8Error` and `ParseNumberError` (three log rows).

## Spec, Lib, And Fixture Agreement

### Spec Items With No `lib/std` Implementation

| Item | Spec | Tracked |
| --- | --- | --- |
| `List.pop`, `insert`, `remove_at`, `clear` | [Built-In Methods](../spec/lang/10-modules.md#built-in-methods) | LIST-PUSH-POP, `LIST-POP` |
| `Map.contains_key`, `keys`, `values`, `get_or` | [Map Methods](../spec/std/collections.md#map-methods) | `STD-1` |
| `eprintln`, `read_line!` helper | [Console](../spec/std/console.md) | plan prose only; no tag |
| `now`, `sleep!` helpers | [Clock Helpers](../spec/std/time.md#clock-helpers) | plan prose only; no tag |
| `read_text!`, `write_text!` helpers | [File Helpers](../spec/std/fs.md#file-helpers) | plan prose only; no tag |
| `Console.write_error_line!` | [Console](../spec/lang/10-modules.md#console) | not tracked |
| `Backoff`, `retry_with!` | [Retry With Backoff](../spec/std/task.md#retry-with-backoff) | RETRY-WITH (held) |
| `snapshot_check`, `PropertyCase` | [Runner Capabilities](../spec/std/testing.md#runner-capabilities) | SNAPSHOT-ROW |
| `pad_start`/`pad_end` `fill` default | [Splitting And Padding](../spec/std/text.md#splitting-and-padding) | `METHOD-DEFAULT` |
| `TypeId`, `SelfRef` `Debug` | [`std-format.debug.std-types`](../spec/std/format.md#r-std-format.debug.std-types) | STD-DEBUG |
| `use std.cli`, `use std.regex` | [cli.md](../spec/std/cli.md), [regex.md](../spec/std/regex.md) | STD-CLI, STD-REGEX |
| `usize` alias and every size | [The usize Alias](../spec/lang/04-type-system.md#the-usize-alias) | SIZES-UNSIGNED: 57 `U32-SIZES` rows, as the plan says |

The six missing helpers share one cause: the std loader renames top-level
names as text, so a helper named like a trait method breaks that method.
**Recommendation:** one tag, such as `STD-HELPERS`, in
`src/KNOWN_ISSUES.md` and a known-failure row per helper, so the root
cause is fixed once.

### `lib/std` Public Items With No Spec

| Module | Items |
| --- | --- |
| `std.text` | `upper`, `trim_start`, `trim_end`, `ends_with`, `contains`, `find`, `is_empty`, `strip_prefix`, `strip_suffix`, `join`, `StringBuilder`, `process_escapes`, `EscapeError` |
| `std.collections` | `List.filter`, `first`, `last`, `reversed`, `sorted_by`, `zip` |
| `std.cmp` | `min`, `max` (the tie rule is a pass-81 call with no rule) |
| `std.console` | `BufferConsole` (only a language-tier example declares its own) |
| `std.process` | `ScriptedProcess` (decided Q14-22, not yet applied) |

**Recommendation:** specify each in its chapter, the text methods in
[String Methods](../spec/std/text.md#string-methods). `process_escapes`
and `EscapeError` look internal to the `r` prefix: make them private.

### Spec Rules With No Fixture

Counted by rule IDs that some fixture file names. A fixture may test a
rule without naming it, so this is a lower bound.

| Chapter | Rules | Named | Chapter | Rules | Named |
| --- | --- | --- | --- | --- | --- |
| cli | 53 | 39 | iter | 51 | 34 |
| cmp | 26 | 7 | json | 129 | 57 |
| collections | 71 | 44 | num | 71 | 28 |
| console | 9 | 5 | ops | 25 | 11 |
| digest | 7 | 5 | option | 13 | 7 |
| encoding | 23 | 11 | path | 3 | 3 |
| error | 27 | 15 | random | 41 | 36 |
| format | 26 | 8 | regex | 119 | 111 |
| fs | 35 | 10 | result | 20 | 12 |
| hash | 30 | 18 | task | 18 | 6 |
| host | 15 | 6 | testing | 107 | 31 |
| text | 48 | 29 | time | 83 | 55 |

Sampled gaps worth a fixture first:

| Chapter | Unnamed rules |
| --- | --- |
| fs | every `MemoryFs` error row (`std-fs.memory.error.*`), `std-fs.write.rename`, `std-fs.write.append` |
| task | `std-task.combinator.retry.loop`, `.last-error`, `.cancel`; `std-task.all-list.*` |
| cmp | the derived `Eq`/`Ord` template rules `std-cmp.derive.*`, `std-cmp.clamp.decl` |
| host | `std-host.args.*`, `std-host.env.names` |
| format | the builder rules `std-format.debug.builder.*` |
| ops | `std-ops.default.std.*` |

### `lib/std` That Disagrees With The Spec

| Finding | Where |
| --- | --- |
| `i32` where the spec says `usize` | throughout; tracked by SIZES-UNSIGNED, not re-listed |
| `Console` lacks `write_error_line!` | `lib/std/console.hd:12` vs [Console](../spec/lang/10-modules.md#console) |
| `ConsoleError` lacks `Debug` | `lib/std/console.hd:55` vs [`std-format.debug.std-types`](../spec/std/format.md#r-std-format.debug.std-types) |
| `Captures` lacks `Debug` | `lib/std/regex.hd:42`, same rule |
| `Choices.pick([])` indexes `items[0]` | `lib/std/testing.hd:131`; the spec states no empty-list rule |
| STDLIB_CALLS regex row says `JsonError` takes byte offsets | false: [`std-json.error.position`](../spec/std/json.md#r-std-json.error.position) counts characters |

### Tests In `test/std`

`test/std` has no file for `json`, `regex`, `cli`, `random`, `encoding`,
`digest`, `fs`, or `task`. Conformance fixtures cover them, and several of
those fixtures are known failures (STD-CLI, STD-LOADER, FLOAT-PARSE).

## Footprint

Measured with the scratchpad `footprint.mts` method on `compileToWat`, one
program per import, each printing one value. WAT bytes and functions are
emitted code; the module list is the use graph that the checker joins.

Every program joins 16 modules through the prelude: `annotation`, `cmp`,
`convert`, `format`, `function`, `hash`, `collections`, `console`, `iter`,
`num`, `ops`, `option`, `result`, `task`, `text`, `prelude`.

| Program | WAT bytes | Functions | Modules added | Cold compile |
| --- | --- | --- | --- | --- |
| scratchpad `tiny` | 15,156 | 22 | none | 187 ms |
| `println("x")` | 14,131 | 21 | none | |
| `println(3)` | 24,114 | 28 | none | |
| `println(debug([1, 2]))` | 34,805 | 40 | none | |
| `[2, 1].sorted()` | 45,870 | 40 | none | |
| `use std.cmp.max` | 29,063 | 32 | none | |
| `use std.text.join` | 23,293 | 27 | none | |
| `use std.collections.counts` | 54,556 | 57 | none | |
| `use std.num.parse_i32` | 52,844 | 48 | none | |
| `use std.hash.hash_of` | 41,287 | 51 | none | |
| `use std.time` (`Duration` display) | 38,634 | 34 | `time` | 209 ms |
| `use std.random.Rng` | 20,404 | 34 | `random` | |
| `use std.encoding.hex_encode` | 40,784 | 37 | `encoding` | |
| `use std.digest.sha256_hex` | 134,980 | 82 | `encoding`, `digest` | 216 ms |
| `use std.json.parse` | 175,366 | 91 | `json` | 221 ms |
| `use std.error`, `host`, `path`, `fs`, `process`, `task` (import only) | 14,131 to 14,357 | 21 | that module (`fs` adds `path`) | |
| any test code (`use std.testing.assert`) | n/a | | `testing`, `testing.arbitrary`, `process`, `random`, `time`, `prelude.testing` | 234 ms |
| `use std.cli`, `use std.regex` | fails: not on the loader list | | | |

Findings:

- Joins are cheap: an unused import adds no emitted code, and the slowest
  cold compile is 47 ms over `tiny`.
- `std.json` is the most expensive import: `parse("1")` emits 175 KB.
  Two functions are 22 KB and 17 KB. Worth a look: whether `parse`
  reaches the float path and `Display` for every number when the text
  has none.
- `sha256` is one 50 KB function: every `wrapping_add` and index is
  inlined with checks. A codegen matter, not a std one.
- Test code joins `std.process` (for `hd_run!`), `std.time` (timeouts), and
  `std.random`. **Recommendation:** move `hd_run!` and `RunOutput` to
  `std.testing.run` or `std.process`, so a test that never runs a program
  does not join `std.process`.
- `std.task` is prelude-joined, which is why `retry_with!` (needs
  `std.time`) is held. The measured join cost above suggests the hold
  costs more than it saves.

## Capability Findings

| # | Finding | Recommendation |
| --- | --- | --- |
| 1 | Every host-touching public function in `lib/std` declares its row: `println`, `args`, `env`, `rng`, `Cli.parse_args`, `hd_run!`, `case_timeout`. The missing helpers (`eprintln`, `read_line!`, `now`, `sleep!`, `read_text!`, `write_text!`) will need theirs. | none |
| 2 | `snapshot_file` reaches the runner through `TestRunner.snapshot_check`, so `lib/std` no longer declares the `snapshot_file_check` primitive, which was outside the [primitives table](../spec/std/README.md#standard-library-primitives). | none |
| 3 | `panic`, `char_scalar`, and `string_from_bytes` are declared in several modules (`ops`, `text`, `collections`, `testing`; `format`, `text`, `hash`). Each is a table primitive; the copies are a loader limitation. | none now; one declaration once modules can share private items |
| 4 | Test providers: `Console`, `Args`, `Env`, `FsRead`/`FsWrite`, `Clock`, `Random` have one. `ConsoleInput` has none. `ScriptedProcess` has no public constructor, so code outside `std` cannot build one. `TestRunner` and `PropertyRunner` have none, which is fine for runner hooks. | `ScriptedInput::new(lines)`; `ScriptedProcess::new(outputs)` |

## Questions For The Owner

### Q1. One Unit For Error Positions

Effect: `text.slice(0, error.position)` cuts at the wrong place after a
non-ASCII character for JSON, hex, base64, time, and number errors, but
not for UTF-8 and regex errors. The confirmed pass-73 row fixed
`DecodeError` in characters, so this needs you.

1. Byte offsets everywhere, as `slice`, `char_indices`, `Utf8Error`, and
   regex already use. **Recommendation.**
2. Characters everywhere; regex and `Utf8Error` change.
3. Keep both, and say per type which unit it uses.

Example: `parse("[\"é\", x]")` reports `UnexpectedCharacter(6)` today, and
would report `7` under answer 1.

### Q2. One Spelling For Milliseconds

Effect: `Duration::milliseconds(n)` and `as_milliseconds()` come from your
L18 decision; agents later added `Timestamp::from_unix_millis`,
`unix_millis()`, and `Instant::from_millis`. A writer must remember which
type takes which word.

1. Rename the agent-made names to L18's word:
   `Timestamp::from_unix_milliseconds`, `unix_milliseconds()`,
   `Instant::from_milliseconds`, `as_milliseconds()`. **Recommendation.**
2. Rename `Duration` to Rust's `from_millis` and `as_millis`.
3. Keep both.

Example: `Timestamp::from_unix_milliseconds(0) + Duration::milliseconds(5)`.

### Q3. `Process.run!` Failure

Effect: a host that cannot start a program, for lack of permission or a
missing executable bit, can only return `.None`, the answer for "no such
program". The caller gets no message. `Process` is language tier.

1. `run!` returns `Result[ProcessOutput, ProcessError]`, with
   `NotFound(program)` and `Other(message)`, as `FsError` does.
   **Recommendation.**
2. Keep `ProcessOutput?`.

Example: `match $.use(Process).run!("git", ["status"], ""):` then
`.Err(ProcessError.NotFound(name)) => ...`.

### Q4. Unsigned Scalar Primitives

Effect: `char_scalar` returns `i32` and `char_from_scalar` takes one, so
std code converts at each use, and `to_digit` already returns `u32`.
Changing a primitive's signature needs your approval.

1. `char_scalar(c: char) -> u32`, `char_from_scalar(point: u32) -> char`.
   **Recommendation.**
2. Keep `i32`.

Example: `lib/std/hash.hd` writes `u32(char_scalar(self))` to undo the sign.
