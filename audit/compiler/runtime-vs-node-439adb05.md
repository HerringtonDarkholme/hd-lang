# Q24: Runtime vs Node, Release Builds (`439adb05`)

Same six programs and method as Q22 (`runtime-vs-node-6572b51d.md`,
deleted in this commit; git history keeps it): build once per program
with `compiler/target/release/hd` rebuilt from this commit, 1 warmup +
5 measured runs each, p50 = median, p95 = max. Each program prints a
checksum; the script refuses to time a program whose hd and JS outputs
differ (all six matched in both passes). Runner: `node v24.19.0
compiler/host/run.mjs` for hd, `node` directly for JS, same machine.
Rerunnable: `node compiler/bench/runtime/run.mjs [workdir]` (debug) and
`node compiler/bench/runtime/run.mjs [workdir] --release` (release; new
switch in this commit).

| Program | What it does | debug hd/JS | release hd/JS |
| --- | --- | ---: | ---: |
| closures | fill 20M list, `map`/`filter`/`fold` with closures | 0.67x (367 / 551) | 0.67x (350 / 525) |
| int-loop | 240M iterations of `%`-heavy i32 accumulation | 3.03x (2454 / 809) | 2.89x (2368 / 820) |
| list-sort | insertion sort of 30k pseudorandom i32 | 7.76x (1308 / 168) | 7.71x (1337 / 173) |
| map-count | 200k inserts + 5k `get_or` reads on `Map[i32, i32]` | 59.97x (2014 / 34) | 60.50x (2024 / 33) |
| string-build | 40k `out + "abcdefgh"` appends (320 KB result) | 82.36x (2426 / 29) | 84.30x (2464 / 29) |
| trait-dispatch | 80M alternating generic calls through a `Step` bound | 1.52x (861 / 565) | 1.51x (875 / 579) |

Times are p50 ms (hd / JS); ratios from p50. Full p50/p95 tables are in
the two script outputs. Geomean hd/JS: **7.00x debug, 6.97x release**.
The debug pass reproduces Q22 (6.95x) within a few percent.

## Does release change the picture? No — by construction, for now

Release changes no ratio beyond run-to-run noise (largest move is
int-loop, 3.03x → 2.89x, inside overlapping p95 ranges). The reason is
in `hd_cli/src/main.rs`: `build_package(release)` passes the flag only
to `write_module` to pick `build/release/` over `build/debug/`; the
`compile(&program)` call takes no profile, so the emitted Wasm is
identical. In particular:

- **string-build (82–84x): release changes nothing.** The quadratic
  `bytes_concat` append stays; no rope/builder appears in either
  profile. Representation issue stands, as Q22 said.
- **map-count (60–61x): release changes nothing.** Per-op boxing and the
  full hash + equality call per `get`/insert are identical in both
  profiles. Implementation-slip hypothesis stands, as Q22 said.
- **int-loop (2.9–3.0x): no verdict on checked arithmetic.** Q22 hoped a
  release profile without overflow checks would separate checks from
  codegen. Since release keeps the same checked lowering
  (`hd_wasm::emit::checked`), this pass cannot say how much of the ~3x
  is checks vs codegen. That separation needs a profile that actually
  drops the checks.

## Limits (carried from Q22)

- `hd build --release` currently selects only the output directory; it
  is not an optimizing profile. A ratio comparison that can move needs a
  profile that changes codegen (unchecked arithmetic, unboxed
  small-int maps, rope strings).
- Node startup (~30 ms) is inside every sample; negligible next to the
  0.3–2.5 s workloads.
- Machine load was elevated during both passes (other agents working);
  absolute times wobble, but hd and JS ran interleaved on the same
  machine so the ratios hold.
- `for x in map` iteration is still unemitted, so map-count verifies
  with `get_or` reads instead of a traversal, as in Q22.
