# Standard library implementation review, 2026-10-17

Issue #350, job B (principle 7: "builtin performance should be high,
don't use naive algorithm"). This review reads every algorithm in
`lib/std` and the runtime and host code it reaches, measures the worst
cases against Node, and names the root causes of the five Pillar 3
metric failures in [metrics-2026-10-17.md](metrics-2026-10-17.md). It
changes no code.

|             |                                                                        |
| ----------- | ---------------------------------------------------------------------- |
| Commit      | `caa69d93a`                                                            |
| Machine     | Apple M3 Max, macOS 26, Node v24.19.0                                  |
| Binary      | `target/release/hd` from `cargo build --release -p hd_cli`             |
| Programs    | `hd build --release`, run as `node host/run.mjs FILE.wasm` (the `hd FILE.wasm` path); host-call programs ran as `hd FILE.wasm` for their grants |
| Timing      | inside the program with `std.time.now()`, median of 3 runs; Node times the same work after a warm-up |
| Profiles    | `node --cpu-prof`, Wasm frames named from the release `hd.names` section |
| Load        | 1-minute average 2.5 to 5.8 during the runs; the final full table ran at 2.69 to 2.76 |
| Scripts     | `target/stdlib-b/` (not committed): `bench.mjs`, `cases/*.hd`, `cases/*.js`, `prof.sh`, `top.mjs` |

## Summary

The library is not slow because of one bad algorithm. Most of it uses the
right algorithm, often the textbook one: the map is an insertion-ordered
table with linear probing, the regex engine is a Pike VM with no
backtracking, dates use Hinnant's `civil_from_days`, and `repeat`,
`replace` and `join` copy each byte once. The time goes to four habits
that repeat across modules:

1. **Float text crosses to the host and runs on BigInt.** `Display` for
   `f64` and `parse_f64` are host primitives. Both hosts implement them
   with arbitrary-precision arithmetic (Steele and White for printing, a
   rational division for parsing), and every call also pays a host
   crossing. One float costs 1.45 µs to print and 0.8 µs to parse,
   20x and 28x Node. Floats are 39% of JSON encode time and 31% of decode
   time in the metric document.
2. **Per-byte abstractions that allocate or call indirectly.** Hot loops
   walk text through `text.bytes()` or `text.chars()` (a closure call per
   byte), copy strings into `List[u8]` one `push` at a time, validate
   UTF-8 again on bytes the library itself just wrote (`from_utf8` in
   JSON, base64 and hex output), split text into one-character strings
   (`split("")` in every number parser), and compare those against string
   literals, each of which is a call into the literal pool.
3. **Intermediate structures.** JSON encode builds a whole `Json` tree
   (with a hash index per object) before it writes text. JSON decode
   builds the tree, then walks it while building a path string for every
   member and item. Regex copies the haystack into a list of `char` and
   a list of offsets before it searches, even when the match is at
   byte 0.
4. **Setup repeated per call.** The regex VM allocates its tables on every
   `run`, once per match in `find_all`, and a new thread list at every
   text position. The property runner instantiates the module and
   rebuilds the whole host import object for every generated case.

Only a few things are asymptotically wrong: substring search is O(n·m)
(`"a"*100000` searched for `"a"*1000 + "b"` takes 84 ms; Node takes
0.2 ms), `Regex.is_match` and `find` are O(text length) even when the
match is at the start (11.6 ms per call on 1.1 MB), the in-memory
`MemoryFs.append_text!` copies the whole file on each append (O(n²) over
many appends), and the map's string hash is unseeded FNV-1a, so keys
chosen by an attacker can collide (a JSON object from untrusted input
builds such a map).

The fixes below are each about an hour. Most are std hd code. Three are
host changes, and four are compiler lowerings or intrinsics where hd
cannot express the fast path.

## Ranked fixes

Speedups are estimates from the measurements and profiles below, on the
named benchmark. "Measured" means a prototype in user code was timed.
Kind: **std** is hd code in `lib/std`; **host** is `host/*.mjs` (and the
same change in `crates/hd_run_wasmtime`); **lowering** is `crates/hd_wasm`
or a new intrinsic body for a std declaration.

