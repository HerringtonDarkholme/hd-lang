#!/bin/bash
# A18: a `type-mismatch` reject case whose actual error is hd's
# "a value never becomes a trait value reference by itself, so write `&`"
# hint, so the case fails for a missing `&`, not for the rule it names.
# Leads only: a few fixtures test exactly that rule (trait.dyn.ref...).
cd /Users/hd/code/test/hd-lang/spec/conformance
HD=/Users/hd/code/test/hd-lang/target/release/hd
awk -F'\t' '$2=="type" && $3=="reject:type-mismatch" && $1 !~ /fixture-package-tree/ {print $1"\t"$4}' cases.tsv |
while IFS=$'\t' read -r f spec; do
  grep -q '^# fixture-package' "$f" && continue
  out=$(HD_CACHE="$1" timeout 30 "$HD" check "$f" 2>&1 | grep -m1 '^error')
  case "$out" in *"never becomes a trait value reference by itself"*) printf '%s\tA18\t%s\n' "$f" "$spec";; esac
done
