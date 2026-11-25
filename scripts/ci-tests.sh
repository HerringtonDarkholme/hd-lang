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
system=$(uname -s)
memory_group=
if [ "$system" = Linux ] && [ -r /proc/self/cgroup ]; then
  while IFS=: read -r hierarchy controllers path; do
    if [ "$hierarchy" = 0 ] && [ -z "$controllers" ]; then
      memory_group="/sys/fs/cgroup$path"
      break
    fi
  done < /proc/self/cgroup
fi

# cost: fixed kernel/cgroup files before and after the one suspect binary;
# no process polling or scan of past runs. memory.peak is cumulative, while
# the before/after oom_kill counters identify kills during this invocation.
memory_snapshot() {
  printf 'cgroup: %s\n' "${memory_group:-unavailable}"
  for metric in memory.current memory.peak memory.max memory.events memory.oom.group; do
    if [ -n "$memory_group" ] && [ -r "$memory_group/$metric" ]; then
      printf '%s:\n' "$metric"
      cat "$memory_group/$metric"
    fi
  done
  if [ -r /proc/meminfo ]; then
    while read -r key value unit; do
      case "$key" in
        MemTotal:|MemAvailable:|SwapTotal:|SwapFree:) printf '%s %s %s\n' "$key" "$value" "$unit" ;;
      esac
    done < /proc/meminfo
  fi
}

while IFS=$'\t' read -r -u 3 dir name exe; do
  if printf '%s/%s\n' "$dir" "$name" | grep -qE "$pattern"; then
    [ -n "$invert" ] && continue
  else
    [ -z "$invert" ] && continue
  fi
  echo "     Running $dir $name ($exe)"
  TIMEFORMAT="     Took %R s wall, %U s user, %S s system: $dir $name"
  resources=
  measured=(timeout --kill-after=10 "${CI_TEST_LIMIT:-600}" "$root/$exe" "$@")
  if [ "$dir/$name" = crates/hd_command/all ] && [ "$system" = Linux ]; then
    report_dir="$root/target/ci-resources/hd_command-all"
    if mkdir -p "$report_dir" && : > "$report_dir/time.txt"; then
      resources=$report_dir
      memory_snapshot > "$resources/memory-before.txt"
      resource_since=$(date -u '+%Y-%m-%d %H:%M:%S')
    else
      echo "     CLI resource diagnostics are unavailable" >&2
    fi
    if [ -n "$resources" ] && [ -x /usr/bin/time ]; then
      # GNU time stays outside timeout's process group, preserving its RSS
      # report when the test or its group is killed. Its status is retained.
      measured=(/usr/bin/time -v -o "$resources/time.txt" "${measured[@]}")
    fi
  fi
  # A hung binary is stopped after ${CI_TEST_LIMIT:-600} s and named, so the
  # job ends with a log instead of hitting its own time limit (run
  # 45277379767 hung for 25 min and kept no log).
  started=$SECONDS
  time (cd "$root/$dir" && "${measured[@]}")
  code=$?
  elapsed=$((SECONDS - started))
  if [ -n "$resources" ]; then
    memory_snapshot > "$resources/memory-after.txt"
    for report in "$resources/memory-before.txt" "$resources/memory-after.txt" "$resources/time.txt"; do
      printf '     Resource report: %s\n' "${report##*/}"
      cat "$report"
    done
    if [ "$code" -ne 0 ]; then
      # A denied kernel-log read is recorded too; it must not hide the test
      # status or stop later binaries. The cgroup counters need no privilege.
      TZ=UTC timeout 5 dmesg --ctime --since "$resource_since" > "$resources/kernel.txt" 2>&1 || true
      cat "$resources/kernel.txt"
    fi
  fi
  # 124 is timeout's own exit; 137 is SIGKILL, from timeout's kill-after only
  # once the limit has passed, otherwise from outside, such as the runner's
  # out-of-memory killer (run 45372886461 was killed after 19 s).
  if [ "$code" -eq 124 ] || { [ "$code" -eq 137 ] && [ "$elapsed" -ge "${CI_TEST_LIMIT:-600}" ]; }; then
    echo "::error::$dir $name ran over ${CI_TEST_LIMIT:-600} s and was stopped (hung test?)"
  elif [ "$code" -gt 128 ]; then
    echo "::error::$dir $name was killed by signal $((code - 128)) after $elapsed s (out of memory?)"
  fi
  [ "$code" -eq 0 ] || status=1
  ran=$((ran + 1))
done 3< target/ci-tests.tsv
if [ "$ran" -eq 0 ]; then
  echo "no test binary matches $pattern" >&2
  exit 1
fi
exit $status
