# wasmtime spike (M2)

Repro for the 2026-10-10 wasmtime spike (hd `7ed3cf50`, wasmtime 49.0.2).
Result: every module the emitter produced validated with only Tier 1,
default-on features (gc, function-references, multi-value, bulk-memory,
reference-types, extended-const), so no emitter change was needed. On
host-free programs wasmtime started in 8 ms against Node's 38 ms and ran a
120M-iteration i32 loop in 615 ms against 1217 ms. The report is in git
history (`git show 44890c30:audit/compiler/wasmtime-audit-2026-10-10.md`).
Run:

```sh
WASMTIME=/path/to/wasmtime sh bench/wasmtime/run.sh
```

Builds the six `bench/runtime/progs/` programs, the `fib`/`data`/`std_types`
samples and two synthetic host-free programs with the current `hd`,
validates every module with the wasmtime CLI (GC flags on), and times
wasmtime vs Node on the host-free ones. Real programs import
`hd:Console`/`hd:rt` host functions, so only validation applies to them;
the synthetic programs run via `--invoke hd.poll` (see the audit doc).
