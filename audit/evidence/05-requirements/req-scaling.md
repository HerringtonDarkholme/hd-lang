<!-- commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/req-probes.ts; date: 2026-09-25T20:54:35.947Z -->

Iterations per sample: 200000. Hidden-param column reads the first chain link's export signature (for row-chain probes that function is the exported `leaf`, which takes one param per concrete entry; the ten `g` links each take a single `$hd.providers` pack parameter).

| probe | reqs | ns per 10-deep call, min / median of 15 | sample ms range | static allocs in user fns | hidden provider params on a link | concat calls/call | pack nodes copied/call | pack lookups/call |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| chain-0 | 0 | 5.6 / 5.6 | 1.13..7.50 | 0 | 0 | 0 | 0 | 0 |
| chain-1 | 1 | 5.6 / 5.7 | 1.13..26.48 | 2 | 1 | 0 | 0 | 0 |
| chain-5 | 5 | 5.7 / 5.7 | 1.14..39.29 | 10 | 5 | 0 | 0 | 0 |
| chain-10 | 10 | 5.7 / 5.7 | 1.13..86.95 | 20 | 10 | 0 | 0 | 0 |
| row-chain-1 | 1 | 13.4 / 37.3 | 2.68..55.64 | 5 | 1 | 2 | 1 | 1 |
| row-chain-5 | 5 | 100.8 / 515.7 | 20.17..225.20 | 17 | 1 | 30 | 25 | 5 |
| row-chain-10 | 10 | 417.0 / 1719.8 | 83.39..651.04 | 32 | 1 | 110 | 100 | 10 |
