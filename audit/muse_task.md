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

### M33. Trim the hd binary's wasmtime features (#278)

The `hd` binary grew from 7.9 MB to 19.1 MB with wasmtime. List the
wasmtime cargo features `hd_run_wasmtime` enables
(`crates/hd_run_wasmtime/Cargo.toml`). Turn off every one the code does
not need (for example `cache`, `wat`, `parallel-compilation` if unused,
`addr2line`, `coredump`, `profiling`, unused component-model parts), and
keep everything the tests use. Done when `cargo test -q -p
hd_run_wasmtime` and the wasmtime-only driver conformance verdicts are
unchanged, and the commit body gives the release `hd` binary size
(`cargo build --release --features wasmtime --bin hd`, benchmark-style
build, allowed for this measurement) and the cold build CPU time, before
and after. Files: `crates/hd_run_wasmtime/Cargo.toml` and any `cfg` lines
in `crates/hd_run_wasmtime/src/**` that the features require.

### M34. Find what holds 3.5 GB during the wasmtime conformance suite

M31 measured a peak RSS of about 3.5 GiB for the wasmtime-only driver
conformance run, mostly engine code memory. With 8 workers, that is
about 440 MB each. Measure where it goes: per-engine reservations
(pooling allocator slots, linear memory reservations, stack sizes), the
number of engines alive at once, and whether stores or instances outlive
their case. Report the top causes with suggested fixes and expected
savings in `future-work/compiler/wasmtime-perf.md`. Change code only for
an obvious leak (an engine, store or instance kept after its case
finishes), with a unit test. Files: `crates/hd_run_wasmtime/src/**`,
`future-work/compiler/wasmtime-perf.md`.

### M35. Split `createHost` in the Node host (#227)

`compiler/host/core.mjs` `createHost` is one very large function. Split it
into one function per host provider (console, fs, time, random, sys,
process, and so on) plus a small assembler, with no behaviour change.
Done when the driver conformance verdicts on Node are identical
(`cargo test -q -p hd_driver --test conformance`, default engine) and
`cargo test -q -p hd_cli` passes. Rebuild `hd` after editing, since it
embeds the host files. Files: `compiler/host/*.mjs` only.

### M36. Split `http.rs` in the wasmtime host

`crates/hd_run_wasmtime/src/http.rs` is 2610 lines (owner, 2026-10-10:
the wasmtime host code is low quality; see `audit/compiler/later.md`).
Split it into a `http/` module with one file per concern (for example
request building, the client and timeouts, the server side, the
host-call glue, and tests), with no behaviour change: move code and
tighten visibility (`pub(crate)` or private), but do not rewrite logic.
Done when `cargo test -q -p hd_run_wasmtime` and the http parity tests
pass, the wasmtime-only conformance verdicts are unchanged, and no file
is over about 600 lines. Files: `crates/hd_run_wasmtime/src/http.rs` and
the new `crates/hd_run_wasmtime/src/http/**`.

### M37. Profile the closures benchmark on wasmtime (#278)

`test/perf/micro/closures.hd` takes about 3.6x as long on wasmtime as on V8
(1337 ms against 374 ms; not GC). After M32's opt-level numbers, profile
it: does the time go to `call_indirect` or funcref table checks,
signature checks, closure environment loads, or host calls? Compare with
the same wasm under Node (`--cpu-prof`). Report the cause and one or two
concrete fixes, either in our wasm codegen (list them for the
orchestrator; do not change the compiler) or in the engine config, as a
section of `future-work/compiler/wasmtime-perf.md`. No code changes unless
it is an engine config setting with a measured win and unchanged
conformance verdicts.

### M38. Measure Node's per-run cost in conformance

M28 found each Node run costs about 101 ms, dominated by starting a
`node` process per case. Measure: process start with an empty script,
`compiler/host/run.mjs` load time, wasm compile, and the run itself, for a
typical fixture. Then evaluate one fix: a long-lived Node worker that
the driver conformance runner sends cases to over stdin (one process per
worker thread). Estimate the gain from the measurements. Write it up as a
section "Conformance on Node" in `future-work/compiler/wasmtime-perf.md`.
No code changes in this job.
