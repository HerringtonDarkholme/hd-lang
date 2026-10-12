# Muse

Muse's work queue. Since 2026-10-11 (owner) the jobs are GitHub issues
labeled `muse` in HerringtonDarkholme/hd-lang; this file keeps only how to
work. List them with `gh issue list --label muse --state open`, read one with
`gh issue view N`, and do them lowest number first, one commit per job. End
the commit body with `Fixes #N` so the push closes the issue. If a job is
blocked or bigger than described, comment on the issue (`gh issue comment N`)
instead of pushing. When no `muse` issue is open, report that and wait.

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

Jobs are GitHub issues labeled `muse`. Priority labels P1..P4 order them
when several are open; otherwise take the lowest number. Old job numbers
(M##) are mapped in `audit/task-issue-map.md`.
