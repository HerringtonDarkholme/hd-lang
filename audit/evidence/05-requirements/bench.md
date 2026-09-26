<!-- commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/bench-run.ts; date: 2026-09-25T20:54:19.679Z -->

Samples: 21 interleaved dev/-O2 calls of `main` per program after 5 warmups; 5 compiles. CPU shared with other audit workers: read ratios, not absolutes. Min is the least-contended sample and is the primary estimator; load average at start: 64.5 on 14 cores.

| program | result | dev wasm B | -O2 wasm B | compile ms (median) | phases ms parse / check / emit / assemble | -O2 pass ms | dev run ms min / median (range) | -O2 run ms min / median (range) | dev/-O2 run (min) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| b1-scalar | 196418 | 2548 | 194 | 44.3 | parse 0.2 / check 0.5 / emit 0.5 / assemble 33.6 | 291.3 | 1.26 / 1.27 (1.26..29.05) | 0.84 / 0.86 (0.84..39.81) | 1.49 |
| b2-generic | 997006 | 2882 | 651 | 89.0 | parse 0.3 / check 0.6 / emit 0.3 / assemble 25.8 | 803.9 | 7.65 / 12.68 (7.65..112.47) | 7.69 / 7.82 (7.69..239.96) | 1.00 |
| b2-mono | 997006 | 2832 | 642 | 8.2 | parse 0.3 / check 0.5 / emit 0.2 / assemble 7.2 | 260.1 | 7.25 / 12.99 (7.25..206.85) | 7.25 / 7.26 (7.25..154.57) | 1.00 |
| b3-req1 | 999974 | 2982 | 548 | 60.1 | parse 0.7 / check 1.8 / emit 0.7 / assemble 28.8 | 167.3 | 10.04 / 26.73 (10.04..229.00) | 8.38 / 14.89 (8.38..116.91) | 1.20 |
| b3-req5 | 999974 | 3550 | 841 | 69.0 | parse 1.0 / check 2.5 / emit 0.5 / assemble 48.3 | 85.1 | 8.06 / 16.71 (8.06..259.01) | 7.94 / 12.10 (7.94..117.93) | 1.02 |
| b4-suspend | 799994 | 4529 | 2078 | 19.9 | parse 0.2 / check 0.4 / emit 5.8 / assemble 13.8 | 184.5 | 10.13 / 12.35 (10.13..215.36) | 10.10 / 12.09 (10.10..156.20) | 1.00 |
| b4-plain | 799994 | 2617 | 217 | 8.4 | parse 0.3 / check 0.2 / emit 0.1 / assemble 7.2 | 7.1 | 0.79 / 0.90 (0.79..69.95) | 0.79 / 0.80 (0.79..39.89) | 1.00 |

| pair | dev ratio (min) | -O2 ratio (min) | dev ratio (median) | -O2 ratio (median) | size ratio dev |
| --- | --- | --- | --- | --- | --- |
| erased / monomorphic (b2-generic / b2-mono) | 1.06 | 1.06 | 0.98 | 1.08 | 1.02 |
| 5-row / 1-row (b3-req5 / b3-req1) | 0.80 | 0.95 | 0.63 | 0.81 | 1.19 |
| suspending / plain (b4-suspend / b4-plain) | 12.80 | 12.72 | 13.73 | 15.15 | 1.73 |

b4-suspend with the dev runtime's `pending`/`trace` imports replaced by constant stubs: min 8.87 ms, median 11.05 ms (8.87..84.30); min ratio to b4-plain dev: 11.21; min ratio to b4-suspend dev: 0.88.
