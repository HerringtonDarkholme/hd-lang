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

### M29. Cache compiled modules in the wasmtime engine

Your M28 note (`future-work/compiler/wasmtime-perf.md`, "Conformance on
wasmtime") found that `Engine::load` (`crates/hd_run_wasmtime/src/lib.rs`)
ignores its `CacheStore` argument and calls `wasmtime::Module::new` on
every call, and that `TestEngine::compile` (`test_engine.rs`) has no
cache. So identical bytes are compiled again (pending-first-poll fixtures
twice; test-harness cases twice, in two forms).
- Add an in-process cache keyed by the wasm bytes' hash (plus anything else
  that changes the compiled code, such as engine config), shared by both
  paths, so identical bytes compile once per process.
- If `CacheStore` is meant for compiled artifacts across runs, look at how
  it is used elsewhere. Only use it when serialized modules are safe and
  simple (wasmtime `Module::serialize`/`deserialize` is `unsafe` to
  deserialize, and `unsafe` is banned except one approved site), so the
  in-process cache is the expected scope.
- Files: `crates/hd_run_wasmtime/src/**` only.
- Done when the wasmtime-only driver conformance suite gives the same
  verdicts, a unit test shows one compilation for repeated identical
  bytes, and the commit body has before/after numbers for the suite wall
  and the summed wasmtime execution time.

### M30. Find the flaky `run::fetch_with_path_requirements_only_has_nothing_to_do`

`compiler/crates/hd_cli/tests/all/run.rs`: this test failed once with
`Invalid argument (os error 22)` and passed alone and on rerun. That was
right after tests moved to `hd_testkit::Scratch` scratch directories and
the offline git config moved to `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_n`
environment variables (commit 5874b720).
- Run it 50 times in a loop, and also under load (`cargo test -q -p
  hd_cli` running at the same time), and find which call returns EINVAL.
  Suspects: a scratch path reused across parallel tests, a removed
  directory used as cwd, or a git child getting a bad environment.
- Fix the cause in test code.
- Files: `compiler/crates/hd_cli/tests/all/run.rs` and
  `compiler/crates/hd_testkit/src/lib.rs` only.
- Done when 50 runs in a row pass under load. Put the cause in the commit
  body.
