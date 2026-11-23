#!/usr/bin/env bash
# Runs test binaries that CI's one release build made (the Build job of
# .github/workflows/test.yml), as `cargo test` runs them: each from its
# package's directory, the whole list even after a failure.
#
# target/ci-tests.tsv lists one binary per line: package directory, test
# target name, executable, all relative to the checkout. A binary runs when
# "<package directory>/<target name>" matches PATTERN (grep -E), or with -v
# when it does not. Arguments after the pattern go to every binary. Each
# binary's wall and CPU seconds follow its output, so a run's compute can
# be read per binary, apart from how busy the runner was.
#
# Usage: scripts/ci-tests.sh [-v] PATTERN [test binary arguments...]
# Cost: one process per selected binary, nothing built.
set -uo pipefail
invert=
if [ "${1:-}" = -v ]; then
  invert=1
  shift
fi
pattern=$1
shift
root=$PWD
# cargo makes the directory test binaries know as CARGO_TARGET_TMPDIR
# before it runs them; tests write their scratch files there.
mkdir -p target/tmp
status=0
ran=0
while IFS=$'\t' read -r -u 3 dir name exe; do
  if printf '%s/%s\n' "$dir" "$name" | grep -qE "$pattern"; then
    [ -n "$invert" ] && continue
  else
    [ -z "$invert" ] && continue
  fi
  echo "     Running $dir $name ($exe)"
  TIMEFORMAT="     Took %R s wall, %U s user, %S s system: $dir $name"
  # A hung binary is stopped after ${CI_TEST_LIMIT:-600} s and named, so the
  # job ends with a log instead of hitting its own time limit (run
  # 45277379767 hung for 25 min and kept no log).
  time (cd "$root/$dir" && timeout --kill-after=10 "${CI_TEST_LIMIT:-600}" "$root/$exe" "$@")
  code=$?
  if [ "$code" -eq 124 ] || [ "$code" -eq 137 ]; then
    echo "::error::$dir $name ran over ${CI_TEST_LIMIT:-600} s and was stopped (hung test?)"
  fi
  [ "$code" -eq 0 ] || status=1
  ran=$((ran + 1))
done 3< target/ci-tests.tsv
if [ "$ran" -eq 0 ]; then
  echo "no test binary matches $pattern" >&2
  exit 1
fi
exit $status
