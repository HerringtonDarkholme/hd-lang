#!/usr/bin/env bash
# The Rust hd under the conformance command contract
# (spec/conformance/README.md, Command Contract), the default compiler of
# spec/check.sh, spec/tools/run-conformance.ts and spec/tools/fuzz.
#
# `hd` has no `parse` command (the CLI specification removed it), so
# `parse FILE` runs the parser alone, through hd_syntax's parse_check
# example. Every other action goes to target/debug/hd. Each binary is
# built in the dev profile when it is missing. Both live under cargo's
# target directory: CARGO_TARGET_DIR when it is set, else target/ at the
# repository root.
#
# hd prints `error: PATH:LINE:COL: CODE: message`, `warning: PATH:LINE:COL:
# CODE: message` and indented `panic: CODE: message` lines; the contract
# reads `PATH:LINE:COL: CODE: message`, `PATH:LINE:COL: warning: CODE:
# message` and `CODE: message`. This adapter rewrites those lines and passes
# everything else through. A program's own standard output (`hd FILE`) is
# compared exactly, so only `check` and `test` output is rewritten there.
set -euo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
target=${CARGO_TARGET_DIR:-$root/target}

contract() {
    sed -E \
        -e 's/^error: (.*:[0-9]+:[0-9]+: )/\1/' \
        -e 's/^warning: (.*:[0-9]+:[0-9]+: )/\1warning: /' \
        -e 's/^[[:space:]]+panic: //'
}

if [ "${1-}" = parse ]; then
    shift
    parser="$target/debug/examples/parse_check"
    [ -x "$parser" ] ||
        cargo build -q --manifest-path "$root/Cargo.toml" -p hd_syntax --example parse_check >&2
    exec "$parser" "$@"
fi
hd="$target/debug/hd"
[ -x "$hd" ] || cargo build -q --manifest-path "$root/Cargo.toml" -p hd_cli >&2
case "${1-}" in
    check | test) "$hd" "$@" 2> >(contract >&2) | contract ;;
    *) "$hd" "$@" 2> >(contract >&2) ;;
esac
