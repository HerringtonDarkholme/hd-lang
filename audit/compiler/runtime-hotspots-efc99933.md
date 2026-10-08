# Q26: Where The Time Goes In map-count And string-build (`efc99933`)

Q22/Q24 put map-count at ~60x and string-build at ~84x vs Node. Profiled
both hd programs (`compiler/bench/runtime/progs/`, debug profile, binary
rebuilt from this commit) with `node --cpu-prof compiler/host/run.mjs
…`, attributing leaf (self) and inclusive stacks from the cpuprofile
tree, plus differential runs of edited copies in /tmp (repo untouched).
`node --prof` logging is disabled in this Node build
(`v24.19.0`), so tick-stack logs were unavailable; cpuprofile stacks
plus scaling experiments carry the attribution instead. No
`compiler/crates/` edits.

## map-count (200k inserts + 5k reads, ~2000 ms): ~98% is Map operations

Leaf self time (1646 samples): **99.2% in `tally`**, 0.4% in
`Map.get_or`, ~0% GC, ~0% host. Inclusive stacks confirm it: 99.6% of
all time sits under `tally` with no deeper Wasm frames — hashing,
probing, equality, boxing and growth are fused into the one loop
function, so the profiler cannot split them from outside.

Differential runs split what the profiler cannot:

| Variant (same arithmetic, /tmp copies) | Wall | Share of original |
| --- | ---: | ---: |
| original Map tally (200k) | ~2007 ms | 100% |
| original at 100k inserts | ~1033 ms | linear: ~10 µs/iteration, no chain blowup at 40 entries/bucket |
| List-indexed counting (`counts[usize(key)]`, no Map) | ~37 ms | ~2% |

So **~1963 of ~2000 ms (98%) is Map work**: per iteration one `get`
(hash + probe + `eq` + `Option` box/unbox) plus one `counts[key] = …`
index-assign (a second full hash + probe + `eq` + set) — two lookups per
insert, ~10 µs per iteration-pair, thousands of Wasm instructions per
op. GC is invisible (~0 samples): small-object allocation is cheap;
the cost is executed instructions, not collection. Scaling is linear
in n at fixed buckets (1.94x for 2x inserts), so this is a large
per-op constant, not a superlinear algorithm.

## string-build (40k appends, ~2592 ms): O(n²) copies + GC

Leaf self time (2121 samples): **91.9% in `build`**, **7.8% in the
Wasm GC**, ~0% host. The 7.8% GC (vs ~0% for map-count) is the 40k
discarded temporaries pressuring the collector.

Copy volume, arithmetically and empirically:

- Each `out + "abcdefgh"` copies the whole accumulated string, so
  append k copies 8k bytes. Total copied = 8·(1+…+40000) ≈ **6.4 GB
  to produce a 320 KB result** — a ~20,000x amplification.
- Measured scaling at fixed code, varying n: 10k → 169 ms, 20k →
  636 ms (3.8x), 40k → 2592 ms (4.1x over 20k, 15.3x over 10k).
  Doubling n quadruples time: textbook O(n²).
- Cross-check: 6.4 GB / 2.6 s ≈ 2.5 GB/s effective copy bandwidth,
  consistent with plain Wasm memcpy, i.e. the time really is the
  copies, not per-append overhead.

## The two or three changes that remove most of it

1. **Rope or builder strings (representation change)** — kills the
   84x. Appends become O(1) amortized; the 6.4 GB of copies and most
   of the 7.8% GC go away together. Nothing else touches string-build:
   92% + 8% are both the append.
2. **Single-probe map update, ideally an entry API
   (implementation slip)** — the loop does two full lookups per
   iteration (`get` for the match, then `counts[key] = …`). One
   hash + one probe per iteration roughly halves the ~98% Map share
   before any representation work.
3. **Unboxed small-int keys/values without per-op `Option` boxing
   (implementation slip)** — every `get` allocates an `Option` and
   every key/value crosses boxed. Specializing the `i32 → i32` shape
   removes the remaining per-op allocation traffic inside the 98%.