| #  | Fix | Where | Kind | Expected speedup | Cost | Moves metric |
| -- | --- | ----- | ---- | ---------------- | ---- | ------------ |
| 1  | Float printing fast path in both hosts | `host/float.mjs` `shortest`; `crates/hd_run_wasmtime/src/prim.rs` `shortest` | host | f64 print 145 → ~25 ms (6x); JSON encode -35% | 1 h | serde encode |
| 2  | Float parsing fast path in both hosts | `host/float.mjs` `parse`; `prim.rs` | host | f64 parse 80 → ~25 ms (3x, with #4 ~10x); JSON decode -25% | 1 h | serde decode |
| 3  | Integer `Display` through the runtime's integer helper | `lib/std/format.hd:371-405`, `signed_text` 417, `unsigned_text` 427 | lowering (intrinsic body) | i64 text 91 → 27 ms (3.4x, the helper's measured speed); string build ~2x | 1 h | string build, serde encode |
| 4  | Number parsers on bytes, not `split("")` | `lib/std/num.hd:364`, 413, 439, 466, 555 | std | int parse 201 → 10 ms (20x, measured) | 1 h | serde decode |
| 5  | JSON render without per-byte closures or revalidation | `lib/std/json.hd:243-329` | std | render 24 → ~10 ms (2.5x) | 1 h | serde encode |
| 6  | JSON encode straight to text, no `Json` tree | `json.hd:721-739`, new writer beside `JsonWriter` 815 | std | encode (no floats) 42 → ~12 ms (3.5x) with #5 | 1 h | serde encode |
| 7  | JSON parse on the string's bytes, zero-copy keys and texts | `json.hd:403-669` | std | parse (no floats) 35 → ~14 ms (2.5x) | 1 h | serde decode |
| 8  | `JsonReader`: lazy error paths, no per-frame `JsonObject`, no copies | `json.hd:928-1150`; `serde.hd:133-146`; `structure.hd:187` | std | from_json 31 → ~12 ms (2.5x) | 1 h | serde decode |
| 9  | Regex VM state allocated once per search | `lib/std/regex.hd:732-901` | std | find_all 312 → ~90 ms (3.5x) | 1 h | regex |
| 10 | Regex on bytes, no `Haystack` copy | `regex.hd:699-727`, every `Haystack::of` | std | is_match at start 11.6 ms → µs; find_all -20% | 1 h | regex |
| 11 | Regex literal prefilter | `regex.hd` `run`, `compile` | std | no-match scan 154 → ~5 ms; literal-led patterns 3x+ | 1 h | regex |
| 12 | String literals without a pool call per use | `crates/hd_wasm/src/rt.rs:1189` `lit_code` and its call sites | lowering | removes 22% of int parse today; 5-20% on string-heavy code | 1 h | all text |
| 13 | Linear-time substring search | `lib/std/text.hd:596-615` | std | worst case 84 → <1 ms; common case 1.2-2x | 1 h | text |
| 14 | `to_utf8` and `from_utf8` without per-byte calls | `text.hd:226-269`; `emit.rs:3536` `string_from_list` | std + lowering | text-utf8 56 → ~10 ms (5x) | 1 h | serde decode |
| 15 | Property cases: reuse the host import object, instantiate synchronously | `host/cases.mjs:90-128`, `host/core.mjs` `createCoreHost` | host | 28k → ~45k cases/s | 1 h | proptest |
| 16 | Wrapping and bit-count methods as Wasm instructions | `lib/std/num.hd:182-260`, 529-540 | lowering (intrinsic body) | sha256 128 → ~45 ms (2.8x) | 1 h | - |
| 17 | `Heap` on `List[T]` with `pop` and a hole | `lib/std/collections.hd:696-791` | std | heap 167 → ~50 ms (3x) | 0.5 h | - |
| 18 | Stable sort with one buffer and short-run insertion sort | `collections.hd:146-172` | std | sort 133 → ~55 ms (2.4x) | 1 h | runtime sort |
| 19 | Debug text of strings by runs, not per character | `lib/std/format.hd:551-560` | std | debug of a record 78 → ~45 ms; proptest show | 0.5 h | proptest |
| 20 | base64 and hex: exact buffers, table lookup, no revalidation | `lib/std/encoding.hd:41-190` | std | base64 round trip 96 → ~25 ms (4x) | 1 h | - |
| 21 | Hashing without a list per integer | `lib/std/hash.hd:24-140` | std (API) | tuple-key map op 190 → ~70 ns | 1 h | - |
| 22 | Derived `Ord` without the variant probe on a data type | `lib/std/cmp.hd:316-331` | std or derivation | derived-Ord sort 55 → ~35 ms | 0.5 h | - |
| 23 | Seeded string hash in `Map` | `crates/hd_wasm/src/map.rs:78-140` | lowering | security; no speed change | 1 h | - |
| 24 | Host path checks: one `realpath` per call at most | `host/node.mjs:63` `resolvePath`, `fsOps` 420 | host | list_dir 41 → ~25 µs, read_text 32 → ~20 µs | 1 h | host-call |
| 25 | Small quadratic spots | `fs.hd:223` (MemoryFs append), `regex.hd:938` (`expand`), `time.hd:215`, 364 | std | O(n²) → O(n) | 1 h | - |

Two items need an owner decision rather than a job; they are at the end of
this section.

### 1. Float printing fast path in both hosts

Today `format_f64` (`host/core.mjs:247`) calls `shortest` in
`host/float.mjs`, which splits the value with a `DataView` and BigInt and
runs Steele and White's free-format algorithm on BigInts. `split` and
`digits` are 47% of the f64-print profile. The Rust host
(`prim.rs:352`) does the same with its own bignum.

Both hosts already have a correct shortest-round-trip printer:
`Number.prototype.toString` in V8 and `{}`/`{:e}` for `f64` in Rust
(Grisu with a Dragon4 fallback in `core`). Their digits are the shortest that read back,
closest to the value, and the hd text's notation thresholds (fixed for a
decimal exponent in [-6, 21)) are JavaScript's own. Take the digits and
exponent from the native printer, then write hd's form (`1.0`, `-0.0`,
`1e+21`, `NaN`, `inf`). Keep the BigInt path for `f32`, where the shortest
digits at `f32` width differ, and as the reference in a property test
that compares both on random bit patterns. The crossing (about 0.9 µs)
remains, so f64 print drops to about 0.25 µs per value.

### 2. Float parsing fast path in both hosts

`parse` in `host/float.mjs:184` builds BigInts and divides for every
number. ECMA-262 requires `parseFloat` to round correctly when the text
has at most 20 significant digits, which covers almost every JSON number:
use it then, and BigInt beyond that. Rust's `str::parse::<f64>` is
correctly rounded at every length. The std side of `parse_f64` (its
grammar check through `split("")`) is fix #4.

### 3. Integer Display through the runtime helper

The runtime already has `Helper::IntToStr` (`rt.rs:311`, emitted at
`emit.rs:2559`): it writes the digits into one fresh byte array. The
compiler uses it for a few forms only. `"${n}"` with `n: usize` takes it
(27 ms for 10⁶ values); `"${n}"` with `n: i64` (91 ms), `n.to_string()`
(72 ms), a generic `Display` call, and an interpolation inside a
template (`"word-${i} "`) all reach std's `signed_text` and
`unsigned_text`, which prepend one digit string at a time
(`digit_text(...) + text`): one literal-pool call, one allocation and one
copy of the text so far per digit.

Give `Display.to_string` of each integer type an intrinsic body that calls
the helper, so every path agrees. A std hd version that fills a
`List[u8]` and converts it was measured at 58 ms, since hd has no way to
build a string's bytes in place; the helper is the fast path hd cannot
express. Integer text is 40% of the string-build profile and the largest
share of its garbage.

