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

### M8. Triage The Missing And Wrong Errors (Research)

282 invalid conformance fixtures fail: 130 with no diagnostic and 152
with a wrong code (`compiler/CONFORMANCE.md`, Failure Buckets; the gap
audit `audit/compiler/gap-audit-2026-10-10.md`, Tier 4). Group them by
root cause in the Rust compiler (read the checker, don't guess from the
fixture name), one group per feature-sized fix, each with its fixtures,
the crate and function where the check belongs, a size estimate, and
whether it matters for soundness. Name groups the task list already has
(#203-#214, #224, #232, #273-#275) so they can be merged into them.
Output: `audit/compiler/missing-errors-triage.md`. No code changes.

### M9. Design: The Test Runner On wasmtime (W4, #246, Design Text)

W1 is on main: `compiler/crates/hd_run_wasmtime`, as built in
`future-work/compiler/engines-and-test-runner.md` §18.7; your M3 host-port
design is in the same doc. Design `hd test` on wasmtime: TestRunner and
PropertyRunner providers, a `Linker` per host kind with `InstancePre`,
the pooling allocator, per-test timeouts through epochs, panic frames
(your M6 line table), and running the driver and CLI conformance runtime
tiers in-process instead of one `node` process per case. Price each
choice (startup per test, memory per instance, parallelism). Output: a
new section of `engines-and-test-runner.md`. No code changes.

### M10. Catalogue Diagnostics Without A Message (Research)

The Rust compiler prints some diagnostics with the code as the whole
message: `syntax-error: syntax-error`, `old-row-separator:
old-row-separator`. Find every code path that emits a diagnostic with no
message text (start in `compiler/crates/hd_syntax`), and for each, the
message the TS prototype prints in the same situation (`src/`, run
`pnpm run hd -- FILE` on a probe file; never `pnpm install`). Output a
table in `audit/compiler/diagnostic-messages.md`: code, emitting
function, situation, TS text, proposed Rust text. This feeds task #252.
No code changes.

### M11. Design: `hd doc` (#266, Design Text)

`hd doc` and `hd doc NAME` have no implementation. Read the spec's
Documentation section in `spec/cli/command-line.md` (pages, links, scope,
looking up one item, output) and whatever the TS prototype has. Design
the Rust implementation: which crate, what the checker must expose (doc
comments, signatures, visibility), page and link generation, the
`--private`, `--out` and `--open` flags, and the CLI cases it must pass.
Output: `future-work/compiler/doc.md`. No code changes.

### M12. Probe The Untested Rules Of Chapters 04-07 (Research)

The gap audit found 1594 spec rules with no fixture and no citation in
the compiler. For `spec/lang/04-type-system.md` through
`07-functions.md`, write a small probe program for each such rule that
describes observable behavior (skip pure definitions), run it on the
Rust `hd` and the TS prototype, and list every rule where Rust rejects a
valid program, accepts an invalid one, or prints different output. Keep
probes in your worktree, uncommitted. Output: a "Probed rules" section
appended to `audit/compiler/gap-audit-2026-10-10.md`. No code changes.

## Questions

(none open)
