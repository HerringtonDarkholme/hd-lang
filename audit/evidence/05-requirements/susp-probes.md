<!-- commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/susp-probes.ts; date: 2026-09-25T20:58:08.278Z -->


## Poll cost versus depth

Leaf frame held pending for 4000 polls; root driven through `__hd_poll`. Median of 5 runs.

| depth | ns per root poll | pending-hook calls per poll | trace-hook calls per poll |
| --- | --- | --- | --- |
| 1 | 273 | 2.00 | 4.00 |
| 4 | 861 | 5.00 | 10.00 |
| 16 | 813 | 17.00 | 34.00 |
| 64 | 5005 | 65.00 | 130.02 |

## Frame code size versus suspension sites

| sites | WAT bytes of main poll fn | child poll calls in main poll fn | frame spill stores in main poll fn | wasm bytes (module) | compile ms (emit / assemble) |
| --- | --- | --- | --- | --- | --- |
| 1 | 3253 | 2 | 4 | 3628 | 0 / 11 |
| 2 | 5858 | 5 | 15 | 3945 | 0 / 16 |
| 4 | 15349 | 14 | 70 | 4994 | 1 / 13 |
| 8 | 60274 | 44 | 396 | 9432 | 1 / 22 |
| 16 | 340226 | 152 | 2584 | 33046 | 6 / 110 |
| 32 | 2555834 | 560 | 18480 | 182223 | 85 / 514 |
| 48 | 9286002 | 1224 | 59976 | 549255 | 152 / 1013 |

## CFG lowering code size versus sites (`v = step!(v) + 0`)

| sites | WAT bytes of main poll fn | wasm bytes (module) | compile ms (emit / assemble) |
| --- | --- | --- | --- |
| 1 | 5457 | 3914 | 0 / 8 |
| 2 | 10362 | 4511 | 0 / 10 |
| 8 | 76880 | 11453 | 1 / 22 |
| 16 | 280742 | 29712 | 2 / 55 |
| 32 | 1206836 | 102139 | 20 / 191 |
| 48 | 3029192 | 228719 | 37 / 349 |

## Cancellation cleanup order

- first poll returned 0 (0 = pending)
- cancellation log: 214365 (expected by spec 11 Cancellation + 06 Deferred Cleanup: 214365)
- normal completion log: 92148365 (expected: 92148365)

## Driver guard

- instance A started (pending); A.__hd_poll(): returned 0
- instance B.main() while A holds its driver: returned 11
- instance A.main() while A's dev frame is started: threw RuntimePanicError: suspension-competing-driver: runtime panic
- host pending callback re-enters C.main() during C.main(): threw RuntimePanicError: suspension-competing-driver: runtime panic
- instance D.main() with divisor 0 (panic inside a drive): threw RuntimePanicError: integer-division-by-zero: runtime panic
- instance D.main() after that panic, divisor 1: threw RuntimePanicError: suspension-competing-driver: runtime panic
