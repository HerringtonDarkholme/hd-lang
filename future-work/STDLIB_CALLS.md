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
| 73 (#224) | `DecodeError` is `InvalidCharacter`, `InvalidLength`, or `InvalidPadding`, each with a character position. A length error comes first and reports the text length. | The Rust `hex` crate checks length first. | confirmed |
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
| 77 (#229) | `Utf8Error` is `InvalidSequence(position)` or `Truncated`. Display gives "invalid UTF-8 sequence at byte N" or "incomplete UTF-8 sequence at the end of the bytes". | Rust's `Utf8Error`, simplified. | own |
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
| 81 (#237) | `std.cmp.max(a, b)` returns `a` on a tie, so it also gives the first of equal values. It was `b`, Rust's choice, and is not in the spec. | One first-among-equals rule for every `min` and `max`, as in Python. | own |
| 81 (#237) | The unspecified `List.zip` in `lib/std` keeps its `List[U]` argument. | The decision covers the specified `Iterator` adapters only. | own |

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

## Errors

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 71 (#221) | `ErrorReport` implements `Display` as the message, then one `caused by: X` line per cause. `chain` stops at the first `.None`. | It matches the Entry Results text. | own |
| 72 (#223) | `ContextError` displays as its message, and its `cause` is the wrapped error. | anyhow's `context`. | confirmed (fields stay private, revisit later) |
| 77 (#229) | `ContextError` Debug is `ContextError { message: "...", cause: "<cause Display>" }`. | `debug_struct` style. | own |
| 81 (#237) | `lib/std/console.hd` declares `ConsoleError`, and its `Display` text for `Closed` is `console closed`. | The spec leaves the text open. Declaring it in std costs nothing in the footprint. | own |

## JSON

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 75 (#226) | Object equality ignores key order. Nesting is limited to 128 levels. A duplicate key keeps its first position and takes the last value. | serde_json. | confirmed |
| 75 (#226) | `at(index)` with a negative index gives `.None`. `pretty` writes empty containers as `[]` and `{}`, with no trailing newline. | Keeps output and lookup predictable. | changed: pass 82 made `index` a `usize`, so no index is negative; the rest stands |
| 84 (#241) | Enums are externally tagged: `{"Variant":{...members}}`, and a variant with no member is the string `"Variant"`. Positional members are keyed `_0`, `_1`. A bare `"Variant"` decodes as `{"Variant":{}}`. | serde's default form; no tag key can clash with a member. One payload shape, an object, keeps the rules few. | own |
| 84 (#241) | Decoding ignores unknown keys. | serde's default; a reader accepts a newer writer's output. | own |
| 84 (#241) | A missing key decodes as `null`, so only a `T?` (giving `.None`) or a `Json` member may be missing; anything else is `MissingField`. A declared member default is not used. `.None` encodes as `null`, with its key kept. | serde's `Option` handling, with no extra trait method. Defaults per field wait for typed member facts. | own |
| 84 (#241) | Only `Map[string, V]` implements `ToJson` and `FromJson`. A map with other keys does not, so the program converts its keys. | A JSON key is a string; the program chooses the text. | own |
| 84 (#241) | `JsonError` gains `WrongType(path, expected)`, `MissingField(path)`, and `UnknownVariant(path, name)`. The path is a string such as `$.users[2].name`, with keys unescaped. `expected` is the scalar's type name, `"array"`, `"object"`, or `"variant"`. `std-json.error.enum.usize` is retired for `std-json.error.variants`. | One error type for `decode`; `serde_path_to_error`'s path form. A string path is cheap to compare and print. | own |
| 84 (#241) | Integers encode exactly and decode only from an integer in range; `1.0` is not an integer. Floats decode from any number; `f32` takes the nearest value, and past its range is `WrongType`. A NaN or an infinity encodes as `null`. | serde_json. | own |
| 84 (#241) | A `char` is a one-character string. An embedded member is one nested key, not flattened. Shared constructor data is not written. | serde's `char`; flattening is opt-in in serde. | own |
| 84 (#241) | No tuple template for `ToJson` or `FromJson` yet. | Not asked for; it can follow as a tuple template. | own |

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
| 82 (#238) | The primitives `bytes_len`, `bytes_at`, `bytes_slice`, and `format_f64_fixed` use `usize`. `list_version`, `char_scalar`, and `char_from_scalar` stay `i32`, since a version and a scalar value are not sizes. | The primitives serve `len`, indexing, `slice`, and `to_fixed`. | own |
| 82 (#238) | `lib/std` and `test/std` keep `i32` until the compiler session adds `usize` and an unsigned `len`, as the Decided, Not Yet Applied row SIZES-UNSIGNED in STDLIB_PLAN.md lists. | `lib/std` cannot name `usize` before the compiler declares it. | own |


## Random Numbers

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 85 (#242) | `Rng` holds its own state and fixes its generator: xoshiro128** 1.0, seeded by folding the `u64` seed to 32 bits and four SplitMix32-style `mix32` steps, as `lib/std`'s `SeededRandom` already does. The spec gives the reference code and test vectors. `SeededRandom` stays unfixed. | Seeded output, such as a level or a fixture's sample, must repeat across implementations. 32-bit steps need no wide multiplication, which the plan's PCG sketch did. | own |
| 85 (#242) | `rng()` seeds an `Rng` from one `next_u64` of `$ Random`. Every `Rng` method is a plain call with the empty row. | Go's `math/rand/v2`: one host draw per generator, and a seeded generator needs no provider. | own |
| 85 (#242) | The surface is `from_seed`, `next_u64`, `int`, `float`, `bool`, `choose`, `shuffle`, and `sample`, named as `Choices` names its draws. `sample`'s count is a `usize`. | The plan's sketch, plus `bool` and `sample` from the task. | own |
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
| 86 (#243) | `Match.start`, `Match.end`, and `RegexError.position` are byte offsets (`usize`), as `string.slice` and `JsonError` take them. | One offset unit across std; Go and Rust report byte offsets. | own |
| 86 (#243) | `RegexError` is a data type with `kind: RegexErrorKind` and `position`; the nine kinds are `MissingParen`, `UnmatchedParen`, `MissingBracket`, `BadEscape`, `BadRange`, `NothingToRepeat`, `BadRepeat`, `UnsupportedGroup`, and `TooLarge`, with Go's `regexp/syntax` texts and `KIND at byte N`. | The task asks for a kind and a position; a kind enum keeps the position in one field. | own |
| 86 (#243) | A quantifier right after another (`a**`, `a{2}{3}`, `a*??`) is `NothingToRepeat`, not a kind of its own. | Go rejects these as a nested repetition; one kind is enough for a script. | own |
| 86 (#243) | A `{` that starts no well-formed count is a literal, and so are `}` and `]` outside a class. A `]` first in a class, and a `-` first, last, or after a range, are items. | RE2 and Go. | own |
| 86 (#243) | Escapes are the six class escapes, `\n`, `\t`, `\r`, and escaped ASCII punctuation. Any other, such as `\b` or `\1`, is `BadEscape`. | Go's `regexp` rule for punctuation; leaving out `\b` keeps part 1 small. | own |
| 86 (#243) | A count is at most 1000, and a pattern's written-out size at most 10,000 instructions (`TooLarge`, position 0). The spec defines the size by a table. | RE2 caps counts at 1000; a size cap keeps O(m × n) meaningful. A precise table keeps the limit the same in every implementation. | own |
| 86 (#243) | `^` and `$` see only the ends of the text; `$` does not match before a final `\n`. | RE2 without `(?m)`. | own |
| 86 (#243) | The matcher is a Pike VM in `lib/std/regex.hd`: one thread per instruction per position, kept in priority order, with an explicit stack for epsilon steps. `is_match` is `find(...).is_some()`. | Leftmost-first submatch order needs the Pike VM's priorities; a stack keeps deep programs off the call stack. | own |
| 86 (#243) | `Regex` has `as_str()` and `Debug`; `Match` has `Eq` and `Debug`; `RegexErrorKind` and `RegexError` have `Eq`, `Debug`, and `Display`. | What fixtures and scripts compare and print. | own |
