# Spike 0c: Representation Benchmark Results

Recorded `2026-10-07T17:37:09Z` on `Darwin mingyangs-macbook-pro.taile9cb8a.ts.net 25.6.0 Darwin Kernel Version 25.6.0: Fri Jul 31 19:18:53 PDT 2026; root:xnu-12377.161.14~5/RELEASE_ARM64_T6031 arm64`. Load: `13:37  9 users, load averages: 12.11 12.01 12.98`. Toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, Wasmtime 49.0.2, Node v24.19.0 / V8 13.6.233.17-node.51. The saved run is exploratory: 3 runtime sample(s), 1 warm-up(s), and 1 compile sample(s) per row. V8 through Node is the primary engine; the Wasmtime column is included only for E1, whose rule names cast cost on Wasmtime.

Times are p50/p95. With one compile sample, compile p50 and p95 are necessarily equal. Missing experiments are recorded instead of extrapolated; the 10,000 x 200-byte E10 fixed-literal compile was stopped after it saturated one core for more than 40 minutes and peaked near 9 GB RSS.

## E0

No completed result; decision deferred.

## E1

| variant | Wasm B | code B | funcs/types/globals | Liftoff compile ms | TurboFan compile ms | V8 instantiate ms | V8 metric p50/p95 | Wasmtime metric p50/p95 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| e1-exact-read | 216 | 101 | 1/3/0 | 0.121/0.121 | 0.121/0.121 | 0.003/0.003 | 21.710/29.063 ns/element | 25.956/26.287 ns/element |
| e1-exact-write | 212 | 97 | 1/3/0 | 0.048/0.048 | 0.042/0.042 | 0.002/0.003 | 6.329/28.508 ns/element | 114.983/115.660 ns/element |
| e1-exact-push | 242 | 126 | 1/3/0 | 0.039/0.039 | 0.049/0.049 | 0.002/0.004 | 8.375/41.944 ns/element | 48.265/48.325 ns/element |
| e1-exact-map | 233 | 118 | 1/3/0 | 0.037/0.037 | 0.036/0.036 | 0.003/0.007 | 5.608/7.300 ns/element | 48.183/48.506 ns/element |
| e1-exact-sort | 235 | 120 | 1/3/0 | 0.028/0.028 | 0.044/0.044 | 0.002/0.002 | 5.010/5.227 ns/element | 26.352/26.719 ns/element |
| e1-eqref-read | 217 | 103 | 1/3/0 | 0.031/0.031 | 0.037/0.037 | 0.001/0.001 | 4.335/4.500 ns/element | 24.977/25.681 ns/element |
| e1-eqref-write | 211 | 97 | 1/3/0 | 0.018/0.018 | 0.023/0.023 | 0.001/0.001 | 3.585/3.956 ns/element | 114.746/114.825 ns/element |
| e1-eqref-push | 241 | 126 | 1/3/0 | 0.028/0.028 | 0.049/0.049 | 0.005/0.007 | 10.508/12.315 ns/element | 50.323/51.154 ns/element |
| e1-eqref-map | 234 | 120 | 1/3/0 | 0.020/0.020 | 0.061/0.061 | 0.001/0.002 | 4.265/4.665 ns/element | 48.327/48.413 ns/element |
| e1-eqref-sort | 238 | 124 | 1/3/0 | 0.025/0.025 | 0.036/0.036 | 0.001/0.002 | 3.848/4.065 ns/element | 26.488/27.188 ns/element |
| e1-exact-12types-size | 1625 | 484 | 121/15/0 | 0.041/0.041 | 0.068/0.068 | 0.002/0.005 | 125.000/250.000 ns/run | 41.000/42.000 ns/run |
| e1-eqref-12types-size | 1533 | 484 | 121/4/0 | 0.035/0.035 | 0.058/0.058 | 0.001/0.001 | 125.000/291.000 ns/run | 42.000/42.000 ns/run |

Decision: recommend A1. On the default-equivalent Wasmtime configuration, eqref read cost changed by -0.979 ns/element and the worst sort/map penalty was 0.5%, within the rule's 1 ns and 5% bounds; treat this as exploratory because the saved run is short.

## E2

No completed result; decision deferred.

## E3

No completed result; decision deferred.

## E4

No completed result; decision deferred.

## E5

No completed result; decision deferred.

## E6

No completed result; decision deferred.

## E7

No completed result; decision deferred.

## E8

No completed result; decision deferred.

## E9

No completed result; decision deferred.

## E10

No completed result. The pathological fixed-literal row was skipped; decision deferred.

## E11

No completed result; decision deferred.

## S1

No completed result; decision deferred.

## S2

No completed result; decision deferred.

## S3

No completed result; decision deferred.

## S4

No completed result; decision deferred.

## S5

No completed result; decision deferred.

## S6

No completed result; decision deferred.

## S7

No completed result; decision deferred.

## Measurement notes

- Peak RSS is a process high-water mark, not a per-row delta. V8 retained Wasm heap and automatic GC collection counts are not exposed by the JS API.
- The E1 Wasmtime column was collected with the explicit `Speed` setting, which matches Wasmtime 49.0.2's default Cranelift optimization level; no opt-level comparison is used in the decision.
