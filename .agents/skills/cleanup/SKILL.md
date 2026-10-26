---
name: cleanup
description: Run the routine repository cleanup for hd-lang (owner rule, about 11:00 and 23:00, and at progress summaries) - cargo clean between jobs, finished worktrees and branches (never parked ones), scratchpad, a text AND folder sweep of every doc against the last 12 hours of decisions, closed issues and commits, deletion of experiment and prototype folders, stale measurements, KNOWN_FAILURES rows, and an issue sweep that closes concluded issues with a log - in one commit with a one-line report. Use whenever a cleanup is due or requested.
---

# Cleanup

Cleanup removes what the last 12 hours made stale, and keeps everything
that is still live or parked. Do it yourself, with no subagents, in one
commit, and report it in one line.

## 0. Collect what changed

Before touching anything, list what changed, so the sweep has targets:

```sh
git log --since="12 hours ago" --format='%h %s' origin/main
gh issue list --state closed --search "closed:>=$(date -v-12H +%Y-%m-%dT%H:%M)" --json number,title
```

Also read the session's owner decisions, including features dropped or
removed, renamed APIs and anything parked.

## 1. Build directories

- `cargo clean` at the repo root, only when no agent job and no gate is
  building. If one is running, clean right after it reports and before
  the next one starts.
- Measure before and after (`du -sh target`) and report the space freed.

## 2. Worktrees and branches

- `git worktree list`. Remove only finished agent worktrees.
- Never remove:
  - a worktree a live agent uses;
  - Muse's `/private/tmp/muse-work`;
  - the standing checkouts (`hd-merge`, `hd-size`, `hd-ir-redesign`).
- Delete local branches whose work is merged, or whose issue is
  concluded.
- **Never delete `park/*` or `wip/*`.** Before deleting any branch, check
  that its commits are on main, or that the branch exists on origin
  (`git ls-remote --heads origin <name>`). Push a local-only parked
  branch to origin first.
- Remote branches already merged into main may be deleted.

## 3. Scratchpad

Delete your own stale files. Leave files a running agent is using: its
tools, logs and measurement directories.

## 4. Docs: text sweep

Check every doc against step 0's list. The docs are:
- root `*.md`;
- `docs/future-work`, `docs/compiler` and `docs/audit`;
- every crate's `DESIGN.md` and `README.md`;
- the bench READMEs;
- `.agents/skills`.

For each doc:
- **Stale references:** grep for every issue closed or parked today
  (`#N\b`), each removed feature, and each renamed API. A historical
  citation ("measured on #667") can stay. A status claim must match
  reality, for example "decided after stage 2", "waits on #N", "approved
  API", a status line, or an index entry.
- **Finished reports and resolved log rows:** delete them. Git history is
  the archive.
- **Overlap:** two docs on one topic become one, and the other's links are
  fixed.
- **Removed features:** delete material for anything the owner closed,
  dropped or removed, and cut its mentions elsewhere. Parked proposals
  stay.
- **Links:** every relative link in an index resolves.

## 5. Docs: folder sweep (do not skip)

A text grep never finds a whole stale directory. List them:

```sh
find docs/future-work -mindepth 1 -type d
```

Design work keeps only the doc plus, for stdlib, its signature-and-example
package in the open design PR. Delete every prototype, experiment, control
script or exploration folder that isn't part of an open design PR. Where a
kept doc cited one, replace the link with "git history at <last commit>,
<path>". A folder an open PR rewrites stays for that PR.

## 6. Measurements

For each measurement, check the date and commit it was taken at.
- `docs/compiler/goals.md` links the twice-daily metrics; refresh any
  pillar numbers written in the text.
- Delete other stale measurements, unless a kept proposal cites them as
  its source and says they predate later changes.

## 7. KNOWN_FAILURES

Drop rows whose issue is closed. Each remaining row cites an open issue.

## 8. Issue sweep

For every open issue, one of these is true. Fix those that aren't.
- **Running or queued:** it is in a lane or in Muse's queue (#58).
- **Blocked:** each blocker has its own ticket, linked from the issue.
  The blockers are queued as work.
- **Waiting on the owner:** labelled `owner-decision`, with the question
  stated.
- **Parked by the owner:** it names the condition that reopens it.
- **An umbrella or index.**

Anything else gets closed:
- a measured miss, a design dead end, or an owner decision not to do it.
  Close it "not planned", with a "Conclusion (don't re-ask)" log of the
  numbers and the reason;
- work with no plan and no blocker that belongs to a later goal. Fold it
  into its umbrella as a sub-item with its measured cost, then close the
  dedicated issue.

Never re-queue a closed or parked item without naming the new evidence.

## 9. Commit and report

- Run `bash spec/check.sh`, then make one commit:
  `Cleanup: <what changed>`, with the bullets in the body and the
  attribution trailer.
- Push.
- Report in one line: space freed, worktrees and branches removed, docs
  changed or deleted, folders deleted, KNOWN_FAILURES rows dropped,
  issues closed.
