#!/usr/bin/env bash
# The gate a lane passes before it is pushed; the selection lives in lane-gate.ts.
#
#   scripts/lane-gate.sh STEP [CASES]
#   scripts/lane-gate.sh --plan [A..B] [CASES]    print the selected steps, run none
#
# Runs what `git diff origin/main...HEAD` can break: the touched crates and their
# dependents' narrow test targets, plus the golden-TIR, server-SDK, metrics-fixture
# and conformance steps the diff reaches. A docs-only diff runs spec-sync only.
# Logs: target/lane-gate/STEP/. Rules and cost: the header of scripts/lane-gate.ts.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)" || exit 2
exec node scripts/lane-gate.ts "$@"
