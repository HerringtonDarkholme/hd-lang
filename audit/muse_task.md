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

### M19. Implement: Http And Net On wasmtime (W3, #245, Code)

After M18. Port `compiler/host/http.mjs` and `net.mjs` per §20.4 (client
threads, no async runtime): grant checks per request and per redirect,
the `Host` header (see commit abbd5395), timeouts, TCP/UDP handles.
Same files and rules as M18. Tests must use local servers only, never
the network.

### M20. Fix: Show A Prompt Before Reading Input (#277, Code)

Both engines hold stdout until stderr is written or the run ends, so a
prompt printed before `read_line!` does not show before the wait. Check
`spec/std/console.md` for the buffering rule first (the spec wins), then
flush held stdout before a ConsoleInput read starts, on both engines.
Files: `compiler/crates/hd_run_wasmtime/**`, `compiler/host/core.mjs`,
the two engines' tests. Add a test that pipes input and checks the
prompt order.

### M21. Implement: Resource Limits On wasmtime (#269, Code)

After M18-M20, implement your M15 design (§ of
`engines-and-test-runner.md`): `--time-limit`, `--max-heap` and the
other program limits on wasmtime, with the panics or errors the spec
names. Same files as M18, plus the flag parsing in
`compiler/crates/hd_cli/src/main.rs` (the `--engine` flag code is the
model). Node behavior stays as is until W5.

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

## Questions

(none open)
