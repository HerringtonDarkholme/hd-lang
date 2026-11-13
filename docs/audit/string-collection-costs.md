# String, List and Map release costs

Measured 2026-11-02 for #1208 (the short-string cases for #1244 have their own section), on compiler commit `caece8ae2` from `origin/main`.
The machine was arm64, Node v24.19.0, with load averages 13.16, 9.24 and 7.86 at the end of
the run.

## Method

Each row is a separate `.hd` and hand-written `.js` program in `bench/micro`. The hot loop
does one named operation; setup and a result check keep the work observable. List rows vary
the element type. Map rows vary the key type and keep an `i32` value, so key hashing is not
mixed with string-value mutation. `map-get-key-string` is also the string-key lookup row.

`node --experimental-strip-types bench/micro/costs.ts` built every hd program in release,
compiled its Wasm once, and timed seven fresh instances from `hd.init` through completion.
Each JavaScript program ran once to warm up, then seven timed calls in its own Node process.
The table gives the minimum, as the existing micro runner does. Wasm compilation and
instantiation are outside the hd timer. `HD_MICRO_RUNS=1` gives a fast correctness run, and
case names after the script select a subset.

The `counts` column is the release `counts` stage view for the marked target expression.
“none” means no retain, release, free or copy operation survived at that source site; it does
not mean the called std implementation allocates nothing. A loop suffix says that the shown
operation runs for each element or entry.

## Update, 2026-11-13

Rerun at `a45fdfc17` with the same runner and seven runs, load average
about 15:

| case | hd ms | JS ms | hd/JS | Wasm B |
| --- | ---: | ---: | ---: | ---: |
| `string-add-build` | 0.5 | 0.2 | 2.40 | 4,339 |
| `string-add-assign-build` | 0.5 | 0.2 | 2.44 | 4,339 |
| `string-builder-push` | 7.5 | 33.9 | 0.22 | 6,396 |
| `list-for-borrow-i32` | 11.9 | 26.3 | 0.45 | 4,224 |
| `list-for-borrow-string` | 3.1 | 6.6 | 0.47 | 4,727 |
| `list-mut-i32` | 4.1 | 8.1 | 0.51 | 4,343 |
| `list-mut-string` | 13.9 | 13.8 | 1.00 | 5,007 |

