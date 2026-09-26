# Baseline Corrections

Commit `bd985d7`, 2026-09-25.

- **Spec edits.** The baseline in `audit/PLAN.md` says the last two commits
  changed about 1,200 lines of specification text, mostly chapter 14. That
  is wrong. `git diff --stat 4cc1312 bd985d7 -- 'spec/[0-9][0-9]-*.md'`
  shows 4 files, +25/-10, all in `1e55fc9`. The large `spec/` line counts
  come from porting `spec/reference-parser/` and the check scripts from
  Python to TypeScript, and from fixture edits. Found by the 1.1 worker and
  confirmed by the coordinator.
