#!/usr/bin/env bash
# The gate an ownership-migration step (#1399) passes before it is pushed.
#
#   scripts/lane-gate.sh STEP
#
# Stops at the first failing stage and prints its log tail:
#   plancheck on #1399, then the step's evidence lines (--show STEP);
#   fmt, clippy on the ownership crates;
#   hd_tir tests; hd_driver --lib --test stages (--test all is CI's);
#   hd_wasm_linear --test stages;
#   focused conformance over the ownership cases, dev and checked.
# Logs: target/lane-gate/STEP/. The full conformance matrix is CI's.
set -uo pipefail
step=${1:?usage: scripts/lane-gate.sh STEP}
root=$(git rev-parse --show-toplevel) || exit 2
cd "$root" || exit 2
logs="$root/target/lane-gate/$step"
mkdir -p "$logs"
export RUST_TEST_THREADS=2 CARGO_BUILD_JOBS=2
cases=binding,assign,field,match,return,for,while,loop,closure,capture,move,borrow,copy,uniq,list,map,string,pointer,box,reference,erased,dyn,suspend,provider,using,drop,cell
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
run plancheck python3 scripts/plancheck.py "$logs/plan.md"
python3 scripts/plancheck.py "$logs/plan.md" --show "$step" | grep -E "^  " >"$logs/evidence.txt"
echo "evidence: $(wc -l <"$logs/evidence.txt" | tr -d ' ') lines for $step in $logs/evidence.txt"
run fmt cargo fmt --check
run clippy cargo clippy -q -p hd_tir -p hd_driver -p hd_wasm_linear --all-targets -- -D warnings
run hd_tir cargo test -q -p hd_tir
run hd_driver cargo test -q -p hd_driver --lib --test stages
run wasm_stages cargo test -q -p hd_wasm_linear --test stages
run conf_dev env HD_CONFORMANCE_ONLY=$cases cargo test -q -p hd_driver --test conformance $conf -- --exact
run conf_checked env HD_CONFORMANCE_CHECKED=1 HD_CONFORMANCE_ONLY=$cases cargo test -q -p hd_driver --test conformance $conf -- --exact
echo PASS
