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

M2 below replaces the per-case Node process.

### M2. Run Programs In Long-Lived Node Servers, Not One Process Each

Each runtime case starts `node host/run.mjs` (`run_wasm` in
`hd_driver/tests/conformance.rs`, and the CLI tier's runs), and Node
startup costs about 30 ms, about 30 s of every conformance pass. Owner,
2026-10-10: stop spawning a process per program.

You may edit, for this job only: the two runner files named in M1, a new
`compiler/host/serve.mjs`, and `compiler/host/run.mjs` only to share code
with it (its command-line behaviour must not change; `hd run` and
`hd test` keep starting Node once per command).

1. `compiler/host/serve.mjs`: a long-lived process that reads one JSON
   request per line on stdin (Wasm path, profile or config path, program
   arguments, stdin text, timeout) and writes one JSON response per line
   on stdout (stdout text, stderr text, exit code, how it ended).
2. Run each program in a `worker_threads` Worker, not in the server's own
   context: a program's `process.exit`, timers, host client threads
   (`http.mjs`, `net.mjs`) and grants must not reach the next program.
   On timeout, `worker.terminate()` and answer; the server keeps running.
   A Worker may be reused only if its program ended cleanly; otherwise
   start a fresh one.
3. The runners start one server per worker thread at the start of the run
   (lazily, on the first runtime case) and send requests to it; if a
   server dies, restart it and rerun that case once.
4. Output must stay **identical**: `compiler/CONFORMANCE.md` regenerated
   after the change is byte-for-byte the same. Every runtime fixture's
   stdout, stderr and exit code must match what the per-process run gave.
5. Measure: wall time of one full driver pass and one CLI pass before and
   after (machine idle), in the commit message.

## Questions

(none open)
