# Standard Library Calls

Every standard-library design choice that the owner did not decide
explicitly is recorded here, so the stdlib audit (task #239) can review
them in one place.

Since 2026-10-03 the owner has delegated stdlib API design. The agents
writing std make these calls and record them here. Language-tier changes,
intrinsics, and host hooks still go to the owner.

Each row lists:

- **Pass:** the spec pass or task that made the call. `git log --grep` on
  its task number finds the commit.
- **Call:** the choice made.
- **Why:** a short reason, usually the precedent followed.
- **Status:**
  - `own`: an agent's call, never reviewed;
  - `confirmed`: the owner later approved it;
  - `changed`: the owner later replaced it. The row then says with what.

Append a row for every new call, newest last, and change a row's status
when the owner reviews it.

## Hashing And Encoding

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 65 (#205) | A `string` hashes as a u64 little-endian byte length, then its UTF-8 bytes, rather than Rust's `0xff` terminator. | One length-first rule covers strings and lists, and `("ab","c")` hashes differently from `("a","bc")`. | own |
| 65 (#205) | A tuple hashes its elements only, with no length, since the arity is fixed. | The type fixes the arity. | own |
| 65 (#205) | `T?` gained a `Hash` impl: a tag byte, then the payload. | It is needed by the optional byte rule. | own |
| 73 (#224) | Hex encodes lowercase and decodes either case. Base64 decoding is strict: canonical padding, zero pad bits, no whitespace. | RFC 4648; Go's `Strict` mode. | confirmed |
| 73 (#224) | `DecodeError` is `InvalidCharacter`, `InvalidLength`, or `InvalidPadding`, each with a character position. A length error comes first and reports the text length. | The Rust `hex` crate checks length first. | changed: the owner's POSITION-UNIT decision (2026-10-03, audit Q1) makes every std error position a byte offset, so lengths and positions count bytes (pass 89) |
| 73 (#224) | `sha256_hex(bytes)` is a one-line helper over `hex_encode(sha256(bytes))`. | A common need. | confirmed |

## Numbers

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 68 (#218) | `unwrap_or`, `map_err`, `checked_*`, `wrapping_*` and `is_nan` were specified because existing spec rules cite them by name. | Those rules need targets. | own |
| 68 (#218) | Integer parsing reads left to right and the first error wins, so `"99999999999x"` is `OutOfRange`. | Rust and the existing lib/std behave the same way. | own |
| 69 (#219) | `abs_diff` has one impl per width; the other helpers have one impl generic over `Integer`. | `Integer` cannot name the matching unsigned type. | own |
| 76 (#228) | `parse_f64` checks the grammar in hd before calling the hook. | Error positions then come from std. | confirmed |
| 77 (#229) | `to_digit(radix)` panics for a radix outside 2 to 36 and reads ASCII digits and letters only. | Rust semantics. | own |
| 78 (#234) | `to_fixed` uses `format_f64_fixed`, which rounds ties to even, keeps the sign of `-0.0`, and allows 0 to 100 digits. | Exact rounding; JavaScript's `toFixed` rounds ties away from zero. | own (the hook is owner-approved) |
| 80 (#236) | The format hooks write `NaN`, `inf`, `-inf`, and the `-` of `-0.0`. Parsing reads the sign and the special words in hd. | `Display` text comes from one place. | own |

## Text

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 77 (#229) | `char` classification uses Unicode 17.0 tables stored as sorted ranges and binary-searched. | It is compact, and the spec named no Unicode version before. | own |
| 77 (#229) | `Utf8Error` is `InvalidSequence(position)` or `Truncated`. Display gives "invalid UTF-8 sequence at byte N" or "incomplete UTF-8 sequence at the end of the bytes". | Rust's `Utf8Error`, simplified. | own; audit #239 says revisit (`Truncated(position)`), not yet applied |
| 78 (#234) | `count("")` gives the number of chars plus one. | Python's `str.count`. | own |
| 78 (#234) | `split_once` with an empty separator matches at index 0. | Rust's `split_once`. | own |
| 78 (#234) | Pad widths count chars (scalar values). | Width means text length. | confirmed |

## Collections And Iterators

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 74 (#225) | `Deque` is a ring buffer over `List[T?]` that doubles when full. `get(i)` never counts from the end. A push or pop during iteration panics with `iterator-invalidated`. | Rust's `VecDeque`; the `List` invalidation rules. | confirmed |
| 74 (#225) | `Heap` is a binary max-heap. `to_sorted_list()` returns an ascending copy and leaves the heap unchanged. `Heap` has no `Eq` and no iteration. | Rust's `BinaryHeap`. | confirmed |
| 74 (#225) | `std.cmp.Reverse` gained its spec. | It is needed for a min-heap. | own |
| 76 (#228) | `List.push` forwards to the prototype's builtin `append` until the compiler renames it. | A prototype limit. | own |
| 76 (#228) | `all_list!` is built from nested `all!` calls, with no intrinsic. | Library first. | own |
| 78 (#234) | `partition` returns `(kept, rest)`. `windows(size)` panics for a size below 1 and gives `[]` for a shorter list. | Rust's order; Rust's `windows`. | own |
| 78 (#234) | `group_by` and `counts` keep first-seen key order. | Map insertion order. | confirmed |
| 78 (#234) | `take_while` consumes the first rejected item. `zip` pulls from `self` first. | Rust semantics. | own |
| 78 (#234) | `Set.insert` and `Set.remove` return whether the set changed. `Set` iterates in insertion order, and its `Eq` ignores order. | Rust's `HashSet` API and Map ordering. | confirmed |
| 81 (#237) | `zip` and `chain` call `other.iter()` once, when they are called, and read `other` only through that iterator. | Rust's `zip` and `chain` call `into_iter` at the call. | own |
| 81 (#237) | An `Iterator` argument to `zip` or `chain` stays an error. The spec notes two workarounds: collect it first, or make it the receiver. | `Iterator` does not implement `Iterable` (`flow.for.iterator-not-iterable`). | own |
| 81 (#237) | `std.cmp.max(a, b)` returns `a` on a tie, so it also gives the first of equal values. It was `b`, Rust's choice, and is not in the spec. | One first-among-equals rule for every `min` and `max`, as in Python. | own; specified in pass 89 ([Min And Max](../../spec/std/cmp.md#min-and-max)) |
| 81 (#237) | The unspecified `List.zip` in `lib/std` keeps its `List[U]` argument. | The decision covers the specified `Iterator` adapters only. | changed: pass 89 specifies `List.zip(other: List[U])` as an eager method, beside `List.map` ([List Access And Building](../../spec/std/collections.md#list-access-and-building)) |

## Time

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 70 (#220) | Every test provider is built with `X::new(...)` and has private fields. | `BufferConsole::new` and `ScriptedProcess::new` already work this way. | own |
| 70 (#220) | `ManualClock` has no `advance`; a test calls `sleep!` on it. `monotonic` counts from the Unix epoch. | Keeps the surface minimal. | own |
| 70 (#220) | `MemoryFs` keeps an explicit `dirs` list next to `files`. | `create_dir_all!` then `list_dir!` needs directories that outlive their files. | own |
| 71 (#221) | `Duration`'s `Add` and `Sub` panic with `integer-overflow` on overflow. `Timestamp + Duration` is defined; `Timestamp - Timestamp` is not, because `since` covers it. | It matches i64 arithmetic. | confirmed |
| 72 (#223) | Go-style `Duration` text writes every smaller unit down to `s` once a larger unit appears (`1h0min0s`), uses `0s` for zero, and never overflows on the most negative value. | Go's `Duration.String`. | own (the style is owner-approved) |
| 74 (#225) | `Date` holds `year`, `month` and `day` as `i32` in the proleptic Gregorian calendar, where year 0 is 1 BC. It implements `Eq` and `Debug`, but not `Display` or `Ord`. | Hinnant's algorithm. | confirmed |
| 74 (#225) | RFC 3339 output is `YYYY-MM-DDThh:mm:ssZ` with `.fff` only when the milliseconds are nonzero. Years outside 0 to 9999 print as Go's `Format` does. Parsing accepts `t`, `z`, `±hh:mm` offsets and any number of fraction digits (truncated), and rejects second 60. | Go's `time` package. | confirmed |
| AW (Muse queue) | `Timestamp` serializes as its RFC 3339 `Display` text and deserializes through `parse_rfc3339`, reporting a bad string with `invalid("Timestamp")`. | Owner decision in job AW; the text is what `to_rfc3339` already gives. | own |
| AW (Muse queue) | `Debug` for `Duration` writes `to_string()`, for `Timestamp` writes `to_rfc3339()`, and for `Instant` writes `Instant(…ms)` via `out.write`. | Owner decision in job AW; `write` is the custom-text builder other std types use. | own |

## Errors

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 71 (#221) | `ErrorReport` implements `Display` as the message, then one `caused by: X` line per cause. `chain` stops at the first `.None`. | It matches the Entry Results text. | own |
| 72 (#223) | `ContextError` displays as its message, and its `cause` is the wrapped error. | anyhow's `context`. | confirmed (fields stay private, revisit later) |
| 77 (#229) | `ContextError` Debug is `ContextError { message: "...", cause: "<cause Display>" }`. | `debug_struct` style. | own |
| 81 (#237) | `lib/std/console.hd` declares `ConsoleError`, and its `Display` text for `Closed` is `console closed`. | The spec leaves the text open. Declaring it in std costs nothing in the footprint. | own; pass 89 adds `Eq` and `Debug` and specifies the text ([Console Errors](../../spec/std/console.md#console-errors)) |

## JSON

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 75 (#226) | Object equality ignores key order. Nesting is limited to 128 levels. A duplicate key keeps its first position and takes the last value. | serde_json. | confirmed |
| 75 (#226) | `at(index)` with a negative index gives `.None`. `pretty` writes empty containers as `[]` and `{}`, with no trailing newline. | Keeps output and lookup predictable. | changed: pass 82 made `index` a `usize`, so no index is negative; the rest stands |
| 84 (#241) | Enums are externally tagged: `{"Variant":{...members}}`, and a variant with no member is the string `"Variant"`. Positional members are keyed `_0`, `_1`. A bare `"Variant"` decodes as `{"Variant":{}}`. | serde's default form; no tag key can clash with a member. One payload shape, an object, keeps the rules few. | own |
| 84 (#241) | Decoding ignores unknown keys. | serde's default; a reader accepts a newer writer's output. | own |
| 84 (#241) | A missing key decodes as `null`, so only a `T?` (giving `.None`) or a `Json` member may be missing; anything else is `MissingField`. A declared member default is not used. `.None` encodes as `null`, with its key kept. | serde's `Option` handling, with no extra trait method. Defaults per field wait for typed member facts. | own |
| 84 (#241) | Only `Map[string, V]` implements `ToJson` and `FromJson` (now `Serialize` and `Deserialize`, #320). A map with other keys does not, so the program converts its keys. | A JSON key is a string; the program chooses the text. | own |
| 84 (#241) | `JsonError` gains `WrongType(path, expected)`, `MissingField(path)`, and `UnknownVariant(path, name)`. The path is a string such as `$.users[2].name`, with keys unescaped. `expected` is the scalar's type name, `"array"`, `"object"`, or `"variant"`. `std-json.error.enum.usize` is retired for `std-json.error.variants`. | One error type for `decode`; `serde_path_to_error`'s path form. A string path is cheap to compare and print. | own |
| 84 (#241) | Integers encode exactly and decode only from an integer in range; `1.0` is not an integer. Floats decode from any number; `f32` takes the nearest value, and past its range is `WrongType`. A NaN or an infinity encodes as `null`. | serde_json. | own |
| 84 (#241) | A `char` is a one-character string. An embedded member is one nested key, not flattened. Shared constructor data is not written. | serde's `char`; flattening is opt-in in serde. | own |
| 84 (#241) | No tuple template for `ToJson` or `FromJson` yet (now `Serialize` and `Deserialize`, #320). | Not asked for; it can follow as a tuple template. | own |

## Testing

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 67 (#217) | A discard is a panic with the message `std.testing: case discarded`, and the runner counts it only before `show`. | A `discard` method is no longer needed. | own |
| 67 (#217) | After the `replay` draws run out, each draw is fresh from the seed. | The prototype returned 0 there. | own |

## Sizes

Pass 82 (#238) applied the owner's batch 76 decision that every size is a
`usize`. These rows record the std choices that the decision left open.

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 82 (#238) | Every std length, count, index, position, and width is `usize`: `len` of `Set`, `Deque`, `Heap`, and `ListView`; `Deque.get`; `ListView`'s `Index[usize]`; `view`, `chunks`, `windows`, `index_of`, and `counts` values; `take`, `skip`, `enumerate`, and `count`; `repeat`, `pad_start`, `pad_end`, and `string.count`; `to_fixed` digits; `Json.at`; and the `position` of `ParseNumberError`, `DecodeError`, `JsonError`, `TimeParseError`, and `Utf8Error`. | The decision's list, read as every size-like value. | own |
| 82 (#238) | The negative-count rules of `take`, `skip`, `repeat`, and `view` are retired, with their panic fixtures; a negated literal is now `unsigned-negation`. `chunks` and `windows` still panic for a size below 1, and `to_fixed` only for digits above 100. | A `usize` cannot be negative; 0 is still a bad piece size. | own |
| 82 (#238) | Retired std IDs got new names: `std-collections.counts.decl`, `std-iter.adapter.take-first`, `.enumerate-usize`, `.skip-first`, `.count-remaining`, `std-text.pad.width-at-most-length`, `std-text.utf8.error.declared`, `std-num.to-fixed.decl`, `.digits-max`, `std-encoding.error.declared`, `std-json.value.accessors.at-index`, `std-json.error.enum.usize`, `std-time.parse-error.declared`, and `std-testing.choices.list-max`, `.map-max`, `.string-max-chars`. | Each old rule named `i32` or a negative value. | own |
| 82 (#238) | `Choices.list` and `Choices.map` take a `usize` `max`, and `Choices.string` a `usize` `max_chars`. `PropertyCase.size` stays `i32`, since it is a reach exponent, not a size. | They bound a generated collection's length. | own |
| 82 (#238) | `TestRunner.row` takes and returns `usize`, `PropertyRunner.start` takes `usize` counts, and `PropertyCase.example` is `usize?`. `Random.fill` takes a `usize` count. | A row is a list index; the rest are counts. These are host traits, so the report asks the owner. | own |
| 82 (#238) | The primitives `bytes_len`, `bytes_at`, `bytes_slice`, and `format_f64_fixed` use `usize`. `list_version`, `char_scalar`, and `char_from_scalar` stay `i32`, since a version and a scalar value are not sizes. | The primitives serve `len`, indexing, `slice`, and `to_fixed`. | changed: the owner's CHAR-SCALAR-U32 decision (2026-10-03, audit Q4) makes them `char_scalar(c) -> u32` and `char_from_scalar(u32) -> char?` (pass 89) |
| 82 (#238) | `lib/std` and `test/std` keep `i32` until the compiler session adds `usize` and an unsigned `len`, as the former SIZES-UNSIGNED row of the stdlib plan listed. | `lib/std` cannot name `usize` before the compiler declares it. | own; done: the compiler session declared `usize` and moved `lib/std` and `test/std` to it |


## Random Numbers

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 85 (#242) | `Rng` holds its own state and fixes its generator: xoshiro128** 1.0, seeded by folding the `u64` seed to 32 bits and four SplitMix32-style `mix32` steps, as `lib/std`'s `SeededRandom` already does. The spec gives the reference code and test vectors. `SeededRandom` stays unfixed. | Seeded output, such as a level or a fixture's sample, must repeat across implementations. 32-bit steps need no wide multiplication, which the plan's PCG sketch did. | own |
| 85 (#242) | `rng()` seeds an `Rng` from one `next_u64` of `$ Random`. Every `Rng` method is a plain call with the empty row. | Go's `math/rand/v2`: one host draw per generator, and a seeded generator needs no provider. | own |
| 85 (#242) | The surface is `from_seed`, `next_u64`, `int`, `float`, `bool`, `choose`, `shuffle`, and `sample`, named as `Choices` names its draws. `sample`'s count is a `usize`. | The plan's sketch, plus `bool` and `sample` from the task. | changed: pass 89 renames `Rng::from_seed` to `Rng::new` and `Choices.pick` to `Choices.choose`, so both draw one item with `choose` |
| 85 (#242) | `int` takes one `Range[i64]`, either `a..b` or `a..=b`, and is not generic over `Integer`. | Go's `Int64N`. A generic version needs a per-width conversion that `Integer` cannot name; it can follow. | own |
| 85 (#242) | `int` is unbiased by rejection: `below(n)` redraws a value under `2^64 mod n` and returns `x mod n`. The whole `i64` range is one raw draw. An empty range panics with `explicit-panic`. | OpenBSD's `arc4random_uniform`; Rust's `gen_range` panics on an empty range. | own |
| 85 (#242) | `float` is the top 53 bits of one draw times 2^-53, in [0, 1). `bool` is the top bit of one draw. | The standard exact construction, as in Rust's `rand`. | own |
| 85 (#242) | `choose` on an empty list returns `.None` and draws nothing. | Rust's `choose`. | own |
| 85 (#242) | `shuffle` is Fisher–Yates from the back. `sample(items, count)` runs `count` Fisher–Yates steps from the front on a copy and returns the first `count` items, in draw order. A count above the length panics with `explicit-panic`. | Rust's `partial_shuffle`; Python's `random.sample` rejects a count past the length, and owner Q18 panics on bad counts. | own |

## Command-Line Parsing

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 85 (#242) | `std.cli` is a builder, `Cli::new(program).flag(...).option(...).positional(...)`, not a typed `FromArgs` derivation. | Derivation needs no protocol change, but it needs fact types for short names and positionals, and both `describe` and `build`. The plan defers typed CLI structs; a `FromArgs` template can later sit on the same parser. Go's `flag` and Node's `parseArgs` are tables too. | own |
| 85 (#242) | Each builder method takes `self` and returns a new `Cli`. A long, short, or positional name declared twice panics with `explicit-panic`. | Value semantics; Go's `flag` panics on a redefined flag. | own |
| 85 (#242) | The forms are `--name`, `-c`, `--name VALUE`, `--name=VALUE`, and `-c VALUE`. No short clusters (`-vq`), no attached short values (`-ofile`, `-o=x`). An option's value is the next argument, whatever its text. | Go's `flag` package. | own |
| 85 (#242) | Flags, options, and positionals interleave. `--` ends the options and is dropped; a lone `-` is a positional. | Node's `parseArgs` and `clap`; the POSIX `--` and `-` conventions. | own |
| 85 (#242) | A repeated flag is set once; a repeated option keeps its last value. | Go's `flag` and Node's `parseArgs`. | own |
| 85 (#242) | Every declared positional is required and filled in order. Extra positionals are kept, in `Parsed.positionals`. | One error kind, and `cmd FILE...` still works. | own |
| 85 (#242) | `CliError` is `UnknownOption(text)`, `MissingValue(name)`, `UnexpectedValue(name)`, and `MissingPositional(name)`, with `Eq`, `Debug`, and `Display`. The first error in argument order wins; missing positionals are checked last. | `UnexpectedValue` covers `--flag=x`, as Node's `ERR_PARSE_ARGS_INVALID_OPTION_VALUE` does. | own |
| 85 (#242) | The `Display` texts are `unknown option TEXT`, `option --NAME needs a value`, `flag --NAME takes no value`, and `missing argument <NAME>`. `UnknownOption` keeps the argument as written, up to its first `=`. | Short and greppable, like Go's `flag provided but not defined`. | own |
| 85 (#242) | No automatic `--help`. A declared flag named `help` that is set skips the missing-positional check. | `tool --help` must reach the program, as `argparse` and `clap` allow, without hidden flags. | own |
| 85 (#242) | `Parsed` has public `flags`, `values`, and `positionals`, plus `flag(name)` and `value(name)`. An undeclared name reads as absent. `Parsed` implements `Eq` and `Debug`. | The plan's sketch. | own |
| 85 (#242) | `usage` writes a `usage:` line, then `arguments:` and `options:` sections with one help column. A term is `-c, --name`, or four spaces and `--name`, plus ` <value>` for an option. No final newline. | Go's `flag` and `hd help` layouts; the four spaces align the long names. | own |
| 85 (#242) | `parse_args` is a `Cli` method with `$ Args`, beside the pure `parse(arguments)`. | The task asks `std.cli` to read `$ Args`; tests use `parse` or `MapArgs`. | own |

## Regular Expressions

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 86 (#243) | Part 1 of `std.regex` has no flags: `(?i)`, `(?m)`, `(?s)`, and every other `(?` but `(?:` is `UnsupportedGroup`. | The task allowed leaving flags out; an error today keeps a later flag from changing a pattern's meaning. | own |
| 86 (#243) | The matcher steps by `char`. `\d` and `\w` are ASCII; `\s` is Unicode White_Space, the property `char.is_whitespace` and `trim` use. `.` matches any `char` but `\n`. | RE2's defaults; RE2's own `\s` is ASCII, so this `\s` follows the task and hd's `trim`. | own |
| 86 (#243) | `Match.start`, `Match.end`, and `RegexError.position` are byte offsets (`usize`), as `string.slice`, `char_indices`, and `Utf8Error` take them. | One offset unit with `slice`; Go and Rust report byte offsets. | confirmed: the owner's POSITION-UNIT decision (2026-10-03) makes every std position a byte offset. Pass 89 corrected the old reason, which said `JsonError` took bytes when it counted characters |
| 86 (#243) | `RegexError` is a data type with `kind: RegexErrorKind` and `position`; the nine kinds are `MissingParen`, `UnmatchedParen`, `MissingBracket`, `BadEscape`, `BadRange`, `NothingToRepeat`, `BadRepeat`, `UnsupportedGroup`, and `TooLarge`, with Go's `regexp/syntax` texts and `KIND at byte N`. | The task asks for a kind and a position; a kind enum keeps the position in one field. | own |
| 86 (#243) | A quantifier right after another (`a**`, `a{2}{3}`, `a*??`) is `NothingToRepeat`, not a kind of its own. | Go rejects these as a nested repetition; one kind is enough for a script. | own |
| 86 (#243) | A `{` that starts no well-formed count is a literal, and so are `}` and `]` outside a class. A `]` first in a class, and a `-` first, last, or after a range, are items. | RE2 and Go. | own |
| 86 (#243) | Escapes are the six class escapes, `\n`, `\t`, `\r`, and escaped ASCII punctuation. Any other, such as `\b` or `\1`, is `BadEscape`. | Go's `regexp` rule for punctuation; leaving out `\b` keeps part 1 small. | own |
| 86 (#243) | A count is at most 1000, and a pattern's written-out size at most 10,000 instructions (`TooLarge`, position 0). The spec defines the size by a table. | RE2 caps counts at 1000; a size cap keeps O(m × n) meaningful. A precise table keeps the limit the same in every implementation. | own |
| 86 (#243) | `^` and `$` see only the ends of the text; `$` does not match before a final `\n`. | RE2 without `(?m)`. | own |
| 86 (#243) | The matcher is a Pike VM in `lib/std/regex.hd`: one thread per instruction per position, kept in priority order, with an explicit stack for epsilon steps. `is_match` is `find(...).is_some()`. | Leftmost-first submatch order needs the Pike VM's priorities; a stack keeps deep programs off the call stack. | own |
| 86 (#243) | `Regex` has `as_str()` and `Debug`; `Match` has `Eq` and `Debug`; `RegexErrorKind` and `RegexError` have `Eq`, `Debug`, and `Display`. | What fixtures and scripts compare and print. | changed: pass 89 renames `as_str()` to `pattern()`, since hd has no `str` type and `Json` says `as_text` |
| 87 (#244) | Flags stay out: `(?i)`, `(?m)`, `(?s)`, and `(?i:...)` remain `UnsupportedGroup`. | `(?i)` needs a case-folding choice (ASCII or Unicode simple folding) that `std.text` has not made, and scoped flags add syntax; an error today keeps a later flag from changing a pattern. | own |
| 87 (#244) | Captures follow RE2's Pike VM: each thread carries its group positions, the first thread to reach an instruction wins, a repeated group reports its last pass, and a group from an earlier iteration keeps its positions. | RE2, Go, and Rust report these submatches; one VM serves `find` and `captures`. | own |
| 87 (#244) | An optional iteration of `*`, `+`, or `{m,}` that would match the empty text is not taken, so `(a*)*` on `"b"` leaves group 1 out. | What the Pike VM's one-thread-per-instruction rule gives; the spec states it so another implementation reports the same groups. | own |
| 87 (#244) | `Captures` has private fields and `len()`, `get(index) -> Match?`, and `name(name) -> Match?`. Out of range, an unknown name, and a group that took no part all give `.None`. | Rust's `Captures` shape, whose `get` and `name` also return an option. | own |
| 87 (#244) | Both `(?P<name>...)` and `(?<name>...)` name a group. A name is an ASCII letter or `_`, then ASCII letters, digits, and `_`. `(?<=` and `(?<!` stay `UnsupportedGroup`. | RE2, Go 1.22, and Rust accept both spellings; no digit first keeps `${1}` a number. | own |
| 87 (#244) | Two new kinds: `BadGroupName` (`invalid named capture`) and `DuplicateGroupName` (`duplicate capture group name`), each at the group's `(`. | Go's text for a bad name, Rust's for a duplicate; a duplicate is a different fix from a bad name. | own |
| 87 (#244) | `find_all` steps one character past an empty match and drops an empty match that starts where the previous match ended, so `a*` on `"baaac"` gives `""`, `"aaa"`, `""`. | Go's and Rust's rule. Python's 3.7 rule would add a second `""` at offset 4. | own |
| 87 (#244) | `captures_all` is added beside `captures`, giving the groups of each `find_all` match. | `replace_all` needs that loop anyway; Rust has `captures_iter` and Go `FindAllStringSubmatch`. | own |
| 87 (#244) | The replacement syntax is `$N` (the longest run of digits), `${N}`, `${name}`, and `$$`. A bare `$name` is not a reference. A missing or absent group gives `""`, and a `$` that starts no reference is itself. | Go's and Rust's syntax without bare names, which an hd string would interpolate; so `$1a` is group 1 then `a`. Neither form fails, as in Go and Rust. | own |
| 87 (#244) | The spec shows the string idiom: `$1` and `$$` need no escape in an hd string, `${name}` is written `\${name}`, and a raw string does not help, since `r"\${x}"` keeps the backslash. Patterns are written as raw strings, `Regex::new(r"^\d+$")`. | `lex.interp.dollar-text` and `lex.prefix.backslash`. | own |
| 87 (#244) | `split` returns k + 1 pieces for k matches, with `""` for a match at either end; `""` with no match gives `[""]`. | Rust's `Regex::split`; Go's `Split` drops some empty pieces by special cases. | own |
| 87 (#244) | The time bound with groups is O(m × g × n) time and O(m × g + n) memory, g the number of groups plus one. `find_all` and its kin run at most 2k + 1 searches, so O(m × n²) at worst. | Each thread copies g slot pairs; Rust documents the same worst case for its iterators. | own |

## Audit Pass 89

Pass 89 (#246) applied the stdlib audit (task #239); its open gaps are in [Open Issues](OPEN_ISSUES.md#library-and-host-design-work).

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 89 (#246) | `Timestamp::from_unix_milliseconds`, `unix_milliseconds()`, and `Instant::from_milliseconds` replace the `millis` names. `Instant` gains `as_milliseconds()`. | Owner decision L18 named `Duration::milliseconds` and `as_milliseconds`; one word for one unit. | own |
| 89 (#246) | `Timestamp` implements `Display` as `to_rfc3339()`. `Instant` and `Date` have no `Display`. | Go's `Time.String`; an instant's origin is the provider's, so its number means nothing alone. | own |
| 89 (#246) | `Ordering` implements `Eq`, specified with its `Debug`; it has no `Display`. | Owner Q21: std value types implement `Eq`. Rust's `Ordering` is `Eq` and not `Display`. | own |
| 89 (#246) | Every std error type implements `Error`: `ParseNumberError`, `Utf8Error`, `ConsoleError`, `DecodeError`, `FsError`, `JsonError`, `TimeParseError`, `CliError`, `RegexError`, and `ResourceError[E]` when `E < Error`. The prelude modules' impls live in `lib/std/error.hd`, so a program that never names `Error` does not join it. | `?` into `Result[T, Error]` and `context` must accept std errors, as Rust's `io::Error` does. | own |
| 89 (#246) | `ResourceError[E]` displays `Operation(e)` as `e` and `Disposed` as `resource disposed`, with no cause. | A wrapper that shows its error's text must not also list it as a cause, or a report prints it twice. | own |
| 89 (#246) | `ConsoleError` implements `Eq` and `Debug`. | `std-format.debug.std-types` and owner Q21. | own |
| 89 (#246) | `ScriptedInput::new(lines)` is the `ConsoleInput` test provider: lines in order, then `.Ok(.None)` forever. | Owner decision 4: a provider beside each host trait. | own |
| 89 (#246) | `BufferConsole` is specified: `new`, `output()`, and `error_output()`, which keeps `write_error_line!` lines apart. | Go tests capture `Stdout` and `Stderr` apart. | own |
| 89 (#246) | `Console.write_error_line!`, `eprintln`, and `error_output` stay out of `lib/std` (STD-HELPERS). | The prototype emits host glue for every host-trait method, so a second `Console` method grows every printing program by five functions. | changed: `lib/std/console.hd` declares them since task #263, and task #281 closed STD-HELPERS (`eprintln` writes to standard error under the default profile) |
| 89 (#246) | `List` gains `get(index) -> T?`, `is_empty`, `take(count) -> List[T]`, and `extend(other)`; `first`, `last`, `filter`, `reversed`, `sorted_by`, and `zip` are specified. `Map` gains `is_empty`. | Rust's `slice::get` and `Vec::extend`. `take` is Kotlin's `List.take`: eager like `List.map`, so it does not clash with the lazy `Iterator.take`. | own |
| 89 (#246) | `items.extend(items)` appends the elements held before the call, so it doubles the list. | It reads `other`'s length once; Rust forbids the aliasing instead. | own |
| 89 (#246) | `Rng::new(seed)` replaces `Rng::from_seed`, and `Choices.choose` replaces `Choices.pick`. `Rng.choose` returns `T?`; `Choices.choose` returns `T` and panics on `[]`. `Rng.int(range)` and `Choices.int(lo, hi)` keep their shapes. | One name per draw. A generator must yield a value, as Hypothesis's `sampled_from` does; a generic `Range` form waits until `Integer` can name it. | own |
| 89 (#246) | `retry!`'s `times` and `Backoff.attempts` are `usize`, and 0 counts as 1. | Owner rule: every count is `usize`. | own |
| 89 (#246) | `std.cmp.min` and `max` are specified, with `a` returned on a tie. | They were public and unspecified. | own |
| 89 (#246) | `upper`, `trim_start`, `trim_end`, `ends_with`, `contains`, `find`, `is_empty`, `strip_prefix`, `strip_suffix`, `join`, and `StringBuilder` are specified as `lib/std` has them. `find` returns a byte offset. | Rust's `str` names; byte offsets are what `slice` takes. | own |
| 89 (#246) | `interpolate`, `process_escapes`, and `EscapeError` stay public and are specified, not made private. `EscapeError.offset` is renamed `position` and gains `Eq`. | Owner decision L22 moved them to `std.text` as prefix helpers. `position` is every other std error's field name. | own |
| 89 (#246) | `Regex.as_str()` is renamed `pattern()`. | Python's name; hd has no `str` type. | own |
| 89 (#246) | The audit's revisit rows are kept: `Map[string, V]` alone encodes as JSON; a repeated CLI option keeps its last value; regex part 1 has no flags and no `\b`. `Utf8Error.Truncated` gains no position yet. | Each widens a surface that no script has hit; an `append` option kind, ASCII `(?i)`, and `\b` are each a later addition that changes no existing result. `Truncated(position)` changes a variant's shape and its `Display`, which no caller has asked for. | own |
| 89 (#246) | Under the owner's POSITION-UNIT decision, a hex or base64 length check counts bytes, so `hex_decode("é0")` is `InvalidLength(3)`. `JsonError` positions move past each non-ASCII character by its byte width. `ParseNumberError` and `TimeParseError` positions keep their values, since every character before them is ASCII. | Go's `encoding/hex` and `base64` check the byte length; a position must be a `slice` bound. | own |
| 89 (#246) | `lib/std` callers convert `char_scalar`'s `u32` to `i32` where the Unicode tables index by `i32`, and keep the unchecked `char_from_scalar` until the compiler returns `char?` (CHAR-SCALAR). | Every caller passes a checked scalar value. | own |

## Process And Pass 90

Pass 90 (#247) applied batch 80's PROCESS-RESULT and the pass-89
leftovers of the stdlib audit (task #239).

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 90 (#247) | `ProcessError` displays `program not found`, `permission denied`, and the `Other` message as written. Its `Debug` writes the variant name and, for `Other`, the `message` field. | `FsError` keeps the host message the same way; the owner gave `NotFound` no payload, so the caller adds the program name. | own |
| 90 (#247) | `hd_run!` panics on `PermissionDenied` and `Other` too, with the error's text (`std-testing.hd-run.start-failure`). | `hd_run!` returns a bare `RunOutput`, so a failed start can only panic, as a missing name already does. | own |
| 90 (#247) | `ScriptedProcess::new(outputs)` answers a program by name only, ignoring `args` and `stdin`, and an unscripted name is `NotFound`. | The pass-70 provider kept the table lookup; Q14-22 asked only for the constructor. | own |
| 90 (#247) | `ExitCode`, `ProcessOutput`, and `Range`, `RangeFrom`, `RangeTo`, `RangeFull` implement `Eq` field by field, so `0..3 != 0..=2`. | Owner Q21; Rust's derived `PartialEq` on `Range` and `RangeInclusive` compares the fields, not the integers. | own |
| 90 (#247) | `Captures` implements `Debug` with the derived builder calls on its private fields. | `std-format.debug.std-types` already required it. | own |
| 90 (#247) | `Json.pretty()` is a method, and the free `pretty(value)` is removed. | Audit inconsistency 20: text output of a value is a method, as `to_rfc3339()` is; one spelling is enough. | own |
| 90 (#247) | `hd_run!` stays in `std.testing`. Moving it out drops only `std.process` from a test program's joins; `std.time` and `std.random` stay, for timeouts and `Choices`. One-it compile time goes from about 240 ms to about 232 ms, within run-to-run noise, and the emitted WAT is unchanged. | The move would change the `use std.testing.hd_run` path that `hd new --app` writes, for about 3 percent of one test's compile time. | own |

## Console Closed (2026-10-03)

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| orchestrator | Under the default profile, `write_line!` and `write_error_line!` return `.Err(ConsoleError.Closed)` when the host cannot write (a closed pipe); `read_line!` returns it when stdin is not attached or a read fails. End of input is `.Ok(.None)`, repeated on later reads (`cli.host.default-profile.console-closed`, `.input-closed`). | `println` already panics on `.Err`, as Rust's `println!` does on a closed pipe; Rust's `read_line` returns `Ok(0)` at end of input on every call. | own |

## Std Batch (#281)

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 281 | `join` is also a method, `List[string].join(separator)`, written as `impl List[string]` in `std.text`. The free `std.text.join(parts, separator)` stays and the method calls it. | Rust's `[String].join`; two usability probes wrote `.join` and failed. The free function is imported by existing code, and an inherent impl on `List[string]` alone is already legal (`trait.inherent.unique-unifying.disjoint`). | own |
| 281 | `parse_u32`, `parse_u64`, and `parse_usize` share the signed grammar and errors, minus `-`: a leading `-` (even `"-0"` and a lone `-`) is `InvalidDigit(0)`. `parse_usize` is `parse_u32` by name, since `usize` is `u32`. | Rust's unsigned `FromStr` rejects `-` as an invalid digit. | own |
| 281 | `Iterator.sum()` and `List.sum()` exist for `T < Num`: they add with `+` in order from 0, so an integer overflow panics with `integer-overflow` and a float total of `[-0.0]` is `0.0`. A list has its own method. | Rust's `Iterator::sum` and Python's `sum`; a list is not an iterator, and `List` already has `min` and `max`. Rust's float sum starts at `-0.0` in recent releases; hd starts at `0.0` to keep one rule for every `Num`. | own |
| 281 | `Path.join(part: string)` takes a `string`, not a `Path`, and does not clean the result. An absolute `part` replaces the path, and `Path("a").join("")` is `Path("a/")`. | Rust's `Path::join` (no `AsRef` in hd, so a text argument; Go's `filepath.Join` cleans, Rust does not). | own |
| 281 | `Path.parent()` returns `Path?`, `file_name()` and `extension()` return `string?`, and `extension` has no leading dot. The components are Rust's on Unix: empty and inner `.` pieces dropped, a leading `.` kept, `..` kept but never a file name. `/` and `""` have no parent; a dotfile has no extension; `a.` has the extension `""`. | Rust's `Path::{parent, file_name, extension}`; Go's `filepath.Ext` keeps the dot. | own |
| 281 | `file_stem`, `is_absolute`, `with_extension`, and `components()` are not added. | The plan named four operations; the others wait for a use. | own |
| 281 | `rotate_left(count: u32)` and `rotate_right(count: u32)` are generic over `Integer`, take the count modulo the bit width, and rotate a signed value's two's-complement bits. | Rust's `rotate_left` and `rotate_right`; a `u32` count matches shift counts. | own |
| 281 | Property edge values (PROP-EDGE-VALUES): a generated case with `size` below 4 takes the edge its seed picks (seed modulo 4: low, high, zero, none) for every fresh draw. The runner rules gain `size` counting generated cases from 0 and consecutive seeds, so four cases cover all three edges. | An owner-approved behavior; a seed-picked edge keeps a discarded case's retry different, which a size-picked edge would not (a generator that always discards the edge would never leave size 0). | own |
| 281 | The default `i32`, `i64`, `u32`, and `u64` generators draw one extra recorded choice (8: smallest, 9: largest, else a small value from -1000 to 1000 or 0 to 1000), so a replayed stream does not depend on the case's edge. | The type's own bounds must be reachable. Drawing the range with `int(MIN, MAX)` would make ordinary draws huge and make a stream mean different values per edge. | own |
| 281 | Two fixtures that wrote `value + 1 == 1 + value` over a generated `i64` now use `wrapping_add`. | The largest `i64` is now generated within four cases, so the old property found a real overflow. | own |

## Scripted Net (#256)

`Net` has no deterministic provider, so code that opens a socket cannot
be unit-tested ([Net Providers](../../spec/std/net.md#net-providers)).
This call designs one on the model of `ScriptedHttp`. The spec is frozen,
so the entry ends with the rules to add once the owner orders the edit.

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| #256 | `std.net` declares four providers beside `Net`: `ScriptedNet` (lookups and the handles that `connect!`, `listen!`, and `bind_udp!` return), `ScriptedStream` (a `TcpStream`), `ScriptedListener` (a `TcpListener`), and `ScriptedUdp` (a `UdpSocket`). All have private fields and implement `Debug`. None lives in `std.testing`. | `ScriptedHttp` lives in `std.http`, `BufferConsole` in `std.console`, `ScriptedProcess` in `std.process`: a provider sits with its trait. A socket needs its own scripted handle, since `Net` returns handles. | own |
| #256 | The test builds each handle itself and keeps it: `ScriptedStream::new(reads) -> mut ScriptedStream`, `ScriptedListener::new(connections) -> mut ScriptedListener`, `ScriptedUdp::new(incoming) -> mut ScriptedUdp`. `ScriptedNet::new() -> mut ScriptedNet` takes no table; setup methods add to it (next row). Handles are `mut` values, so the copy that the net hands to the code under test is the copy the test holds, as `http` is after `$.with(Http=http)`. | One value per socket lets a test assert on exactly the connection it cares about. A table of every connection in `new` would need the accessor to name a connection. | own |
| #256 | `ScriptedNet` setup: `add_host(host, addresses: List[string])` answers `lookup!`; `add_connection(address, stream)` queues a stream for `connect!`; `fail_connection(address, error: NetError)` queues an error for `connect!`; `add_listener(address, listener)` answers `listen!`; `add_udp(address, socket)` answers `bind_udp!`. An address is `host:port` as in `r[std-net.error.address]`. The connections for one address are served in queue order, one per call. | A queue per address lets a test script "the first connect is refused, the retry works". Errors share the queue so their order is exact. | own |
| #256 | A call with nothing scripted fails at once with the `NetError` that a real network gives: `lookup!` of an unknown host is `.Dns(host)`; `connect!`, `listen!`, or `bind_udp!` of an address with no entry (or an emptied queue) is `.Refused(address)`. | `ScriptedHttp` also fails an unscripted call with an error, never a plausible answer. These two variants already mean "no such name" and "nobody there", so no new variant is needed. | own |
| #256 | A read, accept, or receive past the end of the script returns `.Err(.Operation(.Other(message)))`, and `message` is `ScriptedStream has no more reads`, `ScriptedListener has no more connections`, or `ScriptedUdp has no more datagrams`. It never waits. | A provider that returns at once cannot hang, and a forgotten step shows up as a failure that names it. An empty `read!` result would look like a closed peer. | own |
| #256 | End of stream is scripted, not implied: an empty chunk in `reads` is the peer's close, and every later `read!` on that stream returns the empty list again. | `r[std-net.read]` already makes an empty list the end of the stream. A script that ends without one is a test bug, so it errors (previous row). | own |
| #256 | `ScriptedStream::new(reads: List[List[u8]])` returns the chunks in order, one per `read!`. A `read!(max)` returns at most `max` bytes of the front chunk and keeps the rest at the front. `ScriptedStream::failing(reads, error: NetError)` serves `reads`, then returns `.Err(.Operation(error))` from every later `read!` and `write!`. | The chunk is TCP's unit of arrival and `max` splitting matches `r[std-net.read]`. `failing` covers a reset or timeout in the middle of a reply. | own |
| #256 | A test asserts on a stream with `written() -> List[u8]` (every byte of every `write!`, concatenated in order) and `is_closed() -> bool`. There is no per-call write list and no expected-writes script. | TCP keeps no write boundaries, so a per-call list would make a test depend on how the code chunks its writes. `BufferConsole.output()` is likewise a recording, and the test compares it itself; an expected-writes script would add a mismatch error to design. | own |
| #256 | `ScriptedUdp::new(incoming: List[Datagram])` returns its datagrams in order from `receive!`, cut to `max` bytes. `sent() -> List[Datagram]` returns each `send_to!` as a `Datagram` with the destination's host and port. `is_closed()` is as for a stream. | `Datagram` already carries bytes, host, and port in both directions. A datagram is one unit, so unlike a stream it does keep its boundaries. | own |
| #256 | `ScriptedListener::new(connections: List[mut ScriptedStream])` returns its streams in order from `accept!`. `is_closed()` is as for a stream. | One accept, one scripted peer. | own |
| #256 | `ScriptedNet.requested() -> List[string]` returns the address of every `lookup!`, `connect!`, `listen!`, and `bind_udp!` call in order (the host alone for a lookup), the failed ones included. | The counterpart of `ScriptedHttp.sent()`; it lets a test count retries and check what was dialed. | own |
| #256 | Every scripted handle is a closable handle: after `close`, each operation returns `.Err(.Disposed)`, and `close` itself returns `.Ok(())` once. No scripted operation consults a grant, so `NotGranted` appears only when a test injects it with `fail_connection` or `failing`. | `r[std-net.handles.closable]`; a scripted provider opens nothing, as `r[std-http.scripted.no-host]` says. | own |
| #256 | Net has no timeout parameter, and `ScriptedNet` takes no `Clock`. A timeout test scripts the error with `ScriptedStream::failing(reads, .Other("timed out"))`. A program that races a read against `sleep` with `ManualClock` gets the read first, since a scripted read completes at once. | A read that never completes would be the hang this design rules out. `ManualClock` and `ScriptedNet` bind side by side with `$.with(Net=net, Clock=clock)` and share no state. | own |

The three examples below parse clean with `scripts/hd-conformance.sh
parse`; their `tests:` blocks need the implementation to run.

A line-protocol client:

```text
use std.net.{Net, NetError, ScriptedNet, ScriptedStream}

fn text_of(bytes: List[u8]) -> string:
    match string::from_utf8(bytes):
        .Ok(text) => text
        .Err(_) => ""

fn lookup_value!(key: string) -> Result[string, NetError] $ Net:
    let mut net = $.use(Net)
    let mut stream = net.connect!("cache.local", 6379)?
    match stream.write!("GET ${key}\n".to_utf8()):
        .Ok(_) => pass
        .Err(_) => return .Err(.Other("write failed"))
    reply := match stream.read!(64):
        .Ok(bytes) => text_of(bytes)
        .Err(_) => return .Err(.Other("read failed"))
    _ := stream.close()
    .Ok(reply.trim())

tests:
    use std.testing.assert_equal

    it("sends the command and returns the reply"):
        let mut stream = ScriptedStream::new(["VALUE 42\n".to_utf8()])
        let mut net = ScriptedNet::new()
        net.add_connection("cache.local:6379", stream)
        $.with(Net=net):
            assert_equal(lookup_value!("answer"), .Ok("VALUE 42"), reason="reply")
        assert_equal(stream.written(), "GET answer\n".to_utf8(), reason="the command")
        assert_equal(stream.is_closed(), true, reason="closed")
        assert_equal(net.requested(), ["cache.local:6379"], reason="one connect")

    it("reports a failed read"):
        let mut stream = ScriptedStream::failing([], NetError.Other("connection reset"))
        let mut net = ScriptedNet::new()
        net.add_connection("cache.local:6379", stream)
        $.with(Net=net):
            assert_equal(lookup_value!("answer"), .Err(.Other("read failed")), reason="reset")
```

A tiny echo server:

```text
use std.net.{Net, NetError, ScriptedListener, ScriptedNet, ScriptedStream}

fn serve_one!() -> Result[usize, NetError] $ Net:
    let mut net = $.use(Net)
    let mut listener = net.listen!("0.0.0.0", 7)?
    let mut stream = match listener.accept!():
        .Ok(connection) => connection
        .Err(_) => return .Err(.Other("accept failed"))
    let total: usize = 0
    while true:
        chunk := match stream.read!(1024):
            .Ok(bytes) => bytes
            .Err(_) => return .Err(.Other("read failed"))
        if chunk.is_empty():
            break
        total = total + chunk.len()
        match stream.write!(chunk):
            .Ok(_) => pass
            .Err(_) => return .Err(.Other("write failed"))
    _ := stream.close()
    _ := listener.close()
    .Ok(total)

tests:
    use std.testing.assert_equal

    it("echoes every chunk until the peer closes"):
        let mut peer = ScriptedStream::new(["he".to_utf8(), "llo".to_utf8(), []])
        let mut listener = ScriptedListener::new([peer])
        let mut net = ScriptedNet::new()
        net.add_listener("0.0.0.0:7", listener)
        $.with(Net=net):
            assert_equal(serve_one!(), .Ok(5), reason="five bytes")
        assert_equal(peer.written(), "hello".to_utf8(), reason="echoed")
        assert_equal(peer.is_closed(), true, reason="stream closed")

    it("fails when nobody connects"):
        let mut listener = ScriptedListener::new([])
        let mut net = ScriptedNet::new()
        net.add_listener("0.0.0.0:7", listener)
        $.with(Net=net):
            assert_equal(serve_one!(), .Err(.Other("accept failed")), reason="no connection scripted")
```

A retry loop and a UDP exchange:

```text
use std.net.{Datagram, Net, NetError, ScriptedNet, ScriptedStream, ScriptedUdp}

fn connect_with_retry!(host: string, port: u16, attempts: usize) -> Result[void, NetError] $ Net:
    let mut net = $.use(Net)
    let last = NetError.Refused("${host}:${port}")
    let tried: usize = 0
    while tried < attempts:
        match net.connect!(host, port):
            .Ok(_) => return .Ok(())
            .Err(error) => last = error
        tried = tried + 1
    .Err(last)

fn ping!(host: string, port: u16) -> Result[bool, NetError] $ Net:
    let mut net = $.use(Net)
    let mut socket = net.bind_udp!("0.0.0.0", 0)?
    match socket.send_to!(host, port, "ping".to_utf8()):
        .Ok(_) => pass
        .Err(_) => return .Err(.Other("send failed"))
    match socket.receive!(16):
        .Ok(datagram) => .Ok(datagram.bytes == "pong".to_utf8())
        .Err(_) => .Ok(false)

tests:
    use std.testing.assert_equal

    it("retries after a refused connection"):
        let mut net = ScriptedNet::new()
        net.fail_connection("db.local:5432", NetError.Refused("db.local:5432"))
        net.add_connection("db.local:5432", ScriptedStream::new([]))
        $.with(Net=net):
            assert_equal(connect_with_retry!("db.local", 5432, 3), .Ok(()), reason="second try works")
        assert_equal(net.requested().len(), 2, reason="two connects")

    it("runs out of scripted connections with Refused"):
        let mut net = ScriptedNet::new()
        $.with(Net=net):
            assert_equal(connect_with_retry!("db.local", 5432, 2), .Err(.Refused("db.local:5432")), reason="nothing scripted")

    it("answers a ping with a scripted datagram"):
        let mut socket = ScriptedUdp::new([Datagram { bytes: "pong".to_utf8(), host: "10.0.0.7", port: 9000 }])
        let mut net = ScriptedNet::new()
        net.add_udp("0.0.0.0:0", socket)
        $.with(Net=net):
            assert_equal(ping!("10.0.0.7", 9000), .Ok(true), reason="pong")
        assert_equal(socket.sent(), [Datagram { bytes: "ping".to_utf8(), host: "10.0.0.7", port: 9000 }], reason="the ping")
```

**Spec edit needed** (for the owner to order; `spec/` stays untouched
until then). In `spec/std/net.md`, replace the rule `r[std-net.provider.none]`
and the sentence "`std.net` declares no deterministic provider" in the
chapter intro and in Net Providers, and add a Scripted Net section with
these rules:

1. `r[std-net.scripted.decl]`: `std.net` declares `ScriptedNet`, `ScriptedStream`, `ScriptedListener`, and `ScriptedUdp`, each implementing its trait (`Net`, `TcpStream`, `TcpListener`, `UdpSocket`) and `Debug`, with private fields.
2. `r[std-net.scripted.new]`: the four `new` signatures, and `ScriptedStream::failing`.
3. `r[std-net.scripted.setup]`: `add_host`, `add_connection`, `fail_connection`, `add_listener`, `add_udp`, and the address form `host:port`.
4. `r[std-net.scripted.queue]`: connections for one address are served one per call, in queue order.
5. `r[std-net.scripted.lookup-unknown]` and `r[std-net.scripted.refused]`: the `Dns` and `Refused` results for an unscripted call.
6. `r[std-net.scripted.requested]`: `requested()` and what it records.
7. `r[std-net.scripted.read]`, `.read-max`, `.eof`, and `.exhausted`: chunk order, the `max` split, the empty chunk as end of stream, and the error past the end of the script, with each message text.
8. `r[std-net.scripted.write]`: `write!` records its bytes and returns `.Ok(())`; `written()` and `is_closed()`.
9. `r[std-net.scripted.failing]`: the error after the reads, for `read!` and `write!`.
10. `r[std-net.scripted.accept]` and `r[std-net.scripted.udp]`: accept order and its exhausted message; `receive!` order and cut, `sent()`.
11. `r[std-net.scripted.closed]`: `Disposed` after `close`.
12. `r[std-net.scripted.no-host]`: a scripted provider opens no socket and consults no grant.

Also in `spec/std/testing.md`, Unit Test Providers: add the row `Net` | `ScriptedNet` | Scripted Net in net.md, and delete the sentence "Each host capability trait but `Net` has" (it becomes "Each host capability trait has") and the paragraph that says `Net` has no std provider. In `spec/cli/command-line.md` and the website, no change. Then file an implementation issue for `lib/std/net.hd` (about 150 lines of hd) and a conformance fixture per rule.
