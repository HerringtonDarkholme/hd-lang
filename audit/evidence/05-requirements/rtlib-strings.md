<!-- commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/rtlib-strings.ts; date: 2026-09-25T20:46:03.527Z -->

Host provider `TextBridge.join!(s, "")` returns its argument; console case prints via `println`. Crossing time = median(main with crossing) - median(main without). 5 samples, fresh instance each.

| bytes | crossing | median main ms | baseline ms | crossing ms | ns per byte | host import calls |
| --- | --- | --- | --- | --- | --- | --- |
| 1024 | host | 0.55 | 0.03 | 0.52 | 507.4 | ~2052 |
| 1024 | console | 0.14 | 0.03 | 0.11 | 106.3 | ~1025 |
| 131072 | host | 33.15 | 0.33 | 32.81 | 250.4 | ~262148 |
| 131072 | console | 2.62 | 0.33 | 2.29 | 17.5 | ~131073 |
| 1048576 | host | 377.53 | 3.80 | 373.73 | 356.4 | ~2097156 |
| 1048576 | console | 25.61 | 3.80 | 21.81 | 20.8 | ~1048577 |

Host-call counts are from the emitted protocol (one `argument_byte` import call per argument byte, one `result_byte` per result byte, plus begin/poll/result_length; one `console_byte` per output byte plus a terminator). See rtlib-imports.md and host-wrapper.wat.
