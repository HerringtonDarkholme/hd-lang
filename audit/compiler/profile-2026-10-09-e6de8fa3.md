# Compiler Profile: `e6de8fa3`

Date: 2026-10-09.

Status: measurement only; nothing here changes accepted behavior. This
checks the compiler with the work since `88ec3a4f` (the previous
report, `audit/compiler/profile-2026-10-09-88ec3a4f.md`): O22
(#74 follow-ups), O23/O24 (unknown), #225 (typing fixtures with
`--tests`), test-overlay and doc-test wiring, and the R23–R29 design
notes (no code), against
[Resolution And Interfaces](../../future-work/compiler/resolution-and-interfaces.md),
[Scheduler](../../future-work/compiler/scheduler.md),
[Cache](../../future-work/compiler/cache.md), and
[Codegen](../../future-work/compiler/codegen.md).

## Scope And Method

Same harness and method as the previous report:
`bench/profile-signature` at 200 and 2,000 functions per folder (3,009
and 30,009 generated lines, plus embedded std), median of five
processes; signature edits in fresh processes; `hd run` / `hd build` on
the samples through the spec CLI forms (five runs after one warm-up,
seven fresh builds). No harness changes.

Samply still has no usable run on this host and macOS exposes no
counters, so native symbols, allocations and peak RSS remain **no
data**. Rankings use the driver's stage timers. Two full windows
were run (falling load, then low load); medians below are the
low-load window. Edit-path medians moved ±6% between windows (long
runs, load-sensitive) and are reported with that caveat.

## Generated Bench Results

| Input and run | Total | Folder interface | Body | Module finish | Collect | Emit | Module prep | Skim | Header check | Coherence | Link | Wasm |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 3,009 lines, cold | 74.40 ms | 15.5 ms | 26.0 ms | 8.2 ms | 6.5 ms | 3.4 ms | 4.1 ms | 4.6 ms | 1.3 ms | 1.4 ms | 1.1 ms | 187,253 B |
| 3,009 lines, warm | 7.67 ms | 1.6 ms | — | — | 0.02 ms | — | 0.5 ms | 4.5 ms | 0.01 ms | 1.1 ms | cache hit | unchanged |
| 3,009 lines, all `geo` bodies edited | 21.72 ms | 1.6 ms | 2.5 ms | 0.8 ms | 6.6 ms | 2.0 ms | 1.5 ms | 4.5 ms | 0.00 ms | 1.1 ms | 1.0 ms | rebuilt |
| 3,009 lines, public signature and caller edited | 24.45 ms | 3.7 ms | 3.8 ms | 1.1 ms | 6.6 ms | 0.03 ms | 1.1 ms | 4.4 ms | 0.1 ms | 1.1 ms | 1.0 ms | rebuilt |
| 3,009 lines, one comment edited | 8.25 ms | 1.6 ms | — | — | 0.01 ms | — | 1.5 ms | 4.5 ms | 0.01 ms | 0.0 ms | cache hit | unchanged |
| 30,009 lines, cold | 275.14 ms | 35.3 ms | 61.5 ms | 18.1 ms | 22.9 ms | 37.2 ms | 9.3 ms | 14.5 ms | 2.0 ms | 1.4 ms | 11.0 ms | 1,843,871 B |
| 30,009 lines, warm | 22.32 ms | 2.9 ms | — | — | 0.3 ms | — | 1.5 ms | 11.9 ms | 0.0 ms | 0.0 ms | cache hit | unchanged |
| 30,009 lines, all `geo` bodies edited | 139.53 ms | 2.5 ms | 14.6 ms | 6.3 ms | 12.5 ms | 8.1 ms | 10.1 ms | 12.2 ms | 0.8 ms | 0.9 ms | 1.1 ms | rebuilt |
| 30,009 lines, public signature and caller edited | 140.46 ms | 31.2 ms | 37.2 ms | 12.1 ms | 25.1 ms | 0.03 ms | 5.3 ms | 12.2 ms | 0.2 ms | 1.3 ms | 11.2 ms | rebuilt |
| 30,009 lines, one comment edited | 30.17 ms | 2.5 ms | — | — | 0.03 ms | — | 10.0 ms | 12.1 ms | 0.0 ms | 0.0 ms | cache hit | unchanged |

