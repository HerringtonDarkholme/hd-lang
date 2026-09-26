# Phase 4: Runtime Behavior

Commit `bd985d7`, Node v24.19.0, 2026-09-25. The worker could not write
report files; the coordinator wrote this file from the worker's final report.

Scripts are in `audit/scripts/runtime/` (`roundtrip`, `edits`,
`host-values`, `provider-config`, `panics`, `poisoned-instance`,
`diagnostics`) and pass `npx oxlint`. Probes and replay sidecars are in
`audit/probes/runtime/`.

## Headline

- **Round trip:** 82 suspension files.
  - 53 give identical exit status and stdout under run, record, and replay;
    3 of them recorded zero events, so replay proved nothing for those.
  - 13 are static rejects.
  - 16 cannot be recorded.
- **Edits:** unrelated declarations and reordering replay correctly, and
  changed suspending bodies are rejected. **A changed non-suspending helper
  is accepted and prints 63 instead of 42** (F-401). Formatting identity is
  unstable (F-611).
- **Provider configuration:** the Node API rejects `prod-v1` against
  `prod-v2`. The command line hardcodes `cli-default`, so a `ready-gate`
  history replays under `pending-gate` (F-402).
- **Host values:** NaN, -NaN, -0, ±infinity, a subnormal, `f64` max, and all
  multi-byte strings round-trip bit for bit, except a leading U+FEFF (F-400).
- **Panics:** 11 of the 15 spec categories are produced exactly. No panic
  output carries a source location or the `panic()` message. After a panic,
  a Node API instance keeps running with partial state.
- **Diagnostics sample (33 fixtures):**
  - 28 of 33 have the expected code;
  - 23 of 33 point at the exact token, and the rest at a nearby token on the
    right line;
  - 24 messages are actionable, 6 partly, and 3 not.

## Commands and Evidence

| Command                                                                 | Evidence                                   |
| ----------------------------------------------------------------------- | ------------------------------------------ |
| `roundtrip.ts suspension-fixtures.txt`                                   | `roundtrip.tsv`                            |
| `edits.ts`                                                              | `edits.tsv`                                |
| `host-values.ts`                                                        | `host-values.tsv`, `bom-host-boundary.log` |
| `provider-config.ts`; CLI record and replay of `probes/runtime/config/gate.hd` | `provider-config.log`               |
| `hd replay` of a truncated history                                      | `truncated-history.log`                    |
| `panics.ts`                                                             | `panics.tsv`                               |
| `poisoned-instance.ts`; `hd test probes/runtime/instances/*.hd`         | `instances-and-panic-output.log`           |
| `diagnostics.ts` (every fifth distinct code, 33 fixtures)               | `diagnostics-sample.log`, `diagnostics-probes.log` |

## 4.1 Record and Replay

Of the 82 files, 16 cannot be recorded:

- 8 contain only test blocks, and `record` runs only `main`;
- 6 panic and write no sidecar;
- 2 use `pending-gate` and spin forever (F-555); one record ran out of
  memory.

Scenario fixtures cannot be recorded with `--scenario`. Only 4 round trips
include provider-poll events.

Edit experiments (`edits.tsv`):

| Variant                            | Expected | Observed                    |
| ---------------------------------- | -------- | --------------------------- |
| v01, v02, v16 (unrelated edits)    | accept   | accept                      |
| v03, v05, v12, v13 (changed bodies) | reject  | reject                      |
| v04 (non-suspending helper changed) | reject  | **accept, prints 63** (F-401) |
| v06, v11, v15 (formatting)          | decide  | reject                      |
| v07, v08, v09, v10, v14 (formatting) | decide | accept                      |

- The code identity hashes raw declaration text (F-611). A space or comment
  inside a body changes it; a trailing comment, blank lines, or a doc comment
  do not.
