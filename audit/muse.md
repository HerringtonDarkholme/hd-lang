# Muse

Muse's work queue (owner, 2026-10-10: one task file, `audit/muse.md`;
the older `muse_task.md`, `opus_task.md` and `kimi_task.md` are gone, and
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

### M1. Run The Conformance Suites In Parallel

The Rust conformance runner checks, builds and runs its 2845 cases one at
a time on one core (about 100 s per pass), while the TS runner
(`spec/tools/run-conformance.ts`) runs up to 8 at once. Every merge runs
the suite twice, so this is the slowest part of each gate.

You may edit, for this job only:
`compiler/crates/hd_driver/tests/conformance.rs` and
`compiler/crates/hd_cli/tests/cli_conformance.rs` (plus a small helper
module next to them if you need one).

1. Run the cases on a pool of worker threads, sized like the TS runner:
   `HD_TEST_JOBS` if set, else `min(8, available_parallelism())`. Use
   `std::thread::scope` (no new dependency unless the workspace already
   has one that fits). The cases are independent; the shared
   `MemoryStore` cache must stay correct under concurrent use (check its
   locking; if it is not thread-safe, give each worker its own store and
   say so).
2. Keep the output **identical**: `compiler/CONFORMANCE.md` regenerated
   after your change must be byte-for-byte the same as before it (same
   pass list, buckets and order). Sort results by case before writing.
   Keep `HD_CONFORMANCE_ONLY` working.
3. Each runtime case writes `conformance-{serial}.wasm` and `.json` under
   `CARGO_TARGET_TMPDIR`; keep the names unique per case so workers never
   share a file.
4. Do the same for the CLI tier if it is also sequential.
5. Measure: wall time of one full driver pass and one CLI pass before and
   after, on an otherwise idle machine (`uptime` low), and put both in the
   commit message.

## Questions

(none open)