Totals are medians of five; stage columns show one representative run.
New counters since last round: `parse` (38 misses cold — every
embedded std module is parsed, then cached) and `graph` misses 11
(was 5). Wasm grew +333 B at both sizes (1,843,538 → 1,843,871):
the embedded std grew (R10's `it_each`/`it_prop`/`snapshot`
declarations, #177).

| Generated-size increase | Cold | Warm | Body edit | Signature edit | Comment edit | Wasm |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 3,009 → 30,009 lines | 3.7× | 2.9× | 6.4× | 5.7× | 3.7× | 9.8× |

## Serial And Pool

8-worker pool medians; only wall time is compared.

| Input and run | Serial | 8-worker pool | Pool / serial |
| --- | ---: | ---: | ---: |
| 3,009 lines, cold | 74.40 | 38.55 | 0.52× |
| 3,009 lines, warm | 7.67 | 4.46 | 0.58× |
| 3,009 lines, body edit | 21.72 | 18.97 | 0.87× |
| 3,009 lines, comment edit | 8.25 | 5.13 | 0.62× |
| 30,009 lines, cold | 275.14 | 128.99 | 0.47× |
| 30,009 lines, warm | 22.32 | 16.57 | 0.74× |
| 30,009 lines, body edit | 139.53 | 111.59 | 0.80× |
| 30,009 lines, comment edit | 30.17 | 24.75 | 0.82× |

## Change From `88ec3a4f`

| Run | 3,009 lines | 30,009 lines |
| --- | ---: | ---: |
| Cold | 71.66 → 74.40 ms (+4%) | 269.95 → 275.14 ms (+2%) |
| Warm | 7.88 → 7.67 ms (−3%) | 18.73 → 22.32 ms (+19%) |
| All-body edit | 22.02 → 21.72 ms (−1%) | 131.39 → 139.53 ms (+6%) |
| Signature edit | 23.75 → 24.45 ms (+3%) | 141.76 → 140.46 ms (−1%) |
| Comment edit | 11.99 → 8.25 ms (−31%) | 59.77 → 30.17 ms (−50%) |

Two real movements, in opposite directions:

- **Comment-only edits halved (both windows agree: ~60 → ~30 ms
  at 30,009 lines).** The representative breakdown shows why: no
  `Body`, `ModuleFinish`, `Emit` or `Link` stage runs at all — only
  `Skim` + `ModulePrep` + interface validation. A comment changes
  the parse but no TIR, and the pipeline now notices (new `parse`
  counter; test-overlay/doc-test era change detection). Genuine
  improvement from the O-lane caching work.
- **Warm path still elevated (18.73 → 22.32 ms, +19%).** Same
  watch as the last two rounds: no single owning stage (Skim flat
  at ~12, the rest within ±1 ms, balance in untimed overhead).
  Below the flag bar, but now three rounds running — worth one
  targeted look the next time the folder-graph/cache area is
  touched.

Everything else is flat (±4% or window noise). The `graph` miss
count 5 → 11 and the `TestOverlay` counter now appearing in
breakdowns track the test-overlay work; both costless here.

## Samples, Realistic Inputs, And V8

Median of five runs after one warm-up; `hd build` median of seven
fresh builds in the package.

| Command | Result | Median wall time | Previous |
| --- | --- | ---: | ---: |
| `hd samples/hello/hello.hd` | `42` | 41.12 ms | 38.85 ms |
| `hd samples/arith/main.hd` | `7`, `9`, … | 41.48 ms | 38.16 ms |
| `hd run` in `samples/data` | `3`, `-4`, … | 41.31 ms | 39.99 ms |
| `hd samples/defer/main.hd` | `5`, `err bad`, … | 40.65 ms | 40.22 ms |
| `hd samples/err_exit/main.hd` | exit 1, `got 1…` | 41.05 ms | 38.36 ms |
| `hd samples/exit/main.hd` | `total 14 of 5`, … | 39.39 ms | 38.19 ms |
| `hd run` in `samples/fib` | `6765` | 40.82 ms | 37.92 ms |
| `hd run` in `samples/gaps` | `200 OK false`, … | 40.17 ms | 39.24 ms |
| `hd run` in `samples/generic` | `1`, `6`, … | 39.31 ms | 38.49 ms |
| `hd run` in `samples/init` | `hello`, `15`, … | 39.25 ms | 38.71 ms |
| `hd run` in `samples/std_types` | `42` | 39.45 ms | 38.74 ms |
| `hd samples/suspend/main.hd` | `slept`, … | 69.63 ms | 68.43 ms |
| `hd run` in `samples/trait` | `12`, `13`, `101`, `7` | 40.08 ms | 38.51 ms |
| `hd bench/tokenizer/main.hd` | `1`, `2`, … | 39.16 ms | 38.92 ms |
| `hd bench/math/main.hd` | `21`, `832040`, … | 39.07 ms | 39.10 ms |
| `hd run` in `bench/records` | `13`, `82`, … | 40.19 ms | 39.39 ms |
| `hd run` in `bench/traits` | `42`, `81`, … | 39.68 ms | 38.75 ms |
| `hd build` in `samples/trait` | valid 6,633-byte Wasm | 10.71 ms | 7.56 ms / 6,633 B |

All sample times are flat (±2 ms; one 312 ms first-run outlier on
hello discarded and re-measured clean); outputs unchanged,
including the benign `unknown-manifest-key` warnings on the two old
bench manifests.

## Hot Functions At 30,009 Lines, Cold

Named stage medians sum to 211.20 ms of 274.77 ms. The 63.57 ms
remainder (23.1%) is scheduler, task-graph, cache and driver work
outside stage timers.

| Rank | Function or task owner | Time | Wall share | Assessment |
| ---: | --- | ---: | ---: | --- |
| 1 | `Run::body` | 61.5 ms | 22.4% | **Architecture issue:** module-granular body checking (Type Checking §1.7). Flat. |
| 2 | Serial graph dispatch and cold task publication, exact function **no data** | 63.6 ms | 23.1% | **Architecture issue:** 4,006 single-instance `Emit` tasks remain; one `Emit(group)` per folder group is still the designed shape (Scheduler §6.2, Codegen §11.3). Flat. |
| 3 | `Run::folder_iface` | 35.3 ms | 12.8% | **Architecture issue:** interfaces before dependents (Resolution §4.10). Rep-level dip (−7 ms); medians rule it noise. |
| 4 | `Run::emit` | 37.2 ms | 13.5% | **Architecture issue:** emission for 4,006 cold instances (Codegen §13.8). Flat. |
| 5 | `Run::collect` | 22.9 ms | 8.3% | **Implementation slip:** reachable-module decode still decodes every module on a miss (Codegen §11.3). Still the cheapest fix on the board. |
| 6 | `Run::module_finish` | 18.1 ms | 6.6% | **Architecture issue:** per-body reuse is the documented later lever. |
| 7 | `Run::skim` | 14.5 ms | 5.3% | Watch held: flat after the O6/O7 move. |
| 8 | `Run::link` | 11.0 ms | 4.0% | **Architecture issue:** deterministic relocation (Codegen §13.10). |
| 9 | `Run::module_prep` | 9.3 ms | 3.4% | **Architecture issue:** scope preparation after a check miss. |
| 10 | `Universe::overlaps` and coherence assembly | 1.4 ms | 0.5% | **Implementation slip:** tries still specified (Resolution §4.12.3); still cheap here. |

## Findings

- Nothing to flag. One genuine improvement (comment edits skip
  everything past interface validation) and one standing watch
  (warm path, three rounds, still ownerless).
- The untimed remainder (scheduler/graph/cache/driver, 63.57 ms cold)
  stays a top-two row with no function-level owner; sampled symbols
  would still be the way to split it.
