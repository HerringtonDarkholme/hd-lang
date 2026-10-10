# wasmtime Feature Audit And Spike (M2, 2026-10-10)

Owner committed to wasmtime (2026-10-10): the CLI must not need Node.
Timebox 60 minutes. hd commit `7ed3cf50`; wasmtime `49.0.2` (aarch64-macos,
`3c8a3e79a 2026-10-02`), the newest release on 2026-10-10.

## 1. Wasm features the emitter uses

Read `compiler/crates/hd_wasm/src/` (encoded with `wasm-encoder 0.261.0`
via `InstructionSink`, `asm.rs:77-78`, plus raw `0xFB` GC bytes,
`asm.rs:80-88`). No `cfg(feature)` gates; conditionals are runtime
emission choices.

| Proposal | What the emitter uses | Code locations |
| --- | --- | --- |
| GC: struct/array types | `struct`/`array`/`sub`/`rec` types, `eqref`, concrete `funcref` heap types, final/non-final subtypes | `lib.rs:1055-1146`, `asm.rs:250-272` |
| GC: struct ops | `struct.new`, `struct.new_default`, `struct.get`, `struct.set` | `asm.rs:149-166`, `emit.rs:618,651`, `boundary.rs:1022-1024` |
| GC: array ops | `array.new_default`, `array.new_fixed`, `array.new_data`, `array.get[_u]`, `array.set`, `array.len`, **`array.copy`** | `asm.rs:167-197`, `emit.rs:1200,3196`, `rt.rs:910` |
| GC: casts | `ref.cast` (nullable and non-null). `br_on_cast`/`br_on_null` never emitted; branch-on-cast is `ref.is_null` + `br_if`/`select` | `asm.rs:198-201`, `emit.rs:1046,2000,4453` |
| function-references | `ref.func`, `call_ref`, concrete function heap types; declared element segment only when a `ref.func` exists | `asm.rs:141-148`, `lib.rs:1369-1410`, `emit.rs:599,2352,2555,2757` |
| multi-value | multi-param/multi-result function types; block types are `Empty` except one `Result(I32)` | `lib.rs:70,593-594,1131-1134`, `rt.rs:1779`, `asm.rs:116-124` |
| extended-const | `struct.new_default` / `ref.func` / `struct.new` / `array.new_fixed` inside const expressions for globals | `lib.rs:1313,1481-1502` |
| bulk-memory (subset) | one passive data segment + `DataCount`, consumed only by `array.new_data`. No `memory.copy`/`fill`/`init`/`table.*` | `lib.rs:1226-1238,1419-1423` |
| nontrapping-fptoint | `i32/i64.trunc_sat_f32/f64_s/u` | `emit.rs:1476-1485` |
| sign-extension-ops | `i32.extend8_s`, `i32.extend16_s` | `emit.rs:1519-1522` |
| MVP core | 32-bit unshared linear memory (`hd.x`), unshared globals, names section | `lib.rs:1275-1344,1395-1444` |

Explicitly **not** used (zero hits in `src/`): `i31`, `ref.test`,
`br_on_null`/`br_on_cast`, `array.fill`/`array.init_elem`,
`call_indirect`/`table.get`/`set`, tail calls (`return_call`),
exceptions (`try`/`catch`/`throw`/`exnref`), SIMD, threads/atomics
(`shared:false` hardcoded), `memory64` (explicit `false`), externref
(only `eq` + concrete refs), stringref, multiple memories.

## 2. wasmtime release pick and flags

Newest release supporting everything above: **v49.0.2** (latest on
2026-10-10). Per `docs.wasmtime.dev/stability-wasm-proposals` every used
proposal is Tier 1 ✅ (gc, function-references, multi-value,
bulk-memory, reference-types, extended-const, nontrapping-fptoint,
sign-extension-ops). Nothing the emitter uses is unsupported, so **no
emitter avoidance is needed**.

CLI flags (`wasmtime run -W help`, v49.0.2) and `Config` equivalents:

| Feature | CLI flag | `Config` |
| --- | --- | --- |
| GC | `-W gc=y` | `Config::wasm_gc(true)` |
| typed function refs | `-W function-references=y` | `Config::wasm_function_references(true)` |
| extended-const | `-W extended-const=y` | `Config::wasm_extended_const(true)` |
| multi-value | `-W multi-value=y` | `Config::wasm_multi_value(true)` |
| bulk-memory | `-W bulk-memory=y` | `Config::wasm_bulk_memory(true)` |
| reference-types | `-W reference-types=y` | `Config::wasm_reference_types(true)` |

Verified empirically: all spike modules validate with default flags
(gc/function-references are **on by default** in the v49 CLI); with
`-W gc=n` validation rejects at offset `0x1f` with "array indexed types
not supported without the gc feature". Pass the flags explicitly anyway
so the port does not depend on CLI defaults.

Notes for later jobs, not blockers: tail-call and exception-handling are
also Tier 1 in v49 but unused, so they constrain nothing today.
`stack-switching` (possibly relevant to the M3 suspension design) is
Tier 3, work-in-progress, currently x86_64-Linux-only.

## 3. Spike (`compiler/bench/wasmtime/`)

Method: built the six programs in `compiler/bench/runtime/progs/` and
the `fib`, `data`, `std_types` samples with the current `hd` (debug
profile, scratch `HD_CACHE`), listed imports via
`WebAssembly.Module.imports`, validated each with
`wasmtime compile -W gc=y,function-references=y,extended-const=y`, and
timed wasmtime vs Node. Repro script: `compiler/bench/wasmtime/run.sh`.

Findings:

- All six progs import host functions
  (`hd:Console.write_line.start/finish`, `hd:rt.abort/block/stderr`), so
  none runs bare: the wasmtime CLI only auto-hooks WASI imports and
  fails on anything else. Same for the three samples. 11/11 modules
  **validate** with GC flags on.
- Runnable comparison used two synthetic host-free programs (same
  recipe: package with `pub fn main() -> void`, no `$ Console`): `pure`
  (trivial body; one `hd:rt.stderr` import, never called, stubbed with
  `-W unknown-imports-trap=y`) and `pureloop` (120M-iteration i32
  checksum loop, no observable output). Both export `hd.init`/`hd.poll`
  (the host calls init once, then polls to completion per
  `compiler/host/core.mjs`); the spike invokes `hd.poll` via
  `--invoke`, which returns `0`, matching the Node host run (exit 0).
- The loop is really executing (not dead-code-eliminated): it takes
  ~0.6 s / ~1.2 s per run on the two engines.

Timings, 1 warmup + 5 measured, wall ms (machine load ~14-17, not idle):

| Case | wasmtime 49.0.2 | Node v24.19.0 (`host/run.mjs`) |
| --- | --- | --- |
| startup (`pure` poll) | 8, 10, 8, 8, 7 | 38, 39, 39, 38, 37 |
| steady (`pureloop` 120M checksum) | 615, 617, 619, 616, 618 | 1216, 1218, 1217, 1218, 1219 |

Startup (compile + instantiate + run a trivial module) is ~3.5x faster
under wasmtime; steady-state compute on identical emitted code is
~1.96x faster under wasmtime. wasmtime CLI invocations may benefit from
its on-disk code cache after warmup; the comparison still measures what
a CLI user would feel.

## Recommendation

Proceed with the wasmtime port: the newest release validates every
module the emitter produces with only Tier 1, default-on features, and
the spike shows wasmtime beating Node on both startup and steady state
for emitter output. The gating work is the host port itself (M3): every
real program needs `hd:Console`/`hd:rt` imports that only a Rust host
can satisfy. No emitter changes are required for wasmtime validation.
