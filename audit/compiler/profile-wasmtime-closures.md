# Profile: closures bench on wasmtime vs V8, and hd binary size (#52)

hd commit `378a40d5`, 2026-10-12. Read-only job: no compiler crate edits.
Bench: `compiler/bench/runtime/progs/closures.hd` (fill 20M list, then
`iter().map(f).filter(g).fold(h)`), built once with dev `hd` to one
`.wasm`, run warm on each engine (1 warmup + 5–7 measured per cell).
V8 = `node compiler/host/run.mjs`; wasmtime = dev `hd --engine wasmtime`
(`--features wasmtime` build). Machine load 9–14 during the runs, so
cells are ranges with p50; outliers noted, not hidden.

## Headline numbers (dev profile, same .wasm both engines)

| Program | V8 p50 | Wasmtime p50 | wm/V8 |
| --- | --- | --- | --- |
| closures (full) | 386 ms | 2047 ms | 5.3x |
| closures `--release` | 378 ms | ~1900 ms | ~5x |
| startup (trivial) | 39 ms | 12 ms | 0.3x |

The T278 claim of 3.6x did not reproduce here: 5.3x dev, ~5x release
(Cranelift Speed ≈ None on this bench; one 6936 ms spike from machine
contention excluded from the p50). Wasmtime wins process startup; the
whole deficit is execution. Checksums match on both engines everywhere.

Cross-program ratios (dev p50) rule out the obvious suspects:

| Program | Shape | wm/V8 |
| --- | --- | --- |
| int-loop | 240M `%` accumulations, no closures | 0.40x |
| trait-dispatch | 80M generic calls, no closures | 0.81x |
| list-sort | 30k insertion sort | ~1x |
| map-count | 200k inserts + reads | ~1x |
| string-build | 40k appends | ~0.15x |

Wasmtime is at parity or faster on arithmetic, dispatch count, sorting,
maps, and strings — even at Cranelift `-O0`. Not general codegen, not
call count, not host calls (one `write_line` per run).

## Decomposition of the closures gap (20M items, ms p50)

Variants built from copies of the bench (semantics preserved:
loop and fold-only print the same 861551 on both engines):

| Variant | V8 | Wasmtime | Excess vs V8 |
| --- | --- | --- | --- |
| fill only | 209 | 127 | -82 (faster) |
| + plain `for` loop | 360 | 565 | +287 |
| + one `fold` closure | 380 | 875 | +290 more |
| + `map`/`filter` layers (full) | 386 | 2050 | +1169 more |

Bulk allocation is faster on wasmtime. The ~1664 ms gap is ~70%
intermediate lazy layers, ~17% closure invocation, ~17% per-item
indexing shape. On V8 the two adapter layers cost +6 ms total;
on wasmtime +1175 ms (~29 ns/item/layer). One `fold` closure alone
costs +310 ms on wasmtime vs +20 ms on V8.

## Top 3 causes

### 1. Intermediate lazy adapter layers (~70% of the gap)
Each `map`/`filter` is an `Iterator::from_fn` closure over the previous
layer (`lib/std/iter.hd:40-81`): per item, a nested closure call through
a captured env struct, an `Option` wrap, and a `match` — per layer.
Cranelift keeps all of it at `None` and at `Speed`; Turbofan fuses it
to ~0 (full costs V8 +6 ms over fold-only).
Proposed fix: fuse iterator chains at compile time (specialize
fold-over-map/filter into one loop) so no per-item layer traffic
survives to either backend. Owner-side design; no edit made here.

### 2. Indirect closure invocation (~17% of the gap)
The three bench closures capture nothing, yet every call goes through
an env struct and `call_indirect`. Generic direct calls do not suffer
(trait-dispatch at 0.81x), so it is the indirection, not the count.
Proposed fix: devirtualize known closure calls (#53) / capture-free
closures and adapters as constant globals (`audit/compiler/later.md`
Phase 3, codegen §12.2/§13.11). Owner-side design; no edit made here.

### 3. Per-access GC/bounds-check shape (~17% of the gap)
Iterator cursor loads, env struct field accesses, and list bounds
checks stay fully expanded under Cranelift at both opt levels; the
release run (Speed + wrapping arithmetic) barely moved wasmtime
(~2050 → ~1900 ms). This is backend shape, not the opt flag.
Proposed fix: bounds-check elimination and struct-field load folding
for List cursor paths in `hd_wasm` emission, or accept the dev-profile
cost and document that adapter-heavy code wants `--release` plus
cause-1 fusion. Owner-side design; no edit made here.

Ruled out with evidence: host-call overhead (one call per run),
bulk allocation (fill is 0.61x), arithmetic codegen (int-loop 0.40x),
dispatch count (trait-dispatch 0.81x), and the opt flag itself
(Speed ≈ None on this bench: `engine.rs:33-44`,
`cache.rs:131-132` keys the compile cache by opt level, so the
release measurement is genuine).

## hd binary size with wasmtime (dev profile, `cargo bloat`)

| Binary | File | .text |
| --- | --- | --- |
| plain `hd` | 11.6 MiB | 4.9 MiB |
| `hd --features wasmtime` | 32.8 MiB | 13.5 MiB |

Delta +21.2 MiB (+183%). Largest new `.text` contributors:
`cranelift_codegen` 3.6 MiB, `wasmparser` 838 KiB,
`wasmtime_internal_cranelift` 820 KiB, `wasmtime` 641 KiB,
`hd_run_wasmtime` 475 KiB, `rustls` 450 KiB,
`wasmtime_environ` 219 KiB. The wasmtime dependency is already minimal
(`default-features = false`: cranelift, gc, gc-copying,
pooling-allocator, runtime, std; `hd_run_wasmtime/Cargo.toml:23-30`),
so no further feature trimming is available.
Proposed fixes: keep wasmtime off by default so the shipped slim `hd`
stays 11.6 MiB (already the case); ship stripped binaries (dev profile
carries debuginfo); if size matters more, gate `pooling-allocator`
and test-only paths behind the test engine only.

## Method notes
Scratch `HD_CACHE`, bench packages, and timing scripts lived in the
worktree (`.bench-52*`, `.hd-cache-52`) and `/tmp`, all removed after
the runs; only this report is committed. Variant sources were copies,
not edits to `compiler/bench/`. Release timing used `hd build
--release` output run with `hd --engine wasmtime --release`
(`engine.rs:272-283` selects the Speed pipeline); the dev/release
comparison is measurement only, not a gate-profile change.