`s = s + t` and `s += t` now append in place when `s` holds the only
count of its text: `rt:str_append` (`crates/hd_wasm/src/rt.rs`) grows a
uniquely held buffer in place, or moves the bytes to one twice as long,
so a building loop is linear. A borrowing `for x in &xs` loop costs no
count per element: an element reference counts nothing (#1336 phase 10).
The counts column of the tables below was not re-read. The other rows
below are the 2026-11-02 snapshot.

## Strings

| case | hd ms | JS ms | hd/JS | counts per target operation | Wasm B |
| --- | ---: | ---: | ---: | --- | ---: |
| `string-add-build` | 290.1 | 0.2 | 1473.65 | release string | 3,494 |
| `string-add-assign-build` | 288.1 | 0.2 | 1585.76 | release string | 3,494 |
| `string-builder-push` | 15.7 | 33.5 | 0.47 | none | 5,081 |
| `string-slice` | 12.9 | 19.1 | 0.68 | none | 2,314 |
| `string-split` | 52.6 | 10.0 | 5.26 | none | 7,722 |
| `string-trim` | 113.4 | 24.8 | 4.58 | release string | 2,891 |
| `string-eq-short` | 78.6 | 16.5 | 4.75 | none | 5,496 |
| `string-eq-long` | 77.3 | 16.5 | 4.68 | none | 5,595 |
| `map-get-key-string` | 157.0 | 29.9 | 5.24 | none | 5,938 |
| `string-chars` | 70.8 | 71.0 | 1.00 | release `Chars[text]` | 2,506 |
| `string-format-int` | 28.9 | 34.8 | 0.83 | none | 3,593 |

## List

| case | hd ms | JS ms | hd/JS | counts per target operation | Wasm B |
| --- | ---: | ---: | ---: | --- | ---: |
| `list-get-i32` | 8.3 | 8.2 | 1.02 | none | 3,758 |
| `list-get-string` | 24.7 | 13.9 | 1.78 | none | 4,455 |
| `list-contains-i32` | 16.1 | 20.3 | 0.79 | none | 3,957 |
| `list-contains-string` | 114.8 | 32.3 | 3.56 | none | 5,052 |
| `list-for-value-i32` | 9.8 | 26.0 | 0.38 | none | 3,496 |
| `list-for-value-string` | 2.8 | 6.7 | 0.42 | none | 4,013 |
| `list-for-borrow-i32` | 125.4 | 26.1 | 4.81 | retain + release `&i32` per element | 4,008 |
| `list-for-borrow-string` | 32.0 | 6.7 | 4.77 | retain + release `&string` per element | 4,248 |
| `list-push-i32` | 3.4 | 10.6 | 0.32 | none | 3,687 |
| `list-push-string` | 8.4 | 11.4 | 0.74 | none | 4,202 |
| `list-pop-i32` | 11.5 | 13.5 | 0.85 | none | 4,153 |
| `list-pop-string` | 24.3 | 14.3 | 1.71 | none | 4,882 |
| `list-field-write` | 5.5 | 10.8 | 0.51 | none | 3,526 |
| `list-ref-i32` | 4.1 | 8.1 | 0.50 | none | 3,512 |
| `list-ref-string` | 4.2 | 10.9 | 0.38 | none | 4,033 |
| `list-mut-i32` | 40.9 | 8.2 | 4.98 | retain `&mut i32` | 4,268 |
| `list-mut-string` | 48.2 | 13.8 | 3.49 | retain `&mut string` | 7,238 |
| `list-sort-i32` | 62.9 | 38.6 | 1.63 | none | 5,608 |
| `list-sort-string` | 87.3 | 42.2 | 2.07 | none | 6,499 |
| `list-filter-i32` | 16.8 | 21.4 | 0.78 | none | 4,350 |
| `list-filter-string` | 39.5 | 23.0 | 1.72 | none | 5,069 |
| `list-nested` | 13.7 | 7.3 | 1.89 | none | 4,127 |

## Map

| case | hd ms | JS ms | hd/JS | counts per target operation | Wasm B |
| --- | ---: | ---: | ---: | --- | ---: |
| `map-get-key-i32` | 22.7 | 15.2 | 1.49 | none | 4,516 |
| `map-contains-key-i32` | 27.3 | 12.1 | 2.26 | none | 4,556 |
| `map-contains-key-string` | 149.7 | 27.1 | 5.53 | none | 5,979 |
| `map-for-key-i32` | 12.0 | 36.6 | 0.33 | none | 4,650 |
| `map-for-key-string` | 21.9 | 38.0 | 0.58 | retain + release `(string, i32)[0]` per entry | 6,179 |
| `map-keys-i32` | 29.9 | 16.1 | 1.86 | none | 5,194 |
| `map-keys-string` | 43.2 | 14.5 | 2.98 | none | 6,827 |
| `map-add-key-i32` | 26.3 | 45.6 | 0.58 | none | 4,707 |
| `map-add-key-string` | 156.5 | 65.7 | 2.38 | none | 6,264 |
| `map-get-or-insert-key-i32` | 29.4 | 45.9 | 0.64 | 2 retain counted reference | 5,076 |
| `map-get-or-insert-key-string` | 161.1 | 64.3 | 2.50 | 2 retain counted reference | 6,715 |
| `map-nested` | 59.4 | 24.5 | 2.42 | copy `List[i32]` | 6,401 |

## Short-string share

`node --experimental-strip-types bench/micro/string-literal-census.ts` scans the fixed
string-literal construction
sites in 89 shipped benchmark and example source files: `bench/study`, `bench/runtime/progs`,
the five pre-existing `bench/micro` cases, the other runnable benchmark directories,
`examples`, and `website/playground/examples`. It excludes this audit's deliberately small
microcases. One source site is one sample; there is no arbitrary runtime weighting across
programs with different inputs. Interpolated and multiline sites are unknown rather than
assigned a guessed result length.

| maximum UTF-8 bytes | fixed sites | share of 402 fixed sites |
| ---: | ---: | ---: |
| 6 | 203 | 50.5% |
| 8 | 233 | 58.0% |
| 10 | 262 | 65.2% |
| 12 | 286 | 71.1% |

There are another 124 interpolated sites and one multiline site. Thus 286 of all 527 literal
construction sites are known to be at most 12 bytes: a 54.3% lower bound, with 78.0% as the
upper bound if every unknown site were short. This is evidence about source construction
sites, not a claim about allocation-frequency share.

## Short-string cases

Added 2026-11-02 for #1244, measured on `f02cff980` with `HD_MICRO_RUNS=15` (minimum), under
load averages of 19 to 27, with two earlier rows rerun in the same session for scale. Unlike
`string-eq-short` and `map-get-key-string`, these vary their operands: keys and tokens are built
at run time and visited in a varying order, so no comparison is loop-invariant.

| case | hd ms | JS ms | hd/JS | counts per target operation | Wasm B |
| --- | ---: | ---: | ---: | --- | ---: |
| `map-get-short-keys` | 134.8 | 43.6 | 3.09 | none | 7,091 |
| `map-get-varied-keys-i32` | 25.3 | 32.2 | 0.78 | none | 5,227 |
| `string-keyword-match` | 35.9 | 13.8 | 2.61 | none | 7,975 |
| `map-get-key-string` (rerun) | 156.3 | 38.4 | 4.06 | none | 5,938 |
| `string-eq-short` (rerun) | 78.8 | 16.5 | 4.77 | none | 5,496 |

`map-get-short-keys` looks up 1,000 keys of 2 to 4 bytes (`"k0"` to `"k999"`), 5,000,000
times in a permuted order. `map-get-varied-keys-i32` is the same program with `i32` keys: the
same table size, order and count. Its time is the most that packing keys of at most 6 bytes
into the string value (#1215) could approach, since a packed key hashes and compares as one
word. `string-keyword-match` splits a 20-token line once, then compares each token with four
keyword literals, 500,000 times.

## Findings

- On 2026-11-02, repeated `+` and `+=` building were the dominant miss: about 290 ms against
  Node's 0.2 ms. They append in place since, at 0.5 ms (Update, 2026-11-13).
  `StringBuilder.push`, slicing, character iteration and integer formatting are already at or
  below Node in these programs.
- The other string misses are grouped tightly: split, trim, equality and string-key lookup are
  4.58–5.26 times Node. Short and long equality cost the same within noise, so today's fixed
  call/scan path dominates the extra bytes in this range.
- On 2026-11-02, a borrowing List loop was 4.77–4.81 times Node, with a retain and release
  for every element, and mutable element calls retained and were 3.49–4.98 times Node. Element
  references count nothing since: on 2026-11-13 the borrowing loops are 0.45–0.47 times Node
  and the mutable element calls 0.51–1.00 times (Update, 2026-11-13). Short direct `&xs[i]`
  calls have no count operation and beat Node.
- Integer-key Map iteration, update and `get_or_insert` beat Node. String-key get, contains,
  update and `get_or_insert` are 2.38–5.53 times Node, isolating string hashing/comparison as
  the difference. `keys()` is intentionally copying and is 1.86 times Node for integers and
  2.98 times for strings.
- Copying nested collection values remains visible: List-of-List get is 1.89 times Node and
  string-key Map-to-List get is 2.42 times Node, with one `List[i32]` copy at the Map call site.
