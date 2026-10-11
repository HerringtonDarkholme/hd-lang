# Muse

Muse's work queue (owner, 2026-10-10: one task file, `audit/muse_task.md`;
the older `opus_task.md` and `kimi_task.md` are gone, and
git history keeps them). The orchestrating session adds jobs here. Do
them top to bottom, one commit per job, and delete a job's section in the
commit that finishes it. When the queue is empty, report that and wait.

## How To Work

- Work in **one** long-lived git worktree of your own, for example
  `git worktree add /private/tmp/muse-work -b muse/work origin/main`
  the first time, and reuse it. Never write in the shared main checkout
  `/Users/hd/code/test/hd-lang`.
- Start each job from current main: `git fetch origin && git reset --hard
  origin/main`, but only when the worktree holds no unpushed work.
- Use a scratch `HD_CACHE` directory inside your worktree for every `hd`
  run and every cargo test run. Never run two full test runs at once, and
  don't start one while another agent's full run is going (check `uptime`;
  above ~30, wait).
- **Gates before every push** (from `compiler/`): `cargo fmt --check`;
  `cargo clippy --workspace --all-targets --release -- -D warnings`;
  `HD_UPDATE_CONFORMANCE=1 cargo test -q --release --workspace`, then the
  same without the env var; and `bash spec/check.sh` from the repository
  root. Never push with any of them red.
- Push with `git fetch origin && git rebase origin/main`, rerun the gates
  if main moved, then `git push origin HEAD:main`. Never force-push. Add
  files by name, never `git add -A`. A job is done only when its commit
  is on `origin/main`.
- One-line commit subjects under 72 characters, a blank line, then the
  body.
- Never read or print credential files or secret environment variables.

## Don't Touch

- `spec/`: **frozen**. No edits to rule text, std chapters, conformance
  fixtures or indexes. Log any spec or fixture issue as a row in
  `audit/compiler/diagnostic-notes.md` and go on.
- `compiler/crates/`: the orchestrator's lane, **except the files a job
  names**.
- `src/` (the frozen TS prototype), `lib/std/`, `website/`, `guide/`:
  read only.

## Jobs

### M22. Implement: `hd test` On wasmtime (W4, #246, Code)

After M21. Implement your M9 design (`engines-and-test-runner.md` §19,
"Test Runner"): TestRunner and PropertyRunner providers on wasmtime, a
`Linker` per host kind with `InstancePre`, per-test timeouts through
epochs, and `hd test --engine wasmtime` (Node stays the default). Then
run the driver and CLI conformance runtime tiers in-process on wasmtime
behind an env switch, and report the match count and the time per tier
against Node. Files: `compiler/crates/hd_run_wasmtime/**`,
`compiler/crates/hd_cli/src/engine.rs`, `compiler/crates/hd_cli/src/test_cmd.rs`
(engine selection only), the two conformance runners
(`hd_driver/tests/conformance.rs`, `hd_cli/tests/cli_conformance.rs`, the
engine switch only), their tests, and §18-§20 of the doc. Ask before
touching any other file.

### M23. Implement: wasmtime As The Default (W5, #247, Code)

After M22, when every runtime and CLI conformance case matches Node on
wasmtime (or each difference is a logged Node bug): make wasmtime the
default engine for `hd FILE`, `hd run`, `hd test` and prebuilt `.wasm`;
keep `--engine node` working for one release; report what still needs
Node (the website/TS side is out of scope). Same files as M22 plus
`compiler/crates/hd_cli/src/main.rs` (engine default only).

### M16. Design: `hd fix` And `hd cache gc` (#267, #268, Design Text)

Read the spec's Rewriting Source / Fixing and Compiled Cache sections,
the fix-its the Rust checker already attaches (`hd_cli/src/report.rs`
`Fix`), and the compiled cache in `hd_cache`. Design both commands: which
fix-its are safe, how edits are applied without conflicts, the cache
size cap and eviction order, and the CLI cases each must pass. Output:
`future-work/compiler/fix-and-cache-gc.md`. No code changes.

### M17. Triage The M12 Probe Divergences Into Tasks (Research)

Your M12 section of `audit/compiler/gap-audit-2026-10-10.md` lists
divergences for chapters 04-07. Group every (a) and (b) item into
feature-sized fixes with the crate and function where each belongs and a
size estimate, and name the existing task each joins (#203-#214,
#252-#279) or say it needs a new one. Then probe chapters 08-11 the same
way and append their section. Output: appended to the gap audit. No
code changes.
