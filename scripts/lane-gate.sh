#!/usr/bin/env bash
# The gate an ownership-migration step (#1399) passes before it is pushed.
#
#   scripts/lane-gate.sh STEP [CASES]
#
# CASES is a comma-separated list of conformance path or rule parts, the ones this
# step can reach, or `none`. Fixtures this branch adds or changes always run too.
#
# Stops at the first failing stage and prints its log tail:
#   plancheck on #1399, then the step's evidence lines (--show STEP);
#   fmt, clippy on the ownership crates;
#   hd_tir tests; hd_driver --lib --test stages (--test all is CI's);
#   hd_wasm_linear --test stages;
#   conformance over the step's cases: dev, checked and optimized.
# Logs: target/lane-gate/STEP/. Everything else in conformance is CI's: a regression
# outside the named cases shows up there.
set -uo pipefail
step=${1:?usage: scripts/lane-gate.sh STEP [CASES]}
named=${2:-none}
root=$(git rev-parse --show-toplevel) || exit 2
cd "$root" || exit 2
logs="$root/target/lane-gate/$step"
mkdir -p "$logs"
export RUST_TEST_THREADS=2 CARGO_BUILD_JOBS=2 HD_TEST_JOBS=4
touched=$(git diff --name-only origin/main...HEAD -- spec/conformance/runtime spec/conformance/typing | sed -nE 's#.*/([^/]+)\.hd$#\1#p' | paste -sd, -)
cases=$(printf '%s,%s' "$named" "$touched" | sed -E 's/(^|,)none(,|$)/\1/; s/^,+//; s/,+$//')
conf=specification_conformance_passes_except_known_failures

run() {
  local name=$1; shift
  printf '%-16s' "$name"
  local t0=$SECONDS
  if "$@" >"$logs/$name.log" 2>&1 && ! grep -Eq 'FAILED|panicked' "$logs/$name.log"; then
    echo "ok   $((SECONDS - t0))s"
  else
    echo "FAIL $((SECONDS - t0))s  $logs/$name.log"
    tail -20 "$logs/$name.log"
    exit 1
  fi
}

gh issue view 1399 --json body -q .body >"$logs/plan.md" || { echo "cannot fetch #1399"; exit 2; }
run plancheck python3 scripts/plancheck.py "$logs/plan.md" --show "$step"
python3 scripts/plancheck.py "$logs/plan.md" --show "$step" | grep -E "^  " >"$logs/evidence.txt"
echo "evidence: $(wc -l <"$logs/evidence.txt" | tr -d ' ') lines for $step in $logs/evidence.txt"
run fmt cargo fmt --check
run clippy cargo clippy -q -p hd_tir -p hd_opt -p hd_driver -p hd_wasm_linear --all-targets -- -D warnings
run hd_tir cargo test -q -p hd_tir -p hd_opt
run hd_driver cargo test -q -p hd_driver --lib --test stages
run wasm_stages cargo test -q -p hd_wasm_linear --test stages
if [ -n "$cases" ]; then
echo "conformance cases: $cases"
run conf_dev env HD_CONFORMANCE_ONLY=$cases cargo test -q -p hd_driver --test conformance $conf -- --exact
run conf_checked env HD_CONFORMANCE_CHECKED=1 HD_CONFORMANCE_ONLY=$cases cargo test -q -p hd_driver --test conformance $conf -- --exact
# Release-only ownership work (summaries, interprocedural facts) runs only in the
# optimized pipeline; #1405's overflow passed the two runs above. Node engine, no wasmtime.
run conf_opt env HD_CONFORMANCE_PIPELINE=optimized HD_CONFORMANCE_ONLY=$cases cargo test -q -p hd_driver --test conformance $conf -- --exact
else
echo "conformance: no cases named or touched; CI runs them"
fi
# std's own hd tests on the linear backend: ownership steps can break them
# (23b1 double-dropped a suspending dyn method's argument; only this caught it).
run std_rt cargo test -q -p hd_driver --test conformance -- --exact std_runtime_modules_pass_their_hd_tests_on_the_linear_backend
echo PASS
