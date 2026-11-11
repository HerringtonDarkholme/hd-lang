#!/bin/sh
# What remains of the 2026-10-10 wasmtime spike (see README.md): build
# every bench/runtime/progs program and the fib, data and std_types
# samples with the current `hd`, in the debug and release profiles, print
# each program's imports, and validate every module with the wasmtime CLI.
# It times nothing: runtime numbers come from V8, and the spike's timings
# are in git history.
#
# Usage: WASMTIME=... sh bench/wasmtime/run.sh [workdir]
# HD_BIN defaults to target/release/hd (must already be built).
# WASMTIME must point at a wasmtime CLI (spike used v49.0.2).
# Exits non-zero when a build or a validation fails.
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
HD_BIN=${HD_BIN:-"$REPO/target/release/hd"}
: "${WASMTIME:?set WASMTIME to a wasmtime CLI binary}"
WORK=${1:-$(mktemp -d "${TMPDIR:-/tmp}/hd-wasmtime-XXXXXX")}
CACHE=$WORK/cache
WFLAGS="-W gc=y,function-references=y,extended-const=y"
mkdir -p "$CACHE"
FAILED=0

# `set -e` does not apply inside a function called from an `if`, so each
# step that can fail returns its own status.
hd_build() {
  dir=$1
  (cd "$dir" && HD_CACHE="$CACHE" "$HD_BIN" build >/dev/null \
    && HD_CACHE="$CACHE" "$HD_BIN" build --release >/dev/null)
}

build_prog() {
  name=$1; src=$2
  dir=$WORK/$name
  rm -rf "$dir"; mkdir -p "$dir/src" || return 1
  printf '[package]\nname = "%s"\n\n[capabilities]\nConsole = true\n' "$name" > "$dir/hd.toml"
  cp "$src" "$dir/src/main.hd" || return 1
  hd_build "$dir" || return 1
  echo "$dir/build/debug/$name.wasm $dir/build/release/$name.wasm"
}

build_sample() {
  name=$1
  dir=$WORK/sample-$name
  rm -rf "$dir"; mkdir -p "$dir" || return 1
  cp -r "$REPO/examples/$name/"* "$dir/" || return 1
  hd_build "$dir" || return 1
  find "$dir/build/debug" "$dir/build/release" -name '*.wasm'
}

validate() {
  label=$1; wasm=$2
  # shellcheck disable=SC2086
  if "$WASMTIME" compile $WFLAGS -o /dev/null "$wasm"; then
    echo "$label: VALIDATE_OK"
  else
    echo "$label: VALIDATE_FAILED"
    FAILED=$((FAILED + 1))
  fi
}

echo "## validate: bench/runtime/progs and three samples, debug and release"
for src in "$REPO"/bench/runtime/progs/*.hd; do
  p=$(basename "$src" .hd)
  if ! w=$(build_prog "$p" "$src"); then
    echo "$p: BUILD_FAILED"
    FAILED=$((FAILED + 1))
    continue
  fi
  for module in $w; do
    label="$p $(basename "$(dirname "$module")")"
    node -e 'const m=new WebAssembly.Module(require("fs").readFileSync(process.argv[1]));console.log(process.argv[2], "imports:", JSON.stringify(WebAssembly.Module.imports(m).map(i=>i.module+"."+i.name)))' "$module" "$label" \
      || { echo "$label: V8_REJECTED"; FAILED=$((FAILED + 1)); }
    validate "$label" "$module"
  done
done
for s in fib data std_types; do
  if ! w=$(build_sample "$s"); then
    echo "sample-$s: BUILD_FAILED"
    FAILED=$((FAILED + 1))
    continue
  fi
  for module in $w; do
    validate "sample-$s $(basename "$(dirname "$module")")" "$module"
  done
done
echo "workdir kept at: $WORK"
[ "$FAILED" -eq 0 ] || { echo "$FAILED failed"; exit 1; }
