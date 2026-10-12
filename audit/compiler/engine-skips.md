# Engine Skips: fixtures skipped, failing, or gated per engine (#60)

hd commit `e4d290ec`, 2026-10-12. Driver tier
(`compiler/crates/hd_driver/tests/conformance.rs`) and CLI tier
(`compiler/crates/hd_cli/tests/cli_conformance.rs`): every fixture whose
handling differs between wasmtime and the default engine (Node), plus the
engine-independent skips in those tiers. No code edits (read-only job).

Default engine is Node (`test_cmd.rs`: `EngineKind::Node`). There is no
expected-fail mechanism in either harness (`KNOWN_FAILURES.tsv` is empty);
"expected-to-fail" below means recorded fail buckets in
`compiler/CONFORMANCE.md`. No CLI fixture pins `--engine` in any
`expect.txt` (`rg --engine spec/conformance/cli/` is empty), so CLI
program/test steps follow the mode: default runs them on Node,
`wasmtime-only`/`wasmtime` forces wasmtime (`select_engine`,
`cli_conformance.rs:119-164`).

| Fixture | Tier | Engine | Reason (stated) | Still needed? |
| --- | --- | --- | --- | --- |
| `runtime/valid/cancellation-cleanup-order.hd`, `cancellation-lifo-across-three-frames.hd`, `cancellation-loop-and-branch-scopes.hd`, `cancellation-provider-scope-cleanup.hd`, `cancellation-runs-dynamic-method-defer.hd`, `cancellation-skips-unreached-defer.hd`, `cancellation-unwinds-nested-frames.hd` (all `# fixture-runtime-scenario: cancellation-cleanup`) | driver | both | Replacing scenario needs program exports no build emits (cancel of the root suspension, call of `cleanup_ran`, held/intercepted call, second driver) → `Unsupported("RunCase")` (`conformance.rs:1208-1216`). Recorded as the 7 of 11 `unsupported:RunCase`. | yes — until #56 (compiler hooks for runtime scenarios) lands |
| `runtime/valid/cancellation-runs-defer.hd`, `runtime/valid/cancellation-unwinds-suspending-closure.hd` (scenario `cancellation-cleanup` + profile `pending-gate`) | driver | both | Same `RunCase` short-circuit, reached before any engine runs. Latently also wasmtime-gated: `RuntimeProfile::from_name` rejects `pending-gate` (`profiles.rs:21-30`; `wasmtime.rs:25-26` errors, never falls back to Node). | yes — same as above; the profile gap stays latent behind it |
| `runtime/panic/competing-suspension-drivers.hd` (scenario `competing-drivers`), `runtime/panic/reentrant-suspension-poll.hd` (scenario `reentrant-poll`) | driver | both | Same `RunCase` short-circuit (the remaining 2 of the 11 `unsupported:RunCase`; the report lists exactly these 11 fixtures). | yes — until #56 |
| `runtime/valid/resource-disposed-result.hd` (profile `disposed-file`) | driver | both reach check first; wasmtime additionally ungated-profile | Fails at check on both engines with `fail:boundary-private-field` (recorded fail bucket), so no engine runs it. If check ever passes, wasmtime rejects the `disposed-file` profile (`no runtime profile named disposed-file`), an error with no Node fallback. | profile gap: yes (latent); the recorded fail is a separate correctness issue |
| `runtime/valid/test-case-options.hd`, `runtime/valid/it-each-options.hd` (`ignore="..."` cases) | driver | both | A test case with the `ignore` option does not run (README Runtime Execution); the harness filters `test.ignore.is_none()` (`conformance.rs:1030`). Both fixtures pass with their ignored cases skipped. | yes — by design; the fixtures demonstrate `ignore` |
| `cli/dep-add-clash-remote`, `dep-add-inferred-key`, `dep-add-latest`, `dep-add-no-manifest`, `dep-add-pre-release`, `dep-add-query-revisions`, `dep-add-query-versions`, `dep-add-several-remote`, `dep-add-subfolder`, `dep-add-untagged`, `dep-update-query-remote` (all with `remotes/`) | CLI | both (environment) | A case with local remotes (or a `.git` expectation) needs `git` on `PATH`, else `Unsupported("needs git on PATH")` (`cli_conformance.rs:966-974`). `git` is present here, so all run (0 unsupported in the report). | yes — by design; sandbox without `git` skips them |

Checked and clear (run on both engines, no skip, no diff):

- 13 `pending-first-poll` fixtures: entry runs under `console`, then again
  under `pending-write`; wasmtime implements `pending-write`
  (`profiles.rs:27`). All 13 pass-listed, none has a `tests:` block.
- 11 other profile fixtures (`serde-vault` ×7, `pending-write` ×2,
  `misbehaving-host`, `special-float-host`): wasmtime implements every
  profile they name.
- CLI tier: 0 unsupported on default; the 15 recorded fails are
  engine-independent (8 `hd doc`, `check`/`help` wording, and 5
  output-format mismatches on engine-routed commands: `test-outcomes`,
  `typeid-single-file`, `dbg-release`, `release-test-checked`,
  `release-wraps`).

## Evidence from runs (scratch `HD_CACHE` in the worktree)

Driver, `HD_CONFORMANCE_ONLY` subset, default engine (no feature):
5 pass, plus the recorded `fail:boundary-private-field`
(`runtime/valid/resource-disposed-result.hd`) and recorded
`unsupported:RunCase` (`runtime/valid/cancellation-runs-defer.hd`) —
both reproduce exactly.

Driver, same subset, `HD_CONFORMANCE_ENGINE=wasmtime-only`
(`--features wasmtime`): identical fail and unsupported lines;
0 Node entries; profiled/ignore controls pass
(`boundary-public-fields-cross`, `pending-first-poll-branches`,
`test-case-options`, `it-each-options`).

Driver, controls, `HD_CONFORMANCE_ENGINE=wasmtime` (compare, judged by
wasmtime): 4/4 pass, `0 output/status differences`, 3 exact entry
matches, 2/2 test outcome matches (timing excluded); wall Node 2.738s,
wasmtime 0.105s.

CLI, engine-routed subset
(`typeid-single-file,test-outcomes,json-run,wasm-run-built,exit-program-status,test-timeout`),
default mode: `Node programs 10, Node test commands 4`, 0 wasmtime;
4 pass, 2 recorded fails (`test-outcomes`: reporter wording;
`typeid-single-file`: `typeid_single_file.shop.User` vs `shop.User`).

CLI, `wasm-run-built,json-run,exit-program-status`, `wasmtime-only`:
3/3 pass, `Wasmtime programs 9, Node programs 0`.
