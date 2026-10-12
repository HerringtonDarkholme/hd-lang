# wasmtime spike (M2)

Repro for `audit/compiler/wasmtime-audit-2026-10-10.md`:

```sh
WASMTIME=/path/to/wasmtime sh bench/wasmtime/run.sh
```

Builds the six `bench/runtime/progs/` programs, the `fib`/`data`/`std_types`
samples and two synthetic host-free programs with the current `hd`,
validates every module with the wasmtime CLI (GC flags on), and times
wasmtime vs Node on the host-free ones. Real programs import
`hd:Console`/`hd:rt` host functions, so only validation applies to them;
the synthetic programs run via `--invoke hd.poll` (see the audit doc).
