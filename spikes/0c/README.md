# Spike 0c: representation benchmarks

This is the reproducible harness for experiments E0--E11 in
`representation-runtime.md` and S1--S7 in `representation-compile.md`.
It generates Wasm directly from WAT, without using either hd compiler.
V8 through Node is the primary engine. The checked-in timeboxed report
contains the completed E1 matrix and explicitly marks the other experiments
as deferred; it does not use the harness's Cranelift comparison data.

The Wasmtime dependency is pinned at 49.0.2 and the matching wasm-tools
crates at 0.258.3. The V8 engine is the one embedded in the invoking Node
executable. The runner launches Node once with `--liftoff-only` and once
with `--no-liftoff`.

Run a focused experiment from this directory (E1 is the recorded result):

```sh
cargo run --release -- run --quick --only E1
```

`--quick` uses one warm-up, three kernel samples, one compile sample, and
reduced iteration counts. E10's exact 10,000×200-byte fixed initializer is a
known pathological compile and should be selected separately if revisited.
Results are written to `out/results.json`; the checked-in report was rendered
with:

```sh
cargo run --release -- report out/results.json ../../../future-work/compiler/spike-0c-results.md
```

Generated `.wasm` inputs are kept in `out/modules/`. Each JSON row records
its SHA-256-independent stable case name, full Wasm and code-section sizes,
function/type/global counts, compile and instantiation distributions, and
the experiment-specific metric. CPU measurements use `getrusage`; wall
measurements use the monotonic clock. Peak RSS is process-wide, so the
report treats it as a high-water mark rather than a per-case delta.
