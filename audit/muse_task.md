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

### M45. Fix your M44 review findings 1-4

`future-work/compiler/wasmtime-review.md` findings 1-4: the fixture release
runs after its assertions (core.rs), the budget assert races the read it
guards (http), `empty_wake_batch_does_not_require_an_exchange_buffer`
does not prove what its name says, and the malformed-reply-shape test
covers a narrow table. Fix each as the review suggests, so every test
proves what its name claims. Files: `crates/hd_run_wasmtime/src/**` and
`crates/hd_run_wasmtime/tests/**`. Done when `cargo test -q -p
hd_run_wasmtime` passes 10 times in a row under load and the review file
marks each finding "Fixed (M45)". Finding 5 (memory cap) is the
orchestrator's; leave it.

### M46. Split `core.rs` in the wasmtime host

`crates/hd_run_wasmtime/src/core.rs` is 1437 lines. Split it into a
`core/` module the way M36 split http.rs: one file per concern, code
moved verbatim, visibility tightened, no file over about 600 lines. Tests
and the wasmtime-only conformance verdicts unchanged. Files:
`crates/hd_run_wasmtime/src/core.rs` and `crates/hd_run_wasmtime/src/core/**`.

### M47. Split `host.rs` and `test_engine.rs`

The same for `crates/hd_run_wasmtime/src/host.rs` (1121 lines) and
`test_engine.rs` (1066 lines): modules with files of at most about 600
lines, code moved verbatim, tests and verdicts unchanged. One commit per
file.
