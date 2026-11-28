#!/usr/bin/env bash
set -euo pipefail

fuzz_root="$(cd -- "${BASH_SOURCE[0]%/*}/.." && pwd)"
# Cost: exact existing paths, then the shell's PATH lookup; no tree scan,
# compiler installation or compiler build. HD explicitly selects an existing
# compiler; otherwise use the owner's release/debug/PATH priority.
if [[ -n "${HD:-}" ]]; then
  fuzz_compiler="$(command -v -- "$HD" || true)"
elif [[ -x "$fuzz_root/target/release/hd" ]]; then
  fuzz_compiler="$fuzz_root/target/release/hd"
elif [[ -x "$fuzz_root/target/debug/hd" ]]; then
  fuzz_compiler="$fuzz_root/target/debug/hd"
else
  fuzz_compiler="$(command -v hd || true)"
fi
if [[ -z "$fuzz_compiler" || ! -x "$fuzz_compiler" ]]; then
  printf '%s\n' 'fuzz: no existing hd found; set HD or PATH to an existing Wasmtime-enabled compiler' >&2
  exit 2
fi
if [[ "$fuzz_compiler" != /* ]]; then
  fuzz_compiler="$PWD/$fuzz_compiler"
fi
fuzz_args=("$@")
fuzz_explicit=false
for fuzz_arg in "$@"; do
  if [[ "$fuzz_arg" == --hd || "$fuzz_arg" == --hd=* ]]; then
    fuzz_explicit=true
  fi
done
if [[ "$fuzz_explicit" == false ]]; then
  fuzz_args=(--hd "$fuzz_compiler" "${fuzz_args[@]}")
fi
export HD_CACHE="${HD_CACHE:-$fuzz_root/target/fuzz-driver-cache}"
export HD_JOBS=2
cd -- "$fuzz_root/tools/fuzz"
exec "$fuzz_compiler" run --engine wasmtime --time-limit "${HD_FUZZ_TIME_LIMIT:-1h}" -- "${fuzz_args[@]}"
