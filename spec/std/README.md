# Standard Library Specification

This directory holds the stdlib tier of the specification. The numbered
chapters in [`lang/`](../README.md#contents) are the language tier.
Both tiers are one specification, written to one
[Specification Style](../STYLE.md), with one conformance suite.

## Tiers

| Tier | Holds | Where |
| --- | --- | --- |
| language | syntax, static and dynamic semantics, intrinsics, and every std item the compiler must know by name | the [numbered chapters](../README.md#contents) |
| stdlib | std APIs that `lib/std` writes in ordinary hd over the language tier | this directory |
| CLI | how the `hd` command finds and runs a package or a single file | [Command Line](../cli/command-line.md) |

The language tier names a std item only when the compiler must know it:

- a lang item, such as `Iterable` for `for`, `std.process.Termination`, or
  a `std.ops` operator trait;
- an intrinsic, such as `List.push`, `string.len`, or `facts_of(f)`;
- a prelude name and its signature;
- the functions of the test registration calls `it`, `it_each`, `it_prop`,
  and `it_prop_with`, with their position rules and diagnostics;
- the conformance harness: `it`, `assert`, `assert_equal`, and `println`.

This tier keeps what those registration functions do when a test runs:
table rows, property generation and shrinking, the runner capabilities,
reporting, and snapshots ([Testing](testing.md)).

Every other decided std API belongs in this directory.

### The Tier Test

> Could `lib/std` implement the item in ordinary hd, over language-tier
> items only, with no compiler knowledge of its name, and keep the same
> observable behavior?

Yes puts the item in the stdlib tier. A diagnostic code about the item, a
language rule that names it, or a position rule that lists it each mean
no.

Only `std` may add inherent methods to built-in types
([`trait.own.inherent.std`](../lang/09-traits.md#r-trait.own.inherent.std)).
That alone does not make a method language-tier.

## Standard Library Primitives

`lib/std` is ordinary hd over the language tier, plus the primitives
below. A primitive is a function or a method whose body the
implementation supplies. A primitive function is private to the std
module that declares it, so no program calls it. The specification has no
syntax for declaring one; the reference implementation writes
`@intrinsic("name")` before it. A primitive operation is an
[intrinsic method](../lang/09-traits.md#intrinsic-methods), which any
program may call.

Any primitive not in these tables needs the owner's approval.

**Representation.** These read or build the run-time layout of a string,
a `char`, or a list.

| Primitive | Signature | Why it is a primitive |
| --- | --- | --- |
| `bytes_len` | `(s: string) -> usize` | reads the byte length of a string's representation |
| `bytes_at` | `(s: string, i: usize) -> u8` | reads one byte of a string's representation |
| `bytes_slice` | `(s: string, start: usize, end: usize) -> string` | shares the bytes in constant time, as [`module.string.slice.shared`](../lang/10-modules.md#r-module.string.slice.shared) requires |
| `bytes_concat` | `(a: string, b: string) -> string` | allocates a string's representation; hd has no byte buffer |
| `concat_all` | `(parts: List[string]) -> string` | allocates one string's representation for every part, sized first, so `join` and `StringBuilder.build` copy each byte once; hd has no byte buffer |
| `string_from_bytes` | `(bytes: List[u8]) -> string` | builds a string from bytes that the caller has checked are UTF-8 |
| `string_to_bytes` | `(s: string) -> List[u8]` | builds a list of exactly a string's bytes in one sized allocation and one copy, which an hd loop over `bytes_at` and `push` cannot do |
| `format_int` | `[N < Integer](value: N) -> string` | builds the decimal text of an integer, with `-` when negative, in one string buffer; hd would build a byte list first, which is slower and adds code to every program that shows an integer |
| `char_scalar`, `char_from_scalar` | `(c: char) -> u32`, `(point: u32) -> char?` | a `char` is its scalar value at run time; `char_from_scalar` gives `.None` for a value that is not a Unicode scalar value, a surrogate or one above `0x10FFFF` |
| `f64_to_bits`, `f64_from_bits` | `(value: f64) -> u64`, `(bits: u64) -> f64` | the bit cast between an `f64` and its IEEE 754 pattern, one reinterpret instruction; hd has no other way to read a NaN's sign, which [`std-num.float-bits.to`](num.md#r-std-num.float-bits.to) and `total_cmp` need |
| `f32_to_bits`, `f32_from_bits` | `(value: f32) -> u32`, `(bits: u32) -> f32` | the same bit cast for `f32` |
| `list_truncate` | `[T](items: &mut List[T], len: usize) -> void` | shortens the list `items` refers to, to its first `len` elements, the one hook under `List.pop`, `insert`, `remove_at`, and `clear`; panics when `len` is greater than the size. Panic: `index-out-of-bounds` |

**Scans.** These search the bytes of a string or a byte list. Each has an hd reference body
that defines its result, and an implementation may supply a faster body
that gives the same result. The owner approved them in
[#428](https://github.com/HerringtonDarkholme/hd-lang/issues/428).

| Primitive | Signature | Why it is a primitive |
| --- | --- | --- |
| `bytes_index_json` | `(s: string, from: usize) -> usize` | the offset of the first byte at or after `from` that JSON must escape (a byte below `0x20`, `"` or `\`), or `bytes_len(s)` when there is none; SIMD reads 16 bytes per step where an hd loop reads one, and JSON encoding and parsing scan every string byte |
| `bytes_index` | `(s: string, from: usize, byte: u8) -> usize` | the offset of the first byte at or after `from` equal to `byte`, or `bytes_len(s)` when there is none; SIMD compares 16 bytes per step where an hd loop compares one, and one-byte `find`, `split`, `count`, `replace` and `lines`, the regex literal prefilter and the JSON pointer scan are all this search |
| `bytes_mismatch` | `(left: string, right: string) -> usize` | the offset of the first position where `left` and `right` differ; when one is a prefix of the other, the shorter length, and when they are equal, their common length; SIMD compares 16 bytes of each per step where an hd loop compares one, and string `==` and ordering, `starts_with` and `ends_with` are all this comparison |
| `bytes_index_pair` | `(s: string, from: usize, first: u8, last: u8, gap: usize) -> usize` | the offset of the first `i >= from` with `i + gap < bytes_len(s)`, byte `i` equal to `first` and byte `i + gap` equal to `last`, or `bytes_len(s)` when there is none; SIMD tests 16 offsets per step where an hd loop tests one, and `find`, `split` and the other multi-byte substring searches use it to skip to a window whose first and last bytes are the needle's before checking the whole needle |
| `utf8_valid_len` | `(bytes: List[u8]) -> usize` | the length of the longest prefix of `bytes` that is well-formed UTF-8 and ends at a scalar boundary, so `bytes.len()` exactly when all of it is valid; SIMD passes 16 ASCII bytes per step where an hd loop decodes one, and `string::from_utf8`, and through it HTTP bodies, file reads and `std.encoding`, validates every byte |

**Panic.** One primitive raises every checked runtime panic that `lib/std`
raises with a category other than `explicit-panic`.

| Primitive | Signature | Why it is a primitive |
| --- | --- | --- |
| `panic` | `(category: string, message: string) -> void` | the prelude `panic` always has the category `explicit-panic`; this one names a [stable category](../lang/06-control-flow.md#r-flow.panic.known-categories), and an empty message shows none |

**Compiler-level.** The compiler lowers these at each use.

| Primitive | Why it is a primitive |
| --- | --- |
| `facts_of` | the compiler builds each call's facts value ([Function Facts](../lang/14-annotations.md#function-facts)) |
| `task_race_frame`, `task_all_frame` | the polling frames of `race!` and homogeneous task-list driving, which are compiler intrinsics ([`req.combinator.intrinsic`](../lang/11-requirements-and-suspension.md#r-req.combinator.intrinsic)) |

**Runtime.** The memory intrinsics of `std.rt.memory` and its typed
memory are language tier, because the compiler knows them by name, so
[Memory Intrinsics](../lang/10-modules.md#memory-intrinsics) and
[Typed Memory](../lang/10-modules.md#typed-memory) define them.
They are `pub`, but only std may use a
[runtime module](../lang/10-modules.md#runtime-modules), so still no
program calls them.

**Host.** The host supplies these, for now.

| Primitive | Signature | Why it is a primitive |
| --- | --- | --- |
| `format_f64`, `format_f32` | `(value: f64) -> string`, `(value: f32) -> string` | the shortest round-trip decimal text of a float ([Numeric Display](../lang/04-type-system.md#numeric-display)) |
| `string_lower`, `string_upper` | `(text: string) -> string` | Unicode case mapping, which needs the Unicode tables |
| `parse_f64` | `(text: string) -> f64` | the correctly rounded `f64` of an unsigned decimal number text, which needs arbitrary-precision arithmetic |
| `format_f64_fixed` | `(value: f64, digits: usize) -> string` | the correctly rounded fixed-point text of a float, which needs arbitrary-precision arithmetic |
| `map_hash_seed` | `() -> u64` | the run's random key for the map's bucket hash ([`std-hash.map.bucket-hash`](hash.md#r-std-hash.map.bucket-hash)); a program has no other source of randomness that needs no capability |
| `snapshot_update` | `(text: string) -> bool` | the update run's side of [`snapshot`](testing.md#r-std-testing.snapshot.update): in an update run of an `it` test case it records `text` and the call's place and returns true, else it returns false; only `std.testing` calls it |

A host primitive takes and returns each `f32` and `f64` as a raw IEEE
754 value, as a host capability call does by
[`module.profile.host-float.raw`](../lang/10-modules.md#r-module.profile.host-float.raw).
So a NaN, both infinities, and `-0.0` reach a hook unchanged.

`format_f64` and `format_f32` write the whole `Display` text of a float,
by [Numeric Display](../lang/04-type-system.md#numeric-display). The hook,
not `lib/std`, writes the text of
[`types.display.special`](../lang/04-type-system.md#r-types.display.special):
`NaN`, `inf`, `-inf`, and the `-` of `-0.0`.

`parse_f64` takes only an unsigned
[decimal number](num.md#r-std-num.parse-f64.decimal), with no sign and no
special word. `lib/std` checks the grammar and reads the sign before the
call ([`std-num.parse-f64.hook`](num.md#r-std-num.parse-f64.hook)). The
hook returns the `f64` nearest the text's decimal value, rounding a tie to
the one with an even significand, as IEEE 754 rounds to nearest. So a
value past the finite `f64` range is positive infinity, and a value too
small for the smallest subnormal is `0.0`.

`parse_f64` has no error or refusal signal. A call with text outside the
grammar is a bug in `lib/std`, not a case the host handles.

`format_f64_fixed` writes the text that
[`to_fixed`](num.md#fixed-point-text) returns, with every rule of that
section but the check of `digits`. Its caller passes a `digits` from 0 to
100. For a finite value, it writes the multiple of 10 to the power
`-digits` nearest the value's exact binary value, and a tie goes to the
multiple whose last digit is even. A NaN gives `NaN`, and the infinities
give `inf` and `-inf`. The hook, not `lib/std`, writes that text, and the
`-` of a negative value or of `-0.0`.

**Test-runner hooks.** `std.testing` reaches the test runner through the
host capabilities `TestRunner` and `PropertyRunner`
([Runner Capabilities](testing.md#runner-capabilities)), so table rows,
timeouts, snapshot files, and property draws need no primitive.

**Operations.** One intrinsic method per primitive operation. `lib/std`
writes each once, mostly in a numeric-family implementation such as
`impl[N < Num] Add for N`
([`trait.target.numeric-family`](../lang/09-traits.md#r-trait.target.numeric-family)),
and the compiler specializes it for each type.

| Intrinsic method | Implemented for | Why it is a primitive |
| --- | --- | --- |
| `Add.add` | every number type | machine addition, with the integer overflow panic of a debug or test build |
| `Sub.sub` | every number type | machine subtraction, with the integer overflow panic of a debug or test build |
| `Mul.mul` | every number type | machine multiplication, with the integer overflow panic of a debug or test build |
| `Div.div` | every number type | truncating integer division with its panics, or IEEE 754 division |
| `Rem.rem` | every number type | the truncated remainder, with its panic, or the floating remainder |
| `Neg.neg` | the signed integer and floating-point types | machine negation; negating the minimum integer panics in a debug or test build |
| `BitAnd.bit_and` | every integer type | one machine instruction on the bits |
| `BitOr.bit_or` | every integer type | one machine instruction on the bits |
| `BitXor.bit_xor` | every integer type | one machine instruction on the bits |
| `Not.not` | every integer type | one machine instruction on the bits |
| `Shl.shl` | every integer type, with any unsigned integer count | a fixed-width shift, with the shift count panic |
| `Shr.shr` | every integer type, with any unsigned integer count | a fixed-width shift, arithmetic for a signed value, with the shift count panic |
| `Eq.eq` | every number type, `char`, and `bool` | compares the machine values; floats follow IEEE 754 |
| `PartialOrd.partial_cmp` | every number type and `char` | compares the machine values; a NaN operand is unordered |
| `Ord.cmp` | every integer type and `char` | compares the machine values |
| `sqrt` | `f32` and `f64` | the correctly rounded IEEE 754 square root, one machine instruction ([`std-num.float-sqrt`](num.md#r-std-num.float-sqrt)) |

`string`'s `Add`, `Eq`, `PartialOrd`, and `Ord` are not intrinsic: `lib/std`
writes them in hd.

> **Why.** Each primitive is a promise that every implementation must
> keep, and code that the compiler supplies cannot be read or tested as
> hd. A short explicit list keeps the rest of `lib/std` portable.

## Chapters

Each chapter is one unnumbered file named after its std module. The
Spec Tiers migration plan
added the files below, and it is complete. A new decided std module gets a
file of its own.

| File | Module | Rule ID prefix | Scope |
| --- | --- | --- | --- |
| [`testing.md`](testing.md) | `std.testing` | `std-testing` | what `it_each`, `it_prop`, and `it_prop_with` cases do when they run, property tests, the draw limit, derived `Arbitrary`, table-test rows, snapshot files, the `timeout` option, `hd_run!`, the runner capabilities |
| [`iter.md`](iter.md) | `std.iter` | `std-iter` | iterator adapters, including `skip`, `take_while`, `zip`, `chain`, `flat_map`, `any`, `all`, `find`, and `count`; what `collect` builds, `FromIterator` and its impls, `map` on a list or an optional |
| [`text.md`](text.md) | `std.text` | `std-text` | string methods above the intrinsics, including `lines` and `repeat`; `split_once`, `split_whitespace`, padding, and `count`; the `r` prefix |
| [`format.md`](format.md) | `std.format` | `std-format` | the text `debug` returns, `Debug` builders and layout, derived builder calls, `Debug` for the public std types |
| [`time.md`](time.md) | `std.time` | `std-time` | `Duration`, its suffixes, its arithmetic, and its `Display` text; `Timestamp + Duration`; the host trait `Clock`, `Timestamp`, `Instant`, `now`, and `sleep!`; the provider `ManualClock`; the UTC `Date`, RFC 3339 text, and `ParseTimeError`; the serialization opt-in of the time types |
| [`task.md`](task.md) | `std.task` | `std-task` | `Layer` and `apply!`; the `retry!(Backoff)` combinator; `timeout!` and `TimedOut`; `map_all!` |
| [`ops.md`](ops.md) | `std.ops` | `std-ops` | the `Default` trait and its standard implementations |
| [`cmp.md`](cmp.md) | `std.cmp` | `std-cmp` | what derived `Eq`, `PartialOrd`, and `Ord` compare; tuple comparison; `clamp`; `Reverse` |
| [`hash.md`](hash.md) | `std.hash` | `std-hash` | what derived `Hash` hashes; tuple hashing; `DefaultHasher` and `hash_of` |
| [`collections.md`](collections.md) | `std.collections` | `std-collections` | the `List` methods `view` and `chunks`, the `ListView` type, the `List` helpers such as `sorted_by_key`, `group_by`, and `windows`, the `Map` methods `contains_key`, `keys`, `values`, and `get_or`, `counts`, `Set`, `Deque`, and `Heap` |
| [`console.md`](console.md) | `std.console` | `std-console` | `eprintln`, when host console output becomes visible, the host trait `ConsoleInput`, and `read_line!` and `read_all!` |
| [`host.md`](host.md) | `std.host` | `std-host` | the host traits `Args`, `Env`, and `Sys`, the helpers `args` and `env`, the providers `ScriptedArgs`, `ScriptedEnv`, and `ScriptedSys`, `HostError`, and the rules every host operation follows |
| [`fs.md`](fs.md) | `std.fs` | `std-fs` | the host traits `FsRead` and `FsWrite`, `FsError`, `Entry`, the helpers `read_text!` and `write_text!`, and the provider `MemoryFs` |
| [`path.md`](path.md) | `std.path` | `std-path` | the `Path` newtype |
| [`process.md`](process.md) | `std.process` | `std-process` | the `Display` text of `ProcessError`; `Eq` and `Debug` for `ExitCode` and `ProcessOutput`; the provider `ScriptedProcess`; child process inheritance, signal status, and cancellation |
| [`http.md`](http.md) | `std.http` | `std-http` | the host trait `Http`, request sending and checked `get!`, `Request`, `Response`, `Method`, `HttpError`, the provider `ScriptedHttp`, and the playground's `Http` |
| [`server-sdk.md`](server-sdk.md) | `server_sdk.server` (SDK package) | `std-server-sdk` | typed request leases, the SDK-owned `HttpServer` capability, and joined worker loops |
| [`net.md`](net.md) | `std.net` | `std-net` | the host trait `Net` for DNS lookup, TCP, and UDP; the handles `TcpStream`, `TcpListener`, and `UdpSocket`; `Datagram` and `NetError` |
| [`random.md`](random.md) | `std.random` | `std-random` | the host trait `Random`, the provider `SeededRandom`, and the seeded generator `Rng` with `rng` |
| [`option.md`](option.md) | `std.option` | `std-option` | the methods `and_then`, `unwrap_or`, `ok_or`, `is_some`, `is_none`, and `expect` of `T?` |
| [`result.md`](result.md) | `std.result` | `std-result` | the methods `and_then`, `map_err`, `unwrap_or`, `map`, `ok`, `err`, `is_ok`, `is_err`, and `expect` of `Result[T, E]` |
| [`num.md`](num.md) | `std.num` | `std-num` | checked, wrapping, and saturating integer methods, `abs_diff`, and the bit counts on every integer type; `is_nan` and `is_finite`; integer parsing with `ParseNumberError`; `parse_f64`; `to_fixed` |
| [`error.md`](error.md) | `std.error` | `std-error` | the cause chain `chain`, what `root_cause` and `find` return, the boundary-safe `ErrorReport`, `report_of`, and `Result.context` with private error wrappers |
| [`encoding.md`](encoding.md) | `std.encoding` | `std-encoding` | hex and base64 text for bytes: `hex_encode`, `hex_decode`, the enum `Base64` with `encode` and `decode`, and `DecodeError` |
| [`digest.md`](digest.md) | `std.digest` | `std-digest` | `Digest` with constant-time equality, the streaming `Sha256` with `Sha256::hmac`, and `sha256` |
| [`json.md`](json.md) | `std.json` | `std-json` | the untyped `Json` value, `Number`, `parse`, `JsonError`, `pretty`, and the `Display` text; JSON as a format over the serialization opt-in: `to_json`, `from_json`, `encode`, and `decode` |
| [`serde.md`](serde.md) | `std.serde` | `std-serde` | the data model of `Serializer` and `Deserializer`, the calls of derived `Serialize` and `Deserialize`, and the standard implementations |
| [`cli.md`](cli.md) | `std.cli` | `std-cli` | argument parsing for scripts: the `Cli` builder, `parse`, `parse_args`, `Parsed`, `CliError`, and `usage` |
| [`regex.md`](regex.md) | `std.regex` | `std-regex` | regular expressions in the RE2 subset: `Regex`, `is_match`, `find`, `Match`, and `RegexError` |

## Test Provider Names

Each host trait has a test provider in its own module. The name's first
word says how the provider answers:

| Prefix | The provider | Providers |
| --- | --- | --- |
| `Scripted` | answers from data the test supplies | `ScriptedInput`, `ScriptedProcess`, `ScriptedHttp`, `ScriptedNet`, `ScriptedStream`, `ScriptedListener`, `ScriptedUdp`, `ScriptedArgs`, `ScriptedEnv`, `ScriptedSys` |
| `Memory` | keeps its state in memory, as a real host would | `MemoryFs`, `MemoryConsole` |
| `Manual` | holds a state that the test advances | `ManualClock` |
| `Seeded` | draws from a generator that a seed starts | `SeededRandom` |

## Glossary

This glossary lists the terms of the stdlib chapters. Each entry links to
the rule, or the section, that defines the term. The language terms are in
the [language glossary](../README.md#glossary).

| Term | Definition |
| --- | --- |
| **Debug builders** | The `DebugWriter` methods that describe a value as a struct, tuple, list, or map. See [Debug Builders](format.md#debug-builders). |
| **draw limit** | The per-case limit on draws from `Choices`; once it is spent, every draw returns its simplest value. See [`std-testing.limit`](testing.md#r-std-testing.limit). |
| **flag** | A command-line argument that `std.cli` reads as set or not set; it takes no value. See [`std-cli.parser.arg`](cli.md#r-std-cli.parser.arg). |
| **iterator adapters** | Methods of the prelude `Iterator[T]` that wrap an iterator in a new one, or drain it. See [Iterator Adapters](iter.md#iterator-adapters). |
| **option** | A command-line argument that `std.cli` reads with one value. See [`std-cli.parser.arg`](cli.md#r-std-cli.parser.arg). |
| **pad bits** | The low bits of the last base64 symbol that encode no byte; they are zero. See [`std-encoding.base64.encode.pad-bits`](encoding.md#r-std-encoding.base64.encode.pad-bits). |
| **padding** | The one or two `=` characters that end a base64 text. See [`std-encoding.base64.decode.padding`](encoding.md#r-std-encoding.base64.decode.padding). |
| **positional** | A command-line argument that is not a flag or an option; `std.cli` reads it by position. See [`std-cli.parser.arg`](cli.md#r-std-cli.parser.arg). |
| **property test** | A test case whose body runs on inputs drawn from a `Choices` source. See [Property Tests](testing.md#property-tests). |

## Rule IDs

1. A rule ID here starts with its file's prefix: `std-` and the module
   name.
2. A rule that moves here from a numbered chapter is retired there and
   keeps its tail under the new prefix. `module.testing.prop.debug`
   becomes `std-testing.prop.debug`.
3. The retired ID is deleted, as [Stability](../STYLE.md#stability)
   requires.

## Links Between Tiers

- A stdlib chapter may cite any language rule.
- A language chapter links here only from a Note or a See also, never from
  a numbered rule. No language rule depends on stdlib behavior.
- A moved heading is deleted, and every link to it is fixed when it moves.

## Conformance And Diagnostics

- A case's tier is the tier of its `specification` column in
  [`cases.tsv`](../conformance/cases.tsv). A path under `std/` is stdlib.
- Every diagnostic code stays in the [Diagnostics](../README.md#diagnostics)
  table, and every panic category stays in
  [Control Flow](../lang/06-control-flow.md). Each is a compiler check.
