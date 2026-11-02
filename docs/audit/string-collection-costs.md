# String, List and Map release costs

Measured 2026-11-02 for #1208, on compiler commit `caece8ae2` from `origin/main`.
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

## Findings

- Repeated `+` and `+=` building are the dominant miss: about 290 ms against Node's 0.2 ms.
  `StringBuilder.push`, slicing, character iteration and integer formatting are already at or
  below Node in these programs.
- The other string misses are grouped tightly: split, trim, equality and string-key lookup are
  4.58–5.26 times Node. Short and long equality cost the same within noise, so today's fixed
  call/scan path dominates the extra bytes in this range.
- A borrowing List loop is 4.77–4.81 times Node and much slower than the corresponding value
  loop. Its counts view identifies a retain and release for every element. Short direct
  `&xs[i]` calls have no count operation and beat Node; mutable element calls retain and are
  3.49–4.98 times Node.
- Integer-key Map iteration, update and `get_or_insert` beat Node. String-key get, contains,
  update and `get_or_insert` are 2.38–5.53 times Node, isolating string hashing/comparison as
  the difference. `keys()` is intentionally copying and is 1.86 times Node for integers and
  2.98 times for strings.
- Copying nested collection values remains visible: List-of-List get is 1.89 times Node and
  string-key Map-to-List get is 2.42 times Node, with one `List[i32]` copy at the Map call site.
