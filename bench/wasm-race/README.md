# Four-language Wasm race

Egoist's workloads from #1291 in hd, Go, Zig, MoonBit and Silk. Each program is compiled to one Wasm module and run on the plain wasmtime CLI, with the same binary and flags for all five.

The programs measure each language and its standard library as a whole. They stay at parity and use std. When hd cannot express a workload the way the others do, that is a std bug. When hd needs special code to avoid a perf cliff, that is a language bug. Neither is hidden by editing the program. See the tracking issue for results.

## Build

Run from this directory. `HD` is a built `hd` compiler.

```sh
mkdir -p bin
(cd shim && cargo build --release)
(cd hd && $HD build --release)
shim/target/release/hd-wasi-shim hd/build/release/wasm_bench.wasm bin/hd.wasm
(cd go && GOOS=wasip1 GOARCH=wasm go build -o ../bin/go.wasm .)
(cd zig && zig build-exe main.zig -target wasm32-wasi -O ReleaseSafe -lc -femit-bin=../bin/zig.wasm)
(cd moonbit && moon build --target wasm --release)
shim/target/release/moon_stub moonbit/_build/wasm/release/build/cmd/main/main.wasm bin/moonbit.wasm
(cd silk && silk build --target wasm32-wasip1 --release)
cp silk/build/llvm/wasm32-wasip1/release/wasm_bench bin/silk.wasm
```

Silk's release profile is `speed` with the LLVM backend; its link step uses Clang 22 or newer (`--clang <path>` or `SILK_CLANG`, else `clang` on `PATH`). The module is a WASI command with `_start`, so it needs no shim.

`hd-wasi-shim` maps hd's `hd:*` host imports to WASI and adds `_start`. It fixes the hash seed. `moon_stub` replaces MoonBit's externref clock and argument imports with WASI ones.

## Run

```sh
python3 -I run_cli.py hd=bin/hd.wasm go=bin/go.wasm zig=bin/zig.wasm moonbit=bin/moonbit.wasm silk=bin/silk.wasm
```

The flags are `-O opt-level=2 -W max-wasm-stack=4194304`. It runs one discarded round, then 15 interleaved rounds rotating the starting language; `ROUNDS=5` runs 5. Each cell is the median in milliseconds, with the range in parentheses. The last column compares checksums across languages.
