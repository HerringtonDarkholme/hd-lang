# Audit Agent Brief

Shared rules for every audit worker. Read `audit/PLAN.md` "Ground Rules" and
your assigned section before starting.

- Repository: `/Users/hd/code/test/hd-lang`, audited commit `bd985d7`.
- Write only inside `audit/`, and only in the directories assigned to you.
  Never modify `src/`, `spec/`, `test/`, `guide/`, root files, or git state
  (no commit, stash, checkout, reset, worktree). Ignore the stray root file `:w`.
- Execution is the evidence. Run things. A conclusion drawn only from reading
  source is labeled `UNVERIFIED`.
- CLI: `node --experimental-strip-types bin/hd.js <command> [options] FILE`.
  Commands: parse, check, test, run, build --wat, dump-hir,
  explain-requirements, trace, record, replay. See `src/cli.ts` for options
  (except the blind-fixture author, who must not read `src/`).
- Scripts: TypeScript run with `node --experimental-strip-types`, placed in
  your assigned directory. They must pass `npx oxlint <path>`. `tsc` does not
  cover `audit/`.
- Other workers run at the same time, so the CPU is contended and timings
  are noisy. Report ratios, and rerun timing-sensitive steps.
- Findings: one file per verified issue, `audit/findings/F-NNN-short-slug.md`,
  using the shape in `audit/PLAN.md`, numbered only within your range.
  Verify by running before filing. Severity: blocker, major, minor, note.
- Design choices are phrased as open questions for `OPEN_ISSUES.md`, never as
  defaults to implement.
- Evidence files begin with a header line: commit, command, date.
- Deliverable: `audit/evidence/<your-dir>/SUMMARY.md` with:
  - commands run;
  - result tables and headline numbers;
  - finding IDs with titles;
  - phase-7 specification feedback candidates, phrased as questions;
  - anything not done, and why.
- Final reply: at most 300 words. Headline numbers, then each finding ID
  with its severity and a one-line title.
