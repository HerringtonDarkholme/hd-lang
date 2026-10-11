# Muse

Muse's work queue (owner, 2026-10-10: one task file, `audit/muse_task.md`;
the older `opus_task.md` and `kimi_task.md` are gone, and
git history keeps them). The orchestrating session adds jobs here. Do
them top to bottom, one commit per job, and delete a job's section in the
commit that finishes it. When the queue is empty, report that and wait.

## How To Work

- Owner direction, 2026-10-10: do not align native runtime behavior to
  Node.js. Node behavior is subject to change. Follow the spec and owner
  decisions; cross-engine comparisons are evidence, not semantic authority.
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
- **Gates before every push** (owner, 2026-10-11: dev profile only, never
  `--release`; the dev profile is opt-level 1; wasmtime is an off-by-default
  cargo feature and plain cargo commands skip it). From `compiler/`:
  `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`;
  `HD_UPDATE_CONFORMANCE=1 cargo test -q` ONCE (no second run; then
  `git diff compiler/CONFORMANCE.md` must only add passes); and
  `bash spec/check.sh` from the repository root. A job that changes
  `crates/hd_run_wasmtime` also runs `cargo clippy -p hd_run_wasmtime
  --all-targets -- -D warnings` and `cargo test -q -p hd_run_wasmtime`.
  Never `--workspace --all-features` locally: CI covers it. Never push with
  a gate red.
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

(Owner, 2026-10-11: Codex is offline; a weaker Muse is back temporarily.
Jobs are small and each names the files it may change. If a job turns out
bigger than described, stop, push nothing, and report what you found.)

### M39. Long-lived Node workers for driver conformance

Your M38 note ("Conformance on Node" in `future-work/compiler/wasmtime-perf.md`)
estimated about 3-4 ms per run with a long-lived Node worker, against
about 38 ms with a process per case. Build it:
- One Node process per conformance worker thread, started once. It
  loads the host and takes cases over stdin (wasm bytes, the profile, args
  and stdin text), one framed message per case. It returns stdout,
  stderr, the exit status and any panic report exactly as a fresh process
  would.
- Process-global state must not leak between cases: reset the host state
  per case; a case that hangs, crashes the worker or calls exit must kill
  and restart that worker and report the same verdict as today.
- Files: `compiler/crates/hd_driver/tests/conformance.rs` (the Node engine
  path only) and `compiler/host/*.mjs` (a new worker entry is fine; rebuild
  `hd` after editing, since it embeds the host files).
- Done when the driver conformance verdicts on Node are identical
  (`cargo test -q -p hd_driver --test conformance`, default engine),
  `cargo test -q -p hd_cli` passes, and the commit body has the suite wall
  time before and after.

### M40. Measure the same worker idea for `hd test` on Node

Find out whether `hd test` on the Node engine starts one `node` process per
test module, per case or per run (`compiler/crates/hd_cli/src/test_cmd.rs`,
`engine/`). Measure a test-heavy package (for example the std tests or
`spec/conformance` std test fixtures) and estimate the gain of a long-lived
worker. Report in `future-work/compiler/wasmtime-perf.md`. Implement it only
if the gain is over 2x on a realistic package and the change stays inside
the Node engine files; otherwise report only.
