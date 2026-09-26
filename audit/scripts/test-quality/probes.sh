#!/bin/bash
cd /Users/hd/code/test/hd-lang
L=audit/evidence/01-test-quality/probes.log
printf '# commit bd985d7; command: bash audit/scripts/test-quality/probes.sh (each "$ " line below was run from the repo root); date: %s\n' "$(date -u +%Y-%m-%d)" > $L
run() { printf '$ %s\n' "$*" >> $L; "$@" >> $L 2>&1; printf '[exit %s]\n\n' "$?" >> $L; }
show() { printf -- '--- %s:\n' "$1" >> $L; cat "$1" >> $L; printf '\n' >> $L; }
HD=(node --experimental-strip-types bin/hd.js)
STUB=(node --experimental-strip-types audit/scripts/test-quality/stub-hd.ts)
for f in audit/probes/oracle/refparser/*.hd; do show $f; run "${STUB[@]}" parse $f; run "${HD[@]}" parse $f; done
run "${STUB[@]}" parse test/fixtures/frontend/14-lexer-enforces-reserved-punctuation-escapes-and-numeric-separators-6.hd
show audit/probes/oracle/selfcontain/undeclared-key.hd
run "${HD[@]}" check audit/probes/oracle/selfcontain/undeclared-key.hd
run "${HD[@]}" run audit/probes/oracle/selfcontain/undeclared-key.hd
run "${HD[@]}" run test/fixtures/compiler-types/requirements/abi.hd
run "${HD[@]}" run test/fixtures/compiler/06-floating-power-uses-the-host-ieee-pow-primitive.hd
run "${HD[@]}" run test/fixtures/compiler/42-character-literals-carry-unicode-scalar-values-and-compare-in-scalar-ord.hd
run "${HD[@]}" run test/fixtures/compiler/05-integer-power-is-right-associative-checked-and-rejects-negative-exponent-negative.hd
show audit/probes/oracle/weak/runtime-error-wildcard.hd
run "${HD[@]}" check audit/probes/oracle/weak/runtime-error-wildcard.hd
run "${HD[@]}" run audit/probes/oracle/weak/runtime-error-wildcard.hd
run "${HD[@]}" test spec/conformance/runtime/panic/integer-add-overflow.hd
show audit/probes/oracle/weak/negative-zero-equal.hd
run "${HD[@]}" test audit/probes/oracle/weak/negative-zero-equal.hd
run "${HD[@]}" run test/fixtures/compiler/46-explicit-panic-lowers-to-unreachable-and-skips-pending-defer.hd
show audit/probes/oracle/fixtures/compiler/46-explicit-panic-lowers-to-unreachable-and-skips-pending-defer.pass.hd
run "${HD[@]}" run audit/probes/oracle/fixtures/compiler/46-explicit-panic-lowers-to-unreachable-and-skips-pending-defer.pass.hd
show audit/probes/oracle/conformance/runtime/panic/competing-suspension-drivers.pass.hd
run "${HD[@]}" test --scenario competing-drivers audit/probes/oracle/conformance/runtime/panic/competing-suspension-drivers.pass.hd
run "${HD[@]}" check spec/conformance/typing/invalid/nonhost-entry-requirement.hd
run "${HD[@]}" check spec/conformance/typing/invalid/discarded-result.hd
run "${HD[@]}" check --profile disposed-file spec/conformance/runtime/valid/resource-disposed-result.hd
