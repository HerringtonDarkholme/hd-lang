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
- **Gates before every push** (always the dev profile, never `--release`; owner, 2026-10-11) (from `compiler/`): `cargo fmt --check`;
  `cargo clippy --workspace --all-targets -- -D warnings`;
  `HD_UPDATE_CONFORMANCE=1 cargo test -q --workspace`, then the
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


### M24. Review: Your Own wasmtime Work, M18-M22a (Review, Then Fixes)

Do this after M22 and before M23. Review your own commits 3707b290,
43993c72, 91337f6f, 0f319ffc, 0613e39f, 3459768f and ec517898 (the
wasmtime host, `compiler/crates/hd_run_wasmtime`, and its CLI and
conformance hooks) against the feedback below, then fix what you agree
with. This is a review job, not new features.

Feedback from the orchestrator's review (2026-10-10), to weigh, not rules:
- **Size and shape.** ~16.5k lines in seven commits; the crate is 10.8k
  lines with `http.rs` 2.5k, `net.rs` 1.8k, `fs.rs` 1.7k, `core.rs` 1.3k;
  `host.rs::call` ~240 lines, `net.rs::handle_op` ~190. Single commits of
  4.8-6.5k lines are hard to review. Consider splitting providers by
  concern, one shared codec layer from `hd_host_abi`, and smaller commits.
- **Node parity versus the spec.** The host reproduces libuv/Node error
  spellings and messages (`fs.rs` `node_errno`, `node_windows_errno`, ~250
  lines) so output matches Node byte for byte. Once W5 removes Node those
  quirks stay. Where the spec fixes an error's shape, follow the spec;
  where it does not, decide what a Rust host should say and log it.
- **Grants.** M21 edited `hd_driver/src/lib.rs`, `check_cmd.rs` and
  `test_cmd.rs` beyond the job's files without asking first.
- **Global cost.** M21's tracking global allocator (`hd_cli/src/memory_cap.rs`)
  runs on every `hd` invocation; measured 0-8 ms on a 50 ms `hd check`.
  Consider enabling it only when a compiler memory limit is set.
- **Test robustness.** The UDP port-reuse race (fixed in M21b) suggests
  checking the other network and pipe tests for timing assumptions.
- **Owner's view** (`audit/compiler/later.md`, "Wasmtime host code
  quality"): the owner judged this code low quality and wants it
  revisited.
- **Replay readiness** (owner wants resume after the REPL,
  `future-work/compiler/live-execution.md` §5-6): keep every value that
  reaches the guest flowing through three points a recorder can wrap:
  import dispatch (`host.rs::call`), wake delivery (the reactor/driver
  loop), and cancellation/abort. No provider may hand the guest data any
  other way, and provider-internal clock reads that change an outcome
  (timeouts) must surface through those points.

Output: a short written review (agree / disagree with reason, per point)
appended to `engines-and-test-runner.md` §18, then the fixes you agree
with as separate, reviewable commits (each under ~1500 lines), with the
parity tests and full gates green after each. Same file grants as M22,
plus `compiler/crates/hd_cli/src/memory_cap.rs` and `main.rs` for the
allocator change; ask before touching anything else.

(Owner, 2026-10-11: Codex finishes M24 and then goes offline. M23, M16 and
M17 moved back to the orchestrator's task list; git history keeps their
text. Stop after M24.)
