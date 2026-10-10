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

### M2. wasmtime Feature Audit And Spike (Research + Bench)

The owner committed to wasmtime (2026-10-10): the CLI must not need Node.
Before the port starts:
1. List every Wasm feature and proposal our emitter uses (read
   `compiler/crates/hd_wasm/src/`: GC types, typed function references,
   `br_on_cast`, tail calls, exceptions, `array.copy`, i31, multi-value,
   anything else), with the code location of each.
2. Pick the newest wasmtime release that supports all of them (check its
   docs/release notes) and the `Config` flags each needs; note anything
   unsupported and how our emitter would avoid it.
3. Spike under `compiler/bench/wasmtime/` (a new directory; no
   `compiler/crates/` edits): build the six programs in
   `compiler/bench/runtime/progs/` and the samples with the current `hd`,
   run them with the wasmtime CLI (GC flags on) where they have no host
   imports, and time wasmtime vs Node (startup and steady state).
Write `audit/compiler/wasmtime-audit-<date>.md`. Timebox 60 minutes.

### M3. Design: Porting The Node Host To Rust (Design Text)

The host lives in `compiler/host/*.mjs` (core, float, http, net, test,
profiles, run) and will move behind `compiler/crates/hd_run_wasmtime`.
Add a section to `future-work/compiler/engines-and-test-runner.md`: for
each provider and helper, the Node API it uses and its Rust replacement
(std, or a named crate with version, size and why), how the exchange
buffer and `hd_host_abi` codecs are reused, how suspension
(`block_on`, wakes, timers) maps onto wasmtime (sync host calls vs async
support, epoch interruption for timeouts), and how Http/Net client
threads and grants carry over. Include a dependency budget (crates and
binary size). Questions with a recommendation. Timebox 60 minutes.

### M4. Re-Triage The Failure Buckets (Research)

The buckets in `compiler/CONFORMANCE.md` moved a lot (2420 pass, about
336 fail, 84 unsupported). For every bucket with 3 or more cases, group
the cases by first cause (one minimal program each, run with the current
`hd`) and propose a task row per group ("[impl]" or "[check]", N
programs, rule IDs, stage). Write `audit/compiler/triage-<date>.md`.
Read only; timebox 60 minutes.

### M5. Design: The Release Profile In Codegen (#200, Design Text)

`--release` must change codegen per the spec (wrapping or checked
arithmetic as the spec says, `dbg` in release and in dependencies is an
error, the name section, code cache keys per profile). Read the profile
rules in spec/cli/command-line.md and spec/lang, and codegen.md / cache.md,
then add a "Release profile" section to codegen.md: what differs, where
the profile enters `Goal`, and how cache keys separate the profiles.
Questions with a recommendation. Timebox 45 minutes.

### M6. Design: Panic Frames In Test Reports (#201, Design Text)

`cli.test.report.frame` wants a panic's source frame in `hd test`
reports. Read that rule and wasm-layout.md / codegen.md, then add a
section on the line table codegen should emit (what maps to what, where
it is stored, how the runner reads it, cost when unused). Questions with
a recommendation. Timebox 45 minutes.

### M7. Spec Audit Prep (Research, Read Only)

`audit/compiler/diagnostic-notes.md` holds the spec questions logged
while the spec is frozen (about 35 rows: gaps, contradictory fixtures,
missing codes, the 60d16179 change). For each row write one entry in
`audit/compiler/spec-audit-proposal.md`: the rules and fixtures involved,
the options, and a recommendation with its reason, grouped by chapter.
Do not edit `spec/`. Timebox 60 minutes.

## Questions

(none open)
