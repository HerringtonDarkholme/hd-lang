#!/usr/bin/env bash
set -euo pipefail

tools_root="$(cd -- "${BASH_SOURCE[0]%/*}/.." && pwd)"
if [[ $# -eq 0 ]]; then
  printf '%s\n' 'usage: hd-tools.sh TASK [ARGS...]' >&2
  exit 2
fi
tools_task="$1"
shift

# Exact local paths, then PATH; no compiler installation or build. CI sets
# HD=hd to select setup-hd's pinned stage0 even when main's build is present.
if [[ -n "${HD:-}" ]]; then
  tools_compiler="$(command -v -- "$HD" || true)"
elif [[ -x "$tools_root/target/release/hd" ]]; then
  tools_compiler="$tools_root/target/release/hd"
elif [[ -x "$tools_root/target/debug/hd" ]]; then
  tools_compiler="$tools_root/target/debug/hd"
else
  tools_compiler="$(command -v hd || true)"
fi
if [[ -z "$tools_compiler" || ! -x "$tools_compiler" ]]; then
  printf '%s\n' 'tools: no existing hd found; set HD or PATH to a Wasmtime-enabled compiler' >&2
  exit 2
fi
if [[ "$tools_compiler" != /* ]]; then
  tools_compiler="$PWD/$tools_compiler"
fi
export HD_CACHE="${HD_CACHE:-$tools_root/target/tools-cache}"
export HD_JOBS=2
cd -- "$tools_root/tools"
exec "$tools_compiler" run "$tools_task" --release --engine wasmtime \
  --time-limit "${HD_TOOLS_TIME_LIMIT:-10min}" -- "$@"
