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

## JSON

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 75 (#226) | Object equality ignores key order. Nesting is limited to 128 levels. A duplicate key keeps its first position and takes the last value. | serde_json. | confirmed |
| 75 (#226) | `at(index)` with a negative index gives `.None`. `pretty` writes empty containers as `[]` and `{}`, with no trailing newline. | Keeps output and lookup predictable. | confirmed |

## Testing

| Pass | Call | Why | Status |
| --- | --- | --- | --- |
| 67 (#217) | A discard is a panic with the message `std.testing: case discarded`, and the runner counts it only before `show`. | A `discard` method is no longer needed. | own |
| 67 (#217) | After the `replay` draws run out, each draw is fresh from the seed. | The prototype returned 0 there. | own |