### 4. Number parsers on bytes

`parse_integer`, `parse_unsigned` and `parse_f64` start with
`text.split("")`, a list of one-character strings, then match each
against `"0"`…`"9"` in `digit_value` (`num.hd:555`), up to ten string
comparisons per digit, each fetching a literal from the pool. Profile:
`rt:str_eq` 27%, `rt:lit` 22%, `split` 14%. A loop over `text[i]` with
`byte - 48 < 10` parses `"1234567"` 10⁶ times in 10 ms against 201 ms
(prototype in `cases/proto-itoa.hd`). Error positions stay byte offsets,
since every scalar before the first invalid one is ASCII.

### 5. JSON render without per-byte closures or revalidation

`render` writes into a `List[u8]`: `push_text` and `render_text` walk each
string with `text.bytes()` (a closure call per byte) and `push` each byte,
then `bytes_text` runs `string::from_utf8` over the finished output to
validate bytes the writer made from valid strings. In the encode profile
`from_utf8` is 10%, the `bytes` closures 8%, `render_text` 6.5%.

Collect pieces instead: for each string, scan for bytes that need an
escape and push `bytes_slice` runs between them (no copy), the escapes as
literals, and join once with `concat_all`, as `process_escapes` in
`text.hd:54` already does. Numbers go through #3. The output needs no
validation. json.hd may declare the `bytes_at`, `bytes_len`,
`bytes_slice` and `concat_all` intrinsics locally, as `format.hd` does
for `string_from_bytes`.

### 6. JSON encode straight to text

`encode` is `"${to_json(value)}"`: it builds the `Json` tree first. The
`JsonWriter` pushes every value with its key onto two stacks, and each
close copies the frame's items into new lists, pops them one at a time,
and builds a `JsonObject` whose `Map` index hashes every key
(`object_of`, `json.hd:917`). `to_json` alone is 29% of encode time.

