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
