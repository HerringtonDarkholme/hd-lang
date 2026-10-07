# Compiler Profile: `b89ceffe`

Date: 2026-10-07

This report profiles the first split-crate `hd_driver` and `hd` CLI at
`b89ceffe`. It compares the same generated workload with the preceding
walking-skeleton report for `84e1d32b`.

## Scope And Method

The release `hd` binary ran `hd bench 200` (3,009 generated lines) and `hd
bench 2000` (30,009 lines). Each command reports cold, warm, all-private-body
edit and one-comment edit runs from one in-memory cache. The body edit changes
all generated functions in `geo`, not one function.

The six checked-in samples ran through `hd run` on V8 through Node. `hd build`
also built the trait sample. The bench command compiles but does not instantiate
its generated Wasm, so runtime numbers come only from those samples.

`samply` and macOS `sample` were denied permission in this environment during
the immediately preceding profile. The hot-function ranking therefore maps
the driver's built-in task timers to their owning functions. `Parse` is nested
inside its caller and separately timed, so those percentages overlap and must
not be added. Allocation counts and peak RSS remain **no data**: there are no
allocator counters, and `/usr/bin/time -l` cannot access the required system
data here. The bench still has no signature-edit phase, so that case is also
**no data**.

## Generated Bench Results

| Input and run | Total | Parse | Folder interface | Module prep | Body | Module finish | Collect | Emit | Link | Wasm |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3,009 lines, cold | 7.30 ms | 0.82 ms | 1.22 ms | 0.31 ms | 1.41 ms | 1.38 ms | 1.15 ms | 1.08 ms | 0.09 ms | 55,717 B |
| 3,009 lines, warm | 0.55 ms | — | 0.10 ms | 0.002 ms | — | — | 0.002 ms | — | cache hit | unchanged |
| 3,009 lines, all `geo` bodies edited | 5.35 ms | 0.58 ms | 0.11 ms | 0.74 ms | 0.94 ms | 0.91 ms | 1.17 ms | 0.80 ms | 0.11 ms | rebuilt |
| 3,009 lines, one comment edited | 2.98 ms | 0.54 ms | 0.10 ms | 0.68 ms | 0.87 ms | 0.86 ms | 0.003 ms | — | cache hit | unchanged |
| 30,009 lines, cold | 69.38 ms | 9.12 ms | 12.29 ms | 3.29 ms | 13.90 ms | 13.68 ms | 10.31 ms | 9.95 ms | 0.78 ms | 559,718 B |
| 30,009 lines, warm | 5.02 ms | — | 0.85 ms | 0.003 ms | — | — | 0.01 ms | — | cache hit | unchanged |
| 30,009 lines, all `geo` bodies edited | 48.29 ms | 5.86 ms | 0.94 ms | 7.18 ms | 8.45 ms | 8.34 ms | 10.61 ms | 7.00 ms | 0.80 ms | rebuilt |
| 30,009 lines, one comment edited | 27.31 ms | 5.58 ms | 0.85 ms | 6.84 ms | 7.69 ms | 7.79 ms | 0.01 ms | — | cache hit | unchanged |

All requested measured paths remain close to linear from 3k to 30k lines.
Nothing is order-of-magnitude, superlinear or dominated for no design reason.
The 30k warm run is well below the 50 ms edit-latency target even though the
future manifest fast path is not present.

## Change From `84e1d32b`

| Run | 3,009 lines | 30,009 lines |
| --- | ---: | ---: |
| Cold | 10.19 → 7.30 ms (−28%) | 93.21 → 69.38 ms (−26%) |
| Warm | 1.74 → 0.55 ms (−68%) | 16.31 → 5.02 ms (−69%) |
| All-body edit | 5.87 → 5.35 ms (−9%) | 59.30 → 48.29 ms (−19%) |
| Comment edit | 4.02 → 2.98 ms (−26%) | 38.39 → 27.31 ms (−29%) |

The largest improvement is warm module prep: 10.54 ms at 30k lines became
about 3 µs. Splitting the compiler into its designed crates did not introduce
dispatch or serialization regressions. Cold `Collect` is now visible at 10.31
ms because TIR is explicitly decoded across the new crate boundary, but total
cold time still fell by a quarter.

## Samples And V8

| Command | Result | Wall time |
| --- | --- | ---: |
| `hd run samples/hello` | prints `42` | 0.04 s |
| `hd run samples/arith` | expected nine-line arithmetic output | 0.04 s |
| `hd run samples/data` | expected data output | 0.04 s |
| `hd run samples/fib` | prints `6765` | 0.04 s |
| `hd run samples/generic` | expected generic output | 0.04 s |
| `hd run samples/trait` | expected trait output | 0.04 s |
| `hd build samples/trait -o …` | succeeds | below the timer's 0.01 s resolution |

The run timings include process startup and starting Node, so they do not
isolate compiler time or V8 instantiation. They are smoke measurements, not a
runtime comparison.

## Hot Functions At 30,009 Lines, Cold

Percent is stage time divided by 69.38 ms wall time. Parse overlaps its caller.

| Rank | Function or task owner | Time | Wall share | Assessment |
| ---: | --- | ---: | ---: | --- |
| 1 | `Run::body` | 13.90 ms | 20.0% | Architecture issue, acceptable: body checking is the intended cold semantic work and remains module-granular in this slice. |
| 2 | `Run::module_finish` | 13.68 ms | 19.7% | Architecture issue: the module-granular `check` entry follows Type Checking §1.7; per-body reuse is the documented later lever. |
| 3 | `Run::folder_iface` | 12.29 ms | 17.7% | Implementation slip: the skeleton still reaches full parsing while building interfaces; Detailed Design §1.2 separates the header/interface path. |
| 4 | `Run::collect` | 10.31 ms | 14.9% | Architecture issue, acceptable: Codegen §13.2 requires one reachability and instance-collection pass, including TIR decode. |
| 5 | `Run::emit` | 9.95 ms | 14.3% | Architecture issue, acceptable: Codegen §13.8 specifies one emission walk for each of the 4,001 cold instances. |
| 6 | `Run::parse` / `parse_subset` | 9.12 ms | 13.1% | Implementation slip: this is still the temporary subset parser; tuning it is lower value than replacing it. |
| 7 | `Run::skim` / `skim` | 4.27 ms | 6.2% cold, 77.8% warm | Implementation slip: Detailed Design §1.4's manifest/`pkgres` no-edit fast path should avoid rescanning sources. |
| 8 | `Run::module_prep` | 3.29 ms | 4.7% | Implementation is healthy: on a warm hit this falls to about 3 µs, fixing the previous report's main slip. |
| 9 | `Run::link` | 0.78 ms | 1.1% | Architecture issue, acceptable: deterministic ordering and relocation from Codegen §13.10 are not dominant. |
| 10 | `Run::folder_graph` | 0.008 ms | 0.01% | Already negligible; no action. |

## Findings

1. No atrocious performance problem appears. The split-crate compiler is
   faster on every measured path and keeps near-linear scaling.
2. The previous report's module-prep hotspot is fixed. Warm work is now almost
   entirely `skim`; the next implementation step is the manifest/`pkgres`
   no-edit fast path from Detailed Design §1.4, not micro-tuning.
3. Comment edits correctly reuse TIR/code/link outputs but still reparse and
   recheck the edited 15k-line module. That is the documented whole-module
   tradeoff in Cache §5.9.
4. Add signature-edit timing to `hd bench`, expose compilation versus Node
   startup in `hd run`, and add allocation/RSS counters. These are measurement
   gaps; none blocks the current slice.

