# wasmtime spike (M2)

Repro for the 2026-10-10 wasmtime spike (hd `7ed3cf50`, wasmtime 49.0.2).
Result: every module the emitter produced validated with only Tier 1,
default-on features (gc, function-references, multi-value, bulk-memory,
reference-types, extended-const), so no emitter change was needed. The
spike also timed wasmtime against Node; those numbers and the report are in
git history (`git show 44890c30:audit/compiler/wasmtime-audit-2026-10-10.md`).
Runtime numbers now come from V8 only (`bench/runtime/run.ts`).
Run, after `cargo build --release -p hd_cli`:

```sh
WASMTIME=/path/to/wasmtime sh bench/wasmtime/run.sh
```

Builds every `bench/runtime/progs/` program and the `fib`/`data`/`std_types`
examples with the current `hd`, in the debug and release profiles, prints each
program's imports, and validates every module with the wasmtime CLI (GC flags
on). It exits non-zero when a build or a validation fails. It times nothing.
