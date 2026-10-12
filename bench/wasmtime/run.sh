#!/bin/sh
# Spike for M2 (audit/compiler/wasmtime-audit-2026-10-10.md): build the six
# runtime progs, three samples and two synthetic host-free programs with
# the current `hd`, validate every module with the wasmtime CLI (GC flags
# on), and time wasmtime vs Node on the host-free ones.
#
# Usage: HD_BIN=... WASMTIME=... sh bench/wasmtime/run.sh [workdir]
# Defaults: HD_BIN=target/release/hd (must already be built).
# WASMTIME must point at a wasmtime CLI (spike used v49.0.2).
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
HD_BIN=${HD_BIN:-"$REPO/compiler/target/release/hd"}
: "${WASMTIME:?set WASMTIME to a wasmtime CLI binary}"
WORK=${1:-$(mktemp -d hd-wasmtime-XXXXXX)}
CACHE=$WORK/cache
WFLAGS="-W gc=y,function-references=y,extended-const=y"
TRAP="-W unknown-imports-trap=y"
mkdir -p "$CACHE"

build_prog() {
  name=$1; src=$2
  dir=$WORK/$name
  rm -rf "$dir"; mkdir -p "$dir/src"
  printf '[package]\nname = "%s"\n' "$name" > "$dir/hd.toml"
  cp "$src" "$dir/src/main.hd"
  (cd "$dir" && HD_CACHE="$CACHE" "$HD_BIN" build >/dev/null)
  echo "$dir/build/debug/$name.wasm"
}

build_sample() {
  name=$1
  dir=$WORK/sample-$name
  rm -rf "$dir"; mkdir -p "$dir"
  cp -r "$REPO/compiler/samples/$name/"* "$dir/"
  (cd "$dir" && HD_CACHE="$CACHE" "$HD_BIN" build >/dev/null)
  find "$dir/build/debug" -name '*.wasm'
}

time5() {
  # time5 LABEL -- cmd...  (1 warmup + 5 measured)
  label=$1; shift; shift
  "$@" >/dev/null 2>&1
  i=1; while [ $i -le 5 ]; do
    s=$(date +%s%N)
    "$@" >/dev/null 2>&1
    e=$(date +%s%N)
    echo "$label run $i: $(( (e - s) / 1000000 ))ms"
    i=$((i + 1))
  done
}

echo "## validate: six progs + three samples"
for p in closures int-loop list-sort map-count string-build trait-dispatch; do
  w=$(build_prog "$p" "$REPO/compiler/bench/runtime/progs/$p.hd")
  node -e 'const m=new WebAssembly.Module(require("fs").readFileSync(process.argv[1]));console.log(process.argv[2], "imports:", JSON.stringify(WebAssembly.Module.imports(m).map(i=>i.module+"."+i.name)))' "$w" "$p"
  "$WASMTIME" compile $WFLAGS -o /dev/null "$w" && echo "$p: VALIDATE_OK"
done
for s in fib data std_types; do
  w=$(build_sample "$s")
  "$WASMTIME" compile $WFLAGS -o /dev/null "$w" && echo "sample-$s: VALIDATE_OK"
done

echo "## host-free programs (no \$Console): trivial + 120M checksum loop"
printf 'pub fn main() -> void:\n    let x = +40\n    let _y = x + 2\n' > "$WORK/pure.hd"
PURE=$(build_prog pure "$WORK/pure.hd")
{
  echo 'fn checksum(n: i32) -> i32:'
  echo '    let total = +0'
  echo '    let i = +0'
  echo '    while i < n:'
  echo '        r := i % 10007'
  echo '        total = (total + (r * r) % 10007) % 1000003'
  echo '        i = i + 1'
  echo '    total'
  echo ''
  echo 'pub fn main() -> void:'
  echo '    let _c = checksum(120000000)'
} > "$WORK/pureloop.hd"
LOOP=$(build_prog pureloop "$WORK/pureloop.hd")
"$WASMTIME" compile $WFLAGS -o /dev/null "$PURE" && echo "pure: VALIDATE_OK"
"$WASMTIME" compile $WFLAGS -o /dev/null "$LOOP" && echo "pureloop: VALIDATE_OK"

echo "## startup: wasmtime vs node on pure"
# shellcheck disable=SC2086
time5 wasmtime-startup -- "$WASMTIME" run $WFLAGS $TRAP --invoke hd.poll "$PURE"
time5 node-startup -- node "$REPO/compiler/host/run.mjs" "$PURE"
echo "## steady state: wasmtime vs node on pureloop"
# shellcheck disable=SC2086
time5 wasmtime-steady -- "$WASMTIME" run $WFLAGS $TRAP --invoke hd.poll "$LOOP"
time5 node-steady -- node "$REPO/compiler/host/run.mjs" "$LOOP"
echo "workdir kept at: $WORK"
