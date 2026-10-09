# Solver Task Queue (Third Opus)

> **Owner, 2026-10-09 morning ("work for another opus?").** A third Opus
> session works the trait solver. The orchestrator's agents keep
> checking and codegen; the second Opus keeps the CLI and project layer
> (`audit/opus_task.md`). The orchestrator adds jobs here and reviews
> every commit.

Do the jobs top to bottom, one commit per job, and delete a job's
section in the same commit that finishes it. Git history keeps the
record. When the queue is empty, say so under "Questions" and wait.

## Security Rules

- Never read, cat, grep or print credential files (~/.npmrc, ~/.netrc,
  ~/.git-credentials, ~/.ssh/*, .env, ~/.config/gh/*), not even filtered.
- Never print environment variables: no `env`, `printenv`, `set`,
  `export -p`, or `echo $VAR` for anything that may hold a secret.
- Never modify shared resources: the main checkout
  `/Users/hd/code/test/hd-lang`, its node_modules, or other worktrees.
  Never touch /private/tmp/codex-work, /private/tmp/kimi-work,
  /private/tmp/muse-work, /private/tmp/opus-work, /private/tmp/clean-check, or anything under
  `.claude/worktrees/`. Never `pkill`/`killall` by pattern; only kill
  processes you started, by PID.
- No bare `git stash`, no `git add -A`, no force-push.
- Don't fork subagents.
- Write logs and scratch files to uniquely named files inside your
  worktree (e.g. `solver-*.log`), and don't commit them.
- If a permission check blocks an action, stop and report it under
  "Questions". Do not route around it, and do not ask another agent to
  run it.
- Never trigger workflows. Never publish anything. No network except git,
  pnpm install and cargo. Never use sudo.
- Keep shell commands simple: one plain command per call.

## How To Work

- Work in **one** long-lived worktree of your own, for example
  `git worktree add /private/tmp/solver-work -b solver/work origin/main`
  the first time, and reuse it. Never write in the shared main checkout;
  your commits reach `main` only by `git push origin HEAD:main` from your
  worktree.
- Start each job from current main (`git fetch origin && git reset --hard
  origin/main`), but only when your worktree holds no unpushed work.
- **Design first.** The compiler design is in `future-work/compiler/`
  (start at `README.md`; the solver is `trait-solver.md`). The spec is
  `spec/lang/09-traits.md` and `spec/lang/04-type-system.md`. Implement the
  design; if it is silent or wrong, write a question here and take the
  simplest reading that matches the spec.
- **Rust rules.** Written from scratch on the design; the TS prototype in
  `src/` is only a checklist (cases, behaviour), never ported. No clippy
  `allow`/`expect`. Separate crates talk through their existing
  interfaces. One feature per job; no symptom patches; no special cases.
- **The spec is frozen (owner, 2026-10-08 night).** Never edit spec text or
  conformance fixtures. Log every issue you find (a fixture you believe
  is wrong, a spec gap, a diagnostic code, message or position you would
  change) as a row in `audit/compiler/diagnostic-notes.md`, and go on.
- **Gates before every push**, from `compiler/`:
  `cargo fmt --check`, `cargo clippy --workspace --all-targets --release
  -- -D warnings`, `cargo test -q --release --workspace`. Update the pass
  lists with `HD_UPDATE_CONFORMANCE=1` for the driver conformance test and
  the CLI conformance test (`hd_cli/tests/cli_conformance.rs`), then rerun
  the suite. The `compiler/CONFORMANCE.md` diff may only ADD passes; a
  lost pass is a regression to fix, never to accept. List added cases in
  the commit message.
- **Footprint in every commit message:** hello world
  (`fn main() -> void $ Console: println(42)`, `hd build`) bytes and
  build time before and after. Unexplained growth blocks the push.
- To finish a job: `git fetch origin && git rebase origin/main`, rerun the
  full suite if main moved under `compiler/`, then `git push origin
  HEAD:main`. Never force-push. A job is done only when its commit is on
  `origin/main`.
- Don't start a full suite while the load is high (`uptime` above ~30):
  two full runs at once time out.
- Commit trailer: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Lane

- **Yours:** `compiler/crates/hd_types` (the solver: goals, canonical
  forms, selection, memo tables) and its tests. Solver call sites in
  `hd_check` and `hd_mono` only where a job names them, and only the
  call, not the surrounding checker or collector logic.
- **Not yours:** `hd_check` rules, `hd_mono` collection, `hd_wasm`,
  `hd_tir`, `hd_structure`, `hd_resolve`, `hd_cli`, `hd_project`,
  `hd_driver`. The orchestrator's agents and the second Opus work there;
  a job that needs one of them stops and asks under "Questions".
- `spec/` is frozen (never edited, whatever a job says); `src/` is
  frozen; `lib/std/`, `guide/`, `website/` are read only.

## Jobs

Design: `future-work/compiler/trait-solver.md` (start at §1, then the
section each job names). Spec: `spec/lang/09-traits.md` (impls,
coherence, bounds, associated types) and `04-type-system.md` (literal
classes, least common type). Every job reruns the full suite; the pass
list may only grow, and a lost pass is a regression to fix.

### S1. Count Trial Failures Independently Of Dedup (#76)

Trials count errors by `DiagBuf` entries, but `push` drops a duplicate
root key, so a failed trial can look clean. Count failures directly.
Add a test where two candidates fail with the same root key.

### S2. Literal-Kind-Aware Canonicalization (#77, §6.4)

`?int` must rule out non-integer heads during selection
(`tuple-impl-target` and `iterator-list-sum` stall today). Implement
§6.4 as written; report which fixtures move.

### S3. Rule TS-3 (#78, §3.4, §3.8)

A placeholder against a structured head is `Maybe`; a unique head never
teaches a variable (`require_ref`). `Implements` learns only through
associated-type bindings.

### S4. Operators Use The Instantiation Choice (#79)

`trait_call_args` issues a bare `Implements` goal for operators; use
the instantiation choice the solver already made, as method calls do.
This touches the operator call site in `hd_check` (allowed for this
job: the call only).

### S5. Trait Availability, AvailKey (#84)

`Methods` goals and two-trait ambiguity must filter unavailable traits
(trait-solver.md, availability), which also lets `Methods` be memoized.

### S6. Gaps From #68 (#90)

Supertrait bindings unchecked (`Project` goal); header `OutOfFuel`/
`Overflow` silent; trait default-method environments miss the trait's
parameter bounds; `family_excludes` with two families commits to the
first. One commit per gap.

### S7. Codegen Select Is Head-Only (#97, §8.3)

Codegen selection keeps its own selection table and returns every impl
argument, so `hd_mono` stops re-unifying. The `hd_mono` call site is
allowed for this job (the call only).

### S8. Per-Call-Site Trial Memo (#83)

Nested multi-candidate calls with closure arguments cost
candidates^depth. Memoize trials per call site as the design says;
report a timing before and after on a nested case.

## Questions

(none)