Add a second `Serializer`, a text writer, that appends pieces (#5) as the
walk reaches them: `begin_variant` writes `{`, `member` writes the quoted
key, and so on. This is serde_json's `Serializer` design. `to_json` and
`Json` stay for callers that want the tree.

### 7. JSON parse on the string's bytes

`parse` first copies the text into a `List[u8]` with `to_utf8()` (a
closure call and a push per byte). `parse_string` pushes each byte into a
buffer and validates it again with `from_utf8`, although the input was a
valid string. Each object builds a `JsonObject` with a `Map` index. Read
the string through `bytes_at`; return `bytes_slice` of the input for a
string with no escape (zero-copy, as serde_json borrows `&str`), and
build only escaped strings. For objects up to about eight members, look
keys up by a linear scan and build the hash index only past that.

### 8. JsonReader: lazy paths and no copies

For every member `JsonReader.member` builds `"${frame.base}.${m.name}"`,
and for every item `next_item` builds `"${frame.base}[$index]"` (an
integer to text and two concatenations), only to have a path ready for
an error. `begin_list` allocates a `JsonObject::new()` (a map) for every
list frame. `begin_map` copies all keys and values into two new lists.
`DeserializeSource.variant` builds a `List[VariantInfo]` for every value
(`serde.hd:133`), and `next` looks each member up with `Members.at`, a
linear scan (`structure.hd:187`), so a record of k fields costs O(k²).

Keep the path as a stack of keys and indices and render it in `fail`;
store `fields: JsonObject?`; iterate the object in place; index members
by position. A streaming text deserializer, which skips the tree
entirely, is a later job: the `Source` protocol already lets a reader
return members in text order through `Members.find`.

### 9. Regex VM state allocated once per search

`run` (`regex.hd:771`) allocates `marks`, a slot table of
2 × program size × slot count, the scratch slots and the frame stack on
every call, and `find_all` calls it once per match. Inside, it allocates a
new `next` list at every text position (line 792). `add_thread` pushes a
`Frame` record per epsilon step (`push_frame`, a struct allocation).
Profile: `run` 33%, `add_thread` 21%, `push_frame` 7%, GC 10%.

Allocate the VM once per `find_all`/`captures_all` call and reset it per
match; keep two thread lists (Russ Cox's sparse sets) and swap them;
hold the frame stack as two `List[i64]` (pc and slot, with -1 for "none")
instead of records; store slots as `i64` with -1 for an unset slot instead
of `usize?`. This is the Pike VM of RE2 and Rust's `regex-automata`.

### 10. Regex on bytes

Every search starts with `Haystack::of(text)`: a `List[char]` and a
`List[usize]` of offsets for the whole text, built through
`char_indices()`. `is_match` on 1.1 MB with a match at byte 0 takes
11.6 ms per call, all of it this copy. Decode the scalar at the current
byte offset inside `run` (std's `scalar_at` and `scalar_end` logic) and
report byte offsets directly. `Pattern::of` may stay, since patterns are
small.

### 11. Regex literal prefilter

The VM steps every thread over every character, so a search for `zzz` in
1.1 MB with no match takes 51 ms (Node: 0.13 ms). Compute at compile time
the set of possible first bytes (or a required literal prefix) and skip
ahead with a byte scan to the next candidate before starting threads.
Rust's `regex` does this with `memchr`-based prefilters; a lazy DFA is a
larger later step.

### 12. String literals without a pool call per use

Each evaluation of a string literal calls `rt:lit`, which loads the pool
global, reads the cell, tests it for null and returns it. In the
int-parse profile `rt:lit` is 22% of all time; it is also why
`digit_text` dominates integer printing. Fill the pool at start (the
literals are in a passive data segment already) or cache each literal in
its own global so a use is one `global.get`.

### 13. Linear-time substring search

`byte_find` tries every start and compares byte by byte through the
`bytes_at` intrinsic, which checks bounds twice per byte. A needle of 1001
bytes against 100 KB of `a` takes 84 ms. Use Two-Way (Crochemore and
Perrin; Rust's `str::find`, glibc `memmem`), or Go's approach: scan for
the needle's rarest or first byte, then verify, and switch to Rabin-Karp
after too many false starts. `split`, `replace`, `count`, `contains` and
`split_once` all use `byte_find`.

### 14. UTF-8 conversions without per-byte calls

`to_utf8` pushes each byte from the `bytes()` closure; `from_utf8`
validates with list indexing per byte, then `string_from_bytes` copies the
list element by element (`emit.rs:3536`) instead of with `array.copy`.
Text-utf8 is 40x Node. A `string_to_bytes` intrinsic beside
`string_from_bytes` (one `array.copy`), an ASCII fast loop in
`from_utf8`, and `array.copy` in `string_from_list` remove most of it.
JSON decode and every `Hash for string` call `to_utf8`.

### 15. Property cases: cheaper per-case setup

Each generated case runs in a fresh instance (`cases.mjs:116`) and, before
that, a fresh host: `createHost` builds every provider and the whole
import object again. In the worker profile, `instantiate` is 20%, host
JavaScript (import building, providers, `put`) 27.5%, GC 9%, and the Wasm
work itself only 24.5%. Build the import object once per test and swap
the per-case state behind it; call `new WebAssembly.Instance` (synchronous)
on the compiled module; skip `rmSync` of a temporary directory the case
never made.

### 16. Wrapping and bit-count methods as instructions

`wrapping_add` (`num.hd:182`) adds by a loop of carries, up to 64
iterations; `count_ones` and `leading_zeros` test bits one at a time;
`int_min` shifts a bit up to the sign on every call, and `checked_add`,
`checked_sub`, `checked_mul`, `saturating_*` and `rotate_*` call it. In
`sha256`, `wrapping_add` is 53% of the time. Wasm has `i32.add`,
`i64.add` (wrapping), `popcnt` and `clz`. Give these methods intrinsic
bodies; their signatures stay plain hd (the rule for intrinsics). `u32`
could also be written in hd with a `u64` sum, but `u64` and `i64` cannot.

### 17. Heap on List[T]

`Heap` stores `List[T?]` because, as its comment says, the list had no
`pop` then; it has one now. `element` matches the option on every read,
and `less` calls `element` twice. `less` and `element` are 67% of the
heap profile. Store `List[T]`, use `pop`, and sift with a hole (move the
element down once, as Rust's `BinaryHeap` does) instead of swapping.

### 18. Stable sort with one buffer

`sorted_by` is a top-down merge sort that allocates `left`, `right` and
`merged` lists at every level and copies a one-element list through
`filter` with a closure at the leaves. 200,000 `i64` take 44 ms. Use a
bottom-up merge sort over one scratch list of n elements, with insertion
sort for runs under 16 to 32 (the shape of Rust's former stable sort and
of TimSort, which detects existing runs too). This passes the runtime
metric today (1.11x Node, whose comparator sort is slow); the gain shows
against a native sort.

### 19. Debug text of strings by runs

`escape_text` maps every character to a string through `escape_char` and
`scalar_text` (a `List[u8]` and a string per character), then joins them.
It is 40% of `debug` on a record with a string field. Copy runs of plain
characters as slices and escape only `\`, the quote, `$` and control
characters. The property runner calls `debug` on every case's input.

### 20. base64 and hex

Encoders push one byte at a time into a `List[u8]`, pick each symbol
through a chain of comparisons, and validate the ASCII output with
`from_utf8`. Decoders first convert the text to a `List[i32]` through the
`bytes()` closure (`ascii_codes`). Use the alphabet as a string read with
`bytes_at`, build the output with `string_from_bytes` (no validation),
and decode straight from `bytes_at` with a 256-entry table.

### 21. Hashing without a list per integer

`Hash` for every integer builds a `List[u8]` of its bytes
(`signed_bytes`, `unsigned_bytes`) and calls `write` through
`dyn Hasher`; `Hash for string` copies the string with `to_utf8`. A map
whose key is a tuple or a derived record goes through this path: 190 ns
per operation against 22 ns for an `i64` key. Add fixed-width writes to
`Hasher` (`write_u64`, as Rust's `Hasher` has, with a default that calls
`write`) and hash strings through `bytes_at`. This is a std API addition,
to record in docs/future-work/STDLIB_CALLS.md.

### 22. Derived Ord without the variant probe

Derived `Ord.cmp` and `PartialOrd.partial_cmp` walk `other` once with
`VariantIndex` just to read its variant, then walk `self`. For a data type
there is one variant, so the first walk is waste. Derived Eq is 3.2x a
hand-written comparison and derived Ord sorting 2x; skipping the probe
when `Variant.info.of_data` is true removes one of the two walks.

### 23. Seeded string hash in Map

The map hashes a string key with 32-bit FNV-1a and a fixed mix
(`map.rs:78-140`). Keys chosen to collide make probing linear, and JSON
parsing builds a map from untrusted keys. Rust seeds SipHash-1-3 per
process; Go seeds its hash per map. The map iterates in insertion order,
so a per-run seed changes no observable order. A seed from the host at
start is enough.

### 24. Host path checks

The `list_dir!` metric measures the file system more than serde. A call
costs `realpath` (9 µs here), `readdir` (15 µs) and an `lstat` per entry
(0.8 µs each) before the reply is encoded; the encoding is about 3% of
the profile. `resolvePath` runs `realpathSync.native` on each path for
the grant check, and the provider then resolves the path again. Resolve
each path once per call and pass the resolved path to the operation. A
`realpath` cache is unsafe (a symbolic link can change between calls), so
the floor stays around 20 µs: the 5 µs goal cannot be met with a real
`readdir`. The metric should time a host call that returns a record list
without I/O if it means to measure the serde boundary.

### 25. Small quadratic spots

- `MemoryFs.append_text!` (`fs.hd:223`) copies the whole file into a new
  list on every append.
- `expand` (`regex.hd:938`) builds the replacement with `out = out + ...`.
- `to_rfc3339` (`time.hd:215`) joins about fifteen pieces with `+`, and
  `zero_padded` (364) prepends one `"0"` at a time.
- `DebugWriter.margin` (`format.hd:99`) rebuilds the indent per item.

Each is small in practice; a list of pieces and one `join` fixes each.

### Owner questions

1. **Float text in hd.** The spec names `format_f64`, `format_f32` and
   `parse_f64` as host primitives (spec/std/README.md, standard library
   primitives). Fixes #1 and #2 keep that and stop near 0.25 µs per
   value, the crossing. Ryu or Dragonbox for printing and Eisel-Lemire
   for parsing, written in hd, would cost about 30 ns with no crossing,
   but they move these primitives out of the spec's list and need
   64x64-bit multiplication from 32-bit halves. Recommendation: do #1 and
   #2 now, and decide this after the JSON fixes land.
2. **Many property cases per instance.** Even with #15, a property pays an
   instantiation per case. Running the generated cases of one property in
   one instance, with a fresh instance only to shrink or replay a
   failure, would reach the 100k cases/s goal. It weakens isolation
   between cases (a property that mutates module state could see an
   earlier case), which the runner's comment and the `show-before-body`
   rule assume. A related, smaller change: `prop_case` computes
   `debug(value)` for every passing case
   (r-std-testing.runner.show-before-body); the runner could instead
   replay the recorded draws to show a failing input.

## Inventory

Complexities are in the input size n (bytes, elements or digits); m is a
needle or pattern size. "Allocations" counts heap objects per element on
the hot path.

### Text (`lib/std/text.hd`)

| Operation | Today | Cost | Production standard | Verdict |
| --------- | ----- | ---- | ------------------- | ------- |
| `len`, `slice`, `bytes_slice` | intrinsics on a (bytes, offset, length) view (`emit.rs:3703`) | O(1), no copy | same (Go, Rust slices) | good |
| `+`, interpolation | `bytes_concat`, exact-size copy (`emit.rs:1310`) | O(n) | same | good |
| `join`, `StringBuilder.build` | `concat_all`: sum lengths, one copy (`emit.rs:3585`) | O(n), 1 allocation | same | good |
| `StringBuilder.push` | push a span onto `List[string]` (575) | 1 list slot per piece | Rust `String::push_str` grows one buffer | fine |
| `replace`, `split`, `count`, `contains`, `find` | `byte_find` (606): try every start | O(n·m) worst | Two-Way (Rust, glibc), IndexByte + Rabin-Karp (Go) | naive, fix #13 |
| `chars`, `char_indices`, `bytes` | `Iterator::from_fn` closures (174, 188, 203) | 1 indirect call per item | Rust inlines `Chars`; JS engines inline | fair |
| `to_utf8` | push each byte from `bytes()` (226) | O(n), n closure calls | one memcpy | naive, fix #14 |
| `from_utf8` | branchy decode per scalar with list indexing, then element copy (236) | O(n), 2 passes | ASCII fast path + validate (Rust `run_utf8_validation`), memcpy | slow, fix #14 |
| `repeat` | doubling (431) | O(n) bytes | same | good |
| `trim`, `split_whitespace`, `lines` | scalar walk with White_Space test (359-470) | O(n) | same | good |
| `==`, `<` on strings | byte loop through `bytes_at` (843, 855; `cmp.hd:157`) | O(n), 2 bounds checks per byte | memcmp | fair |
| `is_alphabetic` and friends | binary search over a hex range table in a string (690) | O(log r), parses hex per probe | Rust: binary search over a const table | fair |
| `lower`, `upper` | host call (`string_lower`) | crossing + JS `toLowerCase` | native tables | fair |

### Numbers (`num.hd`, `format.hd`, host)

| Operation | Today | Cost | Production standard | Verdict |
| --------- | ----- | ---- | ------------------- | ------- |
| integer `Display` (generic, method, `i64`) | `signed_text`/`unsigned_text`: prepend a digit string per digit (`format.hd:417-447`) | O(d²) bytes, d allocations + d literal calls | itoa: two digits per step into a fixed buffer (Rust `itoa`, Go `strconv`) | naive, fix #3 |
| integer `Display` (`usize` interpolation) | runtime `IntToStr` helper (`rt.rs:311`) | O(d), 1 allocation | same | good |
| `f64`/`f32` `Display` | host `format_f64`: BigInt Steele-White (`float.mjs`), bignum in `prim.rs:352` | ~1.45 µs, crossing | Ryu, Dragonbox, Grisu-Exact | slow, fix #1 |
| `to_fixed` | host BigInt (`format_f64_fixed`) | crossing | Ryu printf | fair (rare) |
| `parse_i32`/`i64`/`u*` | `split("")` + string match per digit (364, 413, 555) | ~10 allocations and comparisons per digit | byte loop | naive, fix #4 |
| `parse_f64` | grammar on `split("")`, then host BigInt division (439, `float.mjs:184`) | ~0.8 µs, crossing | Eisel-Lemire + slow path (Rust `dec2flt`, fast_float) | slow, fixes #2, #4 |
| `wrapping_add`, `wrapping_sub` | carry loop (182) | up to 64 iterations | 1 instruction | naive, fix #16 |
| `count_ones`, `leading_zeros`, `rotate_*` | bit loops (214-260) | up to 64 iterations | `popcnt`, `clz`, `rotl` | naive, fix #16 |
| `checked_*`, `saturating_*` | bounds from `int_min` loop per call (133-210, 529) | ~64 iterations per call unless folded | constants | naive, fix #16 |

### Collections (`collections.hd`, `iter.hd`, compiler)

| Operation | Today | Cost | Production standard | Verdict |
| --------- | ----- | ---- | ------------------- | ------- |
| `List.push` | grow to 2n + 4 (`emit.rs` `grow`) | amortized O(1) | doubling | good |
| `insert`, `remove_at` | shift one at a time (31, 46) | O(n) | memmove | fine |
| `sorted`, `sorted_by` | top-down merge sort, 3 new lists per level (146) | O(n log n), ~3n log n element copies, ~2n allocations | TimSort / driftsort with one buffer, pdqsort for unstable | slow, fix #18 |
| `sorted_by_key` | key per element, then `sorted_by` on pairs (209) | as above | same idea (Rust `sort_by_cached_key`) | good |
| `windows` | slice copy per window (284) | O(n·k) | views (Rust `windows` borrows) | inherent to `List[List[T]]` |
| `Map`, `Set` | insertion-ordered entries + linear-probing index at load ≤ 0.5, stored hashes (`map.rs`) | O(1) expected | CPython compact dict, SwissTable | good; unseeded, fix #23 |
| map hash of `string` | FNV-1a 32 per byte + mix (`map.rs:78`) | O(n) | SipHash-1-3, foldhash, aHash | fine, unseeded |
| map hash of other keys | `hash_of` through `Hash` impls (`map.rs:115`) | list per integer field | fixed-width `write_u64` | slow, fix #21 |
| `Deque` | ring buffer of `T?` with doubling (482, 570) | amortized O(1) | same (Rust `VecDeque`) | good |
| `Heap` | binary heap over `List[T?]` (696) | O(log n), option match per read | binary heap with hole (Rust) | slow, fix #17 |
| `counts`, `group_by` | map updates (364, 224) | O(n) | same | good |
| iterator adapters | closure per stage per item (`iter.hd:24-140`) | 1 indirect call per stage | fused, inlined adapters | fair |

### JSON and serde (`json.hd`, `serde.hd`, `structure.hd`)

| Operation | Today | Cost | Production standard | Verdict |
| --------- | ----- | ---- | ------------------- | ------- |
| `encode` | value → `Json` tree via `JsonWriter` (815), then `render` to `List[u8]` (243), then `from_utf8` (326) | per value: 1 `Json`, 2 stack slots, per object: a map index; per byte: a closure call and a push; one full revalidation | serde_json: walk writes text into one buffer | slow, fixes #5, #6 |
| string escaping | `render_text`: if-chain per byte, push per byte (298) | O(n) calls | escape table, copy unescaped runs (serde_json `ESCAPE`) | slow, fix #5 |
| `parse` | `to_utf8` copy (404), recursive descent on `List[u8]` | 1 extra copy of the input | recursive descent on the input bytes | fair, fix #7 |
| strings in `parse` | push each byte to a buffer, then `from_utf8` (499) | 2 copies + validation per string | borrow when unescaped | slow, fix #7 |
| objects in `parse` | `JsonObject` = list + `Map` index (27) | 1 hash insert per member | serde_json `Map` (BTreeMap/IndexMap); linear for small | fair |
| numbers in `parse` | integers exact in std; floats copied to a list, then `parse_f64` (657) | crossing per float | Eisel-Lemire | slow, fix #2 |
| `from_json` | walk the tree with `JsonReader` (939) | per member: a path string, a map lookup; per list: a `JsonObject::new()`; per map: copies | serde_json `Deserializer` streams from text, paths only on error | slow, fix #8 |
| derived `Deserialize` | `Source.next` → `Members.at` linear scan (`structure.hd:187`); `variant` builds a list per value (`serde.hd:133`) | O(k²) per record | index by position | slow, fix #8 |
| derived `Serialize` | `Structure::walk` with field handles (`serde.hd:86`) | handle calls per field | same pattern (serde derive) | fine |

### Regex (`regex.hd`)

| Operation | Today | Cost | Production standard | Verdict |
| --------- | ----- | ---- | ------------------- | ------- |
| compile | recursive descent to a tree, then a program (74, 628) | O(m) | same (RE2) | good |
| search engine | Pike VM, at most one thread per instruction (771) | O(n·m), no backtracking | Pike VM for captures; lazy DFA and prefilters for search (RE2, Rust `regex`) | right algorithm, no fast paths, fix #11 |
| per-call setup | haystack copy (705) + VM tables (774-786) per `run` | O(n) per `find`; O(m·slots) per match | reuse a cache per regex (Rust `Cache`) | naive, fixes #9, #10 |
| per-step work | new `next` list per position, `Frame` record per epsilon step (792, 896) | allocations per character | sparse sets, int stack | slow, fix #9 |
| classes | linear scan of class items (965) | O(items) | byte classes / ranges table | fine |
| `replace`, `replace_all` | pieces + one join (157); `expand` with `+` (938) | O(n) | same | fine, #25 |

### Hashing, digests, encodings, testing, others

| Operation | Today | Cost | Production standard | Verdict |
| --------- | ----- | ---- | ------------------- | ------- |
| `DefaultHasher` | 64-bit FNV-1a, multiply by 32-bit halves (`hash.hd:165`) | ~8 ops per byte | FNV is fine; foldhash/xxh3 faster | fine |
| `Hash` for integers, strings | list per integer, `to_utf8` per string (`hash.hd:24-140`) | allocations per hash | fixed-width writes | slow, fix #21 |
| derived `Hash`, `Eq`, `Ord` | walkers over field handles (`hash.hd:177`, `cmp.hd:282-331`) | 2-3x hand-written | generated direct code (Rust derive) | fair, fix #22 |
| `sha256` | FIPS 180-4 rounds with generic `wrapping_add`; padding copies byte by byte (`digest.hd:9`, 76) | 8.2 MB/s | ~200-400 MB/s in portable Wasm | slow, fix #16 |
| `hex_*`, `base64_*` | push per byte, symbol by comparisons, revalidation (`encoding.hd`) | ~30 MB/s round trip | table lookup into a sized buffer | slow, fix #20 |
| `SeededRandom` | xoshiro128** on masked `u64` (`random.hd:24`) | a few ops per draw | same | good |
| `Rng.below` | rejection with a `%` threshold (165) | 1-2 divisions | Lemire's multiply-shift | fine |
| `fill` | 8 bits per 32-bit draw (60) | 4x the draws needed | use all 32 bits | fine |
| property generation | `Choices` draws, each recorded by a host call (`testing.hd:117-131`) | 1 crossing per draw | batch the record | fair |
| property cases | fresh instance + host per case (`cases.mjs:116`); `debug(value)` per case (`testing.hd:546`) | ~100 µs CPU per case | many cases per process (QuickCheck, proptest, Hypothesis) | slow, #15 and owner question 2 |
| `Debug` | builder pushes parts, one join (`format.hd:31`); strings escaped per char (551) | O(n) pieces | `fmt::Formatter` | fair, fix #19 |
| `Path` | split on `/`, filter components (`path.hd:70`) | O(n) | same | good |
| dates | Hinnant's `civil_from_days` (`time.hd:333`) | O(1) | same | good |
| RFC 3339 text | concatenations (`time.hd:215`) | ~15 allocations | one buffer | fine, #25 |
| `MemoryFs` append | copy whole file per append (`fs.hd:223`) | O(n²) over appends | append to a buffer | naive, #25 |
| host fs calls | grant check `realpath`, then the syscall (`node.mjs:63`, 420) | ~20-40 µs | one resolution per call | fair, fix #24 |

## Measurements

Release build; times are the median of three runs of the timed region,
taken at a 1-minute load of 2.69 to 2.76 (the final full run). Node runs
the same work in its own process after a warm-up run. "Text" is
`word-0 word-1 ...` of 100,000 words, 1.09 MB.

| Case | Work | hd ms | Node ms | hd / Node |
| ---- | ---- | ----: | ------: | --------: |
| json-encode | 200 orders (45,488 B), 50 encodes | 69 | 4.0 | 17.3 |
| json-encode, no floats | same with integer prices | 42 | - | - |
| json `to_json` only | 50 trees | 20 | - | - |
| json render only, floats / no floats | 50 renders | 55 / 24 | - | - |
| json-decode | 50 decodes | 91 | 7.1 | 12.8 |
| json-decode, no floats | 50 decodes | 63 | - | - |
| json `parse` only, floats / no floats | 50 parses | 57 / 35 | - | - |
| json `from_json` only | 50 reads | 31 | - | - |
| num-f64-print | 100,000 `"${x}"` | 145 | 7.2 | 20.1 |
| num-f64-parse | 100,000 `parse_f64("12345.678")` | 80 | 2.8 | 28.6 |
| num-int-print, `usize` | 10⁶ `"${i}"` | 27 | 9.4 | 2.9 |
| num-int-print, `i64` | 10⁶ `"${i}"` | 91 | - | - |
| num-int-print, method | 10⁶ `i.to_string()` | 72 | - | - |
| num-int-print, prototype hd itoa | 10⁶ values | 58 | - | - |
| num-int-parse | 10⁶ `parse_i64("1234567")` | 201 | 28.6 | 7.0 |
| num-int-parse, prototype byte loop | 10⁶ values | 10 | - | - |
| text-build | 5 × `StringBuilder` of 200,000 `"word-${i} "` | 115 | 41.9 | 2.7 |
| text-build, constant piece | 5 × 200,000 `"word-12345 "` | 24 | 10.8 | 2.2 |
| text-split | 20 × `split(" ")` of text | 63 | 39.6 | 1.6 |
| text-replace | 20 × `replace("word", "w")` | 80 | 56.3 | 1.4 |
| text-chars | 5 × iterate `chars()` of text | 40 | 15.3 | 2.6 |
| text-utf8 | 5 × `to_utf8` + `from_utf8` of text | 56 | 1.4 | 40 |
| text-find-worst | 3 × find `a`×100+`b` in `a`×100,000 | 26 | 0.2 | 130 |
| text-find-worst, m = 1001 | 1 search | 84 | - | - |
| regex-findall | 3 × `[a-z]+-[0-9]+` over text | 312 | 23.6 | 13.2 |
| regex-captures, 2 groups | 1 × `captures_all` over text | 181 | - | - |
| regex-captures, 8 groups | 1 × `captures_all` over text | 390 | - | - |
| regex-ismatch-early | 100 × `is_match("word")`, match at 0 | 1159 | <0.1 | >10,000 |
| regex-nomatch | 3 × `is_match("zzz")` over text | 154 | 0.4 | 385 |
| regex `(a\|aa)*b` | on `a`×4,000 / `a`×8,000 | 3 / 5 | - | linear |
| coll-sort-int | 3 × sort 200,000 `i64` | 133 | 120 | 1.1 |
| coll-sort-str | 3 × sort 100,000 strings | 136 | 56.4 | 2.4 |
| coll-sort-by | 3 × `sorted_by` 200,000 `i64` descending | 132 | - | - |
| derived Ord sort / hand comparator | 100,000 two-field records | 55 / 27 | - | - |
| derived Eq / hand comparison | 10⁶ record comparisons | 16 / 5 | - | - |
| coll-map-int | 3 × 200,000 inserts + lookups | 28 | 30.6 | 0.9 |
| coll-map-str | 3 × 200,000 string-key inserts + lookups | 52 | 44.7 | 1.2 |
| coll-map-tuple | 3 × 20,000 `(i32, i32)`-key inserts + lookups | 24 | 8.5 (string keys) | 2.8 |
| coll-set | 3 × 200,000 inserts + lookups | 24 | - | - |
| coll-heap | 3 × 200,000 push + pop | 167 | - | - |
| coll-deque | 3 × 200,000 push_back + pop_front | 22 | - | - |
| misc-sha256 | 1 MiB | 128 | 0.4 (native) | 320 |
| misc-base64 | 3 × encode + decode 1 MiB | 96 | 0.8 (native) | 120 |
| misc-wrapping | 10⁶ `wrapping_add` | 12 | - | - |
| misc-hashof-str | 100,000 `hash_of(string)` | 23 | - | - |
| misc-debug | 100,000 `debug(record)` | 78 | 50.5 (`util.inspect`) | 1.5 |

Host calls, run as `hd FILE.wasm` (load 2.5): `list_dir!` of 4 entries
41 µs per call, `read_text!` of 25 bytes 32 µs. Raw Node on the same
directory: `realpathSync.native` 9.0 µs, `readdirSync` 15.4 µs, four
`lstatSync` 3.1 µs, `readFileSync` 12.7 µs.

Property suite of the metric (3 properties, `--seed 7`, load 2.6): 2,100
cases each in 324 to 328 ms, 100 cases each in 118 to 122 ms, so 6,000
extra cases in about 205 ms: 29k cases/s, as the metric found.

**Asymptotic cases.** Substring search grows with n·m (26 ms at m = 101,
84 ms at m = 1001 for one third the searches). `is_match` grows with the
text length even when the match is at byte 0. Regex search itself stays
linear on patterns that make backtracking engines explode
(`(a|aa)*b`: 3 ms at 4,000 and 5 ms at 8,000 bytes). Map operations stay
O(1) for integer and string keys. No quadratic string building was found
in `join`, `replace`, `repeat` or `StringBuilder`; the prepend loops in
integer text, `zero_padded` and `margin` are quadratic in short inputs
only.

## Root causes of the metric failures

### JSON encode 0.05x and decode 0.08x

Profiles of the metric document (`json-encode`, `json-decode`, and their
no-float variants):

- **Floats: 39% of encode, 31% of decode.** Each of the 600 prices per
  document crosses to the host, where BigInt arithmetic prints or parses
  it (`float.mjs` `split`, `digits`, `parse`). Root cause: host primitive
  with an arbitrary-precision algorithm on every value. Fixes #1, #2, #4.
- **The tree: 29% of encode.** `to_json` builds a `Json` value per field
  and a map index per object before any text is written. Root cause:
  algorithm in std (two passes through an intermediate). Fix #6.
- **Per-byte output: most of the remaining render time.** `from_utf8`
  revalidation of the output (10% of the no-float encode profile), the
  `bytes()` closures (8%), push per byte, integer text through
  `signed_text`. Root cause: std code, with the literal-pool call
  (lowering) under integer text. Fixes #3, #5, #12.
- **Decode input handling: about a third of parse.** `to_utf8` copy (6%),
  `from_utf8` on each string (6%), per-byte buffer pushes in
  `parse_string` (5%), a map index per object. Fix #7.
- **Tree reading: 34% of decode.** Path strings for every member and item,
  a `JsonObject::new()` per list, copies in `begin_map`, a linear member
  scan, a `List[VariantInfo]` per value. Fix #8.
- **GC: 7% to 9%** of both, from all of the above.

With #1 to #8, encode should land near 0.25 ms per document (0.3x Node)
and decode near 0.4 ms (0.2x). Reaching 0.5x needs the float work in hd
(owner question 1) and a streaming decoder.

### String build 0.19x

The metric pushes `"word-${i} "` with `i: i64`. Integer text is 40% of
the profile (`digit_text` 22%, `unsigned_text` 18.5%) and GC 17%, most of
it the per-digit strings. `StringBuilder.push` and `build` are 13%; they
are fine. Root cause: std's integer `Display` prepends one digit string at
a time and fetches each digit from the literal pool, while the compiler
uses its fast `IntToStr` helper only for some forms (`usize`
interpolation). Fix #3, then #12. Expected: about 2x, so near 0.4x on the
metric.

The metric's two sides also differ: hd is timed over 1,000 and 201,000
words with the difference taken, while Node times one build of 100,000
words. On equal work (`text-build` above) hd is at 0.36x Node today.

### Regex find_all 0.05x

Profile: `run` 33%, `add_thread` 21%, GC 10%, `push_frame` 7%,
`Haystack.of` 5% plus `char_indices` 5%. Root cause: std algorithm
details, not the engine choice. The VM's tables are allocated for every
match, a new thread list for every character, and a record for every
epsilon step; the haystack is copied into two lists first; and nothing
skips the text where no match can start. Fixes #9, #10, #11; the three
together should reach about 0.3x on this pattern and much more on
literal-led patterns.

### Property cases 28k/s

Per case about 100 µs of CPU in each test process. Worker profile:
host JavaScript building a fresh import object and providers 27.5%,
`WebAssembly.instantiate` 20%, GC 9%, Wasm 24.5% (generation, plus
`debug(value)` of every input and a host `record` call per draw). Root
cause: host boundary and runner design (a fresh instance and host per
case), not std. Fix #15 gives about 1.6x; the goal needs many cases per
instance (owner question 2). Fixes #16 and #19 trim the Wasm share.

### Serde boundary (list_dir!) 40 µs, and Fs read_text! 30 µs

Profile: `realpath` 27%, `readdir` 26%, `open` and `close` 21%, `lstat`
5%; the reply's encoding (`enc`, `leb`, `put`) is about 3%. Root cause:
system calls in the host, including a `realpath` for the grant check on
every call. Serde is not the cost. Fix #24 removes the duplicate
resolution; the 5 µs target is below the cost of one `readdir` on this
machine, so the metric needs a host call without I/O to measure the
boundary itself.

## Method notes

- Programs time themselves with `now()` (millisecond resolution) over
  loops of 15 ms or more, so process start-up and compilation are out of
  the numbers.
- Wasm frame names come from the release build's `hd.names` section;
  runtime helpers have no names in a release build, so the helper names
  above (`rt:str_eq`, `rt:lit`) come from a dev build of the same program.
- Node references are idiomatic JavaScript, not tuned code. For SHA-256
  and base64, Node calls native code, so those ratios overstate the gap
  to a good Wasm implementation, which would be 20-40x faster than hd
  today rather than 120-320x.