- Rejection messages are uncaught stack traces that name only the new hash.
- Host values: NaN (`7ff8...`), -NaN, -0 (`8000...`), ±infinity, a
  subnormal, `f64` max, CJK, combining marks, NUL, astral characters, a
  skin-tone modifier, and the empty string all match exactly. A leading
  U+FEFF is lost (F-400).
- Spec status: chapter 11 has no normative replay rules.
  `future-work/RUNTIME_AND_LIBRARY.md` expects things the MVP lacks: an
  entry record, live continuation after the history ends, idempotency keys,
  and compiler-generated site identities. The MVP builds site identities
  from the function name plus a poll count or byte offset.

## 4.2 Panics

Produced exactly:

- `explicit-panic`, `assertion-failed`, `integer-overflow`;
- `integer-division-by-zero` (including `%`);
- `invalid-shift` (including a negative amount);
- `index-out-of-bounds` (including `[-1]`);
- `iterator-invalidated`;
- all four `suspension-*` categories.

Not produced:

- `stack-exhausted` surfaces as a JavaScript `RangeError` (F-161);
- `failed-checked-cast` has no spec construct that raises it;
- the two `annotation-*` categories wait on annotations, which are not
  implemented;
- a negative exponent gives `RuntimeError: unreachable` (F-153, F-406).

Other observations:

- Output is always `CODE: runtime panic`, with no location and no message
  (F-155).
- After a panic, a Node API instance keeps running: `bump()` returns 102. A
  fresh instance returns 1.
- `hd test` shares one instance across `main` and all test blocks (F-403).

## 4.3 Diagnostics Quality

- Sample of 33: 28 have the expected code; the 5 misses are unimplemented
  features. 23 point at the exact token and 10 at a nearby token on the
  right line. None is on the wrong line.
- Nearby-token cases include ambiguous-method, duplicate bound,
  invalid-binary-operands, mutable-field-modifier, tuple index, variant
  pattern field, and for-binding arity.
- Unhelpful messages:
  - a parameter decorator gives "expected a parameter name";
  - `annotate` gives a generic expected-expression;
  - the decorator message is misleading (F-405).
- Multi-error probes: errors in separate functions are all reported (m1,
  m2), but only one per function body (m3, m4, m5, m7; F-605). m6 points at
  `return`, not at its value.

## Findings

| ID    | Severity | Title                                                                   |
| ----- | -------- | ----------------------------------------------------------------------- |
| F-400 | major    | a leading U+FEFF is lost at the host boundary, in replay decoding, and in `lower()` (likely `TextDecoder` BOM stripping) |
| F-401 | minor    | replay accepts a changed body in an executed non-suspending function    |
| F-402 | minor    | the CLI provider-configuration identity is constant, so a profile change is not detected |
| F-403 | major    | `hd test` runs `main` and every test block in one shared instance, against the one-instance-per-test rule in chapter 02; state leaks, and 4 fixtures depend on it |
| F-404 | minor    | a panicking or never-finishing run leaves no history; a short history ends in an uncaught stack trace |
| F-405 | minor    | every decorator gets "only valid on top-level declarations", even at top level |
| F-406 | note     | four panic categories have no producer, and a negative exponent has no category |

Confirmed from other phases: F-153, F-155, F-161, F-204, F-555, F-556,
F-605, F-611.

## Phase-7 Questions

- Should replay code identity be transitive over reachable functions?
- Should identity be computed from a normalized AST, so formatting edits keep
  it stable?
- When the history runs out, should replay stop, or continue live and append?
- Should a panicking run still produce a replayable history?
- Is the command-line profile part of the provider-configuration identity?
- Which construct raises `failed-checked-cast`, or should the category be
  removed? Which category does a negative exponent use?
- Should the runtime enforce instance poisoning, or leave it to the host?

## Not Done

- Scenario replay through the Node API.
- Recording test-block-only fixtures.
- Non-canonical NaN payloads (the JavaScript number boundary canonicalizes
  them).
- Timings (CPU contended).
