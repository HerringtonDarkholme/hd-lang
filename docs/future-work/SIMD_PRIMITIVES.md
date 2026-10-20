# SIMD Byte-Scan Primitives: Proposal

Every byte scan in hd's standard library, finding a byte, splitting on
a separator, finding where JSON needs an escape, validating UTF-8, is
a loop that reads one byte at a time. Wasm's SIMD instructions compare
16 bytes per step, and native libraries such as memchr and simdjson get
large speedups from them. This proposal asks how much of that hd can
have.

It defines five scan functions that std calls (`bytes_index`,
`bytes_index_pair`, `bytes_index_json`, `bytes_mismatch`,
`utf8_valid_len`), each a plain hd declaration whose body the compiler
supplies. The measurements in section 10 show the catch: SIMD cannot
load from hd's strings as they are laid out today, and the 16-byte
layout that would allow it makes every other string operation slower.
So strings keep their layout, and the functions get plain native loops
first, later; SIMD bodies follow only if a layout change ever measures
faster overall, and then they are emitted unconditionally. String copies
already run at memory-copy speed, so they need nothing.

This is the proposal for issue #428, under #426; nothing in it is in the
spec or the compiler yet.

- [1. Motivation](#1-motivation)
- [2. The Proposal On One Page](#2-the-proposal-on-one-page)
- [3. What Blocks SIMD Today](#3-what-blocks-simd-today)
- [4. The Primitives](#4-the-primitives)
- [5. The Callers](#5-the-callers)
- [6. Expected Gains](#6-expected-gains)
- [7. Engine Support](#7-engine-support)
- [8. Footprint](#8-footprint)
- [9. Testing](#9-testing)
- [10. Probe Results (2026-10-17)](#10-probe-results-2026-10-17)
- [11. Open Points](#11-open-points)

## 1. Motivation

Today every byte scan in `lib/std` is an hd loop over `bytes_at`. Each
call of `bytes_at` checks the index against the view's length, unpacks
the view's start, adds it, and reads one byte with `array.get_u`, which
the engine bounds-checks again (`str_at` in `crates/hd_wasm/src/emit.rs`).
That is about 15 Wasm instructions per byte before the loop's own
compare and branch.

The scans that matter are few:

- the substring search in `std.text` (`find`, `contains`, `split`,
  `splitn`, `replace`, `count`, `lines`): the one-byte case is a plain
  loop, and the Two-Way searcher tests one window per step;
- JSON strings, in both directions: `push_escaped` looks for the first
  byte that needs an escape, and the parser's `string` and `escaped`
  look for a quote, a backslash or a control byte;
- UTF-8 validation in `string::from_utf8`, which `std.http` (`text()`,
  `lossy_text`), `std.fs` and `std.encoding` call on whole bodies;
- byte comparison: `string_equal`, `string_compare`, `bytes_match`
  (`starts_with`, `ends_with`, the searcher's verification) and the
  runtime's `StrEq` helper behind `==`, `match` on strings, and map keys.

The current numbers (docs/audit/metrics-2026-10-17.md, release builds):
`split` runs at 461 MB/s (0.90x Node), JSON encode at 26 MB/s (0.05x)
and JSON decode at 22 MB/s (0.08x). The JSON gap is mostly allocation,
not scanning: #426 lists `decode` building a whole tree and walking it,
and `render` making a string per number. Faster scans alone will not
close it.

## 2. The Proposal On One Page

Five std primitives, each a plain hd declaration whose body the
implementation supplies:

```hd
fn bytes_index(text: string, from: usize, byte: u8) -> usize
fn bytes_index_pair(text: string, from: usize, first: u8, last: u8, gap: usize) -> usize
fn bytes_index_json(text: string, from: usize) -> usize
fn bytes_mismatch(left: string, right: string) -> usize
fn utf8_valid_len(bytes: List[u8]) -> usize
```

Each returns a byte offset. A search that finds nothing returns
`bytes_len(text)`, so no primitive builds an `Option`. Each has a short
hd reference body, and the supplied body must give the same result for
every input.

The work comes in two steps:

1. **Native loops now.** The supplied bodies are runtime helpers that
   loop over the backing array directly: one view check per call, not
   per byte. This needs no new Wasm feature and no layout change.
2. **SIMD later, only after a layout decision.** Wasm SIMD128 cannot
   read hd's strings as they are laid out today (section 3). SIMD bodies
   need the owner to approve a change of the string's backing array,
   after a probe on both engines.

The signatures stay the same across both steps, so the std callers are
rewritten once.

## 3. What Blocks SIMD Today

A string is a view `(ref $bytes, i64 span)` over `$bytes = (array i8)`, a
Wasm GC array (crates/hd_wasm_linear/DESIGN.md §15.2). A `List[u8]` stores its
bytes in a packed `(array (mut i8))` the same way.

SIMD128 loads a `v128` only from linear memory (`v128.load` and its
variants). Wasm GC reads an `i8` array one element at a time
(`array.get_u`). No instruction loads 16 bytes, or even 8, from an `i8`
array. The GC proposal's post-MVP list (bulk copy, fill, init from a data
segment) has no wide load either. The repository already records the
same gap for host copies: "Wasm GC has no bulk copy between arrays and
memory" (crates/hd_run_wasmtime/DESIGN.md §17.3).

Assembling a vector from 16 `array.get_u` and 16 `i8x16.replace_lane`
costs more than the scalar compare it replaces. Copying the bytes into
linear memory first is a per-byte loop too. So, with today's layout,
SIMD gives nothing.

Three ways out, cheapest first:

- **Keep the layout; ship native loops (step 1).** The gain is the view
  and bounds work removed from every byte, not 16 bytes per step.
- **Back strings with a wider array.** `$bytes` becomes `(array v128)`,
  padded with zero bytes to a multiple of 16. The Wasm GC spec allows
  `v128` as an array element type; both engines must be probed for it.
  A scan then does one `array.get` per 16 bytes. The costs land on every
  other string operation:
  - `bytes_at` becomes `array.get`, then a lane pick with
    `i8x16.swizzle` and `i8x16.extract_lane_u`: a few more instructions
    per byte read.
  - `array.copy` copies whole lanes, so a concatenation whose source
    start is not a multiple of 16 needs a shifting copy loop.
  - Every string rounds up to 16 bytes, so a 3-byte string's array is
    five times larger. The peak-heap metrics would show it.
  - Literals (`array.new_data`) need their segment padded, and the host
    copy loops change.
  - `List[u8]` keeps its own `(array (mut i8))`, so `utf8_valid_len`
    would need the same change for byte lists, or would stay scalar.
- **The same with `(array i64)`, as SWAR.** Eight bytes per step with
  plain integer instructions, no SIMD128. The same layout costs, for
  half the width.

A future Wasm proposal with a wide load from a packed array would remove
the layout cost. None is in the GC post-MVP list today.

## 4. The Primitives

Each primitive is private to the std module that declares it, as
`bytes_at` is, written with the reference implementation's
`@intrinsic("name")`. Offsets are byte offsets; `from` may equal the
length. A `from` past the length is a bug in `lib/std`, and the body
panics with `index-out-of-bounds`, as `bytes_slice` does.

### Find a byte

```hd
@intrinsic("bytes_index")
fn bytes_index(text: string, from: usize, byte: u8) -> usize:
    panic("intrinsic")
```

The first offset at or after `from` whose byte is `byte`, or
`bytes_len(text)`. The reference body:

```hd
fn bytes_index_reference(text: string, from: usize, byte: u8) -> usize:
    size := bytes_len(text)
    let at = from
    while at < size && bytes_at(text, at) != byte:
        at = at + 1
    at
```

This is C's `memchr` and Go's `IndexByte`. Searching for an ASCII byte
in valid UTF-8 can only stop at a scalar boundary, so a caller may slice
at the result.

### Find a byte pair

```hd
@intrinsic("bytes_index_pair")
fn bytes_index_pair(text: string, from: usize, first: u8, last: u8, gap: usize) -> usize:
    panic("intrinsic")
```

The first offset `at` at or after `from` with `at + gap` below the length,
`bytes_at(text, at) == first`, and `bytes_at(text, at + gap) == last`; or
`bytes_len(text)`. This is the substring prefilter that the memchr
crate's `memmem` uses ("Generic SIMD", Muła): a window can match the
needle only where its first and last bytes do. The searcher still checks
each candidate.

### Find the first byte JSON treats specially

```hd
@intrinsic("bytes_index_json")
fn bytes_index_json(text: string, from: usize) -> usize:
    panic("intrinsic")
```

The first offset at or after `from` whose byte is `"` (34), `\` (92), or
below 32; or `bytes_len(text)`. One predicate serves both directions.
The encoder escapes exactly these bytes ([std.json](../../spec/std/json.md)). The
decoder stops at exactly these bytes inside a string: the end quote, an
escape, or the control-character error. The SIMD body is three compares
and two ORs per 16 bytes, the same as simdjson's string scan.

### Find where two byte strings differ

```hd
@intrinsic("bytes_mismatch")
fn bytes_mismatch(left: string, right: string) -> usize:
    panic("intrinsic")
```

The first offset below both lengths where the bytes differ, or the
shorter length. Equality and order then take a line of hd each:

```hd
fn string_equal(left: string, right: string) -> bool:
    size := bytes_len(left)
    size == bytes_len(right) && bytes_mismatch(left, right) == size

fn string_compare(left: string, right: string) -> i32:
    at := bytes_mismatch(left, right)
    if at < bytes_len(left) && at < bytes_len(right):
        return if bytes_at(left, at) < bytes_at(right, at): -1 else: 1
    if bytes_len(left) < bytes_len(right): -1 else if bytes_len(left) > bytes_len(right): 1 else: 0
```

`bytes_match(text, needle, start)` becomes
`bytes_mismatch(bytes_slice(text, start, start + bytes_len(needle)), needle) == bytes_len(needle)`;
the slice allocates nothing. This is `memcmp` returning a position
instead of a sign, which is what both callers need.

### Validate UTF-8

```hd
@intrinsic("utf8_valid_len")
fn utf8_valid_len(bytes: List[u8]) -> usize:
    panic("intrinsic")
```

The length of the longest prefix of `bytes` that is well-formed UTF-8 and
ends at a scalar boundary. It equals `bytes.len()` exactly when the
whole list is valid. `string::from_utf8` keeps its current loop as the
error path: it calls `utf8_valid_len`, returns at once when the list is
valid, and otherwise runs its scalar decoder from the returned offset.
That offset is the lead byte of the first bad sequence, so the decoder
reports the same `InvalidSequence(index)` or `Truncated` as today.

### Left out on purpose

- **Count a byte.** No std caller counts one byte in a hot loop. A loop
  over `bytes_index` covers `count` with a one-byte needle.
- **Find the first of a byte set.** JSON's set is not a plain set (it
  includes a range), and has its own primitive. No other std caller
  needs a general set yet.
- **Skip JSON whitespace.** Runs between tokens are short (indentation),
  so a 16-byte step rarely pays. simdjson handles whitespace through its
  structural index, which is a parser design, not a primitive.
- **base64 and hex.** SIMD codecs exist (Muła and Lemire), but they are
  transforms, not scans. #426 lists cheaper fixes first: an exact-size
  buffer and no revalidation of output that is ASCII by construction.

## 5. The Callers

| Primitive | Callers in `lib/std` |
| --- | --- |
| `bytes_index` | `search_from` for a one-byte needle (`find`, `split`, `count`, `replace`; `lines` splits on `"\n"`); `find_literal` in regex.hd; the `/` scan of JSON pointers in json.hd |
| `bytes_index_pair` | `search_from` for needles of two bytes or more, as a prefilter before the Two-Way check, with `memory` reset to 0 after each jump |
| `bytes_index_json` | `push_escaped` (encode); the parser's `string` and `escaped` (decode) |
| `bytes_mismatch` | `string_equal`, `string_compare`, `bytes_match`; the runtime `StrEq` helper's loop; the common-prefix loop in regex.hd |
| `utf8_valid_len` | `string::from_utf8`, and through it `Response.text()` and `lossy_text` (http.hd), the file read in fs.hd, and `std.encoding`'s decode |

Each caller keeps its current structure. For example, `push_escaped`
jumps from one special byte to the next instead of stepping:

```hd
fn push_escaped(parts: mut List[string], text: string) -> void:
    size := bytes_len(text)
    let plain: usize = 0
    let index = bytes_index_json(text, 0)
    while index < size:
        if index > plain:
            parts.push(bytes_slice(text, plain, index))
        parts.push(escape_text(bytes_at(text, index)))
        plain = index + 1
        index = bytes_index_json(text, plain)
    if plain == 0:
        if size > 0:
            parts.push(text)
    else if plain < size:
        parts.push(bytes_slice(text, plain, size))
```

## 6. Expected Gains

No number below was measured on hd. The published figures are for
native code on x86_64 or ARM, not for Wasm. Wasm SIMD128 is 16 bytes
wide, as SSE2 and NEON are, and narrower than AVX2. Treat each figure as
an upper bound on what a SIMD body could reach for that scan alone.

- **Step 1, native loops.** The gain is the per-byte view work removed:
  about 15 Wasm instructions per byte down to about 5 (load, compare,
  add, branch), counted from `str_at`. No published figure applies; the
  first implementation measures it.
- **`bytes_index`.** The memchr crate ships SIMD128 routines for wasm32,
  with SWAR as its fallback (its README). Its published benchmarks are
  native, so this proposal cites no Wasm figure.
- **`bytes_index_pair`.** memchr's README compares its `memmem` with Rust
  std's, which it says can use SSE2 but not AVX, over 53 benchmarks on
  x86_64 (record of 2023-08-26). The geometric mean of speed ratios is
  1.03 for memchr against 6.50 for std when the searcher is prebuilt,
  and 1.04 against 5.26 when building is timed. The README credits the
  SIMD prefilter, AVX2, and API design together, so SIMD128 alone gets
  less.
- **`bytes_index_json`.** simdjson parses "gigabytes of data per second
  on a single core", with "a quarter or fewer instructions" than
  RapidJSON (Langdale and Lemire, "Parsing Gigabytes of JSON per
  Second", VLDB Journal 2019). That is a whole-parser result with a
  different design. For hd, the scan's share of encode and decode time
  bounds the gain, and allocation dominates both today (section 1).
- **`bytes_mismatch`.** No published Wasm figure was found. Equality on
  short keys is dominated by the call and the length check, so the gain
  shows only on long strings.
- **`utf8_valid_len`.** The lookup algorithm "outperforms UTF-8
  validation routines used in many libraries and languages by more than
  10 times" (Keiser and Lemire, "Validating UTF-8 In Less Than One
  Instruction Per Byte", Software: Practice and Experience 51(5), 2021).
  simdutf's README quotes a 364% decoding improvement in Node.js over
  Node.js 16 after adopting it, an end-to-end figure.

The done check in #428 asks for 4x on 1 MB inputs on both engines. Step
1 is unlikely to reach that; only SIMD bodies, after the layout change,
could.

## 7. Engine Support

- **V8.** SIMD128 is on by default since Chrome 91 and Firefox 89
  (v8.dev/features/simd). Every browser engine shipped SIMD128 before it
  shipped Wasm GC, so any engine that runs hd's output already has it.
- **wasmtime.** `simd`, `relaxed-simd` and `gc` are all Tier 1 and on by
  default (docs.wasmtime.dev, Wasm proposals page). The Winch baseline
  compiler does not support `gc` at Tier 1, so it cannot run hd anyway.
- **What a SIMD body uses.** `v128` from `array.get` on a `v128` array,
  `i8x16.splat`, `i8x16.eq`, `i8x16.lt_u` (the JSON control-byte test),
  `v128.or`, `v128.any_true`, `i8x16.bitmask` and `i32.ctz` (the offset
  of the first hit). UTF-8 validation also uses `i8x16.swizzle` for the
  lookup tables. All are in the finished SIMD128 set; none needs relaxed
  SIMD.
- **Whole-module effect.** A module that contains one SIMD instruction
  fails to validate on an engine without SIMD128. With both targets
  supporting it, no feature gate is needed, and the owner chose to emit
  SIMD bodies unconditionally if they measure faster.

## 8. Footprint

- **User Wasm.** Each primitive is one runtime helper, linked only when
  a reachable function calls it, as `StrEq` is today. A program that
  never searches, compares, or validates pays nothing; the existing
  "no unused std" metric checks this. The tiny program reaches none of
  the callers. Estimated sizes, to be confirmed by the footprint guard:
  a scalar body is under 100 bytes; a SIMD body with its scalar tail is
  200 to 400 bytes; UTF-8 validation with its tables is the largest.
- **One caveat.** If `StrEq` moves onto `bytes_mismatch`, every program
  that compares strings with `==`, matches on strings, or keys a map by
  string pays the larger body. The scalar `StrEq` could stay for short
  inputs, at the cost of two bodies.
- **The web compiler.** Five helper emitters in `hd_wasm`, each a few
  dozen lines in the style of `StrEq`, plus the five std declarations.
  The growth of the hd_web Wasm is expected to be small; the
  implementation reports the measured size before and after.
- **The layout change, if approved.** Its cost is not in the primitives.
  It is in every string operation and in heap size (section 3), and it
  needs its own footprint and peak-heap report.

## 9. Testing

The rule: for every input, the supplied body returns what the hd
reference body returns.

- **Reference bodies in hd.** Each primitive's reference body lives in
  `lib/std` tests, beside the property tests, and is never called by
  programs.
- **Equivalence properties.** One `it_prop` per primitive compares the
  primitive with its reference on drawn inputs. The draws matter more
  than the count:
  - lengths 0 to 64, around every 16-byte block edge (15, 16, 17, 31, 32,
    33), and a few large inputs up to 1 MB;
  - every view start from 0 to 31, made by slicing a longer string, since
    a view can start anywhere in its array;
  - a match at the first byte, the last byte, a block edge, the byte
    just before `from`, and nowhere;
  - alphabets from dense matches to none, and byte values 0, 31, 32, 127,
    128, and 255 (signed-compare mistakes show there).
- **UTF-8 cases.** Truncated sequences ending at each block edge,
  overlong forms, surrogates (`ED A0 80`), values above U+10FFFF
  (`F4 90 80 80`), stray continuation bytes, and the lead bytes `C0`,
  `C1`, and `F5` to `FF`. The property checks that `from_utf8` returns
  the same `Utf8Error` as the current loop, not only the same length.
- **Both engines.** The same properties run under wasmtime and the JS
  host, with the same seeds.
- **Existing fixtures.** The conformance fixtures for `find`, `split`,
  `replace`, JSON text, and `from_utf8` pin today's results and must
  pass unchanged.
- **Benchmarks.** Rows in `pnpm run metrics` for find, the JSON escape
  scan, and validation on 1 MB inputs, as #428 asks, on both engines.

## 10. Probe Results (2026-10-17)

Hand-written Wasm kernels on an Apple M3 Max, Node 24 (V8 13.6) and
wasmtime 49.0.2 with the repository's configuration, median of 9 warm
calls. Every variant returned the same result. "Today" is
`(array i8)` read one byte at a time; "v128" is `(array v128)` with
`i8x16.eq`, four lanes per step, `any_true` in the loop and a bitmask
only on a hit.

| Kernel | V8 today | V8 v128 | wasmtime today | wasmtime v128 |
| --- | --- | --- | --- | --- |
| find a byte, 1 MiB (ns/byte) | 0.448 | 0.020 (22x) | 1.26 | 0.030 (41x) |
| find a byte, 64 B (ns/call) | 25.1 | 4.1 (6x) | 78.1 | 7.3 (10x) |
| 1 MiB unaligned concat (ns/byte) | 0.017 | 0.038 (2.3x slower) | 0.013 | 0.104 (8x slower) |
| sequential `bytes_at` (ns/byte) | 0.33 | 0.60 (1.8x slower) | 0.97 | 1.19 (1.2x slower) |
| 1 MiB string to the host (ns/byte) | 0.250 | 0.026 (9.5x) | 0.371 | 0.060 (6x) |
| heap object of a 3-byte string | 32 B | 40 B | 32 B | 48 B |

What the numbers say:

- Scans clear #428's 4x target on both engines. SWAR over `(array i64)`
  gets 8x to 9x and pays nearly the same layout costs.
- Step 1 (native loops on today's layout) gains almost nothing: V8
  already removes the view check, and wasmtime gains about 10%.
- Copies already run at SIMD speed: both engines lower `array.copy` on
  an `i8` array to a native memmove above a small size (wasmtime's
  `emit_raw_array_or_table_copy`, V8's `wasm_array_copy`). A v128 layout
  makes unaligned concatenation slower, not faster.
- Byte reads, which every lexer and parser loop makes, get 1.2x to 1.8x
  slower, and short strings take 4 to 12 more bytes on average.
- The host-boundary copy is the one copy that gains, 6x to 10x.

The kernels and runners are kept outside the repository; the probe can
be rerun from them on request.

## 11. Open Points

1. When to land step 1 (native loops); deferred by the owner.
2. The string layout. The owner kept `(array i8)` after the probe
   (2026-10-17); revisit once JSON and text parsing scan through these
   primitives instead of reading byte by byte, and measure real programs
   then.
